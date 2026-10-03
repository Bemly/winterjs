//! runtime/runner：单次 run（脚本/模块入口 + 事件循环 + 收割）。

use super::*;
use mozjs::jsapi::JSObject;
use mozjs::jsval::UndefinedValue;
use mozjs::realm::AutoRealm;
use mozjs::rooted;
use mozjs::rust::RootedGuard;
use mozjs::rust::Runtime;
use url::Url;
use std::ffi::CString;
use crate::error::Error;

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

    tracing::info!(target: "winterjs2::runtime", url = url.as_str(), "module run start");
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
                tracing::debug!(target: "winterjs2::runtime", "entry promise capture attached");
            }
        }
    }

    event_loop(rt, global, ErrorSource::Module { url: url.as_str() }, fetch_rx, ws_rx, watch_rx, child_rx, net_rx, worker_rx, quic_rx, napi_rx, dispatch_rx).await?;

    // 自然退出：派发 process 'exit'（common.mustCall 计数结算点；Node 口径）。
    // 显式 process.exit 已在 JS 侧派发过（process_exited 旗），此处跳过防双发。
    // Entry 路径此前漏派发（--run 文件永不触发 exit 监听，见 §4.188）。
    if state::with_plain(|p| p.process_exited.is_none()) {
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        crate::builtins::node::process_::emit_exit(&mut realm, global.get());
    }

    // 收割入口决议（事件循环的排空已驱动捕获回调）。
    let (fulfillment, rejection) =
        state::with_plain(|p| (p.entry_fulfillment.take(), p.entry_rejection.take()));
    if let Some(reason) = rejection {
        // 入口决议串自带位置（`file:line:col: message`）→ node 形 Script（D4）；
        // 值串（无位置）照旧一行上报。代码框源码按入口文件原文读（仅显示用）。
        let src = url.to_file_path().ok().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        return Err(crate::error::from_entry_reason(&reason, url.as_str(), &src)
            .unwrap_or(Error::Other(reason)));
    }
    if let Some(s) = fulfillment {
        // 脚本语义对齐：决议 undefined 不打印
        if s != "undefined" {
            println!("{s}");
        }
        return Ok(());
    }
    print_completion(rt, global, rval.get(), false)
}

