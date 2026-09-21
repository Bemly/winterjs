//! crypto DSA 面（生成/签名验签/导出）。

use super::common::*;
use mozjs::conversions::ToJSValConvertible as _;

use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;
use crate::jsapi_glue::{report_error, value_to_string, view_bytes, wrap_cx, Frame};

// ── 9h-1 DSA（dsa 0.7 + hazmat；密钥信封 JSON `{p,q,g,x?,y}`，b64，定长不限）─
// PKCS#8/SPKI 编解码走 dsa 自带 Encode（DER 互通真 Node）；导入走 JS 侧 DER 解析
// （轮子无公开 Decode）；签名 deterministic（RFC6979）；验签走 prehash（全哈希档）。

/// 信封解码（`{p,q,g,x?,y?}` b64 → 验证密钥 + 私钥质（`need_x` 时缺 x 即错）。
/// 私钥信封常无 y（Node PKCS#8 省略公钥）：有 x 即 `y=g^x mod p` 补算。
fn dsa_envelope(env: &serde_json::Value, need_x: bool) -> Result<(dsa::VerifyingKey, Option<dsa::BoxedUint>), String> {
    use base64::Engine as _;
    let get = |k: &str| -> Result<Vec<u8>, String> {
        env.get(k)
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("DataError: bad DSA key (missing {k})"))
            .and_then(|s| {
                base64::engine::general_purpose::STANDARD
                    .decode(s)
                    .map_err(|_| format!("DataError: bad DSA key ({k} not base64)"))
            })
    };
    let bu = |b: Vec<u8>| dsa::BoxedUint::from_be_slice_vartime(&b);
    let comp = dsa::Components::from_components(bu(get("p")?), bu(get("q")?), bu(get("g")?))
        .map_err(|_| "DataError: bad DSA parameters".to_string())?;
    let x_raw: Option<Vec<u8>> = match env.get("x").and_then(|v| v.as_str()) {
        Some(s) => Some(base64::engine::general_purpose::STANDARD
            .decode(s)
            .map_err(|_| "DataError: bad DSA key (x not base64)".to_string())?),
        None if need_x => Some(get("x")?),
        None => None,
    };
    let y = match env.get("y").and_then(|v| v.as_str()) {
        Some(s) => bu(base64::engine::general_purpose::STANDARD
            .decode(s)
            .map_err(|_| "DataError: bad DSA key (y not base64)".to_string())?),
        None => match &x_raw {
            // 私钥信封常无 y（Node PKCS#8 省略公钥）：`y=g^x mod p` 补算。
            Some(x) => {
                let gb = rsa::BigUint::from_bytes_be(&comp.g().to_be_bytes());
                let xb = rsa::BigUint::from_bytes_be(x);
                let pb = rsa::BigUint::from_bytes_be(&comp.p().to_be_bytes());
                bu(gb.modpow(&xb, &pb).to_bytes_be())
            }
            None => return Err("DataError: bad DSA key (missing y)".to_string()),
        },
    };
    let vk = dsa::VerifyingKey::from_components(comp, y).map_err(|_| "DataError: bad DSA public key".to_string())?;
    let x = x_raw.map(bu);
    Ok((vk, x))
}

/// `__wjs_dsa_generate(lBits, nBits)` → 信封 JSON（`{p,q,g,x,y}` b64）。
pub unsafe extern "C" fn dsa_generate(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: DSA generate needs modulus and divisor lengths");
        return false;
    }
    let l = value_to_string(&mut cx, frame.arg(0)).parse::<u32>().unwrap_or(0);
    let n = value_to_string(&mut cx, frame.arg(1)).parse::<u32>().unwrap_or(0);
    // Node 兼容面保留 1024/160（上游 dsa 已按 SP 800-57 标记 deprecate，属预期告警，允许）。
    #[allow(deprecated)]
    let size = match (l, n) {
        (1024, 160) => dsa::KeySize::DSA_1024_160,
        (2048, 224) => dsa::KeySize::DSA_2048_224,
        (2048, 256) => dsa::KeySize::DSA_2048_256,
        (3072, 256) => dsa::KeySize::DSA_3072_256,
        _ => {
            report_error(&mut cx, "ERR_INVALID_ARG_VALUE: unsupported DSA size (1024/160, 2048/224, 2048/256, 3072/256)");
            return false;
        }
    };
    let mut rng = rand::rngs::SysRng;
    let out: Result<String, String> = (|| {
        use base64::Engine as _;
        let b64 = &base64::engine::general_purpose::STANDARD;
        let comp = dsa::Components::try_generate_from_rng_with_key_size(&mut rng, size)
            .map_err(|e| format!("OperationError: DSA parameter generation failed ({e:?})"))?;
        let sk = dsa::SigningKey::try_generate_from_rng_with_components(&mut rng, comp.clone())
            .map_err(|e| format!("OperationError: DSA key generation failed ({e:?})"))?;
        let b = |u: &dsa::BoxedUint| b64.encode(u.to_be_bytes());
        Ok(serde_json::json!({
            "p": b(comp.p()),
            "q": b(comp.q()),
            "g": b(comp.g()),
            "x": b(sk.x()),
            "y": b(sk.verifying_key().y()),
        })
        .to_string())
    })();
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

