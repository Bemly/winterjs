//! napi 调用（call_impl + call_function/make_callback）。

use super::*;
use std::ffi::{CStr};
use crate::napi::sys::{napi_env, napi_status, napi_value};
use crate::jsapi_glue::call_three;
use crate::jsapi_glue::get_prop_value;
use crate::jsapi_glue::raw_handle;
use mozjs::jsval::ObjectValue;
use mozjs::jsval::UndefinedValue;

use mozjs::rooted;

// ── 调用面（M1 起；M2 抽出 call_impl 供 make_callback 复用）────────────

/// 共享调用面：实参落 JS 数组（元素逐个 SetElement——数组对象 rooted，天然
/// GC 安全），经 prelude helper `helper(recv, fn, args)`（apply 展开）调用，
/// timers 同款惯例。
///
/// # Safety
/// `env` 有效；argv 为本 env 槽位指针数组；result 可空 = 不取返回值。
unsafe fn call_impl(
    env: napi_env,
    helper: &CStr,
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
    // 防御：func 非 object（悬垂槽位/类型错）即可读失败——Node 同款是 UB，我们
    // 不让它进 JSAPI（§4.79：截断语义下跨回调裸持 napi_value 的现形点）。
    if !fn_v.is_object() {
        let env_ref = unsafe { e(env) };
        env_ref.set_last_error("napi_call_function: func is not an object (stale napi_value?)");
        return NAPI_INVALID_ARG;
    }
    // SAFETY：数组与函数先 rooted 再调用；值均在本 env 槽位（traced）。
    unsafe {
        rooted!(&in(cx) let fn_root = fn_v);
        let arr = mozjs::jsapi::JS::NewArrayObject1(cx.raw_cx(), argc);
        if arr.is_null() {
            return NAPI_GENERIC_FAILURE;
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
                return NAPI_GENERIC_FAILURE;
            }
        }
        rooted!(&in(cx) let recv_root = recv_v);
        let Some(helper_v) = get_prop_value(&mut cx, crate::state::global(), helper) else {
            return NAPI_GENERIC_FAILURE;
        };
        // addon 侧按契约查 pending exception（失败 = 调用抛错）。
        let Some(r) = call_three(
            &mut cx,
            crate::state::global(),
            helper_v,
            recv_root.get(),
            fn_root.get(),
            ObjectValue(arr_root.get()),
        ) else {
            return NAPI_GENERIC_FAILURE;
        };
        if !result.is_null() {
            *result = env_ref.put(r);
        }
    }
    NAPI_OK
}

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
    // SAFETY：env 有效（前置）；helper 名为静态 CStr。
    unsafe { call_impl(env, c"__wjs2_napi_call", recv, func, argc, argv, result) }
}

/// # Safety
/// N-API 约定（async_context 可空；Node 同语义）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_make_callback(
    env: napi_env,
    async_context: sys::napi_async_context,
    recv: napi_value,
    func: napi_value,
    argc: usize,
    argv: *const napi_value,
    result: *mut napi_value,
) -> napi_status {
    // 偏差（记 plan-napi §4）：async_hooks 上下文面未接（async_context 收下
    // 不消费，M3 TSFN/async_work 再议）；调用语义与 call_function 一致。
    let _ = async_context;
    // SAFETY：同 call_function。
    unsafe { call_impl(env, c"__wjs2_napi_call", recv, func, argc, argv, result) }
}
