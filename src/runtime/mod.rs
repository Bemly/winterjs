//! mozjs 边界胶水：引擎初始化、Realm、prelude、脚本求值、事件循环。
//! 本模块是 §6 允许 `unsafe` 的唯一区域（rooting / AutoRealm / FFI），
//! builtins 里的 unsafe 仅限 JSNative 调用帧访问。
//!
//! 事件循环（Phase 1）：
//!   RunJobs 排空微任务（内部 job queue）→ 若有定时器则睡到最近触发时刻 →
//!   触发到期定时器 → 循环；两者皆空即退出（不饿死、不早退）。

use std::ffi::CString;
use std::ptr;
use std::sync::OnceLock;

use mozjs::conversions::{ConversionResult, FromJSValConvertible as _};
use mozjs::gc::RootedGuard;
use mozjs::jsapi::{
    AddPromiseReactions, JS_ClearPendingException, JS_GetProperty, JSObject,
    OnNewGlobalHookOption, PromiseRejectionHandlingState, RunJobs,
    SetPromiseRejectionTrackerCallback,
};
use mozjs::jsval::UndefinedValue;
use mozjs::realm::AutoRealm;
use mozjs::rooted;
use mozjs::rust::{
    CompileOptionsWrapper, RealmOptions, SIMPLE_GLOBAL_CLASS, error_info_from_exception_stack,
    evaluate_script, JSEngine, JSEngineHandle, Runtime,
};
use mozjs::rust::wrappers2::JS_NewGlobalObject;


use crate::builtins;
use crate::builtins::timers;
use crate::modules;use crate::error::Error;
use crate::jsapi_glue::{exc_name, get_prop_string, get_prop_u32, raw_handle, raw_handle_mut, value_to_string};
use crate::state;

mod entry;
mod repl;
mod runner;
mod serve_session;
mod worker_spawn;

pub use entry::*;
pub use repl::*;
pub use runner::*;
pub use serve_session::*;
pub use worker_spawn::*;


#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    /// 文件脚本：求值完成值，事件循环跑完 timers/microtasks 后退出
    Script,
    /// eval：先按普通脚本求值；仅当因顶层 await 语法失败时，用 async IIFE 重包
    Eval,
}

/// 内层（引擎生命周期；`end_session` 在各返回点收尾，见 §4.8/§4.24）。
/// 初始化好的会话（引擎 + realm + 内建 + 通道接收端）。
/// `global_ptr` 为裸指针：调用方必须在任何 JSAPI 调用前立即重 root
/// （`rooted!`），中间不得有 await/JSAPI（无 GC 间隙），见调用点 SAFETY。
/// `state_guard` 必须与 `rt` 同寿（TLS 状态先于引擎销毁，见 `state`）。
struct SessionInit {
    rt: Runtime,
    engine: JSEngineHandle,
    global_ptr: *mut JSObject,
    state_guard: state::StateGuard,
    fetch_rx: tokio::sync::mpsc::UnboundedReceiver<crate::builtins::fetch::FetchMsg>,
    ws_rx: tokio::sync::mpsc::UnboundedReceiver<crate::builtins::ws::WsEvent>,
    watch_rx: tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::fs::WatchEvent>,
    child_rx: tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::child::ChildEvent>,
    net_rx: tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::net::NetEvent>,
    worker_rx: tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::worker::WorkerEvent>,
    quic_rx: tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::quic::QuicEvent>,
    napi_rx: tokio::sync::mpsc::UnboundedReceiver<crate::napi::asyncwork::NapiEvent>,
    dispatch_rx: tokio::sync::mpsc::UnboundedReceiver<usize>,
}

/// 进程级引擎单例：`JSEngine::init()` 每进程只能成功一次（第二次起
/// `AlreadyInitialized`），而 test runner 多文件 / test --watch 都要在同进程
/// 反复 `runtime::run`。本体 init 一次后刻意泄漏（永不 shutdown，§4.8），
/// handle（Clone）给每次 run 复用（§4.24）。
fn engine_handle() -> Result<JSEngineHandle, Error> {
    static HANDLE: OnceLock<JSEngineHandle> = OnceLock::new();
    if let Some(h) = HANDLE.get() {
        return Ok(h.clone());
    }
    let engine = JSEngine::init().map_err(|_| Error::Other("failed to init JS engine".into()))?;
    let handle = engine.handle();
    let _ = HANDLE.set(handle.clone());
    // 本体 forget：Drop 会触发 JS_ShutDown，之后引擎在本进程不可再用（§4.8）。
    std::mem::forget(engine);
    Ok(handle)
}

