//! 本体内存面（WinterJS2.memory/alloc/unsafe*）：分配器三件套。
//!
//! 安全形态（§6 safe-first；裸指针禁出 JS，见 4.40/4.68）：
//! 1. 只读可观测 `memory()`：rss（sysinfo 实测）+ 分配器种类 + 堆三数恒 0
//!    （跨引擎不可比，沿 `process.memoryUsage` 记档口径）。
//! 2. 受控分配 `alloc(size)`：GC 托管零填 Uint8Array（经 `jsapi_glue::uint8_array`）。
//! 3. 手动堆 `unsafeAlloc/size/read/write/free/list`：Rust 侧 `HashMap<id, Vec<u8>>`
//!    id 表托管，JS 只见 id（数字），不见裸指针；UAF/double-free/OOB 一律可读错。
//!    门控 `--allow-ffi`（与 `bun:ffi` 同门；后果开发者自负，用户已拍板三面全要）。
//! 全员经已有轮子（sysinfo 已在树内），零新增依赖。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::context::JSContext;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{report_error, uint8_array, view_bytes, wrap_cx, Frame};

/// 分配器种类（与 `src/alloc.rs` 双端一致；移动端 talc 回退）。
#[cfg(not(any(target_os = "android", target_os = "ohos")))]
pub const ALLOC_KIND: &str = "smmalloc";
#[cfg(any(target_os = "android", target_os = "ohos"))]
pub const ALLOC_KIND: &str = "talc";

/// 受控单次上限（64MB；smmalloc 上限约 8GiB，64MB 既防误触 OOM 又覆盖常规用）。
pub const MAX_ALLOC: usize = 64 << 20;
/// 手动堆单块上限（同上）。
pub const MAX_UNSAFE: usize = 64 << 20;

/// 手动堆表（id → 字节块；Mutex 够用，native 全在 JS 线程串行进）。
static UNSAFE_HEAP: std::sync::LazyLock<std::sync::Mutex<HashMap<u64, Vec<u8>>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// 尺寸校验（纯函数，单测覆盖）。
pub fn validate_len(n: f64, cap: usize) -> Result<usize, String> {
    if !n.is_finite() || n < 0.0 || n.fract() != 0.0 {
        return Err("size must be a non-negative integer".to_string());
    }
    let len = n as usize;
    if len > cap {
        return Err(format!("size exceeds cap ({cap} bytes)"));
    }
    Ok(len)
}

fn arg_len(cx: &mut JSContext, frame: &Frame, i: u32, what: &str, cap: usize) -> Option<usize> {
    if frame.argc() <= i || !frame.arg(i).is_number() {
        report_error(cx, &format!("TypeError: {what} requires a size"));
        return None;
    }
    match validate_len(frame.arg(i).to_number(), cap) {
        Ok(n) => Some(n),
        Err(msg) => {
            report_error(cx, &format!("TypeError: {msg}"));
            None
        }
    }
}

fn arg_id(cx: &mut JSContext, frame: &Frame, i: u32, what: &str) -> Option<u64> {
    if frame.argc() <= i || !frame.arg(i).is_number() {
        report_error(cx, &format!("TypeError: {what} requires an id"));
        return None;
    }
    let n = frame.arg(i).to_number();
    if !n.is_finite() || n < 0.0 || n.fract() != 0.0 {
        report_error(cx, &format!("TypeError: {what} requires an id"));
        return None;
    }
    Some(n as u64)
}

fn rss_bytes() -> u64 {
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    sysinfo::get_current_pid()
        .ok()
        .and_then(|pid| sys.process(pid))
        .map(|p| p.memory())
        .unwrap_or(0)
}

// ── natives ────────────────────────────────────────────────────────────────
// UNSAFE-BOUNDARY：全部 JSNative 入口经 `wrap_cx` + `Frame::from_raw`（结构性边界块）；
// 前置：调用方 realm 内 + 参数槽 rooted 后才分配；手动堆本身无 unsafe（纯 HashMap+Vec）；
// 覆盖：`tests/mem.rs`（正常/报错/边界）+ 黑盒 panic 路径用例。

/// `__wjs2_mem_info()` → JSON `{rss,heapTotal:0,heapUsed:0,external:0,allocator}`。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/mem.rs::mem_info_and_alloc`。
pub unsafe extern "C" fn mem_info(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let rss = rss_bytes();
    tracing::debug!(target: "winterjs2::mem", rss, allocator = ALLOC_KIND, "info");
    let text = serde_json::json!({
        "rss": rss,
        "heapTotal": 0,
        "heapUsed": 0,
        "external": 0,
        "arrayBuffers": 0,
        "allocator": ALLOC_KIND,
    })
    .to_string();
    frame.set_rval({
        rooted!(&in(cx) let mut out = UndefinedValue());
        text.to_jsval(&mut cx, out.handle_mut());
        out.get()
    });
    true
}

