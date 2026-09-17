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

use url::Url;

use crate::builtins;
use crate::builtins::timers;
use crate::modules;use crate::error::Error;
use crate::jsapi_glue::{exc_name, exc_name_is, get_prop_string, get_prop_u32, raw_handle, raw_handle_mut, value_to_string};
use crate::state;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    /// 文件脚本：求值完成值，事件循环跑完 timers/microtasks 后退出
    Script,
    /// eval：先按普通脚本求值；仅当因顶层 await 语法失败时，用 async IIFE 重包
    Eval,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum WrapKind {
    /// `return (code);` —— 单表达式/以 await 表达式为主的场景，完成值 = await 值；行偏移 2
    Return,
    /// 任意语句序列；完成值丢失（文档注明）；行偏移 1
    Plain,
}

fn eval_wrap(code: &str, kind: WrapKind) -> String {
    // 包装行占用户代码之前的行 → 报错行号统一减 line_adjust
    let mut s = String::from(match kind {
        WrapKind::Return => "(async()=>{\nreturn (\n",
        WrapKind::Plain => "(async()=>{\n",
    });
    s.push_str(code);
    s.push_str(match kind {
        WrapKind::Return => "\n);\n",
        WrapKind::Plain => "\n",
    });
    s.push_str("})().then(v => { globalThis.__wjs_value = v; }, e => { globalThis.__wjs_error = e; });");
    s
}

/// 文件入口是否走模块求值：`.ts/.tsx/.mts/.cts/.mjs` 强制；`.js/.jsx` 嗅探 ESM 语法。
/// 解析失败/未知后缀 → None（回落经典路径）。
fn sniff_module(filename: &str, source: &str) -> Option<Url> {
    let url = crate::loader::resolve::entry_url(std::path::Path::new(filename)).ok()?;
    if url.scheme() != "file" {
        return None;
    }
    let path = url.to_file_path().ok()?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    let forced = matches!(ext.as_deref(), Some("ts" | "mts" | "cts" | "tsx" | "mjs"));
    if !forced {
        // `.js` 跟最近 package.json type（`require()` 同口径）；无扩展名同理
        //（`type: module` 包的 extensionless bin，如 oxlint；无 type 即经典）。
        let check_type = matches!(ext.as_deref(), Some("js" | "jsx") | None);
        if !check_type {
            return None;
        }
        if crate::builtins::node::require::nearest_pkg_type(&path).as_deref() == Some("module") {
            tracing::info!(target: "winterjs::runtime", url = url.as_str(), "module detected (package.json type)");
            return Some(url);
        }
        if ext.is_none() {
            return None;
        }
        let loaded = crate::loader::load_js(source, filename, &path).ok()?;
        if !loaded.is_module {
            return None;
        }
    }
    tracing::info!(target: "winterjs::runtime", url = url.as_str(), "module detected");
    Some(url)
}

/// 模块入口全流程：compile → load deps → link → evaluate → 事件循环 → 完成值打印。
async fn run_module(
    rt: &mut Runtime,
    global: &RootedGuard<'_, *mut JSObject>,
    url: &Url,
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
    use mozjs::rust::wrappers2::{ModuleEvaluate, ModuleLink};

    tracing::info!(target: "winterjs::runtime", url = url.as_str(), "module run start");
    rooted!(&in(rt.cx()) let mut rval = UndefinedValue());
    {
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        rooted!(&in(&mut realm) let mut entry: *mut JSObject = std::ptr::null_mut());
        let record = modules::compile_entry(&mut realm, url)?;
        entry.set(record);
        modules::load_dependencies(&mut realm, entry.get())?;
        // SAFETY: entry 为有效 rooted 记录；realm 内同步 link/evaluate
        if !unsafe { ModuleLink(&mut realm, entry.handle()) } {
            return Err(modules::module_error(&mut realm, url.as_str()));
        }
        if !unsafe { ModuleEvaluate(&mut realm, entry.handle(), rval.handle_mut()) } {
            return Err(modules::module_error(&mut realm, url.as_str()));
        }
        // 求值标记（`require(node:)` 复用时跳过二次求值）。
        state::set_module_evaluated(url.as_str().to_owned());
        // 入口 promise 先挂专用捕获（事件循环前挂载，否则收尾会被当未处理 rejection 误报）。
        if rval.is_object() {
            let obj = rval.to_object();
            rooted!(&in(&mut realm) let obj_root: *mut JSObject = obj);
            // SAFETY: obj_root 为有效 rooted 对象
            if unsafe { mozjs::jsapi::IsPromiseObject(raw_handle(obj_root.as_ptr())) } {
                let (fulfilled, rejected) = state::entry_native_values();
                rooted!(&in(&mut realm) let promise = obj_root.get());
                rooted!(&in(&mut realm) let ful_obj: *mut JSObject = fulfilled.to_object());
                rooted!(&in(&mut realm) let rej_obj: *mut JSObject = rejected.to_object());
                // SAFETY: promise/回调均为有效 rooted 函数对象；指针直拷标记位置
                unsafe {
                    let _ = AddPromiseReactions(
                        (&mut realm).raw_cx(),
                        raw_handle(promise.as_ptr()),
                        raw_handle(ful_obj.as_ptr()),
                        raw_handle(rej_obj.as_ptr()),
                    );
                }
                tracing::debug!(target: "winterjs::runtime", "entry promise capture attached");
            }
        }
    }

    event_loop(rt, global, ErrorSource::Module { url: url.as_str() }, fetch_rx, ws_rx, watch_rx, child_rx, net_rx, worker_rx, quic_rx, napi_rx, dispatch_rx).await?;

    // 收割入口决议（事件循环的排空已驱动捕获回调）。
    let (fulfillment, rejection) =
        state::with_plain(|p| (p.entry_fulfillment.take(), p.entry_rejection.take()));
    if let Some(reason) = rejection {
        // 入口决议串自带位置（`file:line:col: message`）或为值串，直接上报
        return Err(Error::Other(reason));
    }
    if let Some(s) = fulfillment {
        // 脚本语义对齐：决议 undefined 不打印
        if s != "undefined" {
            println!("{s}");
        }
        return Ok(());
    }
    print_completion(rt, global, rval.get())
}

