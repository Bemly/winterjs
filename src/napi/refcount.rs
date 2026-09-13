//! napi 引用（plan-napi M3）：`napi_ref` = `NapiEnv.refs` 的 Box 记录指针。
//!
//! - `napi_create_reference(value, initial_refcount)` → RefRec { value:
//!   `Box<Heap<JSVal>>`, refcount }；ref/unref 调 refcount；refcount==0 = weak。
//! - **偏差（记 plan-napi §4）**：SM 无 embedder 可用的弱值通道（Heap 槽不
//!   trace 即 GC 后悬垂，读回即 UB）——weak 引用的值同样进 GC 图（钉至
//!   `napi_delete_reference`）。Node 语义下 weak 引用仅是"可能被回收"的建议，
//!   本实现等价于"引用存续期 = 句柄存续期"，rolldown/napi-rs 的
//!   ref→async→unref→delete 用法不受影响。
//! - `napi_get_reference_value`：恒可取回值（weak 同；Node 在值已回收时返
//!   空值——我们不发生）。

use std::ffi::c_void;

use mozjs::jsapi::Heap;
use mozjs::jsval::JSVal;

use crate::napi::api::{e, NAPI_INVALID_ARG, NAPI_OK};
use crate::napi::sys::{napi_env, napi_ref, napi_status, napi_value};

pub(crate) struct RefRec {
    pub value: Box<Heap<JSVal>>,
    pub refcount: u32,
}

/// # Safety
/// N-API 约定（vendored js_native_api.h:349）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_reference(
    env: napi_env,
    value: napi_value,
    initial_refcount: u32,
    result: *mut napi_ref,
) -> napi_status {
    if value.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(value) };
    let env_ref = unsafe { e(env) };
    let rec = Box::new(RefRec {
        // SAFETY：Box 定址（§4.40）；env trace 按存续期追值（weak 钉值偏差见模块头）。
        value: Heap::boxed(v),
        refcount: initial_refcount,
    });
    // SAFETY：出参为 addon 提供的合法指针；句柄 = 记录指针（会话存续期恒稳）。
    unsafe {
        *result = &*rec as *const RefRec as *const c_void as napi_ref;
    }
    env_ref.refs.push(rec);
    NAPI_OK
}

/// 句柄 → 记录（越界/悬垂返回 None；只做成员比对）。
///
/// # Safety
/// `ref_` 须为本 crate 发出的句柄或悬垂值。
unsafe fn ref_rec(env: napi_env, ref_: napi_ref) -> Option<&'static mut RefRec> {
    let ptr = ref_ as *const RefRec;
    // SAFETY：只做指针比对；命中后 deref 由 Box 归 env 管理（§4.40 恒稳）。
    unsafe {
        let env_ref = e(env);
        let idx = env_ref.refs.iter().position(|r| &**r as *const RefRec == ptr)?;
        Some(&mut env_ref.refs[idx])
    }
}

/// # Safety
/// N-API 约定（vendored js_native_api.h:355 附近；句柄须有效且未删）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_delete_reference(env: napi_env, ref_: napi_ref) -> napi_status {
    let ptr = ref_ as *const RefRec;
    let env_ref = unsafe { e(env) };
    // SAFETY：句柄成员比对后按位摘除（Box drop；dangling 句柄 = invalid_arg）。
    {
        let before = env_ref.refs.len();
        env_ref.refs.retain(|r| &**r as *const RefRec != ptr);
        if env_ref.refs.len() == before {
            env_ref.set_last_error("invalid or deleted napi_ref");
            return NAPI_INVALID_ARG;
        }
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（句柄须有效）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_reference_ref(
    env: napi_env,
    ref_: napi_ref,
    result: *mut u32,
) -> napi_status {
    let Some(rec) = (unsafe { ref_rec(env, ref_) }) else {
        return NAPI_INVALID_ARG;
    };
    rec.refcount = rec.refcount.saturating_add(1);
    // SAFETY：出参为 addon 提供的合法指针（可空 = 不取计数）。
    unsafe {
        if !result.is_null() {
            *result = rec.refcount;
        }
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（0 上再 unref = invalid_arg，Node 口径）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_reference_unref(
    env: napi_env,
    ref_: napi_ref,
    result: *mut u32,
) -> napi_status {
    let Some(rec) = (unsafe { ref_rec(env, ref_) }) else {
        return NAPI_INVALID_ARG;
    };
    if rec.refcount == 0 {
        let env_ref = unsafe { e(env) };
        env_ref.set_last_error("reference refcount underflow");
        return NAPI_INVALID_ARG;
    }
    rec.refcount -= 1;
    // SAFETY：出参为 addon 提供的合法指针（可空）。
    unsafe {
        if !result.is_null() {
            *result = rec.refcount;
        }
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（vendored js_native_api.h:379）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_reference_value(
    env: napi_env,
    ref_: napi_ref,
    result: *mut napi_value,
) -> napi_status {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(rec) = (unsafe { ref_rec(env, ref_) }) else {
        return NAPI_INVALID_ARG;
    };
    let v = rec.value.get();
    // SAFETY：值进槽位（traced，存活期同回调/scope）。
    unsafe {
        *result = e(env).put(v);
    }
    NAPI_OK
}