/// `__wjs2_mem_alloc(size)` → 零填 Uint8Array（GC 托管）。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/mem.rs::mem_info_and_alloc`。
pub unsafe extern "C" fn mem_alloc(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(len) = arg_len(&mut cx, &frame, 0, "mem alloc", MAX_ALLOC) else {
        return false;
    };
    tracing::debug!(target: "winterjs2::mem", len, "alloc");
    let bytes = vec![0u8; len];
    match uint8_array(&mut cx, &bytes) {
        Some(obj) => {
            frame.set_rval(mozjs::jsval::ObjectValue(obj));
            true
        }
        None => {
            report_error(&mut cx, "RangeError: cannot allocate Uint8Array");
            false
        }
    }
}

/// `__wjs2_mem_unsafe_alloc(size)` → id（`--allow-ffi` 门控）。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/mem.rs::mem_unsafe_heap`。
pub unsafe extern "C" fn mem_unsafe_alloc(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if let Err(msg) = crate::permissions::check_ffi() {
        report_error(&mut cx, &msg);
        return false;
    }
    let Some(len) = arg_len(&mut cx, &frame, 0, "mem unsafeAlloc", MAX_UNSAFE) else {
        return false;
    };
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    match UNSAFE_HEAP.lock() {
        Ok(mut t) => {
            t.insert(id, vec![0u8; len]);
            tracing::debug!(target: "winterjs2::mem", id, len, "unsafeAlloc");
            frame.set_rval(mozjs::jsval::DoubleValue(id as f64));
            true
        }
        Err(_) => {
            report_error(&mut cx, "WfsError: mem heap poisoned");
            false
        }
    }
}

/// `__wjs2_mem_unsafe_size(id)` → size。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/mem.rs::mem_unsafe_heap`。
pub unsafe extern "C" fn mem_unsafe_size(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if let Err(msg) = crate::permissions::check_ffi() {
        report_error(&mut cx, &msg);
        return false;
    }
    let Some(id) = arg_id(&mut cx, &frame, 0, "mem unsafeSize") else {
        return false;
    };
    match UNSAFE_HEAP.lock() {
        Ok(t) => match t.get(&id) {
            Some(b) => {
                frame.set_rval(mozjs::jsval::DoubleValue(b.len() as f64));
                true
            }
            None => {
                report_error(&mut cx, "WfsError: unknown mem id (use-after-free?)");
                false
            }
        },
        Err(_) => {
            report_error(&mut cx, "WfsError: mem heap poisoned");
            false
        }
    }
}

