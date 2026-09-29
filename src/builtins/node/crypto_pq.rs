//! crypto pq 域（ml-kem/ml-dsa；对齐 crypto.rs；纯搬移）。

use mozjs::jsval::JSVal;

use crate::jsapi_glue::{report_error, value_to_string, view_bytes, wrap_cx, Frame};
use super::crypto::{set_rval_bytes, set_rval_str};

// ── 9i-4 ml-kem（FIPS 203；`ml-kem` crate 直用，零版本墙）──────────────────
// 真机口径（node 26.8.2 实测）：SPKI 头定长 22B（ek 裸字节 = SPKI[22..]，三档同构）；
// PKCS#8 = SEQ{INT 0, SEQ{OID}, OCTET{ [0](0x80) 64B 种子 }}（LAMPS 种子形，总长 86）；
// JWK kty "AKP"（pub=ek/priv=种子，b64url）；encapsulate(pub|priv) 均收；
// decapsulate 长度不对 → ERR_CRYPTO_OPERATION_FAILED（FIPS 203 隐式拒绝：等长坏文
// 不报错回伪随机密钥）；decapsulate 非 ml-kem 私钥 → 无码错；异步封装形不做。

/// 参数集 → `(OID DER 内容, ek/ct 裸字节数)`。
pub(crate) fn mlkem_params(kind: &str) -> Option<(&'static [u8], usize, usize)> {
    match kind {
        "ml-kem-512" => Some((&[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x04, 0x01], 800, 768)),
        "ml-kem-768" => Some((&[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x04, 0x02], 1184, 1088)),
        "ml-kem-1024" => Some((&[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x04, 0x03], 1568, 1568)),
        _ => None,
    }
}

pub(crate) fn mlkem_kind_by_oid(oid: &[u8]) -> Option<&'static str> {
    match oid {
        [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x04, 0x01] => Some("ml-kem-512"),
        [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x04, 0x02] => Some("ml-kem-768"),
        [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x04, 0x03] => Some("ml-kem-1024"),
        _ => None,
    }
}

/// 小型 DER TLV 拼装（长度 <65536，键封装足够）。
pub(crate) fn mlkem_tlv(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    if body.len() < 128 {
        out.push(body.len() as u8);
    } else if body.len() < 256 {
        out.extend_from_slice(&[0x81, body.len() as u8]);
    } else {
        out.extend_from_slice(&[0x82, (body.len() >> 8) as u8, (body.len() & 0xff) as u8]);
    }
    out.extend_from_slice(body);
    out
}

/// PKCS#8 私钥（种子形）：`SEQ{INT 0, SEQ{OID}, OCTET{[0] 种子64}}`。
pub(crate) fn mlkem_pkcs8(oid: &[u8], seed: &[u8]) -> Vec<u8> {
    let mut body = mlkem_tlv(0x02, &[0]);
    body.extend_from_slice(&mlkem_tlv(0x30, &mlkem_tlv(0x06, oid)));
    body.extend_from_slice(&mlkem_tlv(0x04, &mlkem_tlv(0x80, seed)));
    mlkem_tlv(0x30, &body)
}

/// SPKI 公钥：`SEQ{ SEQ{OID}, BITSTRING(00‖ek) }`。
pub(crate) fn mlkem_spki(oid: &[u8], ek: &[u8]) -> Vec<u8> {
    let mut bit = vec![0u8];
    bit.extend_from_slice(ek);
    let mut body = mlkem_tlv(0x30, &mlkem_tlv(0x06, oid));
    body.extend_from_slice(&mlkem_tlv(0x03, &bit));
    mlkem_tlv(0x30, &body)
}

