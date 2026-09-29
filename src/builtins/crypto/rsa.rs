//! crypto RSA 面（生成/加解密/签名验签/JWK/PSS；含手工 verify 底座）。

use super::common::*;
use mozjs::conversions::ToJSValConvertible as _;

use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;
use crate::jsapi_glue::{report_error, value_to_string, view_bytes, wrap_cx, Frame};

/// RSA 哈希分发（SHA-256/384/512；SHA-1 已废弃，报 `NotSupportedError` 指到 SHA-2）。
macro_rules! rsa_hash_dispatch {
    ($hash:expr, $D:ident, $body:expr) => {{
        match $hash {
            // 10f crypto六轮：SHA-1 经 sha1_010（digest 0.10 系，rsa 0.9 互通；
            // MD5 仍不支持——md-5 树内只有 0.11 系，无 0.10 可直引）。
            "SHA-1" => {
                type $D = sha1_010::Sha1;
                $body
            }
            "SHA-256" => {
                type $D = sha2_010::Sha256;
                $body
            }
            "SHA-384" => {
                type $D = sha2_010::Sha384;
                $body
            }
            "SHA-512" => {
                type $D = sha2_010::Sha512;
                $body
            }
            other => Err(format!("NotSupportedError: RSA with hash '{other}' needs SHA-1/256/384/512")),
        }
    }};
}

/// DER 字节 → `RsaPrivateKey`（`DataError` 口径）。
fn rsa_priv_from_der(der: &[u8]) -> Result<rsa::RsaPrivateKey, String> {
    use rsa::pkcs8::DecodePrivateKey as _;
    rsa::RsaPrivateKey::from_pkcs8_der(der).map_err(|_| "DataError: bad RSA private key (PKCS#8)".to_string())
}

/// DER 字节 → `RsaPublicKey`（`DataError` 口径）。
pub(crate) fn rsa_pub_from_der(der: &[u8]) -> Result<rsa::RsaPublicKey, String> {
    use rsa::pkcs8::DecodePublicKey as _;
    rsa::RsaPublicKey::from_public_key_der(der).map_err(|_| "DataError: bad RSA public key (SPKI)".to_string())
}

/// `__wjs2_rsa_generate(bits, e)` → PKCS#8 DER 私钥（2048/3072/4096；e 常用 65537）。
pub unsafe extern "C" fn rsa_generate(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 || !frame.arg(0).is_number() || !frame.arg(1).is_number() {
        report_error(&mut cx, "TypeError: RSA generate needs modulusLength and publicExponent");
        return false;
    }
    let bits = frame.arg(0).to_number() as usize;
    let e = frame.arg(1).to_number() as u64;
    // 10f crypto二轮：位长下限 512（真机口径；subtle 面 JS 门仍限 2048/3072/4096，
    // 此处仅为 node:crypto 开口，见 `src/builtins/mod.rs` 门控注释）。
    if bits < 512 {
        report_error(&mut cx, "ERR_OSSL_KEY_SIZE_TOO_SMALL: error:1C8000AB:Provider routines::key size too small");
        return false;
    }
    if !(2..=(1 << 33) - 1).contains(&e) {
        report_error(&mut cx, "DataError: bad RSA publicExponent");
        return false;
    }
    if !rng_probe(&mut cx) {
        return false;
    }
    let exp = rsa::BigUint::from(e);
    let key = match rsa::RsaPrivateKey::new_with_exp(&mut SystemRng, bits, &exp) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &format!("OperationError: RSA key generation failed: {e}"));
            return false;
        }
    };
    use rsa::pkcs8::EncodePrivateKey as _;
    let der = match key.to_pkcs8_der() {
        Ok(d) => d.as_bytes().to_vec(),
        Err(e) => {
            report_error(&mut cx, &format!("OperationError: RSA key export failed: {e}"));
            return false;
        }
    };
    tracing::debug!(target: "winterjs2::crypto", bits, "RSA key generated");
    set_rval_bytes(&mut cx, &frame, &der)
}

