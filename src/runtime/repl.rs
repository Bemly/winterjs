//! runtime/repl：交互会话（持久 realm + 短轮询事件泵）。

use super::*;
use mozjs::jsapi::JSObject;
use mozjs::jsval::UndefinedValue;
use mozjs::realm::AutoRealm;
use mozjs::rooted;
use mozjs::rust::RootedGuard;
use mozjs::rust::Runtime;
use std::ffi::CString;
use std::io::IsTerminal as _;
use crate::error::Error;
use crate::repl::{CompReq, CompResp};

/// JS 线程执行补全（只在主循环调用）：求值 `__wjs_cli_complete(line)`
/// （prelude/repl_complete 底座桥）并解析 `[[text, disp, desc], ...], completeOn`；
/// 求值失败/形状不合法回 `(空, line)`（reedline 空集）。
fn cli_complete_js(
    rt: &mut Runtime,
    global: &RootedGuard<'_, *mut JSObject>,
    line: &str,
) -> (Vec<(String, Option<String>, Option<String>)>, String) {
    let fallback = (Vec::new(), line.to_owned());
    let script = format!(
        "JSON.stringify(globalThis.__wjs_cli_complete({}))",
        serde_json::to_string(line).unwrap_or_else(|_| "\"\"".into()),
    );
    let c_filename = CString::new("repl.js").expect("no NUL");
    rooted!(&in(rt.cx()) let mut rval = UndefinedValue());
    let options = CompileOptionsWrapper::new(rt.cx(), c_filename, 1);
    if evaluate_script(rt.cx(), global.handle(), &script, rval.handle_mut(), options).is_err() {
        return fallback;
    }
    let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
    rooted!(&in(&mut realm) let rv = rval.get());
    let Ok(ConversionResult::Success(s)) = String::from_jsval(&mut realm, rv.handle(), ()) else {
        return fallback;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else {
        return fallback;
    };
    let Some(items) = v.get(0).and_then(|x| x.as_array()) else {
        return fallback;
    };
    let complete_on = v
        .get(1)
        .and_then(|x| x.as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| line.to_owned());
    (
        items
            .iter()
            .filter_map(|x| {
                if let Some(t) = x.as_str() {
                    return Some((t.to_owned(), None, None));
                }
                // 桥三元组 `[全文, 左格签名后缀, 描述]`（后两项可 null）。
                let pair = x.as_array()?;
                let text = pair.first()?.as_str()?.to_owned();
                let disp = pair.get(1).and_then(|d| d.as_str()).map(str::to_owned);
                let desc = pair.get(2).and_then(|d| d.as_str()).map(str::to_owned);
                Some((text, disp, desc))
            })
            .collect(),
        complete_on,
    )
}

/// REPL 输出统一口：CRLF 化 + 尾换行 + stdout 刷出（仅 TTY 会话——旗未置位的
/// 管道/黑盒路径字节恒等原 println/eprintln 行为）。
/// TTY 下读行线程 raw mode 期间裸 `\n` 不回车（`crate::repl::crlf` 注记）；
/// 管道化自测/黑盒下 stdout 块缓冲需显式刷（沿用 `print_completion` 实测注记）。
fn repl_out(text: &str, to_stderr: bool) {
    if !crate::repl::tty_output_enabled() {
        if to_stderr {
            eprintln!("{text}");
        } else {
            println!("{text}");
            use std::io::Write as _;
            let _ = std::io::stdout().flush();
        }
        return;
    }
    let mut body = crate::repl::crlf(text);
    if !body.ends_with('\n') {
        body.push_str("\r\n");
    }
    if to_stderr {
        eprint!("{body}");
    } else {
        print!("{body}");
        use std::io::Write as _;
        let _ = std::io::stdout().flush();
    }
}

/// 哨兵发送：行处理轮的全部输出（含 pump 收割的 timer 错误/rejection）落流后
/// 放行读行线程渲染下一轮 prompt（哨兵协议见 `crate::repl::readline_loop`）。
fn send_flush(flush_tx: &tokio::sync::mpsc::UnboundedSender<()>, pending: &mut bool) {
    if *pending {
        *pending = false;
        let _ = flush_tx.send(());
    }
}

/// REPL 单步结果（求值永不抛错：错误打印后继续；只有 `process.exit` 跳出）。
enum ReplStep {
    Done,
    /// top-level await 挂起：promise 存 `tla` 槽，主循环每轮 pump 后查结算。
    Pending,
    Exited(i32),
}

/// TLA 包装文本（R6）：调底座桥 `__wjs_repl_tla_wrap(line)`（prelude/repl_complete，
/// 末表达式 return 化 + .then 双臂）；桥缺位/失败/返回非串（含 null=不改写）回 None。
fn wrap_text_via_bridge(
    rt: &mut Runtime,
    global: &RootedGuard<'_, *mut JSObject>,
    line: &str,
) -> Option<String> {
    // cx 当前 realm 即 global（行处理轮；同 cli_complete_js 口径，不显式 AutoRealm）。
    let c_filename = CString::new("repl.js").expect("no NUL");
    rooted!(&in(rt.cx()) let mut wtext = UndefinedValue());
    let wopts = CompileOptionsWrapper::new(rt.cx(), c_filename, 1);
    let wcall = format!(
        "globalThis.__wjs_repl_tla_wrap({})",
        serde_json::to_string(line).unwrap_or_else(|_| "\"\"".into()),
    );
    let ok = evaluate_script(rt.cx(), global.handle(), &wcall, wtext.handle_mut(), wopts);
    if ok.is_err() || !wtext.get().is_string() {
        return None;
    }
    rooted!(&in(rt.cx()) let wtext_root = wtext.get());
    match String::from_jsval(rt.cx(), wtext_root.handle(), ()) {
        Ok(ConversionResult::Success(s)) if !s.is_empty() => Some(s),
        _ => None,
    }
}

/// TLA 结算提取（R6）：pump 已推进后查 `state.repl_tla` 的 promise；主 realm 内
/// 完成全部 JSAPI（4.116），rejected 当场转 engine pending 并取错误信息——
/// reason 值不跨 realm drop（§4.80）。resolved 值裸出（到 print_completion 间
/// 无 JS 点，take_pending_exception 先例）。
enum TlaOutcome {
    None,
    /// 已结算值（裸 JSVal；realm 已 drop，调用方到 print_completion 间无 JS 点）。
    Resolved(mozjs::jsval::JSVal),
    /// rejected 已转 engine pending 并取走信息（渲染在 realm 外，纯文本）。
    Rejected(mozjs::rust::ErrorInfo, Option<String>),
}

fn settle_tla(
    rt: &mut Runtime,
    global: &RootedGuard<'_, *mut JSObject>,
) -> TlaOutcome {
    if state::with_rooted(|p| p.repl_tla.get().is_undefined()) {
        return TlaOutcome::None;
    }
    let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
    let pv = state::with_rooted(|p| p.repl_tla.get());
    let mut cx = unsafe { crate::jsapi_glue::wrap_cx(realm.raw_cx()) };
    let out = match crate::jsapi_glue::promise_settled_value(&mut cx, pv.to_object()) {
        Some(Some((_, packed))) => crate::jsapi_glue::tla_pack_take(&mut cx, packed.to_object()),
        _ => None,
    };
    if out.is_none() {
        return TlaOutcome::None;
    }
    state::with_rooted(|p| p.repl_tla.set(mozjs::jsval::UndefinedValue()));
    match out {
        Some((true, v)) => TlaOutcome::Resolved(v),
        Some((false, reason)) => {
            crate::jsapi_glue::set_pending_exception(&mut cx, reason);
            rooted!(&in(&mut realm) let mut exc = UndefinedValue());
            // SAFETY: realm 内读取 pending exception（消费异常值）
            let info = { let i = error_info_from_exception_stack(&mut realm, exc.handle_mut()); crate::jsapi_glue::fill_message(&mut realm, i, exc.get()) };
            let kind = exc_name(&mut realm, exc.get());
            match info {
                Some(info) => TlaOutcome::Rejected(info, kind),
                None => TlaOutcome::Rejected(
                    mozjs::rust::ErrorInfo { line: 1, col: 1, message: "uncaught JS exception".into(), ..info.unwrap() },
                    None,
                ),
            }
        }
        None => TlaOutcome::None,
    }
}

/// 消费当前 pending exception 并按 D4 渲染（TTY miette / 非 TTY node 形，
/// 经 repl_out CRLF 化）。REPL 错误与 TLA rejected 共用。返回是否渲染了。
fn render_repl_error(rt: &mut Runtime, global: &RootedGuard<'_, *mut JSObject>, line: &str) -> bool {
    let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
    rooted!(&in(&mut realm) let mut exc = UndefinedValue());
    // SAFETY: realm 内读取 pending exception（消费异常值）
    match { let i = error_info_from_exception_stack(&mut realm, exc.handle_mut()); crate::jsapi_glue::fill_message(&mut realm, i, exc.get()) } {
        Some(info) => {
            let kind = exc_name(&mut realm, exc.get());
            let err = crate::error::Error::script_with_kind(
                "repl.js",
                line,
                info.line.max(1),
                info.col,
                info.message,
                kind,
            );
            let color =
                crate::error::render_color().unwrap_or(crate::settings::ColorChoice::Auto);
            repl_out(&err.render_string(color), true);
            true
        }
        None => {
            repl_out("uncaught JS exception (no stack info)", true);
            false
        }
    }
}

/// REPL 单行求值：经典脚本求值 + 完成值打印。
/// top-level await（R6，node lib/repl.js defaultEval 同构语义）：原码编译失败
/// 且行含 `await` 时试 async-IIFE 包装重试（`await` 在 async 函数内则原码本就
/// 编译通过；在非 async 函数内则包装也失败，报原错）；包装产物恒为 Promise，
/// 存 `tla` 槽挂起（ReplStep::Pending），主循环逐轮 pump 查结算后打印/渲染。
/// 缺口（记 plan3 §7）：声明提升（`let a = await x` 跨行存活）为 node acorn
/// AST 重写语义，待拍板引包后逐字移植；顶层 return 判定同缺。
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
        // TLA 试错：先消费原错（info/exc 在手），包装可编译即改走挂起。
        let mut info = None;
        let mut kind: Option<String> = None;
        {
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            rooted!(&in(&mut realm) let mut exc = UndefinedValue());
            // SAFETY: realm 内读取 pending exception（消费异常值）
            info = { let i = error_info_from_exception_stack(&mut realm, exc.handle_mut()); crate::jsapi_glue::fill_message(&mut realm, i, exc.get()) };
            kind = exc_name(&mut realm, exc.get());
        }
        let is_syntax = kind.as_deref() == Some("SyntaxError");
        if is_syntax && line.contains("await") {
            // 包装文本经底座桥（prelude/repl_complete：末表达式 return 化 +
            // .then 双臂装 handler，rejected 不进 jobqueue 的 unhandled 收割）。
            if let Some(wrapped) = wrap_text_via_bridge(rt, global, line) {
                let c_filename = CString::new("repl.js").expect("no NUL");
                rooted!(&in(rt.cx()) let mut wval = UndefinedValue());
                let options = CompileOptionsWrapper::new(rt.cx(), c_filename, 1);
                let wres = evaluate_script(rt.cx(), global.handle(), &wrapped, wval.handle_mut(), options);
                if wres.is_ok() {
                    // 挂起槽转移（§4.40：state Heap 槽 trace 保活；包装恒返回 Promise）
                    state::with_rooted(|p| p.repl_tla.set(wval.get()));
                    return ReplStep::Pending;
                }
            }
        }
        match info {
            Some(info) => {
                // C 档：D4 渲染复用——render_string 拿文本（TTY miette 图形 /
                // 非 TTY node 形），经 repl_out CRLF 化落 stderr（raw mode 终端
                // 裸 `\n` 阶梯，见 `crate::repl::crlf`）；栈经 note_stack 按
                // message 配对取走（`script_with_kind` 内）。退出码丢弃
                // （REPL 出错只打印不退出）。
                let err = crate::error::Error::script_with_kind(
                    "repl.js",
                    line,
                    info.line.max(1),
                    info.col,
                    info.message,
                    kind,
                );
                let color =
                    crate::error::render_color().unwrap_or(crate::settings::ColorChoice::Auto);
                repl_out(&err.render_string(color), true);
            }
            None => repl_out("uncaught JS exception (no stack info)", true),
        }
        return ReplStep::Done;
    }
    if let Some(code) = exited() {
        return ReplStep::Exited(code);
    }
    if let Err(e) = print_completion(rt, global, rval.get(), crate::repl::tty_output_enabled()) {
        if let Some(code) = exited() {
            return ReplStep::Exited(code);
        }
        repl_out(&e.to_string(), true);
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
    // REPL 专属退出函数（`--run/--eval` 脚本不可见，不污染用户全局）：
    // exit()/quit()/q() 即 process.exit(0)。求值失败不致命（会话照起，仍可用 .exit）。
    {
        let code = "globalThis.exit = function exit() { process.exit(); };\n\
            globalThis.quit = function quit() { process.exit(); };\n\
            globalThis.q = function q() { process.exit(); };";
        let c_filename = CString::new("repl.js").expect("no NUL");
        rooted!(&in(rt.cx()) let mut rval = UndefinedValue());
        let options = CompileOptionsWrapper::new(rt.cx(), c_filename, 1);
        let _ = evaluate_script(rt.cx(), global.handle(), code, rval.handle_mut(), options);
    }
    // acorn + TLA 包装（§0.5 拍板 vendored；prelude/repl_complete::REPL_TLA_JS）
    // 注入 REPL 会话——不进 PRELUDE（全会话加载拖慢 worker/child 小窗口时序
    // 测试的启动，R6b 实测）。注入失败仅降级（TLA 报原语法错，无包装）。
    {
        let c_filename = CString::new("repl.js").expect("no NUL");
        rooted!(&in(rt.cx()) let mut rval = UndefinedValue());
        let options = CompileOptionsWrapper::new(rt.cx(), c_filename, 1);
        let _ = evaluate_script(
            rt.cx(),
            global.handle(),
            &format!(
                "{}{}{}",
                crate::builtins::prelude::vendor::ACORN_JS,
                crate::builtins::prelude::vendor::ACORN_WALK_JS,
                crate::builtins::prelude::repl_complete::REPL_TLA_JS
            ),
            rval.handle_mut(),
            options,
        );
    }
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
    let (flush_tx, flush_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    // 补全跨线程管线（readline 线程 Tab → 本循环 JS 求值 → 回包）。
    let (comp_req_tx, mut comp_req_rx) = tokio::sync::mpsc::unbounded_channel::<CompReq>();
    let (comp_resp_tx, comp_resp_rx) = tokio::sync::mpsc::unbounded_channel::<CompResp>();
    let hist = crate::repl::history_path();
    // readline 是阻塞 IO，独占线程跑（`Editor` 不出线程，无跨线程共享）。
    std::thread::spawn(move || {
        crate::repl::readline_loop(line_tx, hist, flush_rx, comp_req_tx, comp_resp_rx)
    });
    // TTY 会话：用户输出 CRLF 化（raw mode 阶梯，`crate::repl::crlf`）；
    // SIGINT 置忽略——哨兵窗口（行处理期间）读行线程不在 `read_line`（非 raw、
    // ISIG 开），Ctrl-C 会走内核默认终止杀掉整个会话；忽略后该毫秒级窗口的
    // Ctrl-C 丢失，等价于现行为（reedline 在读时才捕获连击）。libc FFI 面。
    if std::io::stdin().is_terminal() {
        crate::repl::set_tty_output(true);
        // SAFETY: `signal` 无内存安全前置条件；REPL 进程生命周期内不恢复默认。
        unsafe { libc::signal(libc::SIGINT, libc::SIG_IGN) };
    }
    println!("winterjs repl (type .exit to quit)");
    // stdout 管道时块缓冲：banner 立即刷出，否则与 stderr 行错序（实测）。
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    tracing::info!(target: "winterjs::runtime", "repl start");

    let err_src = ErrorSource::Script { source: "", filename: "repl.js" };
    // TLA 挂起：state.repl_tla 槽（§4.40 Box<Heap>+trace 模式；跨轮/跨 GC 存活）。
    // Undefined = 无挂起。逐轮 pump 推进 jobqueue，结算后打印/渲染。
    let mut pending_flush = false;
    loop {
        let st = match pump_once(
            &mut rt, &global, err_src, &mut fetch_rx, &mut ws_rx, &mut watch_rx, &mut child_rx, &mut net_rx, &mut worker_rx, &mut quic_rx, &mut napi_rx, &mut dispatch_rx, true,
        )
        .await
        {
            Ok(st) => st,
            Err(e) => {
                if let Some(code) = state::with_plain(|p| p.process_exited) {
                    end_session(rt, engine);
                    return Err(Error::Exit(code));
                }
                repl_out(&e.to_string(), true);
                send_flush(&flush_tx, &mut pending_flush);
                continue;
            }
        };
        if st.exited {
            let code = state::with_plain(|p| p.process_exited).unwrap_or(0);
            end_session(rt, engine);
            return Err(Error::Exit(code));
        }
        // TLA 结算检查（pump 已推进 microtask/timer；先于 unhandled 收割——
        // 包装 .then 双臂已装 handler，rejected 不会进收割，此处消费结果）。
        // 结算即打印/渲染并清槽；未结算时哨兵不发（读行线程继续等，node 同形：
        // await 期间无 prompt）。
        match settle_tla(&mut rt, &global) {
            TlaOutcome::Resolved(v) => {
                if let Err(e) =
                    print_completion(&mut rt, &global, v, crate::repl::tty_output_enabled())
                {
                    repl_out(&e.to_string(), true);
                }
            }
            TlaOutcome::Rejected(info, kind) => {
                let err = crate::error::Error::script_with_kind(
                    "repl.js", "", info.line.max(1), info.col, info.message, kind,
                );
                let color =
                    crate::error::render_color().unwrap_or(crate::settings::ColorChoice::Auto);
                repl_out(&err.render_string(color), true);
            }
            TlaOutcome::None => {}
        }
        // 每轮收割 unhandled rejection（Node 式打印，继续不退出）。
        if let Err(e) = report_unhandled_rejections(&mut rt, &global) {
            repl_out(&e.to_string(), true);
        }
        if pending_flush && state::with_rooted(|p| p.repl_tla.get().is_undefined()) {
            send_flush(&flush_tx, &mut pending_flush);
        }
        tokio::select! {
            line = line_rx.recv() => {
                match line {
                    // EOF（Ctrl-D）或 readline 线程结束：TLA 挂起则先 drain
                    // （等结算落地再收会话；30s 上限防挂死）。
                    None | Some(None)
                        if state::with_rooted(|p| !p.repl_tla.get().is_undefined()) =>
                    {
                        let mut wait = 0u32;
                        while state::with_rooted(|p| !p.repl_tla.get().is_undefined()) && wait < 6000 {
                            let st2 = match pump_once(
                                &mut rt, &global, err_src, &mut fetch_rx, &mut ws_rx, &mut watch_rx, &mut child_rx, &mut net_rx, &mut worker_rx, &mut quic_rx, &mut napi_rx, &mut dispatch_rx, true,
                            ).await {
                                Ok(st2) => st2,
                                Err(_) => break,
                            };
                            if st2.exited {
                                break;
                            }
                            match settle_tla(&mut rt, &global) {
                                TlaOutcome::Resolved(v) => {
                                    if let Err(e) = print_completion(&mut rt, &global, v, crate::repl::tty_output_enabled()) {
                                        repl_out(&e.to_string(), true);
                                    }
                                }
                                TlaOutcome::Rejected(info, kind) => {
                                    let err = crate::error::Error::script_with_kind(
                                        "repl.js", "", info.line.max(1), info.col, info.message, kind,
                                    );
                                    let color = crate::error::render_color().unwrap_or(crate::settings::ColorChoice::Auto);
                                    repl_out(&err.render_string(color), true);
                                }
                                TlaOutcome::None => {}
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                            wait += 1;
                        }
                        break;
                    }
                    None | Some(None) => break,
                    Some(Some(text)) => {
                        // 哨兵挂起：本行处理（含 continue 路径）完毕后放行读行线程。
                        pending_flush = true;
                        if text.trim().is_empty() {
                            continue;
                        }
                        match crate::repl::dot_command(&text) {
                            crate::repl::Dot::Exit => break,
                            crate::repl::Dot::Help => {
                                repl_out(
                                    ".exit  quit the repl\n.help  show this help\n\
                                     .doc <topic>  show MDN docs (e.g. .doc console.log)\n\
                                     exit()/quit()/q()  quit the repl (REPL-only functions)\n\
                                     Ctrl+C twice in 2s / Ctrl+D  quit the repl",
                                    false,
                                );
                                continue;
                            }
                            crate::repl::Dot::Doc(topic) => {
                                if topic.is_empty() {
                                    repl_out(".doc <topic> — e.g. .doc console.log", true);
                                    continue;
                                }
                                match crate::repl_doc::lookup(&topic) {
                                    Some(md) => {
                                        let body = if crate::repl::tty_output_enabled() {
                                            crate::repl_doc::render_tty(md)
                                        } else {
                                            crate::repl_doc::render_plain(md)
                                        };
                                        repl_out(&body, false);
                                    }
                                    None => repl_out(&crate::repl_doc::unknown_hint(&topic), true),
                                }
                                continue;
                            }
                            crate::repl::Dot::Code => {}
                        }
                        match repl_eval(&mut rt, &global, &text).await {
                            ReplStep::Done | ReplStep::Pending => {}
                            ReplStep::Exited(code) => {
                                end_session(rt, engine);
                                return Err(Error::Exit(code));
                            }
                        }
                    }
                }
            }
            // 补全请求：JS 线程真上下文求值（node:repl cliComplete，R5 桥），
            // 回包经 id 配对返回 readline 线程；readline 线程退出后通道断即枯竭。
            req = comp_req_rx.recv() => {
                if let Some(r) = req {
                    let (items, complete_on) = cli_complete_js(&mut rt, &global, &r.line);
                    let _ = comp_resp_tx.send(CompResp { id: r.id, items, complete_on });
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
