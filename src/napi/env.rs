//! `NapiEnv`：napi 会话单例（`state::RootedState` 成员，随 JS 线程 TLS 生灭）。
//!
//! - 值槽位 arena：`Vec<Box<Heap<JSVal>>>`——`Box` 定址（槽地址恒稳，§4.40），
//!   `Heap` 经 RootedState 的 trace 链入 GC。`napi_value` 即槽位地址
//!   （`*mut Heap<JSVal>` 转 `sys::napi_value`，对 addon 不透明）。
//! - M0 偏差（plan-napi §4）：arena 只增不回收；handle scope 仅配平校验
//!   （M2 引入 escape-aware 回收——Addon 回调产物大水位在 M4 实测后再定）。
//! - addon 库表：`libloading::Library` 会话存活、永不 dlclose（地址稳定，
//!   bun:ffi 同口径）。
//! - `napi_module_register` 暂存：dlopen 的 constructor 在 dlopen 调用栈内
//!   同步执行（JS 线程），thread_local 槽由 loader 随后取走。

use std::cell::RefCell;
use std::ffi::CString;
use std::path::PathBuf;

use mozjs::jsapi::Heap;
use mozjs::jsapi::JSContext as RawJSContext;
use mozjs::jsval::JSVal;
use mozjs::context::JSContext;

use crate::napi::sys;

pub struct NapiEnv {
    /// JS 线程专用 raw cx（会话存续期有效；与 `state::global()` 同生命周期纪律）。
    pub cx: *mut RawJSContext,
    /// 值槽位 arena（GC traced，见模块头注）。
    pub slots: Vec<Box<Heap<JSVal>>>,
    /// handle scope 标记（M0 仅配平校验）。
    pub scopes: Vec<usize>,
    /// 已加载 addon 库（path → Library；永不 dlclose）。
    pub libs: Vec<(PathBuf, libloading::Library)>,
    /// `.node` 模块 exports 缓存（require 幂等，Node 口径；exports 进 GC 图）。
    pub modules: Vec<NapiModule>,
    /// last-error 消息（`napi_get_last_error_info` 的 message 指向此缓冲）。
    pub last_error: Option<CString>,
    /// last-error 静态壳（message 指向 `last_error`；env 存续期稳定）。
    pub err_info: sys::napi_extended_error_info,
}

/// 已加载 `.node` 的 exports 记录（`Box<Heap>` 定址，§4.40）。
pub struct NapiModule {
    pub url: String,
    pub exports: Box<Heap<JSVal>>,
}

// SAFETY（§6 前置条件记录）：NapiEnv 仅 JS 线程访问；`cx` raw 指针与会话
// Runtime 同生命周期（RootedState TLS 随线程生灭，§4.24）；trace 只追 JS 值
// 槽位（cx/libs/scopes 非 JS 值不进 GC 图）。
unsafe impl mozjs::gc::Traceable for NapiEnv {
    unsafe fn trace(&self, trc: *mut mozjs::jsapi::JSTracer) {
        // SAFETY：trace 协议（引擎在 GC 期间调用；Trace trait 同前置）。
        unsafe {
            mozjs::rust::Trace::trace(&self.slots, trc);
            for m in &self.modules {
                m.exports.trace(trc);
            }
        }
    }
}

impl NapiEnv {
    pub fn new(cx: *mut RawJSContext) -> Self {
        NapiEnv {
            cx,
            slots: Vec::new(),
            scopes: Vec::new(),
            libs: Vec::new(),
            modules: Vec::new(),
            last_error: None,
            err_info: sys::napi_extended_error_info {
                error_message: std::ptr::null(),
                engine_reserved: std::ptr::null_mut(),
                engine_error_code: 0,
                error_code: 0,
            },
        }
    }

    /// 建槽位（值进 GC 图）→ `napi_value`（槽地址，§4.40 Box 定址）。
    pub fn put(&mut self, v: JSVal) -> sys::napi_value {
        let slot = Heap::boxed(v);
        // SAFETY：Box 归 arena 所有，堆地址恒稳（§4.40）；const→mut 只读用途。
        let ptr: *mut Heap<JSVal> = &*slot as *const Heap<JSVal> as *mut Heap<JSVal>;
        self.slots.push(slot);
        ptr.cast::<sys::napi_value__>()
    }

    /// 读槽位（`napi_value` → `JSVal`；addon 持有的指针在 scope 存活期内有效）。
    ///
    /// # Safety
    /// `v` 必须是本 env `put` 产生的有效槽位（scope 未关）。
    pub unsafe fn get(&self, v: sys::napi_value) -> JSVal {
        // SAFETY：v 为本 env put 产生的有效槽位（调用方契约，见 # Safety）。
        unsafe { (*(v.cast::<Heap<JSVal>>())).get() }
    }

    /// 记 last-error（`napi_get_last_error_info` 读；占位 code 面 M2 补全）。
    pub fn set_last_error(&mut self, msg: &str) {
        self.last_error = Some(CString::new(msg.replace('\0', " ")).unwrap_or_default());
    }
}

/// napi 回调信息（`napi_callback_info` 本体；回调期间有效，addon 不得留存）。
pub struct CbInfo {
    pub argc: u32,
    /// 实参槽位（已拷入 arena，回调返回后仍有效——Node 同语义）。
    pub argv: Vec<sys::napi_value>,
    pub this: sys::napi_value,
    pub data: *mut std::os::raw::c_void,
}

// SAFETY：CbInfo 持有的都是槽位指针 + addon 自带 data；仅回调栈内存续。
unsafe impl Send for CbInfo {}

// dlopen constructor 暂存（`napi_module_register` 写、loader 读后清）。
thread_local! {
    static PENDING_MODULE: RefCell<Option<*mut sys::napi_module>> = const { RefCell::new(None) };
}

/// `napi_module_register` 的暂存写入（见 api.rs 的导出函数）。
pub fn stash_pending_module(m: *mut sys::napi_module) {
    PENDING_MODULE.with(|p| *p.borrow_mut() = Some(m));
}

/// loader 取走暂存（取走即清，防串号）。
pub fn take_pending_module() -> Option<*mut sys::napi_module> {
    PENDING_MODULE.with(|p| p.borrow_mut().take())
}

/// 取/建会话 env（裸指针出闭包：env 生命周期 = 会话 = RootedState TLS；
/// §4.14 纪律——闭包内只做纯字段操作，不嵌套 TLS 调用）。
pub fn ensure(cx: &mut JSContext) -> *mut NapiEnv {
    // SAFETY：JS 线程内取 raw cx（wrap_cx 同前置）。
    let raw = unsafe { cx.raw_cx() };
    crate::state::with_rooted(|s| {
        let e = s.napi.get_or_insert_with(|| NapiEnv::new(raw));
        e as *mut NapiEnv
    })
}
