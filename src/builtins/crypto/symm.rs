//! crypto 对称面（AES-GCM/HMAC/随机数/Subtle digest）。

use super::common::*;
use mozjs::conversions::ToJSValConvertible as _;

use mozjs::jsapi::JSObject;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;
use mozjs::typedarray::{CreateWith, TypedArray, Uint8};
use crate::jsapi_glue::{report_error, value_to_string, view_bytes, wrap_cx, Frame};

pub unsafe extern "C" fn aesgcm_encrypt(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 {
        report_error(&mut cx, "TypeError: AES-GCM encrypt needs key, iv, aad and data");
        return false;
    }
    let (k, iv, aad, plain) = (frame.arg(0), frame.arg(1), frame.arg(2), frame.arg(3));
    let (Some(key), Some(iv), Some(aad), Some(plain)) = (
        view_bytes(&mut cx, k, "AES-GCM key"),
        view_bytes(&mut cx, iv, "AES-GCM iv"),
        opt_view_bytes(&mut cx, aad, "AES-GCM aad"),
        view_bytes(&mut cx, plain, "AES-GCM data"),
    ) else {
        return false;
    };
    if iv.len() != 12 {
        report_error(&mut cx, "OperationError: AES-GCM iv must be 12 bytes");
        return false;
    }
    let aad_ref = aad.as_deref().unwrap_or(&[]);
    match gcm_encrypt_raw(&key, &iv, aad_ref, &plain) {
        Ok(ct) => set_rval_bytes(&mut cx, &frame, &ct),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

fn encrypt_with<A>(key: &[u8], iv: &[u8], aad: &[u8], plain: &[u8]) -> Result<Vec<u8>, String>
where
    A: aes_gcm::aead::KeyInit + aes_gcm::aead::Aead,
{
    use aes_gcm::aead::Payload;
    let cipher = A::new_from_slice(key).map_err(|e| e.to_string())?;
    let nonce =
        aes_gcm::aead::Nonce::<A>::try_from(iv).map_err(|_| "bad nonce".to_string())?;
    cipher
        .encrypt(&nonce, Payload { msg: plain, aad })
        .map_err(|e| e.to_string())
}

/// `__wjs2_aesgcm_decrypt(keyU8, ivU8, aadU8?, dataU8)` → Uint8Array（认证失败抛 OperationError）。
pub unsafe extern "C" fn aesgcm_decrypt(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 {
        report_error(&mut cx, "TypeError: AES-GCM decrypt needs key, iv, aad and data");
        return false;
    }
    let (k, iv, aad, data) = (frame.arg(0), frame.arg(1), frame.arg(2), frame.arg(3));
    let (Some(key), Some(iv), Some(aad), Some(data)) = (
        view_bytes(&mut cx, k, "AES-GCM key"),
        view_bytes(&mut cx, iv, "AES-GCM iv"),
        opt_view_bytes(&mut cx, aad, "AES-GCM aad"),
        view_bytes(&mut cx, data, "AES-GCM data"),
    ) else {
        return false;
    };
    if iv.len() != 12 {
        report_error(&mut cx, "OperationError: AES-GCM iv must be 12 bytes");
        return false;
    }
    if key.len() != 16 && key.len() != 24 && key.len() != 32 {
        report_error(&mut cx, "OperationError: AES-GCM key must be 16, 24 or 32 bytes");
        return false;
    }
    let aad_ref = aad.as_deref().unwrap_or(&[]);
    match gcm_decrypt_raw(&key, &iv, aad_ref, &data) {
        Ok(pt) => set_rval_bytes(&mut cx, &frame, &pt),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

fn decrypt_with<A>(key: &[u8], iv: &[u8], aad: &[u8], data: &[u8]) -> Result<Vec<u8>, String>
where
    A: aes_gcm::aead::KeyInit + aes_gcm::aead::Aead,
{
    use aes_gcm::aead::Payload;
    let cipher = A::new_from_slice(key).map_err(|e| e.to_string())?;
    let nonce =
        aes_gcm::aead::Nonce::<A>::try_from(iv).map_err(|_| "bad nonce".to_string())?;
    cipher
        .decrypt(&nonce, Payload { msg: data, aad })
        .map_err(|e| e.to_string())
}

/// AES-GCM 12B 裸加密（`__wjs2_aesgcm_encrypt` 与 node 侧 `gcm_anyiv` 共用；
/// 错误文案维持 `__wjs2_aesgcm_*` 口径，调用方按需包装）。
pub(crate) fn gcm_encrypt_raw(
    key: &[u8],
    iv: &[u8],
    aad: &[u8],
    plain: &[u8],
) -> Result<Vec<u8>, String> {
    type Aes192Gcm = aes_gcm::AesGcm<aes::Aes192, aes_gcm::aead::consts::U12>;
    if key.len() == 16 {
        encrypt_with::<aes_gcm::Aes128Gcm>(key, iv, aad, plain)
    } else if key.len() == 24 {
        encrypt_with::<Aes192Gcm>(key, iv, aad, plain)
    } else if key.len() == 32 {
        encrypt_with::<aes_gcm::Aes256Gcm>(key, iv, aad, plain)
    } else {
        Err("OperationError: AES-GCM key must be 16, 24 or 32 bytes".to_string())
    }
    .map_err(|e| {
        if e.starts_with("OperationError") {
            e
        } else {
            format!("OperationError: AES-GCM encrypt failed: {e}")
        }
    })
}

/// AES-GCM 12B 裸解密（共用；失败文案与 `__wjs2_aesgcm_decrypt` 一致）。
pub(crate) fn gcm_decrypt_raw(
    key: &[u8],
    iv: &[u8],
    aad: &[u8],
    data: &[u8],
) -> Result<Vec<u8>, String> {
    type Aes192Gcm = aes_gcm::AesGcm<aes::Aes192, aes_gcm::aead::consts::U12>;
    if key.len() == 16 {
        decrypt_with::<aes_gcm::Aes128Gcm>(key, iv, aad, data)
    } else if key.len() == 24 {
        decrypt_with::<Aes192Gcm>(key, iv, aad, data)
    } else if key.len() == 32 {
        decrypt_with::<aes_gcm::Aes256Gcm>(key, iv, aad, data)
    } else {
        Err("bad key".to_string())
    }
    .map_err(|_| "OperationError: AES-GCM decrypt failed (bad key/iv/tag?)".to_string())
}

macro_rules! hmac_with {
    ($hash:ty, $key:expr, $data:expr) => {{
        use hmac::{Hmac, Mac as _, digest::KeyInit as _};
        let mut mac =
            Hmac::<$hash>::new_from_slice($key).map_err(|e| format!("OperationError: {e}"))?;
        mac.update($data);
        mac.finalize().into_bytes().to_vec()
    }};
}

fn hmac_bytes(hash: &str, key: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
    match hash {
        "SHA-1" => Ok(hmac_with!(sha1::Sha1, key, data)),
        "SHA-256" => Ok(hmac_with!(sha2::Sha256, key, data)),
        "SHA-384" => Ok(hmac_with!(sha2::Sha384, key, data)),
        "SHA-512" => Ok(hmac_with!(sha2::Sha512, key, data)),
        other => Err(format!("NotSupportedError: unsupported HMAC hash '{other}'")),
    }
}

/// `__wjs2_hmac_sign(hash, keyU8, dataU8)` → Uint8Array。
pub unsafe extern "C" fn hmac_sign(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: HMAC sign needs hash, key and data");
        return false;
    }
    let hash = value_to_string(&mut cx, frame.arg(0));
    let (Some(key), Some(data)) = (
        view_bytes(&mut cx, frame.arg(1), "HMAC key"),
        view_bytes(&mut cx, frame.arg(2), "HMAC data"),
    ) else {
        return false;
    };
    match hmac_bytes(&hash, &key, &data) {
        Ok(tag) => set_rval_bytes(&mut cx, &frame, &tag),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs2_hmac_verify(hash, keyU8, sigU8, dataU8)` → boolean。
pub unsafe extern "C" fn hmac_verify(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 {
        report_error(&mut cx, "TypeError: HMAC verify needs hash, key, signature and data");
        return false;
    }
    let hash = value_to_string(&mut cx, frame.arg(0));
    let (Some(key), Some(sig), Some(data)) = (
        view_bytes(&mut cx, frame.arg(1), "HMAC key"),
        view_bytes(&mut cx, frame.arg(2), "HMAC signature"),
        view_bytes(&mut cx, frame.arg(3), "HMAC data"),
    ) else {
        return false;
    };
    match hmac_bytes(&hash, &key, &data) {
        Ok(tag) => {
            // 定长比较（长度先行，内容恒时由 Vec 比较短路程度可忽略的侧信道面）
            let ok = tag.len() == sig.len()
                && tag.iter().zip(sig.iter()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0;
            frame.set_rval(mozjs::jsval::BooleanValue(ok));
            true
        }
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// 上限 64KiB（规范 QuotaExceededError）。
const MAX_BYTES: usize = 65536;

macro_rules! try_fill {
    ($cx:expr, $obj:expr, $Marker:ty, $Elem:ty, $SIZE:expr) => {{
        if let Ok(mut arr) = TypedArray::<$Marker, *mut JSObject>::from($obj) {
            if arr.is_shared() {
                report_error($cx, "TypeError: getRandomValues does not accept SharedArrayBuffer views yet");
                return false;
            }
            let detached = arr.as_slice_safe($cx.no_gc()).is_none();
            if detached {
                report_error($cx, "TypeError: getRandomValues view is detached");
                return false;
            }
            let n: usize = arr.len();
            let bytes = n.saturating_mul($SIZE);
            if bytes > MAX_BYTES {
                report_error($cx, "QuotaExceededError: getRandomValues quota is 64KiB");
                return false;
            }
            let mut raw = vec![0u8; bytes];
            if let Err(e) = getrandom::fill(&mut raw) {
                report_error($cx, &format!("OperationError: cannot get random values: {e}"));
                return false;
            }
            if let Some(slot) = arr.as_mut_slice_safe($cx.no_gc_mut()) {
                for (i, cell) in slot.iter_mut().enumerate() {
                    let chunk: [u8; $SIZE] = raw[i * $SIZE..i * $SIZE + $SIZE]
                        .try_into()
                        .unwrap_or([0u8; $SIZE]);
                    *cell = <$Elem>::from_ne_bytes(chunk);
                }
            }
            tracing::trace!(target: "winterjs2::crypto", bytes, "getRandomValues filled");
            return true;
        }
    }};
}

/// `__wjs2_fill_random(view)`：就地填充（prelude 原样返回 view）。
pub unsafe extern "C" fn fill_random(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: getRandomValues requires an argument");
        return false;
    }
    let v = frame.arg(0);
    if !v.is_object() {
        report_error(&mut cx, "TypeError: getRandomValues requires a typed array view");
        return false;
    }
    // SAFETY: is_object 已判定（to_object 为 safe API，见 §6 审计）
    let obj = v.to_object();
    // 浮点视图先判（必须抛，不可按整数填充）
    if TypedArray::<mozjs::typedarray::Float32, *mut JSObject>::from(obj).is_ok()
        || TypedArray::<mozjs::typedarray::Float64, *mut JSObject>::from(obj).is_ok()
    {
        report_error(&mut cx, "TypeError: getRandomValues requires an integer typed array");
        return false;
    }
    try_fill!(&mut cx, obj, mozjs::typedarray::Uint8, u8, 1);
    try_fill!(&mut cx, obj, mozjs::typedarray::Int8, i8, 1);
    try_fill!(&mut cx, obj, mozjs::typedarray::ClampedU8, u8, 1);
    try_fill!(&mut cx, obj, mozjs::typedarray::Uint16, u16, 2);
    try_fill!(&mut cx, obj, mozjs::typedarray::Int16, i16, 2);
    try_fill!(&mut cx, obj, mozjs::typedarray::Uint32, u32, 4);
    try_fill!(&mut cx, obj, mozjs::typedarray::Int32, i32, 4);
    let got = value_to_string(&mut cx, v);
    report_error(
        &mut cx,
        &format!("TypeError: getRandomValues needs an integer TypedArray, got {got}"),
    );
    false
}

/// `__wjs2_random_uuid()` → v4 字符串。
pub unsafe extern "C" fn random_uuid(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let s = uuid::Uuid::new_v4().hyphenated().to_string();
    rooted!(&in(cx) let mut v = UndefinedValue());
    s.to_jsval(&mut cx, v.handle_mut());
    frame.set_rval(v.get());
    true
}

/// `__wjs2_subtle_digest(alg, view)` → Uint8Array（SHA-1/256/384/512；`sha1`/`sha2` 轮子）。
/// prelude 包一层 async 即得规范的 Promise 返回（计算本身同步，无需事件循环改动）。
pub unsafe extern "C" fn subtle_digest(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: digest requires an algorithm and data");
        return false;
    }
    let alg = value_to_string(&mut cx, frame.arg(0));
    let data = frame.arg(1);
    let bytes = match view_bytes(&mut cx, data, "digest data") {
        Some(b) => b,
        None => return false,
    };
    let out: Vec<u8> = match alg.trim().to_ascii_lowercase().as_str() {
        "sha-1" => {
            use sha1::Digest as _;
            sha1::Sha1::digest(&bytes).to_vec()
        }
        "sha-256" => {
            use sha2::Digest as _;
            sha2::Sha256::digest(&bytes).to_vec()
        }
        "sha-384" => {
            use sha2::Digest as _;
            sha2::Sha384::digest(&bytes).to_vec()
        }
        "sha-512" => {
            use sha2::Digest as _;
            sha2::Sha512::digest(&bytes).to_vec()
        }
        other => {
            report_error(&mut cx, &format!("NotSupportedError: unsupported digest algorithm '{other}'"));
            return false;
        }
    };
    rooted!(&in(cx) let mut obj: *mut JSObject = std::ptr::null_mut());
    // SAFETY: realm 内创建；obj 为 rooted 出参；out 存活到调用返回（§6 审计：边界调用）
    let ok = unsafe {
        TypedArray::<Uint8, *mut JSObject>::create(&mut cx, CreateWith::Slice(&out), obj.handle_mut())
    };
    if ok.is_err() || obj.is_null() {
        report_error(&mut cx, "RangeError: cannot allocate digest output");
        return false;
    }
    frame.set_rval(mozjs::jsval::ObjectValue(obj.get()));
    true
}