/// `__wjs_dsa_sign(hash, envJson, data)` → DER 签名（deterministic RFC6979）。
pub unsafe extern "C" fn dsa_sign(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: DSA sign needs hash, key and data");
        return false;
    }
    let hash = value_to_string(&mut cx, frame.arg(0));
    let (Some(env), Some(data)) = (
        serde_json::from_str::<serde_json::Value>(&value_to_string(&mut cx, frame.arg(1))).ok(),
        view_bytes(&mut cx, frame.arg(2), "DSA data"),
    ) else {
        report_error(&mut cx, "TypeError: DSA sign needs a key envelope and data");
        return false;
    };
    let digest = match ec_hash(&hash, &data) {
        Ok(h) => h,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let out: Result<Vec<u8>, String> = (|| {
        use dsa::signature::SignatureEncoding as _;
        let (vk, x) = dsa_envelope(&env, true)?;
        let x = x.expect("need_x");
        let sk = dsa::SigningKey::from_components(vk, x).map_err(|_| "DataError: bad DSA private key".to_string())?;
        let sig = sk
            .sign_prehashed_rfc6979::<sha2::Sha256>(&digest)
            .map_err(|_| "OperationError: DSA sign failed".to_string())?;
        Ok(sig.to_vec())
    })();
    match out {
        Ok(der) => set_rval_bytes(&mut cx, &frame, &der),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_dsa_verify(hash, envJson, sigDer, data)` → boolean（prehash 全哈希档）。
pub unsafe extern "C" fn dsa_verify(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 {
        report_error(&mut cx, "TypeError: DSA verify needs hash, key, signature and data");
        return false;
    }
    let hash = value_to_string(&mut cx, frame.arg(0));
    let (Some(env), Some(sig), Some(data)) = (
        serde_json::from_str::<serde_json::Value>(&value_to_string(&mut cx, frame.arg(1))).ok(),
        view_bytes(&mut cx, frame.arg(2), "DSA signature"),
        view_bytes(&mut cx, frame.arg(3), "DSA data"),
    ) else {
        report_error(&mut cx, "TypeError: DSA verify needs a key envelope, signature and data");
        return false;
    };
    let digest = match ec_hash(&hash, &data) {
        Ok(h) => h,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let out: Result<bool, String> = (|| {
        use dsa::signature::hazmat::PrehashVerifier as _;
        let (vk, _) = dsa_envelope(&env, false)?;
        // 10f crypto二轮：签名形态错回 false，不抛（空签名套件点名；UNSAFE-BOUNDARY 同族）。
        let Ok(sig) = dsa::Signature::try_from(sig.as_slice()) else {
            return Ok(false);
        };
        Ok(vk.verify_prehash(&digest, &sig).is_ok())
    })();
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

/// `__wjs_dsa_export(envJson)` → JSON `{privDer?, pubDer}`（b64；PKCS#8/SPKI）。
pub unsafe extern "C" fn dsa_export(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: DSA export needs a key envelope");
        return false;
    }
    let Some(env) = serde_json::from_str::<serde_json::Value>(&value_to_string(&mut cx, frame.arg(0))).ok() else {
        report_error(&mut cx, "TypeError: DSA export needs a key envelope");
        return false;
    };
    let out: Result<String, String> = (|| {
        use base64::Engine as _;
        use dsa::pkcs8::{EncodePrivateKey as _, EncodePublicKey as _};
        let b64 = &base64::engine::general_purpose::STANDARD;
        let (vk, x) = dsa_envelope(&env, false)?;
        let pub_der = vk.to_public_key_der().map_err(|e| format!("OperationError: DSA export failed ({e})"))?;
        let mut o = serde_json::json!({ "pubDer": b64.encode(pub_der.as_bytes()) });
        if let Some(x) = x {
            let sk = dsa::SigningKey::from_components(vk, x).map_err(|_| "DataError: bad DSA private key".to_string())?;
            let priv_der = sk.to_pkcs8_der().map_err(|e| format!("OperationError: DSA export failed ({e})"))?;
            o["privDer"] = serde_json::Value::String(b64.encode(priv_der.as_bytes()));
        }
        Ok(o.to_string())
    })();
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
