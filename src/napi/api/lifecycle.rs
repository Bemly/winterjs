//! napi 生命周期杂面（uv/fatal/async/scope/script/memory）。

use super::*;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use crate::napi::sys::{napi_env, napi_status, napi_value};
use crate::jsapi_glue::raw_handle;
use crate::jsapi_glue::value_to_string;
use mozjs::jsval::UndefinedValue;

use mozjs::rooted;

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
/// N-API 约定（触发 uncaught 异常路径：本仓经 pending exception 传播到顶层，
/// 用户可见结局与 Node 一致——报错 + exit 1；偏差记 plan-napi §4）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_fatal_exception(env: napi_env, err: napi_value) -> napi_status {
    if err.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(err) };
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：槽位值；标准 pending 传播路径（trampoline pending 优先）。
    unsafe {
        rooted!(&in(cx) let v_root = v);
        mozjs::jsapi::JS_SetPendingException(
            cx.raw_cx(),
            raw_handle(v_root.as_ptr()),
            mozjs::jsapi::JS::ExceptionStackBehavior::Capture,
        );
    }
    NAPI_OK
}

// ── M2：异步上下文 / callback scope / run_script / external memory ──────

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_async_init(
    env: napi_env,
    async_resource: napi_value,
    async_resource_name: napi_value,
    result: *mut sys::napi_async_context,
) -> napi_status {
    let _ = (env, async_resource, async_resource_name);
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    // 偏差（记 plan-napi §4）：async_hooks 上下文未接——发哑句柄（堆 1 字节，
    // async_destroy 释放；句柄非空即合 N-API 契约）。
    // SAFETY：result 为 addon 提供的合法出参。
    unsafe { *result = Box::into_raw(Box::new(0u8)) as sys::napi_async_context };
    NAPI_OK
}

/// # Safety
/// N-API 约定（context 须来自 async_init 且未销毁）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_async_destroy(
    env: napi_env,
    async_context: sys::napi_async_context,
) -> napi_status {
    let _ = env;
    if async_context.is_null() {
        return NAPI_INVALID_ARG;
    }
    // SAFETY：句柄由 async_init 发出（Box::into_raw），一次配对释放。
    unsafe { drop(Box::from_raw(async_context as *mut u8)) };
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_open_callback_scope(
    env: napi_env,
    resource_object: napi_value,
    context: sys::napi_async_context,
    result: *mut sys::napi_callback_scope,
) -> napi_status {
    let _ = (resource_object, context);
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    // 偏差（记 plan-napi §4）：async context 面未接——发 env 哑句柄（非空、
    // 会话内唯一；close 只验非空）。
    // SAFETY：result 为 addon 提供的合法出参。
    unsafe { *result = env as usize as sys::napi_callback_scope };
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_close_callback_scope(
    env: napi_env,
    scope: sys::napi_callback_scope,
) -> napi_status {
    let _ = env;
    if scope.is_null() {
        return NAPI_INVALID_ARG;
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（script 为 string；同步求值，promise 反应交给外层事件循环）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_run_script(
    env: napi_env,
    script: napi_value,
    result: *mut napi_value,
) -> napi_status {
    if script.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(script) };
    if !v.is_string() {
        return sys::napi_status_napi_string_expected;
    }
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：value 已验字符串；evaluate_script 自进当前 global 的 realm
    //（同 global 无 realm 切换，返回后 addon 栈帧的 realm 语境不变）。
    let code = value_to_string(&mut cx, v);
    unsafe {
        rooted!(&in(cx) let global = crate::state::global());
        rooted!(&in(cx) let mut rval = UndefinedValue());
        let filename = CString::new("napi_run_script").unwrap_or_default();
        let options = mozjs::rust::CompileOptionsWrapper::new(&cx, filename, 1);
        if mozjs::rust::evaluate_script(&mut cx, global.handle(), &code, rval.handle_mut(), options)
            .is_err()
        {
            // pending exception 已置（addon 按 N-API 契约处理/传播）。
            return NAPI_GENERIC_FAILURE;
        }
        *result = e(env).put(rval.get());
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_adjust_external_memory(
    env: napi_env,
    change_in_bytes: i64,
    adjusted_value: *mut i64,
) -> napi_status {
    if adjusted_value.is_null() {
        return NAPI_INVALID_ARG;
    }
    let env_ref = unsafe { e(env) };
    env_ref.external_mem += change_in_bytes;
    // SAFETY：result 为 addon 提供的合法出参（累计值，Node 口径）。
    unsafe { *adjusted_value = env_ref.external_mem };
    NAPI_OK
}
