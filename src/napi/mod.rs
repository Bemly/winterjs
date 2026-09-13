//! napi：host 侧 Node-API 实现（plan-napi.md，napi 分支）。
//!
//! 在 mozjs 上手写 Node-API C ABI（无轮子可引，§0.5 调研见 plan-napi §1）：
//! - `sys.rs`：vendored Node 头（v26.8.2）经 bindgen 生成的类型 + 常量
//!   （函数声明 blocklist——函数由本模块 `#[no_mangle] pub unsafe extern "C" fn`
//!   **定义并导出**，build.rs 再按平台把符号挂进宿主导出表）。
//! - `env.rs`：`NapiEnv` 会话单例（存 `state::RootedState`，随 TLS 线程生灭，
//!   §4.24 哲学）：值槽位 arena（`Box<Heap>` 定址，§4.40 铁律）+ scope 栈 +
//!   addon 库表 + last-error。
//! - `api.rs`：`napi_*` 函数族（M0 子集）+ 单一 trampoline（reserved slots
//!   携带 addon 回调指针/`data`，JS 调用 → C 回调）。
//! - `loader.rs`：`require()` 的 `.node` 截获（`--allow-ffi` 门控）→ dlopen →
//!   `napi_module_register`（constructor 暂存）‖ `napi_register_module_v1`
//!   （dlsym 双入口）→ register(env, exports) → exports 返回 require 管线。
//!
//! 契约与偏差（记档，plan-napi §4）：`napi_value` = `*mut Heap<JSVal>`（对
//! addon 完全不透明，N-API 契约即如此）；M0 槽位 arena 只增不回收（scope 仅
//! 配平校验，M2 引入 escape-aware 回收）；env 跨调用存续（Node 同语义）；
//! 所有 `napi_*` 仅 JS 线程可调（TSFN 的跨线程面在 M3 经事件循环通道）。

pub mod api;
pub mod env;
pub mod loader;
pub mod sys;

use mozjs::jsval::JSVal;
use std::path::Path;

/// `require()` 的 `.node` 入口（`require_value` 截获点）：
/// 权限门控（复用 `--allow-ffi`，用户拍板）→ dlopen + register → exports。
pub fn load(
    cx: &mut mozjs::context::JSContext,
    global: *mut mozjs::jsapi::JSObject,
    spec: &str,
    path: &Path,
) -> Result<JSVal, String> {
    crate::permissions::check_ffi()
        .map_err(|m| format!("PermissionError: loading native module '{spec}': {m}"))?;
    loader::load(cx, global, spec, path)
}
