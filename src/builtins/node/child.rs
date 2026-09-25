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

/// 内嵌 ESM 源（`node:child_process`；同步子集，见头注；§0.9 按域分块：
/// `child_proc.js` ChildProcess 类 + `child_spawn.js` spawn/exec 族，concat 字节恒等）。
pub const SOURCE: &str = concat!(
    include_str!("child_proc.js"),
    include_str!("child_proc_onexit.js"),
    include_str!("child_spawn.js"),
);

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