/// 会话初始化（引擎/realm/内建/prelude/通道；`run` 与 `repl` 共用）。
/// 同步函数：内部无 await（prelude 求值全同步），返回即交接，无 GC 间隙。
fn init_session(argv: Vec<String>) -> Result<SessionInit, Error> {
    // JS engine handle 进程级单例（见 `engine_handle`；每次 run 复用同一引擎）。
    let engine = engine_handle()?;
    let mut rt = Runtime::new(engine.clone());
    // 会话 cx 裸指针（指针值会话期稳定；worker 中断槽绑定用——Runtime 经 §4.8
    // 刻意泄漏，指针进程生命期有效）。SAFETY: 仅取指针值，不据此执行 JSAPI。
    let raw_cx = unsafe { rt.cx().raw_cx() };
    // TLS 状态必须先于引擎销毁（见 state::shutdown 文档）
    let state_guard = state::StateGuard;
    modules::install_hooks(&rt);

    // SAFETY: 引擎初始化后、首段脚本前启用内部 job queue（JS shell 同款），
    // Promise 微任务由此排队，RunJobs 排空。
    // SharedArrayBuffer + Atomics（Node 全局形态；jsdom 等生态直引用）。
    let mut options = RealmOptions::default();
    options.creationOptions_.sharedMemoryAndAtomics_ = true;
    rooted!(&in(rt.cx()) let global = unsafe {
        JS_NewGlobalObject(
            rt.cx(),
            &SIMPLE_GLOBAL_CLASS,
            ptr::null_mut(),
            OnNewGlobalHookOption::FireOnNewGlobalHook,
            &*options,
        )
    });

    // §4.1：进入 global realm 后再做 JSAPI 初始化（内建、prelude、rejection 追踪器）
    let dispatch_rx;
    {
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        // SAFETY: realm 内启用内部 job queue（realm 外调用会 SEGV）；
        // 不启用则 RunJobs 无队列可用，同样 SEGV（AGENTS §4.7 记坑）。
        unsafe { crate::jobqueue::install((&mut realm).raw_cx()) };
        // SM 异步任务派发桥（wasm compile 等）：接收端交事件循环，
        // 闭包指针进 PlainState（pending 计数读取 + 会话身份）。
        let (drx, closure_ptr) =
            // SAFETY: realm 内取 raw cx（jobqueue 装配同款）
            unsafe { crate::dispatch::install((&mut realm).raw_cx()) };
        dispatch_rx = drx;
        state::with_plain(|p| p.dispatch_closure = Some(closure_ptr));
        state::init(&mut realm);
        state::set_global(global.get());
        state::set_line_adjust(0);
        state::set_argv(argv);
        // PlainState 跨 run 复用（test runner 同进程多文件）：上一会话的
        // sqlite worker 端点全部摘除，线程在 channel 断开后自退；storage 同理
        //（模块自有静态表，见 builtins/storage.rs）。
        state::sqlite_reset();
        crate::builtins::storage::reset_session();
        builtins::define_all(&mut realm, global.get())?;

        // SAFETY: realm 内；追踪器仅在 JS 线程被引擎回调
        unsafe {
            SetPromiseRejectionTrackerCallback(
                (&mut realm).raw_cx(),
                Some(rejection_tracker),
                ptr::null_mut(),
            );
        }
        // terminate 中断钩子（10f，仅 worker 会话）：JS_AddInterruptCallback——
        // 回调只读共享终止旗（忙循环斩断；idle 路径照旧走事件循环检查点）。
        if WORKER_BOOT.with(|b| b.borrow().is_some()) {
            // SAFETY: realm 内取 raw cx（jobqueue 装配同款），指针即时使用。
            let raw = unsafe { (&mut realm).raw_cx() };
            crate::builtins::node::worker::term_install(raw);
        }

        let prelude_filename = CString::new("__wjs_prelude.js").expect("no NUL");
        let prelude_options = CompileOptionsWrapper::new(&realm, prelude_filename, 1);
        rooted!(&in(&mut realm) let mut prelude_rval = UndefinedValue());
        // prelude 是项目自带常量脚本，语法必然正确
        let ok = evaluate_script(
            &mut realm,
            global.handle(),
            builtins::PRELUDE.as_str(),
            prelude_rval.handle_mut(),
            prelude_options,
        );
        if ok.is_err() {
            return Err(pending_error_in_realm(
                &mut realm,
                builtins::PRELUDE.as_str(),
                "__wjs_prelude.js",
                0,
            ));
        }

        // 线程身份落地（10f 前移到 node prelude 之前：process.env 代理构建期
        // 就要读 env 快照——worker 会话带快照、主会话真 env）。纯 state 操作
        // 无 JSAPI；worker 中断钩子 cx 绑定同批（Runtime §4.8 泄漏保活）。
        match WORKER_BOOT.with(|b| b.borrow_mut().take()) {
            Some(boot) => {
                let wid = boot.worker_id;
                crate::builtins::node::worker::worker_boot_from_slot(boot);
                crate::builtins::node::worker::term_bind_cx(wid, raw_cx);
            }
            None => state::worker_session_init(true, 0),
        }
        // Phase 4a：node 全局（process；同“语法必然正确”约定，失败即内部错）。
        let node_prelude = crate::builtins::node::node_prelude();
        let node_filename = CString::new("__wjs_node_prelude.js").expect("no NUL");
        let node_options = CompileOptionsWrapper::new(&realm, node_filename, 1);
        rooted!(&in(&mut realm) let mut node_rval = UndefinedValue());
        let ok = evaluate_script(
            &mut realm,
            global.handle(),
            &node_prelude,
            node_rval.handle_mut(),
            node_options,
        );
        if ok.is_err() {
            return Err(pending_error_in_realm(
                &mut realm,
                &node_prelude,
                "__wjs_node_prelude.js",
                0,
            ));
        }

        // 缓存 prelude 辅助函数值（timers/structuredClone/fetch/ws 交付要用）
        for (prop, idx) in [
            (c"__wjs_call", 0u8),
            (c"__wjs_entries", 1u8),
            (c"__wjs_make_response", 2u8),
            (c"__wjs_make_fetch_error", 3u8),
            (c"__wjs_ws_emit", 4u8),
            (c"__wjs_uncaught", 5u8),
            (c"__wjs_uncaught_count", 6u8),
        ] {
            rooted!(&in(&mut realm) let mut v = UndefinedValue());
            // SAFETY: global 为有效 rooted 对象
            let ok = unsafe {
                JS_GetProperty((&mut realm).raw_cx(), raw_handle(global.as_ptr()), prop.as_ptr(), raw_handle_mut(v.as_ptr()))
            };
            if !ok || !v.is_object() {
                return Err(Error::Other(format!(
                    "prelude helper {} missing",
                    prop.to_str().unwrap_or("?")
                )));
            }
            let got = v.get();
            state::with_rooted(|s| match idx {
                0 => s.call_fn.set(got),
                1 => s.entries_fn.set(got),
                2 => s.make_response_fn.set(got),
                3 => s.make_fetch_error_fn.set(got),
                4 => s.ws_emit_fn.set(got),
                5 => s.uncaught_fn.set(got),
                _ => s.uncaught_count_fn.set(got),
            });
        }
    }
    let global_ptr = global.get();

    // fetch/ws 驱动端点：发送端进 TLS，接收端由调用方持有并传给事件循环
    let (fetch_tx, fetch_rx) = tokio::sync::mpsc::unbounded_channel();
    state::with_plain(|p| p.fetch_tx = Some(fetch_tx));
    let (ws_tx, ws_rx) = tokio::sync::mpsc::unbounded_channel();
    state::with_plain(|p| p.ws_tx = Some(ws_tx));
    let (watch_tx, watch_rx) = tokio::sync::mpsc::unbounded_channel();
    state::with_plain(|p| p.watch_tx = Some(watch_tx));
    let (child_tx, child_rx) = tokio::sync::mpsc::unbounded_channel();
    state::with_plain(|p| p.child_tx = Some(child_tx));
    let (net_tx, net_rx) = tokio::sync::mpsc::unbounded_channel();
    state::with_plain(|p| p.net_tx = Some(net_tx));
    let (worker_tx, worker_rx) = tokio::sync::mpsc::unbounded_channel();
    state::with_plain(|p| p.worker_tx = Some(worker_tx));
    // 会话序号（BC 自发排除等跨会话寻址用；每会话一次，线程生灭即换号）。
    state::session_seq_init();
    let (quic_tx, quic_rx) = tokio::sync::mpsc::unbounded_channel();
    state::with_plain(|p| p.quic_tx = Some(quic_tx));
    // napi 第 8 通道（async_work/TSFN；Sender 由 create 时克隆进 rec）
    let (napi_tx, napi_rx) = tokio::sync::mpsc::unbounded_channel();
    state::with_plain(|p| p.napi_tx = Some(napi_tx));
    // worker boot 收尾放 init 末（主会话无操作；worker 回传收件箱 + 发 Online）。
    crate::builtins::node::worker::worker_booted();

    Ok(SessionInit { rt, engine, global_ptr, state_guard, fetch_rx, ws_rx, watch_rx, child_rx, net_rx, worker_rx, quic_rx, napi_rx, dispatch_rx })
}

