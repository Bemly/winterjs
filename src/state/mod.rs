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

mod child;
mod fetch;
mod net;
mod ports;
mod quic;
mod serve;
mod sqlite;
mod vm;
mod watch;
mod worker;
mod ws;

pub use child::*;
pub use fetch::*;
pub use net::*;
pub use ports::*;
pub use quic::*;
pub use serve::*;
pub use sqlite::*;
pub use vm::*;
pub use watch::*;
pub use worker::*;
pub use ws::*;


/// 一个已注册的定时器。`at` 为触发时刻（interval 为上次触发 + 间隔，漂移校正）。
/// `callback`/`args` 经 `Box` 定址（mozjs `Heap::set` 后禁移动，见 §4.40）。
/// `period` 为注册时的钳制延迟（refresh 用）；`unrefed` 为 node ref 语义位
/// （事件循环 idle 判定忽略，触发不因它豁免）。
pub struct TimerEntry {
    pub id: u32,
    pub callback: Box<Heap<JSVal>>,
    pub args: Box<Heap<JSVal>>, // JS 数组，由 prelude 打包
    pub at: Instant,
    pub interval: Option<Duration>,
    pub period: Duration,
    pub unrefed: bool,
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

/// process.nextTick 原生队列条目（args 为 JS 数组；`Box` 定址 §4.40；
/// 排空在 pump 的 RunJobs 前后各一轮——node 口径 tick/微任务双层调度）。
pub struct NextTickEntry {
    pub cb: Box<Heap<JSVal>>,
    pub args: Box<Heap<JSVal>>,
}

// SAFETY: 追踪全部 JS 值槽位。
unsafe impl Traceable for NextTickEntry {
    unsafe fn trace(&self, trc: *mut JSTracer) { unsafe {
        self.cb.trace(trc);
        self.args.trace(trc);
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
    /// 本地 pair 的待派发 wire（10f：`port_post` 本地路由——`receiveMessageOnPort`
    /// 同步可收，事件派发由 pump 逐轮统一驱动；纯 Rust 串，无 GC 值）。
    pub pending: Vec<String>,
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
    /// 读端 task 是否已退出（EOF/错后 break；写端退出时若读端已走则补 Close）。
    pub reader_done: bool,
    /// ref 计数位（10a 真计数：unref 摘循环续命，ref 装回；默认 true）。
    pub refed: bool,
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
    /// BC 同会话同步收信 pending（(sub_id, wire)；`receiveMessageOnPort` 同步
    /// 口 + pump 逐轮派发双消费，端口 pending 同款模型，10f）。
    pub bc_pending: Vec<(u64, String)>,
    pub fetch_callbacks: Vec<FetchCallback>, // 未决 fetch 的 resolve/reject（按 id 取出）
    pub fetch_streams: Vec<FetchStreamState>, // 流式 body（chunk 泵；cancel/终态时移除）
    pub make_response_fn: Heap<JSVal>, // prelude 的 __wjs_make_response
    pub make_fetch_error_fn: Heap<JSVal>, // prelude 的 __wjs_make_fetch_error
    pub ws_emit_fn: Heap<JSVal>, // prelude 的 __wjs_ws_emit
    pub uncaught_fn: Heap<JSVal>, // prelude 的 __wjs_uncaught（timer 回调未捕获异常分发）
    pub uncaught_count_fn: Heap<JSVal>, // prelude 的 __wjs_uncaught_count（监听器探针）
    pub next_ticks: Vec<NextTickEntry>, // process.nextTick 原生队列（pump RunJobs 前后各收割一轮）
    pub vm_last_error: Heap<JSVal>, // vm_run 暂存的原始异常对象（JS 侧 __vmCall 取走重建，保 realm 身份）
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
        self.vm_last_error.trace(trc);
        self.next_ticks.trace(trc);
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
        self.uncaught_fn.trace(trc);
        self.uncaught_count_fn.trace(trc);
        self.napi.trace(trc);
    }}
}

/// 不含 GC 指针的状态。
#[derive(Default)]
pub struct PlainState {
    pub next_timer_id: u32,
    pub cleared_during_fire: HashSet<u32>,
    /// 触发期 unref 侧账（entry 已摘表时旗的落点；id 单调不复用，残留无害）。
    pub unrefed_ids: HashSet<u32>,
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
    /// 存活 fs 流数（ReadStream/WriteStream 构造→close；sync 底座无原生句柄，
    /// 不计数则循环提前退出而 close 永不到，write-stream-end 套件现形）。
    pub fs_stream_open: usize,
    pub watch_drivers: HashMap<u64, (notify::RecommendedWatcher, bool)>,
    /// 各路 watch 见过的文件（Create→rename/change 二判据；close 即清）。
    pub watch_seen: HashMap<u64, HashSet<String>>,
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
    /// BoundSocket TCP 占位 listener 保活表（token → listener；close/adopt/listen 消费释放）。
    pub net_held: HashMap<u64, std::net::TcpListener>,
    pub net_hold_next: u64,
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
    /// worker env 快照（JSON 对象串；None=继承真 env——主会话/SHARE_ENV，10f）。
    pub worker_env_json: Option<String>,
    /// worker 线程专有：构造 `options.name`（`threadName` 导出用；10f）。
    pub worker_name: Option<String>,
    /// worker 线程专有：fork 子进程（有 IPC 通道，`process.send` 族不装桩；10f）。
    pub worker_is_fork: bool,
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
    /// node:sqlite worker 表（10d；turso 底座，见 node/sqlite.rs）。
    pub nsqlite_next_id: u64,
    pub nsqlite_workers: HashMap<u64, crate::builtins::node::sqlite::NodeSqliteWorker>,
    /// 在飞请求数（Head 分发 +1；End/Fail 终结 -1；常驻服务另 +1）。
    /// 收件箱不进 TLS：axum 线程经进程级全局直投（serve_bridge 全局端点，§4.153）。
    pub serve_open: usize,
    /// 在飞响应通道（id → head/body 发送端；终结即摘，见 `serve_take`）。
    pub serve_resps: HashMap<u64, crate::serve_bridge::ServeRespTx>,
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

/// 未处理 rejection 表计数（event_loop 检查点用：非空即 fatal 跳出，
/// §4.70 姊妹案——开着的句柄让循环永不 idle 时，循环尾收割永不到）。
pub fn unhandled_pending() -> usize {
    with_rooted(|s| s.unhandled.len())
}

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
        // 10f：worker 内非对象 rejection 走原始值信封（error-primitive 套件
        // 断同一性；主进程完成值/报错显示不受影响）。
        if !worker_is_main()
            && let Some(m) = crate::jsapi_glue::exc_prim_marker(cx, reason)
        {
            return m;
        }
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

/// napi 异步 keep-alive（未完成 async_work + refed TSFN；事件循环退出条件用）。
pub fn napi_pending() -> usize {
    crate::napi::asyncwork::pending_count()
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
    /// 超时杀已作用（Exited 事件带上，prelude 据此落 `killed` 位——exec timeout
    /// 系 err.killed=true 断言；10f）。
    pub timed_out: bool,
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

    /// serve 配对：在飞 +1/终结 -1，重复终结不重复减，head 通道单次消费。
    #[test]
    #[serial]
    fn serve_head_take_pairing() {
        use crate::serve_bridge::{ServeBodyMsg, ServeRespHead, ServeRespTx};
        fn resp_tx() -> ServeRespTx {
            let (head_tx, _head_rx) = tokio::sync::oneshot::channel();
            let (body_tx, _body_rx) = tokio::sync::mpsc::unbounded_channel();
            ServeRespTx { head_tx: Some(head_tx), body_tx, upgrade_tx: None }
        }
        let base = serve_open();
        serve_hold_server();
        assert_eq!(serve_open(), base + 1);
        // Head 落账 +1；重复 Head 覆盖不重复加。
        serve_head(501, resp_tx());
        assert_eq!(serve_open(), base + 2);
        serve_head(501, resp_tx());
        assert_eq!(serve_open(), base + 2);
        // head 通道单次消费：首次 Some，二次 None（表项与计数保留）。
        assert!(serve_take_head(501).is_some());
        assert!(serve_take_head(501).is_none());
        assert_eq!(serve_open(), base + 2);
        assert!(serve_body_tx(501).is_some());
        // 终结摘表 -1；重复终结回 None 且计数不动。
        assert!(serve_take(501).is_some());
        assert_eq!(serve_open(), base + 1);
        assert!(serve_take(501).is_none());
        assert_eq!(serve_open(), base + 1);
        assert!(serve_body_tx(501).is_none());
        serve_release_server();
        assert_eq!(serve_open(), base);
        // 消息形断言（改枚举即红）。
        assert_eq!(ServeRespHead::internal_error().status, 500);
        assert!(matches!(ServeBodyMsg::End, ServeBodyMsg::End));
    }
}