/// 致命错收尾（node `triggerUncaughtException` 尾段）：先打印错误，再以 exitCode=1 派发
/// process 'exit'（mustCall 核对照跑；监听可改 exitCode / 再 exit），返回静默 `Exit`。
/// 仅 CLI 主进程主线程生效（登记了渲染配色）；worker/testrun 原错透传自理。
fn fatal_exit(rt: &mut Runtime, global: &RootedGuard<'_, *mut JSObject>, e: Error) -> Error {
    let e = map_exit_sentinel(e);
    if matches!(e, Error::Exit(_)) || !state::worker_is_main() {
        return e;
    }
    let Some(color) = crate::error::render_color() else { return e };
    if state::with_plain(|p| p.process_exited.is_some()) {
        return e;
    }
    let _ = e.render(color);
    state::set_exit_code(Some(1));
    {
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        crate::builtins::node::process_::emit_exit(&mut realm, global.get());
    }
    Error::Exit(state::with_plain(|p| p.process_exited).or(state::exit_code()).unwrap_or(1))
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

async fn run_inner(
    source: &str,
    filename: &str,
    mode: Mode,
    extra_args: &[String],
) -> Result<(), Error> {
    tracing::info!(target: "winterjs2::runtime", filename, source_len = source.len(), ?mode, "run start");
    // process.argv（prelude 求值前就绪；execPath 失败回退名）。
    // argv[0] 取真实 OS 值（spawn arg0 自举回显，spawn-argv0 套件点名），
    // 余下按模式拼（execPath 缺失才回退）。
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "winterjs2".into());
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
            let r = run_module(&mut rt, &global, &url, &mut fetch_rx, &mut ws_rx, &mut watch_rx, &mut child_rx, &mut net_rx, &mut worker_rx, &mut quic_rx, &mut napi_rx, &mut dispatch_rx).await
                .map_err(|e| fatal_exit(&mut rt, &global, e));
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
                    "__wjs2_require_main({})",
                    serde_json::to_string(url.as_str()).unwrap_or_else(|_| "\"\"".into())
                );
                // 引导脚本用 `__wjs2_` 名：栈里这一帧属宿主管线，node 形渲染按前缀滤掉（D4）。
                let options = CompileOptionsWrapper::new(rt.cx(), c"__wjs2_main_bootstrap.js".into(), 1);
                let res = evaluate_script(rt.cx(), global.handle(), &main_src, rval.handle_mut(), options);
                if res.is_err() {
                    // P2-process R7：入口抛错先走 uncaught 分发（capture→监听），
                    // 接住即转事件循环（脚本中止但进程续活）；无人接才走原 fatal。
                    let entry_handled = {
                        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
                        crate::builtins::node::process_::dispatch_entry_throw(
                            &mut realm,
                            global.get(),
                        )
                    };
                    if entry_handled {
                        // 转下方事件循环。
                    } else {
                    let err = {
                        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
                        rooted!(&in(&mut realm) let mut exc = UndefinedValue());
                        // SAFETY: realm 内读取 pending exception（消费异常值）
                        match { let i = error_info_from_exception_stack(&mut realm, exc.handle_mut()); crate::jsapi_glue::fill_message(&mut realm, i, exc.get()) } {
                            Some(info) => {
                                // 10f：worker 内非对象异常（throw 42 等）走原始值信封
                                //（error-primitive 套件断同一性；主进程显示不受影响）。
                                let prim = if !state::worker_is_main() {
                                    crate::jsapi_glue::exc_prim_marker(&mut realm, exc.get())
                                } else {
                                    None
                                };
                                // 2026-09-25：require 透传原异常后，位置即真实抛点——
                                // 抛点在入口文件用入口名 + 源码（代码框对得上），
                                // 否则（node:internal/… 等）如实报其文件名、不给代码框。
                                let in_entry = info.filename.is_empty() || state::debug_key(&info.filename) == url.as_str();
                                let (shown, src): (&str, &str) = if in_entry {
                                    (filename, source)
                                } else {
                                    (info.filename.as_str(), "")
                                };
                                Error::script_with_kind(
                                    shown, src, info.line.max(1), info.col,
                                    prim.unwrap_or(info.message),
                                    crate::jsapi_glue::exc_name(&mut realm, exc.get()))
                            }
                            None => Error::Other("uncaught JS exception (no stack info)".into()),
                        }
                    };
                    let err = fatal_exit(&mut rt, &global, err);
                    end_session(rt, engine);
                    return Err(err);
                    }
                }
                if let Err(e) = event_loop(&mut rt, &global, ErrorSource::Script { source, filename }, &mut fetch_rx, &mut ws_rx, &mut watch_rx, &mut child_rx, &mut net_rx, &mut worker_rx, &mut quic_rx, &mut napi_rx, &mut dispatch_rx).await {
                    let e = fatal_exit(&mut rt, &global, e);
                    end_session(rt, engine);
                    return Err(e);
                }
                // 自然退出派发 process 'exit'（CJS 主模块路径此前漏派发——node 套件几乎
                // 全走这条，common.mustCall 退出核对从未执行）。显式 exit 已派发过则跳过。
                if state::with_plain(|p| p.process_exited.is_none()) {
                    let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
                    crate::builtins::node::process_::emit_exit(&mut realm, global.get());
                }
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
        // node 口径：`-e` 自带 builtin 全局量（max-header-size 套件 `-p
        // 'http.maxHeaderSize'` 形；真机 26.8.2 实测 30 项，test/sqlite 除外）。
        // 独立 setup 脚本先行（行号零影响；失败忽略；已存在即跳过）。
        // 仅 Eval 模式（--run 文件/REPL 不走，真机同）。
        // F1 懒加载：eager require 29 模块占 --eval 启动 175ms（--run 空文件
        // 127ms vs --eval 1 302ms 实测）；此处只装 getter，首次访问才 require
        // 并原位替换为值（enumerable/configurable/writable 与直接赋值一致）。
        if mode == Mode::Eval {
            let setup = CString::new("eval-globals.js").unwrap_or_else(|_| c"eval.js".into());
            let setup_options = CompileOptionsWrapper::new(rt.cx(), setup, 1);
            rooted!(&in(rt.cx()) let mut setup_rval = UndefinedValue());
            const SETUP: &str = r#"try {
  if (typeof require === "function") {
    for (const __m of ["http","https","http2","fs","path","os","util","crypto","stream","events","url","querystring","net","dns","dgram","child_process","cluster","worker_threads","vm","assert","buffer","process","console","timers","zlib","readline","tty","v8","sys"]) {
      try {
        if (globalThis[__m] !== undefined) continue;
        Object.defineProperty(globalThis, __m, {
          configurable: true,
          enumerable: true,
          get() {
            let v;
            try { v = require("node:" + __m); } catch (e) { return undefined; }
            try {
              Object.defineProperty(globalThis, __m, { value: v, writable: true, configurable: true, enumerable: true });
            } catch (e) {}
            return v;
          },
          set(v) {
            try {
              Object.defineProperty(globalThis, __m, { value: v, writable: true, configurable: true, enumerable: true });
            } catch (e) {}
          }
        });
      } catch (e) {}
    }
  }
} catch (e) {}"#;
            let _ = evaluate_script(rt.cx(), global.handle(), SETUP, setup_rval.handle_mut(), setup_options);
        }
        // evaluate_script 内部自进 realm；rval 为 rooted 出参，跨事件循环存活
        let res = evaluate_script(rt.cx(), global.handle(), source, rval.handle_mut(), options);
        if res.is_err() {
            if mode == Mode::Eval {
                let r = eval_syntax_fallback(&mut rt, &global, source, filename, &mut fetch_rx, &mut ws_rx, &mut watch_rx, &mut child_rx, &mut net_rx, &mut worker_rx, &mut quic_rx, &mut napi_rx, &mut dispatch_rx).await
                    .map_err(|e| fatal_exit(&mut rt, &global, e));
                // §4.8：跳过引擎/运行时析构（StoreBuffer 悬垂边在 destroyRuntime 的小 GC 里 SEGV）
                end_session(rt, engine);
                return r;
            }
            // 模块重试：经典 SyntaxError 且能按模块解析 → 改走模块求值。
            // （`await` 在参数位置按标识符解析，报的不是 await 错而是 missing-paren，
            // 故不能只认 await 文案；真语法错误则保留原始经典报错。见 §4.17。）
            let (info_opt, is_syntax, kind, prim, entry_handled) = {
                let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
                rooted!(&in(&mut realm) let mut exc = UndefinedValue());
                // SAFETY: realm 内读取 pending exception（消费异常值）
                let info = { let i = error_info_from_exception_stack(&mut realm, exc.handle_mut()); crate::jsapi_glue::fill_message(&mut realm, i, exc.get()) };
                let kind = exc_name(&mut realm, exc.get());
                let is_syntax = kind.as_deref() == Some("SyntaxError");
                // P2-process R7：入口抛错先走 uncaught 分发（capture→监听），
                // 接住即转事件循环（脚本中止但进程续活）；语法错照旧走重试/渲染。
                let entry_handled = if !is_syntax {
                    crate::builtins::node::process_::dispatch_entry_throw(
                        &mut realm,
                        global.get(),
                    )
                } else {
                    false
                };
                // 10f：worker 内非对象异常走原始值信封（同模块路径）。
                let prim = if !state::worker_is_main() {
                    crate::jsapi_glue::exc_prim_marker(&mut realm, exc.get())
                } else {
                    None
                };
                (info, is_syntax, kind, prim, entry_handled)
            };
            if entry_handled {
                // 脚本中止但进程续活：落到下方的事件循环（mustCall 在 exit 结算）。
            } else {
                if is_syntax
                    && let Ok(url) = crate::loader::resolve::entry_url(std::path::Path::new(filename))
                    && let Ok(path) = url.to_file_path()
                    && crate::loader::load_js(source, filename, &path).is_ok()
                {
                    tracing::info!(target: "winterjs2::runtime", url = url.as_str(), "retrying as module");
                    let r = run_module(&mut rt, &global, &url, &mut fetch_rx, &mut ws_rx, &mut watch_rx, &mut child_rx, &mut net_rx, &mut worker_rx, &mut quic_rx, &mut napi_rx, &mut dispatch_rx).await
                        .map_err(|e| fatal_exit(&mut rt, &global, e));
                    end_session(rt, engine);
                    return r;
                }
                let err = match info_opt {
                    Some(info) => Error::script_with_kind(
                        filename, source, info.line.max(1), info.col,
                        prim.unwrap_or(info.message), kind),
                    None => Error::Other("uncaught JS exception (no stack info)".into()),
                };
                let err = fatal_exit(&mut rt, &global, err);
                end_session(rt, engine);
                return Err(err);
            }
        }
    }

    // 未包装成功的场景（含全部 Script 与无顶层 await 的 Eval）：
    // 完成值就是 rval（老行为）；仅 async IIFE 包装路径才读 __wjs2_value。
    if let Err(e) = event_loop(&mut rt, &global, ErrorSource::Script { source, filename }, &mut fetch_rx, &mut ws_rx, &mut watch_rx, &mut child_rx, &mut net_rx, &mut worker_rx, &mut quic_rx, &mut napi_rx, &mut dispatch_rx).await {
        let e = fatal_exit(&mut rt, &global, e);
        end_session(rt, engine);
        return Err(e);
    }
    // 自然退出：派发 process 'exit'（common.mustCall 计数结算点；Node 口径）。
    // 显式 process.exit 已在 JS 侧派发过（process_exited 旗），此处跳过防双发。
    if state::with_plain(|p| p.process_exited.is_none()) {
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        crate::builtins::node::process_::emit_exit(&mut realm, global.get());
    }
    let r = print_completion(&mut rt, &global, rval.get(), false);
    // §4.8：跳过引擎/运行时析构（带 timer 的路径在 JS_DestroyContext 里 SEGV）。
    // CLI 进程即将退出，内存由 OS 回收；见 AGENTS §4.8。
    end_session(rt, engine);
    r
}

/// 独立线程跑一段脚本（test runner 用，§4.24）：JS 线程私有的 CONTEXT TLS /
/// state TLS 全随线程生灭，Runtime 照 §4.8 泄漏。同步接口：调用方阻塞等结果
/// （与 sqlite worker 同哲学）。栈给 16MB（引擎 STACK_QUOTA 按主线程量级假设，
/// 线程默认栈远不够）。
pub fn run_isolated(source: String, filename: String, extra_args: Vec<String>) -> Result<(), Error> {
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("winterjs2-test-file".into())
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