/// 会话收尾（各返回点调用）：刻意泄漏 Runtime 与 engine handle（§4.8 —— 正常
/// drop 在带 timer/microtask 残留的路径上，JS_DestroyContext 的收尾 GC 即 SEGV，
/// 实测复现）。多 run 进程（test runner 多文件 / test --watch）的隔离靠
/// `run_isolated` 的每文件独立线程：CONTEXT TLS 随线程消亡，下一次
/// `Runtime::new` 不受影响（§4.24 —— 单线程内建第二个 Runtime 会直接炸）。
fn end_session(rt: Runtime, engine: JSEngineHandle) {
    // napi env cleanup hooks（M4）：JS 线程 + 引擎存活期内的最后收敛点
    //（hook 无 env 参、不可能进 JSAPI——lifecycle.rs 模块头注）。
    crate::napi::lifecycle::run_cleanup_hooks();
    crate::napi::lifecycle::run_wrap_finalizers();
    std::mem::forget(rt);
    std::mem::forget(engine);
}

/// 事件循环错误源：脚本（源码直给）或模块（按异常文件名查调试信息回映射）。
#[derive(Clone, Copy)]
pub(crate) enum ErrorSource<'a> {
    Script { source: &'a str, filename: &'a str },
    Module { url: &'a str },
}

