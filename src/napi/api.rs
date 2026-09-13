//! `napi_*` 函数族（M0 hello 子集 + 基础面；全集按 plan-napi §6 名单逐 M 补）。
//!
//! 统一形态：`#[unsafe(no_mangle)] pub unsafe extern "C" fn napi_xxx(env: napi_env, ...)`
//! ——`env` 即 `NapiEnv`（会话单例）指针 cast；JSAPI 调用经 `wrap_cx(env.cx)`。
//! 纪律：JS 对象落槽位前必须先 `rooted!`（建值与落槽之间可能 GC）；
//! reserved slots 私有值（GC 不可见）携带 addon 回调指针/`data`。

use std::ffi::{c_char, c_int, c_void, CStr, CString};

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::jsapi::{JS_IsExceptionPending, JSContext as RawJSContext, JSObject};
use mozjs::jsval::{JSVal, ObjectValue, UndefinedValue};
use mozjs::rooted;
use mozjs::context::JSContext;

use crate::jsapi_glue::raw_handle;
use crate::napi::env::{CbInfo, NapiEnv};
use crate::napi::sys;
use crate::napi::sys::{
    napi_callback, napi_callback_info, napi_env, napi_status, napi_value,
};

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
    unsafe { JSContext::from_ptr(std::ptr::NonNull::new_unchecked(e(env).cx)) }
}

