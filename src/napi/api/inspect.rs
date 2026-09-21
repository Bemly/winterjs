//! napi 内省与抛错（typeof/throw/new_target/last-error）。

use super::*;
use std::ffi::{c_char, CString};
use crate::napi::sys::{napi_callback_info, napi_env, napi_status, napi_value};
use crate::napi::env::CbInfo;


/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_typeof(
    env: napi_env,
    value: napi_value,
    result: *mut sys::napi_valuetype,
) -> napi_status {
    if value.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let env_ref = unsafe { e(env) };
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：槽位读取；cx raw 在会话存续。
    unsafe {
        let v = env_ref.get(value);
        *result = if v.is_undefined() {
            sys::napi_valuetype_napi_undefined
        } else if v.is_null() {
            sys::napi_valuetype_napi_null
        } else if v.is_boolean() {
            sys::napi_valuetype_napi_boolean
        } else if v.is_number() {
            sys::napi_valuetype_napi_number
        } else if v.is_string() {
            sys::napi_valuetype_napi_string
        } else if v.is_object() {
            if mozjs::jsapi::JS_ObjectIsFunction(v.to_object()) {
                sys::napi_valuetype_napi_function
            } else if crate::napi::class::object_is_class(
                &mut cx,
                v.to_object(),
                &crate::napi::class::NAPI_EXTERNAL_CLASS,
            ) {
                // napi_create_external 产物（Node 同口径：typeof = external）。
                sys::napi_valuetype_napi_external
            } else {
                sys::napi_valuetype_napi_object
            }
        } else if v.is_symbol() {
            sys::napi_valuetype_napi_symbol
        } else if v.is_bigint() {
            sys::napi_valuetype_napi_bigint
        } else {
            sys::napi_valuetype_napi_object
        };
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（throw 后回调返回 NULL，trampoline 据此传播 pending exception）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_throw_error(
    env: napi_env,
    code: *const c_char,
    msg: *const c_char,
) -> napi_status {
    // SAFETY：throw_* 收 char*（js_native_api.h:400）；建 Error 值后置 pending。
    unsafe {
        let mut msg_v: napi_value = std::ptr::null_mut();
        if napi_create_string_utf8(env, msg, usize::MAX, &mut msg_v) != sys::napi_status_napi_ok {
            return sys::napi_status_napi_generic_failure;
        }
        let code_v: napi_value = if code.is_null() {
            std::ptr::null_mut()
        } else {
            let mut v: napi_value = std::ptr::null_mut();
            if napi_create_string_utf8(env, code, usize::MAX, &mut v) != sys::napi_status_napi_ok {
                return sys::napi_status_napi_generic_failure;
            }
            v
        };
        let mut out: napi_value = std::ptr::null_mut();
        let st = crate::napi::value::napi_create_error(env, code_v, msg_v, &mut out);
        if st != sys::napi_status_napi_ok || out.is_null() {
            return st;
        }
        crate::napi::value::napi_throw(env, out)
    }
}

/// # Safety
/// N-API 约定（cbinfo 由 trampoline 建立，回调期间有效）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_new_target(
    env: napi_env,
    cbinfo: napi_callback_info,
    result: *mut napi_value,
) -> napi_status {
    let _ = env;
    if cbinfo.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    // 非构造调用时 trampoline 存的是 undefined 槽（Node 同语义）。
    // SAFETY：cbinfo 由 trampoline 建立且在回调栈内有效。
    unsafe {
        *result = (&*(cbinfo as *const CbInfo)).new_target;
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_last_error_info(
    env: napi_env,
    result: *mut *const sys::napi_extended_error_info,
) -> napi_status {
    let env_ref = unsafe { e(env) };
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    // 消息缓冲随 env 会话存续（NapiEnv 在 RootedState 内地址稳定）；
    // 指针到下次调用前有效（Node 同语义）。
    env_ref
        .last_error
        .get_or_insert_with(|| CString::new("no error").unwrap_or_default());
    let msg_ptr = env_ref.last_error.as_deref().unwrap_or_default().as_ptr() as *mut c_char;
    env_ref.err_info.error_message = msg_ptr;
    // SAFETY：err_info 属于 env（会话存续期稳定），只读借出。
    unsafe {
        *result = &env_ref.err_info;
    }
    NAPI_OK
}