/// `__wjs2_rsa_public(privDer)` → SPKI DER 公钥。
pub unsafe extern "C" fn rsa_public(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: RSA public needs a private key");
        return false;
    }
    let Some(der) = view_bytes(&mut cx, frame.arg(0), "RSA private key") else {
        return false;
    };
    let priv_key = match rsa_priv_from_der(&der) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    use rsa::pkcs8::EncodePublicKey as _;
    let spki = match priv_key.to_public_key().to_public_key_der() {
        Ok(d) => d.as_bytes().to_vec(),
        Err(e) => {
            report_error(&mut cx, &format!("OperationError: RSA public export failed: {e}"));
            return false;
        }
    };
    set_rval_bytes(&mut cx, &frame, &spki)
}

/// `__wjs2_rsa_sign(hash, privDer, data)` → 签名（RSASSA-PKCS1-v1_5）。
pub unsafe extern "C" fn rsa_sign(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: RSA sign needs hash, key and data");
        return false;
    }
    let hash = value_to_string(&mut cx, frame.arg(0));
    let (Some(der), Some(data)) = (
        view_bytes(&mut cx, frame.arg(1), "RSA private key"),
        view_bytes(&mut cx, frame.arg(2), "RSA data"),
    ) else {
        return false;
    };
    let priv_key = match rsa_priv_from_der(&der) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let out: Result<Vec<u8>, String> = rsa_hash_dispatch!(hash.as_str(), D, {
        use rsa::signature::Signer as _;
        // 10f crypto二轮：`Signer::sign` 内部 unwrap（摘要+11 超钥长即 panic→139），
        // 先验长度转可读错（UNSAFE-BOUNDARY panic 路径；sha256/384/512 = 32/48/64）。
        let need = match hash.as_str() {
            "SHA-384" => 48 + 11,
            "SHA-512" => 64 + 11,
            _ => 32 + 11,
        };
        use rsa::traits::PublicKeyParts as _;
        let k = priv_key.size();
        if need > k {
            Err("ERR_OSSL_RSA_DIGEST_TOO_BIG_FOR_RSA_KEY: error:02000070:rsa routines::digest too big for rsa key".to_string())
        } else {
            let sk = rsa::pkcs1v15::SigningKey::<D>::new(priv_key);
            Ok(Box::<[u8]>::from(sk.sign(&data)).into_vec())
        }
    });
    match out {
        Ok(sig) => set_rval_bytes(&mut cx, &frame, &sig),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs2_rsa_verify(hash, pubDer, sig, data)` → boolean。
pub unsafe extern "C" fn rsa_verify(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 {
        report_error(&mut cx, "TypeError: RSA verify needs hash, key, signature and data");
        return false;
    }
    let hash = value_to_string(&mut cx, frame.arg(0));
    let (Some(der), Some(sig), Some(data)) = (
        view_bytes(&mut cx, frame.arg(1), "RSA public key"),
        view_bytes(&mut cx, frame.arg(2), "RSA signature"),
        view_bytes(&mut cx, frame.arg(3), "RSA data"),
    ) else {
        return false;
    };
    let pub_key = match rsa_pub_from_der(&der) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let out: Result<bool, String> = rsa_hash_dispatch!(hash.as_str(), D, {
        use rsa::signature::Verifier as _;
        let vk = rsa::pkcs1v15::VerifyingKey::<D>::new(pub_key);
        match rsa::pkcs1v15::Signature::try_from(sig.as_slice()) {
            Ok(s) => Ok(vk.verify(&data, &s).is_ok()),
            Err(_) => Err("OperationError: bad RSA signature length".to_string()),
        }
    });
    match out {
        Ok(ok) => {
            frame.set_rval(mozjs::jsval::BooleanValue(ok));
            true
        }
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs2_rsa_encrypt(hash, pubDer, data, label?)` → 密文（RSA-OAEP，label 可选）。
pub unsafe extern "C" fn rsa_encrypt(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: RSA encrypt needs hash, key and data");
        return false;
    }
    let hash = value_to_string(&mut cx, frame.arg(0));
    let (Some(der), Some(data)) = (
        view_bytes(&mut cx, frame.arg(1), "RSA public key"),
        view_bytes(&mut cx, frame.arg(2), "RSA data"),
    ) else {
        return false;
    };
    let label: Option<String> = if frame.argc() > 3 {
        match opt_view_bytes(&mut cx, frame.arg(3), "RSA label") {
            Some(Some(bytes)) => match String::from_utf8(bytes) {
                Ok(s) => Some(s),
                Err(_) => {
                    report_error(&mut cx, "DataError: RSA label must be UTF-8");
                    return false;
                }
            },
            Some(None) => None,
            None => return false,
        }
    } else {
        None
    };
    let pub_key = match rsa_pub_from_der(&der) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    if !rng_probe(&mut cx) {
        return false;
    }
    let out: Result<Vec<u8>, String> = rsa_hash_dispatch!(hash.as_str(), D, {
        let padding = match label {
            Some(l) => rsa::Oaep::new_with_label::<D, _>(l),
            None => rsa::Oaep::new::<D>(),
        };
        pub_key
            .encrypt(&mut SystemRng, padding, &data)
            .map_err(|e| format!("OperationError: RSA encrypt failed: {e}"))
    });
    match out {
        Ok(ct) => set_rval_bytes(&mut cx, &frame, &ct),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs2_rsa_decrypt(hash, privDer, data, label?)` → 明文（RSA-OAEP）。
pub unsafe extern "C" fn rsa_decrypt(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: RSA decrypt needs hash, key and data");
        return false;
    }
    let hash = value_to_string(&mut cx, frame.arg(0));
    let (Some(der), Some(data)) = (
        view_bytes(&mut cx, frame.arg(1), "RSA private key"),
        view_bytes(&mut cx, frame.arg(2), "RSA data"),
    ) else {
        return false;
    };
    let label: Option<String> = if frame.argc() > 3 {
        match opt_view_bytes(&mut cx, frame.arg(3), "RSA label") {
            Some(Some(bytes)) => match String::from_utf8(bytes) {
                Ok(s) => Some(s),
                Err(_) => {
                    report_error(&mut cx, "DataError: RSA label must be UTF-8");
                    return false;
                }
            },
            Some(None) => None,
            None => return false,
        }
    } else {
        None
    };
    let priv_key = match rsa_priv_from_der(&der) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let out: Result<Vec<u8>, String> = rsa_hash_dispatch!(hash.as_str(), D, {
        let padding = match label {
            Some(l) => rsa::Oaep::new_with_label::<D, _>(l),
            None => rsa::Oaep::new::<D>(),
        };
        priv_key
            .decrypt(padding, &data)
            // 10f crypto二轮：解密失败（错钥/错标签/错哈希/篡改）统一口径（真机逐字）。
            .map_err(|_| "ERR_OSSL_RSA_OAEP_DECODING_ERROR: error:02000079:rsa routines::oaep decoding error".to_string())
    });
    match out {
        Ok(pt) => set_rval_bytes(&mut cx, &frame, &pt),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs2_rsa_jwk(privDer, pubDer)` → JWK 参数 JSON（含私钥段；prelude 组 JWK 对象）。
pub unsafe extern "C" fn rsa_jwk(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: RSA JWK needs private and public keys");
        return false;
    }
    let (Some(priv_der), Some(pub_der)) = (
        view_bytes(&mut cx, frame.arg(0), "RSA private key"),
        view_bytes(&mut cx, frame.arg(1), "RSA public key"),
    ) else {
        return false;
    };
    let priv_key = match rsa_priv_from_der(&priv_der) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let pub_key = match rsa_pub_from_der(&pub_der) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    use rsa::traits::PublicKeyParts as _;
    use rsa::traits::PrivateKeyParts as _;
    // CRT 参数经预计算取（生成/导入路径必跑 precompute；None 即密钥损坏）。
    let (Some(dp), Some(dq), Some(qi)) = (priv_key.dp(), priv_key.dq(), priv_key.qinv()) else {
        report_error(&mut cx, "DataError: bad RSA key (no CRT params)");
        return false;
    };
    let (_, qi_bytes) = qi.to_bytes_be();
    let (p, q) = match (priv_key.primes().first(), priv_key.primes().get(1)) {
        (Some(p), Some(q)) => (p, q),
        _ => {
            report_error(&mut cx, "DataError: RSA key missing primes");
            return false;
        }
    };
    let json = serde_json::json!({
        "n": bignum(pub_key.n()),
        "e": bignum(pub_key.e()),
        "d": bignum(priv_key.d()),
        "p": bignum(p),
        "q": bignum(q),
        "dp": bignum(dp),
        "dq": bignum(dq),
        "qi": b64url(&qi_bytes),
    })
    .to_string();
    rooted!(&in(cx) let mut v = UndefinedValue());
    json.to_jsval(&mut cx, v.handle_mut());
    frame.set_rval(v.get());
    true
}

/// `__wjs2_rsa_jwk_pub(pubDer)` → 公钥参数 JSON（`{n,e}`；spki import 组 algorithm 用）。
pub unsafe extern "C" fn rsa_jwk_pub(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: RSA JWK needs a public key");
        return false;
    }
    let Some(pub_der) = view_bytes(&mut cx, frame.arg(0), "RSA public key") else {
        return false;
    };
    let pub_key = match rsa_pub_from_der(&pub_der) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    use rsa::traits::PublicKeyParts as _;
    let json = serde_json::json!({
        "n": bignum(pub_key.n()),
        "e": bignum(pub_key.e()),
    })
    .to_string();
    rooted!(&in(cx) let mut v = UndefinedValue());
    json.to_jsval(&mut cx, v.handle_mut());
    frame.set_rval(v.get());
    true
}

