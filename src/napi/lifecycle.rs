//! napi 环境生命周期面（plan-napi M4）：env cleanup hooks + node 版本面。
//!
//! - cleanup hooks：`end_session` 收敛点触发（JS 线程、引擎存活期内最后一个
//!   环节——hook 无 env 参（napi_cleanup_hook 契约：仅 arg），不可能调 JSAPI；
//!   会话生灭随 §4.24 线程边界）。同 (fun, arg) 重复 add = invalid_arg（Node 口径）。
//! - `napi_get_node_version`：返回与 `process.version` 同源的静态结构
//!   （CalVer 口径；进程级快照一次，指针交 addon 长期持有）。

use std::ffi::{c_char, c_void, CString};
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use crate::napi::api::{e, NAPI_INVALID_ARG, NAPI_OK};
use crate::napi::sys;
use crate::napi::sys::{napi_cleanup_hook, napi_env, napi_status};

/// (fun, arg) 去重键（指针位比对；arg 存续期由 addon 自持）。
#[derive(PartialEq, Eq)]
struct HookKey(usize, usize);

/// # Safety
/// N-API 约定（vendored node_api.h:188）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_add_env_cleanup_hook(
    env: napi_env,
    fun: napi_cleanup_hook,
    arg: *mut c_void,
) -> napi_status {
    let Some(f) = fun else {
        return NAPI_INVALID_ARG;
    };
    let env_ref = unsafe { e(env) };
    let key = HookKey(f as usize, arg as usize);
    if env_ref
        .cleanup_hooks
        .iter()
        .any(|(g, a)| HookKey(g.map_or(0, |h| h as usize), *a as usize) == key)
    {
        env_ref.set_last_error("cleanup hook already registered");
        return NAPI_INVALID_ARG;
    }
    env_ref.cleanup_hooks.push((Some(f), arg));
    NAPI_OK
}

/// # Safety
/// N-API 约定（vendored node_api.h:191；未注册即 invalid_arg）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_remove_env_cleanup_hook(
    env: napi_env,
    fun: napi_cleanup_hook,
    arg: *mut c_void,
) -> napi_status {
    let Some(f) = fun else {
        return NAPI_INVALID_ARG;
    };
    let env_ref = unsafe { e(env) };
    let key = HookKey(f as usize, arg as usize);
    let before = env_ref.cleanup_hooks.len();
    env_ref
        .cleanup_hooks
        .retain(|(g, a)| HookKey(g.map_or(0, |h| h as usize), *a as usize) != key);
    if env_ref.cleanup_hooks.len() == before {
        env_ref.set_last_error("cleanup hook not found");
        return NAPI_INVALID_ARG;
    }
    NAPI_OK
}

/// 会话收尾触发（end_session 调；后进先出——Node 口径）。
pub fn run_cleanup_hooks() {
    let Some(env_ptr) = crate::state::napi_env_ptr() else {
        return;
    };
    // SAFETY：JS 线程（end_session 在 JS 线程收敛）；env 会话存续。
    let hooks = unsafe {
        let env_ref = &mut *env_ptr;
        std::mem::take(&mut env_ref.cleanup_hooks)
    };
    for (f, arg) in hooks.into_iter().rev() {
        if let Some(f) = f {
            // SAFETY：addon 注册的 hook（无 env 参，不可能进 JSAPI——模块头注）。
            unsafe { f(arg) };
        }
    }
}

/// 会话收尾触发：任意对象 napi_wrap 的 finalizer（end_session 调；后进先出，
/// 与 cleanup hooks 同序原则。napi-rs 的 finalizer 只 free Rust 分配——
/// Promise 回调闭包盒，不进 JSAPI）。
pub fn run_wrap_finalizers() {
    let Some(env_ptr) = crate::state::napi_env_ptr() else {
        return;
    };
    // SAFETY：JS 线程（end_session 在 JS 线程收敛）；env 会话存续。
    let boxes = unsafe {
        let env_ref = &mut *env_ptr;
        std::mem::take(&mut env_ref.wrap_boxes)
    };
    for rec in boxes.into_iter().rev() {
        if let Some(f) = rec.finalize {
            // SAFETY：addon 注册的 finalizer（只析构 native payload）。
            unsafe { f(rec.env, rec.payload, rec.hint) };
        }
    }
}

/// # Safety
/// N-API 约定（vendored node_api.h:194；result 收静态结构指针）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_node_version(
    env: napi_env,
    result: *mut *const sys::napi_node_version,
) -> napi_status {
    let _ = env; // 版本面与 env 无关（Node 同）
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    // 与 process.version 同源（CARGO_PKG_VERSION，CalVer YY.MM.PATCH）。
    // 进程级快照一次（指针交 addon 长期持有，永不再变——AtomicPtr 只读发布）。
    static INIT: AtomicUsize = AtomicUsize::new(0);
    static SNAPSHOT: AtomicPtr<sys::napi_node_version> = AtomicPtr::new(std::ptr::null_mut());
    let v = env!("CARGO_PKG_VERSION");
    let mut it = v.split('.');
    let major: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    // SAFETY：仅 JS 线程调用（napi 契约）；INIT 单次发布，此后只读。
    unsafe {
        if INIT.load(Ordering::SeqCst) == 0 {
            let release: &'static CString =
                Box::leak(Box::new(CString::new(format!("v{v}")).unwrap_or_default()));
            let boxed = Box::new(sys::napi_node_version {
                major,
                minor,
                patch,
                release: release.as_ptr() as *const c_char,
            });
            SNAPSHOT.store(Box::into_raw(boxed), Ordering::SeqCst);
            INIT.store(1, Ordering::SeqCst);
        }
        *result = SNAPSHOT.load(Ordering::SeqCst);
    }
    NAPI_OK
}
