//! `node:child_process` 同步子集 + 异步 `spawn`（plan Phase 4d/c-4x）。
//! 同步经 `std::process` 阻塞跑；异步经 `tokio::process` + 事件循环分发
//! `exit/close/error`；stdio 支持 `inherit`/`ignore`/`pipe`（字符串三同或三元数组）。
//! pipe 口径（10f 起 node 真语义）：stdout/stderr 为 legacy Readable 面
//!（`on('data'/'end'/'close'/'error')/once/off/setEncoding/pause/resume/
//! destroy`；data 缺省 Buffer，setEncoding 后为串——真机 `Readable`，
//! spawnPromisified 系套件口径；内部经 Web ReadableStream 泵接 Rust 块序，
//! 出块序 = 进程出序，exit 前必先送达残留数据再调 onexit/onclose）；
//! stdin 为 legacy Writable 面（Socket 写半部：write/end/writable/readable，
//! stdin 套件直调 write；web WritableStream 记偏差退役）；
//! 无 max_buffer 上限（流式消费）。
//! `detached:true` 在 unix 起 setsid 组长，kill 走组杀（`nix` 轮子；win 回退直杀）。
//! 结果走 JSON 桥（二进制 base64）；错误形状由 prelude 组装（见 SOURCE）。
//!
//! `fork`（M5 vitest 牵引）：worker 线程底座的进程形 fork（零新 native）。
//! 子会话跑目标模块（argv `[execPath, module, ...args]`，与真机同形），子端
//! `process.send/disconnect/on('message')/connected/channel` 经 parentPort 桥接；
//! 父端为 `ChildProcess`（`send/on('message')/disconnect/connected/kill` 全语义，
//! 关通道后 send 回 false + 异步 `ERR_IPC_CHANNEL_CLOSED`，真机口径）。
//! 偏差（记档）：同进程线程（无独立进程；cwd/execArgv/silent/stdio/
//! serialization/timeout/detached 接受忽略，stdio 恒 null；env 透传 worker
//! 快照，缺省继承）；子发消息无监听即丢
//! （EventEmitter 口径）；kill 信号值忽略（terminate 语义）；message/disconnect
//! 为单监听器位（spawn 路径 exit/close 同款风格）；控制信封单键对象
//! `{__wjs_fork_ctl:"disconnect"}` 不投递给用户。

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
    /// 超时毫秒（0/缺省=无限；超时发 killSignal，缺省 SIGTERM，真机口径）。
    timeout_ms: u64,
    /// detached 组长化（unix setsid；kill 走组杀，见 `make_detached`）。
    detached: bool,
    /// shell（exec 由调用方拼好；spawn 经 shlex 拼）。
    shell: bool,
    /// shell 路径（字符串 shell 用；None=默认 shell）。
    shell_path: Option<String>,
    /// argv0（unix arg0；None 不设）。
    argv0: Option<String>,
    /// 超时 kill 信号号（None=SIGTERM；JS 侧已按 os.signals 表校验归一）。
    kill_signo: Option<i32>,
    /// 超时 kill 信号名（上报用，与 kill_signo 同源；None=SIGTERM）。
    kill_signame: Option<String>,
    /// stdin 输入（base64；None=null）。
    input_b64: Option<String>,
    /// stdio 透传（inherit 即继承父端对应流，不捕获；缺省全 pipe）。
    /// ignore 与缺省同（null + 空回），仅此处显式区分以便将来校验。
    stdin_inherit: bool,
    stdout_inherit: bool,
    stderr_inherit: bool,
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
    // stdio 语义（input 优先于 stdin 透传；inherit 即继承父端流、不捕获；
    // 其余 pipe 捕获；ignore/null 空）。
    cmd.stdin(if input.is_some() {
        Stdio::piped()
    } else if opts.stdin_inherit {
        Stdio::inherit()
    } else {
        Stdio::null()
    });
    cmd.stdout(if opts.stdout_inherit {
        Stdio::inherit()
    } else {
        Stdio::piped()
    });
    cmd.stderr(if opts.stderr_inherit {
        Stdio::inherit()
    } else {
        Stdio::piped()
    });
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
                "pid": 0,
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
    // 输出泵线程（maxbuf 套件现形）：1MB+ 输出会撑满 64K 管道——若等退出
    // 后再读，子进程永阻塞、父进程永 try_wait，死锁。读与等必须并发。
    // inherit 流无 pipe 即无线程（空回）。
    let out_h = child.stdout.take().map(|mut out| {
        std::thread::spawn(move || {
            let mut v = Vec::new();
            let _ = out.read_to_end(&mut v);
            v
        })
    });
    let err_h = child.stderr.take().map(|mut err| {
        std::thread::spawn(move || {
            let mut v = Vec::new();
            let _ = err.read_to_end(&mut v);
            v
        })
    });
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
            // 超时杀：killSignal（缺省 SIGTERM，真机口径；detached 组杀同信号，
            // 组杀失败回退直杀）。直杀后阻塞 wait（忽略 SIGTERM 的子进程即等，
            // 与真机同）。
            let signo = opts.kill_signo.unwrap_or(15);
            #[cfg(unix)]
            if opts.detached {
                let pid = child.id();
                unsafe {
                    if libc::kill(-(pid as i32), signo) != 0 {
                        libc::kill(pid as i32, signo);
                    }
                }
            }
            #[cfg(unix)]
            if !opts.detached {
                unsafe {
                    libc::kill(child.id() as i32, signo);
                }
            }
            #[cfg(not(unix))]
            let _ = child.kill();
            let _ = child.wait();
            break true;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let stdout = out_h.map(|h| h.join().unwrap_or_default()).unwrap_or_default();
    let stderr = err_h.map(|h| h.join().unwrap_or_default()).unwrap_or_default();
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
        "signal": if timed_out { serde_json::Value::String(opts.kill_signame.clone().unwrap_or_else(|| "SIGTERM".into())) } else { signal.map(serde_json::Value::String).unwrap_or(serde_json::Value::Null) },
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
    /// `timed_out`：超时杀已作用（prelude 落 `killed` 位，exec timeout 系断言）。
    Exited { status: Option<i32>, signal: Option<String>, timed_out: bool },
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
    cmd.stdin(match stdio[0].as_str() {
        "ignore" => Stdio::null(),
        "pipe" => Stdio::piped(),
        _ => Stdio::inherit(),
    });
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
    let kill_signo = opts.kill_signo;
    state::child_add(id, child, detached, target, stdin_tx, pipes_expected);
    tracing::info!(target: "winterjs::child", id, file = file.as_str(), pipe_in, pipe_out, pipe_err, "spawned");
    handle.spawn(async move {
        if timeout_ms > 0 {
            tokio::time::sleep(Duration::from_millis(timeout_ms)).await;
            // 超时杀：killSignal（缺省 SIGTERM，真机口径；detached 组杀同信号，
            // 组杀失败回退直杀）。记号先置——Exited 事件据此带 timed_out。
            let signo = kill_signo.unwrap_or(15).to_string();
            state::child_mark_timeout(id);
            state::child_kill(id, &signo);
        }
        loop {
            if let Some(exit) = state::child_try_wait(id) {
                // 残留输出先落定再发 Exited（泵记数，见 child_pipes_flushed）。
                if state::child_pipes_flushed(id) {
                    let (status, signal) = status_parts(exit);
                    let timed_out = state::child_take_timed_out(id);
                    let _ = tx.send(ChildEvent { id, kind: ChildKind::Exited { status, signal, timed_out } });
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
    use crate::jsapi_glue::{call_one, call_two, get_prop_value, parse_json};
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
        ChildKind::Exited { status, signal, timed_out } => {
            // 超时杀先落 killed 位（exec timeout 系 err.killed=true 断言；
            // 顺序在 onexit/onclose 之前——collect 侧 close 处理器读 child.killed）。
            if *timed_out {
                if let Some(hook) = get_prop_value(cx, target_root.get(), c"__markKilled") {
                    if hook.is_object() {
                        let _ = call_one(cx, global, hook, UndefinedValue());
                    }
                }
            }
            // exit 后注销 abort 监听（spawn-timeout-kill-signal 套件
            // listenerCount(signal, 'abort') 归零断言；自有箭头属性 §4.34）。
            if let Some(hook) = get_prop_value(cx, target_root.get(), c"__onExited") {
                if hook.is_object() {
                    let _ = call_one(cx, global, hook, UndefinedValue());
                }
            }
            // 先关流（残留块已送达），再走原 exit/close 双调
            push_pipe_close(cx, global, target_root.get());
            // node 口径：exit/close 双参 (code, signal)——单对象形是旧偏差，
            // spawnPromisified 系解构 close(code, signal) 现形（10f url）。
            let status_json = serde_json::json!(status).to_string();
            let signal_json = serde_json::json!(signal).to_string();
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
                let Some(status_v) = parse_json(cx, global, &status_json) else {
                    state::child_remove(ev.id);
                    return Err(failed(cx));
                };
                let Some(signal_v) = parse_json(cx, global, &signal_json) else {
                    state::child_remove(ev.id);
                    return Err(failed(cx));
                };
                ok &= call_two(cx, global, handler, status_v, signal_v).is_some();
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
        let mut c = std::process::Command::new(opts.shell_path.as_deref().unwrap_or(default_shell()));
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
        #[cfg(unix)]
        if let Some(a0) = &opts.argv0 {
            use std::os::unix::process::CommandExt as _;
            c.arg0(a0);
        }
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
import { Worker } from "node:worker_threads";
import { pathToFileURL } from "node:url";
import * as fs from "node:fs";
import __osDefault from "node:os";
import { getSystemErrorName as __uvName } from "node:util";
import errors from 'node:internal/errors';
import { addAbortListener } from 'node:internal/events/abort_listener';
const {
  codes: {
    ERR_INVALID_ARG_TYPE: { HideStackFramesError: ERR_INVALID_ARG_TYPE },
    ERR_INVALID_ARG_VALUE: { HideStackFramesError: ERR_INVALID_ARG_VALUE },
    ERR_IPC_ONE_PIPE,
    ERR_INVALID_HANDLE_TYPE,
    ERR_MISSING_ARGS,
  },
} = errors;
const __SIGS = __osDefault.constants.signals;
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
  // node execFile 口径（v26 源码对拍）：{encoding:'utf8', ...opts} 展开合并——
  // opts 带 encoding 键（即便 undefined）即覆盖为 undefined → Buffer（实测
  // probe：{encoding:undefined} 走 Buffer，{} 走 utf8 串）；键缺席 → utf8 串。
  const o = { encoding: "utf8", timeoutMs: 0, shell: true, maxBuffer: 1024 * 1024, inputB64: null, killSignal: "SIGTERM", killSigno: 15 };
  if (opts === undefined || opts === null) { o.env = __childEnv(o); return o; }
  if (typeof opts === "string") { o.encoding = opts; return o; }
  if (opts !== null && typeof opts === "object" && "encoding" in opts) o.encoding = opts.encoding;
  if (opts.timeout !== undefined) o.timeoutMs = Number(opts.timeout);
  if (opts.shell !== undefined) { __nullCheck(opts.shell, "options.shell"); o.shell = opts.shell; }
  if (opts.maxBuffer !== undefined) o.maxBuffer = Number(opts.maxBuffer);
  if (opts.cwd !== undefined) { __nullCheck(String(opts.cwd), "options.cwd", "must be a string, Uint8Array, or URL without null bytes"); o.cwd = String(opts.cwd); }
  // argv0：校验后忽略（异步 exec 族 argv0 落地另案；reject-null-bytes 套件只断抛错）。
  if (opts.argv0 !== undefined && opts.argv0 !== null) {
    if (typeof opts.argv0 !== "string") throw new ERR_INVALID_ARG_TYPE("options.argv0", "string", opts.argv0);
    __nullCheck(opts.argv0, "options.argv0");
  }
  if (opts.env !== undefined) o.env = { ...opts.env };
  // killSignal：undefined/null/合法信号过；类型先行 ARG_TYPE，落空 UNKNOWN_SIGNAL
  //（sanitizeKillSignal 口径；exec timeout-kill 套件）。
  if (opts.killSignal !== undefined && opts.killSignal !== null) {
    const ks = opts.killSignal;
    if (typeof ks !== "string" && typeof ks !== "number") {
      throw new ERR_INVALID_ARG_TYPE("options.killSignal", ["string", "number"], ks);
    }
    const hit = __sigResolve(ks);
    if (!hit) {
      const e = new TypeError(`Unknown signal: ${String(ks)}`);
      e.code = "ERR_UNKNOWN_SIGNAL"; throw e;
    }
    o.killSignal = hit.name; o.killSigno = hit.signo;
  }
  // signal：undefined 过；余下必须 AbortSignal 实例（真机 validateAbortSignal 口径，
  // null 也抛——abortcontroller 系套件 {}/函数形 ARG_TYPE）。
  if (opts.signal !== undefined) {
    if (!(opts.signal instanceof AbortSignal)) {
      throw new ERR_INVALID_ARG_TYPE("options.signal", "AbortSignal", opts.signal);
    }
    o.signal = opts.signal;
  }
  if (opts.input !== undefined && opts.input !== null) {
    const b = opts.input;
    if (typeof b !== "string" && !(b instanceof Uint8Array) && !(b instanceof ArrayBuffer) && !ArrayBuffer.isView(b)) {
      throw new ERR_INVALID_ARG_TYPE("options.input", ["string", "Buffer", "TypedArray", "DataView", "ArrayBuffer"], b);
    }
    const u8 = typeof b === "string" ? new TextEncoder().encode(b)
      : (b instanceof Uint8Array ? b
        : (b instanceof ArrayBuffer ? new Uint8Array(b)
          : new Uint8Array(b.buffer, b.byteOffset, b.byteLength)));
    o.inputB64 = __b64enc(u8);
  }
  o.env = __childEnv(o);
  return o;
}
function __normSpawnOpts(opts) {
  // 缺省 encoding "buffer"（真机口径；exec 系另为 utf8，不串）。
  const o = { encoding: "buffer", timeoutMs: 0, shell: false, shellPath: null, maxBuffer: 1024 * 1024, inputB64: null, killSigno: 15, killSigname: "SIGTERM", argv0: null, cwd: null, detached: false, stdioInherit: [false, false, false] };
  if (opts === undefined || opts === null) { o.env = __childEnv(o); return o; }
  if (opts.encoding !== undefined) o.encoding = opts.encoding;
  // 字符串选项（cwd/argv0）：undefined/null 过，余下非串即 ARG_TYPE + \0 校验。
  for (const k of ["cwd", "argv0"]) {
    const v = opts[k];
    if (v === undefined || v === null) continue;
    if (typeof v !== "string") {
      throw new ERR_INVALID_ARG_TYPE(`options.${k}`, "string", v);
    }
    __nullCheck(v, `options.${k}`, k === "cwd" ? "must be a string, Uint8Array, or URL without null bytes" : undefined);
    o[k === "cwd" ? "cwd" : "argv0"] = v;
  }
  // 布尔选项（detached/windowsHide/windowsVerbatimArguments）：undefined/null/布尔过。
  // detached 真传 native；windows 系 unix 忽略（记档）。
  for (const k of ["detached", "windowsHide", "windowsVerbatimArguments"]) {
    const v = opts[k];
    if (v === undefined || v === null) continue;
    if (typeof v !== "boolean") {
      throw new ERR_INVALID_ARG_TYPE(`options.${k}`, "boolean", v);
    }
    if (k === "detached") o.detached = v;
  }
  // shell：undefined/null/布尔/字符串过（字符串即 shell 路径）+ \0 校验；
  // 余下 ARG_TYPE。
  if (opts.shell !== undefined && opts.shell !== null) {
    if (typeof opts.shell === "boolean") { o.shell = opts.shell; o.shellPath = null; }
    else if (typeof opts.shell === "string") { __nullCheck(opts.shell, "options.shell"); o.shell = true; o.shellPath = opts.shell; }
    else throw new ERR_INVALID_ARG_TYPE("options.shell", ["boolean", "string"], opts.shell);
  }
  // uid/gid：undefined/null/非负整数过（值忽略，记档）；非 number 即 ARG_TYPE，
  // 非整数/负数/NaN/Inf 即 RANGE。
  for (const k of ["uid", "gid"]) {
    const v = opts[k];
    if (v === undefined || v === null) continue;
    if (typeof v !== "number") throw new ERR_INVALID_ARG_TYPE(`options.${k}`, "number", v);
    if (!Number.isInteger(v) || v < 0) {
      const e = new RangeError(`The value of "options.${k}" is out of range. It must be a non-negative integer. Received ${v}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
  }
  // timeout：undefined/null/非负整数过；非 number 即 ARG_TYPE；
  // 负数/NaN/Inf/小数即 RANGE。
  if (opts.timeout !== undefined && opts.timeout !== null) {
    const v = opts.timeout;
    if (typeof v !== "number") throw new ERR_INVALID_ARG_TYPE("options.timeout", "number", v);
    if (!Number.isInteger(v) || v < 0) {
      const e = new RangeError(`The value of "options.timeout" is out of range. It must be a non-negative integer. Received ${v}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
    o.timeoutMs = v;
  }
  // maxBuffer：undefined/null/非负数/Infinity/小数过（Infinity 即不限）；
  // 非 number 即 ARG_TYPE；NaN/负数/-Inf 即 RANGE。小数下取整
  //（整数长度下与原值等价，Rust 侧 u64 收敛）。
  if (opts.maxBuffer !== undefined && opts.maxBuffer !== null) {
    const v = opts.maxBuffer;
    if (typeof v !== "number") throw new ERR_INVALID_ARG_TYPE("options.maxBuffer", "number", v);
    if (Number.isNaN(v) || v < 0) {
      const e = new RangeError(`The value of "options.maxBuffer" is out of range. It must be a non-negative number. Received ${v}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
    o.maxBuffer = Number.isFinite(v) ? Math.floor(v) : null;
  }
  // killSignal：undefined/null/合法信号（名大小写不敏感/表中数字）过；
  // 类型先行（布尔/数组/对象/函数即 ARG_TYPE，真机口径），
  // 再查表落空即 ERR_UNKNOWN_SIGNAL。
  if (opts.killSignal !== undefined && opts.killSignal !== null) {
    const ks = opts.killSignal;
    if (typeof ks !== "string" && typeof ks !== "number") {
      throw new ERR_INVALID_ARG_TYPE("options.killSignal", ["string", "number"], ks);
    }
    const hit = __sigResolve(ks);
    if (!hit) {
      const e = new TypeError(`Unknown signal: ${String(ks)}`);
      e.code = "ERR_UNKNOWN_SIGNAL"; throw e;
    }
    o.killSigno = hit.signo; o.killSigname = hit.name;
  }
  if (opts.env !== undefined) o.env = { ...opts.env };
  // stdio：字符串整体形（inherit/ignore/pipe）或三元数组（逐流；超界忽略）；
  // 余下收敛 pipe（校验面另案）。同步族只用透传标记（inherit 即不捕获）。
  o.stdioInherit = [false, false, false];
  const __stdioOne = (v) => v === "inherit" ? "inherit" : "pipe";
  if (opts.stdio !== undefined && opts.stdio !== null) {
    if (typeof opts.stdio === "string") {
      const m = __stdioOne(opts.stdio);
      o.stdioInherit = [m === "inherit", m === "inherit", m === "inherit"];
    } else if (Array.isArray(opts.stdio)) {
      for (let i = 0; i < 3; i++) {
        if (i < opts.stdio.length) o.stdioInherit[i] = __stdioOne(opts.stdio[i]) === "inherit";
      }
    }
  }
  if (opts.input !== undefined && opts.input !== null) {
    const b = opts.input;
    if (typeof b !== "string" && !(b instanceof Uint8Array) && !(b instanceof ArrayBuffer) && !ArrayBuffer.isView(b)) {
      throw new ERR_INVALID_ARG_TYPE("options.input", ["string", "Buffer", "TypedArray", "DataView", "ArrayBuffer"], b);
    }
    const u8 = typeof b === "string" ? new TextEncoder().encode(b)
      : (b instanceof Uint8Array ? b
        : (b instanceof ArrayBuffer ? new Uint8Array(b)
          : new Uint8Array(b.buffer, b.byteOffset, b.byteLength)));
    o.inputB64 = __b64enc(u8);
  }
  o.env = __childEnv(o);
  return o;
}
// 信号归一（os.signals 表单源）：名大小写不敏感、数字须命中表值；
// 命中回 {name, signo}，余下 null（含布尔/数组/对象/函数）。
function __sigResolve(v) {
  if (typeof v === "string") {
    const up = v.toUpperCase();
    for (const k of Object.keys(__SIGS)) if (k === up) return { name: k, signo: __SIGS[k] };
    return null;
  }
  if (typeof v === "number" && Number.isInteger(v)) {
    for (const k of Object.keys(__SIGS)) if (__SIGS[k] === v) return { name: k, signo: v };
  }
  return null;
}
// \0 校验（node validateArgumentNullCheck 口径：仅字符串含 \0 才抛；
// reason 缺省 'must be a string without null bytes'，cwd/modulePath 系另传）。
function __nullCheck(s, name, reason) {
  if (typeof s === "string" && s.includes("\0")) {
    throw new ERR_INVALID_ARG_VALUE(name, s, reason ?? "must be a string without null bytes");
  }
}
// 自举翻译（input/timeout/maxbuf 套件：子进程即自身时，Node 形 argv
// （`-e`/`-p` 脚本/裸文件）映射到本仓全 flag CLI；他家二进制原样透传。
// `-e` extras 透传（本仓 --eval 尾参作脚本 argv，最佳 effort）。
function __selfArgv(file, args) {
  if (file !== process.execPath) return [file, args];
  const a = [...args];
  if (a[0] === "-e" || a[0] === "-p") return [file, ["--eval", ...a.slice(1)]];
  if (a[0] !== undefined && !String(a[0]).startsWith("-")) return [file, ["--run", ...a]];
  return [file, a];
}
// 同步族错误 errno 表（真机实测；余下 -4094）。
function __syncErrno(code) {
  return { ENOENT: -2, EACCES: -13, ENOBUFS: -55, ETIMEDOUT: -60 }[code] ?? -4094;
}
function __toOut(b64, encoding) {
  const bytes = __b64dec(b64 || "");
  // 真机口径：encoding 缺省（spawnSync）即 "buffer"，回真 Buffer
  // （deepStrictEqual 裸 Uint8Array 即不等，原型不同）。
  if (encoding === "buffer" || encoding === null || encoding === undefined) return Buffer.from(bytes);
  return new TextDecoder(String(encoding)).decode(bytes);
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
    err.errno = __syncErrno(code);
  }
  throw err;
}
// shell 串首自举翻译（execsync-maxbuf / exec-encoding / exec-timeout 系套件）：
// `"<execPath>" -e/-p/-pe X` 经 shell 跑自身；`$NODE` token（env 透传）与
// `${VAR}`（node 测试 helper escapePOSIXShell 的 env 间接形，opts.env 解引用）
// 同理。仅串首二进制位 + 纯 [pe] 组合旗才改写为 `--eval`；**裸文件形**（首个
// 参数解引用后不以 `-` 开头）改写为 `--run <file>`（与 spawnSync 的 __selfArgv
// 同规则）；余下一律原样（误伤用户脚本更糟）。
function __selfCmd(cmd, env) {
  const s = String(cmd);
  const m = s.match(/^("[^"]*"|'[^']*'|\$NODE|\$\{[A-Za-z_][A-Za-z0-9_]*\}|\S+)\s*([\s\S]*)$/);
  if (!m) return cmd;
  const bin = m[1], rest = m[2] ?? "";
  const unq = (t) => ((t.startsWith('"') && t.endsWith('"')) || (t.startsWith("'") && t.endsWith("'"))) ? t.slice(1, -1) : t;
  const envGet = (t) => {
    const u = unq(t);
    const vm = /^\$\{([A-Za-z_][A-Za-z0-9_]*)\}$/.exec(u);
    if (vm && env && Object.prototype.hasOwnProperty.call(env, vm[1])) return String(env[vm[1]]);
    return u;
  };
  const selfBin = envGet(bin) === process.execPath || unq(bin) === "$NODE";
  if (!selfBin) return cmd;
  // 旗形（-e/-p/-pe …）——二 token 正则的 \s* 已吃掉分隔空白，此处用 \s*。
  const fm = rest.match(/^\s*-([A-Za-z]+)\s?([\s\S]*)$/);
  if (fm && /^[pe]+$/.test(fm[1])) return `${bin} --eval ${fm[2]}`;
  // 裸文件形（首个参数解引用后非旗）
  const am = rest.match(/^\s*("[^"]*"|'[^']*'|\$NODE|\$\{[A-Za-z_][A-Za-z0-9_]*\}|\S+)([\s\S]*)$/);
  if (am) {
    const a0 = envGet(am[1]);
    if (!a0.startsWith("-")) return `${bin} --run ${am[1]}${am[2] ?? ""}`;
  }
  return cmd;
}
// send 参数校验（node target.send/_send 口径，父子双侧共用；send-type-error
// 套件逐项：options 非对象即 ARG_TYPE；message 缺席 MISSING_ARGS、非
// string/object/number/boolean 即 ARG_TYPE；非空句柄无 fd 移交即
// INVALID_HANDLE_TYPE）。
function __validateSendOptions(options) {
  if (options !== undefined && (typeof options !== "object" || options === null)) {
    throw new ERR_INVALID_ARG_TYPE("options", "object", options);
  }
}
function __validateSendMessage(message, handle) {
  if (message === undefined) throw new ERR_MISSING_ARGS("message");
  if (typeof message !== "string" && typeof message !== "object" &&
      typeof message !== "number" && typeof message !== "boolean") {
    throw new ERR_INVALID_ARG_TYPE("message", ["string", "object", "number", "boolean"], message);
  }
  if (handle !== undefined && handle !== null) throw new ERR_INVALID_HANDLE_TYPE();
}
// node 口径（实测 probe）：abort 错 = Error 实例，name='AbortError'、
// code='ABORT_ERR'、cause=signal.reason（原生 abort() 为 DOMException AbortError）。
function __abortError(reason) {
  const err = new Error("This operation was aborted");
  err.name = "AbortError";
  err.code = "ABORT_ERR";
  err.cause = reason;
  return err;
}
export function execSync(cmd, opts) {
  const o = __normExecOpts(opts);
  __nullCheck(String(cmd), "command");
  // execSync 缺省 Buffer（真机实测；exec 异步缺省 utf8）：未显式给编码即改 buffer。
  if (typeof opts !== "string" && (opts === undefined || opts === null || opts.encoding === undefined)) o.encoding = "buffer";
  cmd = __selfCmd(String(cmd), o.env ?? process.env);
  const r = JSON.parse(__wjs_cp_exec(cmd, JSON.stringify({
    cwd: o.cwd ?? null, env: o.env ?? null, timeout_ms: o.timeoutMs,
    shell: !!o.shell, input_b64: o.inputB64, max_buffer: o.maxBuffer,
  })));
  if (r.spawnErr || r.timedOut || r.status !== 0) __spawnError(cmd, r, o.encoding);
  return __toOut(r.stdout_b64, o.encoding);
}
export function spawnSync(file, args, opts) {
  if (args !== undefined && args !== null && !Array.isArray(args)) { opts = args; args = []; }
  const o = __normSpawnOpts(opts);
  __nullCheck(String(file), "file");
  for (let i = 0; i < (args || []).length; i++) __nullCheck(String(args[i]), `args[${i}]`);
  const f = String(file);
  const origArgs = [...(args || [])].map(String);
  const [f2, a2] = __selfArgv(f, origArgs);
  const r = JSON.parse(__wjs_cp_spawn(f2, JSON.stringify(a2), JSON.stringify({
    cwd: o.cwd ?? null, env: o.env ?? null, timeout_ms: o.timeoutMs,
    shell: o.shell, shell_path: o.shellPath, kill_signo: o.killSigno,
    kill_signame: o.killSigname, argv0: o.argv0, detached: !!o.detached,
    stdin_inherit: !!o.stdioInherit[0], stdout_inherit: !!o.stdioInherit[1],
    stderr_inherit: !!o.stdioInherit[2],
    input_b64: o.inputB64, max_buffer: o.maxBuffer,
  })));
  const stdout = __toOut(r.stdout_b64, o.encoding);
  const stderr = __toOut(r.stderr_b64, o.encoding);
  // 真机口径（实测）：成功 output=[null, stdout, stderr]，失败 output=null；
  // error 挂 code/errno/syscall/path（message 非枚举）.
  const out = {
    pid: r.pid,
    output: null,
    stdout, stderr,
    status: r.status,
    signal: r.timedOut ? o.killSigname : r.signal,
  };
  if (!r.spawnErr && !r.timedOut) out.output = [null, stdout, stderr];
  if (r.spawnErr && !r.timedOut) {
    const code = (r.spawnErr.match(/^([A-Z_]+): /) || [])[1] || "UNKNOWN";
    out.error = new Error(`spawnSync ${f} ${code}`);
    out.error.code = code;
    out.error.errno = __syncErrno(code);
    out.error.syscall = `spawnSync ${f}`;
    out.error.path = f;
    // 真机口径（spawnsync.js 点名）：spawnargs 为参数数组（不含 file 本体）。
    out.error.spawnargs = origArgs;
  }
  if (r.timedOut && !out.error) {
    out.error = new Error(`spawnSync ${f} ETIMEDOUT`);
    out.error.code = "ETIMEDOUT";
    out.error.errno = -60;
    out.error.syscall = `spawnSync ${f}`;
    out.error.path = f;
    out.error.spawnargs = origArgs;
  }
  return out;
}
// legacy Readable 面（node 真机口径）：Web ReadableStream 外壳，供
// spawnPromisified 系套件（setEncoding + on('data')）与 CLI 自省使用。
// data 缺省 Buffer（§4.83），setEncoding 后为串；'data' 挂载即流动泵。
function __legacyReadable(web) {
  const listeners = {};
  let flowing = false;
  let paused = false;
  let ended = false;
  let destroyed = false;
  let enc = null;
  let reader = null;
  // paused 读缓冲（flush-stdio 套件：on('readable') + read() 循环；flowing 期
  // read() 恒 null，数据走 'data'）。
  let buf = [];
  let pumping = false;
  let pipes = null;
  const emit = (ev, ...args) => {
    for (const l of [...(listeners[ev] || [])]) {
      try { l(...args); } catch {}
    }
  };
  async function pump() {
    if (reader === null) reader = web.getReader();
    if (pumping) return;
    pumping = true;
    try {
      for (;;) {
        while (buf.length > 0 && flowing && !paused && !destroyed) emit("data", buf.shift());
        if (destroyed) return;
        let r;
        try { r = await reader.read(); } catch (e) { emit("error", e); return; }
        if (r.done) {
          while (buf.length > 0 && flowing && !paused && !destroyed) emit("data", buf.shift());
          ended = true;
          emit("end");
          // close 递延一轮（真机先 end 后 close 异步序；迟挂 close 仍到。
          // destroy() 的同步 close 保持——用户主动销毁即时语义）。
          queueMicrotask(() => emit("close"));
          if (!destroyed && !flowing && buf.length > 0) emit("readable");
          return;
        }
        let chunk = Buffer.from(r.value);
        if (enc !== null) chunk = chunk.toString(enc);
        buf.push(chunk);
        if (flowing && !paused && !destroyed) {
          while (buf.length > 0 && flowing && !paused && !destroyed) emit("data", buf.shift());
        } else if (!destroyed) {
          emit("readable");
        }
      }
    } finally { pumping = false; }
  }
  const api = {
    on(ev, cb) {
      (listeners[ev] ||= []).push(cb);
      if (ev === "data") { flowing = true; pump(); }
      else if (ev === "readable" || ev === "end" || ev === "close") {
        // 'end'/'close' 单监听也要泵（kill 套件：只挂 end 即要 EOF；
        // 非 flowing，不改数据流向，只观测终止）。
        pump();
      }
      return api;
    },
    once(ev, cb) {
      const w = (...a) => { api.off(ev, w); cb(...a); };
      return api.on(ev, w);
    },
    off(ev, cb) {
      const l = listeners[ev];
      if (l) { const i = l.indexOf(cb); if (i !== -1) l.splice(i, 1); }
      return api;
    },
    removeListener(ev, cb) { return api.off(ev, cb); },
    removeAllListeners(ev) {
      if (ev !== undefined) delete listeners[ev];
      else for (const k of Object.keys(listeners)) delete listeners[k];
      return api;
    },
    setEncoding(e) { enc = e === null ? null : String(e); return api; },
    pause() { paused = true; return api; },
    resume() { paused = false; flowing = true; pump(); return api; },
    // 最小 pipe 面（stdio-inherit 套件 `child.stderr.pipe(process.stderr)`；
    // 数据经 write 透传，end 默认透传）。
    pipe(dest, opts) {
      const onData = (d) => { try { dest.write(d); } catch {} };
      const onEnd = () => { try { if (!opts || opts.end !== false) dest.end(); } catch {} };
      api.on("data", onData);
      api.on("end", onEnd);
      ((pipes ||= [])).push({ dest, onData, onEnd });
      return dest;
    },
    unpipe(dest) {
      if (!pipes) return api;
      const rest = [];
      for (const p of pipes) {
        if (dest !== undefined && p.dest !== dest) { rest.push(p); continue; }
        api.off("data", p.onData);
        api.off("end", p.onEnd);
      }
      pipes = rest;
      return api;
    },
    destroy() {
      if (destroyed) return api;
      destroyed = true;
      try { if (reader !== null) reader.cancel(); } catch {}
      emit("close");
      return api;
    },
    // paused 读（flowing 期恒 null；暂停期取缓冲，空即 null）。
    read() {
      pump();
      if (flowing && !paused) return null;
      if (buf.length > 0) return buf.shift();
      return null;
    },
    get destroyed() { return destroyed; },
    // execFile collect 的串化判定读此位（node 流同形；setEncoding 后即真）。
    get readableEncoding() { return enc; },
    // 兼容桩（pipe-dataflow 套件直改 `stdout._handle.readStart` 断言永不调用；
    // 本泵模型不经过 readStart，桩恒静默）。
    _handle: { readStart() {}, readStop() {} },
  };
  return api;
}

// legacy Writable 面（node stdin 真机口径：Socket 形——stdin 套件直调
// `cat.stdin.write('hello')`/`.end()` 并断 writable/readable 位；web
// WritableStream 无这些成员）。write 回调恒异步触发（§4.74）。
function __legacyWritable(id) {
  const listeners = {};
  let destroyed = false;
  let ended = false;
  let buffered = 0;
  const emit = (ev, ...args) => {
    for (const l of [...(listeners[ev] || [])]) {
      try { l(...args); } catch {}
    }
  };
  const api = {
    on(ev, cb) { (listeners[ev] ||= []).push(cb); return api; },
    once(ev, cb) {
      const w = (...a) => { api.off(ev, w); cb(...a); };
      w.__wjs_orig = cb;
      return api.on(ev, w);
    },
    off(ev, cb) {
      const l = listeners[ev];
      if (l) {
        let i = l.findIndex((f) => f === cb || f.__wjs_orig === cb);
        while (i >= 0) { l.splice(i, 1); i = l.findIndex((f) => f === cb || f.__wjs_orig === cb); }
      }
      return api;
    },
    removeListener(ev, cb) { return api.off(ev, cb); },
    removeAllListeners(ev) {
      if (ev !== undefined) delete listeners[ev];
      else for (const k of Object.keys(listeners)) delete listeners[k];
      return api;
    },
    write(chunk, cb) {
      if (destroyed || ended) {
        const err = Object.assign(new Error("write after end"), { code: "ERR_STREAM_WRITE_AFTER_END" });
        if (typeof cb === "function") queueMicrotask(() => cb(err));
        else emit("error", err);
        return false;
      }
      const u8 = typeof chunk === "string" ? new TextEncoder().encode(chunk)
        : (chunk instanceof Uint8Array ? chunk : new Uint8Array(chunk?.buffer ?? chunk));
      const ok = __wjs_child_stdin_write(id, __b64enc(u8));
      if (!ok) {
        const err = Object.assign(new Error("This socket has been ended by the other party"), { code: "EPIPE" });
        if (typeof cb === "function") queueMicrotask(() => cb(err));
        else emit("error", err);
        return false;
      }
      if (typeof cb === "function") queueMicrotask(() => cb(null));
      // 背压（big-write-end 套件：恒 true 即 `while(write)` 死循环）：
      // 16KB 高水位（Socket 缺省），超即 false + 下轮记账清零发 drain。
      // task 侧无写完成回执，“入队即走”近似——投递保序不受记账影响
      // （unbounded 通道 FIFO，end 关排在写后）。
      buffered += u8.length;
      if (buffered > 16384) {
        queueMicrotask(() => { buffered = 0; emit("drain"); });
        return false;
      }
      return true;
    },
    end(chunk, cb) {
      if (chunk !== undefined && chunk !== null) api.write(chunk);
      if (ended) return api;
      ended = true;
      __wjs_child_stdin_close(id);
      queueMicrotask(() => emit("finish"));
      if (typeof cb === "function") queueMicrotask(() => cb());
      return api;
    },
    destroy() {
      if (destroyed) return api;
      destroyed = true;
      __wjs_child_stdin_close(id);
      emit("close");
      return api;
    },
    get writable() { return !destroyed && !ended; },
    get writableEnded() { return ended; },
    get destroyed() { return destroyed; },
    // stdin 是 Socket（Duplex）的写半部：readable 恒 false（stdin 套件点名）。
    get readable() { return false; },
  };
  return api;
}
export class ChildProcess {
  #id = 0;
  #killed = false;
  #onexit = null;
  #onclose = null;
  #onerror = null;
  #onspawn = null;
  // 多监听列表（on 累积/off 摘除；单分发位经 __install 落 fan-out）。
  #exitL = [];
  #closeL = [];
  // close 到达记录（流 end 后迟挂 close 即时重放，见 on；真机 exit→stdio 关→
  // close 异步序在本仓同派发内完成，记录补迟挂一拍）。
  #closeArgs = null;
  #errorL = [];
  #spawnL = [];
  #msgL = [];
  #internalL = [];
  #discL = [];
  #onmessage = null;
  #ondisconnect = null;
  #oninternal = null;
  exitCode = null;
  signalCode = null;
  spawnfile = null;
  spawnargs = null;
  channel = null;
  // fork 面（worker 线程底座，见文末 `fork`）：置位后 message/disconnect/
  // send/disconnect/kill/unref 走线程通道；spawn 子进程保持原语义。
  __forkChild = false;
  __worker = null;
  __connected = false;
  __exitCode = null;
  // fork 出口单发旗（abort 路径自 synth exit 后，线程 WExit 被拦）。
  __exitDone = false;
  __killSignal = "SIGTERM";
  __init(id, stdio) {
    this.#id = id;
    this.__initStreams(stdio, id);
    // close 记录常驻（迟挂重放：到达即记，不等首监听）。
    this.__install("close");
    // node 口径：spawn 成功后 pid 为自有数据属性（hasOwn true）；
    // 未成功（id=0）保持原型 getter 的 undefined。
    if (id !== 0) {
      try { Object.defineProperty(this, "pid", { value: __wjs_child_pid(id), writable: true, configurable: true, enumerable: true }); } catch {}
    }
    return this;
  }
  // 流初始化（正常 spawn 与死句柄路径共用；id=0 即死句柄——native 查表恒失败，
  // 写返回 false。cwd 套件错误路径仍要 child.stdout.setEncoding 可用）。
  __initStreams(stdio, id) {
    const id2 = id;
    // 超时杀落位（Rust dispatch 在 Exited 前调）：自有箭头属性而非原型方法——
    // dispatch 以 global 为 this 调钩子（§4.34），#killed 须走闭包捕获。
    this.__markKilled = () => { this.#killed = true; };
    // pipe 口径：stdout/stderr 为 live ReadableStream（Rust 泵按块 enqueue，
    // exit 前残留必达，见 dispatch）；stdin 为 WritableStream（写失败即子进程已走）。
    const mkOut = (push, close) => {
      let ctl = null;
      const stream = new ReadableStream({ start(c) { ctl = c; }, cancel() {} });
      this[push] = (b64) => { try { ctl.enqueue(__b64dec(b64)); } catch {} };
      this[close] = () => { try { ctl.close(); } catch {} };
      return __legacyReadable(stream);
    };
    if (stdio[1] === "pipe") this.stdout = mkOut("__pushOut", "__closeOut");
    else this.stdout = null;
    if (stdio[2] === "pipe") this.stderr = mkOut("__pushErr", "__closeErr");
    else this.stderr = null;
    if (stdio[0] === "pipe") {
      this.stdin = __legacyWritable(id2);
    } else this.stdin = null;
    // node 口径：stdio 数组恒在（与 stdin/stdout/stderr 同一对象；spawn-error
    //套件在 ENOENT 路径亦断言）。
    this.stdio = [this.stdin, this.stdout, this.stderr];
    return this;
  }
  // error 事件（spawn 预检失败/abort；无监听即抛——node 'error' 语义）。
  __emitError(err) {
    if (typeof this.onerror === "function") this.onerror(err);
    else throw err;
  }
  __emitAbort(reason) { this.__emitError(__abortError(reason)); }
  // 死句柄 close（spawn 预检失败：close(-errno, null)；exit 不发——真机 probe）。
  __emitDeadClose(errno) {
    if (typeof this.onclose === "function") this.onclose(errno ?? -2, null);
  }
  // node 口径：spawn 未成功（#id=0 占位）pid 恒 undefined（execFile ENOENT
  // 套件 typeof 点名；真机 ChildProcess 在 spawn 成功前根本无 pid 属性）
  get pid() { return this.#id === 0 ? undefined : __wjs_child_pid(this.#id); }
  get killed() { return this.#killed; }
  kill(signal) {
    if (this.__forkChild) {
      if (this.__exitCode !== null) return false;
      this.#killed = true;
      try { this.__worker.terminate(); } catch { return false; }
      return true;
    }
    // 信号名经 os.signals 表归一为数字；未知信号抛 ERR_UNKNOWN_SIGNAL
    //（真机 convertToValidSignal 口径；0 为存在性检查直接透传）。
    let sig = "15";
    if (signal !== undefined) {
      if (signal === 0) sig = "0";
      else {
        const hit = __sigResolve(signal);
        if (!hit) {
          const e = new TypeError(`Unknown signal: ${String(signal)}`);
          e.code = "ERR_UNKNOWN_SIGNAL"; throw e;
        }
        sig = String(hit.signo);
      }
    }
    const ok = __wjs_child_kill(this.#id, sig);
    if (ok) this.#killed = true;
    return ok;
  }
  // node internal/child_process.js ChildProcess.prototype.spawn 逐字口径：
  // validateObject(options) → stdio 归一（含 ipc 检出）→ 有 ipc 才验 envPairs →
  //验 file（string）→ 验 args（array）→ 起进程。校验序即语义（constructor 套件
  //逐块点名）。起进程段复用模块级 spawn(file, args, opts) 的归一/预检/自举/
  //native 落地（stdio 传复合形态时走 __spawnInto）。
  spawn(options) {
    if (typeof options !== "object" || options === null) {
      throw new ERR_INVALID_ARG_TYPE("options", "object", options);
    }
    // stdio 归一：string/array 均可；4 元 ipc 形保留（envPairs 校验用）。
    let stdioOpt = options.stdio !== undefined ? options.stdio : "pipe";
    let hasIpc = false;
    if (typeof stdioOpt === "string") {
      if (stdioOpt === "ipc") hasIpc = true;
    } else if (Array.isArray(stdioOpt)) {
      hasIpc = stdioOpt.includes("ipc");
    } else {
      throw new ERR_INVALID_ARG_VALUE("stdio", stdioOpt);
    }
    if (hasIpc) {
      if (options.envPairs !== undefined) {
        if (!Array.isArray(options.envPairs)) {
          throw new ERR_INVALID_ARG_TYPE("options.envPairs", "Array", options.envPairs);
        }
      }
    }
    if (typeof options.file !== "string") {
      throw new ERR_INVALID_ARG_TYPE("options.file", "string", options.file);
    }
    let args;
    if (options.args === undefined) args = [];
    else {
      if (!Array.isArray(options.args)) {
        throw new ERR_INVALID_ARG_TYPE("options.args", "Array", options.args);
      }
      args = options.args;
    }
    // 落地：与模块级 spawn 同一道 __spawnInto（proc=this，事件接线直挂 this）。
    __spawnInto(this, options.file, args, __normSpawnAsyncOpts({
      cwd: options.cwd, detached: options.detached, stdio: stdioOpt,
      shell: options.shell, uid: options.uid, gid: options.gid,
      windowsHide: options.windowsHide,
      windowsVerbatimArguments: options.windowsVerbatimArguments,
    }));
    return 0;
  }
  __idOf() { return this.#id; }
  // node 口径：显式资源管理（`using cat = spawn(...)`）——dispose 即 kill()
  //（destroy 套件；asyncDispose 同步落定）。
  [Symbol.dispose]() { try { this.kill(); } catch {} }
  [Symbol.asyncDispose]() { try { this.kill(); } catch {} return Promise.resolve(); }
  // node 口径：起进程成功后 nextTick 发 'spawn'（onSpawnNT）——早于任何
  // data/exit/close 分发（spawn-event 套件 didSpawn 门）。
  __emitSpawn() {
    if (typeof this.onspawn === "function") {
      try { this.onspawn(); } catch (e) { this.__emitError(e); }
    }
  }
  on(event, cb) {
    if (typeof cb !== "function") throw new TypeError("listener must be a function");
    // node 口径：同事件多监听并存（spawn-event 套件挂两个 'spawn'；旧单槽
    //实现后挂顶掉先挂，didSpawn 永 false）。列表累积 + fan-out 落分发位。
    if (event === "exit") { this.#exitL.push(cb); this.__install("exit"); }
    else if (event === "close") {
      this.#closeL.push(cb); this.__install("close");
      // close 已到后迟挂即时重放（流 end 后挂 close 形；once 包裹亦经此路）。
      if (this.#closeArgs !== null) {
        const self = this;
        queueMicrotask(() => {
          if (self.#closeL.includes(cb)) { try { cb(...self.#closeArgs); } catch {} }
        });
      }
    }
    else if (event === "error") { this.#errorL.push(cb); this.__install("error"); }
    else if (event === "spawn") { this.#spawnL.push(cb); this.__install("spawn"); }
    else if (event === "message" || event === "disconnect" || event === "internalMessage") {
      if (!this.__forkChild) {
        // spawn 子进程无 fd-passing 通道（记档缺口）：监听即明错，不静默吞
        throw Object.assign(new Error("ERR_NOT_SUPPORTED: child IPC channel not supported (use fork)"), { code: "ERR_NOT_SUPPORTED" });
      }
      if (event === "message") { this.#msgL.push(cb); this.__install("message"); }
      else if (event === "internalMessage") { this.#internalL.push(cb); this.__install("internalMessage"); }
      else { this.#discL.push(cb); this.__install("disconnect"); }
    }
    else throw new Error(`NotSupportedError: ChildProcess event '${event}' (exit/close/error/spawn/message/disconnect/internalMessage)`);
    return this;
  }
  // 监听列表扇出到单分发位（exit/close 走访问器 wrap 落码；空表即摘除）。
  __install(event) {
    if (event === "exit") {
      const ls = [...this.#exitL];
      this.onexit = ls.length ? ((code, signal) => { for (const fn of ls) fn(code, signal); }) : null;
    } else if (event === "close") {
      const self = this;
      const ls = [...this.#closeL];
      // 常驻记录（空表亦装：无监听到达仍记，供迟挂重放；同步扇出时序不动）。
      this.onclose = ((code, signal) => {
        self.#closeArgs = [code, signal];
        for (const fn of ls) fn(code, signal);
      });
    } else if (event === "error") {
      const ls = [...this.#errorL];
      this.onerror = ls.length ? ((...a) => { for (const fn of ls) fn(...a); }) : null;
    } else if (event === "spawn") {
      const ls = [...this.#spawnL];
      this.onspawn = ls.length ? (() => { for (const fn of ls) fn(); }) : null;
    } else if (event === "message") {
      const ls = [...this.#msgL];
      this.#onmessage = ls.length ? ((m) => { for (const fn of ls) fn(m); }) : null;
    } else if (event === "internalMessage") {
      const ls = [...this.#internalL];
      this.#oninternal = ls.length ? ((m) => { for (const fn of ls) fn(m); }) : null;
    } else if (event === "disconnect") {
      const ls = [...this.#discL];
      this.#ondisconnect = ls.length ? (() => { for (const fn of ls) fn(); }) : null;
    }
  }
  once(event, cb) {
    if (typeof cb !== "function") throw new TypeError("listener must be a function");
    const self = this;
    const wrapped = (...args) => { self.off(event, wrapped); cb(...args); };
    wrapped.__wjs_orig = cb;
    return this.on(event, wrapped);
  }
  off(event, cb) {
    const match = (fn) => fn === cb || (typeof fn === "function" && fn.__wjs_orig === cb);
    const drop = (ls) => { const i = ls.findIndex(match); if (i >= 0) ls.splice(i, 1); };
    if (event === "exit") { drop(this.#exitL); this.__install("exit"); }
    else if (event === "close") { drop(this.#closeL); this.__install("close"); }
    else if (event === "error") { drop(this.#errorL); this.__install("error"); }
    else if (event === "spawn") { drop(this.#spawnL); this.__install("spawn"); }
    else if (event === "message") { drop(this.#msgL); this.__install("message"); }
    else if (event === "internalMessage") { drop(this.#internalL); this.__install("internalMessage"); }
    else if (event === "disconnect") { drop(this.#discL); this.__install("disconnect"); }
    return this;
  }
  removeListener(event, cb) { return this.off(event, cb); }
  // node 口径（kill-sigwinch 套件）：清指定事件（缺省全清）监听。
  removeAllListeners(event) {
    if (event === undefined) {
      for (const e of ["exit", "close", "error", "spawn", "message", "disconnect", "internalMessage"]) this.__clearAll(e);
    } else {
      if (!["exit", "close", "error", "spawn", "message", "disconnect", "internalMessage"].includes(event)) {
        throw new Error(`NotSupportedError: ChildProcess event '${event}' (exit/close/error/spawn/message/disconnect/internalMessage)`);
      }
      this.__clearAll(event);
    }
    return this;
  }
  __clearAll(event) {
    if (event === "exit") { this.#exitL.length = 0; this.__install("exit"); }
    else if (event === "close") { this.#closeL.length = 0; this.__install("close"); }
    else if (event === "error") { this.#errorL.length = 0; this.__install("error"); }
    else if (event === "spawn") { this.#spawnL.length = 0; this.__install("spawn"); }
    else if (event === "message") { this.#msgL.length = 0; this.__install("message"); }
    else if (event === "internalMessage") { this.#internalL.length = 0; this.__install("internalMessage"); }
    else if (event === "disconnect") { this.#discL.length = 0; this.__install("disconnect"); }
  }
  // 手动派发（execfile 套件直调 child.emit('close', …)；真机 EventEmitter 口径，
  // 走访问器 wrap 以便 exitCode/signalCode 落定）。
  emit(event, ...args) {
    if (event === "exit" && typeof this.onexit === "function") { this.onexit(...args); return true; }
    if (event === "close" && typeof this.onclose === "function") { this.onclose(...args); return true; }
    if (event === "error" && typeof this.onerror === "function") { this.onerror(...args); return true; }
    if (event === "spawn" && typeof this.onspawn === "function") { this.onspawn(...args); return true; }
    return false;
  }
  // exit/close 经访问器 wrap：落定退出码（直接赋值亦生效，Node 的 exitCode 语义）。
  // node 口径：回调双参 (code, signal)；null/undefined 的位不动（exit 用旧值，
  // close 用 null——真机 close 在 signal 死亡时 exitCode 仍 null）。
  set onexit(cb) {
    this.#onexit = (typeof cb === "function") ? ((code, signal) => {
      if (code !== undefined && code !== null) this.exitCode = code;
      if (signal !== undefined && signal !== null) this.signalCode = signal;
      cb(code, signal);
    }) : cb;
  }
  get onexit() { return this.#onexit; }
  set onclose(cb) {
    this.#onclose = (typeof cb === "function") ? ((code, signal) => {
      if (this.exitCode === null && code !== undefined && code !== null) this.exitCode = code;
      if (this.signalCode === null && signal !== undefined && signal !== null) this.signalCode = signal;
      cb(code, signal);
    }) : cb;
  }
  get onclose() { return this.#onclose; }
  set onerror(cb) { this.#onerror = cb; }
  get onerror() { return this.#onerror; }
  set onspawn(cb) { this.#onspawn = cb; }
  get onspawn() { return this.#onspawn; }
  send(message, ...rest) {
    if (!this.__forkChild) {
      throw Object.assign(new Error("ERR_NOT_SUPPORTED: child send() needs an IPC channel (use fork)"), { code: "ERR_NOT_SUPPORTED" });
    }
    // node target.send 口径（send-type-error 套件）：函数位移 + options 对象
    // 校验先行（连接态无关）；message/句柄校验随后。
    let handle, options, cb = null;
    const a = [...rest];
    if (a.length > 0 && typeof a[0] === "function") { cb = a.shift(); }
    else {
      handle = a.shift();
      if (a.length > 0 && typeof a[0] === "function") { cb = a.shift(); }
      else {
        options = a.shift();
        if (a.length > 0 && typeof a[0] === "function") { cb = a.shift(); }
      }
    }
    if (options !== undefined) __validateSendOptions(options);
    if (!this.__connected) {
      // Node 口径：关通道后 send 回 false，并异步报 ERR_IPC_CHANNEL_CLOSED。
      const err = new Error("Channel closed");
      err.code = "ERR_IPC_CHANNEL_CLOSED";
      if (cb) queueMicrotask(() => cb(err));
      else queueMicrotask(() => this.__emitForkError(err));
      return false;
    }
    __validateSendMessage(message, handle);
    try {
      this.__worker.postMessage(message);
    } catch (e) {
      const err = e instanceof Error ? e : new Error(String(e));
      if (!err.code) err.code = "ERR_IPC_CHANNEL_CLOSED";
      if (cb) queueMicrotask(() => cb(err));
      else queueMicrotask(() => this.__emitForkError(err));
      return false;
    }
    if (cb) queueMicrotask(() => cb(null));
    return true;
  }
  disconnect() {
    if (!this.__forkChild) {
      throw Object.assign(new Error("ERR_NOT_SUPPORTED: child disconnect() needs an IPC channel (use fork)"), { code: "ERR_NOT_SUPPORTED" });
    }
    // node 口径：已断开再调即 'error' 发射 ERR_IPC_DISCONNECTED（无监听即抛，
    // disconnect 套件 assert.throws 形）。
    if (!this.__connected) {
      const err = new Error("IPC channel is already disconnected");
      err.code = "ERR_IPC_DISCONNECTED";
      this.__emitForkError(err);
      return;
    }
    this.__connected = false;
    // 控制信封（单键载荷，子端 shim 解释为 disconnect，不投递给用户）。
    try { this.__worker.postMessage({ __wjs_fork_ctl: "disconnect" }); } catch {}
    if (typeof this.#ondisconnect === "function") {
      try { this.#ondisconnect(); } catch {}
    }
  }
  __emitForkError(err) {
    if (typeof this.#onerror === "function") this.#onerror(err);
    else throw err;
  }
  __onForkMessage(m) {
    // NODE_ 前缀分流（internal 套件）：cmd 首段 NODE_ 即内部消息，
    // 余下一律普通 message（真机 cluster 协议口径）。
    if (m !== null && typeof m === "object" && !Array.isArray(m) &&
        typeof m.cmd === "string" && m.cmd.startsWith("NODE_")) {
      if (typeof this.#oninternal === "function") this.#oninternal(m);
      return;
    }
    if (typeof this.#onmessage === "function") this.#onmessage(m);
  }
  __onForkExit(code) {
    if (this.__exitDone) return; // abort 路径已 synth（exit(null, killSignal)）
    this.__exitDone = true;
    this.__exitCode = code;
    if (this.__connected) this.__connected = false;
    // node 口径：exit/close 双参 (code, signal)（fork 旧单对象形一并翻转，
    // 与 spawn 派发同形）。
    if (typeof this.onexit === "function") this.onexit(code, null);
    if (typeof this.onclose === "function") this.onclose(code, null);
  }
  get connected() { return !!this.__connected; }
  unref() {
    if (this.__worker) { try { this.__worker.unref(); } catch {} }
    return this;
  }
  ref() {
    if (this.__worker) { try { this.__worker.ref(); } catch {} }
    return this;
  }
}
// cwd 归一（真机 normalizeSpawnArguments/getPathFromURL 口径，实测 probe）：
// 字符串直通；file: URL 取 pathname（host 必须 localhost/空——darwin 平台文案）；
// 非 file: 抛 ERR_INVALID_URL_SCHEME "The URL must be of scheme file"；
// 非串非 URL 抛 ARG_TYPE。
// worker env 快照会话：子进程缺省继承 worker 的 env 视图（含 worker 内写入；
// 真机 spawnSync 的 env 缺省即 worker 的 process.env——process-env 套件口径）。
// 主会话回 null（Rust 侧继承真进程 env，原语义）。
function __childEnv(o) {
  // node 口径：env 缺省继承（worker 快照/真进程）；for-in 含原型键（env 套件
  // FOO 经原型）；undefined 值跳过；余下 String() 化（null → "null"，否则
  // Rust 侧 JSON 解析报 "must be JSON"）；键值 \0 校验（reject-null-bytes 套件）。
  if (o.env === undefined || o.env === null) {
    if (__wjs_worker_env_snapshot() !== undefined) o.env = { ...process.env };
    else return o.env;
  }
  const out = {};
  for (const k in o.env) {
    const v = o.env[k];
    if (v === undefined) continue;
    __nullCheck(k, `options.env['${k}']`);
    __nullCheck(typeof v === "string" ? v : String(v), `options.env['${k}']`);
    out[k] = typeof v === "string" ? v : String(v);
  }
  o.env = out;
  return o.env;
}
function __cwdPath(v) {
  __nullCheck(v, "options.cwd", "must be a string, Uint8Array, or URL without null bytes");
  if (typeof v === "string") return v;
  if (v !== null && typeof v === "object" && typeof v.href === "string") {
    const u = (typeof URL === "function" && v instanceof URL) ? v : new URL(v.href);
    if (u.protocol !== "file:") {
      const e = new Error("The URL must be of scheme file");
      e.code = "ERR_INVALID_URL_SCHEME";
      throw e;
    }
    if (u.hostname !== "" && u.hostname !== "localhost") {
      const e = new Error(`File URL host must be "localhost" or empty on ${process.platform}`);
      e.code = "ERR_INVALID_FILE_URL_HOST";
      throw e;
    }
    return decodeURIComponent(u.pathname);
  }
  throw new ERR_INVALID_ARG_TYPE("options.cwd", ["string", "URL"], v);
}

// spawn 预检（execvp 语义：失败走异步 error + close(-errno)，不抛、pid undefined）。
// 返回 null = 可起；否则返回待发 error（code/errno/syscall/path）。
// 相对路径按新 cwd 解析（node chdir 先于 exec）；裸名沿 PATH（子 env 优先）。
function __spawnPreflight(file, o) {
  const mk = (code, errno) => {
    const e = new Error(`spawn ${file} ${code}`);
    e.code = code;
    e.errno = errno;
    e.syscall = `spawn ${file}`;
    e.path = file;
    return e;
  };
  let dir = o.cwd;
  if (dir !== null && dir !== undefined && dir !== "") {
    try {
      const st = fs.statSync(dir);
      if (!st.isDirectory()) return mk("ENOTDIR", __syncErrno("ENOTDIR"));
    } catch {
      return mk("ENOENT", __syncErrno("ENOENT"));
    }
  } else {
    dir = null;
  }
  const probe = (p) => {
    try { fs.accessSync(p, fs.constants.X_OK); return true; } catch (e2) {
      return e2 && (e2.code === "EACCES" || e2.code === "EPERM") ? "eacces" : false;
    }
  };
  if (file.includes("/")) {
    const p = (dir !== null && !file.startsWith("/")) ? dir + "/" + file : file;
    const hit = probe(p);
    if (hit === "eacces") return mk("EACCES", __syncErrno("EACCES"));
    if (!hit) return mk("ENOENT", __syncErrno("ENOENT"));
    return null;
  }
  const pathVar = String((o.env && o.env.PATH) || process.env.PATH || "").split(":");
  for (const d of pathVar) {
    if (!d) continue;
    if (probe(d + "/" + file) === true) return null;
  }
  return mk("ENOENT", __syncErrno("ENOENT"));
}

// shell 模式预检（只查 cwd——命令本体经 shell 解析，PATH/可执行位由 shell 管）。
function __spawnPreflightCwd(o) {
  const dir = o.cwd;
  if (dir !== null && dir !== undefined && dir !== "") {
    try {
      const st = fs.statSync(dir);
      if (!st.isDirectory()) return __preflightErr("ENOTDIR", dir);
    } catch {
      return __preflightErr("ENOENT", dir);
    }
  }
  return null;
}
function __preflightErr(code, file) {
  const e = new Error(`spawn ${file} ${code}`);
  e.code = code;
  e.errno = __syncErrno(code);
  e.syscall = `spawn ${file}`;
  e.path = file;
  return e;
}
function __normSpawnAsyncOpts(opts) {
  // node 口径：stdio 缺省（整体缺或数组缺项）一律 'pipe'——spawnPromisified
  // 系套件直接读 child.stdout/stderr（'child.stderr is null' 现形于
  // test-url-parse-deprecation）；旧默认 inherit 是偏差。
  const o = { cwd: null, env: null, detached: false, stdio: ["pipe", "pipe", "pipe"], timeoutMs: 0, signal: null, killSigname: "SIGTERM", killSigno: 15, shell: undefined };
  if (opts === undefined || opts === null) { o.env = __childEnv(o); return o; }
  if (opts.cwd !== undefined && opts.cwd !== null) {
    o.cwd = __cwdPath(opts.cwd);
    // node：'' 不 chdir（cwd 套件 'number' pid 断言），Rust 侧空串即失败——置 null。
    if (o.cwd === "") o.cwd = null;
  }
  if (opts.env !== undefined) o.env = { ...opts.env };
  if (opts.detached !== undefined) o.detached = !!opts.detached;
  // uid/gid：validateInt32 逐字口径（真机 26.8.2 实测：非 number 即 ARG_TYPE，
  // 非整数/超 int32 即 RANGE；范围内负数过校验，spawn 期 EPERM 见下）。
  for (const k of ["uid", "gid"]) {
    const v = opts[k];
    if (v === undefined || v === null) continue;
    if (typeof v !== "number") throw new ERR_INVALID_ARG_TYPE(`options.${k}`, "number", v);
    if (!Number.isInteger(v)) {
      const e = new RangeError(`The value of "options.${k}" is out of range. It must be an integer. Received ${v}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
    if (v < -2147483648 || v > 2147483647) {
      const e = new RangeError(`The value of "options.${k}" is out of range. It must be >= -2147483648 && <= 2147483647. Received ${v}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
  }
  // uid/gid：真机 _handle.spawn 同步 EPERM 口径——非特权指定他 id 即抛
  //（uid-gid 套件非 root 形；message 正则匹配）。值本身记档忽略。
  if (opts.uid !== undefined && opts.uid !== null && typeof opts.uid === "number") {
    if (typeof process.getuid === "function" && opts.uid !== process.getuid()) {
      const e = new Error("spawn EPERM");
      e.code = "EPERM"; e.errno = -1; e.syscall = "spawn"; throw e;
    }
  }
  if (opts.gid !== undefined && opts.gid !== null && typeof opts.gid === "number") {
    if (typeof process.getgroups === "function" && !process.getgroups().includes(opts.gid)) {
      const e = new Error("spawn EPERM");
      e.code = "EPERM"; e.errno = -1; e.syscall = "spawn"; throw e;
    }
  }
  // shell：boolean/string（node normalizeSpawnArguments 口径；spawn-shell 套件）。
  if (opts.shell !== undefined && opts.shell !== null) {
    if (typeof opts.shell !== "boolean" && typeof opts.shell !== "string") {
      throw new ERR_INVALID_ARG_TYPE("options.shell", ["boolean", "string"], opts.shell);
    }
    __nullCheck(opts.shell, "options.shell");
    o.shell = opts.shell;
  }
  // argv0：undefined/null 过；余下须字符串（真机 validateString）+ \0 校验；
  // 值由 Rust 侧经 unix arg0 落地（spawn-argv0 套件），此处只验不存。
  if (opts.argv0 !== undefined && opts.argv0 !== null) {
    if (typeof opts.argv0 !== "string") {
      throw new ERR_INVALID_ARG_TYPE("options.argv0", "string", opts.argv0);
    }
    __nullCheck(opts.argv0, "options.argv0");
  }
  // timeout：validateTimeout 逐字口径——非 number ARG_TYPE，负数/非整数 RANGE
  //（spawn-timeout-kill-signal 套件 'badValue'/{} 点名）。
  if (opts.timeout !== undefined && opts.timeout !== null) {
    if (typeof opts.timeout !== "number") {
      throw new ERR_INVALID_ARG_TYPE("options.timeout", "number", opts.timeout);
    }
    if (!Number.isInteger(opts.timeout) || opts.timeout < 0) {
      const e = new RangeError(`The value of "options.timeout" is out of range. It must be an integer >= 0. Received ${opts.timeout}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
    o.timeoutMs = opts.timeout;
  }
  // signal：undefined 过；余下必须 AbortSignal 实例（真机 validateAbortSignal）。
  if (opts.signal !== undefined) {
    if (!(opts.signal instanceof AbortSignal)) {
      throw new ERR_INVALID_ARG_TYPE("options.signal", "AbortSignal", opts.signal);
    }
    o.signal = opts.signal;
  }
  // killSignal：undefined/null/合法信号过；类型先行 ARG_TYPE，落空 UNKNOWN_SIGNAL。
  if (opts.killSignal !== undefined && opts.killSignal !== null) {
    const ks = opts.killSignal;
    if (typeof ks !== "string" && typeof ks !== "number") {
      throw new ERR_INVALID_ARG_TYPE("options.killSignal", ["string", "number"], ks);
    }
    const hit = __sigResolve(ks);
    if (!hit) {
      const e = new TypeError(`Unknown signal: ${String(ks)}`);
      e.code = "ERR_UNKNOWN_SIGNAL"; throw e;
    }
    o.killSigno = hit.signo; o.killSigname = hit.name;
  }
  if (opts.stdio !== undefined) {
    const one = (s) => {
      if (!["inherit", "ignore", "pipe"].includes(s)) {
        throw new Error(`NotSupportedError: spawn stdio '${s}' (inherit/ignore/pipe)`);
      }
      return s;
    };
    // node 口径：裸 'ipc' 非法（ARG_VALUE）；数组双 ipc 即 ERR_IPC_ONE_PIPE。
    if (typeof opts.stdio === "string") {
      if (opts.stdio === "ipc") throw new ERR_INVALID_ARG_VALUE("stdio", opts.stdio);
      o.stdio = [one(opts.stdio), one(opts.stdio), one(opts.stdio)];
    }
    else if (Array.isArray(opts.stdio)) {
      // 三元数组（缺省补 pipe；Node 的复杂组合如 fd 重定向不在此列，文档记录）
      if (opts.stdio.length > 3) {
        if (opts.stdio.filter((s) => s === "ipc").length > 1) throw new ERR_IPC_ONE_PIPE();
        throw new Error("NotSupportedError: spawn stdio array takes at most 3 entries");
      }
      // 流对象元（pipe-dataflow/merge/reuse 套件）：可读/可写流即转交位
      // （Rust 侧仍按 pipe 建真管，转交纯 JS 搭桥，见 __stdioWire）。
      o.stdio = [0, 1, 2].map((i) => {
        const s = opts.stdio[i];
        if (s === undefined) return "pipe";
        if (typeof s === "string") return one(s);
        if (s !== null && typeof s === "object" &&
            (typeof s.on === "function" || typeof s.write === "function")) {
          (o.stdioStreams ??= [])[i] = s;
          return "pipe";
        }
        throw new Error(`NotSupportedError: spawn stdio '${s}' (inherit/ignore/pipe)`);
      });
    } else {
      throw new Error("NotSupportedError: spawn stdio must be a string or array");
    }
  }
  if (opts.timeout !== undefined) o.timeoutMs = Number(opts.timeout);
  o.env = __childEnv(o);
  return o;
}
export function spawn(file, args, opts) {
  // normalizeSpawnArguments 逐字口径（真机 26.8.2 实测）：
  // file 必填字符串（空即 ARG_VALUE）；args 数组/缺席，余下非对象即 ARG_TYPE，
  // 纯对象回落为 options；options 缺席即 {}，显式 null/非对象即 ARG_TYPE。
  if (typeof file !== "string") throw new ERR_INVALID_ARG_TYPE("file", "string", file);
  if (file.length === 0) throw new ERR_INVALID_ARG_VALUE("file", file, "cannot be empty");
  if (Array.isArray(args)) { args = [...args]; }
  else if (args === undefined || args === null) { args = []; }
  else if (typeof args !== "object") { throw new ERR_INVALID_ARG_TYPE("args", "object", args); }
  else { opts = args; args = []; }
  if (opts === undefined) opts = {};
  else if (typeof opts !== "object" || opts === null || Array.isArray(opts)) { throw new ERR_INVALID_ARG_TYPE("options", "object", opts); }
  const o = __normSpawnAsyncOpts(opts);
  return __spawnInto(new ChildProcess(), file, args, o);
}
// stdio 流转交（pipe-dataflow/merge/reuse 套件）：spawn 数组元为流对象时
// 按位搭桥——stdin 位可读流 data/end 转入子 stdin；stdout/stderr 位可写流
// 接子对应流 data（end 不转：共享写端由持有者关，merge 套件多写者语义）。
function __stdioWire(proc, streams) {
  if (!streams) return;
  try {
    const src = streams[0];
    if (src && typeof src.on === "function" && proc.stdin) {
      src.on("data", (d) => { try { proc.stdin.write(d); } catch {} });
      src.on("end", () => { try { proc.stdin.end(); } catch {} });
    }
    const out = streams[1];
    if (out && typeof out.write === "function" && proc.stdout) {
      proc.stdout.on("data", (d) => { try { out.write(d); } catch {} });
    }
    const err = streams[2];
    if (err && typeof err.write === "function" && proc.stderr) {
      proc.stderr.on("data", (d) => { try { err.write(d); } catch {} });
    }
  } catch {}
}
// spawn 落地（函数与 ChildProcess.prototype.spawn 方法共用；proc 既是
// native 事件 target 也是返回对象——事件接线必须挂最终对象，禁中转搬运）。
function __spawnInto(proc, file, args, o) {
  // \0 校验（reject-null-bytes 套件；函数与方法共道）。
  __nullCheck(String(file), "file");
  for (let i = 0; i < (args || []).length; i++) __nullCheck(String(args[i]), `args[${i}]`);
  proc.spawnfile = String(file);
  proc.spawnargs = [...(args || [])].map(String);
  // shell（node normalizeSpawnArguments 口径）：file+args 空格拼接成 sh -c 串，
  // spawnfile 换 shell 本体、spawnargs 换 shell 形（spawn-shell 套件断
  // spawnargs 末元 = 拼串）；args 非空即 DEP0190（node 逐字文案）。
  let f2 = String(file);
  let a2 = [...(args || [])].map(String);
  if (o.shell !== undefined) {
    if (a2.length > 0) {
      process.emitWarning("Passing args to a child process with shell option true can lead to security vulnerabilities, as the arguments are not escaped, only concatenated.", "DeprecationWarning", "DEP0190");
    }
    const command = [f2, ...a2].join(" ");
    f2 = typeof o.shell === "string" ? o.shell : "/bin/sh";
    a2 = ["-c", __selfCmd(command, o.env ?? process.env)];
    proc.spawnfile = f2;
    proc.spawnargs = a2;
  }
  // execvp 预检（shell 时跳过——命令经 shell 解析；cwd 照查）：失败走异步
  // error + close(-errno)（cwd 套件：pid 'undefined'、error.code='ENOENT'、
  // exit 不发、close code -2——真机 probe）。
  const pf = o.shell === undefined ? __spawnPreflight(String(file), o) : __spawnPreflightCwd(o);
  if (pf !== null) {
    // spawn-error 套件：err.spawnargs 为参数数组（与 spawnargs 同值）。
    pf.spawnargs = [...proc.spawnargs];
    proc.__initStreams(o.stdio, 0);
    queueMicrotask(() => {
      proc.__emitError(pf);
      proc.__emitDeadClose(pf.errno);
    });
    return proc;
  }
  const [f3, a3] = __selfArgv(f2, a2);
  const id = __wjs_spawn_start(String(f3), JSON.stringify(a3), JSON.stringify({
    cwd: o.cwd, env: o.env, detached: o.detached, timeout_ms: o.timeoutMs,
    kill_signo: o.killSigno, kill_signame: o.killSigname,
  }), proc, JSON.stringify(o.stdio));
  // abort（node spawn 逐字口径：预中止经 nextTick 走同一 onAbortListener——
  // 子进程已起，kill(killSignal) 作用到才发 error(AbortError, cause=reason)；
  // exit(null, killSignal) 由真实进程死亡自带；exit 后注销监听——
  // spawn-timeout-kill-signal 套件 listenerCount 断言）。
  if (o.signal) {
    const onAbort = () => {
      if (proc.exitCode !== null || proc.signalCode !== null) return;
      if (__wjs_child_kill(id, String(o.killSigno))) proc.__emitAbort(o.signal.reason);
    };
    const disposable = addAbortListener(o.signal, onAbort);
    proc.__onExited = () => { try { disposable[Symbol.dispose](); } catch {} };
  }
  proc.__init(id, o.stdio);
  // stdio 流转交（数组流对象元，转交位搭桥）。
  __stdioWire(proc, o.stdioStreams);
  // 'spawn' 事件 nextTick/microtask 发射（监听挂载在 spawn() 返回后同步发生，
  // 恒早于数据/退出分发）。
  queueMicrotask(() => { try { proc.__emitSpawn(); } catch {} });
  return proc;
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
// exec 族（10f 重写，node 架构：execFile = spawn + 收集 + close 回调 + 返回
// live ChildProcess；exec = execFile('/bin/sh', ['-c', cmd])（normalizeExecArgs
// 口径）。旧 __asyncOneShot 一次性内核退役——"无 live 句柄"偏差消账。
// maxBuffer 超限 kill（真机 ERR_CHILD_PROCESS_STDIO_MAXBUFFER）；timeout 走
// spawn 既有 timeout_ms（killSignal 自定值偏差记档）；错误 cmd/code/killed/
// signal 照真机挂载。execFile 无回调即抛 ERR_INVALID_ARG_TYPE（真机口径）。
function __execCollect(child, o, cmdStr, args, cb) {
  // node execFile v26 逐字口径（exithandler/errorhandler/kill/data 三面）：
  // encoding 非 buffer 且 isEncoding 才串化（否则 Buffer concat，'invalid' 同）；
  // maxBuffer 超限截断入块再 kill（RangeError ERR_CHILD_PROCESS_STDIO_MAXBUFFER
  // 优先于通用失败错）；Infinity 即不限；killed = child.killed || 本地 killed。
  const encoding = (o.encoding !== "buffer" && Buffer.isEncoding(o.encoding)) ? o.encoding : null;
  const _stdout = [];
  const _stderr = [];
  let stdoutLen = 0;
  let stderrLen = 0;
  let killed = false;
  let exited = false;
  let ex = null;

  function exithandler(code, signal) {
    if (exited) return;
    exited = true;
    if (!cb) return;
    let stdout;
    let stderr;
    if (encoding || child.stdout?.readableEncoding) stdout = _stdout.join("");
    else stdout = Buffer.concat(_stdout);
    if (encoding || child.stderr?.readableEncoding) stderr = _stderr.join("");
    else stderr = Buffer.concat(_stderr);
    if (!ex && code === 0 && signal === null) {
      cb(null, stdout, stderr);
      return;
    }
    let cmd = cmdStr;
    if (args?.length) cmd += ` ${args.join(" ")}`;
    ex ||= __genericNodeError(`Command failed: ${cmd}\n${stderr}`, {
      code: (typeof code === "number" && code < 0) ? __uvName(code) : code,
      killed: child.killed || killed,
      signal: signal,
    });
    ex.cmd = cmdStr;
    cb(ex, stdout, stderr);
  }
  function errorhandler(e) {
    ex = e;
    if (child.stdout) child.stdout.destroy();
    if (child.stderr) child.stderr.destroy();
    exithandler();
  }
  function kill() {
    if (child.stdout) child.stdout.destroy();
    if (child.stderr) child.stderr.destroy();
    killed = true;
    try {
      child.kill(o.killSignal);
    } catch (e) {
      ex = e;
      exithandler();
    }
  }
  if (child.stdout) {
    if (encoding) child.stdout.setEncoding(encoding);
    child.stdout.on("data", (chunk) => {
      if (o.maxBuffer === Infinity) { _stdout.push(chunk); return; }
      const len = typeof chunk === "string" ? Buffer.byteLength(chunk, encoding) : chunk.length;
      stdoutLen += len;
      if (stdoutLen > o.maxBuffer) {
        const truncatedLen = o.maxBuffer - (stdoutLen - len);
        _stdout.push(typeof chunk === "string" ? chunk.slice(0, truncatedLen) : chunk.slice(0, truncatedLen));
        ex = __maxBufferErr("stdout");
        kill();
      } else {
        _stdout.push(chunk);
      }
    });
  }
  if (child.stderr) {
    if (encoding) child.stderr.setEncoding(encoding);
    child.stderr.on("data", (chunk) => {
      if (o.maxBuffer === Infinity) { _stderr.push(chunk); return; }
      const len = typeof chunk === "string" ? Buffer.byteLength(chunk, encoding) : chunk.length;
      stderrLen += len;
      if (stderrLen > o.maxBuffer) {
        const truncatedLen = o.maxBuffer - (stderrLen - len);
        _stderr.push(typeof chunk === "string" ? chunk.slice(0, truncatedLen) : chunk.slice(0, truncatedLen));
        ex = __maxBufferErr("stderr");
        kill();
      } else {
        _stderr.push(chunk);
      }
    });
  }
  child.once("error", errorhandler);
  child.once("close", exithandler);
}
// node genericNodeError（execFile 失败错；killed/signal/code 挂载）。
function __genericNodeError(message, opts) {
  const err = new Error(message);
  const { code, killed, signal } = opts;
  err.code = code ?? null;
  err.killed = !!killed;
  err.signal = signal ?? null;
  return err;
}
function __maxBufferErr(stream) {
  const err = new RangeError(`${stream} maxBuffer length exceeded`);
  err.code = "ERR_CHILD_PROCESS_STDIO_MAXBUFFER";
  return err;
}

// spawn 预检（execFile 专用）：绝对/相对路径直查；裸名沿 PATH 找（node
// spawn 的 PATH 解析在 fork 失败即 ENOENT，pid 不发号——本仓 id 先发，故补查）。
function __canSpawnFile(file) {
  if (file.includes("/")) {
    try { fs.accessSync(file); return true; } catch { return false; }
  }
  const path = String(globalThis.process.env.PATH || "").split(":");
  for (const d of path) {
    if (!d) continue;
    try { fs.accessSync(d + "/" + file); return true; } catch { /* next */ }
  }
  return false;
}

export function execFile(file, args, opts, cb) {
  // normalizeExecFileArgs 逐字口径（真机 26.8.2 实测）：args 数组拷贝/函数即
  // 回调/纯对象回落 options/余下（字符串等）原位留待 spawn 位校验；
  // options 函数即回调/显式 null 即 {}/数组与非对象即 ARG_TYPE；
  // callback 给了但非函数即 ARG_TYPE（含 Received 段）。
  if (Array.isArray(args)) { args = [...args]; }
  else if (args !== undefined && args !== null && typeof args === "object") { cb = opts; opts = args; args = null; }
  else if (typeof args === "function") { cb = args; opts = null; args = null; }
  if (args === undefined || args === null) args = [];
  else if (!Array.isArray(args)) { throw new ERR_INVALID_ARG_TYPE("args", "object", args); }
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  else if (opts !== undefined && opts !== null) {
    if (typeof opts !== "object" || Array.isArray(opts)) { throw new ERR_INVALID_ARG_TYPE("options", "object", opts); }
  }
  if (opts === undefined || opts === null) opts = {};
  if (cb !== undefined && cb !== null && typeof cb !== "function") {
    throw new ERR_INVALID_ARG_TYPE("callback", "function", cb);
  }
  const o = __normExecOpts(opts);
  __nullCheck(String(file), "file");
  for (let i = 0; i < (args ?? []).length; i++) __nullCheck(String(args[i]), `args[${i}]`);
  // shell 透传（execFile 缺省 false——__normExecOpts 的 true 缺省是 execSync/exec
  // 语义，此处按 opts 原样）。
  const shellOpt = (opts !== null && typeof opts === "object" && opts.shell !== undefined) ? opts.shell : undefined;
  const argv = (args ?? []).map(String);
  const cmdStr = String(file);
  // execvp 预检（shell 模式跳过——命令经 shell 解析）：失败走异步 error + 回调
  //（不抛；node ENOENT 时 pid 恒 undefined——死句柄 + 微任务回调）。
  if (shellOpt === undefined && !__canSpawnFile(String(file))) {
    const err = new Error(`spawn ${file} ENOENT`);
    err.code = "ENOENT";
    err.errno = -2;
    err.syscall = `spawn ${file}`;
    err.path = String(file);
    err.cmd = cmdStr;
    queueMicrotask(() => cb && cb(err, "", ""));
    return new ChildProcess();
  }
  const child = spawn(String(file), argv, {
    cwd: o.cwd,
    env: o.env,
    signal: o.signal,
    shell: shellOpt,
    stdio: ["pipe", "pipe", "pipe"],
  });
  child.__cmdStr = cmdStr;
  __execCollect(child, o, cmdStr, argv, cb);
  // timeout/killSignal 在 execFile 层（node 口径——spawn 调用不带 timeout）。
  if (o.timeoutMs > 0 && Number.isFinite(o.timeoutMs)) {
    const tid = setTimeout(() => {
      try { child.kill(o.killSignal); } catch { /* 已退即走下 */ }
    }, o.timeoutMs);
    if (typeof tid.unref === "function") tid.unref();
  }
  return child;
}

export function exec(command, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  // node 口径：callback 可缺席（返回 live child）；给了但非函数才 ARG_TYPE。
  if (cb !== undefined && cb !== null && typeof cb !== "function") {
    const err = new TypeError("The \"callback\" argument must be of type function");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const o = __normExecOpts(opts);
  __nullCheck(String(command), "command");
  const cmdStr = String(command);
  // 自举翻译（exec-encoding/timeout 系：escapePOSIXShell 的 ${ESCAPED_n} env
  // 间接形 + 裸文件形；err.cmd 保持原文——改写只作用于 /bin/sh -c 的串）。
  const child = execFile(cmdStr, undefined, { ...o, timeout: o.timeoutMs, encoding: o.encoding }, cb && ((err, stdout, stderr) => {
    if (err) err.cmd = cmdStr;
    cb(err, stdout, stderr);
  }));
  return child;
}
// node customPromiseExecFunction 逐字口径：promise.child 挂原函数返回值；
// 原函数在 executor **外**调用——同步校验抛错不被 Promise 吞（abortcontroller
// 套件 assert.throws 直调 promisify(exec) 形）。
function __customPromiseExec(orig) {
  return function (...args) {
    let res;
    let rej;
    const promise = new Promise((resolve, reject) => { res = resolve; rej = reject; });
    promise.child = orig(...args, (err, stdout, stderr) => {
      if (err !== null && err !== undefined) {
        err.stdout = stdout;
        err.stderr = stderr;
        rej(err);
      } else {
        res({ stdout, stderr });
      }
    });
    return promise;
  };
}
Object.defineProperty(exec, Symbol.for("nodejs.util.promisify.custom"), {
  value: __customPromiseExec(exec),
  enumerable: false,
});
Object.defineProperty(execFile, Symbol.for("nodejs.util.promisify.custom"), {
  value: __customPromiseExec(execFile),
  enumerable: false,
});
export function execFileSync(file, args, opts) {
  if (args !== undefined && args !== null && !Array.isArray(args)) { opts = args; args = []; }
  const o = __normSpawnOpts(opts);
  __nullCheck(String(file), "file");
  for (let i = 0; i < (args || []).length; i++) __nullCheck(String(args[i]), `args[${i}]`);
  const f = String(file);
  const [f2, a2] = __selfArgv(f, [...(args || [])].map(String));
  const r = JSON.parse(__wjs_cp_spawn(f2, JSON.stringify(a2), JSON.stringify({
    cwd: o.cwd ?? null, env: o.env ?? null, timeout_ms: o.timeoutMs,
    shell: false, kill_signo: o.killSigno, kill_signame: o.killSigname,
    argv0: o.argv0, detached: !!o.detached,
    stdin_inherit: !!o.stdioInherit[0], stdout_inherit: !!o.stdioInherit[1],
    stderr_inherit: !!o.stdioInherit[2],
    input_b64: o.inputB64, max_buffer: o.maxBuffer,
  })));
  if (r.spawnErr || r.timedOut || r.status !== 0) __spawnError(file, r, o.encoding);
  return __toOut(r.stdout_b64, o.encoding);
}
// fork 子会话入口（worker eval 串；占位 `__FORK_MOD__`/`__FORK_ARGV__` 由
// `fork()` 经 replacer 函数填 JSON——`format!` 拼 JS 禁花括号转义，见 §4.44）。
// 子端 IPC 面：process.send/disconnect/on('message')/connected/channel，
// 经 parentPort 与父端 ChildProcess 桥接；控制信封 `{__wjs_fork_ctl:
// // "disconnect"}` 单键载荷不投递给用户（见父端 `disconnect()`）。
const __FORK_CHILD_SRC = `
import { parentPort } from "node:worker_threads";
const __mod = __FORK_MOD__;
const __forkArgs = __FORK_ARGV__;
process.argv = [process.execPath, __mod, ...__forkArgs];
process.connected = true;
process.channel = { ref() {}, unref() {}, hasRef() { return true; } };
process.send = (message, ...rest) => {
  // 校验内联（eval 会话无模块作用域；与 __validateSend* 同口径，code/name 逐字。
  // 注意：本块在外层模板字符串内，禁用模板字面量与插值写法，一律字符串拼接）。
  const __received = (v) => v === null ? "null" : (typeof v === "string" ? ("'" + v + "'") : String(v));
  let handle, options, cb = null;
  const a = [...rest];
  if (a.length > 0 && typeof a[0] === "function") { cb = a.shift(); }
  else {
    handle = a.shift();
    if (a.length > 0 && typeof a[0] === "function") { cb = a.shift(); }
    else {
      options = a.shift();
      if (a.length > 0 && typeof a[0] === "function") { cb = a.shift(); }
    }
  }
  if (options !== undefined && (typeof options !== "object" || options === null)) {
    const e = new TypeError('The "options" argument must be of type object. Received ' + __received(options));
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  if (!process.connected || parentPort === null) {
    const err = new Error("Channel closed");
    err.code = "ERR_IPC_CHANNEL_CLOSED";
    if (cb) queueMicrotask(() => cb(err));
    else queueMicrotask(() => process.__wjs_emit("error", err));
    return false;
  }
  if (message === undefined) {
    const e = new TypeError('The "message" argument must be specified');
    e.code = "ERR_MISSING_ARGS"; throw e;
  }
  if (typeof message !== "string" && typeof message !== "object" &&
      typeof message !== "number" && typeof message !== "boolean") {
    const e = new TypeError('The "message" argument must be one of type string, object, number, or boolean.');
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  if (handle !== undefined && handle !== null) {
    const e = new TypeError("This handle type cannot be sent");
    e.code = "ERR_INVALID_HANDLE_TYPE"; throw e;
  }
  try {
    parentPort.postMessage(message);
  } catch (e) {
    const err = e instanceof Error ? e : new Error(String(e));
    if (!err.code) err.code = "ERR_IPC_CHANNEL_CLOSED";
    if (cb) queueMicrotask(() => cb(err));
    else queueMicrotask(() => process.__wjs_emit("error", err));
    return false;
  }
  if (cb) queueMicrotask(() => cb(null));
  return true;
};
process.disconnect = () => {
  if (!process.connected) return;
  process.connected = false;
  try { parentPort.close(); } catch {}
  process.__wjs_emit("disconnect");
};
parentPort.on("message", (message) => {
  if (message !== null && typeof message === "object" && !Array.isArray(message) &&
      Object.keys(message).length === 1 && message.__wjs_fork_ctl === "disconnect") {
    if (process.connected) {
      process.connected = false;
      try { parentPort.close(); } catch {}
      process.__wjs_emit("disconnect");
    }
    return;
  }
  // NODE_ 前缀分流（子进程侧与父端 __onForkMessage 同口径：cluster
  // NODE_CLUSTER 信封走 internalMessage；listen-twice 套件点名）。
  if (message !== null && typeof message === "object" && !Array.isArray(message) &&
      typeof message.cmd === "string" && message.cmd.indexOf("NODE_") === 0) {
    process.__wjs_emit("internalMessage", message);
    return;
  }
  process.__wjs_emit("message", message);
});
parentPort.on("close", () => {
  if (process.connected) {
    process.connected = false;
    process.__wjs_emit("disconnect");
  }
});
// 内部监听不续命子会话（worker 空转即退，真机口径：无用户监听的子进程
// 脚本结束即退，迟发消息即 ERR_IPC_CHANNEL_CLOSED；有用户监听才续命）：
// newListener 已置 listening 位，此处复位；process 系手写表（无 newListener
// 事件），故直包 message 订阅入口（once/addListener 走 on，removeListener
// 走 off）；投递走 listenerCount 门控，不受 counting 位影响。
// 注意：本块在外层模板字符串内，禁用模板字面量与插值写法。
try { __wjs_port_unlisten(parentPort.__id); } catch {}
const __ppId = parentPort.__id;
const __procListen = () => { try { __wjs_port_listen(__ppId); } catch {} };
const __procUnlisten = () => {
  if (process.listenerCount("message") === 0) { try { __wjs_port_unlisten(__ppId); } catch {} }
};
const __procOn = process.on;
process.on = function (type, cb) {
  if (type === "message" && typeof cb === "function") __procListen();
  return __procOn.call(this, type, cb);
};
process.addListener = process.on;
const __procOff = process.off;
process.off = function (type, cb) {
  const r = __procOff.call(this, type, cb);
  if (type === "message") __procUnlisten();
  return r;
};
process.removeListener = process.off;
const __procRemoveAll = process.removeAllListeners;
process.removeAllListeners = function (type) {
  const r = __procRemoveAll.call(this, type);
  if (type === undefined || type === "message") __procUnlisten();
  return r;
};
await import(__mod);
`;
function __normForkOpts(opts) {
  const o = { execPath: process.execPath, killSignal: "SIGTERM", silent: false, signal: null, killSigname: "SIGTERM", killSigno: 15 };
  if (opts === undefined || opts === null) return o;
  if (typeof opts !== "object") {
    const err = new TypeError("The \"options\" argument must be of type object");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  // 线程底座：cwd/env/execArgv/silent/stdio/serialization/timeout/detached 等
  // 接受忽略（同进程线程，无独立进程环境；stdio 恒 null，见 `fork` 文档）；
  // \0 照验（reject-null-bytes 套件）。
  if (opts.killSignal !== undefined) o.killSignal = String(opts.killSignal);
  if (opts.cwd !== undefined && opts.cwd !== null) {
    if (typeof opts.cwd !== "string") throw new ERR_INVALID_ARG_TYPE("options.cwd", "string", opts.cwd);
    __nullCheck(opts.cwd, "options.cwd", "must be a string, Uint8Array, or URL without null bytes");
  }
  if (opts.argv0 !== undefined && opts.argv0 !== null) {
    if (typeof opts.argv0 !== "string") throw new ERR_INVALID_ARG_TYPE("options.argv0", "string", opts.argv0);
    __nullCheck(opts.argv0, "options.argv0");
  }
  if (opts.execPath !== undefined) { __nullCheck(String(opts.execPath), "options.execPath"); o.execPath = String(opts.execPath); }
  if (opts.execArgv !== undefined) {
    if (!Array.isArray(opts.execArgv)) throw new ERR_INVALID_ARG_TYPE("options.execArgv", "Array", opts.execArgv);
    opts.execArgv.forEach((a, i) => __nullCheck(String(a), `options.execArgv[${i}]`));
  }
  // env：透传 worker 快照（fork 炸弹案：旧"值忽略"致自定义 env 丢失，
  // 子复走父分支指数 fork；真机 env 缺省即 process.env 拷贝）。
  // \0 照验（reject-null-bytes 套件）。
  if (opts.env !== undefined && opts.env !== null) {
    for (const k in opts.env) {
      const v = opts.env[k];
      if (v === undefined) continue;
      __nullCheck(k, `options.env['${k}']`);
      __nullCheck(typeof v === "string" ? v : String(v), `options.env['${k}']`);
    }
    o.env = { ...opts.env };
  }
  if (opts.silent !== undefined) o.silent = !!opts.silent;
  if (opts.signal !== undefined) {
    if (!(opts.signal instanceof AbortSignal)) {
      throw new ERR_INVALID_ARG_TYPE("options.signal", "AbortSignal", opts.signal);
    }
    o.signal = opts.signal;
  }
  const hit = __sigResolve(o.killSignal);
  if (hit) { o.killSigname = hit.name; o.killSigno = hit.signo; }
  return o;
}
export function fork(modulePath, args, opts) {
  if (modulePath === undefined || modulePath === null ||
      (typeof modulePath !== "string" && !(modulePath instanceof URL))) {
    const err = new TypeError("The \"modulePath\" argument must be of type string or URL");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  // args/options 归一（真机 fork 口径：缺席即 []/{}；纯对象回落 options；
  // 余下非数组即 ARG_TYPE/Array；options 数组与非对象即 ARG_TYPE）。
  if (args === undefined || args === null) { args = []; }
  else if (typeof args === "object" && !Array.isArray(args)) { opts = args; args = []; }
  else if (!Array.isArray(args)) { throw new ERR_INVALID_ARG_TYPE("args", "Array", args); }
  if (opts === undefined || opts === null) opts = {};
  else if (typeof opts !== "object" || Array.isArray(opts)) { throw new ERR_INVALID_ARG_TYPE("options", "object", opts); }
  if (modulePath instanceof URL && modulePath.protocol !== "file:") {
    const err = new TypeError("The \"modulePath\" argument must be a file URL");
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
  __nullCheck(String(modulePath instanceof URL ? modulePath.href : modulePath), "modulePath", "must be a string, Uint8Array, or URL without null bytes");
  for (let i = 0; i < (args || []).length; i++) __nullCheck(String(args[i]), `args[${i}]`);
  const o = __normForkOpts(opts);
  const modStr = modulePath instanceof URL ? modulePath.href : String(modulePath);
  const fileUrl = /^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(modStr) ? modStr : pathToFileURL(modStr).href;
  const argsArr = [...(args || [])].map(String);
  const proc = new ChildProcess();
  proc.__forkChild = true;
  proc.__killSignal = o.killSignal;
  proc.spawnfile = o.execPath;
  proc.spawnargs = [o.execPath, fileUrl, ...argsArr];
  // 预中止：线程不起（真机不起子进程同理），异步 error + exit/close(null, killSignal)
  //（fork-abort-signal 套件 exit mustCall(null, 'SIGTERM'/'SIGKILL')）。
  if (o.signal && o.signal.aborted) {
    const signame = o.killSigname;
    const reason = o.signal.reason;
    queueMicrotask(() => {
      proc.__emitAbort(reason);
      if (typeof proc.onexit === "function") proc.onexit(null, signame);
      if (typeof proc.onclose === "function") proc.onclose(null, signame);
    });
    return proc;
  }
  // silent（或 stdio: 'pipe'）：stdout/stderr 挂无数据管形流——pipe/unpipe 形状
  // 在（silent 套件 `child.stderr.pipe(process.stderr, {end:false})` 必须可用）；
  // 数据面偏差记档：线程底座子输出直走共享 stdio。
  if (o.silent) {
    proc.stdout = __forkNullStream();
    proc.stderr = __forkNullStream();
  }
  const src = __FORK_CHILD_SRC
    .replace("__FORK_MOD__", () => JSON.stringify(fileUrl))
    .replace("__FORK_ARGV__", () => JSON.stringify(argsArr));
  const worker = new Worker(src, { eval: true, __wjs_forkChild: true, env: o.env });
  proc.__worker = worker;
  proc.__connected = true;
  // 非 silent（stdio 继承）：stdout/stderr 恒 null（真机 26 逐项：fork 未 silent
  // 时 `c.stdout === null` 三面全真——继承无管道句柄；silent 才挂管形流）。
  proc.stdin = null;
  if (!o.silent) {
    proc.stdout = null;
    proc.stderr = null;
  }
  proc.channel = { ref() {}, unref() {} };
  worker.on("message", (m) => proc.__onForkMessage(m));
  worker.on("error", (e) => {
    if (typeof proc.onerror === "function") proc.onerror(e);
  });
  worker.on("exit", (code) => proc.__onForkExit(code));
  // abort（node：error 先行 + terminate；exit(null, killSignal) 由 abort 路径
  // 自发——线程 exit 事件被 __exitDone 旗拦下）。
  if (o.signal) {
    o.signal.addEventListener("abort", () => {
      if (proc.__exitDone) return;
      proc.__exitDone = true;
      try { worker.terminate(); } catch { /* 已退即走下 */ }
      proc.__emitAbort(o.signal.reason);
      if (typeof proc.onexit === "function") proc.onexit(null, o.killSigname);
      if (typeof proc.onclose === "function") proc.onclose(null, o.killSigname);
    }, { once: true });
  }
  return proc;
}
// fork silent 管形流（无数据——见 `fork` 文档偏差；pipe/unpipe/on 形状在）。
function __forkNullStream() {
  const listeners = {};
  return {
    on(ev, cb) { (listeners[ev] ||= []).push(cb); return this; },
    once(ev, cb) { const w = (...a) => { this.off(ev, w); cb(...a); }; w.__wjs_orig = cb; return this.on(ev, w); },
    off(ev, cb) {
      const l = listeners[ev];
      if (l) {
        let i = l.findIndex((f) => f === cb || f.__wjs_orig === cb);
        while (i >= 0) { l.splice(i, 1); i = l.findIndex((f) => f === cb || f.__wjs_orig === cb); }
      }
      return this;
    },
    removeListener(ev, cb) { return this.off(ev, cb); },
    pipe(dest) { return dest; },
    unpipe() { return this; },
    setEncoding() { return this; },
    pause() { return this; },
    resume() { return this; },
    read() { return null; },
    destroy() { return this; },
    get readable() { return false; },
    get destroyed() { return false; },
  };
}
export default { execSync, spawnSync, spawn, exec, execFile, execFileSync, fork, ChildProcess };
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
