//! JS 线程私有状态：timers 注册表、console 计数、未处理 rejection 清单、内部辅助函数值。
//! 只允许在 JS 独占线程访问（AGENTS §6）。所有持 JS 值的字段集中在 `RootedState`，
//! 经 `RootedTraceableBox` 整体跨 GC 保活；`PlainState` 不含 GC 指针。

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use mozjs::context::JSContext;
use mozjs::gc::{RootedTraceableBox, Traceable};
use mozjs::jsapi::{Heap, JS_GetFunctionObject, JS_NewFunction, JSObject, JSTracer};
use mozjs::jsval::{JSVal, ObjectValue, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{Frame, get_prop_string, get_prop_u32, value_to_string};
use crate::loader::sourcemap::remap_location;

/// 一个已注册的定时器。`at` 为触发时刻（interval 为上次触发 + 间隔，漂移校正）。
/// `callback`/`args` 经 `Box` 定址（mozjs `Heap::set` 后禁移动，见 §4.39）。
pub struct TimerEntry {
    pub id: u32,
    pub callback: Box<Heap<JSVal>>,
    pub args: Box<Heap<JSVal>>, // JS 数组，由 prelude 打包
    pub at: Instant,
    pub interval: Option<Duration>,
}

// SAFETY: 只追踪 GC 字段；Instant/Duration/u32 无 GC 指针。
unsafe impl Traceable for TimerEntry {
    unsafe fn trace(&self, trc: *mut JSTracer) { unsafe {
        self.callback.trace(trc);
        self.args.trace(trc);
    }}
}

/// 一个已编译的模块：URL（spec 键）+ 跨 GC 保活的模块记录（`Box` 定址，见 §4.39）。
pub struct ModuleEntry {
    pub url: String,
    pub record: Box<Heap<*mut JSObject>>,
}

// SAFETY: 只追踪 record（URL 无 GC 指针）。
unsafe impl Traceable for ModuleEntry {
    unsafe fn trace(&self, trc: *mut JSTracer) { unsafe {
        self.record.trace(trc);
    }}
}

/// 一个已加载的 CJS 模块：URL + 跨 GC 保活的 `module.exports`（循环引用 prefab；`Box` 定址）。
pub struct CjsEntry {
    pub url: String,
    pub exports: Box<Heap<JSVal>>,
}

// SAFETY: 只追踪 exports（URL 无 GC 指针）。
unsafe impl Traceable for CjsEntry {
    unsafe fn trace(&self, trc: *mut JSTracer) { unsafe {
        self.exports.trace(trc);
    }}
}

/// 一路 `fs.watch` 的 JS 监听（事件循环分发时取出，**保留**注册，多次触发；`Box` 定址）。
pub struct WatchCallback {
    pub id: u64,
    pub listener: Box<Heap<JSVal>>,
}

// SAFETY: 只追踪 listener（id 无 GC 指针）。
unsafe impl Traceable for WatchCallback {
    unsafe fn trace(&self, trc: *mut JSTracer) { unsafe {
        self.listener.trace(trc);
    }}
}

/// 一个异步子进程的 JS 目标对象（`onexit/onclose/onerror` 走属性读；close 前保留；`Box` 定址）。
pub struct ChildTarget {
    pub id: u64,
    pub target: Box<Heap<JSVal>>,
}

pub struct NetTarget {
    pub id: u64,
    pub target: Box<Heap<JSVal>>,
}

/// 一个 vm 上下文的独立 global（新 compartment；run 时重进其 realm；`Box` 定址）。
pub struct VmCtx {
    pub id: u64,
    pub global: Box<Heap<*mut JSObject>>,
}

// SAFETY: 只追踪 global（id 无 GC 指针）。
unsafe impl Traceable for VmCtx {
    unsafe fn trace(&self, trc: *mut JSTracer) { unsafe {
        self.global.trace(trc);
    }}
}

// SAFETY: 只追踪 target（id 无 GC 指针）。
unsafe impl Traceable for NetTarget {
    unsafe fn trace(&self, trc: *mut JSTracer) { unsafe {
        self.target.trace(trc);
    }}
}

/// 主会话侧一个运行中 worker 的句柄（发命令/寻址其 parentPort 用）。
pub struct WorkerHandle {
    pub worker_id: u64,
    pub thread_id: u64,
    pub inbox_tx: tokio::sync::mpsc::UnboundedSender<crate::builtins::node::worker::WorkerEvent>,
    pub parent_port: u64,
    pub counted: bool,
    pub exited: bool,
}

/// 一个运行中 worker 的 JS 目标（`message/error/exit/online` 走 `__ev`；Exit 后摘除）。
pub struct WorkerTarget {
    pub id: u64,
    pub target: Box<Heap<JSVal>>,
}

// SAFETY: 只追踪 target（id 无 GC 指针）。
unsafe impl Traceable for WorkerTarget {
    unsafe fn trace(&self, trc: *mut JSTracer) { unsafe {
        self.target.trace(trc);
    }}
}
/// 计数规则（Node paused 口径）：`counted = open && refed && listening`，
/// 只有正在监听的端口才续命事件循环（`port_listen/unlisten` 由 JS 监听装卸驱动）。
pub struct WorkerPort {
    pub id: u64,
    pub peer: u64,
    pub peer_tx: tokio::sync::mpsc::UnboundedSender<crate::builtins::node::worker::WorkerEvent>,
    pub target: Option<Box<Heap<JSVal>>>,
    pub open: bool,
    pub refed: bool,
    pub listening: bool,
    pub counted: bool,
    pub peer_closed: bool,
    /// 对端是 worker（parentPort→Worker 对象，`WMsg` 变体），而非普通端口。
    pub peer_is_worker: bool,
}

// SAFETY: 只追踪 target（通道/旗无 GC 指针）。
unsafe impl Traceable for WorkerPort {
    unsafe fn trace(&self, trc: *mut JSTracer) { unsafe {
        self.target.trace(trc);
    }}
}

/// socket/server 表项（写端命令通道 + 半关旗；收尾单出口见 node/net.rs）。
pub struct NetEntry {
    pub cmd_tx: tokio::sync::mpsc::UnboundedSender<crate::builtins::node::net::NetCmd>,
    pub half_read: bool,
    pub half_write: bool,
    pub close_sent: bool,
    /// 写端 task 是否存活（destroy 后死亡；读端见 EOF 时若已死则直接收尾）。
    pub writer_alive: bool,
}

// SAFETY: 只追踪 target（id 无 GC 指针）。
unsafe impl Traceable for ChildTarget {
    unsafe fn trace(&self, trc: *mut JSTracer) { unsafe {
        self.target.trace(trc);
    }}
}

/// 模块调试信息（纯 Rust，无 GC 指针）：报错时按文件名找回原始源码回映射。
#[derive(Default, Clone)]
pub struct ModuleDebug {
    /// 转译前源码（.ts 原文；.js 即求值源码本身）。
    pub original: String,
    /// sourcemap JSON（TS 转译产物；JS 源为 None）。
    pub map: Option<String>,
}

/// 一个未决 fetch 的 resolve/reject（事件循环结算时取出并移除；`Box` 定址）。
pub struct FetchCallback {
    pub id: u64,
    pub resolve: Box<Heap<JSVal>>,
    pub reject: Box<Heap<JSVal>>,
}

// SAFETY: 只追踪两个回调值（id 无 GC 指针）。
unsafe impl Traceable for FetchCallback {
    unsafe fn trace(&self, trc: *mut JSTracer) { unsafe {
        self.resolve.trace(trc);
        self.reject.trace(trc);
    }}
}

/// 流式 body 的等待 pull（resolve/reject 存 RootedState 被 GC 追踪；`Box` 定址）。
pub struct StreamWaiter {
    pub resolve: Box<Heap<JSVal>>,
    pub reject: Box<Heap<JSVal>>,
}

// SAFETY: 只追踪两个回调值。
unsafe impl Traceable for StreamWaiter {
    unsafe fn trace(&self, trc: *mut JSTracer) { unsafe {
        self.resolve.trace(trc);
        self.reject.trace(trc);
    }}
}

/// 一条流式 body 的 Rust 侧状态（chunk/等待者/终态；字节无 GC 指针）。
pub struct FetchStreamState {
    pub id: u64,
    pub chunks: std::collections::VecDeque<Vec<u8>>,
    pub waiters: Vec<StreamWaiter>,
    pub done: bool,
    pub error: Option<String>,
}

// SAFETY: 只追踪 waiters（id/chunks/done/error 无 GC 指针）。
unsafe impl Traceable for FetchStreamState {
    unsafe fn trace(&self, trc: *mut JSTracer) { unsafe {
        self.waiters.trace(trc);
    }}
}

/// 全部跨 GC 存活的 JS 值。
#[derive(Default)]
pub struct RootedState {
    pub timers: Vec<TimerEntry>,
    pub unhandled: Vec<Box<Heap<*mut JSObject>>>, // 未处理 rejection 的 promise（`Box` 定址）
    pub call_fn: Heap<JSVal>,                // prelude 的 __wjs_call(cb, args)
    pub entries_fn: Heap<JSVal>,             // prelude 的 __wjs_entries(v)
    pub on_fulfilled: Heap<JSVal>,           // rejection 捕获用 native
    pub on_rejected: Heap<JSVal>,
    pub entry_fulfilled: Heap<JSVal>, // 模块入口 TLA 决议捕获用 native
    pub entry_rejected: Heap<JSVal>,
    pub modules: Vec<ModuleEntry>, // URL → 已编译模块记录（循环/去重，spec 同结果）
    pub cjs_modules: Vec<CjsEntry>, // URL → CJS `module.exports`（执行前预注册，循环可见半成品）
    pub watch_listeners: Vec<WatchCallback>, // fs.watch 监听（close 前保留，多次分发）
    pub child_targets: Vec<ChildTarget>, // 异步子进程目标（exit/close 后摘除）
    pub net_targets: Vec<NetTarget>, // node:net 目标（Close 后摘除）
    pub worker_ports: Vec<WorkerPort>, // worker MessagePort 端（close 后摘除）
    pub worker_targets: Vec<WorkerTarget>, // 运行中 worker 的 JS 目标（Exit 后摘除）
    pub vm_contexts: Vec<VmCtx>, // node:vm 上下文 global（release 摘除，会话终由 OS 回收）
    pub fetch_callbacks: Vec<FetchCallback>, // 未决 fetch 的 resolve/reject（按 id 取出）
    pub fetch_streams: Vec<FetchStreamState>, // 流式 body（chunk 泵；cancel/终态时移除）
    pub make_response_fn: Heap<JSVal>, // prelude 的 __wjs_make_response
    pub make_fetch_error_fn: Heap<JSVal>, // prelude 的 __wjs_make_fetch_error
    pub ws_emit_fn: Heap<JSVal>, // prelude 的 __wjs_ws_emit
}

// SAFETY: 同 TimerEntry，全字段 Traceable 或无 GC 指针。
unsafe impl Traceable for RootedState {
    unsafe fn trace(&self, trc: *mut JSTracer) { unsafe {
        self.timers.trace(trc);
        self.unhandled.trace(trc);
        self.call_fn.trace(trc);
        self.entries_fn.trace(trc);
        self.on_fulfilled.trace(trc);
        self.on_rejected.trace(trc);
        self.entry_fulfilled.trace(trc);
        self.entry_rejected.trace(trc);
        self.modules.trace(trc);
        self.cjs_modules.trace(trc);
        self.watch_listeners.trace(trc);
        self.child_targets.trace(trc);
        self.net_targets.trace(trc);
        self.worker_ports.trace(trc);
        self.worker_targets.trace(trc);
        self.vm_contexts.trace(trc);
        self.fetch_callbacks.trace(trc);
        self.fetch_streams.trace(trc);
        self.make_response_fn.trace(trc);
        self.make_fetch_error_fn.trace(trc);
        self.ws_emit_fn.trace(trc);
    }}
}

/// 不含 GC 指针的状态。
#[derive(Default)]
pub struct PlainState {
    pub next_timer_id: u32,
    pub cleared_during_fire: HashSet<u32>,
    pub console_counts: HashMap<String, u32>,
    pub console_times: HashMap<String, Instant>,
    pub console_indent: usize,
    pub rejection_reasons: Vec<String>,
    /// 模块入口 TLA 决议（`entry_*_native` 记录，`runtime` 收割，与通用捕获隔离）。
    pub entry_fulfillment: Option<String>,
    pub entry_rejection: Option<String>,
    /// 模块调试信息（URL → 原始源码/转译产物/sourcemap；报错回映射用）。
    pub module_debug: HashMap<String, ModuleDebug>,
    /// 模块加载 hook 暂存的友好错误（hook 返回 false，中断加载后由外层取出上报）。
    pub module_load_error: Option<crate::error::Error>,
    /// fetch 驱动端点（`run()` 初始化；接收端由事件循环持有，无 JS 值，可跨 await）。
    pub fetch_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::fetch::FetchMsg>>,
    pub fetch_next_id: u64,
    pub fetch_pending: usize,
    /// 未决 fetch 的任务句柄（`fetch_abort` 取消用；结算/取消时移除，不参与计数）。
    pub fetch_tasks: HashMap<u64, tokio::task::AbortHandle>,
    /// fs.watch 驱动端点（接收端由事件循环持有；watcher 本体同表保活）。
    pub watch_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::node::fs::WatchEvent>>,
    pub watch_next_id: u64,
    /// 存活 watch 数（仅 persistent 计数；事件循环退出条件用）。
    pub watch_open: usize,
    pub watch_drivers: HashMap<u64, (notify::RecommendedWatcher, bool)>,
    /// 异步子进程驱动端点（接收端由事件循环持有；Child 本体同表保活供 kill）。
    pub child_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::node::child::ChildEvent>>,
    pub child_next_id: u64,
    /// 存活子进程数（exit/close 结算时减；事件循环退出条件用）。
    pub child_open: usize,
    pub child_procs: HashMap<u64, ChildEntry>,
    /// 网络驱动端点（node:net；接收端由事件循环持有）。
    pub net_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::node::net::NetEvent>>,
    pub net_next_id: u64,
    /// 存活 socket/server 数（Close 结算时减；事件循环退出条件用）。
    pub net_open: usize,
    pub net_sockets: HashMap<u64, NetEntry>,
    /// worker 驱动端点（MessagePort/Worker；接收端由事件循环持有）。
    pub worker_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::node::worker::WorkerEvent>>,
    pub worker_next_id: u64,
    /// 存活计数（ref'd 端口 + 运行中 worker；事件循环退出条件用）。
    pub worker_open: usize,
    /// 本会话线程身份（主=true/0；worker 线程由 spawn 侧显式改写，默认 false 须经
    /// `worker_session_init` 矫正——init_session 对每个会话都调一次）。
    pub worker_is_main: bool,
    pub worker_thread_id: u64,
    /// worker 线程专有（主会话为 None）：克隆入参 JSON + 父端口 id。
    pub worker_data_json: Option<String>,
    pub worker_parent_port: Option<u64>,
    /// worker 终止旗（`WTerminate` 到达置位；事件循环检查点退出，见 §4.18 顺序）。
    pub worker_terminated: bool,
    /// 主会话的 worker 句柄表（worker_id → 发往 worker 会话收件箱的端点等）。
    pub worker_handles: HashMap<u64, WorkerHandle>,
    /// worker 线程专有：主会话收件箱（WOnline/WMsg/WError/WExit 发往此处）。
    pub worker_main_inbox: Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::node::worker::WorkerEvent>>,
    /// worker 线程专有：spawn 侧 rendezvous（回传本会话收件箱 + parentPort，一次性）。
    pub worker_rendezvous: Option<std::sync::mpsc::Sender<(
        tokio::sync::mpsc::UnboundedSender<crate::builtins::node::worker::WorkerEvent>,
        u64,
    )>>,
    /// vm 上下文 id 分配（单调；release 不复用，与 fd 表同哲学）。
    pub vm_next_id: u64,
    /// WebSocket 驱动端点（同上）+ 发送端表 + 存活计数（事件循环退出条件用）。
    pub ws_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::ws::WsEvent>>,
    pub ws_next_id: u64,
    pub ws_open: usize,
    pub ws_sinks: HashMap<u64, tokio::sync::mpsc::UnboundedSender<crate::builtins::ws::WsOut>>,
    /// bun:sqlite worker 表（每 Database 一条线程；req/resp channel，见 bun/sqlite.rs）。
    pub sqlite_next_id: u64,
    pub sqlite_workers: HashMap<u64, crate::builtins::bun::sqlite::SqliteWorker>,
    /// eval 包装（async IIFE）引入的行偏移，报错行号统一校正。
    pub line_adjust: u32,
    /// 全局对象裸指针。前置条件：run() 里的 rooted! global 活过整个事件循环，
    /// 本指针只是它的借用副本，绝不在此之外解引用。
    pub global: *mut JSObject,
    /// `process.argv` 全量（含 execPath/脚本位；prelude 经 JSON 桥读）。
    pub argv: Vec<String>,
    /// `process.exitCode`（None=未设→0；收尾映射 `Error::Exit`）。
    pub exit_code: Option<i32>,
    /// `process.exit()` 已调用（哨兵码；哨兵错被用户 catch 也照退，检查点强制）。
    pub process_exited: Option<i32>,
    /// 已求值的 ESM（`require(node:)` 复用时跳过二次求值；入口求值后也记）。
    pub evaluated_modules: HashSet<String>,
    /// 主模块 URL（`.cjs` 入口经 require 起；`require.main` 用）。
    pub main_module: Option<String>,
}

thread_local! {
    static ROOTED: RefCell<Option<RootedTraceableBox<RefCell<RootedState>>>> = RefCell::new(None);
    static PLAIN: RefCell<PlainState> = RefCell::new(PlainState::default());
}

pub fn line_adjust() -> u32 {
    PLAIN.with(|p| p.borrow().line_adjust)
}

pub fn set_line_adjust(n: u32) {
    PLAIN.with(|p| p.borrow_mut().line_adjust = n);
}

pub fn set_global(global: *mut JSObject) {
    PLAIN.with(|p| p.borrow_mut().global = global);
}

pub fn global() -> *mut JSObject {
    PLAIN.with(|p| p.borrow().global)
}

/// 引擎初始化后、首段脚本前调用一次。创建跨 GC 根与 rejection 捕获 natives。
pub fn init(cx: &mut JSContext) {
    ROOTED.with(|r| {
        let mut slot = r.borrow_mut();
        if slot.is_some() {
            return;
        }
        let boxed = RootedTraceableBox::new(RefCell::new(RootedState::default()));
        // SAFETY: cx 为引擎当前线程的活跃 context；raw 调用不触发 GC
        unsafe {
            let rcx = cx.raw_cx();
            let on_fulfilled = JS_NewFunction(
                rcx,
                Some(on_fulfilled_native),
                0,
                0,
                c"__wjs_onFulfilled".as_ptr(),
            );
            let on_rejected = JS_NewFunction(
                rcx,
                Some(on_rejected_native),
                1,
                0,
                c"__wjs_onRejected".as_ptr(),
            );
            let entry_fulfilled = JS_NewFunction(
                rcx,
                Some(entry_fulfilled_native),
                1,
                0,
                c"__wjs_entryFulfilled".as_ptr(),
            );
            let entry_rejected = JS_NewFunction(
                rcx,
                Some(entry_rejected_native),
                1,
                0,
                c"__wjs_entryRejected".as_ptr(),
            );
            assert!(
                !on_fulfilled.is_null()
                    && !on_rejected.is_null()
                    && !entry_fulfilled.is_null()
                    && !entry_rejected.is_null(),
                "capture natives"
            );
            {
                let s = boxed.borrow_mut();
                s.on_fulfilled.set(ObjectValue(JS_GetFunctionObject(on_fulfilled)));
                s.on_rejected.set(ObjectValue(JS_GetFunctionObject(on_rejected)));
                s.entry_fulfilled.set(ObjectValue(JS_GetFunctionObject(entry_fulfilled)));
                s.entry_rejected.set(ObjectValue(JS_GetFunctionObject(entry_rejected)));
            }
        }
        *slot = Some(boxed);
    });
}

/// 必须在引擎仍存活时调用（run() 结束前，经 StateGuard）：把 RootedTraceableBox
/// 从 thread_local 摘除并就地销毁，避免 TLS 析构晚于引擎导致的 SEGV/abort。
pub fn shutdown() {
    // 刻意泄漏 RootedState：Runtime 的 StoreBuffer 记有指向这些 Heap 槽位的边，
    // 若在引擎销毁前 drop 槽位，destroyRuntime 的小 GC 会解引用悬垂边而 SEGV。
    // 进程退出时由 OS 回收（AGENTS §4.8）。
    ROOTED.with(|r| {
        if let Some(boxed) = r.borrow_mut().take() {
            std::mem::forget(boxed);
        }
    });
}

/// run() 作用域守卫：无论正常返回还是 `?` 提前返回，都在引擎销毁前拆除 TLS 状态。
pub struct StateGuard;

impl Drop for StateGuard {
    fn drop(&mut self) {
        shutdown();
    }
}

pub fn with_rooted<R>(f: impl FnOnce(&mut RootedState) -> R) -> R {
    ROOTED.with(|r| {
        let slot = r.borrow_mut();
        let box_ref = slot.as_ref().expect("state::init not called");
        let mut s = box_ref.borrow_mut();
        f(&mut s)
    })
}

pub fn with_plain<R>(f: impl FnOnce(&mut PlainState) -> R) -> R {
    PLAIN.with(|p| f(&mut p.borrow_mut()))
}

pub fn next_timer_id() -> u32 {
    with_plain(|p| {
        p.next_timer_id += 1;
        p.next_timer_id
    })
}

// ── rejection 捕获 natives ─────────────────────────────────────────────

/// SAFETY: 由引擎以有效调用帧调用（JSNative 约定）。
unsafe extern "C" fn on_fulfilled_native(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool { unsafe {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper（JS 线程单实例）
    let mut cx = JSContext::from_ptr(std::ptr::NonNull::new_unchecked(cx_raw));
    let frame = Frame::from_raw(vp, argc);
    frame.set_rval(UndefinedValue());
    let _ = &mut cx;
    true
}}

/// SAFETY: 同上；arg0 为 rejection reason。
unsafe extern "C" fn on_rejected_native(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool { unsafe {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = JSContext::from_ptr(std::ptr::NonNull::new_unchecked(cx_raw));
    let frame = Frame::from_raw(vp, argc);
    let reason = if argc > 0 { frame.arg(0) } else { UndefinedValue() };
    let s = value_to_string(&mut cx, reason);
    with_plain(|p| p.rejection_reasons.push(s));
    frame.set_rval(UndefinedValue());
    true
}}

/// 供 runtime 在事件循环收尾把捕获 natives 挂到未处理 promise 上。
pub fn capture_native_values() -> (JSVal, JSVal) {
    with_rooted(|s| (s.on_fulfilled.get(), s.on_rejected.get()))
}

/// 供 runtime 把入口 TLA promise 的决议收割到专用槽（与通用捕获隔离）。
pub fn entry_native_values() -> (JSVal, JSVal) {
    with_rooted(|s| (s.entry_fulfilled.get(), s.entry_rejected.get()))
}

/// 存入一组 fetch 回调（调用方已分配 id）。
pub fn push_fetch_callback(id: u64, resolve: JSVal, reject: JSVal) {
    with_rooted(|s| {
        // `Heap::boxed` 定址（set 后禁移动，见 §4.39；Vec push 会搬运元素）。
        s.fetch_callbacks.push(FetchCallback {
            id,
            resolve: Heap::boxed(resolve),
            reject: Heap::boxed(reject),
        });
    });
}

/// 取出并移除一组 fetch 回调（结算用；未知 id 返回 None）。
pub fn take_fetch_callback(id: u64) -> Option<(JSVal, JSVal)> {
    with_rooted(|s| {
        s.fetch_callbacks
            .iter()
            .position(|c| c.id == id)
            .map(|i| {
                let c = s.fetch_callbacks.remove(i);
                (c.resolve.get(), c.reject.get())
            })
    })
}

/// 分配 fetch id 并计数（调用方随后 spawn 任务；失败路径须配套 `fetch_unpend`）。
pub fn fetch_alloc() -> Option<(u64, tokio::sync::mpsc::UnboundedSender<crate::builtins::fetch::FetchMsg>)> {
    with_plain(|p| {
        let tx = p.fetch_tx.clone()?;
        p.fetch_next_id += 1;
        let id = p.fetch_next_id;
        p.fetch_pending += 1;
        Some((id, tx))
    })
}

/// fetch 计数减一（结算/取消时调用；与 `fetch_alloc` 严格配对，见 `fetch_abort` 注释）。
pub fn fetch_unpend() {
    with_plain(|p| p.fetch_pending = p.fetch_pending.saturating_sub(1));
}

/// 登记任务句柄（spawn 后调用；建任务失败路径不调，调用方配套 `fetch_unpend`）。
pub fn fetch_task(id: u64, handle: tokio::task::AbortHandle) {
    with_plain(|p| {
        p.fetch_tasks.insert(id, handle);
    });
}

/// 任务完成记账（句柄移除；计数由回调摘除侧负责，见下）。
pub fn fetch_task_done(id: u64) {
    with_plain(|p| {
        p.fetch_tasks.remove(&id);
    });
}

/// 取消未决 fetch（幂等；未知 id 静默成功）。
/// 计数规则：`fetch_pending` 与回调摘除严格配对——`settle` 取走回调时减、
/// 此处取走回调时减；`settle` 的 None 分支（回调已被此处取走）不再减。
/// 残留消息（取消时已在途）落到 None 分支即丢弃，无副作用。
/// 流等待者不归此处：调用方（`fetch::fetch_abort`）经 `stream_abort` 取走并拒绝。
pub fn fetch_abort(id: u64) {
    if let Some(handle) = with_plain(|p| p.fetch_tasks.remove(&id)) {
        handle.abort();
    }
    let removed = with_rooted(|s| {
        s.fetch_callbacks
            .iter()
            .position(|c| c.id == id)
            .map(|i| s.fetch_callbacks.remove(i))
            .is_some()
    });
    if removed {
        fetch_unpend();
    }
}

// ── 流式 body 状态（事件循环 + pull 两侧）──────────────────────────────────
// 背压说明：channel 无界 + Rust 侧 VecDeque 无界（消费慢即堆内存；与 prelude
// streams 的 tee 简化同哲学，文档记录，不静默丢数据）。

/// 响应头到达时建流（`deliver` 的 stream 分支；重复建幂等覆盖）。
pub fn stream_add(id: u64) {
    with_rooted(|s| {
        if !s.fetch_streams.iter().any(|st| st.id == id) {
            s.fetch_streams.push(FetchStreamState {
                id,
                chunks: std::collections::VecDeque::new(),
                waiters: Vec::new(),
                done: false,
                error: None,
            });
        }
    });
}

/// pull 动作（纯状态操作；JS 调用由 native 侧执行，避开 TLS 嵌套借用 §4.14）。
pub enum StreamPull {
    /// 立即给 chunk。
    Chunk(Vec<u8>),
    /// 流已终结（done/未知 id/cancel 后）→ resolve null。
    Done,
    /// 流已失败 → reject。
    Failed(String),
    /// 已排队（native 不再调用）。
    Queued,
}

/// prelude `pull` 进（resolve/reject 为 JS 函数值；未知 id 按 Done 处理）。
/// 终结后全消费的流此处摘除（内存有界；见 `stream_on_finish` 语义注释）。
pub fn stream_pull(id: u64, resolve: JSVal, reject: JSVal) -> StreamPull {
    with_rooted(|s| {
        let Some(pos) = s.fetch_streams.iter().position(|st| st.id == id) else {
            return StreamPull::Done;
        };
        // 先给缓冲 chunk（终结与否皆然；终结态的残留缓冲照常交付）。
        if let Some(chunk) = s.fetch_streams[pos].chunks.pop_front() {
            return StreamPull::Chunk(chunk);
        }
        if s.fetch_streams[pos].done {
            // 全消费即摘除；后继 pull 走未知 id → Done。
            if s.fetch_streams[pos].waiters.is_empty() {
                s.fetch_streams.remove(pos);
            }
            return StreamPull::Done;
        }
        if let Some(err) = s.fetch_streams[pos].error.clone() {
            return StreamPull::Failed(err);
        }
        s.fetch_streams[pos].waiters.push(StreamWaiter {
            resolve: Heap::boxed(resolve),
            reject: Heap::boxed(reject),
        });
        StreamPull::Queued
    })
}

/// chunk 到达的泵动作（调用方执行 JS 回调）。
pub enum StreamPump {
    /// 唤醒首个等待者给 chunk。
    Wake(JSVal, Vec<u8>),
    /// 终结交付：各等待者按序拿残留 chunk，拿不到的 resolve-null。
    DoneAll(Vec<(JSVal, Option<Vec<u8>>)>),
    /// 全部等待者 reject（失败终结）。
    FailAll(Vec<JSVal>, String),
    /// 缓存/丢弃（无动作）。
    Buffered,
}

/// chunk 到达（事件循环；未知/已终结流即丢弃——cancel 后残留或 Done 后多发）。
pub fn stream_on_chunk(id: u64, chunk: Vec<u8>) -> StreamPump {
    with_rooted(|s| {
        let Some(st) = s.fetch_streams.iter_mut().find(|st| st.id == id) else {
            return StreamPump::Buffered;
        };
        if st.done || st.error.is_some() {
            return StreamPump::Buffered;
        }
        if !st.waiters.is_empty() {
            let w = st.waiters.remove(0);
            return StreamPump::Wake(w.resolve.get(), chunk);
        }
        st.chunks.push_back(chunk);
        StreamPump::Buffered
    })
}

/// 终态语义（事件循环）：
/// - Done：等待者按序分残留 chunk，分不到的 resolve-null；残留缓冲保留给后继 pull；
///   流标 done 保留（后继 pull 消费完即摘，见 `stream_pull`）。
/// - Failed：等待者全 reject；流标 error 保留（后继 pull 照常 reject）。
/// 存活计数（`stream_pending`）只看未终结流：终结流不续命事件循环（类比 Node
/// 的 EOF socket：无人消费的残留数据随进程退出丢弃，文档记录）。
pub fn stream_on_finish(id: u64, err: Option<String>) -> StreamPump {
    with_rooted(|s| {
        let Some(pos) = s.fetch_streams.iter().position(|st| st.id == id) else {
            return StreamPump::Buffered;
        };
        if let Some(e) = err {
            let st = &mut s.fetch_streams[pos];
            st.error = Some(e.clone());
            let rejects: Vec<JSVal> = st.waiters.drain(..).map(|w| w.reject.get()).collect();
            return StreamPump::FailAll(rejects, e);
        }
        let st = &mut s.fetch_streams[pos];
        st.done = true;
        let mut out = Vec::with_capacity(st.waiters.len());
        for w in st.waiters.drain(..) {
            let chunk = st.chunks.pop_front();
            out.push((w.resolve.get(), chunk));
        }
        StreamPump::DoneAll(out)
    })
}

/// 中止流并取走全部等待者（调用方负责 reject；残留缓冲一并丢弃）。
/// 返回 `(found, waiters)`。cancel 与 `fetch::fetch_abort` 共用。
pub fn stream_abort(id: u64) -> (bool, Vec<(JSVal, JSVal)>) {
    with_rooted(|s| {
        let Some(pos) = s.fetch_streams.iter().position(|st| st.id == id) else {
            return (false, Vec::new());
        };
        let mut st = s.fetch_streams.remove(pos);
        let waiters: Vec<(JSVal, JSVal)> =
            st.waiters.drain(..).map(|w| (w.resolve.get(), w.reject.get())).collect();
        (true, waiters)
    })
}

/// 存活流数（任务未终结；事件循环退出条件用，语义见 `stream_on_finish`）。
pub fn stream_pending() -> usize {
    with_rooted(|s| s.fetch_streams.iter().filter(|st| !st.done && st.error.is_none()).count())
}

/// 未决 fetch 数（事件循环退出条件用）。
pub fn fetch_pending() -> usize {
    with_plain(|p| p.fetch_pending)
}

/// 分配 WebSocket id（失败路径无需配套调用，尚未计数）。
pub fn ws_alloc() -> Option<(u64, tokio::sync::mpsc::UnboundedSender<crate::builtins::ws::WsEvent>)> {
    with_plain(|p| {
        let tx = p.ws_tx.clone()?;
        p.ws_next_id += 1;
        let id = p.ws_next_id;
        p.ws_open += 1;
        Some((id, tx))
    })
}

/// 登记发送端。
pub fn ws_add_sink(id: u64, tx: tokio::sync::mpsc::UnboundedSender<crate::builtins::ws::WsOut>) {
    with_plain(|p| {
        p.ws_sinks.insert(id, tx);
    });
}

/// 发出消息（未知 id 返回 false）；close 同通道（未知 id 静默成功，幂等）。
pub fn ws_send(id: u64, msg: crate::builtins::ws::WsOut) -> bool {
    with_plain(|p| p.ws_sinks.get(&id).map(|tx| tx.send(msg).is_ok()).unwrap_or(false))
}

pub fn ws_close(id: u64, code: u16, reason: String) {
    with_plain(|p| {
        if let Some(tx) = p.ws_sinks.get(&id) {
            let _ = tx.send(crate::builtins::ws::WsOut::Close { code, reason });
        }
    });
}

/// 清理发送端 + target + 存活计数（close/error 结算时调用）。
pub fn ws_remove(id: u64) {
    with_plain(|p| {
        p.ws_sinks.remove(&id);
        p.ws_open = p.ws_open.saturating_sub(1);
    });
}

/// 存活 WebSocket 数（事件循环退出条件用）。
pub fn ws_open() -> usize {
    with_plain(|p| p.ws_open)
}

// ── bun:sqlite worker 表（natives 阻塞往返，见 bun/sqlite.rs）───────────────

/// 登记 worker 并分配 id。
pub fn sqlite_add(worker: crate::builtins::bun::sqlite::SqliteWorker) -> u64 {
    with_plain(|p| {
        p.sqlite_next_id += 1;
        let id = p.sqlite_next_id;
        p.sqlite_workers.insert(id, worker);
        id
    })
}

/// 取 worker 端点（Clone 出用；channel 均可 Clone，不持 TLS 借用做阻塞 IO）。
pub fn sqlite_worker(id: u64) -> Option<crate::builtins::bun::sqlite::SqliteWorker> {
    with_plain(|p| p.sqlite_workers.get(&id).map(|w| crate::builtins::bun::sqlite::SqliteWorker {
        req_tx: w.req_tx.clone(),
        resp_rx: w.resp_rx.clone(),
    }))
}

/// 摘除 worker（close 调用；drop 掉的 req_tx 让线程自退）。
pub fn sqlite_remove(id: u64) {
    with_plain(|p| {
        p.sqlite_workers.remove(&id);
    });
}

/// 会话重置（`init_session` 调用）：上一会话的 worker 端点全数 drop，
/// 线程在 channel 断开后自退（PlainState 跨 run 复用，见本文件头注）。
pub fn sqlite_reset() {
    with_plain(|p| {
        p.sqlite_workers.clear();
    });
}

// ── 模块入口 TLA 决议捕获 natives ────────────────────────────────────────

/// SAFETY: 由引擎以有效调用帧调用；arg0 为 resolution 值。
unsafe extern "C" fn entry_fulfilled_native(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool { unsafe {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = JSContext::from_ptr(std::ptr::NonNull::new_unchecked(cx_raw));
    let frame = Frame::from_raw(vp, argc);
    if argc > 0 {
        let s = value_to_string(&mut cx, frame.arg(0));
        with_plain(|p| p.entry_fulfillment = Some(s));
    }
    frame.set_rval(UndefinedValue());
    true
}}

/// SAFETY: 同上；arg0 为 rejection reason。Error 对象提 file/line/col（TS 回映射），
/// 非对象值退化 ToString（无位置）。
unsafe extern "C" fn entry_rejected_native(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool { unsafe {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = JSContext::from_ptr(std::ptr::NonNull::new_unchecked(cx_raw));
    let frame = Frame::from_raw(vp, argc);
    let reason = if argc > 0 { frame.arg(0) } else { UndefinedValue() };
    // 先算串再进 with_plain：entry_reason_string 内部查 module_debug 也走 with_plain，嵌套即 panic
    let s = entry_reason_string(&mut cx, reason);
    with_plain(|p| p.entry_rejection = Some(s));
    frame.set_rval(UndefinedValue());
    true
}}

/// rejection reason → `file:line:col: message`（无位置信息时退化为值串）。
fn entry_reason_string(cx: &mut JSContext, reason: JSVal) -> String {
    if !reason.is_object() {
        return value_to_string(cx, reason);
    }
    let obj = reason.to_object();
    rooted!(&in(cx) let obj_root: *mut JSObject = obj);
    let message =
        get_prop_string(cx, obj_root.get(), c"message").unwrap_or_else(|| value_to_string(cx, reason));
    let file = get_prop_string(cx, obj_root.get(), c"fileName").unwrap_or_default();
    if file.is_empty() {
        return message;
    }
    let line = get_prop_u32(cx, obj_root.get(), c"lineNumber").unwrap_or(1).max(1);
    let col = get_prop_u32(cx, obj_root.get(), c"columnNumber").unwrap_or(1).max(1);
    let map = with_plain(|p| p.module_debug.get(&file).and_then(|d| d.map.clone()));
    let (line, col) = remap_location(map.as_deref(), line, col);
    format!("{file}:{line}:{col}: {message}")
}

/// console 计数等纯 Rust 状态访问（builtins 用）。
pub fn console_state<R>(f: impl FnOnce(&mut PlainState) -> R) -> R {
    with_plain(f)
}

// ── process 状态（argv/exitCode/exit，见 plan Phase 4）──────────────────────
/// run() 入口存 argv（`[execPath, script, ...extras]`；eval 为 `[execPath, ...extras]`）。
pub fn set_argv(argv: Vec<String>) {
    with_plain(|p| p.argv = argv);
}

/// `process.exitCode` 读（None→0 口径由调用方定；此处原样返回）。
pub fn exit_code() -> Option<i32> {
    with_plain(|p| p.exit_code)
}

/// `process.exitCode = n`（截断 i32；Node 要求整数，此处由 prelude 校验）。
pub fn set_exit_code(code: i32) {
    with_plain(|p| p.exit_code = Some(code));
}

// ── CJS 缓存 / ESM 求值集 / 主模块（`require` 用，见 plan Phase 4d）─────────
/// CJS 导出命中（clone 出值；调用方 rooted 化）。
pub fn cjs_find(url: &str) -> Option<JSVal> {
    with_rooted(|s| s.cjs_modules.iter().find(|m| m.url == url).map(|m| m.exports.get()))
}

/// CJS 预注册（执行前占位，循环可见半成品；重复注册保留首个）。
pub fn cjs_register(url: String, exports: JSVal) {
    with_rooted(|s| {
        if !s.cjs_modules.iter().any(|m| m.url == url) {
            s.cjs_modules.push(CjsEntry { url, exports: Heap::boxed(exports) });
        }
    });
}

/// CJS 移除（执行失败清场，Node 同语义）。
pub fn cjs_remove(url: &str) {
    with_rooted(|s| {
        s.cjs_modules.retain(|m| m.url != url);
    });
}

/// ESM 已求值查询/标记（`require(node:)` 跳过二次求值用）。
pub fn module_evaluated(url: &str) -> bool {
    with_plain(|p| p.evaluated_modules.contains(url))
}

/// ESM 求值标记。
pub fn set_module_evaluated(url: String) {
    with_plain(|p| {
        p.evaluated_modules.insert(url);
    });
}

/// 主模块登记/读取（`.cjs` 入口；`require.main` 用）。
pub fn set_main_module(url: String) {
    with_plain(|p| p.main_module = Some(url));
}

/// 主模块 URL（无则 None）。
pub fn main_module() -> Option<String> {
    with_plain(|p| p.main_module.clone())
}

// ── fs.watch 驱动（`notify` 线程 → channel → 事件循环，见 node/fs.rs）────────

/// 分配 watch id（调用方随后建 watcher；失败路径无需配套调用，尚未计数）。
pub fn watch_alloc() -> Option<(
    u64,
    tokio::sync::mpsc::UnboundedSender<crate::builtins::node::fs::WatchEvent>,
)> {
    with_plain(|p| {
        let tx = p.watch_tx.clone()?;
        p.watch_next_id += 1;
        Some((p.watch_next_id, tx))
    })
}

/// 登记驱动 + 监听（persistent 计存活；非 persistent 只收事件不续命）。
pub fn watch_add(
    id: u64,
    driver: notify::RecommendedWatcher,
    listener: JSVal,
    persistent: bool,
) {
    with_rooted(|s| s.watch_listeners.push(WatchCallback { id, listener: Heap::boxed(listener) }));
    with_plain(|p| {
        p.watch_drivers.insert(id, (driver, persistent));
        if persistent {
            p.watch_open += 1;
        }
    });
}

/// 取监听（分发用；保留注册，close 前一直有效）。
pub fn watch_listener(id: u64) -> Option<JSVal> {
    with_rooted(|s| s.watch_listeners.iter().find(|w| w.id == id).map(|w| w.listener.get()))
}

/// 关闭一路 watch（幂等；残留事件落空）。
pub fn watch_remove(id: u64) {
    with_rooted(|s| {
        s.watch_listeners.retain(|w| w.id != id);
    });
    with_plain(|p| {
        if let Some((_, persistent)) = p.watch_drivers.remove(&id) {
            if persistent {
                p.watch_open = p.watch_open.saturating_sub(1);
            }
        }
    });
}

/// 存活 watch 数（persistent；事件循环退出条件用）。
pub fn watch_open() -> usize {
    with_plain(|p| p.watch_open)
}

// ── 网络驱动（node:net；task → channel → 事件循环，同 child 模型）───────────

/// 分配网络 id + 事件端点。
pub fn net_alloc() -> Option<(
    u64,
    tokio::sync::mpsc::UnboundedSender<crate::builtins::node::net::NetEvent>,
)> {
    with_plain(|p| {
        let tx = p.net_tx.clone()?;
        p.net_next_id += 1;
        Some((p.net_next_id, tx))
    })
}

/// 登记客户端/服务端 socket（native 侧；返回写端命令接收端交泵 task）。
pub fn net_socket_add(
    id: u64,
    target: JSVal,
) -> tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::net::NetCmd> {
    with_rooted(|s| s.net_targets.push(NetTarget { id, target: Heap::boxed(target) }));
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    with_plain(|p| {
        p.net_sockets.insert(id, NetEntry { cmd_tx: tx, half_read: false, half_write: false, close_sent: false, writer_alive: true });
        p.net_open += 1;
    });
    rx
}

/// server accept 出的连接：无 target 入表（JS 侧 attach 后补），返回命令接收端。
pub fn net_conn_add() -> (u64, tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::net::NetCmd>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let id = with_plain(|p| {
        p.net_next_id += 1;
        let id = p.net_next_id;
        p.net_sockets.insert(id, NetEntry { cmd_tx: tx, half_read: false, half_write: false, close_sent: false, writer_alive: true });
        p.net_open += 1;
        id
    });
    (id, rx)
}

/// 事后补登记 target（server 连接 attach）。
pub fn net_target_add(id: u64, target: JSVal) {
    with_rooted(|s| s.net_targets.push(NetTarget { id, target: Heap::boxed(target) }));
}

pub fn net_target(id: u64) -> Option<JSVal> {
    with_rooted(|s| s.net_targets.iter().find(|t| t.id == id).map(|t| t.target.get()))
}

/// 发命令（socket 已收尾即 false）。
pub fn net_cmd(id: u64, cmd: crate::builtins::node::net::NetCmd) -> bool {
    with_plain(|p| p.net_sockets.get(&id).is_some_and(|e| e.cmd_tx.send(cmd).is_ok()))
}

/// 置半关旗（读端）；返回是否两侧都已半关（旗已随 purge 清空不重发）。
pub fn net_half_read(id: u64) -> bool {
    with_plain(|p| {
        if let Some(e) = p.net_sockets.get_mut(&id) {
            e.half_read = true;
            e.half_read && e.half_write
        } else {
            false
        }
    })
}

/// 置半关旗（写端）。
pub fn net_half_write(id: u64) -> bool {
    with_plain(|p| {
        if let Some(e) = p.net_sockets.get_mut(&id) {
            e.half_write = true;
            e.half_read && e.half_write
        } else {
            false
        }
    })
}

/// 标记写端 task 退出（返回此前是否存活）。
pub fn net_writer_exit(id: u64) -> bool {
    with_plain(|p| {
        if let Some(e) = p.net_sockets.get_mut(&id) {
            std::mem::replace(&mut e.writer_alive, false)
        } else {
            false
        }
    })
}

/// 写端是否已死（死则读端 EOF 需代行收尾，End 命令无人消费）。
pub fn net_writer_dead(id: u64) -> bool {
    with_plain(|p| p.net_sockets.get(&id).is_some_and(|e| !e.writer_alive))
}

/// Close 单次发送旗（task 侧防双发；purge 由 dispatch 完成后统一做）。
pub fn net_close_once(id: u64) -> bool {
    with_plain(|p| {
        if let Some(e) = p.net_sockets.get_mut(&id) {
            if e.close_sent {
                false
            } else {
                e.close_sent = true;
                true
            }
        } else {
            false
        }
    })
}

/// 收尾清除（entry + target；Close 派发后调用；返回首次 true）。
pub fn net_purge(id: u64) -> bool {
    let entry = with_plain(|p| p.net_sockets.remove(&id));
    with_rooted(|s| s.net_targets.retain(|t| t.id != id));
    if entry.is_some() {
        with_plain(|p| p.net_open = p.net_open.saturating_sub(1));
        true
    } else {
        false
    }
}

/// 存活 socket/server 数（事件循环退出条件用）。
pub fn net_open() -> usize {
    with_plain(|p| p.net_open)
}

// ── worker 驱动（MessagePort/Worker；channel → 事件循环，同 net 模型）────────

/// 会话线程身份初始化（`init_session` 对每个会话都调：主调 (true, 0)，9f-3 的
/// worker 线程起后改写 (false, id)；PlainState::default 全 false/0 不可直接用）。
pub fn worker_session_init(is_main: bool, thread_id: u64) {
    with_plain(|p| {
        p.worker_is_main = is_main;
        p.worker_thread_id = thread_id;
    });
}

pub fn worker_is_main() -> bool {
    with_plain(|p| p.worker_is_main)
}

pub fn worker_thread_id() -> u64 {
    with_plain(|p| p.worker_thread_id)
}

/// 建直连端口对（同会话回环；初始未监听不计数，`port_listen` 后续命）。
pub fn port_pair() -> Option<(u64, u64)> {
    with_plain(|p| {
        let tx = p.worker_tx.clone()?;
        p.worker_next_id += 1;
        let a = p.worker_next_id;
        p.worker_next_id += 1;
        let b = p.worker_next_id;
        with_rooted(|s| {
            s.worker_ports.push(WorkerPort { id: a, peer: b, peer_tx: tx.clone(), target: None, open: true, refed: true, listening: false, counted: false, peer_closed: false, peer_is_worker: false });
            s.worker_ports.push(WorkerPort { id: b, peer: a, peer_tx: tx, target: None, open: true, refed: true, listening: false, counted: false, peer_closed: false, peer_is_worker: false });
        });
        Some((a, b))
    })
}

/// 建跨会话端口（worker parentPort 用；对端地址是 worker id，走 `WMsg`）。
pub fn port_alloc_cross(
    peer_worker: u64,
    peer_tx: tokio::sync::mpsc::UnboundedSender<crate::builtins::node::worker::WorkerEvent>,
) -> u64 {
    let id = with_plain(|p| {
        p.worker_next_id += 1;
        p.worker_next_id
    });
    with_rooted(|s| {
        s.worker_ports.push(WorkerPort { id, peer: peer_worker, peer_tx, target: None, open: true, refed: true, listening: false, counted: false, peer_closed: false, peer_is_worker: true });
    });
    id
}

/// 按 `open && refed && listening` 重算，返回计数净变化（+1/0/-1）。
fn port_recount(id: u64) -> i64 {
    with_rooted(|s| match s.worker_ports.iter_mut().find(|p| p.id == id) {
        Some(p) => {
            let want = p.open && p.refed && p.listening;
            if want == p.counted {
                0
            } else {
                p.counted = want;
                if want { 1 } else { -1 }
            }
        }
        None => 0,
    })
}

fn port_bump(delta: i64) {
    with_plain(|p| {
        if delta > 0 {
            p.worker_open += delta as usize;
        } else {
            p.worker_open = p.worker_open.saturating_sub((-delta) as usize);
        }
    });
}

/// 监听装上（有 message 监听即续命）。
pub fn port_listen(id: u64) {
    with_rooted(|s| {
        if let Some(p) = s.worker_ports.iter_mut().find(|p| p.id == id) {
            p.listening = true;
        }
    });
    port_bump(port_recount(id));
}

/// 监听卸完（无 message 监听即不续命；JS 侧末个移除时调）。
pub fn port_unlisten(id: u64) {
    with_rooted(|s| {
        if let Some(p) = s.worker_ports.iter_mut().find(|p| p.id == id) {
            p.listening = false;
        }
    });
    port_bump(port_recount(id));
}

pub fn port_has_ref(id: u64) -> bool {
    with_rooted(|s| s.worker_ports.iter().find(|p| p.id == id).is_some_and(|p| p.refed))
}

/// 登记端口 JS 目标（MessagePort 构造时 attach；dispatch 经 `__ev` 回调）。
pub fn port_attach(id: u64, target: JSVal) {
    with_rooted(|s| {
        if let Some(p) = s.worker_ports.iter_mut().find(|p| p.id == id) {
            p.target = Some(Heap::boxed(target));
        }
    });
}

pub fn port_target(id: u64) -> Option<JSVal> {
    with_rooted(|s| {
        s.worker_ports.iter().find(|p| p.id == id).and_then(|p| p.target.as_ref().map(|t| t.get()))
    })
}

/// 发往对端（本端已关/对端已关即丢弃，Node 同款静默；parentPort 走 `WMsg`）。
pub fn port_post(id: u64, json: String) -> bool {
    let route = with_rooted(|s| {
        s.worker_ports.iter().find(|p| p.id == id).and_then(|p| {
            if !p.open || p.peer_closed {
                return None;
            }
            Some((p.peer, p.peer_tx.clone(), p.peer_is_worker))
        })
    });
    match route {
        Some((peer, tx, true)) => tx.send(crate::builtins::node::worker::WorkerEvent::WMsg { worker_id: peer, json }).is_ok(),
        Some((peer, tx, false)) => tx.send(crate::builtins::node::worker::WorkerEvent::PortMsg { to: peer, json }).is_ok(),
        None => false,
    }
}

/// 本端关闭（首次 true；摘 target；计过数即减；尽力通知对端记 peer_closed）。
pub fn port_close(id: u64) -> bool {
    let route = with_rooted(|s| {
        let mut out = None;
        if let Some(p) = s.worker_ports.iter_mut().find(|p| p.id == id) {
            if !p.open {
                return None;
            }
            p.open = false;
            p.target = None;
            let was = p.counted;
            p.counted = false;
            out = Some((p.peer, p.peer_tx.clone(), was));
        }
        out
    });
    match route {
        Some((peer, tx, was)) => {
            if was {
                port_bump(-1);
            }
            let _ = tx.send(crate::builtins::node::worker::WorkerEvent::PortClose { to: peer });
            true
        }
        None => false,
    }
}

/// 取消引用（端口不再续命事件循环；幂等）。
pub fn port_unref(id: u64) {
    with_rooted(|s| {
        if let Some(p) = s.worker_ports.iter_mut().find(|p| p.id == id) {
            p.refed = false;
        }
    });
    port_bump(port_recount(id));
}

/// 重新引用（unref 的逆操作；关闭/无监听即无操作）。
pub fn port_ref(id: u64) {
    with_rooted(|s| {
        if let Some(p) = s.worker_ports.iter_mut().find(|p| p.id == id) {
            p.refed = true;
        }
    });
    port_bump(port_recount(id));
}

/// 对端关闭到达：记 peer_closed（后续 post 静默丢弃；本端不派 close，Node 口径）。
pub fn port_peer_closed(id: u64) {
    with_rooted(|s| {
        if let Some(p) = s.worker_ports.iter_mut().find(|p| p.id == id) {
            p.peer_closed = true;
        }
    });
}

/// 存活计数（ref'd 端口 + 运行中 worker；事件循环退出条件用）。
pub fn worker_open() -> usize {
    with_plain(|p| p.worker_open)
}

// ── Worker 线程（9f-3；spawn/parentPort/workerData/exit/terminate）─────────

/// worker 线程起后初始化（`runtime::run_worker_thread` 经 boot 槽调；含权限继承）。
pub fn worker_boot(thread_id: u64, data_json: Option<String>, parent_port: u64) {
    with_plain(|p| {
        p.worker_is_main = false;
        p.worker_thread_id = thread_id;
        p.worker_data_json = data_json;
        p.worker_parent_port = Some(parent_port);
        p.worker_terminated = false;
    });
    if let Some(perms) = crate::permissions::cli_snapshot() {
        crate::permissions::install(perms);
    }
}

pub fn worker_data_json() -> Option<String> {
    with_plain(|p| p.worker_data_json.clone())
}

pub fn worker_parent_port() -> Option<u64> {
    with_plain(|p| p.worker_parent_port)
}

/// 终止旗置位（`WTerminate` 分发时调；事件循环检查点见 `pump_once`）。
pub fn worker_terminate_flag() {
    with_plain(|p| p.worker_terminated = true);
}

pub fn worker_terminated() -> bool {
    with_plain(|p| p.worker_terminated)
}

/// 登记 worker 句柄（主会话；运行中计 1 存活，可 unref 摘）。
pub fn worker_handle_add(h: WorkerHandle) {
    with_plain(|p| {
        if h.counted {
            p.worker_open += 1;
        }
        p.worker_handles.insert(h.worker_id, h);
    });
}

/// 取 worker 发件端点（post/terminate 用；已退出即 None）。
pub fn worker_inbox(worker_id: u64) -> Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::node::worker::WorkerEvent>> {
    with_plain(|p| p.worker_handles.get(&worker_id).filter(|h| !h.exited).map(|h| h.inbox_tx.clone()))
}

/// 取 worker 的 parentPort 寻址（主→worker 投递用）。
pub fn worker_parent_port_of(worker_id: u64) -> Option<u64> {
    with_plain(|p| p.worker_handles.get(&worker_id).filter(|h| !h.exited).map(|h| h.parent_port))
}

/// worker 退出结算（计过数即减；标记 exited 防双减；返回是否首次）。
pub fn worker_exited(worker_id: u64) -> bool {
    with_plain(|p| match p.worker_handles.get_mut(&worker_id) {
        Some(h) if !h.exited => {
            h.exited = true;
            if h.counted {
                h.counted = false;
                p.worker_open = p.worker_open.saturating_sub(1);
            }
            true
        }
        _ => false,
    })
}

/// worker 取消息线程 id（`Worker.threadId` 用）。
pub fn worker_tid(worker_id: u64) -> Option<u64> {
    with_plain(|p| p.worker_handles.get(&worker_id).map(|h| h.thread_id))
}

/// worker ref/unref（unref 后不续命主循环）。
pub fn worker_set_ref(worker_id: u64, refed: bool) {
    with_plain(|p| {
        if let Some(h) = p.worker_handles.get_mut(&worker_id) {
            if !h.exited && h.counted != refed {
                h.counted = refed;
                if refed {
                    p.worker_open += 1;
                } else {
                    p.worker_open = p.worker_open.saturating_sub(1);
                }
            }
        }
    });
}

/// 登记 worker JS 目标（`Worker` 构造时 attach）。
pub fn worker_target_add(id: u64, target: JSVal) {
    with_rooted(|s| s.worker_targets.push(WorkerTarget { id, target: Heap::boxed(target) }));
}

pub fn worker_target(id: u64) -> Option<JSVal> {
    with_rooted(|s| s.worker_targets.iter().find(|t| t.id == id).map(|t| t.target.get()))
}

/// 摘除 worker JS 目标（Exit 派发后调；返回首次 true）。
pub fn worker_target_remove(id: u64) -> bool {
    with_rooted(|s| {
        let n0 = s.worker_targets.len();
        s.worker_targets.retain(|t| t.id != id);
        s.worker_targets.len() != n0
    })
}

// ── vm 上下文（同 Runtime 内多 global，各占新 compartment）────────────────
///
/// 登记新 global（`Box` 定址，`Heap::set` 后禁移动 §4.40），返回单调 id。

pub fn vm_add(global: *mut JSObject) -> u64 {
    let id = with_plain(|p| {
        p.vm_next_id += 1;
        p.vm_next_id
    });
    with_rooted(|s| {
        s.vm_contexts.push(VmCtx { id, global: Heap::boxed(global) });
    });
    id
}

/// 取上下文 global 裸指针（调用方必须立即重 root，中间无 GC 间隙，见 runtime 约定）。
pub fn vm_global(id: u64) -> Option<*mut JSObject> {
    with_rooted(|s| s.vm_contexts.iter().find(|c| c.id == id).map(|c| c.global.get()))
}

/// 摘除上下文（JS 侧 FinalizationRegistry/显式释放用；不在即 false）。
pub fn vm_release(id: u64) -> bool {
    let n0 = with_rooted(|s| {
        let n0 = s.vm_contexts.len();
        s.vm_contexts.retain(|c| c.id != id);
        n0
    });
    with_rooted(|s| s.vm_contexts.len() != n0)
}

// ── 异步子进程驱动（task → channel → 事件循环，见 node/child.rs）────────────

/// 子进程表项（kill 用；`detached` 决定组杀；`stdin_tx` 供 pipe 写/关，无则 None；
/// `pipes_expected/done` 保证残留输出先于 Exited 送达，见 child.rs）。
pub struct ChildEntry {
    pub child: tokio::process::Child,
    pub detached: bool,
    pub stdin_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::node::child::StdinCmd>>,
    pub pipes_expected: u8,
    pub pipes_done: u8,
}

/// 分配子进程 id（调用方随后 spawn + 登记；失败路径无需配套调用）。
pub fn child_alloc() -> Option<(
    u64,
    tokio::sync::mpsc::UnboundedSender<crate::builtins::node::child::ChildEvent>,
)> {
    with_plain(|p| {
        let tx = p.child_tx.clone()?;
        p.child_next_id += 1;
        Some((p.child_next_id, tx))
    })
}

/// 登记进程本体 + JS 目标（target 为 prelude ChildProcess 对象；pipe 时带 stdin 通道
/// 与期望落定的 pipe 泵数）。
pub fn child_add(
    id: u64,
    child: tokio::process::Child,
    detached: bool,
    target: JSVal,
    stdin_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::node::child::StdinCmd>>,
    pipes_expected: u8,
) {
    with_rooted(|s| s.child_targets.push(ChildTarget { id, target: Heap::boxed(target) }));
    with_plain(|p| {
        p.child_procs.insert(id, ChildEntry { child, detached, stdin_tx, pipes_expected, pipes_done: 0 });
        p.child_open += 1;
    });
}

/// stdin 写/关（pipe 专用；子进程已走即 false，调用方转流错误）。
pub fn child_stdin_send(id: u64, cmd: crate::builtins::node::child::StdinCmd) -> bool {
    with_plain(|p| {
        p.child_procs
            .get(&id)
            .and_then(|e| e.stdin_tx.as_ref())
            .is_some_and(|tx| tx.send(cmd).is_ok())
    })
}

/// pipe 泵落定记数（EOF/错皆记；返回是否全部落定）。泵 task 退出路径必调。
pub fn child_pipe_done(id: u64) -> bool {
    with_plain(|p| {
        let Some(e) = p.child_procs.get_mut(&id) else {
            return true;
        };
        e.pipes_done = e.pipes_done.saturating_add(1);
        e.pipes_done >= e.pipes_expected
    })
}

/// pipe 是否全部落定（等待 task 发 Exited 前查；表项已摘除也算落定）。
pub fn child_pipes_flushed(id: u64) -> bool {
    with_plain(|p| {
        p.child_procs
            .get(&id)
            .is_none_or(|e| e.pipes_done >= e.pipes_expected)
    })
}

/// 取 JS 目标（分发用；保留注册，终态时摘除）。
pub fn child_target(id: u64) -> Option<JSVal> {
    with_rooted(|s| s.child_targets.iter().find(|c| c.id == id).map(|c| c.target.get()))
}

/// 终态记账（target + 本体移除 + 存活减一；kill 后残留事件落空）。
pub fn child_remove(id: u64) {
    with_rooted(|s| {
        s.child_targets.retain(|c| c.id != id);
    });
    with_plain(|p| {
        p.child_procs.remove(&id);
        p.child_open = p.child_open.saturating_sub(1);
    });
}

/// 发信号（`SIGKILL`/`SIGTERM`/数字；detached 走组杀，unix；win 直接杀）。
/// 返回是否作用到存活进程（未知 id/已退出为 false）。
pub fn child_kill(id: u64, sig: &str) -> bool {
    with_plain(|p| {
        let Some(entry) = p.child_procs.get_mut(&id) else {
            return false;
        };
        #[cfg(unix)]
        {
            use nix::sys::signal::{kill, Signal};
            use nix::unistd::Pid;
            let pid = entry.child.id().unwrap_or(0) as i32;
            if pid <= 0 {
                return false;
            }
            let signal = match sig.trim().to_ascii_uppercase().as_str() {
                "SIGKILL" | "KILL" | "9" => Signal::SIGKILL,
                _ => Signal::SIGTERM,
            };
            let target = if entry.detached { Pid::from_raw(-pid) } else { Pid::from_raw(pid) };
            if kill(target, signal).is_ok() {
                return true;
            }
            // 组杀失败回退直杀（如已非组长）。
            if entry.detached && kill(Pid::from_raw(pid), signal).is_ok() {
                return true;
            }
            // 同步杀不动则置异步杀（task 侧收尾；此处报 false 由调用方定）。
            entry.child.start_kill().is_ok()
        }
        #[cfg(not(unix))]
        {
            let _ = sig;
            entry.child.start_kill().is_ok()
        }
    })
}

/// 存活子进程数（事件循环退出条件用）。
pub fn child_open() -> usize {
    with_plain(|p| p.child_open)
}

/// 非阻塞收尸（`try_wait` 到即收，无僵尸；返回原始状态，映射由调用方做）。
pub fn child_try_wait(id: u64) -> Option<std::process::ExitStatus> {
    with_plain(|p| p.child_procs.get_mut(&id)?.child.try_wait().ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    /// 端口计数状态机（`counted = open && refed && listening`）需 rooted 会话，
    /// 单测起不来引擎——由黑盒全链覆盖（`phase9f_worker_channel_roundtrip` 的
    /// close/unref/`phase9f_worker_thread_info_boundary` 的 th-unref 行）。

    /// worker 句柄计数：运行中 +1，unref 摘，退出结算防双减。
    #[test]
    #[serial]
    fn worker_handle_counting() {
        let base = worker_open();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        worker_handle_add(WorkerHandle { worker_id: 9001, thread_id: 7, inbox_tx: tx, parent_port: 0, counted: true, exited: false });
        assert_eq!(worker_open(), base + 1);
        assert_eq!(worker_tid(9001), Some(7));
        worker_set_ref(9001, false);
        assert_eq!(worker_open(), base);
        worker_set_ref(9001, true);
        assert_eq!(worker_open(), base + 1);
        assert!(worker_exited(9001)); // 首次 true
        assert_eq!(worker_open(), base);
        assert!(!worker_exited(9001)); // 防双减
        assert_eq!(worker_open(), base);
        assert!(worker_inbox(9001).is_none()); // 已退出即无端点
        with_plain(|p| p.worker_handles.remove(&9001));
    }
}
