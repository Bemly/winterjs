//! napi scope 家族（plan-napi M2）：handle / escapable handle 统一栈。
//!
//! 语义（N-API 契约，对齐 Node）：
//! - `napi_value` 只在其创建时的 scope 存活期内有效；close 即截断 arena 回收
//!   （§4.77 pin-all 反例实测：不截断则 external/wrap 对象永不可达死态，
//!   finalizer 永不跑；addon 跨窗持有必须走 ref，§4.76）。
//! - 栈严格 LIFO：错序 close 返回 generic_failure（Node 同期 assert/UB，我们
//!   给可读错误）；escape 每 scope 至多一次，重复返回
//!   `napi_escape_called_twice`。
//! - handle 为 1-based 栈深（对 addon 不透明）。

use crate::napi::api::e;
use crate::napi::env::ScopeEntry;
use crate::napi::sys;
use crate::napi::sys::{napi_env, napi_status, napi_value};

const NAPI_OK: napi_status = sys::napi_status_napi_ok;
const NAPI_INVALID_ARG: napi_status = sys::napi_status_napi_invalid_arg;

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
    env_ref.scopes.push(ScopeEntry {
        mark: env_ref.slots.len(),
        escapable: false,
        escaped: false,
    });
    // SAFETY：result 为 addon 提供的合法出参；handle = 1-based 栈深。
    unsafe { *result = env_ref.scopes.len() as sys::napi_handle_scope };
    NAPI_OK
}

/// # Safety
/// N-API 约定（scope 必须按开栈逆序配对关闭）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_close_handle_scope(
    env: napi_env,
    scope: sys::napi_handle_scope,
) -> napi_status {
    // SAFETY：env 有效（会话单例）；错序路径只动自身栈。
    unsafe { close_scope_top(env, scope as usize, false) }
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_open_escapable_handle_scope(
    env: napi_env,
    result: *mut sys::napi_escapable_handle_scope,
) -> napi_status {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let env_ref = unsafe { e(env) };
    env_ref.scopes.push(ScopeEntry {
        mark: env_ref.slots.len(),
        escapable: true,
        escaped: false,
    });
    // SAFETY：result 为 addon 提供的合法出参。
    unsafe { *result = env_ref.scopes.len() as sys::napi_escapable_handle_scope };
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_close_escapable_handle_scope(
    env: napi_env,
    scope: sys::napi_escapable_handle_scope,
) -> napi_status {
    // SAFETY：同 close_handle_scope。
    unsafe { close_scope_top(env, scope as usize, true) }
}

/// 关闭栈顶 scope（严格 LIFO：`depth` 必须等于当前栈深且 kind 匹配），
/// 截断 arena 至开栈水位（§4.77 反例实证：不截断则 external/wrap 对象
/// 永不可达死态，finalizer 永不跑；addon 跨窗持有走 ref，§4.76）。
///
/// # Safety
/// `env` 为本 crate 发出的有效指针。
unsafe fn close_scope_top(env: napi_env, depth: usize, expect_escapable: bool) -> napi_status {
    let env_ref = unsafe { e(env) };
    if depth == 0 || depth != env_ref.scopes.len() {
        env_ref.set_last_error("handle scope closed out of order");
        return sys::napi_status_napi_generic_failure;
    }
    let entry = &env_ref.scopes[depth - 1];
    if entry.escapable != expect_escapable {
        env_ref.set_last_error("handle scope kind mismatch on close");
        return sys::napi_status_napi_generic_failure;
    }
    let entry = env_ref.scopes.pop().expect("depth checked");
    env_ref.slots.truncate(entry.mark);
    NAPI_OK
}

/// # Safety
/// N-API 约定（escapee 须为该 scope 内创建的值；每 scope 至多一次）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_escape_handle(
    env: napi_env,
    scope: sys::napi_escapable_handle_scope,
    escapee: napi_value,
    result: *mut napi_value,
) -> napi_status {
    if escapee.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let env_ref = unsafe { e(env) };
    let depth = scope as usize;
    // 只要求 scope 在栈上且未 escape 过（放宽 Node 的"最近 escapable"口径，
    // 兼容任意嵌套形态；每 scope 一次不变）。
    if depth == 0 || depth > env_ref.scopes.len() {
        env_ref.set_last_error("escape handle on closed scope");
        return NAPI_INVALID_ARG;
    }
    let entry = &env_ref.scopes[depth - 1];
    if !entry.escapable {
        env_ref.set_last_error("escape handle on non-escapable scope");
        return NAPI_INVALID_ARG;
    }
    if entry.escaped {
        return sys::napi_status_napi_escape_called_twice;
    }
    let v = unsafe { env_ref.get(escapee) };
    let escaped = env_ref.put_escaped(v);
    env_ref.scopes[depth - 1].escaped = true;
    // SAFETY：result 为 addon 提供的合法出参。
    unsafe { *result = escaped };
    NAPI_OK
}

/// 宿主入口（trampoline / loader register）的 escape 回收点：进入时记水位，
/// 回调返回后截断。内层入口只收到自己的水位，外层 escape 产物不受影响。
///
/// # Safety
/// `env` 为本 crate 发出的有效指针。
pub unsafe fn escape_truncate_to(env: napi_env, base: usize) {
    let env_ref = unsafe { e(env) };
    if env_ref.escape_slots.len() > base {
        env_ref.escape_slots.truncate(base);
    }
}
