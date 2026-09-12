//! `node:child_process` 同步子集 + 异步 `spawn`（plan Phase 4d/c-4x）。
//! 同步经 `std::process` 阻塞跑；异步经 `tokio::process` + 事件循环分发
//! `exit/close/error`；stdio 支持 `inherit`/`ignore`/`pipe`（字符串三同或三元数组）。
//! pipe 口径（文档记录）：stdout/stderr 为 live `ReadableStream`（出的块序 = 进程
//! 出序；exit 前必先送达残留数据再调 onexit/onclose）；stdin 为 `WritableStream`
//!（write 满即发，close 关写端；子进程已走即写失败）；无 max_buffer 上限（流式消费）。
//! `detached:true` 在 unix 起 setsid 组长，kill 走组杀（`nix` 轮子；win 回退直杀）。
//! 结果走 JSON 桥（二进制 base64）；错误形状由 prelude 组装（见 SOURCE）。

use std::io::{Read as _, Write as _};
use std::time::{Duration, Instant};

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::jsapi::JSObject;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};
use crate::state;

/// spawn 选项（prelude 传 JSON；`None` 表缺省）。
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct SpawnOpts {
    cwd: Option<String>,
    /// 全量替换环境（None=继承；`{}`=清空，Node 同语义）。
    env: Option<std::collections::HashMap<String, String>>,
    /// 超时毫秒（0/缺省=无限；超时杀直系，`signal="SIGKILL"`）。
    timeout_ms: u64,
    /// detached 组长化（unix setsid；kill 走组杀，见 `make_detached`）。
    detached: bool,
    /// shell（exec 由调用方拼好；spawn 经 shlex 拼）。
    shell: bool,
    /// stdin 输入（base64；None=null）。
    input_b64: Option<String>,
    /// 最大缓冲字节（默认 1MiB；超即 ENOBUFS）。
    max_buffer: Option<usize>,
}

/// 默认 shell（unix `/bin/sh`；win `cmd.exe`）。
fn default_shell() -> &'static str {
    if cfg!(windows) { "cmd.exe" } else { "/bin/sh" }
}

/// `detached` 组长化（unix setsid；`pre_exec` 只调 async-signal-safe 的 setsid）。
#[cfg(unix)]
fn make_detached(cmd: &mut std::process::Command) {
    use std::os::unix::process::CommandExt as _;
    // SAFETY: pre_exec 闭包跑在 fork 后 exec 前，只调 setsid（async-signal-safe），
    // 不触 Rust 运行时/锁/堆；setsid 失败忽略（退化普通子进程，kill 回退直杀）。
    unsafe {
        cmd.pre_exec(|| {
            let _ = nix::unistd::setsid();
            Ok(())
        });
    }
}

/// 退出状态 → `(status|null, signal|null)`（Node 形状；unix 取信号名常用集）。
fn status_parts(st: std::process::ExitStatus) -> (Option<i32>, Option<String>) {
    if let Some(code) = st.code() {
        return (Some(code), None);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        let name = match st.signal() {
            Some(1) => "SIGHUP",
            Some(2) => "SIGINT",
            Some(9) => "SIGKILL",
            Some(13) => "SIGPIPE",
            Some(15) => "SIGTERM",
            Some(n) => return (None, Some(format!("SIG{n}"))),
            None => return (None, None),
        };
        return (None, Some(name.to_string()));
    }
    #[cfg(not(unix))]
    return (None, None);
}

/// base64（JSON 桥二进制；`base64` 直引轮子）。
fn b64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// 命令执行（spawn + 超时轮询 + 全量读出；调用方定 shell 包装）。
/// 返回 JSON：`{spawnErr?, status, signal, stdout_b64, stderr_b64, timedOut}`。
fn run_command(
    mut cmd: std::process::Command,
    opts: &SpawnOpts,
    input: Option<Vec<u8>>,
) -> serde_json::Value {
    use std::process::Stdio;
    cmd.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() });
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(dir) = &opts.cwd {
        cmd.current_dir(dir);
    }
    #[cfg(unix)]
    if opts.detached {
        make_detached(&mut cmd);
    }
    if let Some(env) = &opts.env {
        cmd.env_clear();
        cmd.envs(env);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let code = crate::builtins::node::fs::io_code(&e);
            return serde_json::json!({
                "spawnErr": format!("{code}: spawn: {e}"),
                "pid": -1,
                "status": null, "signal": null,
                "stdout_b64": "", "stderr_b64": "",
                "timedOut": false,
            });
        }
    };
    let pid = child.id() as i64;
    if let Some(data) = input {
        if let Some(mut stdin) = child.stdin.take() {
            // 管道写失败（子进程早退）忽略，判 exit 时见分晓。
            let _ = stdin.write_all(&data);
        }
    }
    let deadline = if opts.timeout_ms > 0 {
        Some(Instant::now() + Duration::from_millis(opts.timeout_ms))
    } else {
        None
    };
    let timed_out = loop {
        match child.try_wait() {
            Ok(Some(_)) => break false,
            Ok(None) => {}
            Err(_) => break false,
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            // 超时杀（detached 组杀，unix；其余直杀；组杀顺延见头注）。
            #[cfg(unix)]
            if opts.detached {
                let pid = child.id();
                use nix::sys::signal::{kill, Signal};
                use nix::unistd::Pid;
                let _ = kill(Pid::from_raw(-(pid as i32)), Signal::SIGKILL)
                    .or_else(|_| kill(Pid::from_raw(pid as i32), Signal::SIGKILL));
            }
            let _ = child.kill();
            let _ = child.wait();
            break true;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_end(&mut stdout);
    }
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_end(&mut stderr);
    }
    // wait 收尸（try_wait 已见退出则即返；超时路径已 wait）。
    let status = child.wait().ok();
    let (status_code, signal) = status.map(status_parts).unwrap_or((None, None));
    let max = opts.max_buffer.unwrap_or(1024 * 1024);
    if stdout.len() + stderr.len() > max {
        return serde_json::json!({
            "spawnErr": "ENOBUFS: stdout maxBuffer exceeded",
            "pid": pid,
            "status": status_code, "signal": signal,
            "stdout_b64": b64(&stdout), "stderr_b64": b64(&stderr),
            "timedOut": timed_out,
        });
    }
    serde_json::json!({
        "spawnErr": null,
        "pid": pid,
        "status": if timed_out { serde_json::Value::Null } else { status_code.map(serde_json::Value::from).unwrap_or(serde_json::Value::Null) },
        "signal": if timed_out { serde_json::Value::String("SIGKILL".into()) } else { signal.map(serde_json::Value::String).unwrap_or(serde_json::Value::Null) },
        "stdout_b64": b64(&stdout),
        "stderr_b64": b64(&stderr),
        "timedOut": timed_out,
    })
}