/// `__wjs2_rsa_import_priv(nU8, eU8, dU8)` → PKCS#8 DER（p/q 按 SP 800-56B 恢复）。
pub unsafe extern "C" fn rsa_import_priv(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: RSA import needs n, e and d");
        return false;
    }
    let (Some(n), Some(e), Some(d)) = (
        view_bytes(&mut cx, frame.arg(0), "RSA n"),
        view_bytes(&mut cx, frame.arg(1), "RSA e"),
        view_bytes(&mut cx, frame.arg(2), "RSA d"),
    ) else {
        return false;
    };
    let key = match rsa::RsaPrivateKey::from_components(
        rsa::BigUint::from_bytes_be(&n),
        rsa::BigUint::from_bytes_be(&e),
        rsa::BigUint::from_bytes_be(&d),
        vec![],
    ) {
        Ok(k) => k,
        Err(_) => {
            report_error(&mut cx, "DataError: bad RSA JWK (n/e/d)");
            return false;
        }
    };
    use rsa::pkcs8::EncodePrivateKey as _;
    let der = match key.to_pkcs8_der() {
        Ok(d) => d.as_bytes().to_vec(),
        Err(e) => {
            report_error(&mut cx, &format!("OperationError: RSA import failed: {e}"));
            return false;
        }
    };
    set_rval_bytes(&mut cx, &frame, &der)
}

