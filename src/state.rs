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
pub struct TimerEntry {
    pub id: u32,
    pub callback: Heap<JSVal>,
    pub args: Heap<JSVal>, // JS 数组，由 prelude 打包
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

/// 一个已编译的模块：URL（spec 键）+ 跨 GC 保活的模块记录。
pub struct ModuleEntry {
    pub url: String,
    pub record: Heap<*mut JSObject>,
}

// SAFETY: 只追踪 record（URL 无 GC 指针）。
unsafe impl Traceable for ModuleEntry {
    unsafe fn trace(&self, trc: *mut JSTracer) { unsafe {
        self.record.trace(trc);
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

/// 一个未决 fetch 的 resolve/reject（事件循环结算时取出并移除）。
pub struct FetchCallback {
    pub id: u64,
    pub resolve: Heap<JSVal>,
    pub reject: Heap<JSVal>,
}

// SAFETY: 只追踪两个回调值（id 无 GC 指针）。
unsafe impl Traceable for FetchCallback {
    unsafe fn trace(&self, trc: *mut JSTracer) { unsafe {
        self.resolve.trace(trc);
        self.reject.trace(trc);
    }}
}

/// 流式 body 的等待 pull（resolve/reject 存 RootedState 被 GC 追踪）。
pub struct StreamWaiter {
    pub resolve: Heap<JSVal>,
    pub reject: Heap<JSVal>,
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
    pub unhandled: Vec<Heap<*mut JSObject>>, // 未处理 rejection 的 promise
    pub call_fn: Heap<JSVal>,                // prelude 的 __wjs_call(cb, args)
    pub entries_fn: Heap<JSVal>,             // prelude 的 __wjs_entries(v)
    pub on_fulfilled: Heap<JSVal>,           // rejection 捕获用 native
    pub on_rejected: Heap<JSVal>,
    pub entry_fulfilled: Heap<JSVal>, // 模块入口 TLA 决议捕获用 native
    pub entry_rejected: Heap<JSVal>,
    pub modules: Vec<ModuleEntry>, // URL → 已编译模块记录（循环/去重，spec 同结果）
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
    /// WebSocket 驱动端点（同上）+ 发送端表 + 存活计数（事件循环退出条件用）。
    pub ws_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::ws::WsEvent>>,
    pub ws_next_id: u64,
    pub ws_open: usize,
    pub ws_sinks: HashMap<u64, tokio::sync::mpsc::UnboundedSender<crate::builtins::ws::WsOut>>,
    /// eval 包装（async IIFE）引入的行偏移，报错行号统一校正。
    pub line_adjust: u32,
    /// 全局对象裸指针。前置条件：run() 里的 rooted! global 活过整个事件循环，
    /// 本指针只是它的借用副本，绝不在此之外解引用。
    pub global: *mut JSObject,
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
        let resolve_h = Heap::default();
        resolve_h.set(resolve);
        let reject_h = Heap::default();
        reject_h.set(reject);
        s.fetch_callbacks.push(FetchCallback { id, resolve: resolve_h, reject: reject_h });
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
        let resolve_h = Heap::default();
        resolve_h.set(resolve);
        let reject_h = Heap::default();
        reject_h.set(reject);
        s.fetch_streams[pos].waiters.push(StreamWaiter { resolve: resolve_h, reject: reject_h });
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