/// `process.exit` 哨兵错识别（native 报 `__wjs_exit:<code>`，见 `node/process_.rs`）。
/// 覆盖裸消息与 `unhandled rejection: …` 单因包装。
pub fn exit_code_from_message(msg: &str) -> Option<i32> {
    if let Some(code) = msg.strip_prefix("__wjs_exit:") {
        return code.trim().parse().ok();
    }
    if let Some(reason) = msg.strip_prefix("unhandled rejection: ") {
        return exit_code_from_message(reason.trim());
    }
    None
}

/// 哨兵优先：错误消息带哨兵即转静默退出（`process.exit` 被 catch 也由旗兜底，见 `run`）。
fn map_exit_sentinel(err: Error) -> Error {
    let message = match &err {
        Error::Script { message, .. } => Some(message.clone()),
        Error::Other(message) => Some(message.clone()),
        _ => None,
    };
    match message.and_then(|m| exit_code_from_message(&m)) {
        Some(code) => Error::Exit(code),
        None => err,
    }
}

/// 求值 `source`（名 `filename`）并打印完成值；事件循环排空 timers/microtasks。
/// `extra_args` 进 `process.argv`（Script：`[exec, filename, ...]`；Eval：`[exec, ...]`）。
pub async fn run(source: &str, filename: &str, mode: Mode, extra_args: &[String]) -> Result<(), Error> {
    let r = run_inner(source, filename, mode, extra_args).await;
    // exit 优先于一切错误（哨兵被用户 catch 也照退，靠 `process_exited` 旗）。
    if let Some(code) = state::with_plain(|p| p.process_exited) {
        return Err(Error::Exit(code));
    }
    match r {
        Err(e) => Err(map_exit_sentinel(e)),
        Ok(()) => match state::exit_code() {
            // exitCode 非零即静默退出（Node 语义；0 照常 Ok）。
            Some(code) if code != 0 => Err(Error::Exit(code)),
            _ => Ok(()),
        },
    }
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

/// worker 线程规格（`node:worker_threads` spawn 用；move 进独立线程）。
pub struct WorkerThreadSpec {
    pub worker_id: u64,
    pub file: Option<String>,
    pub code: Option<String>,
    pub argv: Vec<String>,
    pub boot: crate::builtins::node::worker::WorkerBoot,
}

impl WorkerThreadSpec {
    pub fn new(
        worker_id: u64,
        src: String,
        is_eval: bool,
        boot: crate::builtins::node::worker::WorkerBoot,
    ) -> Self {
        let exe = std::env::current_exe()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "winterjs".into());
        if is_eval {
            WorkerThreadSpec { worker_id, file: None, code: Some(src), argv: vec![exe], boot }
        } else {
            let argv = vec![exe, src.clone()];
            WorkerThreadSpec { worker_id, file: Some(src), code: None, argv, boot }
        }
    }
}

