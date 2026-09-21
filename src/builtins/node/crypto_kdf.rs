//! crypto kdf 域（pbkdf2/scrypt/hkdf/argon2；对齐 crypto.rs；纯搬移）。

use mozjs::jsval::JSVal;

use crate::jsapi_glue::{report_error, value_to_string, view_bytes, wrap_cx, Frame};
use super::crypto::set_rval_bytes;

// ── 9e-1d KDF（pbkdf2/scrypt/hkdf/argon2；全员树内轮子，零新增）──────────────

/// KDF 摘要分发（HMAC 系：sha1/sha256/sha384/sha512/md5；sha3 无 block-API，记档）。
macro_rules! kdf_hash_dispatch {
    ($hash:expr, $D:ident, $body:expr) => {{
        match $hash {
            "SHA-1" => {
                type $D = sha1::Sha1;
                $body
            }
            "SHA-256" => {
                type $D = sha2::Sha256;
                $body
            }
            "SHA-384" => {
                type $D = sha2::Sha384;
                $body
            }
            "SHA-512" => {
                type $D = sha2::Sha512;
                $body
            }
            "MD5" => {
                type $D = md5::Md5;
                $body
            }
            other => Err(format!("NotSupportedError: KDF hash '{other}' needs SHA-1/256/384/512/MD5")),
        }
    }};
}

