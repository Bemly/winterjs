//! napi 取值（get_value_* 系 JT→C 转换）。

use super::*;
use std::ffi::{c_char};
use crate::napi::sys::{napi_env, napi_status, napi_value};


// ── 值读族（M0 补：fixture add() 需 get_value_double；lazy-bind 缺符号即
// SIGSEGV——dyld 惰性绑定落到空桩，2026-09-13 实测，plan-napi §4 记档）────

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_value_double(
    env: napi_env,
    value: napi_value,
    result: *mut f64,
) -> napi_status {
    if value.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(value) };
    if !v.is_number() {
        return sys::napi_status_napi_number_expected;
    }
    // SAFETY：result 为 addon 提供的合法出参。
    unsafe { *result = v.to_number() };
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_value_int32(
    env: napi_env,
    value: napi_value,
    result: *mut i32,
) -> napi_status {
    if value.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(value) };
    if !v.is_number() {
        return sys::napi_status_napi_number_expected;
    }
    // SAFETY：同上（to_number 双 tag 兼容；截断为 N-API int32 语义）。
    unsafe { *result = v.to_number() as i32 };
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_value_uint32(
    env: napi_env,
    value: napi_value,
    result: *mut u32,
) -> napi_status {
    if value.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(value) };
    if !v.is_number() {
        return sys::napi_status_napi_number_expected;
    }
    // SAFETY：同上（mod 2^32 截断为 N-API 语义）。
    unsafe { *result = v.to_number() as u32 };
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_value_int64(
    env: napi_env,
    value: napi_value,
    result: *mut i64,
) -> napi_status {
    if value.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(value) };
    if !v.is_number() {
        return sys::napi_status_napi_number_expected;
    }
    // SAFETY：同上（截断为 N-API 语义）。
    unsafe { *result = v.to_number() as i64 };
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_value_bool(
    env: napi_env,
    value: napi_value,
    result: *mut bool,
) -> napi_status {
    if value.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(value) };
    if !v.is_boolean() {
        return sys::napi_status_napi_boolean_expected;
    }
    // SAFETY：同上。
    unsafe { *result = v.to_boolean() };
    NAPI_OK
}

/// # Safety
/// N-API 约定（buf==NULL → result=长度；bufsize 不足截断 + NUL）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_value_string_utf8(
    env: napi_env,
    value: napi_value,
    buf: *mut c_char,
    bufsize: usize,
    result: *mut usize,
) -> napi_status {
    if value.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(value) };
    if !v.is_string() {
        return sys::napi_status_napi_string_expected;
    }
    // SAFETY：value 已验为字符串；value_to_string 取其内容（safe glue）。
    let s = unsafe {
        let mut cx = cx_of(env);
        crate::jsapi_glue::value_to_string(&mut cx, v)
    };
    if buf.is_null() {
        if !result.is_null() {
            unsafe { *result = s.len() };
        }
        return NAPI_OK;
    }
    // 拷贝 min(len, bufsize-1) + NUL（Node 口径）。
    let cap = bufsize.saturating_sub(1);
    let n = cap.min(s.len());
    unsafe {
        std::ptr::copy_nonoverlapping(s.as_ptr().cast::<c_char>(), buf, n);
        *buf.add(n) = 0;
        if !result.is_null() {
            *result = n;
        }
    }
    NAPI_OK
}