/// PKCS#8 → `(OID, 种子)`（结构不合规即 None；种子长按参数集，ml-kem 64 / ml-dsa 32）。
pub(crate) fn mlkem_pkcs8_seed(der: &[u8], seed_len: usize) -> Option<(&[u8], &[u8])> {
    let (t, hl, cl) = crate::builtins::crypto::der_tlv(der)?;
    if t != 0x30 {
        return None;
    }
    let body = &der[hl..hl + cl];
    let (t1, h1, c1) = crate::builtins::crypto::der_tlv(body)?;
    if t1 != 0x02 || body[h1..h1 + c1] != [0] {
        return None;
    }
    let rest = &body[h1 + c1..];
    let (t2, h2, c2) = crate::builtins::crypto::der_tlv(rest)?;
    if t2 != 0x30 {
        return None;
    }
    let alg = &rest[h2..h2 + c2];
    let (t3, h3, c3) = crate::builtins::crypto::der_tlv(alg)?;
    if t3 != 0x06 {
        return None;
    }
    let oid = &alg[h3..h3 + c3];
    let rest2 = &rest[h2 + c2..];
    let (t4, h4, c4) = crate::builtins::crypto::der_tlv(rest2)?;
    if t4 != 0x04 {
        return None;
    }
    let inner = &rest2[h4..h4 + c4];
    let (t5, h5, c5) = crate::builtins::crypto::der_tlv(inner)?;
    if t5 == 0x80 && c5 == seed_len {
        return Some((oid, &inner[h5..h5 + c5]));
    }
    // 10f crypto五轮：展开形（OneAsymmetricKey OCTET 内嵌 SEQ{ OCTET(seed), … }，
    // 套件 ml_dsa_44_private.pem 指纹：SEQ{ OCTET(32), OCTET(2560) }）——
    // 首子 OCTET 即种子；种子形错长仍 None（t5==0x80 不入此分支）。
    if t5 == 0x30 {
        let seq = &inner[h5..h5 + c5];
        if let Some((u5, v5, w5)) = crate::builtins::crypto::der_tlv(seq) {
            if u5 == 0x04 && w5 == seed_len {
                return Some((oid, &seq[v5..v5 + w5]));
            }
        }
    }
    None
}

/// SPKI → `(OID, ek)`（BIT STRING 首字节须为 0 未用位）。
pub(crate) fn mlkem_spki_ek(der: &[u8]) -> Option<(&[u8], &[u8])> {
    let (t, hl, cl) = crate::builtins::crypto::der_tlv(der)?;
    if t != 0x30 {
        return None;
    }
    let body = &der[hl..hl + cl];
    let (t1, h1, c1) = crate::builtins::crypto::der_tlv(body)?;
    if t1 != 0x30 {
        return None;
    }
    let alg = &body[h1..h1 + c1];
    let (t2, h2, c2) = crate::builtins::crypto::der_tlv(alg)?;
    if t2 != 0x06 {
        return None;
    }
    let oid = &alg[h2..h2 + c2];
    let rest = &body[h1 + c1..];
    let (t3, h3, c3) = crate::builtins::crypto::der_tlv(rest)?;
    if t3 != 0x03 || c3 < 2 || rest[h3] != 0 {
        return None;
    }
    Some((oid, &rest[h3 + 1..h3 + c3]))
}

/// 64B 种子 → `(PKCS#8, SPKI)`（展开即校验，坏种子报错）。
pub(crate) fn mlkem_expand(kind: &str, seed: &[u8; 64]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let (oid, ek_len, _) = mlkem_params(kind)
        .ok_or_else(|| "NotSupportedError: unsupported ml-kem parameter set".to_string())?;
    macro_rules! case {
        ($P:ty, $name:expr) => {
            if kind == $name {
                use ml_kem::KeyExport as _;
                let dk = ml_kem::DecapsulationKey::<$P>::from_seed((*seed).into());
                let ek = dk.encapsulation_key().to_bytes();
                if ek.as_slice().len() != ek_len {
                    return Err("OperationError: ml-kem ek length mismatch".into());
                }
                return Ok((mlkem_pkcs8(oid, seed), mlkem_spki(oid, ek.as_slice())));
            }
        };
    }
    case!(ml_kem::MlKem512, "ml-kem-512");
    case!(ml_kem::MlKem768, "ml-kem-768");
    case!(ml_kem::MlKem1024, "ml-kem-1024");
    Err("NotSupportedError: unsupported ml-kem parameter set".into())
}