/// 单轮推进统计（`pump_once` 返回；调用方累加记数）。
#[derive(Default)]
struct PumpStats {
    /// `process.exit` 已调（调用方收尾退出，见 §4.18 检查点顺序）。
    exited: bool,
    /// 入口 promise 已决议失败（调用方跳出收割上报，不等自然排空）。
    entry_failed: bool,
    /// 本轮结算过（§4.18：结算后必须再跑一轮 RunJobs，不可直接退）。
    progressed: bool,
    timers: usize,
    /// 本轮触发的 unrefed 定时器数（不计入 progressed——存活判定与 node
    /// uv_loop_alive 同口径只看 refed 面；套件 unref.js 的 1ms unrefed
    /// interval 否则空转到 LONG_TIME 才退）。
    timers_unrefed: usize,
    fetch: usize,
    ws: usize,
    watch: usize,
    child: usize,
    net: usize,
    worker: usize,
    quic: usize,
    napi: usize,
    dispatch: usize,
}

/// 事件循环单轮推进：RunJobs 排空 → exit 检查 → 同步结算 → 到期 timer 触发。
/// park/等待由调用方做（`event_loop` 跑到 idle，`repl` 回 select 等输入）。
async fn pump_once(
    rt: &mut Runtime,
    global: &RootedGuard<'_, *mut JSObject>,
    err: ErrorSource<'_>,
    fetch_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::fetch::FetchMsg>,
    ws_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::ws::WsEvent>,
    watch_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::fs::WatchEvent>,
    child_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::child::ChildEvent>,
    net_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::net::NetEvent>,
    worker_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::worker::WorkerEvent>,
    quic_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::quic::QuicEvent>,
    napi_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::napi::asyncwork::NapiEvent>,
    dispatch_rx: &mut tokio::sync::mpsc::UnboundedReceiver<usize>,
    unrefed_when_idle: bool,
) -> Result<PumpStats, Error> {
    use crate::builtins::{fetch, node::child as node_child, node::fs as node_fs, node::net as node_net, node::quic as node_quic, node::worker as node_worker, ws};
    use crate::napi::asyncwork as napi_aw;
    let mut st = PumpStats::default();
    // SM 异步任务派发（wasm compile/instantiate 完成回调）：先运行再 RunJobs——
    // DispatchableRun 内部只 resolve promise（结算排 microtask），同一轮
    // RunJobs 排空反应 job（§4.18：结算点后到 park 前必有 RunJobs）。
    while let Ok(ptr) = dispatch_rx.try_recv() {
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        // SAFETY: realm 内取 raw cx（jobqueue 装配同款）
        unsafe { crate::dispatch::run_dispatchable((&mut realm).raw_cx(), ptr) };
        st.dispatch += 1;
        st.progressed = true;
    }
    // nextTick/微任务双层排空（node 口径，10f）：先收割同步期入队的 tick，
    // 再 RunJobs 排微任务；微任务期新入队的 tick（promise 链内的 nextTick）由
    // 循环再次收割——即 node 的"微任务排空后才跑它们"语义（V8 checkpoint
    // 原子性；queueMicrotask 同队列 FIFO 做不到，compose/pipeline 对拍现形）。
    loop {
        {
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            crate::builtins::node::process_::drain_next_ticks(&mut realm, global.get(), err)?;
        }
        {
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            // SAFETY: realm 内排空内部 job queue
            unsafe { RunJobs((&mut realm).raw_cx()) };
        }
        if state::with_rooted(|s| s.next_ticks.is_empty()) {
            break;
        }
    }
    // process.exit() 被 catch 后的兜底：检查点照退（`run` 外层转 Exit）。
    // 注意顺序：必须在 RunJobs 之后——抛错的 job 会截断当轮排空，
    // 反应 job 留到下一轮；检查放排空前即饿死它们（实测：模块顶层 exit 必发）。
    if state::with_plain(|p| p.process_exited.is_some()) {
        st.exited = true;
        return Ok(st);
    }
    // worker 终止旗（`WTerminate` 置位；与 process.exit 同检查点顺序——RunJobs 之后）。
    if state::worker_terminated() {
        st.exited = true;
        return Ok(st);
    }
    // 入口 promise 已决议失败（顶层 `await import` 炸等）：Node 口径即 fatal——
    // 不等事件循环自然排空（开着的句柄如 worker 端口会让循环永不 idle，
    // fork 缺失模块即挂死于此）；置旗由 event_loop 跳出，收割路径照常上报。
    // 注意顺序：同上在 RunJobs 之后；`process.exit` 优先（既有语义不动）。
    if state::with_plain(|p| p.entry_rejection.is_some()) {
        st.entry_failed = true;
        return Ok(st);
    }

    // 已完成的 fetch/ws 先结算（不阻塞）。结算会同步决议 promise（排队 microtask），
    // 故本轮结算过就不能直接退——必须再跑一轮 RunJobs 排空（§4.18）。
    while let Ok(msg) = fetch_rx.try_recv() {
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        fetch::settle(&mut realm, global.get(), msg, err)?;
        st.fetch += 1;
        st.progressed = true;
    }
    while let Ok(ev) = ws_rx.try_recv() {
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        ws::dispatch(&mut realm, global.get(), ev, err)?;
        st.ws += 1;
        st.progressed = true;
    }
    while let Ok(ev) = watch_rx.try_recv() {
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        node_fs::dispatch(&mut realm, global.get(), ev, err)?;
        st.watch += 1;
        st.progressed = true;
    }
    while let Ok(ev) = child_rx.try_recv() {
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        node_child::dispatch(&mut realm, global.get(), ev, err)?;
        st.child += 1;
        st.progressed = true;
    }
    while let Ok(ev) = net_rx.try_recv() {
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        node_net::dispatch(&mut realm, global.get(), ev, err)?;
        st.net += 1;
        st.progressed = true;
    }
    while let Ok(ev) = worker_rx.try_recv() {
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        node_worker::dispatch(&mut realm, global.get(), ev, err)?;
        st.worker += 1;
        st.progressed = true;
    }
    // 本地 pair 的 pending wire（10f：postMessage 本地路由直投表；pump 逐轮
    // 派发保持 task 级节奏——纯微任务链式 ping-pong 会饿死定时器）。
    for (to, wire) in state::take_port_pending() {
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        node_worker::dispatch(&mut realm, global.get(), crate::builtins::node::worker::WorkerEvent::PortMsg { to, json: wire }, err)?;
        st.worker += 1;
        st.progressed = true;
    }
    // BC 同会话 pending（10f：bc_pub 本会话路由直投表；派发语义与跨会话 BcMsg 同）。
    for (to, wire) in state::take_bc_pending() {
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        node_worker::dispatch(&mut realm, global.get(), crate::builtins::node::worker::WorkerEvent::BcMsg { to, json: wire }, err)?;
        st.worker += 1;
        st.progressed = true;
    }
    while let Ok(ev) = quic_rx.try_recv() {
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        node_quic::dispatch(&mut realm, global.get(), ev, err)?;
        st.quic += 1;
        st.progressed = true;
    }
    while let Ok(ev) = napi_rx.try_recv() {
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        napi_aw::dispatch(&mut realm, global.get(), ev, err)?;
        st.napi += 1;
        st.progressed = true;
    }

    {
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        // node `uv_run`：循环不 alive 即一相不跑——unref 项（含 0ms 的 unref immediate）
        // 只在他者续命时触发（immediate-unref 三套件）。REPL/serve 常驻，照触发。
        let allow_unrefed = unrefed_when_idle || st.progressed || !loop_idle();
        let (fired, fired_unrefed) = timers::fire_due(&mut realm, global.get(), err, allow_unrefed)?;
        st.timers = fired;
        st.timers_unrefed = fired_unrefed;
    }
    Ok(st)
}

