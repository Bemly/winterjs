//! crypto 椭圆曲线面（P-256/384/521 + secp256k1：ECDH/ECDSA/JWK）。

use super::common::*;

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;
use crate::jsapi_glue::{report_error, value_to_string, view_bytes, wrap_cx, Frame};

/// `__wjs_ec_generate(curve)` → PKCS#8 DER 私钥（熵源 `getrandom`，失败即 `OperationError`）。
pub unsafe extern "C" fn ec_generate(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: EC generate needs a curve");
        return false;
    }
    let curve = value_to_string(&mut cx, frame.arg(0));
    let Some(size) = curve_size(&curve) else {
        report_error(&mut cx, &format!("NotSupportedError: unsupported curve '{curve}' (P-256/384/521/secp256k1)"));
        return false;
    };
    // 极小概率越界（随机标量 ≥ 阶）即重试，而非报错。
    for _ in 0..8 {
        let mut raw = vec![0u8; size];
        if getrandom::fill(&mut raw).is_err() {
            report_error(&mut cx, "OperationError: cannot get random values");
            return false;
        }
        let out: Result<Vec<u8>, String> = with_curve!(curve.as_str(), |C, Secret, Public, Signing, Verifying, Sig, K| {
            use K::elliptic_curve::pkcs8::EncodePrivateKey as _;
            // C 仅作 `FieldBytes::<C>` 类型参（值位置用不上，`#[allow(dead_code)]` 在宏臂上）。
            K::elliptic_curve::FieldBytes::<C>::try_from(raw.as_slice())
                .map_err(|_| String::new())
                .and_then(|fb| Secret::from_bytes(&fb).map_err(|_| String::new()))
                .and_then(|sk| sk.to_pkcs8_der().map_err(|_| String::new()))
                .map(|d| d.as_bytes().to_vec())
        });
        match out {
            Ok(der) => {
                tracing::debug!(target: "winterjs::crypto", curve = curve.as_str(), "EC key generated");
                return set_rval_bytes(&mut cx, &frame, &der);
            }
            Err(e) if e.is_empty() => continue,
            Err(e) => {
                report_error(&mut cx, &e);
                return false;
            }
        }
    }
    report_error(&mut cx, "OperationError: EC key generation failed");
    false
}

