//! runtime/repl：交互会话（持久 realm + 短轮询事件泵）。

use super::*;
use mozjs::jsapi::JSObject;
use mozjs::jsval::UndefinedValue;
use mozjs::realm::AutoRealm;
use mozjs::rooted;
use mozjs::rust::RootedGuard;
use mozjs::rust::Runtime;
use std::ffi::CString;
use crate::error::Error;

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
        match { let i = error_info_from_exception_stack(&mut realm, exc.handle_mut()); crate::jsapi_glue::fill_message(&mut realm, i, exc.get()) } {
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