fn set_rval_str(cx: &mut mozjs::context::JSContext, frame: &Frame, s: &str) {
    rooted!(&in(cx) let mut v = UndefinedValue());
    s.to_jsval(cx, v.handle_mut());
    frame.set_rval(v.get());
}

fn parse_opts(cx: &mut mozjs::context::JSContext, frame: &Frame, i: u32) -> Option<SpawnOpts> {
    if frame.argc() <= i || frame.arg(i).is_undefined() || frame.arg(i).is_null() {
        return Some(SpawnOpts::default());
    }
    let s = value_to_string(cx, frame.arg(i));
    match serde_json::from_str(&s) {
        Ok(o) => Some(o),
        Err(_) => {
            report_error(cx, "TypeError: child_process options must be JSON");
            None
        }
    }
}

/// `__wjs_cp_exec(cmdStr, optsJson)` → 结果 JSON（shell 由 prelude 包）。
pub unsafe extern "C" fn cp_exec(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: exec needs a command");
        return false;
    }
    let cmd_str = value_to_string(&mut cx, frame.arg(0));
    if let Err(msg) = crate::permissions::check_run(&cmd_str) {
        report_error(&mut cx, &msg);
        return false;
    }
    let Some(opts) = parse_opts(&mut cx, &frame, 1) else {
        return false;
    };
    // shell:false 即直接执行（`shlex::split` 切词；切不出报 EINVAL 风）。
    let cmd = if opts.shell {
        let mut c = std::process::Command::new(default_shell());
        if cfg!(windows) {
            c.arg("/C").arg(&cmd_str);
        } else {
            c.arg("-c").arg(&cmd_str);
        }
        c
    } else {
        match shlex::split(&cmd_str) {
            Some(parts) if !parts.is_empty() => {
                let mut c = std::process::Command::new(&parts[0]);
                c.args(&parts[1..]);
                c
            }
            _ => {
                report_error(&mut cx, "EINVAL: exec: cannot parse command without shell");
                return false;
            }
        }
    };
    let input = opts
        .input_b64
        .as_deref()
        .map(|b| {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD.decode(b).unwrap_or_default()
        })
        .filter(|v| !v.is_empty());
    let out = run_command(cmd, &opts, input);
    set_rval_str(&mut cx, &frame, &out.to_string());
    true
}

// ── 异步 spawn（`tokio::process` + 事件循环；stdio inherit/ignore/pipe）──────
// 所有权模型：`Child` 本体常驻 state 表（kill 经 `start_kill`/nix 同步调）；
// 等待 task 轮询 `try_wait`；pipe 泵独立 task 按块发事件；顺序保证：等待 task
// 见到退出后等 pipe 泵全部落定（state 计数器）才发 Exited，残留数据必先送达。

/// task → 事件循环（纯数据）。
pub struct ChildEvent {
    pub id: u64,
    pub kind: ChildKind,
}

pub enum ChildKind {
    /// 正常退出/被信号杀（含超时杀；spawn 失败走同步报错，不进事件）。
    Exited { status: Option<i32>, signal: Option<String> },
    /// pipe stdout 一块（base64；exit 前必先送达，见等待 task 的合流）。
    Stdout { data_b64: String },
    /// pipe stderr 一块（同上）。
    Stderr { data_b64: String },
}

/// stdin 写端命令（prelude WritableStream → 写 task；Orders 按序执行）。
pub enum StdinCmd {
    Write(Vec<u8>),
    Close,
}