/// `__wjs2_rsa_import_pub(nU8, eU8)` → SPKI DER。
pub unsafe extern "C" fn rsa_import_pub(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: RSA import needs n and e");
        return false;
    }
    let (Some(n), Some(e)) = (
        view_bytes(&mut cx, frame.arg(0), "RSA n"),
        view_bytes(&mut cx, frame.arg(1), "RSA e"),
    ) else {
        return false;
    };
    let key = match rsa::RsaPublicKey::new(
        rsa::BigUint::from_bytes_be(&n),
        rsa::BigUint::from_bytes_be(&e),
    ) {
        Ok(k) => k,
        Err(_) => {
            report_error(&mut cx, "DataError: bad RSA JWK (n/e)");
            return false;
        }
    };
    use rsa::pkcs8::EncodePublicKey as _;
    let der = match key.to_public_key_der() {
        Ok(d) => d.as_bytes().to_vec(),
        Err(e) => {
            report_error(&mut cx, &format!("OperationError: RSA import failed: {e}"));
            return false;
        }
    };
    set_rval_bytes(&mut cx, &frame, &der)
}
// 边界惯例同文件头：每 native 固定 `wrap_cx` + `Frame::from_raw` 两块
//（UNSAFE-BOUNDARY，结构性计数；黑盒见 tests/cli.rs `subtle_c4x_*`）。
// AES-192 经泛型 `AesGcm<Aes192, U12>`（aes-gcm 只给 128/256 起别名，无新依赖）。