thread_local! {
    /// worker 线程 boot 槽（`run_worker_thread` 置入，`init_session` 取出；
    /// 主会话恒 None。线程局部的跨函数传参，不经 JS 线程外）。
    static WORKER_BOOT: std::cell::RefCell<Option<crate::builtins::node::worker::WorkerBoot>> =
        std::cell::RefCell::new(None);
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
        // sqlite worker 端点全部摘除，线程在 channel 断开后自退。
        state::sqlite_reset();
        builtins::define_all(&mut realm, global.get())?;

        // SAFETY: realm 内；追踪器仅在 JS 线程被引擎回调
        unsafe {
            SetPromiseRejectionTrackerCallback(
                (&mut realm).raw_cx(),
                Some(rejection_tracker),
                ptr::null_mut(),
            );
        }

        let prelude_filename = CString::new("__wjs_prelude.js").expect("no NUL");
        let prelude_options = CompileOptionsWrapper::new(&realm, prelude_filename, 1);
        rooted!(&in(&mut realm) let mut prelude_rval = UndefinedValue());
        // prelude 是项目自带常量脚本，语法必然正确
        let ok = evaluate_script(
            &mut realm,
            global.handle(),
            builtins::PRELUDE,
            prelude_rval.handle_mut(),
            prelude_options,
        );
        if ok.is_err() {
            return Err(pending_error_in_realm(
                &mut realm,
                builtins::PRELUDE,
                "__wjs_prelude.js",
                0,
            ));
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
    // 线程身份默认主（worker 线程起后由 spawn 侧改写，见 state::worker_session_init）。
    // worker 线程带 boot 槽：取出落地（身份/workerData/parentPort/权限继承）。
    match WORKER_BOOT.with(|b| b.borrow_mut().take()) {
        Some(boot) => crate::builtins::node::worker::worker_boot_from_slot(boot),
        None => state::worker_session_init(true, 0),
    }
    // worker boot 收尾放 init 末（主会话无操作；worker 回传收件箱 + 发 Online）。
    crate::builtins::node::worker::worker_booted();

    Ok(SessionInit { rt, engine, global_ptr, state_guard, fetch_rx, ws_rx, watch_rx, child_rx, net_rx, worker_rx, quic_rx, napi_rx, dispatch_rx })
}

async fn run_inner(
    source: &str,
    filename: &str,
    mode: Mode,
    extra_args: &[String],
) -> Result<(), Error> {
    tracing::info!(target: "winterjs::runtime", filename, source_len = source.len(), ?mode, "run start");
    // process.argv（prelude 求值前就绪；execPath 失败回退名）。
    // argv[0] 取真实 OS 值（spawn arg0 自举回显，spawn-argv0 套件点名），
    // 余下按模式拼（execPath 缺失才回退）。
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "winterjs".into());
    let argv0 = std::env::args().next().unwrap_or_else(|| exe.clone());
    let argv: Vec<String> = match mode {
        Mode::Script => std::iter::once(argv0)
            .chain(std::iter::once(filename.to_owned()))
            .chain(extra_args.iter().cloned())
            .collect(),
        Mode::Eval => std::iter::once(argv0).chain(extra_args.iter().cloned()).collect(),
    };
    let init = init_session(argv)?;
    // 声明顺序即 drop 逆序：`rt` 若先于 `engine` drop（`?` 早退路径），Runtime
    // 的 Drop 会走引擎析构（StoreBuffer 悬垂，§4.8 SEGV）。两值在各返回点都经
    // `end_session` 泄漏；handle 本体泄漏无害（进程级单例，见 `engine_handle`）。
    let engine = init.engine;
    let mut rt = init.rt;
    let _state_guard = init.state_guard;
    // SAFETY: `init_session` 返回到此重 root 之间无任何 JSAPI 调用（无 GC 间隙），
    // 裸指针回 rooted guard 安全；此后 `global` 与 `rt` 同作用域存活。
    rooted!(&in(rt.cx()) let global = init.global_ptr);
    let mut fetch_rx = init.fetch_rx;
    let mut ws_rx = init.ws_rx;
    let mut watch_rx = init.watch_rx;
    let mut child_rx = init.child_rx;
    let mut net_rx = init.net_rx;
    let mut worker_rx = init.worker_rx;
    let mut quic_rx = init.quic_rx;
    let mut napi_rx = init.napi_rx;
    let mut dispatch_rx = init.dispatch_rx;

    rooted!(&in(rt.cx()) let mut rval = UndefinedValue());

    // 模块嗅探（仅 Script；Eval 保持经典语义，import 即 SyntaxError）。
    // 解析失败 → 回落经典（经典求值会给出它自己的报错）。
    if mode == Mode::Script {
        if let Some(url) = sniff_module(filename, source) {
            let r = run_module(&mut rt, &global, &url, &mut fetch_rx, &mut ws_rx, &mut watch_rx, &mut child_rx, &mut net_rx, &mut worker_rx, &mut quic_rx, &mut napi_rx, &mut dispatch_rx).await;
            // §4.8：跳过引擎/运行时析构
            end_session(rt, engine);
            return r;
        }
    }

    // 用户脚本求值（`.cjs` 经 require 主模块起，不打印 exports；见 node/require.rs）。
    // 10f：typeless `.js`/`.jsx` 同理（9j `cjs_interop` 口径复用——入口经典求值
    // 无 file base，相对 require/`__filename` 全挂，套件点名；TLA/ESM 已在
    // sniff 分流，此处 `is_module=false` 只收纯经典脚本）。
    let is_cjs_entry = mode == Mode::Script && {
        let p = std::path::Path::new(filename);
        if p.extension().is_some_and(|e| e == "cjs" || e == "cts") {
            true
        } else {
            crate::loader::resolve::entry_url(p)
                .ok()
                .is_some_and(|u| crate::modules::cjs_interop(&u, false, source))
        }
    };
    if is_cjs_entry {
        let url = crate::loader::resolve::entry_url(std::path::Path::new(filename));
        match url {
            Ok(url) => {
                state::set_main_module(url.as_str().to_owned());
                let main_src = format!(
                    "__wjs_require_main({})",
                    serde_json::to_string(url.as_str()).unwrap_or_else(|_| "\"\"".into())
                );
                let c_filename = CString::new(filename).unwrap_or_else(|_| c"main.cjs".into());
                let options = CompileOptionsWrapper::new(rt.cx(), c_filename, 1);
                let res = evaluate_script(rt.cx(), global.handle(), &main_src, rval.handle_mut(), options);
                if res.is_err() {
                    let err = {
                        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
                        rooted!(&in(&mut realm) let mut exc = UndefinedValue());
                        // SAFETY: realm 内读取 pending exception（消费异常值）
                        match error_info_from_exception_stack(&mut realm, exc.handle_mut()) {
                            Some(info) => {
                                // 10f：worker 内非对象异常（throw 42 等）走原始值信封
                                //（error-primitive 套件断同一性；主进程显示不受影响）。
                                let prim = if !state::worker_is_main() {
                                    crate::jsapi_glue::exc_prim_marker(&mut realm, exc.get())
                                } else {
                                    None
                                };
                                Error::script_with_kind(
                                    filename, &main_src, info.line.max(1), info.col,
                                    prim.unwrap_or(info.message),
                                    crate::jsapi_glue::exc_name(&mut realm, exc.get()))
                            }
                            None => Error::Other("uncaught JS exception (no stack info)".into()),
                        }
                    };
                    end_session(rt, engine);
                    return Err(err);
                }
                event_loop(&mut rt, &global, ErrorSource::Script { source: &main_src, filename }, &mut fetch_rx, &mut ws_rx, &mut watch_rx, &mut child_rx, &mut net_rx, &mut worker_rx, &mut quic_rx, &mut napi_rx, &mut dispatch_rx).await?;
                end_session(rt, engine);
                return Ok(());
            }
            Err(e) => {
                end_session(rt, engine);
                return Err(e);
            }
        }
    }
    {
        let c_filename = CString::new(filename).unwrap_or_else(|_| c"script.js".into());
        let options = CompileOptionsWrapper::new(rt.cx(), c_filename, 1);
        // evaluate_script 内部自进 realm；rval 为 rooted 出参，跨事件循环存活
        let res = evaluate_script(rt.cx(), global.handle(), source, rval.handle_mut(), options);
        if res.is_err() {
            if mode == Mode::Eval {
                let r = eval_syntax_fallback(&mut rt, &global, source, filename, &mut fetch_rx, &mut ws_rx, &mut watch_rx, &mut child_rx, &mut net_rx, &mut worker_rx, &mut quic_rx, &mut napi_rx, &mut dispatch_rx).await;
                // §4.8：跳过引擎/运行时析构（StoreBuffer 悬垂边在 destroyRuntime 的小 GC 里 SEGV）
                end_session(rt, engine);
                return r;
            }
            // 模块重试：经典 SyntaxError 且能按模块解析 → 改走模块求值。
            // （`await` 在参数位置按标识符解析，报的不是 await 错而是 missing-paren，
            // 故不能只认 await 文案；真语法错误则保留原始经典报错。见 §4.17。）
            let (info_opt, is_syntax, kind, prim) = {
                let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
                rooted!(&in(&mut realm) let mut exc = UndefinedValue());
                // SAFETY: realm 内读取 pending exception（消费异常值）
                let info = error_info_from_exception_stack(&mut realm, exc.handle_mut());
                let kind = exc_name(&mut realm, exc.get());
                let is_syntax = kind.as_deref() == Some("SyntaxError");
                // 10f：worker 内非对象异常走原始值信封（同模块路径）。
                let prim = if !state::worker_is_main() {
                    crate::jsapi_glue::exc_prim_marker(&mut realm, exc.get())
                } else {
                    None
                };
                (info, is_syntax, kind, prim)
            };
            if is_syntax
                && let Ok(url) = crate::loader::resolve::entry_url(std::path::Path::new(filename))
                && let Ok(path) = url.to_file_path()
                && crate::loader::load_js(source, filename, &path).is_ok()
            {
                tracing::info!(target: "winterjs::runtime", url = url.as_str(), "retrying as module");
                let r = run_module(&mut rt, &global, &url, &mut fetch_rx, &mut ws_rx, &mut watch_rx, &mut child_rx, &mut net_rx, &mut worker_rx, &mut quic_rx, &mut napi_rx, &mut dispatch_rx).await;
                end_session(rt, engine);
                return r;
            }
            let err = match info_opt {
                Some(info) => Error::script_with_kind(
                    filename, source, info.line.max(1), info.col,
                    prim.unwrap_or(info.message), kind),
                None => Error::Other("uncaught JS exception (no stack info)".into()),
            };
            end_session(rt, engine);
            return Err(err);
        }
    }

    // 未包装成功的场景（含全部 Script 与无顶层 await 的 Eval）：
    // 完成值就是 rval（老行为）；仅 async IIFE 包装路径才读 __wjs_value。
    event_loop(&mut rt, &global, ErrorSource::Script { source, filename }, &mut fetch_rx, &mut ws_rx, &mut watch_rx, &mut child_rx, &mut net_rx, &mut worker_rx, &mut quic_rx, &mut napi_rx, &mut dispatch_rx).await?;
    // 自然退出：派发 process 'exit'（common.mustCall 计数结算点；Node 口径）。
    // 显式 process.exit 已在 JS 侧派发过（process_exited 旗），此处跳过防双发。
    if state::with_plain(|p| p.process_exited.is_none()) {
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        crate::builtins::node::process_::emit_exit(&mut realm, global.get());
    }
    let r = print_completion(&mut rt, &global, rval.get());
    // §4.8：跳过引擎/运行时析构（带 timer 的路径在 JS_DestroyContext 里 SEGV）。
    // CLI 进程即将退出，内存由 OS 回收；见 AGENTS §4.8。
    end_session(rt, engine);
    r
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

/// 独立线程跑一段脚本（test runner 用，§4.24）：JS 线程私有的 CONTEXT TLS /
/// state TLS 全随线程生灭，Runtime 照 §4.8 泄漏。同步接口：调用方阻塞等结果
/// （与 sqlite worker 同哲学）。栈给 16MB（引擎 STACK_QUOTA 按主线程量级假设，
/// 线程默认栈远不够）。
pub fn run_isolated(source: String, filename: String, extra_args: Vec<String>) -> Result<(), Error> {
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("winterjs-test-file".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            let tokio_rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = tx.send(Err(Error::Other(format!("cannot start async runtime: {e}"))));
                    return;
                }
            };
            let outcome = tokio::task::LocalSet::new().block_on(&tokio_rt, async {
                run(&source, &filename, Mode::Script, &extra_args).await
            });
            // Runtime 已在 end_session 泄漏；线程退出即清 CONTEXT TLS（§4.24）。
            let _ = tx.send(outcome);
        });
    spawned.map_err(|e| Error::Other(format!("cannot spawn test thread: {e}")))?;
    rx.recv().unwrap_or_else(|_| Err(Error::Other("test file thread died".into())))
}