/// pipe 读端句柄（二选一进泵 task）。
enum PipeHandle {
    Out(tokio::process::ChildStdout),
    Err(tokio::process::ChildStderr),
}

fn arg_string(cx: &mut mozjs::context::JSContext, frame: &Frame, i: u32, what: &str) -> Option<String> {
    if frame.argc() <= i {
        report_error(cx, &format!("TypeError: {what} requires an argument"));
        return None;
    }
    Some(value_to_string(cx, frame.arg(i)))
}

/// `__wjs_spawn_start(file, argsJson, optsJson, target, stdioStr)` → id。
/// target 为 prelude ChildProcess 对象（存 RootedState，事件读其 `on*` 属性）。
pub unsafe extern "C" fn spawn_start(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(file), Some(args_s)) = (
        arg_string(&mut cx, &frame, 0, "spawn"),
        arg_string(&mut cx, &frame, 1, "spawn"),
    ) else {
        return false;
    };
    let args: Vec<String> = serde_json::from_str(&args_s).unwrap_or_default();
    if let Err(msg) = crate::permissions::check_run(&[file.clone(), args.first().cloned().unwrap_or_default()].join(" ")) {
        report_error(&mut cx, &msg);
        return false;
    }
    let mut opts = if frame.argc() > 2 {
        match parse_opts(&mut cx, &frame, 2) {
            Some(o) => o,
            None => return false,
        }
    } else {
        SpawnOpts::default()
    };
    if frame.argc() < 4 || !frame.arg(3).is_object() {
        report_error(&mut cx, "TypeError: spawn internals missing target");
        return false;
    }
    let target = frame.arg(3);
    // stdio（prelude 已归一为三元 JSON 数组；每项 inherit/ignore/pipe）。
    let stdio: Vec<String> = if frame.argc() > 4 && frame.arg(4).is_string() {
        match serde_json::from_str::<Vec<String>>(&value_to_string(&mut cx, frame.arg(4))) {
            Ok(v) if v.len() == 3 && v.iter().all(|s| matches!(s.as_str(), "inherit" | "ignore" | "pipe")) => v,
            _ => {
                report_error(&mut cx, "TypeError: spawn stdio must be inherit/ignore/pipe");
                return false;
            }
        }
    } else {
        vec!["inherit".to_string(); 3]
    };
    let (pipe_in, pipe_out, pipe_err) = (stdio[0] == "pipe", stdio[1] == "pipe", stdio[2] == "pipe");
    let Some((id, tx)) = state::child_alloc() else {
        report_error(&mut cx, "failed to load settings: child driver not installed");
        return false;
    };
    let handle = tokio::runtime::Handle::try_current();
    let Ok(handle) = handle else {
        report_error(&mut cx, "OperationError: no async runtime for spawn");
        return false;
    };
    use std::process::Stdio;
    let mut cmd = tokio::process::Command::new(&file);
    cmd.args(&args);
    cmd.stdin(if pipe_in { Stdio::piped() } else { Stdio::null() });
    cmd.stdout(match stdio[1].as_str() {
        "ignore" => Stdio::null(),
        "pipe" => Stdio::piped(),
        _ => Stdio::inherit(),
    });
    cmd.stderr(match stdio[2].as_str() {
        "ignore" => Stdio::null(),
        "pipe" => Stdio::piped(),
        _ => Stdio::inherit(),
    });
    if let Some(dir) = &opts.cwd {
        cmd.current_dir(dir);
    }
    if let Some(env) = opts.env.take() {
        cmd.env_clear();
        cmd.envs(&env);
    }
    #[cfg(unix)]
    if opts.detached {
        make_detached(cmd.as_std_mut());
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            use crate::builtins::node::fs::io_code;
            report_error(&mut cx, &format!("{}: spawn {file}: {e}", io_code(&e)));
            return false;
        }
    };
    // pipe 泵（落定必记数，EOF/错皆记，见 child_pipe_done）。
    // stdin 写 task：handle 与 rx 此处齐备，随 child_add 登记 tx（写端 drop 即 EOF）。
    let stdin_tx = if pipe_in {
        match child.stdin.take() {
            Some(h) => {
                let (wtx, mut wrx) = tokio::sync::mpsc::unbounded_channel::<StdinCmd>();
                handle.spawn(async move {
                    use tokio::io::AsyncWriteExt as _;
                    let mut h = h;
                    while let Some(cmd) = wrx.recv().await {
                        match cmd {
                            StdinCmd::Write(data) => {
                                if h.write_all(&data).await.is_err() {
                                    break;
                                }
                            }
                            StdinCmd::Close => break,
                        }
                    }
                });
                Some(wtx)
            }
            None => None,
        }
    } else {
        None
    };
    let mut pipes_expected: u8 = 0;
    for (piped, take) in [
        (pipe_out, true),
        (pipe_err, false),
    ] {
        if !piped {
            continue;
        }
        let handle_opt = if take { child.stdout.take().map(PipeHandle::Out) } else { child.stderr.take().map(PipeHandle::Err) };
        let Some(handle_h) = handle_opt else {
            continue;
        };
        pipes_expected += 1;
        let txc = tx.clone();
        handle.spawn(async move {
            use tokio::io::AsyncReadExt as _;
            let mut buf = [0u8; 8192];
            match handle_h {
                PipeHandle::Out(mut h) => loop {
                    match h.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            if txc.send(ChildEvent { id, kind: ChildKind::Stdout { data_b64: b64(&buf[..n]) } }).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                },
                PipeHandle::Err(mut h) => loop {
                    match h.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            if txc.send(ChildEvent { id, kind: ChildKind::Stderr { data_b64: b64(&buf[..n]) } }).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                },
            }
            state::child_pipe_done(id);
        });
    }
    let timeout_ms = opts.timeout_ms;
    let detached = opts.detached;
    state::child_add(id, child, detached, target, stdin_tx, pipes_expected);
    tracing::info!(target: "winterjs::child", id, file = file.as_str(), pipe_in, pipe_out, pipe_err, "spawned");
    handle.spawn(async move {
        if timeout_ms > 0 {
            tokio::time::sleep(Duration::from_millis(timeout_ms)).await;
            // 超时杀（组杀优先；进程已退则无操作，见 `child_kill` 幂等）。
            state::child_kill(id, "SIGKILL");
        }
        loop {
            if let Some(exit) = state::child_try_wait(id) {
                // 残留输出先落定再发 Exited（泵记数，见 child_pipes_flushed）。
                if state::child_pipes_flushed(id) {
                    let (status, signal) = status_parts(exit);
                    let _ = tx.send(ChildEvent { id, kind: ChildKind::Exited { status, signal } });
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    frame.set_rval(mozjs::jsval::Int32Value(id as i32));
    true
}

/// `__wjs_child_kill(id, signal)` → boolean（存活即作用）。
pub unsafe extern "C" fn child_kill(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 || !frame.arg(0).is_number() {
        report_error(&mut cx, "TypeError: kill needs a numeric id");
        return false;
    }
    let sig = if frame.argc() > 1 { value_to_string(&mut cx, frame.arg(1)) } else { "SIGTERM".into() };
    let ok = state::child_kill(frame.arg(0).to_number() as u64, &sig);
    frame.set_rval(mozjs::jsval::BooleanValue(ok));
    true
}

/// `__wjs_child_stdin_write(id, b64)` → boolean（入队即 true；子进程已走/非 pipe 即 false，
/// prelude 转 `ERR_STREAM_DESTROYED`）。
pub unsafe extern "C" fn child_stdin_write(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 || !frame.arg(0).is_number() || !frame.arg(1).is_string() {
        report_error(&mut cx, "TypeError: stdin write needs id and base64 data");
        return false;
    }
    use base64::Engine as _;
    let data = base64::engine::general_purpose::STANDARD
        .decode(value_to_string(&mut cx, frame.arg(1)))
        .unwrap_or_default();
    let ok = state::child_stdin_send(frame.arg(0).to_number() as u64, StdinCmd::Write(data));
    frame.set_rval(mozjs::jsval::BooleanValue(ok));
    true
}

/// `__wjs_child_stdin_close(id)`（幂等；写端关即子进程见 EOF）。
pub unsafe extern "C" fn child_stdin_close(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 || !frame.arg(0).is_number() {
        report_error(&mut cx, "TypeError: stdin close needs a numeric id");
        return false;
    }
    state::child_stdin_send(frame.arg(0).to_number() as u64, StdinCmd::Close);
    frame.set_rval(UndefinedValue());
    true
}

/// `__wjs_child_pid(id)` → pid｜-1。
pub unsafe extern "C" fn child_pid(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 仅读状态（无 cx 上的 JSAPI 调用）
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let _ = cx_raw;
    let pid = if frame.argc() > 0 && frame.arg(0).is_number() {
        state::with_plain(|p| {
            p.child_procs
                .get(&(frame.arg(0).to_number() as u64))
                .and_then(|e| e.child.id())
                .map(|id| id as i32)
                .unwrap_or(-1)
        })
    } else {
        -1
    };
    frame.set_rval(mozjs::jsval::Int32Value(pid));
    true
}

/// 事件循环分发一条子进程事件（读 target 的 `onexit/onclose/onerror`；终态摘除）。
/// exit 与 close 同事件双调（Node 紧随语义，此处同 tick，文档记录）。
/// pipe 块经 target 的 `__pushOut/__pushErr` 进流；Exited 先关流再调回调
///（块序在前由等待 task 的落定计数保证，见 spawn_start）。
/// 前置条件：cx 已进入 global 所属 realm（事件循环上下文）。
pub fn dispatch(
    cx: &mut mozjs::context::JSContext,
    global: *mut JSObject,
    ev: ChildEvent,
    err: crate::runtime::ErrorSource<'_>,
) -> Result<(), crate::error::Error> {
    use crate::jsapi_glue::{call_one, get_prop_value, parse_json};
    let failed = |cx: &mut mozjs::context::JSContext| match err {
        crate::runtime::ErrorSource::Script { source, filename } => {
            crate::jsapi_glue::pending_exception_error(cx, global, source, filename)
        }
        crate::runtime::ErrorSource::Module { url } => crate::modules::module_error(cx, url),
    };
    // target 缺失（已摘除）：残留事件落空（Exited 顺带记账）。
    let Some(target_v) = state::child_target(ev.id) else {
        if matches!(ev.kind, ChildKind::Exited { .. }) {
            state::child_remove(ev.id);
        }
        return Ok(());
    };
    if !target_v.is_object() {
        if matches!(ev.kind, ChildKind::Exited { .. }) {
            state::child_remove(ev.id);
        }
        return Ok(());
    }
    rooted!(&in(cx) let target_root: *mut JSObject = target_v.to_object());
    match &ev.kind {
        ChildKind::Stdout { data_b64 } => {
            push_pipe_chunk(cx, global, target_root.get(), c"__pushOut", data_b64)
        }
        ChildKind::Stderr { data_b64 } => {
            push_pipe_chunk(cx, global, target_root.get(), c"__pushErr", data_b64)
        }
        ChildKind::Exited { status, signal } => {
            // 先关流（残留块已送达），再走原 exit/close 双调
            push_pipe_close(cx, global, target_root.get());
            let json = serde_json::json!({ "status": status, "signal": signal }).to_string();
            let mut ok = true;
            // exit 与 close 同事件双调（`onerror` 永不触发：spawn 失败走同步抛错）。
            for name in [c"onexit", c"onclose"] {
                let Some(handler) = get_prop_value(cx, target_root.get(), name) else {
                    state::child_remove(ev.id);
                    return Err(failed(cx));
                };
                if handler.is_undefined() || handler.is_null() || !handler.is_object() {
                    continue;
                }
                let event_obj = match parse_json(cx, global, &json) {
                    Some(o) => o,
                    None => {
                        state::child_remove(ev.id);
                        return Err(failed(cx));
                    }
                };
                ok &= call_one(cx, global, handler, event_obj).is_some();
            }
            state::child_remove(ev.id);
            if ok { Ok(()) } else { Err(failed(cx)) }
        }
    }
}

/// pipe 块推进流（`__pushOut/__pushErr` 缺失即非 pipe，忽略；钩子抛错吞掉，
/// 流控制器抛错 prelude 侧已吞——块丢失好过事件循环炸）。
fn push_pipe_chunk(
    cx: &mut mozjs::context::JSContext,
    global: *mut JSObject,
    target: *mut JSObject,
    name: &std::ffi::CStr,
    data_b64: &str,
) -> Result<(), crate::error::Error> {
    use crate::jsapi_glue::{call_one, get_prop_value};
    let Some(hook) = get_prop_value(cx, target, name) else {
        return Ok(());
    };
    if hook.is_undefined() || hook.is_null() || !hook.is_object() {
        return Ok(());
    }
    rooted!(&in(cx) let mut v = UndefinedValue());
    data_b64.to_jsval(cx, v.handle_mut());
    let _ = call_one(cx, global, hook, v.get());
    Ok(())
}

/// 关 pipe 流（`__closeOut/__closeErr` 缺失即忽略）。
fn push_pipe_close(
    cx: &mut mozjs::context::JSContext,
    global: *mut JSObject,
    target: *mut JSObject,
) {
    use crate::jsapi_glue::{call_one, get_prop_value};
    for name in [c"__closeOut", c"__closeErr"] {
        let Some(hook) = get_prop_value(cx, target, name) else {
            continue;
        };
        if hook.is_undefined() || hook.is_null() || !hook.is_object() {
            continue;
        }
        rooted!(&in(cx) let v = UndefinedValue());
        let _ = call_one(cx, global, hook, v.get());
    }
}

/// `__wjs_cp_spawn(fileStr, argsJson, optsJson)` → 结果 JSON（同步版）。
/// `shell:true` 时经 `shlex::try_quote` 拼串（拼不出退单引号包裹，文档记录）。
pub unsafe extern "C" fn cp_spawn(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: spawn needs file and args");
        return false;
    }
    let file = value_to_string(&mut cx, frame.arg(0));
    let args_s = value_to_string(&mut cx, frame.arg(1));
    let args: Vec<String> = serde_json::from_str(&args_s).unwrap_or_default();
    if let Err(msg) = crate::permissions::check_run(&[file.clone(), args.first().cloned().unwrap_or_default()].join(" ")) {
        report_error(&mut cx, &msg);
        return false;
    }
    let Some(opts) = parse_opts(&mut cx, &frame, 2) else {
        return false;
    };
    let cmd = if opts.shell {
        // shell 拼串经 `shlex::try_quote`（含换行等拼不出时退单引号包裹，文档记录）。
        let quote = |a: &str| {
            shlex::try_quote(a)
                .map(|c| c.into_owned())
                .unwrap_or_else(|_| format!("'{}'", a.replace('\'', "'\\''")))
        };
        let mut c = std::process::Command::new(default_shell());
        let line = std::iter::once(file.clone())
            .chain(args.clone())
            .map(|a| quote(&a))
            .collect::<Vec<_>>()
            .join(" ");
        if cfg!(windows) {
            c.arg("/C").arg(&line);
        } else {
            c.arg("-c").arg(&line);
        }
        c
    } else {
        let mut c = std::process::Command::new(&file);
        c.args(&args);
        c
    };
    let input = opts
        .input_b64
        .as_deref()
        .map(|b| {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD.decode(b).unwrap_or_default()
        })
        .filter(|v| !v.is_empty());
    let out = run_command(cmd, &opts, input);
    set_rval_str(&mut cx, &frame, &out.to_string());
    true
}

/// 内嵌 ESM 源（`node:child_process`；同步子集，见头注）。
pub const SOURCE: &str = r#"
function __b64dec(s) {
  s = String(s).replace(/-/g, "+").replace(/_/g, "/");
  while (s.length % 4) s += "=";
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}
function __b64enc(u8) {
  let s = "";
  for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000));
  return btoa(s);
}
function __normExecOpts(opts) {
  const o = { encoding: "utf8", timeoutMs: 0, shell: true, maxBuffer: 1024 * 1024, inputB64: null };
  if (opts === undefined || opts === null) return o;
  if (typeof opts === "string") { o.encoding = opts; return o; }
  if (opts.encoding !== undefined) o.encoding = opts.encoding;
  if (opts.timeout !== undefined) o.timeoutMs = Number(opts.timeout);
  if (opts.shell !== undefined) o.shell = opts.shell;
  if (opts.maxBuffer !== undefined) o.maxBuffer = Number(opts.maxBuffer);
  if (opts.cwd !== undefined) o.cwd = String(opts.cwd);
  if (opts.env !== undefined) o.env = { ...opts.env };
  if (opts.input !== undefined && opts.input !== null) {
    const b = typeof opts.input === "string" ? new TextEncoder().encode(opts.input) : opts.input;
    o.inputB64 = __b64enc(b instanceof Uint8Array ? b : new Uint8Array(b));
  }
  return o;
}
function __normSpawnOpts(opts) {
  const o = { encoding: "utf8", timeoutMs: 0, shell: false, maxBuffer: 1024 * 1024, inputB64: null };
  if (opts === undefined || opts === null) return o;
  if (opts.encoding !== undefined) o.encoding = opts.encoding;
  if (opts.timeout !== undefined) o.timeoutMs = Number(opts.timeout);
  if (opts.shell !== undefined) o.shell = !!opts.shell;
  if (opts.maxBuffer !== undefined) o.maxBuffer = Number(opts.maxBuffer);
  if (opts.cwd !== undefined) o.cwd = String(opts.cwd);
  if (opts.env !== undefined) o.env = { ...opts.env };
  if (opts.input !== undefined && opts.input !== null) {
    const b = typeof opts.input === "string" ? new TextEncoder().encode(opts.input) : opts.input;
    o.inputB64 = __b64enc(b instanceof Uint8Array ? b : new Uint8Array(b));
  }
  return o;
}
function __toOut(b64, encoding) {
  const bytes = __b64dec(b64 || "");
  if (encoding === "buffer" || encoding === null) return bytes;
  return new TextDecoder(String(encoding || "utf8")).decode(bytes);
}
function __spawnError(cmd, r, encoding) {
  const msg = r.timedOut
    ? `Command failed: ${cmd}\nTimed out`
    : `Command failed: ${cmd}${r.status !== null ? ` (exit ${r.status})` : ""}`;
  const err = new Error(msg + (r.stderr_b64 ? "\n" + new TextDecoder().decode(__b64dec(r.stderr_b64)) : ""));
  err.status = r.status;
  err.signal = r.signal;
  err.stdout = __toOut(r.stdout_b64, encoding);
  err.stderr = __toOut(r.stderr_b64, encoding);
  if (r.spawnErr) {
    const code = (r.spawnErr.match(/^([A-Z_]+): /) || [])[1] || "UNKNOWN";
    err.code = code;
  }
  throw err;
}
export function execSync(cmd, opts) {
  const o = __normExecOpts(opts);
  const r = JSON.parse(__wjs_cp_exec(String(cmd), JSON.stringify({
    cwd: o.cwd ?? null, env: o.env ?? null, timeout_ms: o.timeoutMs,
    shell: !!o.shell, input_b64: o.inputB64, max_buffer: o.maxBuffer,
  })));
  if (r.spawnErr || r.timedOut || r.status !== 0) __spawnError(cmd, r, o.encoding);
  return __toOut(r.stdout_b64, o.encoding);
}
export function spawnSync(file, args, opts) {
  if (args !== undefined && !Array.isArray(args)) { opts = args; args = []; }
  const o = __normSpawnOpts(opts);
  const r = JSON.parse(__wjs_cp_spawn(String(file), JSON.stringify([...(args || [])].map(String)), JSON.stringify({
    cwd: o.cwd ?? null, env: o.env ?? null, timeout_ms: o.timeoutMs,
    shell: o.shell, input_b64: o.inputB64, max_buffer: o.maxBuffer,
  })));
  const out = {
    pid: r.pid,
    status: r.status,
    signal: r.signal,
    stdout: __toOut(r.stdout_b64, o.encoding),
    stderr: __toOut(r.stderr_b64, o.encoding),
  };
  if (r.spawnErr && !r.timedOut) {
    const code = (r.spawnErr.match(/^([A-Z_]+): /) || [])[1] || "UNKNOWN";
    out.error = Object.assign(new Error(`${code}: spawn ${file}`), { code, syscall: "spawn" });
  }
  if (r.timedOut && !out.error) {
    out.error = Object.assign(new Error(`Timed out: spawn ${file}`), { code: "ETIMEDOUT", syscall: "spawn" });
  }
  return out;
}
export class ChildProcess {
  #id = 0;
  #killed = false;
  #onexit = null;
  #onclose = null;
  #onerror = null;
  #onspawn = null;
  exitCode = null;
  signalCode = null;
  spawnfile = null;
  spawnargs = null;
  channel = null;
  __init(id, stdio) {
    this.#id = id;
    // pipe 口径：stdout/stderr 为 live ReadableStream（Rust 泵按块 enqueue，
    // exit 前残留必达，见 dispatch）；stdin 为 WritableStream（写失败即子进程已走）。
    const mkOut = (push, close) => {
      let ctl = null;
      const stream = new ReadableStream({ start(c) { ctl = c; }, cancel() {} });
      this[push] = (b64) => { try { ctl.enqueue(__b64dec(b64)); } catch {} };
      this[close] = () => { try { ctl.close(); } catch {} };
      return stream;
    };
    if (stdio[1] === "pipe") this.stdout = mkOut("__pushOut", "__closeOut");
    else this.stdout = null;
    if (stdio[2] === "pipe") this.stderr = mkOut("__pushErr", "__closeErr");
    else this.stderr = null;
    if (stdio[0] === "pipe") {
      const id2 = id;
      this.stdin = new WritableStream({
        write(chunk) {
          const b = typeof chunk === "string" ? new TextEncoder().encode(chunk) : chunk;
          const u8 = b instanceof Uint8Array ? b : new Uint8Array(b);
          if (!__wjs_child_stdin_write(id2, __b64enc(u8))) {
            throw Object.assign(new Error("ERR_STREAM_DESTROYED: stdin is closed"), { code: "ERR_STREAM_DESTROYED" });
          }
        },
        close() { __wjs_child_stdin_close(id2); },
      });
    } else this.stdin = null;
    return this;
  }
  get pid() { return __wjs_child_pid(this.#id); }
  get killed() { return this.#killed; }
  kill(signal) {
    const ok = __wjs_child_kill(this.#id, signal === undefined ? "SIGTERM" : String(signal));
    if (ok) this.#killed = true;
    return ok;
  }
  on(event, cb) {
    if (typeof cb !== "function") throw new TypeError("listener must be a function");
    if (event === "exit") this.onexit = cb;
    else if (event === "close") this.onclose = cb;
    else if (event === "error") this.onerror = cb;
    else if (event === "spawn") this.onspawn = cb;
    else if (event === "message" || event === "disconnect") {
      // 无 fd-passing 通道（记档缺口）：监听即明错，不静默吞
      throw Object.assign(new Error("ERR_NOT_SUPPORTED: child IPC channel not supported (no fork/send)"), { code: "ERR_NOT_SUPPORTED" });
    }
    else throw new Error(`NotSupportedError: ChildProcess event '${event}' (exit/close/error/spawn)`);
    return this;
  }
  // exit/close 经访问器 wrap：落定退出码（直接赋值亦生效，Node 的 exitCode 语义）
  set onexit(cb) {
    this.#onexit = (typeof cb === "function") ? ((ev) => {
      this.exitCode = ev && ev.status !== undefined ? ev.status : this.exitCode;
      this.signalCode = ev && ev.signal !== undefined ? ev.signal : this.signalCode;
      cb(ev);
    }) : cb;
  }
  get onexit() { return this.#onexit; }
  set onclose(cb) {
    this.#onclose = (typeof cb === "function") ? ((ev) => {
      if (this.exitCode === null) this.exitCode = ev && ev.status !== undefined ? ev.status : null;
      if (this.signalCode === null) this.signalCode = ev && ev.signal !== undefined ? ev.signal : null;
      cb(ev);
    }) : cb;
  }
  get onclose() { return this.#onclose; }
  set onerror(cb) { this.#onerror = cb; }
  get onerror() { return this.#onerror; }
  set onspawn(cb) { this.#onspawn = cb; }
  get onspawn() { return this.#onspawn; }
  send() {
    throw Object.assign(new Error("ERR_NOT_SUPPORTED: child send() needs an IPC channel (fork unsupported)"), { code: "ERR_NOT_SUPPORTED" });
  }
  disconnect() {
    throw Object.assign(new Error("ERR_NOT_SUPPORTED: child disconnect() needs an IPC channel (fork unsupported)"), { code: "ERR_NOT_SUPPORTED" });
  }
  get connected() { return false; }
  unref() { return this; }
  ref() { return this; }
}
function __normSpawnAsyncOpts(opts) {
  const o = { cwd: null, env: null, detached: false, stdio: ["inherit", "inherit", "inherit"], timeoutMs: 0 };
  if (opts === undefined || opts === null) return o;
  if (opts.cwd !== undefined) o.cwd = String(opts.cwd);
  if (opts.env !== undefined) o.env = { ...opts.env };
  if (opts.detached !== undefined) o.detached = !!opts.detached;
  if (opts.stdio !== undefined) {
    const one = (s) => {
      if (!["inherit", "ignore", "pipe"].includes(s)) {
        throw new Error(`NotSupportedError: spawn stdio '${s}' (inherit/ignore/pipe)`);
      }
      return s;
    };
    if (typeof opts.stdio === "string") o.stdio = [one(opts.stdio), one(opts.stdio), one(opts.stdio)];
    else if (Array.isArray(opts.stdio)) {
      // 三元数组（缺省补 inherit；Node 的复杂组合如 fd 重定向不在此列，文档记录）
      if (opts.stdio.length > 3) throw new Error("NotSupportedError: spawn stdio array takes at most 3 entries");
      o.stdio = [0, 1, 2].map((i) => opts.stdio[i] === undefined ? "inherit" : one(opts.stdio[i]));
    } else {
      throw new Error("NotSupportedError: spawn stdio must be a string or array");
    }
  }
  if (opts.timeout !== undefined) o.timeoutMs = Number(opts.timeout);
  return o;
}
export function spawn(file, args, opts) {
  if (args !== undefined && !Array.isArray(args)) { opts = args; args = []; }
  const o = __normSpawnAsyncOpts(opts);
  const proc = new ChildProcess();
  proc.spawnfile = String(file);
  proc.spawnargs = [...(args || [])].map(String);
  const id = __wjs_spawn_start(String(file), JSON.stringify([...(args || [])].map(String)), JSON.stringify({
    cwd: o.cwd, env: o.env, detached: o.detached, timeout_ms: o.timeoutMs,
  }), proc, JSON.stringify(o.stdio));
  return proc.__init(id, o.stdio);
}
function __asyncOneShot(kind, run) {
  // run(): 同步 core 调用（抛转回调 err）；无 live 句柄（记档偏差）
  return (...args) => {
    const cb = args.findLast((a) => typeof a === "function");
    const rest = args.filter((a) => typeof a !== "function");
    if (typeof cb !== "function") {
      const err = new TypeError(`${kind} requires a callback for async form`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    queueMicrotask(() => {
      try {
        const [outErr, stdout, stderr] = run(...rest);
        cb(outErr, stdout, stderr);
      } catch (e) {
        cb(e);
      }
    });
    return undefined;
  };
}
export const exec = __asyncOneShot("exec", (cmd, opts) => {
  const o = __normExecOpts(opts);
  const r = JSON.parse(__wjs_cp_exec(String(cmd), JSON.stringify({
    cwd: o.cwd ?? null, env: o.env ?? null, timeout_ms: o.timeoutMs,
    shell: !!o.shell, input_b64: o.inputB64, max_buffer: o.maxBuffer,
  })));
  if (r.spawnErr || r.timedOut || r.status !== 0) {
    try {
      __spawnError(cmd, r, o.encoding);
    } catch (e) {
      return [e, __toOut(r.stdout_b64, o.encoding), __toOut(r.stderr_b64, o.encoding)];
    }
  }
  return [null, __toOut(r.stdout_b64, o.encoding), __toOut(r.stderr_b64, o.encoding)];
});
export const execFile = __asyncOneShot("execFile", (file, args, opts) => {
  if (args !== undefined && !Array.isArray(args)) { opts = args; args = []; }
  const o = __normSpawnOpts(opts);
  const r = JSON.parse(__wjs_cp_spawn(String(file), JSON.stringify([...(args || [])].map(String)), JSON.stringify({
    cwd: o.cwd ?? null, env: o.env ?? null, timeout_ms: o.timeoutMs,
    shell: false, input_b64: o.inputB64, max_buffer: o.maxBuffer,
  })));
  if (r.spawnErr || r.timedOut || r.status !== 0) {
    try {
      __spawnError(file, r, o.encoding);
    } catch (e) {
      return [e, __toOut(r.stdout_b64, o.encoding), __toOut(r.stderr_b64, o.encoding)];
    }
  }
  return [null, __toOut(r.stdout_b64, o.encoding), __toOut(r.stderr_b64, o.encoding)];
});
export function execFileSync(file, args, opts) {
  if (args !== undefined && !Array.isArray(args)) { opts = args; args = []; }
  const o = __normSpawnOpts(opts);
  const r = JSON.parse(__wjs_cp_spawn(String(file), JSON.stringify([...(args || [])].map(String)), JSON.stringify({
    cwd: o.cwd ?? null, env: o.env ?? null, timeout_ms: o.timeoutMs,
    shell: false, input_b64: o.inputB64, max_buffer: o.maxBuffer,
  })));
  if (r.spawnErr || r.timedOut || r.status !== 0) __spawnError(file, r, o.encoding);
  return __toOut(r.stdout_b64, o.encoding);
}
export default { execSync, spawnSync, spawn, exec, execFile, execFileSync, ChildProcess };
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_parts_mapping() {
        // 仅平台无关断言（信号名走 unix 分支，CI 覆盖）。
        let _ = default_shell();
        let out = run_command(
            {
                let c = std::process::Command::new("definitely-missing-binary-xyz");
                c
            },
            &SpawnOpts::default(),
            None,
        );
        assert!(out["spawnErr"].as_str().unwrap_or("").contains("ENOENT"));
        assert!(out["status"].is_null());
    }
}