/// `__wjs2_mem_unsafe_write(id, offset, u8)`.
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/mem.rs::mem_unsafe_heap`。
pub unsafe extern "C" fn mem_unsafe_write(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if let Err(msg) = crate::permissions::check_ffi() {
        report_error(&mut cx, &msg);
        return false;
    }
    let Some(id) = arg_id(&mut cx, &frame, 0, "mem unsafeWrite") else {
        return false;
    };
    if frame.argc() <= 1 || !frame.arg(1).is_number() {
        report_error(&mut cx, "TypeError: mem unsafeWrite requires an offset");
        return false;
    }
    let off = frame.arg(1).to_number();
    if !off.is_finite() || off < 0.0 || off.fract() != 0.0 {
        report_error(&mut cx, "TypeError: mem unsafeWrite requires an offset");
        return false;
    }
    if frame.argc() <= 2 {
        report_error(&mut cx, "TypeError: mem unsafeWrite requires data");
        return false;
    }
    let Some(data) = view_bytes(&mut cx, frame.arg(2), "mem unsafeWrite") else {
        return false;
    };
    let off = off as usize;
    match UNSAFE_HEAP.lock() {
        Ok(mut t) => match t.get_mut(&id) {
            Some(slot) => {
                if off.saturating_add(data.len()) > slot.len() {
                    report_error(&mut cx, "RangeError: mem unsafeWrite out of bounds");
                    return false;
                }
                slot[off..off + data.len()].copy_from_slice(&data);
                frame.set_rval(UndefinedValue());
                true
            }
            None => {
                report_error(&mut cx, "WfsError: unknown mem id (use-after-free?)");
                false
            }
        },
        Err(_) => {
            report_error(&mut cx, "WfsError: mem heap poisoned");
            false
        }
    }
}

/// `__wjs2_mem_unsafe_read(id, offset, len)` → Uint8Array 拷贝。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/mem.rs::mem_unsafe_heap`。
pub unsafe extern "C" fn mem_unsafe_read(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if let Err(msg) = crate::permissions::check_ffi() {
        report_error(&mut cx, &msg);
        return false;
    }
    let Some(id) = arg_id(&mut cx, &frame, 0, "mem unsafeRead") else {
        return false;
    };
    if frame.argc() <= 2 || !frame.arg(1).is_number() || !frame.arg(2).is_number() {
        report_error(&mut cx, "TypeError: mem unsafeRead requires offset and length");
        return false;
    }
    let (off, len) = (frame.arg(1).to_number(), frame.arg(2).to_number());
    if !off.is_finite() || off < 0.0 || off.fract() != 0.0 || !len.is_finite() || len < 0.0 || len.fract() != 0.0 {
        report_error(&mut cx, "TypeError: mem unsafeRead requires offset and length");
        return false;
    }
    let (off, len) = (off as usize, len as usize);
    let chunk = match UNSAFE_HEAP.lock() {
        Ok(t) => match t.get(&id) {
            Some(slot) => {
                if off.saturating_add(len) > slot.len() {
                    report_error(&mut cx, "RangeError: mem unsafeRead out of bounds");
                    return false;
                }
                slot[off..off + len].to_vec()
            }
            None => {
                report_error(&mut cx, "WfsError: unknown mem id (use-after-free?)");
                return false;
            }
        },
        Err(_) => {
            report_error(&mut cx, "WfsError: mem heap poisoned");
            return false;
        }
    };
    match uint8_array(&mut cx, &chunk) {
        Some(obj) => {
            frame.set_rval(mozjs::jsval::ObjectValue(obj));
            true
        }
        None => {
            report_error(&mut cx, "RangeError: cannot allocate Uint8Array");
            false
        }
    }
}

/// `__wjs2_mem_unsafe_free(id)`（幂等：重复 free 即错，不静默吞）。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/mem.rs::mem_errors_boundary`。
pub unsafe extern "C" fn mem_unsafe_free(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if let Err(msg) = crate::permissions::check_ffi() {
        report_error(&mut cx, &msg);
        return false;
    }
    let Some(id) = arg_id(&mut cx, &frame, 0, "mem unsafeFree") else {
        return false;
    };
    match UNSAFE_HEAP.lock() {
        Ok(mut t) => {
            if t.remove(&id).is_some() {
                tracing::debug!(target: "winterjs2::mem", id, "unsafeFree");
                frame.set_rval(UndefinedValue());
                true
            } else {
                report_error(&mut cx, "WfsError: unknown mem id (double-free?)");
                false
            }
        }
        Err(_) => {
            report_error(&mut cx, "WfsError: mem heap poisoned");
            false
        }
    }
}

/// `__wjs2_mem_unsafe_list()` → id 数组 JSON。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/mem.rs::mem_errors_boundary`。
pub unsafe extern "C" fn mem_unsafe_list(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if let Err(msg) = crate::permissions::check_ffi() {
        report_error(&mut cx, &msg);
        return false;
    }
    let mut ids: Vec<u64> = match UNSAFE_HEAP.lock() {
        Ok(t) => t.keys().copied().collect(),
        Err(_) => {
            report_error(&mut cx, "WfsError: mem heap poisoned");
            return false;
        }
    };
    ids.sort_unstable();
    let text = serde_json::json!(ids).to_string();
    frame.set_rval({
        rooted!(&in(cx) let mut out = UndefinedValue());
        text.to_jsval(&mut cx, out.handle_mut());
        out.get()
    });
    true
}

#[cfg(test)]
mod tests {
    use super::{MAX_ALLOC, validate_len};

    #[test]
    fn mem_len_validation() {
        assert_eq!(validate_len(0.0, MAX_ALLOC), Ok(0));
        assert_eq!(validate_len(16.0, MAX_ALLOC), Ok(16));
        assert!(validate_len(-1.0, MAX_ALLOC).is_err());
        assert!(validate_len(1.5, MAX_ALLOC).is_err());
        assert!(validate_len(f64::NAN, MAX_ALLOC).is_err());
        assert!(validate_len((MAX_ALLOC + 1) as f64, MAX_ALLOC).is_err());
    }

    #[test]
    fn mem_alloc_kind_known() {
        assert!(["smmalloc", "talc"].contains(&super::ALLOC_KIND));
    }
}
