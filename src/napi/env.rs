//! `NapiEnv`：napi 会话单例（`state::RootedState` 成员，随 JS 线程 TLS 生灭）。
//!
//! - 值槽位 arena：`Vec<Box<Heap<JSVal>>>`——`Box` 定址（槽地址恒稳，§4.40），
//!   `Heap` 经 RootedState 的 trace 链入 GC。`napi_value` 即槽位地址
//!   （`*mut Heap<JSVal>` 转 `sys::napi_value`，对 addon 不透明）。
//! - 槽位回收（M2）：trampoline 回调按进入水位截断（Node 契约：回调内建值
//!   仅回调存活期有效）；显式 handle scope 由 scope.rs 统一栈管理（close 截
//!   断 + LIFO 校验）；escape 产物入独立池，由宿主入口按水位回收。
//! - addon 库表：`libloading::Library` 会话存活、永不 dlclose（地址稳定，
//!   bun:ffi 同口径）。
//! - `napi_module_register` 暂存：dlopen 的 constructor 在 dlopen 调用栈内
//!   同步执行（JS 线程），thread_local 槽由 loader 随后取走。

use std::cell::RefCell;
use std::ffi::{c_void, CString};
use std::path::PathBuf;

use mozjs::jsapi::Heap;
use mozjs::jsapi::JSContext as RawJSContext;
use mozjs::jsval::JSVal;
use mozjs::context::JSContext;

use crate::napi::sys;
use crate::napi::sys::napi_cleanup_hook;

/// scope 栈条目（M2：统一栈，handle/escapable 同栈严格 LIFO）。
pub struct ScopeEntry {
    /// 开栈时的 arena 水位（close 时截断回收）。
    pub mark: usize,
    /// true = escapable handle scope。
    pub escapable: bool,
    /// escapable 专用：`napi_escape_handle` 已用过（每 scope 一次）。
    pub escaped: bool,
}

pub struct NapiEnv {
    /// JS 线程专用 raw cx（会话存续期有效；与 `state::global()` 同生命周期纪律）。
    pub cx: *mut RawJSContext,
    /// 值槽位 arena（GC traced，见模块头注）。
    pub slots: Vec<Box<Heap<JSVal>>>,
    /// scope 栈（handle/escapable 统一；close 严格 LIFO + 截断回收，M2）。
    pub scopes: Vec<ScopeEntry>,
    /// escape 产物槽（`napi_escape_handle` 的存活区——scope 截断不会波及；
    /// 由各宿主入口（trampoline/loader register）按进入时水位截断回收）。
    pub escape_slots: Vec<Box<Heap<JSVal>>>,
    /// `napi_wrap` 隐藏键（`Symbol.for("__wjs_napi_wrap")`，会话缓存；traced）。
    pub wrap_sym: Option<Box<Heap<JSVal>>>,
    /// `napi_adjust_external_memory` 累计（Node 口径返回累计值）。
    pub external_mem: i64,
    /// 活跃 deferred（`napi_create_promise` 的 promise Heap 槽位；
    /// 句柄 = 槽位地址，落定即摘——一次性，promise.rs）。
    pub deferreds: Vec<Box<Heap<JSVal>>>,
    /// 活跃引用（`napi_ref` 本体；句柄 = Box 指针，refcount.rs；Box 定址铁律）。
    pub refs: Vec<Box<crate::napi::refcount::RefRec>>,
    /// 活跃 async_work（句柄 = id；asyncwork.rs）。
    pub async_works: std::collections::HashMap<u64, crate::napi::asyncwork::AsyncWorkRec>,
    /// 活跃 TSFN（句柄 = id；asyncwork.rs）。
    pub tsfns: std::collections::HashMap<u64, crate::napi::asyncwork::TsfnRec>,
    /// 已排队未完成的 async_work 数（事件循环 keep-alive；JS 线程读写）。
    pub async_pending: usize,
    /// napi 句柄 id 发号器（async_work/TSFN 共用；0 保留）。
    pub next_napi_id: u64,
    /// env cleanup hooks（end_session 收敛点 LIFO 触发，lifecycle.rs）。
    pub cleanup_hooks: Vec<(napi_cleanup_hook, *mut c_void)>,
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
                mozjs::rust::Trace::trace(&self.escape_slots, trc);
                mozjs::rust::Trace::trace(&self.deferreds, trc);
                for r in &self.refs {
                    // weak（refcount==0）同追：SM 无 embedder 弱值通道，不追即
                    // GC 后悬垂（ref.rs 模块头偏差记档）。
                    r.value.trace(trc);
                }
                if let Some(sym) = &self.wrap_sym {
                    sym.trace(trc);
                }
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
            escape_slots: Vec::new(),
            wrap_sym: None,
            external_mem: 0,
            deferreds: Vec::new(),
            refs: Vec::new(),
            async_works: std::collections::HashMap::new(),
            tsfns: std::collections::HashMap::new(),
            async_pending: 0,
            next_napi_id: 0,
            cleanup_hooks: Vec::new(),
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

    /// escape 产物槽（`escape_slots`；地址恒稳，scope 截断不波及）。
    pub fn put_escaped(&mut self, v: JSVal) -> sys::napi_value {
        let slot = Heap::boxed(v);
        // SAFETY：Box 归 escape 池所有，堆地址恒稳（§4.40）。
        let ptr: *mut Heap<JSVal> = &*slot as *const Heap<JSVal> as *mut Heap<JSVal>;
        self.escape_slots.push(slot);
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
    /// 实参槽位（回调期间有效；trampoline 返回时随 mark 截断——Node 同契约）。
    pub argv: Vec<sys::napi_value>,
    pub this: sys::napi_value,
    pub data: *mut std::os::raw::c_void,
    /// new.target 槽位（非构造调用 = undefined 槽；`napi_get_new_target` 读）。
    pub new_target: sys::napi_value,
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