/// worker 线程入口（`node:worker_threads` spawn 用，§4.24 哲学：每 worker 独立
/// OS 线程 + 完整会话；Runtime 照 §4.8 泄漏，线程退出即清 TLS）。
/// 退出码经 `WExit` 事件回主会话（成功 0/未捕获错 1/终止 1/`process.exit(n)`→n），
/// 错误文案经 `WError` 先行（随后必跟 `WExit{1}`）。detached 线程，主侧不等。
pub fn run_worker_thread(spec: WorkerThreadSpec) {
    use crate::builtins::node::worker::WorkerEvent;
    let spawned = std::thread::Builder::new()
        .name(format!("winterjs-worker-{}", spec.worker_id))
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            let wid = spec.worker_id;
            // 主收件箱先取一份（会话前失败路径用；会话起后走 state）。
            let early_inbox = spec.boot.main_inbox.clone();
            let early_rendezvous = spec.boot.rendezvous.clone();
            // 早失败（文件读错/runtime 起不来）：WError/WExit 先排队，再发 rendezvous
            //（parked 收件箱——后续投递静默失败），保证主侧不超时、事件不丢
            //（attach 在 spawn 返回后同步发生，早于任何分发）。
            let fail_early = |message: String| {
                let _ = early_inbox.send(WorkerEvent::WError { worker_id: wid, message });
                let _ = early_inbox.send(WorkerEvent::WExit { worker_id: wid, code: 1 });
                let (park_tx, park_rx) = tokio::sync::mpsc::unbounded_channel();
                drop(park_rx); // 接收端已死：后续投递即失败，不堆积
                let _ = early_rendezvous.send((park_tx, 0));
            };
            // 源码决议：文件直读；eval 串嗅探 ESM→落临时 .mjs（复用文件管线），
            // 否则经典求值（文件名 `worker-eval-<id>.js`）。
            let (source, filename, tmp): (String, String, Option<std::path::PathBuf>) =
                match (spec.file, spec.code) {
                    (Some(path), _) => {
                        let path = std::path::PathBuf::from(&path);
                        let abs = if path.is_absolute() {
                            path
                        } else {
                            std::env::current_dir().unwrap_or_default().join(&path)
                        };
                        match std::fs::read_to_string(&abs) {
                            Ok(s) => (s, abs.to_string_lossy().into_owned(), None),
                            Err(_) => {
                                // 10f 对拍：node 缺主模块 error 事件文案
                                // /Cannot find module '<path>'/（esm-missing-main 套件）。
                                fail_early(format!("Cannot find module '{}'", abs.display()));
                                return;
                            }
                        }
                    }
                    (None, Some(code)) => {
                        let is_module = std::env::current_dir()
                            .ok()
                            .and_then(|cwd| crate::loader::load_js(&code, "worker-eval.mjs", &cwd).ok())
                            .is_some_and(|l| l.is_module);
                        if is_module {
                            let tmp = std::env::temp_dir().join(format!(
                                "winterjs-worker-{}-{}.mjs",
                                std::process::id(),
                                wid
                            ));
                            match std::fs::write(&tmp, &code) {
                                Ok(()) => (code, tmp.to_string_lossy().into_owned(), Some(tmp)),
                                Err(e) => {
                                    fail_early(format!("Worker: cannot stage eval source ({e})"));
                                    return;
                                }
                            }
                        } else {
                            (code, format!("worker-eval-{wid}.js"), None)
                        }
                    }
                    (None, None) => {
                        fail_early("Worker: no filename or eval source".into());
                        return;
                    }
                };
            // boot 入槽（init_session 取出落地）；argv 照 file/eval 形态。
            WORKER_BOOT.with(|b| *b.borrow_mut() = Some(spec.boot));
            let tokio_rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    fail_early(format!("Worker: cannot start async runtime ({e})"));
                    return;
                }
            };
            let outcome = tokio::task::LocalSet::new().block_on(&tokio_rt, async {
                run(&source, &filename, Mode::Script, &spec.argv).await
            });
            // 退出码映射 + 临时文件清理（best effort）。
            if let Some(tmp) = tmp {
                let _ = std::fs::remove_file(&tmp);
            }
            let code = match &outcome {
                Err(Error::Exit(c)) => *c,
                Err(e) => {
                    let message = worker_error_text(e);
                    let inbox = state::with_plain(|p| p.worker_main_inbox.clone());
                    if let Some(tx) = inbox {
                        let _ = tx.send(WorkerEvent::WError { worker_id: wid, message });
                    }
                    1
                }
                Ok(()) => {
                    if state::worker_terminated() { 1 } else { 0 }
                }
            };
            let inbox = state::with_plain(|p| p.worker_main_inbox.clone());
            if let Some(tx) = inbox {
                let _ = tx.send(WorkerEvent::WExit { worker_id: wid, code });
            }
        });
    if let Err(e) = spawned {
        tracing::warn!(target: "winterjs::runtime", worker_id = spec.worker_id, "worker thread spawn failed: {e}");
    }
}