/// `__wjs2_pss_sign(hash, saltLen, privDer, data)` → 签名（RSA-PSS，salt 随机）。
pub unsafe extern "C" fn pss_sign(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 || !frame.arg(1).is_number() {
        report_error(&mut cx, "TypeError: RSA-PSS sign needs hash, saltLength, key and data");
        return false;
    }
    let hash = value_to_string(&mut cx, frame.arg(0));
    let salt = frame.arg(1).to_number();
    if !salt.is_finite() || salt < 0.0 || salt > 512.0 || salt.fract() != 0.0 {
        report_error(&mut cx, "OperationError: bad RSA-PSS saltLength");
        return false;
    }
    let (Some(der), Some(data)) = (
        view_bytes(&mut cx, frame.arg(2), "RSA private key"),
        view_bytes(&mut cx, frame.arg(3), "RSA data"),
    ) else {
        return false;
    };
    let priv_key = match rsa_priv_from_der(&der) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let out: Result<Vec<u8>, String> = rsa_hash_dispatch!(hash.as_str(), D, {
        use rsa::signature::{RandomizedSigner as _, SignatureEncoding as _};
        let sk = rsa::pss::SigningKey::<D>::new_with_salt_len(priv_key, salt as usize);
        sk.try_sign_with_rng(&mut SystemRng, &data)
            .map(|s| s.to_vec())
            .map_err(|e| format!("OperationError: RSA-PSS sign failed: {e}"))
    });
    match out {
        Ok(sig) => set_rval_bytes(&mut cx, &frame, &sig),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs2_pss_verify(hash, saltLen, pubDer, sig, data)` → boolean。
pub unsafe extern "C" fn pss_verify(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 5 || !frame.arg(1).is_number() {
        report_error(&mut cx, "TypeError: RSA-PSS verify needs hash, saltLength, key, signature and data");
        return false;
    }
    let hash = value_to_string(&mut cx, frame.arg(0));
    let salt = frame.arg(1).to_number();
    if !salt.is_finite() || salt < 0.0 || salt > 512.0 || salt.fract() != 0.0 {
        report_error(&mut cx, "OperationError: bad RSA-PSS saltLength");
        return false;
    }
    let (Some(der), Some(sig), Some(data)) = (
        view_bytes(&mut cx, frame.arg(2), "RSA public key"),
        view_bytes(&mut cx, frame.arg(3), "RSA signature"),
        view_bytes(&mut cx, frame.arg(4), "RSA data"),
    ) else {
        return false;
    };
    let pub_key = match rsa_pub_from_der(&der) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let out: Result<bool, String> = rsa_hash_dispatch!(hash.as_str(), D, {
        use rsa::signature::Verifier as _;
        let vk = rsa::pss::VerifyingKey::<D>::new_with_salt_len(pub_key, salt as usize);
        match rsa::pss::Signature::try_from(sig.as_slice()) {
            Ok(s) => Ok(vk.verify(&data, &s).is_ok()),
            Err(_) => Err("OperationError: bad RSA-PSS signature length".to_string()),
        }
    });
    match out {
        Ok(ok) => {
            frame.set_rval(mozjs::jsval::BooleanValue(ok));
            true
        }
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// RSA-PSS 手工验签（RFC 8017 §8.1.2/9.1.2 EMSA-PSS，OpenSSL 兼容口径；
/// salt 长按证书参数，DB 前导零无 ≥8 强制——随 RFC 8017）。
pub(crate) fn rsa_pss_verify_manual(
    pub_key: &rsa::RsaPublicKey,
    hash: &str,
    mgf_hash: &str,
    salt_len: usize,
    m_hash: &[u8],
    sig: &[u8],
) -> bool {
    use rsa::traits::PublicKeyParts as _;
    let n = pub_key.n();
    let e = pub_key.e();
    let mod_bits = n.bits() as usize;
    let em_bits = mod_bits - 1;
    let em_len = em_bits.div_ceil(8);
    let h_len = m_hash.len();
    if em_len < h_len + salt_len + 2 || sig.len() != em_len {
        return false;
    }
    let s = rsa::BigUint::from_bytes_be(sig);
    if s >= *n {
        return false;
    }
    let em_int = s.modpow(e, n).to_bytes_be();
    let mut em = vec![0u8; em_len - em_int.len()];
    em.extend_from_slice(&em_int);
    // 左侧 8emLen - emBits 位必须为零；末字节 0xbc
    let top_bits = em_len * 8 - em_bits;
    if top_bits > 0 && (em[0] >> (8 - top_bits)) != 0 {
        return false;
    }
    if em[em_len - 1] != 0xbc {
        return false;
    }
    let db_len = em_len - h_len - 1;
    let masked_db = &em[..db_len];
    let h = &em[db_len..db_len + h_len];
    let Ok(db_mask) = mgf1_with(mgf_hash, h, db_len) else {
        return false;
    };
    let mut db: Vec<u8> = masked_db.iter().zip(db_mask.iter()).map(|(a, b)| a ^ b).collect();
    if top_bits > 0 {
        db[0] &= 0xff >> top_bits;
    }
    if db_len < salt_len + 1 {
        return false;
    }
    let ps_len = db_len - salt_len - 1;
    if db[..ps_len].iter().any(|&b| b != 0) || db[ps_len] != 0x01 {
        return false;
    }
    let salt = &db[ps_len + 1..];
    let mut m_prime = vec![0u8; 8];
    m_prime.extend_from_slice(m_hash);
    m_prime.extend_from_slice(salt);
    match x509_digest(hash, &m_prime) {
        Ok(h_prime) => h_prime == h,
        Err(_) => false,
    }
}

/// RSA PKCS#1 v1.5 手工验签：`em = sig^e mod n` 须为
/// `00 01 FF×(≥8) 00 ‖ DigestInfo ‖ H`（OpenSSL 口径：FF 非定长但 ≥8）。
pub(crate) fn rsa_v15_verify_manual(pub_key: &rsa::RsaPublicKey, prefix: &[u8], digest: &[u8], sig: &[u8]) -> bool {
    use rsa::traits::PublicKeyParts as _;
    let n = pub_key.n();
    let e = pub_key.e();    let k = (n.bits() as usize).div_ceil(8);
    if sig.len() != k {
        return false;
    }
    let s = rsa::BigUint::from_bytes_be(sig);
    if s >= *n {
        return false;
    }
    let em = s.modpow(e, n).to_bytes_be();
    let mut full = vec![0u8; k - em.len()];
    full.extend_from_slice(&em);
    // EMSA 下界：2 + 8 + 1 + T；T = prefix + digest
    if full.len() < 2 + 8 + 1 + prefix.len() + digest.len() {
        return false;
    }
    if full[0] != 0x00 || full[1] != 0x01 {
        return false;
    }
    let rest = &full[2..];
    let ff = rest.iter().take_while(|&&b| b == 0xFF).count();
    if ff < 8 || rest[ff] != 0x00 {
        return false;
    }
    let mut expect = Vec::with_capacity(prefix.len() + digest.len());
    expect.extend_from_slice(prefix);
    expect.extend_from_slice(digest);
    rest[ff + 1..].iter().eq(expect.iter())
}