/// 存活判定（node `uv_loop_alive` 口径：只看 refed 面——refed 定时器与各类未决句柄）。
fn loop_idle() -> bool {
    timers::next_deadline().is_none()
        && state::fetch_pending() == 0
        && state::ws_open() == 0
        && state::stream_pending() == 0
        && state::watch_open() == 0
        && state::child_open() == 0
        && state::net_open() == 0
        && state::worker_open() == 0
        && state::quic_open() == 0
        && state::napi_pending() == 0
        && crate::dispatch::pending() == 0
}

/// 事件循环：RunJobs 排空微任务 → 等（最近定时器 / fetch / ws 先到者）→
/// 结算完成项 → 触发到期定时器，直到定时器、未决 fetch、存活 ws 皆空。
async fn event_loop(
    rt: &mut Runtime,
    global: &RootedGuard<'_, *mut JSObject>,
    err: ErrorSource<'_>,
    fetch_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::fetch::FetchMsg>,
    ws_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::ws::WsEvent>,
    watch_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::fs::WatchEvent>,
    child_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::child::ChildEvent>,
    net_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::net::NetEvent>,
    worker_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::worker::WorkerEvent>,
    quic_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::quic::QuicEvent>,
    napi_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::napi::asyncwork::NapiEvent>,
    dispatch_rx: &mut tokio::sync::mpsc::UnboundedReceiver<usize>,
) -> Result<(), Error> {
    use crate::builtins::{fetch, node::child as node_child, node::fs as node_fs, node::net as node_net, node::quic as node_quic, node::worker as node_worker, ws};
    use crate::napi::asyncwork as napi_aw;
    let mut iterations: u64 = 0;
    let mut timers_fired: usize = 0;
    let mut unrefed_grace = false;
    let mut fetches_settled: usize = 0;
    let mut ws_settled: usize = 0;
    let mut watches_settled: usize = 0;
    let mut children_settled: usize = 0;
    let mut nets_settled: usize = 0;
    let mut workers_settled: usize = 0;
    let mut quics_settled: usize = 0;
    let mut napis_settled: usize = 0;
    let mut dispatches_settled: usize = 0;
    macro_rules! settle_fetch {
        ($msg:expr) => {{
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            fetch::settle(&mut realm, global.get(), $msg, err)?;
            fetches_settled += 1;
        }};
    }
    macro_rules! settle_ws {
        ($ev:expr) => {{
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            ws::dispatch(&mut realm, global.get(), $ev, err)?;
            ws_settled += 1;
        }};
    }
    macro_rules! settle_child {
        ($ev:expr) => {{
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            node_child::dispatch(&mut realm, global.get(), $ev, err)?;
            children_settled += 1;
        }};
    }
    macro_rules! settle_net {
        ($ev:expr) => {{
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            node_net::dispatch(&mut realm, global.get(), $ev, err)?;
            nets_settled += 1;
        }};
    }
    macro_rules! settle_worker {
        ($ev:expr) => {{
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            node_worker::dispatch(&mut realm, global.get(), $ev, err)?;
            workers_settled += 1;
        }};
    }
    macro_rules! settle_napi {
        ($ev:expr) => {{
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            napi_aw::dispatch(&mut realm, global.get(), $ev, err)?;
            napis_settled += 1;
        }};
    }
    macro_rules! settle_quic {
        ($ev:expr) => {{
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            node_quic::dispatch(&mut realm, global.get(), $ev, err)?;
            quics_settled += 1;
        }};
    }
    macro_rules! settle_dispatch {
        ($ptr:expr) => {{
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            // SAFETY: realm 内取 raw cx（jobqueue 装配同款）
            unsafe { crate::dispatch::run_dispatchable((&mut realm).raw_cx(), $ptr) };
            dispatches_settled += 1;
        }};
    }
    macro_rules! settle_watch {
        ($ev:expr) => {{
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            node_fs::dispatch(&mut realm, global.get(), $ev, err)?;
            watches_settled += 1;
        }};
    }
    // 排障：`WINTERJS_HANG_EXIT=<秒>`（sweep 缺省 20）——事件循环持续无进展超时即发
    // process 'exit'（node 套件 common 的 mustCall 核对随之打印"哪个回调没被调"及其
    // 创建栈），再以 1 退出。把 TIMEOUT 件变成带定位的红件；未设即不启用（默认语义不变）。
    let hang_limit = std::env::var("WINTERJS_HANG_EXIT")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|&n| n > 0)
        .map(std::time::Duration::from_secs);
    let mut last_progress = std::time::Instant::now();
    let mut before_exit_sent = false;
    loop {
        // 单轮推进与 `repl` 共用（§4.18 检查点顺序在内保持）。
        let st = pump_once(rt, global, err, fetch_rx, ws_rx, watch_rx, child_rx, net_rx, worker_rx, quic_rx, napi_rx, dispatch_rx, false).await?;
        if st.exited {
            return Ok(());
        }
        // 入口已失败即跳出（收割路径上报；未处理 rejection 收尾不受影响——
        // 入口带专用捕获，不进 `unhandled` 表，见 `run_module`）。
        if st.entry_failed {
            break;
        }
        // 未处理 rejection 非空即跳出（node 口径：checkpoint 末仍无处理即 fatal，
        // 不等自然排空——开着的句柄（子进程/socket）会让循环永不 idle，循环尾的
        // report_unhandled_rejections 永不到（§4.70 姊妹案，eval/module 双路同修）；
        // 上报路径不变，仍由本函数尾收割。tracker Handled 已在表内摘除，
        // checkpoint 时非空 = 整轮排空后确无处理。REPL 不走此检查（逐轮收割
        // 不退出，node REPL 同款）。
        if state::unhandled_pending() > 0 {
            break;
        }
        iterations += 1;
        timers_fired += st.timers;
        fetches_settled += st.fetch;
        ws_settled += st.ws;
        watches_settled += st.watch;
        children_settled += st.child;
        nets_settled += st.net;
        workers_settled += st.worker;
        quics_settled += st.quic;
        napis_settled += st.napi;
        dispatches_settled += st.dispatch;
        // timer 触发同样排队 microtask（回调内决议 promise），必须算 progress，
        // 否则 idle 检查提前退出、反应 job 被丢（§4.18 同类，TLA 必挂）。
        // unrefed 触发不算推进（node 存活判定只看 refed 面）：否则 idle 后的
        // 1ms unrefed interval 每轮都到点，循环空转到 LONG_TIME 才退（套件
        // unref.js 现形）。其 microtask 由下方 grace 轮保证排空。
        let progressed = st.progressed || st.timers > st.timers_unrefed;
        let progressed_unrefed_only =
            !st.progressed && st.timers > 0 && st.timers == st.timers_unrefed;
        if progressed || st.timers > 0 {
            last_progress = std::time::Instant::now();
        }

        let idle = loop_idle();
        if idle && !progressed {
            if progressed_unrefed_only && !unrefed_grace {
                // §4.18 完整形态：unrefed 回调排的 microtask 也要一轮 RunJobs
                // ——给一轮宽限再退，不无限宽限（否则 unrefed interval 空转）。
                unrefed_grace = true;
                continue;
            }
            // node `SpinEventLoop`：排空 → 发 'beforeExit' → 仍 alive 则续转，否则退。
            // 每次排空只发一次；监听排了新任务（循环再非 idle）才复位再发。
            if !before_exit_sent {
                before_exit_sent = true;
                let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
                if crate::builtins::node::process_::queue_before_exit(&mut realm, global.get()) {
                    continue;
                }
            }
            break;
        }
        before_exit_sent = false;
        // §4.18 推广：本轮结算/触发过就不能直接 park——结算可能只排了 microtask
        // （如 worker 端口 `__ev` 的 queueMicrotask），park 进 select 即再无 RunJobs
        // 机会（无 timer 时直接 hang，有 timer 则延迟到 sleep 醒才送达）。
        // 回顶下一轮 pump 先 RunJobs 排空；无新进展即 park，不忙转。
        if progressed {
            continue;
        }
        if let Some(lim) = hang_limit {
            if last_progress.elapsed() >= lim {
                hang_exit(rt, global, lim);
                return Err(Error::Exit(1));
            }
        }
        // park 唤醒目标用 next_wake（含 unrefed：到点须醒去触发，套件
        // unrefd-interval-still-fires）；存活/idle 判定上面已用 refed-only
        // 的 next_deadline 定案，走到这里说明循环确有存活理由。
        let hang_at = hang_limit.map(|l| last_progress + l);
        let wake = match (timers::next_wake(), hang_at) {
            (Some(a), Some(h)) => Some(a.min(h)),
            (a, None) => a,
            (None, h) => h,
        };
        match wake {
            Some(at) => {
                let tokio_at = tokio::time::Instant::from_std(at);
                tokio::select! {
                    _ = tokio::time::sleep_until(tokio_at) => {}
                    msg = fetch_rx.recv() => {
                        if let Some(msg) = msg {
                            settle_fetch!(msg);
                        }
                    }
                    ev = ws_rx.recv() => {
                        if let Some(ev) = ev {
                            settle_ws!(ev);
                        }
                    }
                    wev = watch_rx.recv() => {
                        if let Some(wev) = wev {
                            settle_watch!(wev);
                        }
                    }
                    cev = child_rx.recv() => {
                        if let Some(cev) = cev {
                            settle_child!(cev);
                        }
                    }
                    nev = net_rx.recv() => {
                        if let Some(nev) = nev {
                            settle_net!(nev);
                        }
                    }
                    wev2 = worker_rx.recv() => {
                        if let Some(wev2) = wev2 {
                            settle_worker!(wev2);
                        }
                    }
                    qev = quic_rx.recv() => {
                        if let Some(qev) = qev {
                            settle_quic!(qev);
                        }
                    }
                    nev2 = napi_rx.recv() => {
                        if let Some(nev2) = nev2 {
                            settle_napi!(nev2);
                        }
                    }
                    dptr = dispatch_rx.recv() => {
                        if let Some(ptr) = dptr {
                            settle_dispatch!(ptr);
                        }
                    }
                }
            }
            // 无定时器但有未决项：睡到有完成为止（到此必非 idle——全 idle 只剩
            // microtask 时上方的 `progressed` 分支已回顶排空，不会 park 永睡）。
            None => {
                tokio::select! {
                    msg = fetch_rx.recv() => {
                        if let Some(msg) = msg {
                            settle_fetch!(msg);
                        }
                    }
                    ev = ws_rx.recv() => {
                        if let Some(ev) = ev {
                            settle_ws!(ev);
                        }
                    }
                    wev = watch_rx.recv() => {
                        if let Some(wev) = wev {
                            settle_watch!(wev);
                        }
                    }
                    cev = child_rx.recv() => {
                        if let Some(cev) = cev {
                            settle_child!(cev);
                        }
                    }
                    nev = net_rx.recv() => {
                        if let Some(nev) = nev {
                            settle_net!(nev);
                        }
                    }
                    wev2 = worker_rx.recv() => {
                        if let Some(wev2) = wev2 {
                            settle_worker!(wev2);
                        }
                    }
                    qev = quic_rx.recv() => {
                        if let Some(qev) = qev {
                            settle_quic!(qev);
                        }
                    }
                    nev2 = napi_rx.recv() => {
                        if let Some(nev2) = nev2 {
                            settle_napi!(nev2);
                        }
                    }
                    dptr = dispatch_rx.recv() => {
                        if let Some(ptr) = dptr {
                            settle_dispatch!(ptr);
                        }
                    }
                }
            }
        }
    }
    tracing::info!(target: "winterjs::runtime", iterations, timers_fired, fetches_settled, ws_settled, watches_settled, children_settled, nets_settled, workers_settled, quics_settled, napis_settled, dispatches_settled, "event loop drained");

    report_unhandled_rejections(rt, global)
}