/// `__wjs2_mlkem_gen(kind)` → JSON `{pkcs8, spki}`（b64；种子 getrandom 自造）。
pub unsafe extern "C" fn mlkem_gen(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let kind = if frame.argc() > 0 { value_to_string(&mut cx, frame.arg(0)) } else { String::new() };
    if mlkem_params(&kind).is_none() {
        report_error(&mut cx, "NotSupportedError: unsupported ml-kem parameter set");
        return false;
    }
    let mut seed = [0u8; 64];
    if getrandom::fill(&mut seed).is_err() {
        report_error(&mut cx, "OperationError: cannot get random values");
        return false;
    }
    match mlkem_expand(&kind, &seed) {
        Ok((pkcs8, spki)) => {
            use base64::Engine as _;
            let json = serde_json::json!({
                "pkcs8": base64::engine::general_purpose::STANDARD.encode(pkcs8),
                "spki": base64::engine::general_purpose::STANDARD.encode(spki),
            })
            .to_string();
            set_rval_str(&mut cx, &frame, &json);
            true
        }
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs2_mlkem_seed_from_pkcs8(der)` → JSON `{kind, spki}`（b64；导入即展开校验）。
pub unsafe extern "C" fn mlkem_seed_from_pkcs8(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(der) = (if frame.argc() > 0 { view_bytes(&mut cx, frame.arg(0), "ml-kem key") } else { None }) else {
        return false;
    };
    let parsed = match mlkem_pkcs8_seed(&der, 64) {
        Some(p) => p,
        None => {
            report_error(&mut cx, "TypeError: Invalid PKCS#8 key");
            return false;
        }
    };
    let Some(kind) = mlkem_kind_by_oid(parsed.0) else {
        report_error(&mut cx, "TypeError: Invalid PKCS#8 key");
        return false;
    };
    let mut seed = [0u8; 64];
    seed.copy_from_slice(parsed.1);
    match mlkem_expand(kind, &seed) {
        Ok((_, spki)) => {
            use base64::Engine as _;
            let json = serde_json::json!({
                "kind": kind,
                "seed": base64::engine::general_purpose::STANDARD.encode(seed),
                "spki": base64::engine::general_purpose::STANDARD.encode(spki),
            })
            .to_string();
            set_rval_str(&mut cx, &frame, &json);
            true
        }
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs2_mlkem_kind_from_spki(der)` → kind 串（OID + ek 长度校验，失败回空串）。
pub unsafe extern "C" fn mlkem_kind_from_spki(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(der) = (if frame.argc() > 0 { view_bytes(&mut cx, frame.arg(0), "ml-kem key") } else { None }) else {
        return false;
    };
    let kind = mlkem_spki_ek(&der)
        .and_then(|(oid, ek)| {
            mlkem_kind_by_oid(oid).filter(|k| mlkem_params(k).is_some_and(|(_, ek_len, _)| ek_len == ek.len()))
        })
        .unwrap_or("");
    use mozjs::conversions::ToJSValConvertible as _;
    kind.to_jsval(&mut cx, frame.rval_mut());
    true
}

/// 三参数集分发：封装（ek 裸字节 → 借用 `&Key` 构造 → `encapsulate_with_rng`）。
macro_rules! mlkem_encaps_case {
    ($kind:expr, $ek:expr, $P:ty, $name:expr) => {
        if $kind == $name {
            Some((|| -> Result<(Vec<u8>, Vec<u8>), String> {
                let arr = <&ml_kem::Key<ml_kem::EncapsulationKey<$P>>>::try_from($ek)
                    .map_err(|_| "ERR_CRYPTO_OPERATION_FAILED: Encapsulation failed".to_string())?;
                let key = ml_kem::EncapsulationKey::<$P>::new(arr)
                    .map_err(|_| "ERR_CRYPTO_OPERATION_FAILED: Encapsulation failed".to_string())?;
                use ml_kem::Encapsulate as _;
                let (ct, sk) = key.encapsulate_with_rng(&mut rand::rng());
                Ok((ct.as_slice().to_vec(), sk.as_slice().to_vec()))
            })())
        } else {
            None
        }
    };
}

/// 三参数集分发：解封装（种子 → `KeyInit::new` → `decapsulate_slice`，长度内建校验）。
macro_rules! mlkem_decaps_case {
    ($kind:expr, $seed:expr, $ct:expr, $P:ty, $name:expr) => {
        if $kind == $name {
            Some((|| -> Result<Vec<u8>, String> {
                let arr = <&ml_kem::Key<ml_kem::DecapsulationKey<$P>>>::try_from($seed)
                    .map_err(|_| "ERR_CRYPTO_OPERATION_FAILED: Decapsulation failed".to_string())?;
                use ml_kem::KeyInit as _;
                let dk = ml_kem::DecapsulationKey::<$P>::new(arr);
                use ml_kem::Decapsulate as _;
                dk.decapsulate_slice($ct)
                    .map(|sk| sk.as_slice().to_vec())
                    .map_err(|_| "ERR_CRYPTO_OPERATION_FAILED: Decapsulation failed".to_string())
            })())
        } else {
            None
        }
    };
}

/// `__wjs2_mlkem_encaps(keyDer, isPriv)` → JSON `{ct, sk}`（b64）。
/// keyDer：isPriv=0 为 SPKI（ek 直用），=1 为 PKCS#8（种子展开）。
pub unsafe extern "C" fn mlkem_encaps(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "ERR_CRYPTO_OPERATION_FAILED: Encapsulation failed");
        return false;
    }
    let Some(der) = view_bytes(&mut cx, frame.arg(0), "ml-kem key") else {
        return false;
    };
    let is_priv = frame.arg(1).to_number() != 0.0;
    let parsed = if is_priv {
        mlkem_pkcs8_seed(&der, 64).and_then(|(oid, seed)| mlkem_kind_by_oid(oid).map(|k| (k, seed.to_vec(), Vec::new())))
    } else {
        mlkem_spki_ek(&der).and_then(|(oid, ek)| mlkem_kind_by_oid(oid).map(|k| (k, Vec::new(), ek.to_vec())))
    };
    let Some((kind, seed, ek)) = parsed else {
        report_error(&mut cx, "ERR_CRYPTO_OPERATION_FAILED: Encapsulation failed");
        return false;
    };
    let Some((_, ek_len, _)) = mlkem_params(kind) else {
        report_error(&mut cx, "ERR_CRYPTO_OPERATION_FAILED: Encapsulation failed");
        return false;
    };
    // 私钥入参：先展开种子得 ek（一并完成校验）。
    let ek_bytes: Vec<u8> = if is_priv {
        let mut s = [0u8; 64];
        s.copy_from_slice(&seed);
        match mlkem_expand(kind, &s) {
            Ok((_, spki)) => match mlkem_spki_ek(&spki) {
                Some((_, e)) => e.to_vec(),
                None => {
                    report_error(&mut cx, "ERR_CRYPTO_OPERATION_FAILED: Encapsulation failed");
                    return false;
                }
            },
            Err(e) => {
                report_error(&mut cx, &e);
                return false;
            }
        }
    } else {
        ek
    };
    if ek_bytes.len() != ek_len {
        report_error(&mut cx, "ERR_CRYPTO_OPERATION_FAILED: Encapsulation failed");
        return false;
    }
    let enc: Result<(Vec<u8>, Vec<u8>), String> =
        mlkem_encaps_case!(kind, ek_bytes.as_slice(), ml_kem::MlKem512, "ml-kem-512")
            .or_else(|| mlkem_encaps_case!(kind, ek_bytes.as_slice(), ml_kem::MlKem768, "ml-kem-768"))
            .or_else(|| mlkem_encaps_case!(kind, ek_bytes.as_slice(), ml_kem::MlKem1024, "ml-kem-1024"))
            .unwrap_or_else(|| Err("ERR_CRYPTO_OPERATION_FAILED: Encapsulation failed".into()));
    match enc {
        Ok((ct, sk)) => {
            use base64::Engine as _;
            let json = serde_json::json!({
                "ct": base64::engine::general_purpose::STANDARD.encode(ct),
                "sk": base64::engine::general_purpose::STANDARD.encode(sk),
            })
            .to_string();
            set_rval_str(&mut cx, &frame, &json);
            true
        }
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs2_mlkem_decaps(pkcs8Der, ct)` → 32B 共享密钥；长度不对报
/// `ERR_CRYPTO_OPERATION_FAILED`（等长坏文按 FIPS 203 走隐式拒绝，不报错）。
pub unsafe extern "C" fn mlkem_decaps(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "ERR_CRYPTO_OPERATION_FAILED: Decapsulation failed");
        return false;
    }
    let (Some(der), Some(ct)) = (
        view_bytes(&mut cx, frame.arg(0), "ml-kem key"),
        view_bytes(&mut cx, frame.arg(1), "ciphertext"),
    ) else {
        return false;
    };
    let out: Result<Vec<u8>, String> = (|| {
        let Some((oid, seed)) = mlkem_pkcs8_seed(&der, 64)
            .and_then(|(o, s)| mlkem_kind_by_oid(o).map(|k| (k, s)))
        else {
            return Err("ERR_CRYPTO_OPERATION_FAILED: Decapsulation failed".into());
        };
        let Some((_, _, ct_len)) = mlkem_params(oid) else {
            return Err("ERR_CRYPTO_OPERATION_FAILED: Decapsulation failed".into());
        };
        if ct.len() != ct_len {
            return Err("ERR_CRYPTO_OPERATION_FAILED: Decapsulation failed".into());
        }
        mlkem_decaps_case!(oid, seed, ct.as_slice(), ml_kem::MlKem512, "ml-kem-512")
            .or_else(|| mlkem_decaps_case!(oid, seed, ct.as_slice(), ml_kem::MlKem768, "ml-kem-768"))
            .or_else(|| mlkem_decaps_case!(oid, seed, ct.as_slice(), ml_kem::MlKem1024, "ml-kem-1024"))
            .unwrap_or_else(|| Err("ERR_CRYPTO_OPERATION_FAILED: Decapsulation failed".into()))
    })();
    match out {
        Ok(sk) => set_rval_bytes(&mut cx, &frame, &sk),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

// ── 9i-6 ml-dsa（FIPS 204；`ml-dsa` crate，纯签名 hash=null）───────────────
// 真机口径（node 26.8.2 实测）：SPKI = 22B 头 + 裸 pk（1312/1952/2592）；
// PKCS#8 = SEQ{INT 0, SEQ{OID}, OCTET{[0] 32B 种子}}（总长 54，OID …3.4.3.17/18/19）；
// `crypto.sign(null, data, key)` 纯签名（非 null 即 ERR_OSSL_INVALID_DIGEST）；
// 签名长 2420/3309/4627；JWK kty "AKP"（priv=32B 种子）。本仓 Signer 走 crate 的
// 确定性档（真机为 hedged，双方互验不受影响）。

pub(crate) fn mldsa_params(kind: &str) -> Option<(&'static [u8], usize, usize)> {
    match kind {
        "ml-dsa-44" => Some((&[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x03, 0x11], 1312, 2420)),
        "ml-dsa-65" => Some((&[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x03, 0x12], 1952, 3309)),
        "ml-dsa-87" => Some((&[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x03, 0x13], 2592, 4627)),
        _ => None,
    }
}

pub(crate) fn mldsa_kind_by_oid(oid: &[u8]) -> Option<&'static str> {
    match oid {
        [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x03, 0x11] => Some("ml-dsa-44"),
        [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x03, 0x12] => Some("ml-dsa-65"),
        [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x03, 0x13] => Some("ml-dsa-87"),
        _ => None,
    }
}

/// dotted 串 OID → kind（X.509 证书签名算法用）。
pub(crate) fn mldsa_kind_by_oid_str(oid: &str) -> Option<&'static str> {
    match oid {
        "2.16.840.1.101.3.4.3.17" => Some("ml-dsa-44"),
        "2.16.840.1.101.3.4.3.18" => Some("ml-dsa-65"),
        "2.16.840.1.101.3.4.3.19" => Some("ml-dsa-87"),
        _ => None,
    }
}

/// SPKI → `(kind, 裸 pk)`（X.509 验签用；OID/pk 长度不符即 None）。
pub(crate) fn mldsa_spki_pk(spki: &[u8]) -> Option<(&'static str, Vec<u8>)> {
    let (oid, pk) = mlkem_spki_ek(spki)?;
    let kind = mldsa_kind_by_oid(oid)?;
    mldsa_params(kind).is_some_and(|(_, pk_len, _)| pk_len == pk.len()).then(|| (kind, pk.to_vec()))
}

/// 32B 种子 → `(PKCS#8, SPKI)`（展开即校验）。
pub(crate) fn mldsa_expand(kind: &str, seed: &[u8; 32]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let (oid, pk_len, _) = mldsa_params(kind)
        .ok_or_else(|| "NotSupportedError: unsupported ml-dsa parameter set".to_string())?;
    macro_rules! case {
        ($P:ty, $name:expr) => {
            if kind == $name {
                use ml_dsa::{Keypair as _, KeyExport as _};
                let sk = ml_dsa::SigningKey::<$P>::from_seed(&(*seed).into());
                let pk = sk.verifying_key().to_bytes();
                if pk.as_slice().len() != pk_len {
                    return Err("OperationError: ml-dsa pk length mismatch".into());
                }
                return Ok((mlkem_pkcs8(oid, seed), mlkem_spki(oid, pk.as_slice())));
            }
        };
    }
    case!(ml_dsa::MlDsa44, "ml-dsa-44");
    case!(ml_dsa::MlDsa65, "ml-dsa-65");
    case!(ml_dsa::MlDsa87, "ml-dsa-87");
    Err("NotSupportedError: unsupported ml-dsa parameter set".into())
}

/// `__wjs2_mldsa_gen(kind)` → JSON `{pkcs8, spki}`（b64；种子 getrandom 自造）。
pub unsafe extern "C" fn mldsa_gen(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let kind = if frame.argc() > 0 { value_to_string(&mut cx, frame.arg(0)) } else { String::new() };
    if mldsa_params(&kind).is_none() {
        report_error(&mut cx, "NotSupportedError: unsupported ml-dsa parameter set");
        return false;
    }
    let mut seed = [0u8; 32];
    if getrandom::fill(&mut seed).is_err() {
        report_error(&mut cx, "OperationError: cannot get random values");
        return false;
    }
    match mldsa_expand(&kind, &seed) {
        Ok((pkcs8, spki)) => {
            use base64::Engine as _;
            let json = serde_json::json!({
                "pkcs8": base64::engine::general_purpose::STANDARD.encode(pkcs8),
                "spki": base64::engine::general_purpose::STANDARD.encode(spki),
            })
            .to_string();
            set_rval_str(&mut cx, &frame, &json);
            true
        }
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs2_mldsa_seed_from_pkcs8(der)` → JSON `{kind, seed, spki}`（b64；导入即展开校验）。
pub unsafe extern "C" fn mldsa_seed_from_pkcs8(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(der) = (if frame.argc() > 0 { view_bytes(&mut cx, frame.arg(0), "ml-dsa key") } else { None }) else {
        return false;
    };
    let Some((oid, seed)) = mlkem_pkcs8_seed(&der, 32).and_then(|(o, s)| mldsa_kind_by_oid(o).map(|k| (k, s))) else {
        report_error(&mut cx, "TypeError: Invalid PKCS#8 key");
        return false;
    };
    let mut s = [0u8; 32];
    s.copy_from_slice(seed);
    match mldsa_expand(oid, &s) {
        Ok((_, spki)) => {
            use base64::Engine as _;
            let json = serde_json::json!({
                "kind": oid,
                "seed": base64::engine::general_purpose::STANDARD.encode(s),
                "spki": base64::engine::general_purpose::STANDARD.encode(spki),
            })
            .to_string();
            set_rval_str(&mut cx, &frame, &json);
            true
        }
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs2_mldsa_kind_from_spki(der)` → kind 串（OID + pk 长度校验，失败回空串）。
pub unsafe extern "C" fn mldsa_kind_from_spki(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(der) = (if frame.argc() > 0 { view_bytes(&mut cx, frame.arg(0), "ml-dsa key") } else { None }) else {
        return false;
    };
    let kind = mlkem_spki_ek(&der)
        .and_then(|(oid, pk)| mldsa_kind_by_oid(oid).filter(|k| mldsa_params(k).is_some_and(|(_, pk_len, _)| pk_len == pk.len())))
        .unwrap_or("");
    use mozjs::conversions::ToJSValConvertible as _;
    kind.to_jsval(&mut cx, frame.rval_mut());
    true
}

/// `__wjs2_mldsa_public(pkcs8Der)` → SPKI DER（私钥派生公钥，createPublicKey 链用）。
pub unsafe extern "C" fn mldsa_public(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(der) = (if frame.argc() > 0 { view_bytes(&mut cx, frame.arg(0), "ml-dsa key") } else { None }) else {
        return false;
    };
    let Some((oid, seed)) = mlkem_pkcs8_seed(&der, 32).and_then(|(o, s)| mldsa_kind_by_oid(o).map(|k| (k, s))) else {
        report_error(&mut cx, "TypeError: Invalid PKCS#8 key");
        return false;
    };
    let mut s = [0u8; 32];
    s.copy_from_slice(seed);
    match mldsa_expand(oid, &s) {
        Ok((_, spki)) => set_rval_bytes(&mut cx, &frame, &spki),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs2_mldsa_sign(pkcs8Der, data)` → 签名（确定性档，FIPS 204 可选形；空上下文）。
pub unsafe extern "C" fn mldsa_sign(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: ml-dsa sign needs key and data");
        return false;
    }
    let (Some(der), Some(data)) = (
        view_bytes(&mut cx, frame.arg(0), "ml-dsa key"),
        view_bytes(&mut cx, frame.arg(1), "data"),
    ) else {
        return false;
    };
    let Some((oid, seed)) = mlkem_pkcs8_seed(&der, 32).and_then(|(o, s)| mldsa_kind_by_oid(o).map(|k| (k, s))) else {
        report_error(&mut cx, "TypeError: Invalid PKCS#8 key");
        return false;
    };
    let mut s = [0u8; 32];
    s.copy_from_slice(seed);
    use ml_dsa::Signer as _;
    let out: Result<Vec<u8>, String> = match oid {
        "ml-dsa-44" => {
            let sig = ml_dsa::SigningKey::<ml_dsa::MlDsa44>::from_seed(&s.into()).sign(&data);
            Ok(sig.encode().as_slice().to_vec())
        }
        "ml-dsa-65" => {
            let sig = ml_dsa::SigningKey::<ml_dsa::MlDsa65>::from_seed(&s.into()).sign(&data);
            Ok(sig.encode().as_slice().to_vec())
        }
        _ => {
            let sig = ml_dsa::SigningKey::<ml_dsa::MlDsa87>::from_seed(&s.into()).sign(&data);
            Ok(sig.encode().as_slice().to_vec())
        }
    };
    match out {
        Ok(sig) => set_rval_bytes(&mut cx, &frame, &sig),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs2_mldsa_verify(pubDer, sig, data)` → boolean。
pub unsafe extern "C" fn mldsa_verify(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: ml-dsa verify needs key, signature and data");
        return false;
    }
    let (Some(der), Some(sig), Some(data)) = (
        view_bytes(&mut cx, frame.arg(0), "ml-dsa key"),
        view_bytes(&mut cx, frame.arg(1), "signature"),
        view_bytes(&mut cx, frame.arg(2), "data"),
    ) else {
        return false;
    };
    let Some((oid, pk)) = mlkem_spki_ek(&der).and_then(|(o, p)| mldsa_kind_by_oid(o).map(|k| (k, p))) else {
        report_error(&mut cx, "TypeError: Invalid SPKI key");
        return false;
    };
    let ok = mldsa_verify_core(oid, pk, &sig, &data);
    frame.set_rval(mozjs::jsval::BooleanValue(ok));
    true
}

/// 验签核（native 与 X.509 验签共用；pub/kind/sig/data 逐项校验，不过即 false）。
pub(crate) fn mldsa_verify_core(kind: &str, pk: &[u8], sig: &[u8], data: &[u8]) -> bool {
    use ml_dsa::KeyInit as _;
    macro_rules! case {
        ($P:ty, $name:expr) => {
            if kind == $name {
                let (Some(arr), Some(sig)) = (
                    <&ml_dsa::common::Key<ml_dsa::VerifyingKey<$P>>>::try_from(pk).ok(),
                    ml_dsa::Signature::<$P>::try_from(sig).ok(),
                ) else {
                    return false;
                };
                let vk = ml_dsa::VerifyingKey::<$P>::new(arr);
                use ml_dsa::Verifier as _;
                return vk.verify(data, &sig).is_ok();
            }
        };
    }
    case!(ml_dsa::MlDsa44, "ml-dsa-44");
    case!(ml_dsa::MlDsa65, "ml-dsa-65");
    case!(ml_dsa::MlDsa87, "ml-dsa-87");
    false
}