/// `__wjs_ec_public(curve, privDer)` → SPKI DER 公钥。
pub unsafe extern "C" fn ec_public(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: EC public needs curve and private key");
        return false;
    }
    let curve = value_to_string(&mut cx, frame.arg(0));
    let Some(der) = view_bytes(&mut cx, frame.arg(1), "EC private key") else {
        return false;
    };
    let out: Result<Vec<u8>, String> = with_curve!(curve.as_str(), |C, Secret, Public, Signing, Verifying, Sig, K| {
        use K::elliptic_curve::pkcs8::{DecodePrivateKey as _, EncodePublicKey as _};
        Secret::from_pkcs8_der(&der)
            .map_err(|_| "DataError: bad EC private key (PKCS#8)".to_string())
            .and_then(|sk: Secret| {
                Public::from_secret_scalar(&sk.to_nonzero_scalar())
                    .to_public_key_der()
                    .map_err(|e| format!("OperationError: EC public export failed: {e}"))
                    .map(|d| d.as_bytes().to_vec())
            })
    });
    match out {
        Ok(spki) => set_rval_bytes(&mut cx, &frame, &spki),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_ecdsa_sign(curve, hash, privDer, data)` → 裸 `r‖s` 签名（WebCrypto 口径，非 DER）。
pub unsafe extern "C" fn ecdsa_sign(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 {
        report_error(&mut cx, "TypeError: ECDSA sign needs curve, hash, key and data");
        return false;
    }
    let curve = value_to_string(&mut cx, frame.arg(0));
    let hash = value_to_string(&mut cx, frame.arg(1));
    let (Some(der), Some(data)) = (
        view_bytes(&mut cx, frame.arg(2), "ECDSA private key"),
        view_bytes(&mut cx, frame.arg(3), "ECDSA data"),
    ) else {
        return false;
    };
    let digest = match ec_hash(&hash, &data) {
        Ok(h) => h,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let out: Result<Vec<u8>, String> = with_curve!(curve.as_str(), |C, Secret, Public, Signing, Verifying, Sig, K| {
        // IIFE：`?`/early-return 作用于闭包（外层函数返 bool，`?` 直写即 E0277）。
        (|| -> Result<Vec<u8>, String> {
            use K::ecdsa::signature::hazmat::PrehashSigner as _;
            use K::elliptic_curve::pkcs8::DecodePrivateKey as _;
            let sk = Secret::from_pkcs8_der(&der)
                .map_err(|_| "DataError: bad ECDSA private key (PKCS#8)".to_string())?;
            let signer = Signing::from_bytes(&sk.to_bytes())
                .map_err(|_| "DataError: bad ECDSA private key".to_string())?;
            // `sign_prehash` 有裸/ DER 双实现（`Signature` vs `der::Signature`），结果类型注解消歧。
            let sig: Sig = signer
                .sign_prehash(&digest)
                .map_err(|_| "OperationError: ECDSA sign failed".to_string())?;
            Ok(sig.to_bytes().to_vec())
        })()
    });
    match out {
        Ok(sig) => set_rval_bytes(&mut cx, &frame, &sig),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_ecdsa_verify(curve, hash, pubDer, sigRaw, data)` → boolean（裸 `r‖s` 口径）。
pub unsafe extern "C" fn ecdsa_verify(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 5 {
        report_error(&mut cx, "TypeError: ECDSA verify needs curve, hash, key, signature and data");
        return false;
    }
    let curve = value_to_string(&mut cx, frame.arg(0));
    let hash = value_to_string(&mut cx, frame.arg(1));
    let (Some(der), Some(sig), Some(data)) = (
        view_bytes(&mut cx, frame.arg(2), "ECDSA public key"),
        view_bytes(&mut cx, frame.arg(3), "ECDSA signature"),
        view_bytes(&mut cx, frame.arg(4), "ECDSA data"),
    ) else {
        return false;
    };
    let digest = match ec_hash(&hash, &data) {
        Ok(h) => h,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let out: Result<bool, String> = with_curve!(curve.as_str(), |C, Secret, Public, Signing, Verifying, Sig, K| {
        (|| -> Result<bool, String> {
            use K::ecdsa::signature::hazmat::PrehashVerifier as _;
            use K::elliptic_curve::pkcs8::DecodePublicKey as _;
            let pk = Public::from_public_key_der(&der)
                .map_err(|_| "DataError: bad ECDSA public key (SPKI)".to_string())?;
            let vk = Verifying::from_sec1_bytes(&pk.to_sec1_bytes())
                .map_err(|_| "DataError: bad ECDSA public key".to_string())?;
            let sig = Sig::try_from(sig.as_slice())
                .map_err(|_| "OperationError: bad ECDSA signature length".to_string())?;
            // 高 S 归一化（OpenSSL 接受可锻造签名；k256 验签拒 high-S，Node 同兼容）。
            let sig = sig.normalize_s();
            Ok(vk.verify_prehash(&digest, &sig).is_ok())
        })()
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

/// `__wjs_ecdh_derive(curve, privDer, pubDer)` → 原始共享秘密（定长：32/48/66）。
pub unsafe extern "C" fn ecdh_derive(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: ECDH derive needs curve, private and public keys");
        return false;
    }
    let curve = value_to_string(&mut cx, frame.arg(0));
    let (Some(priv_der), Some(pub_der)) = (
        view_bytes(&mut cx, frame.arg(1), "ECDH private key"),
        view_bytes(&mut cx, frame.arg(2), "ECDH public key"),
    ) else {
        return false;
    };
    let out: Result<Vec<u8>, String> = with_curve!(curve.as_str(), |C, Secret, Public, Signing, Verifying, Sig, K| {
        (|| -> Result<Vec<u8>, String> {
            use K::elliptic_curve::pkcs8::{DecodePrivateKey as _, DecodePublicKey as _};
            let sk = Secret::from_pkcs8_der(&priv_der)
                .map_err(|_| "DataError: bad ECDH private key (PKCS#8)".to_string())?;
            let pk = Public::from_public_key_der(&pub_der)
                .map_err(|_| "DataError: bad ECDH public key (SPKI)".to_string())?;
            let shared = K::elliptic_curve::ecdh::diffie_hellman(sk.to_nonzero_scalar(), pk.as_affine());
            Ok(shared.raw_secret_bytes().as_slice().to_vec())
        })()
    });
    match out {
        Ok(secret) => set_rval_bytes(&mut cx, &frame, &secret),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_ec_jwk(curve, privDer, pubDer)` → JWK 坐标 JSON（`{x,y,d?}`，base64url 定长）。
pub unsafe extern "C" fn ec_jwk(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: EC JWK needs curve, private and public keys");
        return false;
    }
    let curve = value_to_string(&mut cx, frame.arg(0));
    let (Some(priv_der), Some(pub_der)) = (
        view_bytes(&mut cx, frame.arg(1), "EC private key"),
        view_bytes(&mut cx, frame.arg(2), "EC public key"),
    ) else {
        return false;
    };
    let Some(size) = curve_size(&curve) else {
        report_error(&mut cx, &format!("NotSupportedError: unsupported curve '{curve}'"));
        return false;
    };
    let out: Result<String, String> = with_curve!(curve.as_str(), |C, Secret, Public, Signing, Verifying, Sig, K| {
        (|| -> Result<String, String> {
            use K::elliptic_curve::pkcs8::{DecodePrivateKey as _, DecodePublicKey as _};
            use K::elliptic_curve::sec1::ToSec1Point as _;
            let sk = Secret::from_pkcs8_der(&priv_der)
                .map_err(|_| "DataError: bad EC private key (PKCS#8)".to_string())?;
            let pk = Public::from_public_key_der(&pub_der)
                .map_err(|_| "DataError: bad EC public key (SPKI)".to_string())?;
            let point = pk.to_sec1_point(false);
            let (Some(x), Some(y)) = (point.x(), point.y()) else {
                return Err("DataError: bad EC public key (no coordinates)".to_string());
            };
            Ok(serde_json::json!({
                "x": b64url(&fixed_pad(x.as_slice(), size)?),
                "y": b64url(&fixed_pad(y.as_slice(), size)?),
                "d": b64url(&fixed_pad(sk.to_bytes().as_slice(), size)?),
            })
            .to_string())
        })()
    });
    match out {
        Ok(json) => {
            rooted!(&in(cx) let mut v = UndefinedValue());
            json.to_jsval(&mut cx, v.handle_mut());
            frame.set_rval(v.get());
            true
        }
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_ec_jwk_pub(curve, pubDer)` → 公钥坐标 JSON（`{x,y}`；非导出私钥时用）。
pub unsafe extern "C" fn ec_jwk_pub(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: EC JWK needs curve and public key");
        return false;
    }
    let curve = value_to_string(&mut cx, frame.arg(0));
    let Some(pub_der) = view_bytes(&mut cx, frame.arg(1), "EC public key") else {
        return false;
    };
    let Some(size) = curve_size(&curve) else {
        report_error(&mut cx, &format!("NotSupportedError: unsupported curve '{curve}'"));
        return false;
    };
    let out: Result<String, String> = with_curve!(curve.as_str(), |C, Secret, Public, Signing, Verifying, Sig, K| {
        (|| -> Result<String, String> {
            use K::elliptic_curve::pkcs8::DecodePublicKey as _;
            use K::elliptic_curve::sec1::ToSec1Point as _;
            let pk = Public::from_public_key_der(&pub_der)
                .map_err(|_| "DataError: bad EC public key (SPKI)".to_string())?;
            let point = pk.to_sec1_point(false);
            let (Some(x), Some(y)) = (point.x(), point.y()) else {
                return Err("DataError: bad EC public key (no coordinates)".to_string());
            };
            Ok(serde_json::json!({
                "x": b64url(&fixed_pad(x.as_slice(), size)?),
                "y": b64url(&fixed_pad(y.as_slice(), size)?),
            })
            .to_string())
        })()
    });
    match out {
        Ok(json) => {
            rooted!(&in(cx) let mut v = UndefinedValue());
            json.to_jsval(&mut cx, v.handle_mut());
            frame.set_rval(v.get());
            true
        }
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_ec_import_priv(curve, dU8)` → PKCS#8 DER（JWK `d` 进）。
pub unsafe extern "C" fn ec_import_priv(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: EC import needs curve and key");
        return false;
    }
    let curve = value_to_string(&mut cx, frame.arg(0));
    let Some(d) = view_bytes(&mut cx, frame.arg(1), "EC key") else {
        return false;
    };
    let Some(size) = curve_size(&curve) else {
        report_error(&mut cx, &format!("NotSupportedError: unsupported curve '{curve}'"));
        return false;
    };
    if d.len() != size {
        report_error(&mut cx, "DataError: bad EC JWK (d length)");
        return false;
    }
    let out: Result<Vec<u8>, String> = with_curve!(curve.as_str(), |C, Secret, Public, Signing, Verifying, Sig, K| {
        use K::elliptic_curve::pkcs8::EncodePrivateKey as _;
        K::elliptic_curve::FieldBytes::<C>::try_from(d.as_slice())
            .map_err(|_| "DataError: bad EC JWK (d)".to_string())
            .and_then(|fb| Secret::from_bytes(&fb).map_err(|_| "DataError: bad EC JWK (d)".to_string()))
            .and_then(|sk| {
                sk.to_pkcs8_der()
                    .map_err(|e| format!("OperationError: EC import failed: {e}"))
                    .map(|doc| doc.as_bytes().to_vec())
            })
    });
    match out {
        Ok(der) => set_rval_bytes(&mut cx, &frame, &der),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// SPKI/PKCS#8 算法参数 OID → 曲线名（纯函数，单测覆盖；未知/非 EC 即 ""）。
/// 注意是 parameters 里的曲线 OID，不是 algorithm 本身（后者恒为 id-ecPublicKey）。
pub(crate) fn ec_curve_name(der: &[u8]) -> &'static str {
    let params = spki::SubjectPublicKeyInfoRef::try_from(der)
        .ok()
        .and_then(|s| s.algorithm.parameters)
        .or_else(|| {
            pkcs8::PrivateKeyInfoRef::try_from(der)
                .ok()
                .and_then(|p| p.algorithm.parameters)
        });
    let oid = params
        .and_then(|any| any.decode_as::<der::asn1::ObjectIdentifier>().ok())
        .map(|o| o.to_string());
    match oid.as_deref() {
        Some("1.2.840.10045.3.1.7") => "P-256",
        Some("1.3.132.0.34") => "P-384",
        Some("1.3.132.0.35") => "P-521",
        Some("1.3.132.0.10") => "secp256k1",
        _ => "",
    }
}

/// `__wjs_ec_guess_curve(der)` → 曲线名（SPKI/PKCS#8 的算法 OID 直判；
/// 试解循环靠坐标长度会把 secp256k1 误判成 P-256（同 32 字节），必须看 OID）。
/// 未知/非 EC 即空串（调用方继续试别的类型）。
pub unsafe extern "C" fn ec_guess_curve(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: EC curve guess needs DER");
        return false;
    }
    let Some(der) = view_bytes(&mut cx, frame.arg(0), "EC key") else {
        return false;
    };
    let name = ec_curve_name(&der);
    use mozjs::conversions::ToJSValConvertible as _;
    name.to_jsval(&mut cx, frame.rval_mut());
    true
}

/// `__wjs_ec_import_pub(curve, xU8, yU8)` → SPKI DER（JWK `x/y` 或 raw 公钥进）。
pub unsafe extern "C" fn ec_import_pub(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: EC import needs curve, x and y");
        return false;
    }
    let curve = value_to_string(&mut cx, frame.arg(0));
    let (Some(x), Some(y)) = (
        view_bytes(&mut cx, frame.arg(1), "EC x"),
        view_bytes(&mut cx, frame.arg(2), "EC y"),
    ) else {
        return false;
    };
    let Some(size) = curve_size(&curve) else {
        report_error(&mut cx, &format!("NotSupportedError: unsupported curve '{curve}'"));
        return false;
    };
    if x.len() != size || y.len() != size {
        report_error(&mut cx, "DataError: bad EC JWK (x/y length)");
        return false;
    }
    let out: Result<Vec<u8>, String> = with_curve!(curve.as_str(), |C, Secret, Public, Signing, Verifying, Sig, K| {
        use K::elliptic_curve::pkcs8::EncodePublicKey as _;
        let mut prefixed = Vec::with_capacity(1 + 2 * size);
        prefixed.push(0x04);
        prefixed.extend_from_slice(&x);
        prefixed.extend_from_slice(&y);
        Public::from_sec1_bytes(&prefixed)
            .map_err(|_| "DataError: bad EC JWK (point not on curve)".to_string())
            .and_then(|pk| {
                pk.to_public_key_der()
                    .map_err(|e| format!("OperationError: EC import failed: {e}"))
                    .map(|d| d.as_bytes().to_vec())
            })
    });
    match out {
        Ok(der) => set_rval_bytes(&mut cx, &frame, &der),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_ec_import_compressed(curve, sec1)` → SPKI DER（10f crypto五轮：
/// 压缩/混合 SEC1 点导入——轮子内解压 + 上曲线校验；非法即 `DataError`。
/// UNSAFE-BOUNDARY: 前置 = 同文件既有 ec 系 natives（`wrap_cx` + `Frame::from_raw`
/// 边界块；`view_bytes` 越界断言）；覆盖 = `tests/node/crypto.rs`
/// `phase10f_crypto_raw_seed_parity` 的 `r5-ec-compressed-*` 行（正常/坏点/错长）。
pub unsafe extern "C" fn ec_import_compressed(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: EC compressed import needs curve and bytes");
        return false;
    }
    let curve = value_to_string(&mut cx, frame.arg(0));
    let Some(sec1) = view_bytes(&mut cx, frame.arg(1), "EC point") else {
        return false;
    };
    let out: Result<Vec<u8>, String> = with_curve!(curve.as_str(), |C, Secret, Public, Signing, Verifying, Sig, K| {
        use K::elliptic_curve::pkcs8::EncodePublicKey as _;
        // 注：`PublicKey::from_sec1_bytes` 为固有方法（含压缩解压），无需 trait 导入。
        Public::from_sec1_bytes(&sec1)
            .map_err(|_| "DataError: bad EC point (not on curve)".to_string())
            .and_then(|pk| {
                pk.to_public_key_der()
                    .map_err(|e| format!("OperationError: EC import failed: {e}"))
                    .map(|d| d.as_bytes().to_vec())
            })
    });
    match out {
        Ok(der) => set_rval_bytes(&mut cx, &frame, &der),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}