/// 未处理 rejection 收尾上报（Node 式 fatal）：挂捕获 reactions → 再排空一轮。
/// `event_loop` 尾与 `repl` 每轮共用；REPL 侧出错只打印不退出（调用方定）。
fn report_unhandled_rejections(
    rt: &mut Runtime,
    global: &RootedGuard<'_, *mut JSObject>,
) -> Result<(), Error> {
    let unhandled = state::with_rooted(|s| {
        s.unhandled
            .iter()
            .map(|h| h.get())
            .collect::<Vec<*mut JSObject>>()
    });
    if !unhandled.is_empty() {
        tracing::warn!(target: "winterjs::runtime", count = unhandled.len(), "unhandled rejections detected");
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        let (on_fulfilled, on_rejected) = state::capture_native_values();
        for promise_obj in unhandled {
            rooted!(&in(&mut realm) let promise = promise_obj);
            rooted!(&in(&mut realm) let ful_obj: *mut JSObject = on_fulfilled.to_object());
            rooted!(&in(&mut realm) let rej_obj: *mut JSObject = on_rejected.to_object());
            // SAFETY: promise/回调均为有效 rooted 函数对象；指针直拷标记位置
            unsafe {
                let _ = AddPromiseReactions(
                    (&mut realm).raw_cx(),
                    raw_handle(promise.as_ptr()),
                    raw_handle(ful_obj.as_ptr()),
                    raw_handle(rej_obj.as_ptr()),
                );
            }
        }
        // SAFETY: realm 内排空捕获反应，on_rejected_native 记录 reason
        unsafe { RunJobs((&mut realm).raw_cx()) };
        let reasons = state::with_plain(|p| std::mem::take(&mut p.rejection_reasons));
        if !reasons.is_empty() {
            return Err(Error::Other(format!(
                "unhandled rejection: {}",
                reasons.join("; ")
            )));
        }
    }
    Ok(())
}

