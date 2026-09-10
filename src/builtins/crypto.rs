//! `crypto`：getRandomValues（`getrandom`）+ randomUUID（`uuid`）。
//! 全 safe（读/写均经 `as_*_slice_safe` + `NoGC` 令牌）；BigInt 视图暂不支持。

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::jsapi::JSObject;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;
use mozjs::typedarray::{CreateWith, TypedArray, Uint8};

use crate::jsapi_glue::{report_error, wrap_cx, Frame};
use crate::jsapi_glue::value_to_string;

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
            tracing::trace!(target: "winterjs::crypto", bytes, "getRandomValues filled");
            return true;
        }
    }};
}

/// `__wjs_fill_random(view)`：就地填充（prelude 原样返回 view）。
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

/// `__wjs_random_uuid()` → v4 字符串。
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

/// `__wjs_subtle_digest(alg, view)` → Uint8Array（SHA-1/256/384/512；`sha1`/`sha2` 轮子）。
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
    let bytes = match super::encoding::view_bytes(&mut cx, data, "digest data") {
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