/// C 字符串参数（len == `napi_auto_length`（SIZE_MAX）/ -1 时取 strlen）。
///
/// # Safety
/// `s` 在长度语义内必须是合法 UTF-8 可读内存。
unsafe fn cstr_of_len(s: *const c_char, len: usize) -> Result<String, napi_status> {
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

const NAPI_OK: napi_status = sys::napi_status_napi_ok;
const NAPI_INVALID_ARG: napi_status = sys::napi_status_napi_invalid_arg;
const NAPI_GENERIC_FAILURE: napi_status = sys::napi_status_napi_generic_failure;

// ── trampoline：JS native 函数 → addon C 回调 ────────────────────────────
// reserved slots：0 = addon 回调指针、1 = data。帧布局（jsapi_glue Frame 约定）：
// vp[0]=callee/rval 槽、vp[1]=this、vp[2..]=实参。

pub unsafe extern "C" fn napi_trampoline(
    cx_raw: *mut RawJSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY：引擎回调帧 + 会话 env（JS 线程）；整个函数体即边界（UNSAFE-BOUNDARY：
    // reserved slots 指针须由 napi_create_function 写入；覆盖 tests/napi.rs）。
    return trampoline_inner(cx_raw, argc, vp);

    fn trampoline_inner(
        cx_raw: *mut RawJSContext,
        argc: u32,
        vp: *mut JSVal,
    ) -> bool {
    // SAFETY：同上（引擎回调帧 + 会话 env + reserved slots 前置）。
    unsafe {
    let Some(env) = crate::state::napi_env_ptr() else {
        return false;
    };
    let callee = *vp;
    if !callee.is_object() {
        return false;
    }
    let fobj = callee.to_object();
    let cb_ptr = (*mozjs::jsapi::js::GetFunctionNativeReserved(fobj, 0)).to_private() as usize;
    let data = (*mozjs::jsapi::js::GetFunctionNativeReserved(fobj, 1)).to_private();
    if cb_ptr == 0 {
        return false;
    }
    // SAFETY：指针由 napi_create_function 写入（addon 回调，签名按 N-API 头）。
    let cb: unsafe extern "C" fn(napi_env, napi_callback_info) -> napi_value =
        std::mem::transmute(cb_ptr);

    // 实参/this 先拷入 env 槽位（回调返回后仍有效；Node 同语义）。
    let env_ref = e(env as napi_env);
    let mut argv = Vec::with_capacity(argc as usize);
    for i in 0..argc {
        argv.push(env_ref.put(*vp.add(2 + i as usize)));
    }
    let this = env_ref.put(*vp.add(1));
    let info = CbInfo {
        argc,
        argv,
        this,
        data: data as *mut c_void,
    };
    let r = cb(env as napi_env, &info as *const CbInfo as napi_callback_info);
    // SAFETY：info 借用的槽位都在 env arena 内（traced），cb 返回后仅指针作废。
    let env_ref = e(env as napi_env);
    if !r.is_null() {
        *vp = env_ref.get(r);
        true
    } else if JS_IsExceptionPending(cx_raw) {
        false
    } else {
        *vp = UndefinedValue();
        true
    }
    }
    }
}

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
        let fun = mozjs::jsapi::js::NewFunctionWithReserved(
            cx.raw_cx(),
            Some(napi_trampoline),
            0,
            0,
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
    if obj.is_null() || value.is_null() {
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
        let val = env_ref.get(value);
        if !crate::jsapi_glue::define_prop(&mut cx, obj_v.to_object(), &cname, val) {
            return NAPI_GENERIC_FAILURE;
        }
    }
    NAPI_OK
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
                let n = (*argc).min(ci.argc as usize);
                *argc = n;
                for i in 0..n {
                    *argv.add(i) = ci.argv[i];
                }
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
    // SAFETY：槽位读取；cx raw 在会话内存续。
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
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_open_handle_scope(
    env: napi_env,
    result: *mut sys::napi_handle_scope,
) -> napi_status {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let env_ref = unsafe { e(env) };
    env_ref.scopes.push(env_ref.slots.len());
    // M0：标记句柄以 index 编码（对 addon 不透明；arena 只增不缩，见模块偏差）。
    // SAFETY：result 为 addon 提供的合法出参。
    unsafe { *result = env_ref.scopes.len() as sys::napi_handle_scope };
    NAPI_OK
}

/// # Safety
/// N-API 约定（scope 必须配对关闭）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_close_handle_scope(
    env: napi_env,
    scope: sys::napi_handle_scope,
) -> napi_status {
    let env_ref = unsafe { e(env) };
    // M0：只配平校验，不缩 arena（偏差记 plan-napi §4；M2 引入 escape-aware 回收）。
    let idx = scope as usize;
    debug_assert!(idx <= env_ref.scopes.len(), "napi_close_handle_scope out of order");
    while env_ref.scopes.len() > idx.saturating_sub(1) && !env_ref.scopes.is_empty() {
        env_ref.scopes.pop();
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

// ── uv / 杂项垫片（plan-napi §1：rolldown 面仅 uv_run）──────────────────

/// libuv 垫片：rolldown 经 dlsym 查表；M0 stub（UV_RUN_* 语义 M3 接事件循环）。
///
/// # Safety
/// addon 传入的 loop 指针按 libuv ABI；本实现忽略。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uv_run(_loop_: *mut c_void, _mode: c_int) -> c_int {
    0
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_uv_event_loop(
    env: napi_env,
    result: *mut *mut c_void,
) -> napi_status {
    // SAFETY：返回 env 本体作不透明句柄（本仓 uv 垫片不消费其内容）。
    unsafe {
        if result.is_null() {
            return NAPI_INVALID_ARG;
        }
        *result = env as *mut c_void;
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（fatal 路径不返回）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_fatal_error(
    location: *const c_char,
    _location_len: usize,
    message: *const c_char,
    _message_len: usize,
) -> ! {
    let loc = if location.is_null() {
        "napi".to_string()
    } else {
        // SAFETY：addon 保证 NUL 结尾。
        unsafe { CStr::from_ptr(location).to_string_lossy().into_owned() }
    };
    let msg = if message.is_null() {
        "fatal error".to_string()
    } else {
        // SAFETY：同上。
        unsafe { CStr::from_ptr(message).to_string_lossy().into_owned() }
    };
    eprintln!("FATAL napi error in {loc}: {msg}");
    std::process::abort();
}

/// # Safety
/// N-API 约定（M0：记 last-error 即返；真 fatal 异常传播面 M2 补）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_fatal_exception(env: napi_env, err: napi_value) -> napi_status {
    unsafe { e(env).set_last_error("fatal exception (napi)") };
    let _ = err;
    NAPI_GENERIC_FAILURE
}

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

// ── 调用面（M1 补：props fixture 与 rolldown 回调都需要）────────────────

/// # Safety
/// N-API 约定（argv 为本 env 槽位指针数组；result 可空 = 不取返回值）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_call_function(
    env: napi_env,
    recv: napi_value,
    func: napi_value,
    argc: usize,
    argv: *const napi_value,
    result: *mut napi_value,
) -> napi_status {
    if func.is_null() {
        return NAPI_INVALID_ARG;
    }
    let env_ref = unsafe { e(env) };
    let recv_v = if recv.is_null() {
        UndefinedValue()
    } else {
        unsafe { env_ref.get(recv) }
    };
    let fn_v = unsafe { env_ref.get(func) };
    let mut cx = unsafe { cx_of(env) };
    // 参数落 JS 数组（元素逐个 SetElement——数组对象 rooted，天然 GC 安全；
    // N 参展开经 prelude helper `__wjs_napi_call(recv, fn, args)`，timers 同款惯例）。
    // SAFETY：数组与函数先 rooted 再调用；值均在本 env 槽位（traced）。
    unsafe {
        rooted!(&in(cx) let fn_root = fn_v);
        let arr = mozjs::jsapi::JS::NewArrayObject1(cx.raw_cx(), argc);
        if arr.is_null() {
            return sys::napi_status_napi_generic_failure;
        }
        rooted!(&in(cx) let arr_root = arr);
        for i in 0..argc {
            let av = *argv.add(i);
            let val = env_ref.get(av);
            rooted!(&in(cx) let val_root = val);
            if !mozjs::jsapi::JS_SetElement(
                cx.raw_cx(),
                raw_handle(&arr_root.get()),
                i as u32,
                raw_handle(val_root.as_ptr()),
            ) {
                return sys::napi_status_napi_generic_failure;
            }
        }
        rooted!(&in(cx) let recv_root = recv_v);
        let Some(helper) = crate::jsapi_glue::get_prop_value(&mut cx, crate::state::global(), c"__wjs_napi_call") else {
            return sys::napi_status_napi_generic_failure;
        };
        let Some(r) = crate::jsapi_glue::call_three(
            &mut cx,
            crate::state::global(),
            helper,
            recv_root.get(),
            fn_root.get(),
            ObjectValue(arr_root.get()),
        ) else {
            // addon 侧按契约查 pending exception
            return sys::napi_status_napi_generic_failure;
        };
        if !result.is_null() {
            *result = env_ref.put(r);
        }
    }
    NAPI_OK
}
