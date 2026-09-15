//! JS 线程私有状态：timers 注册表、console 计数、未处理 rejection 清单、内部辅助函数值。
//! 只允许在 JS 独占线程访问（AGENTS §6）。所有持 JS 值的字段集中在 `RootedState`，
//! 经 `RootedTraceableBox` 整体跨 GC 保活；`PlainState` 不含 GC 指针。

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicU64;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use mozjs::context::JSContext;
use mozjs::gc::{RootedTraceableBox, Traceable};
use mozjs::jsapi::{Heap, JS_GetFunctionObject, JS_NewFunction, JSObject, JSTracer};
use mozjs::jsval::{JSVal, ObjectValue, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{Frame, get_prop_string, get_prop_u32, value_to_string};
use crate::loader::sourcemap::remap_location;

/// 一个已注册的定时器。`at` 为触发时刻（interval 为上次触发 + 间隔，漂移校正）。
/// `callback`/`args` 经 `Box` 定址（mozjs `Heap::set` 后禁移动，见 §4.40）。
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

/// 一个已编译的模块：URL（spec 键）+ 跨 GC 保活的模块记录（`Box` 定址，见 §4.40）。
pub struct ModuleEntry {
    pub url: String,
    pub record: Box<Heap<*mut JSObject>>,
    /// `ModuleEvaluate` 已跑（require(esm) 幂等门；动态 import 由引擎级联求值，
    /// 也置位防二次 evaluate）。
    pub evaluated: bool,
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

/// 一个 vm 模块记录（SourceTextModule 编译产物；新 compartment 归属其 ctx；
/// `Box` 定址，见 §4.40）。状态机（linked/evaluated 一次语义）由 JS 壳 نگه，
/// Rust 侧只记位防重复 link/evaluate；has_imports 为 true 者 v1 拒绝 link
///（带导入的 linker 切片后续做，见 vm.rs 模块头注）。
pub struct VmMod {
    pub id: u64,
    pub ctx: u64,
    pub identifier: String,
    pub record: Box<Heap<*mut JSObject>>,
    pub has_imports: bool,
    /// 静态依赖 specifier 表（`dependencySpecifiers` 面；纯数据，无 GC 指针）。
    pub deps: Vec<String>,
    pub linked: bool,
    pub evaluated: bool,
}

// SAFETY: 只追踪 record（其余无 GC 指针）。
unsafe impl Traceable for VmMod {
    unsafe fn trace(&self, trc: *mut JSTracer) { unsafe {
        self.record.trace(trc);
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

/// 一个 QUIC endpoint/会话的 JS 目标（`__ev` 回调；Close 后摘除；`Box` 定址）。
pub struct QuicTarget {
    pub id: u64,
    pub target: Box<Heap<JSVal>>,
}

// SAFETY: 只追踪 target（id 无 GC 指针）。
unsafe impl Traceable for QuicTarget {
    unsafe fn trace(&self, trc: *mut JSTracer) { unsafe {
        self.target.trace(trc);
    }}
}

/// QUIC endpoint 表项（监听 socket + accept 任务；close 时 abort）。
pub struct QuicEndpointEntry {
    pub ep: quinn::Endpoint,
    pub accept_task: tokio::task::AbortHandle,
    pub closing: bool,
}

/// QUIC 会话表项（连接句柄；驱动任务跑命令/accept/数据报/`closed()` 守望）。
pub struct QuicSessionEntry {
    pub conn: Option<quinn::Connection>,
    pub driver: Option<tokio::task::AbortHandle>,
    pub local: String,
    pub remote: String,
    pub cmd_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::node::quic::QuicSessCmd>>,
    /// H3 分支命令端点（9i-9；服务端 Respond / 客户端 Request）。
    pub h3_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::node::quic::QuicH3Cmd>>,
    /// 发起侧自有 endpoint（socket 保活；收尾时 close + 释放。服务端会话为 None，
    /// 其 socket 归 endpoint 表项管）。
    pub client_ep: Option<quinn::Endpoint>,
}

/// QUIC 流方向（本地视角）。
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum QuicStreamDir {
    Bidi,
    /// 本地只写（对端只读）。
    Send,
    /// 本地只读（对端只写）。
    Recv,
}

/// QUIC 流表项（读写半端各有任务持有；任一半终结即整流收尾，单出口哲学）。
/// 注：流 id 即 map key，方向由创建点经任务/事件传递，不在此存储（曾存 id/dir，dead_code，已删）。
pub struct QuicStreamEntry {
    pub sess: u64,
    pub quic_id: Option<u64>,
    pub write_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::node::quic::QuicStreamCmd>>,
    pub read_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::node::quic::QuicStreamCmd>>,
    pub write_task: Option<tokio::task::AbortHandle>,
    pub read_task: Option<tokio::task::AbortHandle>,
    pub done: bool,
}
/// 迁移后的转发路由（源表项变转发器：收到的投递/关闭原样递往新址，对端无感知）。
/// 通道无 GC 指针，不追踪（`peer_tx` 同款）。
pub struct PortForward {
    pub tx: tokio::sync::mpsc::UnboundedSender<crate::builtins::node::worker::WorkerEvent>,
    pub to: u64,
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
    /// 非空即转发器（迁移后源表项；`target` 已摘，不计数）。
    pub forward: Option<PortForward>,
    /// 迁移邀约中（offer 后，forward 到达前）：保留 target 收竞态消息，不计数。
    pub moved: bool,
    /// 承接迁移的表项记源路由（本端 close 时发 `PortDrop` 拆转发器）。
    pub via: Option<(tokio::sync::mpsc::UnboundedSender<crate::builtins::node::worker::WorkerEvent>, u64)>,
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
    pub quic_ep_targets: Vec<QuicTarget>, // QUIC endpoint 的 JS 目标（Close 后摘除）
    pub quic_sess_targets: Vec<QuicTarget>, // QUIC 会话的 JS 目标（Close 后摘除）
    pub quic_stream_targets: Vec<QuicTarget>, // QUIC 流的 JS 目标（Close 后摘除）
    pub vm_contexts: Vec<VmCtx>, // node:vm 上下文 global（release 摘除，会话终由 OS 回收）
    pub vm_mods: Vec<VmMod>, // node:vm 模块记录（link/evaluate 后摘除，会话终由 OS 回收）
    pub bc_targets: Vec<BcTarget>, // BroadcastChannel 订阅目标（unsub/close 后摘除）
    pub fetch_callbacks: Vec<FetchCallback>, // 未决 fetch 的 resolve/reject（按 id 取出）
    pub fetch_streams: Vec<FetchStreamState>, // 流式 body（chunk 泵；cancel/终态时移除）
    pub make_response_fn: Heap<JSVal>, // prelude 的 __wjs_make_response
    pub make_fetch_error_fn: Heap<JSVal>, // prelude 的 __wjs_make_fetch_error
    pub ws_emit_fn: Heap<JSVal>, // prelude 的 __wjs_ws_emit
    pub napi: Option<crate::napi::env::NapiEnv>, // napi 会话单例（首个 .node require 建起；plan-napi §2）
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
        self.quic_ep_targets.trace(trc);
        self.quic_sess_targets.trace(trc);
        self.quic_stream_targets.trace(trc);
        self.vm_contexts.trace(trc);
        self.vm_mods.trace(trc);
        self.bc_targets.trace(trc);
        self.fetch_callbacks.trace(trc);
        self.fetch_streams.trace(trc);
        self.make_response_fn.trace(trc);
        self.make_fetch_error_fn.trace(trc);
        self.ws_emit_fn.trace(trc);
        self.napi.trace(trc);
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
    /// napi 第 8 通道发送端（async_work/TSFN；JS 线程 create 时克隆进 rec，
    /// OS 线程只经 rec.tx 发送——禁经 TLS 取，§4.24）。
    pub napi_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::napi::asyncwork::NapiEvent>>,
    /// SM 异步任务派发闭包（dispatch::install 泄漏的 Box 指针：pending 计数
    /// 读取与会话身份；进程存活期恒有效，与 §4.8 Runtime 泄漏同哲学）。
    pub dispatch_closure: Option<*mut std::ffi::c_void>,
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
    /// 会话序号（进程级单调；跨会话寻址的身份键，如 BC 自发排除）。
    pub worker_session_seq: u64,
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
    /// QUIC 驱动端点（endpoint/会话；接收端由事件循环持有）。
    pub quic_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::node::quic::QuicEvent>>,
    pub quic_next_id: u64,
    /// 存活数（监听中 endpoint + 存活会话；事件循环退出条件用）。
    pub quic_open: usize,
    pub quic_endpoints: HashMap<u64, QuicEndpointEntry>,
    pub quic_sessions: HashMap<u64, QuicSessionEntry>,
    pub quic_streams: HashMap<u64, QuicStreamEntry>,
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

/// napi 会话单例裸指针（api.rs trampoline/函数族用；JS 线程专用，§4.24 纪律）。
/// 裸指针出 TLS 闭包：NapiEnv 生命周期 = 会话 = RootedState TLS 本体。
pub fn napi_env_ptr() -> Option<*mut crate::napi::env::NapiEnv> {
    with_rooted(|s| s.napi.as_mut().map(|e| e as *mut crate::napi::env::NapiEnv))
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
        // `Heap::boxed` 定址（set 后禁移动，见 §4.40；Vec push 会搬运元素）。
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

/// napi 异步 keep-alive（未完成 async_work + refed TSFN；事件循环退出条件用）。
pub fn napi_pending() -> usize {
    crate::napi::asyncwork::pending_count()
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
            s.worker_ports.push(WorkerPort { id: a, peer: b, peer_tx: tx.clone(), target: None, open: true, refed: true, listening: false, counted: false, peer_closed: false, peer_is_worker: false, forward: None, moved: false, via: None });
            s.worker_ports.push(WorkerPort { id: b, peer: a, peer_tx: tx, target: None, open: true, refed: true, listening: false, counted: false, peer_closed: false, peer_is_worker: false, forward: None, moved: false, via: None });
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
        s.worker_ports.push(WorkerPort { id, peer: peer_worker, peer_tx, target: None, open: true, refed: true, listening: false, counted: false, peer_closed: false, peer_is_worker: true, forward: None, moved: false, via: None });
    });
    id
}

/// 按 `open && refed && listening` 重算，返回计数净变化（+1/0/-1）。
fn port_recount(id: u64) -> i64 {
    with_rooted(|s| match s.worker_ports.iter_mut().find(|p| p.id == id) {
        Some(p) => {
            let want = p.open && p.refed && p.listening && !p.moved && p.forward.is_none();
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

/// 摘目标（`__wjs_port_detach` 用）：迁移排空后/静默摘除，不通知对端，不碰路由。
pub fn port_detach(id: u64) {
    with_rooted(|s| {
        if let Some(p) = s.worker_ports.iter_mut().find(|p| p.id == id) {
            p.target = None;
            if p.counted {
                p.counted = false;
                port_bump(-1);
            }
        }
    });
}

/// 取转发路由（`PortMsg` 分发回退用：`forwarded` 排空摘 target 后到达的迟到
/// 消息改道新址，不丢）。
pub fn port_forward_route(id: u64) -> Option<(PortTx, u64)> {
    with_rooted(|s| {
        s.worker_ports.iter().find(|p| p.id == id).and_then(|p| {
            p.forward.as_ref().map(|f| (f.tx.clone(), f.to))
        })
    })
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

/// 发往对端（本端已关/对端已关即丢弃，Node 同款静默；parentPort 走 `WMsg`；
/// 转发器表项改道新址，发送失败即自拆）。
pub fn port_post(id: u64, json: String) -> bool {
    enum Route {
        Direct { peer: u64, tx: tokio::sync::mpsc::UnboundedSender<crate::builtins::node::worker::WorkerEvent>, is_worker: bool },
        Forward { tx: tokio::sync::mpsc::UnboundedSender<crate::builtins::node::worker::WorkerEvent>, to: u64 },
    }
    let route = with_rooted(|s| {
        s.worker_ports.iter().find(|p| p.id == id).and_then(|p| {
            if !p.open || p.peer_closed {
                return None;
            }
            if let Some(f) = p.forward.as_ref() {
                return Some(Route::Forward { tx: f.tx.clone(), to: f.to });
            }
            Some(Route::Direct { peer: p.peer, tx: p.peer_tx.clone(), is_worker: p.peer_is_worker })
        })
    });
    match route {
        Some(Route::Direct { peer, tx, is_worker: true }) => tx.send(crate::builtins::node::worker::WorkerEvent::WMsg { worker_id: peer, json }).is_ok(),
        Some(Route::Direct { peer, tx, is_worker: false }) => tx.send(crate::builtins::node::worker::WorkerEvent::PortMsg { to: peer, json }).is_ok(),
        Some(Route::Forward { tx, to }) => {
            if tx.send(crate::builtins::node::worker::WorkerEvent::PortMsg { to, json }).is_ok() {
                true
            } else {
                // 新址已死：自拆转发器，后续直落。
                with_rooted(|s| {
                    if let Some(p) = s.worker_ports.iter_mut().find(|p| p.id == id) {
                        p.open = false;
                        p.forward = None;
                    }
                });
                false
            }
        }
        None => false,
    }
}

/// 本端关闭（首次 true；摘 target；计过数即减；尽力通知对端记 peer_closed；
/// 承接表项另发 `PortDrop` 拆源转发器）。
pub fn port_close(id: u64) -> bool {
    let route = with_rooted(|s| {
        let mut out = None;
        if let Some(p) = s.worker_ports.iter_mut().find(|p| p.id == id) {
            if !p.open {
                return None;
            }
            p.open = false;
            p.target = None;
            p.forward = None;
            let was = p.counted;
            p.counted = false;
            out = Some((p.peer, p.peer_tx.clone(), was, p.via.clone()));
        }
        out
    });
    match route {
        Some((peer, tx, was, via)) => {
            if was {
                port_bump(-1);
            }
            // 原语义保持：`PortClose{to: peer}`（cross 表项的 peer 是 worker_id，
            // 对端查不到即无视，静默；见旧实现）。
            let _ = tx.send(crate::builtins::node::worker::WorkerEvent::PortClose { to: peer });
            if let Some((via_tx, via_id)) = via {
                let _ = via_tx.send(crate::builtins::node::worker::WorkerEvent::PortDrop { to: via_id });
            }
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

/// 对端关闭到达：记 peer_closed（后续 post 静默丢弃；本端不派 close，Node 口径；
/// 到达转发器则递往新址后自拆）。
pub fn port_peer_closed(id: u64) {
    let fwd = with_rooted(|s| {
        let mut out = None;
        if let Some(p) = s.worker_ports.iter_mut().find(|p| p.id == id) {
            if p.forward.is_some() {
                out = p.forward.as_ref().map(|f| (f.tx.clone(), f.to));
                p.open = false;
                p.forward = None;
            } else {
                p.peer_closed = true;
            }
        }
        out
    });
    if let Some((tx, to)) = fwd {
        let _ = tx.send(crate::builtins::node::worker::WorkerEvent::PortClose { to });
    }
}

// ── 端口迁移（9i-2；`transferList` 的 MessagePort 件）───────────────────────
//
// 表是会话级的，id 只在源会话有效；跨会话迁移走进程级邀约槽：offer 侧快照
// 路由并摘 target（转 silent：对端不感知），accept 侧建表并回发 `PortForward`
// 把源表项升为转发器（对端永远只认原路由，多一跳）。同会话迁移走同一路径。

type PortTx = tokio::sync::mpsc::UnboundedSender<crate::builtins::node::worker::WorkerEvent>;

/// 迁移邀约（offer 会话存，accept 会话取走即删）。
struct PortOffer {
    source_tx: PortTx,
    source_id: u64,
    peer: u64,
    peer_tx: PortTx,
    peer_is_worker: bool,
}

static PORT_XFER: OnceLock<Mutex<HashMap<String, PortOffer>>> = OnceLock::new();
static PORT_XFER_NEXT: AtomicU64 = AtomicU64::new(1);
/// 会话序号分配（进程级单调；`init_session` 内调）。
static SESSION_SEQ: AtomicU64 = AtomicU64::new(1);

/// 取本会话序号（0 表未初始化，调用方须在 `init_session` 后用）。
pub fn session_seq() -> u64 {
    with_plain(|p| p.worker_session_seq)
}

/// 分配并记下本会话序号（`init_session` 内调一次）。
pub fn session_seq_init() -> u64 {
    let n = SESSION_SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    with_plain(|p| p.worker_session_seq = n);
    n
}

fn port_xfer() -> &'static Mutex<HashMap<String, PortOffer>> {
    PORT_XFER.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 邀约迁移（`__wjs_port_offer` 用）：表项须 open 且未迁移；成功保留 target 收
/// 竞态消息（`forwarded` 到达排空），置 moved 停计数，回 nonce 串；失败回 None。
pub fn port_offer(id: u64) -> Option<String> {
    let own_tx = with_plain(|p| p.worker_tx.clone())?;
    let snap = with_rooted(|s| {
        let p = s.worker_ports.iter_mut().find(|p| p.id == id)?;
        if !p.open || p.moved || p.forward.is_some() {
            return None;
        }
        p.moved = true;
        if p.counted {
            p.counted = false;
            port_bump(-1);
        }
        Some((p.peer, p.peer_tx.clone(), p.peer_is_worker))
    });
    let (peer, peer_tx, peer_is_worker) = snap?;
    let n = PORT_XFER_NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let nonce = format!("px{n}");
    if port_xfer().lock().ok()?.insert(nonce.clone(), PortOffer { source_tx: own_tx, source_id: id, peer, peer_tx, peer_is_worker }).is_some() {
        return None; // 计数器回绕碰撞（实践不发生），邀约作废
    }
    Some(nonce)
}

/// 撤邀约（`__wjs_port_withdraw` 用）：消息组装失败/未引用转让的槽回收；
/// 源表项保持摘 target（JS 侧已 neutered），对端无感知。
pub fn port_withdraw(nonce: &str) -> bool {
    port_xfer().lock().ok().is_some_and(|mut m| m.remove(nonce).is_some())
}

/// 承接迁移（`__wjs_port_accept` 用）：nonce 有效即在当前会话建表（路由直连原
/// 对端），回发 `PortForward` 升源表项为转发器，回新 id；失败回 None。
pub fn port_accept(nonce: &str) -> Option<u64> {
    let offer = port_xfer().lock().ok()?.remove(nonce)?;
    let own_tx = with_plain(|p| p.worker_tx.clone())?;
    let id = with_plain(|p| {
        p.worker_next_id += 1;
        p.worker_next_id
    });
    with_rooted(|s| {
        s.worker_ports.push(WorkerPort {
            id,
            peer: offer.peer,
            peer_tx: offer.peer_tx,
            target: None,
            open: true,
            refed: true,
            listening: false,
            counted: false,
            peer_closed: false,
            peer_is_worker: offer.peer_is_worker,
            forward: None,
            moved: false,
            via: Some((offer.source_tx.clone(), offer.source_id)),
        });
    });
    let _ = offer.source_tx.send(crate::builtins::node::worker::WorkerEvent::PortForward {
        to: offer.source_id,
        dest_tx: own_tx,
        dest_id: id,
    });
    Some(id)
}

/// 源表项升为转发器（`PortForward` 派发用；表项已死即丢弃）。
pub fn port_forward_set(id: u64, dest_tx: PortTx, dest_id: u64) {
    with_rooted(|s| {
        if let Some(p) = s.worker_ports.iter_mut().find(|p| p.id == id) {
            if !p.open {
                return;
            }
            p.forward = Some(PortForward { tx: dest_tx, to: dest_id });
        }
    });
}

/// 拆转发器（`PortDrop` 派发用；承接端已关）。
pub fn port_drop(id: u64) {
    with_rooted(|s| {
        if let Some(p) = s.worker_ports.iter_mut().find(|p| p.id == id) {
            p.open = false;
            p.forward = None;
            p.target = None;
            if p.counted {
                p.counted = false;
                port_bump(-1);
            }
        }
    });
}

// ── BroadcastChannel（9i-2；同名跨会话扇出）──────────────────────────────
// 落法：订阅是进程级注册表（名 → 订阅者收件箱），投递走各会话收件箱的
// `BcMsg` 事件；会话侧 `bc_targets` 持 JS 目标（`Box` 定址 §4.40）。
// 计数抄端口口径（`open && refed && listening`），`worker_open` 同一池子。

/// 一个 BC 订阅的 JS 目标。
pub struct BcTarget {
    pub id: u64,
    pub name: String,
    pub target: Option<Box<Heap<JSVal>>>,
    pub open: bool,
    pub refed: bool,
    pub listening: bool,
    pub counted: bool,
}

// SAFETY: 只追踪 target（其余无 GC 指针）。
unsafe impl Traceable for BcTarget {
    unsafe fn trace(&self, trc: *mut JSTracer) { unsafe {
        self.target.trace(trc);
    }}
}

/// 进程级订阅项（投递地址；`sub` 是归属会话内的本地 id，`sess` 排自发）。
#[derive(Clone)]
struct BcSub {
    sess: u64,
    tx: PortTx,
    sub: u64,
}

static BC_REGISTRY: OnceLock<Mutex<HashMap<String, Vec<BcSub>>>> = OnceLock::new();

fn bc_registry() -> &'static Mutex<HashMap<String, Vec<BcSub>>> {
    BC_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn bc_recount(id: u64) {
    let delta = with_rooted(|s| match s.bc_targets.iter_mut().find(|b| b.id == id) {
        Some(b) => {
            let want = b.open && b.refed && b.listening;
            if want == b.counted {
                0
            } else {
                b.counted = want;
                if want { 1 } else { -1 }
            }
        }
        None => 0,
    });
    port_bump(delta);
}

/// 订阅（`__wjs_bc_sub` 用）：进程注册 + 会话建表，回本地 sub id。
pub fn bc_sub(name: String) -> Option<u64> {
    let own_tx = with_plain(|p| p.worker_tx.clone())?;
    let id = with_plain(|p| {
        p.worker_next_id += 1;
        p.worker_next_id
    });
    with_rooted(|s| {
        s.bc_targets.push(BcTarget {
            id,
            name: name.clone(),
            target: None,
            open: true,
            refed: true,
            listening: false,
            counted: false,
        });
    });
    bc_registry().lock().ok()?.entry(name).or_default().push(BcSub { sess: session_seq(), tx: own_tx, sub: id });
    Some(id)
}

/// 退订（`__wjs_bc_unsub` 用）：双表摘除；计过数即减。
pub fn bc_unsub(id: u64) {
    let name = with_rooted(|s| {
        let mut out = None;
        if let Some(i) = s.bc_targets.iter().position(|b| b.id == id) {
            let b = s.bc_targets.remove(i);
            if b.counted {
                port_bump(-1);
            }
            out = Some(b.name);
        }
        out
    });
    if let Some(name) = name {
        let me = session_seq();
        if let Ok(mut reg) = bc_registry().lock() {
            if let Some(v) = reg.get_mut(&name) {
                v.retain(|e| !(e.sess == me && e.sub == id));
                if v.is_empty() {
                    reg.remove(&name);
                }
            }
        }
    }
}

/// 扇出（`__wjs_bc_pub` 用）：同名订阅全发（除发送者自身 `(sess, sub)`），
/// 关闭/死亡即摘。
pub fn bc_pub(name: &str, except_sub: u64, json: String) {
    let me = session_seq();
    let subs: Vec<BcSub> = bc_registry().lock().ok().and_then(|reg| reg.get(name).cloned()).unwrap_or_default();
    let mut dead: Vec<(u64, u64)> = Vec::new();
    for e in &subs {
        if e.sess == me && e.sub == except_sub {
            continue;
        }
        if e.tx.send(crate::builtins::node::worker::WorkerEvent::BcMsg { to: e.sub, json: json.clone() }).is_err() {
            dead.push((e.sess, e.sub));
        }
    }
    if !dead.is_empty() {
        if let Ok(mut reg) = bc_registry().lock() {
            if let Some(v) = reg.get_mut(name) {
                v.retain(|e| !dead.contains(&(e.sess, e.sub)));
                if v.is_empty() {
                    reg.remove(name);
                }
            }
        }
    }
}

/// 旗变更（`__wjs_bc_flags` 用）：`listen/unlisten/ref/unref` 四档。
pub fn bc_flags(id: u64, what: &str) {
    with_rooted(|s| {
        if let Some(b) = s.bc_targets.iter_mut().find(|b| b.id == id) {
            match what {
                "listen" => b.listening = true,
                "unlisten" => b.listening = false,
                "ref" => b.refed = true,
                "unref" => b.refed = false,
                _ => return,
            }
        } else {
            return;
        }
    });
    bc_recount(id);
}

/// 登记 BC JS 目标（构造时 attach）。
pub fn bc_attach(id: u64, target: JSVal) {
    with_rooted(|s| {
        if let Some(b) = s.bc_targets.iter_mut().find(|b| b.id == id) {
            b.target = Some(Heap::boxed(target));
        }
    });
}

/// 取 BC 目标（`BcMsg` 派发用）。
pub fn bc_target(id: u64) -> Option<JSVal> {
    with_rooted(|s| {
        s.bc_targets.iter().find(|b| b.id == id).and_then(|b| b.target.as_ref().map(|t| t.get()))
    })
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

// ── QUIC 驱动（endpoint/会话；task → channel → 事件循环，同 net 模型）─────

/// QUIC 事件端点（接收端由事件循环持有；无即会话外，不分配）。
pub fn quic_tx_clone() -> Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::node::quic::QuicEvent>> {
    with_plain(|p| p.quic_tx.clone())
}

/// 分配 QUIC id（endpoint/会话共享单调空间）。
pub fn quic_alloc_id() -> u64 {
    with_plain(|p| {
        p.quic_next_id += 1;
        p.quic_next_id
    })
}

/// 登记 endpoint（监听中计 1 存活）。
pub fn quic_ep_insert(id: u64, ep: quinn::Endpoint, accept_task: tokio::task::AbortHandle) {
    with_plain(|p| {
        p.quic_endpoints.insert(id, QuicEndpointEntry { ep, accept_task, closing: false });
        p.quic_open += 1;
    });
}

/// 开始关闭（置 closing + abort accept 环；返回 endpoint 供发 Close 帧）。
/// 重复关即 None（防双发 `EndpointClosed`）。
pub fn quic_ep_begin_close(id: u64) -> Option<quinn::Endpoint> {
    with_plain(|p| match p.quic_endpoints.get_mut(&id) {
        Some(e) if !e.closing => {
            e.closing = true;
            e.accept_task.abort();
            Some(e.ep.clone())
        }
        _ => None,
    })
}

pub fn quic_ep_get(id: u64) -> Option<quinn::Endpoint> {
    with_plain(|p| p.quic_endpoints.get(&id).map(|e| e.ep.clone()))
}

/// 摘除 endpoint（abort accept 任务；计过数即减；返回是否首次）。
pub fn quic_ep_remove(id: u64) -> bool {
    with_plain(|p| match p.quic_endpoints.remove(&id) {
        Some(e) => {
            e.accept_task.abort();
            p.quic_open = p.quic_open.saturating_sub(1);
            true
        }
        None => false,
    })
}

/// 登记会话（存活计 1；conn/cmd 建好后补，见 `quic_sess_set_conn`）。
pub fn quic_sess_insert(id: u64, local: String, remote: String) {
    with_plain(|p| {
        p.quic_sessions.insert(id, QuicSessionEntry { conn: None, driver: None, local, remote, cmd_tx: None, h3_tx: None, client_ep: None });
        p.quic_open += 1;
    });
}

/// 补登记驱动任务句柄（spawn 后；重复登记 abort 旧柄）。
pub fn quic_sess_set_driver(id: u64, driver: tokio::task::AbortHandle) {
    with_plain(|p| {
        if let Some(e) = p.quic_sessions.get_mut(&id) {
            if let Some(old) = e.driver.replace(driver) {
                old.abort();
            }
        }
    });
}

/// 会话寻址（info 用；conn 建好前也能读）。
pub fn quic_sess_addrs(id: u64) -> Option<(String, String)> {
    with_plain(|p| p.quic_sessions.get(&id).map(|e| (e.local.clone(), e.remote.clone())))
}

/// 补登记连接句柄（握手成功后；驱动任务等 `closed()` 用不上它，info/stats/close 用）。
pub fn quic_sess_set_conn(id: u64, conn: quinn::Connection) {
    with_plain(|p| {
        if let Some(e) = p.quic_sessions.get_mut(&id) {
            e.conn = Some(conn);
        }
    });
}

pub fn quic_sess_conn(id: u64) -> Option<quinn::Connection> {
    with_plain(|p| p.quic_sessions.get(&id).and_then(|e| e.conn.clone()))
}

/// 摘除会话（abort 驱动；名下流由分发侧收尾；自有 endpoint 关后释放；
/// 计过数即减；返回是否首次）。
pub fn quic_sess_remove(id: u64) -> bool {
    with_plain(|p| match p.quic_sessions.remove(&id) {
        Some(e) => {
            if let Some(d) = e.driver {
                d.abort();
            }
            if let Some(ep) = e.client_ep {
                ep.close(0u32.into(), b"bye");
            }
            p.quic_open = p.quic_open.saturating_sub(1);
            true
        }
        None => false,
    })
}

/// 寄存发起侧 endpoint（socket 保活到会话收尾；任务结束即 move 进来）。
pub fn quic_sess_set_client_ep(id: u64, ep: quinn::Endpoint) {
    with_plain(|p| {
        if let Some(e) = p.quic_sessions.get_mut(&id) {
            e.client_ep = Some(ep);
        }
    });
}

/// 登记会话命令端点（驱动任务持有接收端；open 流/关会话走此通道）。
pub fn quic_sess_set_cmd(
    id: u64,
    cmd_tx: tokio::sync::mpsc::UnboundedSender<crate::builtins::node::quic::QuicSessCmd>,
) {
    with_plain(|p| {
        if let Some(e) = p.quic_sessions.get_mut(&id) {
            e.cmd_tx = Some(cmd_tx);
        }
    });
}

/// 发会话命令（会话已摘即 false）。
pub fn quic_sess_cmd(id: u64, cmd: crate::builtins::node::quic::QuicSessCmd) -> bool {
    with_plain(|p| {
        p.quic_sessions.get(&id).and_then(|e| e.cmd_tx.clone()).is_some_and(|tx| tx.send(cmd).is_ok())
    })
}

/// 登记 H3 分支命令端点（9i-9；h3 驱动/服务任务持有接收端）。
pub fn quic_sess_set_h3_cmd(
    id: u64,
    tx: tokio::sync::mpsc::UnboundedSender<crate::builtins::node::quic::QuicH3Cmd>,
) {
    with_plain(|p| {
        if let Some(e) = p.quic_sessions.get_mut(&id) {
            e.h3_tx = Some(tx);
        }
    });
}

/// 取 H3 命令端点（会话已摘/非 H3 即 None）。
pub fn quic_sess_h3_cmd(id: u64) -> Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::node::quic::QuicH3Cmd>> {
    with_plain(|p| p.quic_sessions.get(&id).and_then(|e| e.h3_tx.clone()))
}

/// 登记 endpoint JS 目标（`QuicEndpoint` 构造时 attach）。
pub fn quic_ep_target_add(id: u64, target: JSVal) {
    with_rooted(|s| s.quic_ep_targets.push(QuicTarget { id, target: Heap::boxed(target) }));
}

pub fn quic_ep_target(id: u64) -> Option<JSVal> {
    with_rooted(|s| s.quic_ep_targets.iter().find(|t| t.id == id).map(|t| t.target.get()))
}

/// 摘除 endpoint JS 目标（Close 派发后调；返回首次 true）。
pub fn quic_ep_target_remove(id: u64) -> bool {
    with_rooted(|s| {
        let n0 = s.quic_ep_targets.len();
        s.quic_ep_targets.retain(|t| t.id != id);
        s.quic_ep_targets.len() != n0
    })
}

/// 登记会话 JS 目标（`QuicSession` 构造时 attach）。
pub fn quic_sess_target_add(id: u64, target: JSVal) {
    with_rooted(|s| s.quic_sess_targets.push(QuicTarget { id, target: Heap::boxed(target) }));
}

pub fn quic_sess_target(id: u64) -> Option<JSVal> {
    with_rooted(|s| s.quic_sess_targets.iter().find(|t| t.id == id).map(|t| t.target.get()))
}

/// 摘除会话 JS 目标（Close 派发后调；返回首次 true）。
pub fn quic_sess_target_remove(id: u64) -> bool {
    with_rooted(|s| {
        let n0 = s.quic_sess_targets.len();
        s.quic_sess_targets.retain(|t| t.id != id);
        s.quic_sess_targets.len() != n0
    })
}

/// 存活数（监听中 endpoint + 存活会话；事件循环退出条件用）。
pub fn quic_open() -> usize {
    with_plain(|p| p.quic_open)
}

// ── QUIC 流（9g-2；半端任务各持一端，任一半终结即整流收尾）────────────────

/// 登记流（半端任务句柄随后补；`done` 防双重收尾）。
pub fn quic_stream_insert(id: u64, sess: u64, _dir: QuicStreamDir) {
    with_plain(|p| {
        p.quic_streams.insert(
            id,
            QuicStreamEntry {
                sess,
                quic_id: None,
                write_tx: None,
                read_tx: None,
                write_task: None,
                read_task: None,
                done: false,
            },
        );
    });
}

/// 补登记 quic 流 id（`StreamOpened/Accepted` 后）。
pub fn quic_stream_set_qid(id: u64, qid: u64) {
    with_plain(|p| {
        if let Some(e) = p.quic_streams.get_mut(&id) {
            e.quic_id = Some(qid);
        }
    });
}

/// 补登记半端（写端/读端任务各调一次）。
pub fn quic_stream_set_ends(
    id: u64,
    write_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::node::quic::QuicStreamCmd>>,
    write_task: Option<tokio::task::AbortHandle>,
    read_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::node::quic::QuicStreamCmd>>,
    read_task: Option<tokio::task::AbortHandle>,
) {
    with_plain(|p| {
        if let Some(e) = p.quic_streams.get_mut(&id) {
            if write_tx.is_some() {
                e.write_tx = write_tx;
                e.write_task = write_task;
            }
            if read_tx.is_some() {
                e.read_tx = read_tx;
                e.read_task = read_task;
            }
        }
    });
}

/// 发流写命令（写端已摘即 false）。
pub fn quic_stream_write_cmd(id: u64, cmd: crate::builtins::node::quic::QuicStreamCmd) -> bool {
    with_plain(|p| {
        p.quic_streams.get(&id).and_then(|e| e.write_tx.clone()).is_some_and(|tx| tx.send(cmd).is_ok())
    })
}

/// 发流读命令（目前仅 `Stop`；读端已摘即 false）。
pub fn quic_stream_read_cmd(id: u64, cmd: crate::builtins::node::quic::QuicStreamCmd) -> bool {
    with_plain(|p| {
        p.quic_streams.get(&id).and_then(|e| e.read_tx.clone()).is_some_and(|tx| tx.send(cmd).is_ok())
    })
}

/// 流收尾（abort 半端任务；`done` 置位防双收；返回是否首次）。
pub fn quic_stream_finish(id: u64) -> bool {
    with_plain(|p| match p.quic_streams.get_mut(&id) {
        Some(e) if !e.done => {
            e.done = true;
            if let Some(t) = e.write_task.take() {
                t.abort();
            }
            if let Some(t) = e.read_task.take() {
                t.abort();
            }
            e.write_tx = None;
            e.read_tx = None;
            true
        }
        _ => false,
    })
}

/// 摘除流记录（收尾后调；会话摘除时顺带清其流——任务已 abort，无泄漏）。
pub fn quic_stream_remove(id: u64) {
    with_plain(|p| {
        p.quic_streams.remove(&id);
    });
}

/// 会话名下全流 id（会话收尾时逐个 `quic_stream_finish` 用）。
pub fn quic_session_streams(sess: u64) -> Vec<u64> {
    with_plain(|p| p.quic_streams.iter().filter(|(_, e)| e.sess == sess).map(|(id, _)| *id).collect())
}

/// 登记流 JS 目标（`QuicStream` 构造时 attach）。
pub fn quic_stream_target_add(id: u64, target: JSVal) {
    with_rooted(|s| s.quic_stream_targets.push(QuicTarget { id, target: Heap::boxed(target) }));
}

pub fn quic_stream_target(id: u64) -> Option<JSVal> {
    with_rooted(|s| s.quic_stream_targets.iter().find(|t| t.id == id).map(|t| t.target.get()))
}

/// 流所属会话的 JS 目标（对端开流事件挂到会话下用）。
pub fn quic_sess_target_by_stream(stream: u64) -> Option<JSVal> {
    let sess = with_plain(|p| p.quic_streams.get(&stream).map(|e| e.sess))?;
    quic_sess_target(sess)
}

/// 摘除流 JS 目标（Close 派发后调；返回首次 true）。
pub fn quic_stream_target_remove(id: u64) -> bool {
    with_rooted(|s| {
        let n0 = s.quic_stream_targets.len();
        s.quic_stream_targets.retain(|t| t.id != id);
        s.quic_stream_targets.len() != n0
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
    // 上下文摘除时顺带摘其名下未释放模块（record 随 global 走，无悬垂）。
    with_rooted(|s| s.vm_mods.retain(|m| m.ctx != id));
    with_rooted(|s| s.vm_contexts.len() != n0)
}

// ── vm 模块（9i-1；SourceTextModule 编译产物，归属其 ctx 的 compartment）───

/// 登记模块记录，返回单调 id（`Box` 定址 §4.40）。
pub fn vm_mod_add(
    ctx: u64,
    identifier: String,
    record: *mut JSObject,
    has_imports: bool,
    deps: Vec<String>,
) -> u64 {
    let id = with_plain(|p| {
        p.vm_next_id += 1;
        p.vm_next_id
    });
    with_rooted(|s| {
        s.vm_mods.push(VmMod {
            id,
            ctx,
            identifier,
            record: Heap::boxed(record),
            has_imports,
            deps,
            linked: false,
            evaluated: false,
        });
    });
    id
}

/// 取模块（record 裸指针 + 状态快照；调用方立即重 root，中间无 GC 间隙）。
pub fn vm_mod_get(id: u64) -> Option<(*mut JSObject, u64, bool, bool, bool)> {
    with_rooted(|s| {
        s.vm_mods.iter().find(|m| m.id == id).map(|m| {
            (m.record.get(), m.ctx, m.has_imports, m.linked, m.evaluated)
        })
    })
}

/// 取模块标识（报错信息用）。
pub fn vm_mod_identifier(id: u64) -> Option<String> {
    with_rooted(|s| s.vm_mods.iter().find(|m| m.id == id).map(|m| m.identifier.clone()))
}

/// 取静态依赖表 JSON（`dependencySpecifiers` 面）。
pub fn vm_mod_deps_json(id: u64) -> Option<String> {
    with_rooted(|s| {
        s.vm_mods.iter().find(|m| m.id == id).map(|m| {
            serde_json::Value::Array(m.deps.iter().map(|d| serde_json::Value::String(d.clone())).collect())
                .to_string()
        })
    })
}

/// 置 link 位（重复 link 由 JS 壳按 status 机拦截，此处幂等）。
pub fn vm_mod_set_linked(id: u64) {
    with_rooted(|s| {
        if let Some(m) = s.vm_mods.iter_mut().find(|m| m.id == id) {
            m.linked = true;
        }
    });
}

/// 置 evaluate 位（幂等）。
pub fn vm_mod_set_evaluated(id: u64) {
    with_rooted(|s| {
        if let Some(m) = s.vm_mods.iter_mut().find(|m| m.id == id) {
            m.evaluated = true;
        }
    });
}

/// 摘除模块（重复释放 false）。
pub fn vm_mod_release(id: u64) -> bool {
    let n0 = with_rooted(|s| {
        let n0 = s.vm_mods.len();
        s.vm_mods.retain(|m| m.id != id);
        n0
    });
    with_rooted(|s| s.vm_mods.len() != n0)
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
