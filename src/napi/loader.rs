//! `.node` 加载管线：require 截获 → dlopen → 注册 → exports。
//! 注册双入口（plan-napi §1 实测）：`NAPI_MODULE` 宏走 constructor →
//! `napi_module_register` 暂存；手写 addon 直接导出 `napi_register_module_v1`
//! （dlsym 回落）。库会话缓存、永不 dlclose（bun:ffi 同口径）。

use std::path::Path;
use mozjs::jsapi::Heap;
use crate::jsapi_glue::{raw_handle_mut, value_to_string};

use mozjs::jsapi::{JSObject, JS_NewPlainObject};
use mozjs::jsval::{JSVal, ObjectValue, UndefinedValue};
use mozjs::rooted;
use mozjs::context::JSContext;

use crate::napi::sys::{napi_env, napi_value};

/// 加载 `.node`（`napi::load` 的实现；权限门控在 `napi::load` 前置）。
pub fn load(
    cx: &mut JSContext,
    _global: *mut JSObject,
    spec: &str,
    path: &Path,
) -> Result<JSVal, String> {
    let env_ptr = crate::napi::env::ensure(cx);
    // require 幂等（Node 口径）：同 URL 直接回缓存 exports。
    // SAFETY：env 是会话单例（JS 线程专用）。
    if let Some(hit) = unsafe { (*env_ptr).modules.iter().find(|m| m.url == url_str(spec, path)) } {
        let v = hit.exports.get();
        return Ok(v);
    }
    // SAFETY：env 是会话单例（JS 线程专用）；下方借用在 reg 调用前全部结束。
    let env = unsafe { &mut *env_ptr };
    // 库缓存命中 → 复用（constructor 只在首载时跑）。
    let lib_ptr = if let Some((_, lib)) = env.libs.iter().find(|(p, _)| p == path) {
        lib as *const libloading::Library
    } else {
        let lib = unsafe { libloading::Library::new(path) }
            .map_err(|e| format!("cannot load native module '{spec}': {e}"))?;
        env.libs.push((path.to_path_buf(), lib));
        // push 可能扩容 Vec，但 Library 本体是堆 Box——地址恒稳。
        let (_, lib) = env.libs.last().expect("just pushed");
        lib as *const libloading::Library
    };
    let lib = unsafe { &*lib_ptr };
    // 注册函数：constructor 暂存优先（NAPI_MODULE 宏路径），回落 dlsym。
    let pending = crate::napi::env::take_pending_module();
    let reg: unsafe extern "C" fn(napi_env, napi_value) -> napi_value = if let Some(m) = pending {
        // SAFETY：napi_module 为 addon 静态数据，进程内存续。
        let m = unsafe { &*m };
        m.nm_register_func
            .ok_or_else(|| format!("native addon '{spec}' registered without a register function"))?
    } else {
        // SAFETY：符号签名按 node_api.h（napi_addon_register_func）。
        let sym = unsafe { lib.get(b"napi_register_module_v1") }
            .map_err(|e| format!("native addon '{spec}' has no register entry: {e}"))?;
        *sym
    };
    // exports 建 slot → register → rv 非空且非 undefined 即 exports（Node 口径）。
    // SAFETY：JS 对象先 rooted 再落槽（建值与落槽间可能 GC）；env 裸指针借用
    // 不跨 reg 调用（addon 内部经 api.rs 自取 env）。
    unsafe {
        let obj = JS_NewPlainObject(cx.raw_cx());
        if obj.is_null() {
            return Err("napi: cannot create exports object".into());
        }
        rooted!(&in(cx) let obj_root: *mut JSObject = obj);
        let exports = (*env_ptr).put(ObjectValue(obj_root.get()));
        let rv = reg(env_ptr as napi_env, exports);
        // register 期间 addon 可能遗留 pending（register 直调无 trampoline 收口
        // ——遗留即污染后续所有 JSAPI 面，napi-rs ctor 实测拒绝初始化）。
        if mozjs::jsapi::JS_IsExceptionPending(cx.raw_cx()) {
            rooted!(&in(cx) let mut exc = UndefinedValue());
            mozjs::jsapi::JS_GetPendingException(cx.raw_cx(), raw_handle_mut(exc.as_ptr()));
            mozjs::jsapi::JS_ClearPendingException(cx.raw_cx());
            let msg = value_to_string(cx, exc.get());
            return Err(format!("native addon '{spec}' threw during registration: {msg}"));
        }
        let out = if !rv.is_null() && !(*env_ptr).get(rv).is_undefined() {
            (*env_ptr).get(rv)
        } else {
            ObjectValue(obj_root.get())
        };
        (*env_ptr).modules.push(crate::napi::env::NapiModule {
            url: url_str(spec, path),
            exports: Heap::boxed(out),
        });
        tracing::debug!(target: "winterjs::napi", spec, "native module loaded");
        Ok(out)
    }
}

/// 模块缓存键（绝对路径规一；spec 与 path 双参留 spec 语义可读）。
fn url_str(spec: &str, path: &Path) -> String {
    let _ = spec;
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned())
}
