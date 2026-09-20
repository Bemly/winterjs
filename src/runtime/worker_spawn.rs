//! runtime/worker_spawn：worker 线程起停（引擎单例 + 独立线程）。

use super::*;

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
    pub(crate) static WORKER_BOOT: std::cell::RefCell<Option<crate::builtins::node::worker::WorkerBoot>> =
        std::cell::RefCell::new(None);
}

/// worker 线程入口（`node:worker_threads` spawn 用，§4.24 哲学：每 worker 独立
/// OS 线程 + 完整会话；Runtime 照 §4.8 泄漏，线程退出即清 TLS）。
/// 退出码经 `WExit` 事件回主会话（成功 0/未捕获错 1/终止 1/`process.exit(n)`→n），
/// 错误文案经 `WError` 先行（随后必跟 `WExit{1}`）。detached 线程，主侧不等。
pub fn run_worker_thread(spec: WorkerThreadSpec) {
    use crate::builtins::node::worker::WorkerEvent;
    // 终止槽（主线程注册；worker 线程 TLS 绑定同旗——interrupt 回调读取）。
    let term_slot = crate::builtins::node::worker::term_slot_register(spec.worker_id);
    let spawned = std::thread::Builder::new()
        .name(format!("winterjs-worker-{}", spec.worker_id))
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            let wid = spec.worker_id;
            // 终止旗进 TLS（interrupt 回调与 WError 抑制同源读取）。
            crate::builtins::node::worker::term_tls_bind(term_slot.flag.clone());
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
                    // 终止引发的 uncatchable interrupt 错：不发 WError（真机口径——
                    // terminate 的错误不进 worker 'error' 事件，vm-context-terminate
                    // 套件 error mustNotCall 点名），只按终止退出码 1 收场。
                    if !crate::builtins::node::worker::term_tls_flagged() {
                        let inbox = state::with_plain(|p| p.worker_main_inbox.clone());
                        if let Some(tx) = inbox {
                            let _ = tx.send(WorkerEvent::WError { worker_id: wid, message });
                        }
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
