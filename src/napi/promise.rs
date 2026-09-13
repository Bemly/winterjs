//! napi promise/deferred（plan-napi M3）：经 SM 原生 promise 机制落地。
//!
//! - `napi_create_promise`：`JS::NewPromiseObject(executor=null)`——无 executor
//!   的 promise 只能经 `JS::Resolve/RejectPromise` 落定（引擎文档同口径），
//!   恰好就是 deferred 语义。deferred 句柄 = `NapiEnv.deferreds` 的 Box 槽位
//!   指针（§4.40 定址；resolve/reject 时取走并清槽，deferred 一次性）。
//! - 落定走引擎标准路径（reaction job 入同一 job queue，事件循环 RunJobs
//!   排空）——§4.18 结算点纪律由既有循环拓扑保证（回调返回后引擎继续排空）。
//! - `napi_promise_is_pending`/`napi_promise_is_done_with_result` 未在 vendored
//!   头出现（实验面），不做。

use std::ffi::c_void;

use mozjs::jsapi::{Heap, JSObject, JS_ClearPendingException};
use mozjs::jsval::{JSVal, ObjectValue};
use mozjs::rooted;

use crate::jsapi_glue::raw_handle;
use crate::napi::api::{cx_of, e, NAPI_GENERIC_FAILURE, NAPI_INVALID_ARG, NAPI_OK};
use crate::napi::sys::{napi_deferred, napi_env, napi_status, napi_value};

/// # Safety
/// N-API 约定（vendored js_native_api.h:501）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_promise(
    env: napi_env,
    deferred: *mut napi_deferred,
    promise: *mut napi_value,
) -> napi_status {
    if deferred.is_null() || promise.is_null() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    let env_ref = unsafe { e(env) };
    // SAFETY：promise 对象先 rooted 再落槽/入 deferred 表（建值与落槽间可能 GC）。
    unsafe {
        rooted!(&in(cx) let executor: *mut JSObject = std::ptr::null_mut());
        let p = mozjs::jsapi::JS::NewPromiseObject(cx.raw_cx(), raw_handle(&executor.get()));
        if p.is_null() {
            return NAPI_GENERIC_FAILURE;
        }
        rooted!(&in(cx) let p_root: *mut JSObject = p);
        let slot = Heap::boxed(ObjectValue(p_root.get()));
        // deferred 句柄 = Heap 槽位地址（Box 堆地址恒稳，§4.40）。
        let handle =
            &*slot as *const Heap<JSVal> as *const c_void as napi_deferred;
        env_ref.deferreds.push(slot);
        *deferred = handle;
        *promise = env_ref.put(ObjectValue(p_root.get()));
    }
    NAPI_OK
}

/// deferred 句柄是否仍有效（未落定；槽位指针比对）。
///
/// # Safety
/// `handle` 须为本 crate 发出的 deferred 句柄或悬垂值（只比对不 deref）。
unsafe fn deferred_live(env: napi_env, handle: napi_deferred) -> bool {
    let slot = handle as *const Heap<JSVal>;
    // SAFETY：只做指针比对，不 deref 悬垂句柄。
    unsafe {
        e(env).deferreds.iter().any(|s| &**s as *const Heap<JSVal> == slot)
    }
}

/// deferred 落定共享路径：取 promise → Resolve/Reject → 清槽（Node 口径：
/// deferred 一次性，落定后句柄失效）。
///
/// # Safety
/// `env` 有效；`handle` 语义同 deferred_live；`resolution` 为有效 napi_value。
unsafe fn settle_deferred(
    env: napi_env,
    handle: napi_deferred,
    resolution: napi_value,
    reject: bool,
) -> napi_status {
    if resolution.is_null() {
        return NAPI_INVALID_ARG;
    }
    if !unsafe { deferred_live(env, handle) } {
        let env_ref = unsafe { e(env) };
        env_ref.set_last_error("deferred already settled or invalid");
        return NAPI_INVALID_ARG;
    }
    let slot = handle as *const Heap<JSVal>;
    let promise_v = unsafe { (*slot).get() };
    if !promise_v.is_object() {
        return NAPI_GENERIC_FAILURE;
    }
    let v = unsafe { e(env).get(resolution) };
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：promise 与落定值先 rooted 再落定（落定入 reaction job，可能 GC）。
    unsafe {
        rooted!(&in(cx) let p_root: *mut JSObject = promise_v.to_object());
        rooted!(&in(cx) let v_root = v);
        let ok = if reject {
            mozjs::jsapi::JS::RejectPromise(
                cx.raw_cx(),
                raw_handle(&p_root.get()),
                raw_handle(v_root.as_ptr()),
            )
        } else {
            mozjs::jsapi::JS::ResolvePromise(
                cx.raw_cx(),
                raw_handle(&p_root.get()),
                raw_handle(v_root.as_ptr()),
            )
        };
        if !ok {
            JS_ClearPendingException(cx.raw_cx());
            return NAPI_GENERIC_FAILURE;
        }
        // 清槽（deferred 一次性；Heap drop 自带 clearing barrier，§4.40）。
        e(env).deferreds.retain(|s| &**s as *const Heap<JSVal> != slot);
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（vendored js_native_api.h:504）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_resolve_deferred(
    env: napi_env,
    deferred: napi_deferred,
    resolution: napi_value,
) -> napi_status {
    // SAFETY：句柄/值语义同 settle_deferred。
    unsafe { settle_deferred(env, deferred, resolution, false) }
}

/// # Safety
/// N-API 约定（vendored js_native_api.h:507）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_reject_deferred(
    env: napi_env,
    deferred: napi_deferred,
    rejection: napi_value,
) -> napi_status {
    // SAFETY：同上。
    unsafe { settle_deferred(env, deferred, rejection, true) }
}

/// # Safety
/// N-API 约定（vendored js_native_api.h:510）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_is_promise(
    env: napi_env,
    value: napi_value,
    is_promise: *mut bool,
) -> napi_status {
    if value.is_null() || is_promise.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(value) };
    if !v.is_object() {
        // SAFETY：出参直写。
        unsafe { *is_promise = false };
        return NAPI_OK;
    }
    let cx = unsafe { cx_of(env) };
    // SAFETY：谓词（收 HandleObject，无 GC 点）。
    unsafe {
        rooted!(&in(cx) let obj_root: *mut JSObject = v.to_object());
        *is_promise = mozjs::jsapi::JS::IsPromiseObject(raw_handle(&obj_root.get()));
    }
    NAPI_OK
}
