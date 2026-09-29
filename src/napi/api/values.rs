//! napi 值构造（函数/字符串/数字/单例/全局对象/cb_info）。

use super::*;
use std::ffi::{c_char, c_void, CString};
use crate::napi::sys::{napi_callback, napi_callback_info, napi_env, napi_status, napi_value};
use crate::jsapi_glue::call_three;
use crate::jsapi_glue::get_prop_value;
use mozjs::jsval::ObjectValue;
use mozjs::jsval::UndefinedValue;
use crate::napi::env::CbInfo;

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::context::JSContext;
use mozjs::jsapi::JSObject;
use mozjs::jsval::JSVal;
use mozjs::rooted;

// ── M0 函数族 ────────────────────────────────────────────────────────────

/// # Safety
/// N-API 约定（addon 传入的 env/result 指针合法）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_version(env: napi_env, result: *mut u32) -> napi_status {
    let _ = env;
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    // 实现/承诺面：Node-API 8（plan-napi §3；更高版本面逐 M 补齐后再升）。
    // SAFETY：result 为 addon 提供的合法出参。
    unsafe { *result = 8 };
    NAPI_OK
}

/// # Safety
/// N-API 约定；constructor（dlopen 栈内）调用，仅 JS 线程。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_module_register(module: *mut sys::napi_module) {
    crate::napi::env::stash_pending_module(module);
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_function(
    env: napi_env,
    utf8name: *const c_char,
    length: usize,
    cb: napi_callback,
    data: *mut c_void,
    result: *mut napi_value,
) -> napi_status {
    if result.is_null() || cb.is_none() {
        return NAPI_INVALID_ARG;
    }
    let name = match unsafe { cstr_of_len(utf8name, length) } {
        Ok(s) if !s.is_empty() => s,
        Ok(_) => "__napianonymous".to_string(),
        Err(st) => return st,
    };
    let Ok(cname) = CString::new(name) else {
        return NAPI_GENERIC_FAILURE;
    };
    let mut cx = unsafe { cx_of(env) };
    let env_ref = unsafe { e(env) };
    // SAFETY：cx 在 realm 内（addon 由 require 管线在 JS 执行栈上调入）。
    unsafe {
        // JSFUN_CONSTRUCTOR：Node 口径 addon 函数可 new（构造帧经同一 trampoline）
        let fun = mozjs::jsapi::js::NewFunctionWithReserved(
            cx.raw_cx(),
            Some(napi_trampoline),
            0,
            mozjs::jsapi::JSFUN_CONSTRUCTOR,
            cname.as_ptr(),
        );
        if fun.is_null() {
            env_ref.set_last_error("failed to create function");
            return NAPI_GENERIC_FAILURE;
        }
        let fobj = mozjs::jsapi::JS_GetFunctionObject(fun);
        rooted!(&in(cx) let froot: *mut JSObject = fobj);
        mozjs::jsapi::js::SetFunctionNativeReserved(
            froot.get(),
            0,
            &mozjs::jsval::PrivateValue(cb.unwrap() as *const c_void),
        );
        mozjs::jsapi::js::SetFunctionNativeReserved(
            froot.get(),
            1,
            &mozjs::jsval::PrivateValue(data),
        );
        *result = env_ref.put(ObjectValue(froot.get()));
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_set_named_property(
    env: napi_env,
    obj: napi_value,
    utf8name: *const c_char,
    value: napi_value,
) -> napi_status {
    if obj.is_null() || value.is_null() || utf8name.is_null() {
        return NAPI_INVALID_ARG;
    }
    let name = match unsafe { cstr_of_len(utf8name, usize::MAX) } {
        Ok(s) => s,
        Err(st) => return st,
    };
    let Ok(cname) = CString::new(name) else {
        return NAPI_GENERIC_FAILURE;
    };
    let mut cx = unsafe { cx_of(env) };
    let env_ref = unsafe { e(env) };
    // SAFETY：槽位有效（scope 存活期内）；cx 在 realm 内。
    unsafe {
        let obj_v = env_ref.get(obj);
        if !obj_v.is_object() {
            return NAPI_INVALID_ARG;
        }
        let s = mozjs::jsapi::JS_NewStringCopyN(cx.raw_cx(), cname.as_ptr(), cname.as_bytes().len());
        if s.is_null() {
            return NAPI_GENERIC_FAILURE;
        }
        rooted!(&in(cx) let key_root = mozjs::jsval::StringValue(&*s));
        let val = env_ref.get(value);
        set_via_helper(&mut cx, obj_v, key_root.get(), val)
    }
}

/// napi set 面的统一通道：经 sloppy prelude helper `__wjs2_napi_set`
/// （`obj[key] = value`）——JSAPI JS_SetProperty 是 strict 语义（只读/冻结
/// 属性抛 TypeError），Node 的 napi_set_property 走 v8 非严格 set（静默
/// 返回 ok，2026-09-14 实测对齐）。
///
/// # Safety
/// `cx`/值槽位语义同调用方（N-API 面内）。
pub(crate) unsafe fn set_via_helper(
    cx: &mut JSContext,
    obj_v: JSVal,
    key_v: JSVal,
    val: JSVal,
) -> napi_status {
    // SAFETY：helper 与值均 rooted/槽位存活；异常经 pending 传播。
    {
        let Some(helper) = get_prop_value(cx, crate::state::global(), c"__wjs2_napi_set") else {
            return NAPI_GENERIC_FAILURE;
        };
        match call_three(cx, crate::state::global(), helper, obj_v, key_v, val) {
            Some(_) => NAPI_OK,
            None => NAPI_GENERIC_FAILURE,
        }
    }
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_string_utf8(
    env: napi_env,
    str_: *const c_char,
    length: usize,
    result: *mut napi_value,
) -> napi_status {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let s = match unsafe { cstr_of_len(str_, length) } {
        Ok(s) => s,
        Err(st) => return st,
    };
    let env_ref = unsafe { e(env) };
    // SAFETY：to_jsval 只写 rooted 出参。
    unsafe {
        let mut cx = cx_of(env);
        rooted!(&in(cx) let mut v = UndefinedValue());
        s.as_str().to_jsval(&mut cx, v.handle_mut());
        *result = env_ref.put(v.get());
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_int32(
    env: napi_env,
    value: i32,
    result: *mut napi_value,
) -> napi_status {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    // SAFETY：Int32Value 保留 int32 tag（get_value_int32 的 to_int32 断言依赖）。
    unsafe { *result = e(env).put(mozjs::jsval::Int32Value(value)) };
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_uint32(
    env: napi_env,
    value: u32,
    result: *mut napi_value,
) -> napi_status {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    // SAFETY：UInt32Value 保留最小 tag 表示。
    unsafe { *result = e(env).put(mozjs::jsval::UInt32Value(value)) };
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_int64(
    env: napi_env,
    value: i64,
    result: *mut napi_value,
) -> napi_status {
    // SAFETY：同族建值（int64 → double 精度损失为 N-API 语义）。
    unsafe { create_number(env, value as f64, result) }
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_double(
    env: napi_env,
    value: f64,
    result: *mut napi_value,
) -> napi_status {
    // SAFETY：同族建值。
    unsafe { create_number(env, value, result) }
}

/// # Safety
/// 调用方持有效 env/result。
unsafe fn create_number(env: napi_env, value: f64, result: *mut napi_value) -> napi_status {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    // SAFETY：f64 位顶起 JSVal 无副作用。
    unsafe {
        *result = e(env).put(mozjs::jsval::DoubleValue(value));
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_undefined(env: napi_env, result: *mut napi_value) -> napi_status {
    // SAFETY：单值建槽。
    unsafe {
        if result.is_null() {
            return NAPI_INVALID_ARG;
        }
        *result = e(env).put(UndefinedValue());
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_null(env: napi_env, result: *mut napi_value) -> napi_status {
    // SAFETY：单值建槽。
    unsafe {
        if result.is_null() {
            return NAPI_INVALID_ARG;
        }
        *result = e(env).put(mozjs::jsval::NullValue());
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_boolean(
    env: napi_env,
    value: bool,
    result: *mut napi_value,
) -> napi_status {
    // SAFETY：单值建槽。
    unsafe {
        if result.is_null() {
            return NAPI_INVALID_ARG;
        }
        *result = e(env).put(mozjs::jsval::BooleanValue(value));
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_global(env: napi_env, result: *mut napi_value) -> napi_status {
    // SAFETY：state::global 为会话当前 global（与 cx 同 realm）。
    unsafe {
        if result.is_null() {
            return NAPI_INVALID_ARG;
        }
        *result = e(env).put(mozjs::jsval::ObjectValue(crate::state::global()));
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_cb_info(
    env: napi_env,
    cbinfo: napi_callback_info,
    argc: *mut usize,
    argv: *mut napi_value,
    this_arg: *mut napi_value,
    data: *mut *mut c_void,
) -> napi_status {
    let _ = env;
    if cbinfo.is_null() {
        return NAPI_INVALID_ARG;
    }
    // SAFETY：cbinfo 由 trampoline 建立且在回调栈内有效。
    unsafe {
        let ci = &*(cbinfo as *const CbInfo);
        if !argc.is_null() {
            if argv.is_null() {
                *argc = ci.argc as usize;
            } else {
                let requested = *argc;
                let actual = ci.argc as usize;
                // Node 口径（js_native_api_v8.cc `Args()`）：请求槽位多于实际
                // 参数时，余下槽位全部填同一 undefined——官方 js-native-api
                // 套件（3_callbacks）在 argc==1 断言后照读 args[1]，留垃圾
                // 槽位 = addon 侧 UB 读（139）；undefined 槽位在回调窗口内存活。
                let undef = e(env).put(mozjs::jsval::UndefinedValue());
                for i in 0..requested {
                    *argv.add(i) = if i < actual { ci.argv[i] } else { undef };
                }
                *argc = actual;
            }
        }
        if !this_arg.is_null() {
            *this_arg = ci.this;
        }
        if !data.is_null() {
            *data = ci.data;
        }
    }
    NAPI_OK
}