/// `__wjs_kdf_pbkdf2(hashStr, passU8, saltU8, roundsNum, lenNum)` → 派生密钥。
pub unsafe extern "C" fn kdf_pbkdf2(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 5 {
        report_error(&mut cx, "TypeError: PBKDF2 needs hash, password, salt, rounds and length");
        return false;
    }
    let hash = value_to_string(&mut cx, frame.arg(0));
    let (Some(pass), Some(salt)) = (
        view_bytes(&mut cx, frame.arg(1), "PBKDF2 password"),
        view_bytes(&mut cx, frame.arg(2), "PBKDF2 salt"),
    ) else {
        return false;
    };
    if !frame.arg(3).is_number() || !frame.arg(4).is_number() {
        report_error(&mut cx, "TypeError: PBKDF2 rounds/length must be numbers");
        return false;
    }
    let rounds = frame.arg(3).to_number();
    let len = frame.arg(4).to_number();
    if !(1.0..=2147483647.0).contains(&rounds) {
        report_error(&mut cx, "ERR_OUT_OF_RANGE: PBKDF2 iterations out of range");
        return false;
    }
    if !(0.0..=1073741824.0).contains(&len) {
        report_error(&mut cx, "ERR_OUT_OF_RANGE: PBKDF2 key length out of range");
        return false;
    }
    let mut out = vec![0u8; len as usize];
    let r: Result<(), String> = kdf_hash_dispatch!(hash.as_str(), D, {
        pbkdf2::pbkdf2_hmac::<D>(&pass, &salt, rounds as u32, &mut out);
        Ok(())
    });
    match r {
        Ok(()) => set_rval_bytes(&mut cx, &frame, &out),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_kdf_scrypt(passU8, saltU8, nNum, rNum, pNum, lenNum, maxmemNum)` → 派生密钥。
pub unsafe extern "C" fn kdf_scrypt(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 7 {
        report_error(&mut cx, "TypeError: scrypt needs password, salt, N, r, p, length and maxmem");
        return false;
    }
    let (Some(pass), Some(salt)) = (
        view_bytes(&mut cx, frame.arg(0), "scrypt password"),
        view_bytes(&mut cx, frame.arg(1), "scrypt salt"),
    ) else {
        return false;
    };
    let nums: Option<Vec<f64>> = (2..7)
        .map(|i| {
            let v = frame.arg(i);
            if v.is_number() { Some(v.to_number()) } else { None }
        })
        .collect();
    let Some(nums) = nums else {
        report_error(&mut cx, "TypeError: scrypt N/r/p/length/maxmem must be numbers");
        return false;
    };
    let (n_f, r_f, p_f, len_f, maxmem_f) = (nums[0], nums[1], nums[2], nums[3], nums[4]);
    if n_f <= 1.0 || n_f.fract() != 0.0 || (n_f.log2().fract() != 0.0 && n_f > 1.0) {
        report_error(&mut cx, "ERR_CRYPTO_INVALID_SCRYPT_PARAMS: Invalid scrypt params");
        return false;
    }
    let (n, r, p) = (n_f as u64, r_f as u32, p_f as u32);
    // 内存上限（默认 32MiB，Node 同款；128·N·r·p 口径）
    let mem = (n as u128) * (r as u128) * (p as u128) * 128;
    if r == 0 || p == 0 || mem > maxmem_f as u128 {
        report_error(&mut cx, "ERR_CRYPTO_INVALID_SCRYPT_PARAMS: Invalid scrypt params");
        return false;
    }
    if !(1.0..=1073741824.0).contains(&len_f) {
        report_error(&mut cx, "ERR_OUT_OF_RANGE: scrypt key length out of range");
        return false;
    }
    let log_n = n_f.log2() as u8;
    let params = match scrypt::Params::new(log_n, r, p) {
        Ok(p) => p,
        Err(_) => {
            report_error(&mut cx, "ERR_CRYPTO_INVALID_SCRYPT_PARAMS: Invalid scrypt params");
            return false;
        }
    };
    let mut out = vec![0u8; len_f as usize];
    match scrypt::scrypt(&pass, &salt, &params, &mut out) {
        Ok(()) => set_rval_bytes(&mut cx, &frame, &out),
        Err(_) => {
            report_error(&mut cx, "ERR_CRYPTO_INVALID_SCRYPT_PARAMS: Invalid scrypt params");
            false
        }
    }
}

/// `__wjs_kdf_hkdf(hashStr, ikmU8, saltU8, infoU8, lenNum)` → OKM（空 salt 即零串，RFC 口径）。
pub unsafe extern "C" fn kdf_hkdf(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 5 {
        report_error(&mut cx, "TypeError: HKDF needs hash, ikm, salt, info and length");
        return false;
    }
    let hash = value_to_string(&mut cx, frame.arg(0));
    let (Some(ikm), Some(salt), Some(info)) = (
        view_bytes(&mut cx, frame.arg(1), "HKDF ikm"),
        view_bytes(&mut cx, frame.arg(2), "HKDF salt"),
        view_bytes(&mut cx, frame.arg(3), "HKDF info"),
    ) else {
        return false;
    };
    if !frame.arg(4).is_number() {
        report_error(&mut cx, "TypeError: HKDF length must be a number");
        return false;
    }
    let len = frame.arg(4).to_number();
    if !(0.0..=1073741824.0).contains(&len) {
        report_error(&mut cx, "ERR_OUT_OF_RANGE: HKDF length out of range");
        return false;
    }
    let mut out = vec![0u8; len as usize];
    let r: Result<(), String> = kdf_hash_dispatch!(hash.as_str(), D, {
        let hk = hkdf::Hkdf::<D>::new(if salt.is_empty() { None } else { Some(&salt) }, &ikm);
        match hk.expand(&info, &mut out) {
            Ok(()) => Ok(()),
            Err(e) => Err(format!("OperationError: HKDF expand failed: {e}")),
        }
    });
    match r {
        Ok(()) => set_rval_bytes(&mut cx, &frame, &out),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_kdf_argon2(algoStr, msgU8, nonceU8, secretU8, adU8, parNum, tagNum, memNum, passNum)` → tag。
pub unsafe extern "C" fn kdf_argon2(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 9 {
        report_error(&mut cx, "TypeError: Argon2 needs algorithm, message, nonce, secret, ad and params");
        return false;
    }
    let algo = value_to_string(&mut cx, frame.arg(0));
    let (Some(msg), Some(nonce), Some(secret), Some(ad)) = (
        view_bytes(&mut cx, frame.arg(1), "Argon2 message"),
        view_bytes(&mut cx, frame.arg(2), "Argon2 nonce"),
        view_bytes(&mut cx, frame.arg(3), "Argon2 secret"),
        view_bytes(&mut cx, frame.arg(4), "Argon2 associated data"),
    ) else {
        return false;
    };
    let nums: Option<Vec<f64>> = (5..9)
        .map(|i| {
            let v = frame.arg(i);
            if v.is_number() { Some(v.to_number()) } else { None }
        })
        .collect();
    let Some(nums) = nums else {
        report_error(&mut cx, "TypeError: Argon2 params must be numbers");
        return false;
    };
    let algorithm = match algo.as_str() {
        "argon2d" => argon2::Algorithm::Argon2d,
        "argon2i" => argon2::Algorithm::Argon2i,
        "argon2id" => argon2::Algorithm::Argon2id,
        _ => {
            report_error(&mut cx, "ERR_INVALID_ARG_VALUE: Argon2 algorithm must be argon2d/argon2i/argon2id");
            return false;
        }
    };
    let mut builder = argon2::ParamsBuilder::new();
    if !ad.is_empty() {
        match argon2::AssociatedData::new(&ad) {
            Ok(data) => {
                builder.data(data);
            }
            Err(e) => {
                report_error(&mut cx, &format!("ERR_OUT_OF_RANGE: bad Argon2 associatedData ({e})"));
                return false;
            }
        }
    }
    builder.m_cost(nums[2] as u32);
    builder.t_cost(nums[3] as u32);
    builder.p_cost(nums[0] as u32);
    builder.output_len(nums[1] as usize);
    let params = match builder.build() {
        Ok(p) => p,
        Err(e) => {
            report_error(&mut cx, &format!("ERR_OUT_OF_RANGE: bad Argon2 params ({e})"));
            return false;
        }
    };
    let ctx = if secret.is_empty() {
        argon2::Argon2::new(algorithm, argon2::Version::V0x13, params)
    } else {
        match argon2::Argon2::new_with_secret(&secret, algorithm, argon2::Version::V0x13, params) {
            Ok(c) => c,
            Err(e) => {
                report_error(&mut cx, &format!("ERR_OUT_OF_RANGE: bad Argon2 secret ({e})"));
                return false;
            }
        }
    };
    let mut out = vec![0u8; nums[1] as usize];
    match ctx.hash_password_into(&msg, &nonce, &mut out) {
        Ok(()) => set_rval_bytes(&mut cx, &frame, &out),
        Err(e) => {
            report_error(&mut cx, &format!("OperationError: Argon2 failed ({e})"));
            false
        }
    }
}