/// `WINTERJS_HANG_EXIT` 到点：打印存活句柄计数，发 process 'exit'（mustCall 核对）。
fn hang_exit(rt: &mut Runtime, global: &RootedGuard<'_, *mut JSObject>, lim: std::time::Duration) {
    eprintln!(
        "winterjs: event loop made no progress for {}s (WINTERJS_HANG_EXIT) — open: timers={} fetch={} \
         ws={} stream={} watch={} child={} net={} worker={} quic={} napi={} dispatch={}",
        lim.as_secs(),
        timers::next_deadline().is_some() as u8,
        state::fetch_pending(),
        state::ws_open(),
        state::stream_pending(),
        state::watch_open(),
        state::child_open(),
        state::net_open(),
        state::worker_open(),
        state::quic_open(),
        state::napi_pending(),
        crate::dispatch::pending(),
    );
    let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
    crate::builtins::node::process_::emit_exit(&mut realm, global.get());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_sentinel_parse() {
        assert_eq!(exit_code_from_message("__wjs_exit:3"), Some(3));
        assert_eq!(exit_code_from_message("__wjs_exit: 0"), Some(0));
        assert_eq!(exit_code_from_message("unhandled rejection: __wjs_exit:2"), Some(2));
        assert_eq!(exit_code_from_message("__wjs_exit:abc"), None);
        assert_eq!(exit_code_from_message("plain-boom"), None);
        assert_eq!(exit_code_from_message(""), None);
        // 位置前缀不误判（收割串自带位置时由旗兜底，此处只认裸哨兵）。
        assert_eq!(exit_code_from_message("a.mjs:1:1: __wjs_exit:3"), None);
    }

    #[test]
    fn exit_sentinel_map() {
        let err = Error::Other("__wjs_exit:9".into());
        assert!(matches!(map_exit_sentinel(err), Error::Exit(9)));
        let err = Error::Other("boom".into());
        assert!(matches!(map_exit_sentinel(err), Error::Other(_)));
    }
}