/// worker 未捕获错误转文案（`WError` 用）。
/// 10f 对拍：error 事件透传**原错误**——Script 消息不带 "Worker: " 前缀
/// （uncaught-exception 套件断 `String(err) === 'Error: foo'`）；kind 已知且非
/// Error 时导出 "Kind: " 前缀，worker.js 侧按类名还原错误类（SyntaxError 套件
/// 断 `err.constructor === SyntaxError`）。启动期失败（Other/`_`）维持
/// "Worker: " 前缀（esm-missing-main 套件口径）。
fn worker_error_text(e: &Error) -> String {
    match e {
        Error::Script { message, kind, .. } => match kind {
            Some(k) if k != "Error" => format!("{k}: {message}"),
            _ => message.clone(),
        },
        // 原始值信封直通（勿加前缀——JS 侧按 `__wjs_prim:` 还原）。
        Error::Other(message) if message.starts_with("__wjs_prim:") => message.clone(),
        Error::Other(message) => format!("Worker: {message}"),
        _ => format!("Worker: {e}"),
    }
}


/// eval 首次求值失败：若为 SyntaxError，用 async IIFE 重包一次。
/// （触发条件放宽到一切 SyntaxError：`await` 在参数位置报的不是 await 错，见 §4.17；
/// 包装也解不出的真语法错误回落原始报错。）
async fn eval_syntax_fallback(
    rt: &mut Runtime,
    global: &RootedGuard<'_, *mut JSObject>,
    source: &str,
    filename: &str,
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
    let original = {
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        rooted!(&in(&mut realm) let mut exc = UndefinedValue());
        // realm 内读取 pending exception（会消费异常值）
        let info = error_info_from_exception_stack(&mut realm, exc.handle_mut());
        let exc_kind = exc_name(&mut realm, exc.get());
        let is_syntax = exc_kind.as_deref() == Some("SyntaxError");
        if !is_syntax {
            // 非语法错误：直接用已捕获的信息报错（异常已被消费，勿再取）
            return match info {
                Some(info) => Err(Error::script_with_kind(
                    filename,
                    source,
                    info.line.saturating_sub(state::line_adjust()).max(1),
                    info.col,
                    info.message,
                    exc_kind,
                )),
                None => Err(Error::Other("uncaught JS exception (no stack info)".into())),
            };
        }
        // SAFETY: 首次失败发生在解析期（无副作用），清除后重跑包装版
        unsafe { JS_ClearPendingException((&mut realm).raw_cx()) };
        info.map(|info| {
            Error::script_with_kind(
                filename,
                source,
                info.line.saturating_sub(state::line_adjust()).max(1),
                info.col,
                info.message,
                exc_kind,
            )
        })
        .unwrap_or_else(|| Error::Other("uncaught JS exception (no stack info)".into()))
    };

    // 先试 return 包装（保住完成值），纯语句序列再退普通包装
    for (kind, adjust) in [(WrapKind::Return, 2u32), (WrapKind::Plain, 1u32)] {
        tracing::debug!(target: "winterjs::runtime", ?kind, adjust, "eval fallback trying wrap");
        state::set_line_adjust(adjust);
        let wrapped = eval_wrap(source, kind);
        let c_filename = CString::new(filename).unwrap_or_else(|_| c"eval.js".into());
        let options = CompileOptionsWrapper::new(rt.cx(), c_filename, 1);
        rooted!(&in(rt.cx()) let mut wrapped_rval = UndefinedValue());
        // 同 run()；包装版行号偏移经 line_adjust 校正
        let res = evaluate_script(rt.cx(), global.handle(), &wrapped, wrapped_rval.handle_mut(), options);
        if res.is_ok() {
            event_loop(rt, global, ErrorSource::Script { source, filename }, fetch_rx, ws_rx, watch_rx, child_rx, net_rx, worker_rx, quic_rx, napi_rx, dispatch_rx).await?;
            let r = extract_eval_result(rt, global, source, filename);
            // engine/rt 由外层 run() 统一 forget（见 §4.8）
            return r;
        }
        // 语法错误 → 换下一种包装；运行期错误 → 直接上报（勿重跑）
        let (info, is_syntax, exc_kind) = {
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            rooted!(&in(&mut realm) let mut exc = UndefinedValue());
            // realm 内读取 pending exception（消费异常值）
            let info = error_info_from_exception_stack(&mut realm, exc.handle_mut());
            let exc_kind = exc_name(&mut realm, exc.get());
            let is_syntax = exc_kind.as_deref() == Some("SyntaxError");
            if is_syntax {
                // SAFETY: 解析期失败无副作用，清除后重试
                unsafe { mozjs::jsapi::JS_ClearPendingException((&mut realm).raw_cx()) };
            }
            (info, is_syntax, exc_kind)
        };
        if !is_syntax {
            return match info {
                Some(info) => Err(Error::script_with_kind(
                    filename,
                    source,
                    info.line.saturating_sub(adjust).max(1),
                    info.col,
                    info.message,
                    exc_kind,
                )),
                None => Err(Error::Other("uncaught JS exception (no stack info)".into())),
            };
        }
    }
    // 包装也解不出：回落首次的原始报错（而非 exhausted，保住定位）。
    Err(original)
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
        let (fired, fired_unrefed) = timers::fire_due(&mut realm, global.get(), err)?;
        st.timers = fired;
        st.timers_unrefed = fired_unrefed;
    }
    Ok(st)
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
    loop {
        // 单轮推进与 `repl` 共用（§4.18 检查点顺序在内保持）。
        let st = pump_once(rt, global, err, fetch_rx, ws_rx, watch_rx, child_rx, net_rx, worker_rx, quic_rx, napi_rx, dispatch_rx).await?;
        if st.exited {
            return Ok(());
        }
        // 入口已失败即跳出（收割路径上报；未处理 rejection 收尾不受影响——
        // 入口带专用捕获，不进 `unhandled` 表，见 `run_module`）。
        if st.entry_failed {
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

        let timers_empty = timers::next_deadline().is_none();
        let idle = timers_empty
            && state::fetch_pending() == 0
            && state::ws_open() == 0
            && state::stream_pending() == 0
            && state::watch_open() == 0
            && state::child_open() == 0
            && state::net_open() == 0
            && state::worker_open() == 0
            && state::quic_open() == 0
            && state::napi_pending() == 0
            && crate::dispatch::pending() == 0;
        if idle && !progressed {
            if progressed_unrefed_only && !unrefed_grace {
                // §4.18 完整形态：unrefed 回调排的 microtask 也要一轮 RunJobs
                // ——给一轮宽限再退，不无限宽限（否则 unrefed interval 空转）。
                unrefed_grace = true;
                continue;
            }
            break;
        }
        // §4.18 推广：本轮结算/触发过就不能直接 park——结算可能只排了 microtask
        // （如 worker 端口 `__ev` 的 queueMicrotask），park 进 select 即再无 RunJobs
        // 机会（无 timer 时直接 hang，有 timer 则延迟到 sleep 醒才送达）。
        // 回顶下一轮 pump 先 RunJobs 排空；无新进展即 park，不忙转。
        if progressed {
            continue;
        }
        // park 唤醒目标用 next_wake（含 unrefed：到点须醒去触发，套件
        // unrefd-interval-still-fires）；存活/idle 判定上面已用 refed-only
        // 的 next_deadline 定案，走到这里说明循环确有存活理由。
        match timers::next_wake() {
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

/// REPL 单步结果（求值永不抛错：错误打印后继续；只有 `process.exit` 跳出）。
enum ReplStep {
    Done,
    Exited(i32),
}

/// REPL 单行求值：经典脚本求值 + 完成值打印（顶层 await 暂不支持，见内注）。
async fn repl_eval(
    rt: &mut Runtime,
    global: &RootedGuard<'_, *mut JSObject>,
    line: &str,
) -> ReplStep {
    let exited = || state::with_plain(|p| p.process_exited);
    let c_filename = CString::new("repl.js").expect("no NUL");
    rooted!(&in(rt.cx()) let mut rval = UndefinedValue());
    let options = CompileOptionsWrapper::new(rt.cx(), c_filename, 1);
    let res = evaluate_script(rt.cx(), global.handle(), line, rval.handle_mut(), options);
    if res.is_err() {
        // exit 优先于一切错误（哨兵被用户 catch 也照退，见 `run`）。
        if let Some(code) = exited() {
            return ReplStep::Exited(code);
        }
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        rooted!(&in(&mut realm) let mut exc = UndefinedValue());
        // SAFETY: realm 内读取 pending exception（消费异常值）
        match error_info_from_exception_stack(&mut realm, exc.handle_mut()) {
            Some(info) => {
                eprintln!("repl.js:{}:{}: {}", info.line.max(1), info.col, info.message);
                if exc_name_is(&mut realm, exc.get(), "SyntaxError") && line.contains("await") {
                    eprintln!("hint: top-level await is not supported in repl yet (wrap in an async function)");
                }
            }
            None => eprintln!("uncaught JS exception (no stack info)"),
        }
        return ReplStep::Done;
    }
    if let Some(code) = exited() {
        return ReplStep::Exited(code);
    }
    if let Err(e) = print_completion(rt, global, rval.get()) {
        if let Some(code) = exited() {
            return ReplStep::Exited(code);
        }
        eprintln!("{e}");
    }
    ReplStep::Done
}

/// 交互式 REPL（plan Phase 7-e3）：持久会话（`init_session`）+ 行编辑/历史/
 /// 高亮/括号续行（`repl` 模块，readline 独占线程经 channel 投递）+ 事件泵。
/// 输入等待用 5ms 短轮询（不 park）：timers/fetch 照常推进且永不饿死输入；
/// timer 回调抛错打印后继续；stdin 非 TTY 时退化逐行读（照跑，无 ANSI）。
pub async fn repl() -> Result<(), Error> {
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "winterjs".into());
    let init = init_session(vec![exe])?;
    // 声明顺序即 drop 逆序（`engine` 先，见 `run_inner` 处注释，§4.22）。
    let engine = init.engine;
    let mut rt = init.rt;
    let _state_guard = init.state_guard;
    // SAFETY: 返回到此重 root 之间无 JSAPI 调用（无 GC 间隙）。
    rooted!(&in(rt.cx()) let global = init.global_ptr);
    let mut fetch_rx = init.fetch_rx;
    let mut ws_rx = init.ws_rx;
    let mut watch_rx = init.watch_rx;
    let mut child_rx = init.child_rx;
    let mut net_rx = init.net_rx;
    let mut worker_rx = init.worker_rx;
    let mut quic_rx = init.quic_rx;
    let mut napi_rx = init.napi_rx;
    let mut dispatch_rx = init.dispatch_rx;

    let (line_tx, mut line_rx) = tokio::sync::mpsc::unbounded_channel::<Option<String>>();
    let hist = crate::repl::history_path();
    // readline 是阻塞 IO，独占线程跑（`Editor` 不出线程，无跨线程共享）。
    std::thread::spawn(move || crate::repl::readline_loop(line_tx, hist));
    println!("winterjs repl (type .exit to quit)");
    // stdout 管道时块缓冲：banner 立即刷出，否则与 stderr 行错序（实测）。
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    tracing::info!(target: "winterjs::runtime", "repl start");

    let err_src = ErrorSource::Script { source: "", filename: "repl.js" };
    loop {
        let st = match pump_once(
            &mut rt, &global, err_src, &mut fetch_rx, &mut ws_rx, &mut watch_rx, &mut child_rx, &mut net_rx, &mut worker_rx, &mut quic_rx, &mut napi_rx, &mut dispatch_rx,
        )
        .await
        {
            Ok(st) => st,
            Err(e) => {
                if let Some(code) = state::with_plain(|p| p.process_exited) {
                    end_session(rt, engine);
                    return Err(Error::Exit(code));
                }
                eprintln!("{e}");
                continue;
            }
        };
        if st.exited {
            let code = state::with_plain(|p| p.process_exited).unwrap_or(0);
            end_session(rt, engine);
            return Err(Error::Exit(code));
        }
        // 每轮收割 unhandled rejection（Node 式打印，继续不退出）。
        if let Err(e) = report_unhandled_rejections(&mut rt, &global) {
            eprintln!("{e}");
        }
        tokio::select! {
            line = line_rx.recv() => {
                match line {
                    // EOF（Ctrl-D）或 readline 线程结束。
                    None | Some(None) => break,
                    Some(Some(text)) => {
                        if text.trim().is_empty() {
                            continue;
                        }
                        match crate::repl::dot_command(&text) {
                            crate::repl::Dot::Exit => break,
                            crate::repl::Dot::Help => {
                                println!(".exit  quit the repl");
                                println!(".help  show this help");
                                continue;
                            }
                            crate::repl::Dot::Code => {}
                        }
                        match repl_eval(&mut rt, &global, &text).await {
                            ReplStep::Done => {}
                            ReplStep::Exited(code) => {
                                end_session(rt, engine);
                                return Err(Error::Exit(code));
                            }
                        }
                    }
                }
            }
            // 短轮询：只做唤醒，工作全在顶部的 pump（channel 无 peek，
            // select 直收会吞掉 fetch/ws 消息，见设计注记）。
            _ = tokio::time::sleep(std::time::Duration::from_millis(5)) => {}
        }
    }
    tracing::info!(target: "winterjs::runtime", "repl done");
    end_session(rt, engine);
    Ok(())
}

/// SAFETY: rejection 追踪器由引擎在 JS 线程回调。
unsafe extern "C" fn rejection_tracker(
    _cx: *mut mozjs::jsapi::JSContext,
    _muted_errors: bool,
    promise: mozjs::jsapi::Handle<*mut JSObject>,
    state_: PromiseRejectionHandlingState,
    _data: *mut std::ffi::c_void,
) {
    match state_ {
        PromiseRejectionHandlingState::Unhandled => {
            // `Heap::boxed` 定址（set 后禁移动，见 §4.40）。
            tracing::debug!(target: "winterjs::promise", promise = ?promise.get(), "rejection unhandled");
            state::with_rooted(|s| s.unhandled.push(mozjs::jsapi::Heap::boxed(promise.get())));
        }
        PromiseRejectionHandlingState::Handled => {
            tracing::trace!(target: "winterjs::promise", promise = ?promise.get(), "rejection handled");
            state::with_rooted(|s| s.unhandled.retain(|h| h.get() != promise.get()));
        }
    }
}

/// 在已持有的 realm 里把 pending exception 转成 Error（返回前异常被消费）。
fn pending_error_in_realm(
    realm: &mut AutoRealm<'_>,
    source: &str,
    filename: &str,
    line_adjust: u32,
) -> Error {
    rooted!(&in(realm) let mut exc = UndefinedValue());
    // realm 内读取 pending exception
    match error_info_from_exception_stack(realm, exc.handle_mut()) {
        Some(info) => Error::script(
            filename,
            source,
            info.line.saturating_sub(line_adjust).max(1),
            info.col,
            info.message,
        ),
        None => Error::Other("uncaught JS exception (no stack info)".into()),
    }
}

/// eval 结果：__wjs_error 优先（格式与未捕获异常一致），否则打印 __wjs_value。
fn extract_eval_result(
    rt: &mut Runtime,
    global: &RootedGuard<'_, *mut JSObject>,
    source: &str,
    filename: &str,
) -> Result<(), Error> {
    let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
    rooted!(&in(&mut realm) let mut err = UndefinedValue());
    // SAFETY: realm 内读全局属性；raw 调用不触发 GC
    let ok = unsafe {
        JS_GetProperty((&mut realm).raw_cx(), raw_handle(global.as_ptr()), c"__wjs_error".as_ptr(), raw_handle_mut(err.as_ptr()))
    };
    if ok && !err.is_undefined() {
        // Error 对象读 message/lineNumber/columnNumber；非对象值退化为 ToString
        let (message, line, col) = if err.is_object() {
            let obj = err.to_object();
            rooted!(&in(&mut realm) let obj_root: *mut JSObject = obj);
            let message = get_prop_string(&mut realm, obj_root.get(), c"message")
                .unwrap_or_default();
            let line = get_prop_u32(&mut realm, obj_root.get(), c"lineNumber")
                .map(|l| l.saturating_sub(state::line_adjust()))
                .unwrap_or(1);
            let col = get_prop_u32(&mut realm, obj_root.get(), c"columnNumber").unwrap_or(1);
            (message, line, col)
        } else {
            (value_to_string(&mut realm, err.get()), 1, 1)
        };
        let message = if message.is_empty() {
            value_to_string(&mut realm, err.get())
        } else {
            message
        };
        return Err(Error::script(filename, source, line.max(1), col, message));
    }

    rooted!(&in(&mut realm) let mut val = UndefinedValue());
    // SAFETY: realm 内读全局属性；raw 调用不触发 GC
    let ok = unsafe {
        JS_GetProperty((&mut realm).raw_cx(), raw_handle(global.as_ptr()), c"__wjs_value".as_ptr(), raw_handle_mut(val.as_ptr()))
    };
    if ok && !val.is_undefined() {
        println!("{}", value_to_string(&mut realm, val.get()));
    }
    Ok(())
}

/// Script 模式完成值打印（与既有行为一致：undefined 不打印）。
fn print_completion(
    rt: &mut Runtime,
    global: &RootedGuard<'_, *mut JSObject>,
    rval: mozjs::jsval::JSVal,
) -> Result<(), Error> {
    if rval.is_undefined() {
        return Ok(());
    }
    let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
    rooted!(&in(&mut realm) let rv = rval);
    match String::from_jsval(&mut realm, rv.handle(), ()) {
        Ok(ConversionResult::Success(s)) => println!("{s}"),
        _ => println!("<non-stringifiable result>"),
    }
    // 同上：REPL 管道下完成值行立即刷出（`run`/`eval` 单次进程无感，顺手）。
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    Ok(())
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
