//! `napi_*` 函数族（M0 hello 子集 + 基础面；全集按 plan-napi §6 名单逐 M 补）。
//!
//! 统一形态：`#[unsafe(no_mangle)] pub unsafe extern "C" fn napi_xxx(env: napi_env, ...)`
//! ——`env` 即 `NapiEnv`（会话单例）指针 cast；JSAPI 调用经 `wrap_cx(env.cx)`。
//! 纪律：JS 对象落槽位前必须先 `rooted!`（建值与落槽之间可能 GC）；
//! reserved slots 私有值（GC 不可见）携带 addon 回调指针/`data`。

use std::ffi::{c_char, CStr};
use std::ptr::NonNull;

use mozjs::context::JSContext;

use crate::napi::env::NapiEnv;
use crate::napi::sys;
use crate::napi::sys::{napi_env, napi_status};

mod call;
mod coerce;
mod inspect;
mod lifecycle;
mod trampoline;
mod values;

pub use trampoline::*;
pub use values::*;


// ── 内部辅助 ─────────────────────────────────────────────────────────────

/// # Safety
/// `env` 必须是本 crate 发给 addon 的有效 `NapiEnv` 指针（会话内存续）。
pub(crate) unsafe fn e<'a>(env: napi_env) -> &'a mut NapiEnv {
    // SAFETY：调用方持有本 crate 发出的有效 env 指针（前置已记录）。
    unsafe { &mut *(env as *mut NapiEnv) }
}

/// JS 线程 wrapper（napi_* 只在 JS 线程调用，plan-napi §4）。
///
/// # Safety
/// 同上；cx raw 指针在会话存续期有效。
pub(crate) unsafe fn cx_of(env: napi_env) -> JSContext {
    // SAFETY：napi_* 仅 JS 线程调用，raw cx 会话存续期有效（模块头注）。
    unsafe { JSContext::from_ptr(NonNull::new_unchecked(e(env).cx)) }
}

/// C 字符串参数（len == `napi_auto_length`（SIZE_MAX）/ -1 时取 strlen）。
///
/// # Safety
/// `s` 在长度语义内必须是合法 UTF-8 可读内存。
pub(crate) unsafe fn cstr_of_len(s: *const c_char, len: usize) -> Result<String, napi_status> {
    if s.is_null() {
        return Err(sys::napi_status_napi_invalid_arg);
    }
    let bytes = unsafe {
        if len == usize::MAX {
            CStr::from_ptr(s).to_bytes()
        } else {
            std::slice::from_raw_parts(s.cast::<u8>(), len)
        }
    };
    String::from_utf8(bytes.to_vec()).map_err(|_| sys::napi_status_napi_generic_failure)
}

pub(crate) const NAPI_OK: napi_status = sys::napi_status_napi_ok;
pub(crate) const NAPI_INVALID_ARG: napi_status = sys::napi_status_napi_invalid_arg;
pub(crate) const NAPI_GENERIC_FAILURE: napi_status = sys::napi_status_napi_generic_failure;
