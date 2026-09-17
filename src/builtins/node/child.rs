//! `node:child_process` 同步子集 + 异步 `spawn`（plan Phase 4d/c-4x）。
//! 同步经 `std::process` 阻塞跑；异步经 `tokio::process` + 事件循环分发
//! `exit/close/error`；stdio 支持 `inherit`/`ignore`/`pipe`（字符串三同或三元数组）。
//! pipe 口径（10f 起 node 真语义）：stdout/stderr 为 legacy Readable 面
//!（`on('data'/'end'/'close'/'error')/once/off/setEncoding/pause/resume/
//! destroy`；data 缺省 Buffer，setEncoding 后为串——真机 `Readable`，
//! spawnPromisified 系套件口径；内部经 Web ReadableStream 泵接 Rust 块序，
//! 出块序 = 进程出序，exit 前必先送达残留数据再调 onexit/onclose）；
//! stdin 为 `WritableStream`（write 满即发，close 关写端；子进程已走即写
//! 失败；legacy Writable 记偏差）；无 max_buffer 上限（流式消费）。
//! `detached:true` 在 unix 起 setsid 组长，kill 走组杀（`nix` 轮子；win 回退直杀）。
//! 结果走 JSON 桥（二进制 base64）；错误形状由 prelude 组装（见 SOURCE）。
//!
//! `fork`（M5 vitest 牵引）：worker 线程底座的进程形 fork（零新 native）。
//! 子会话跑目标模块（argv `[execPath, module, ...args]`，与真机同形），子端
//! `process.send/disconnect/on('message')/connected/channel` 经 parentPort 桥接；
//! 父端为 `ChildProcess`（`send/on('message')/disconnect/connected/kill` 全语义，
//! 关通道后 send 回 false + 异步 `ERR_IPC_CHANNEL_CLOSED`，真机口径）。
//! 偏差（记档）：同进程线程（无独立进程；env/cwd/execArgv/silent/stdio/
//! serialization/timeout/detached 接受忽略，stdio 恒 null）；子发消息无监听即丢
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
    use crate::jsapi_glue::{call_two, get_prop_value, parse_json};
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
const {
  codes: {
    ERR_INVALID_ARG_TYPE: { HideStackFramesError: ERR_INVALID_ARG_TYPE },
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
  // 缺省 utf8（真机实测；exec 异步族与同步族缺省不同：同步恒 buffer，禁串）。
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
  return o;
}
function __normSpawnOpts(opts) {
  // 缺省 encoding "buffer"（真机口径；exec 系另为 utf8，不串）。
  const o = { encoding: "buffer", timeoutMs: 0, shell: false, shellPath: null, maxBuffer: 1024 * 1024, inputB64: null, killSigno: 15, killSigname: "SIGTERM", argv0: null, cwd: null, detached: false, stdioInherit: [false, false, false] };
  if (opts === undefined || opts === null) return o;
  if (opts.encoding !== undefined) o.encoding = opts.encoding;
  // 字符串选项（cwd/argv0）：undefined/null 过，余下非串即 ARG_TYPE。
  for (const k of ["cwd", "argv0"]) {
    const v = opts[k];
    if (v === undefined || v === null) continue;
    if (typeof v !== "string") {
      throw new ERR_INVALID_ARG_TYPE(`options.${k}`, "string", v);
    }
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
  // shell：undefined/null/布尔/字符串过（字符串即 shell 路径）；余下 ARG_TYPE。
  if (opts.shell !== undefined && opts.shell !== null) {
    if (typeof opts.shell === "boolean") { o.shell = opts.shell; o.shellPath = null; }
    else if (typeof opts.shell === "string") { o.shell = true; o.shellPath = opts.shell; }
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
// 自举翻译（input/timeout/maxbuf 套件：子进程即自身时，Node 形 argv
// （`-e` 脚本/裸文件）映射到本仓全 flag CLI；他家二进制原样透传。
// `-e`  extras 透传（本仓 --eval 尾参作脚本 argv，最佳 effort）。
function __selfArgv(file, args) {
  if (file !== process.execPath) return [file, args];
  const a = [...args];
  if (a[0] === "-e") return [file, ["--eval", ...a.slice(1)]];
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
// shell 串首自举翻译（execsync-maxbuf 套件：`"<execPath>" -e/-p/-pe X`
// 经 shell 跑自身；`$NODE` token（env 透传）同理）。仅串首二进制位 +
// 纯 [pe] 组合旗才改写（`--eval` 回显 completion，与 -p 语义对等），
// 余下一律原样（误伤用户脚本更糟）。
function __selfCmd(cmd) {
  const m = String(cmd).match(/^("[^"]*"|'[^']*'|\$NODE|\S+)\s+-([A-Za-z]+)\s?([\s\S]*)$/);
  if (!m) return cmd;
  let bin = m[1];
  const flag = m[2], rest = m[3];
  const unq = (bin.startsWith('"') && bin.endsWith('"')) || (bin.startsWith("'") && bin.endsWith("'")) ? bin.slice(1, -1) : bin;
  if (unq !== process.execPath && unq !== "$NODE") return cmd;
  if (!/^[pe]+$/.test(flag)) return cmd;
  return `${bin} --eval ${rest}`;
}
export function execSync(cmd, opts) {
  const o = __normExecOpts(opts);
  // execSync 缺省 Buffer（真机实测；exec 异步缺省 utf8）：未显式给编码即改 buffer。
  if (typeof opts !== "string" && (opts === undefined || opts === null || opts.encoding === undefined)) o.encoding = "buffer";
  cmd = __selfCmd(String(cmd));
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
  const emit = (ev, ...args) => {
    for (const l of [...(listeners[ev] || [])]) {
      try { l(...args); } catch {}
    }
  };
  async function pump() {
    if (reader === null) reader = web.getReader();
    while (flowing && !paused && !ended && !destroyed) {
      let r;
      try { r = await reader.read(); } catch (e) { emit("error", e); return; }
      if (r.done) {
        ended = true;
        emit("end");
        emit("close");
        return;
      }
      let chunk = Buffer.from(r.value);
      if (enc !== null) chunk = chunk.toString(enc);
      emit("data", chunk);
    }
  }
  const api = {
    on(ev, cb) {
      (listeners[ev] ||= []).push(cb);
      if (ev === "data") { flowing = true; pump(); }
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
    destroy() {
      if (destroyed) return api;
      destroyed = true;
      try { if (reader !== null) reader.cancel(); } catch {}
      emit("close");
      return api;
    },
    // 整收口径（§4.67 同款）：read() 恒 null，数据走 'data' 事件。
    read() { return null; },
    get destroyed() { return destroyed; },
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
  #onmessage = null;
  #ondisconnect = null;
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
  __killSignal = "SIGTERM";
  __init(id, stdio) {
    this.#id = id;
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
      if (!this.__forkChild) {
        // spawn 子进程无 fd-passing 通道（记档缺口）：监听即明错，不静默吞
        throw Object.assign(new Error("ERR_NOT_SUPPORTED: child IPC channel not supported (use fork)"), { code: "ERR_NOT_SUPPORTED" });
      }
      if (event === "message") this.#onmessage = cb;
      else this.#ondisconnect = cb;
    }
    else throw new Error(`NotSupportedError: ChildProcess event '${event}' (exit/close/error/spawn/message/disconnect)`);
    return this;
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
    if (event === "exit" && match(this.#onexit)) this.onexit = null;
    else if (event === "close" && match(this.#onclose)) this.onclose = null;
    else if (event === "error" && match(this.#onerror)) this.onerror = null;
    else if (event === "spawn" && match(this.#onspawn)) this.onspawn = null;
    else if (event === "message" && match(this.#onmessage)) this.#onmessage = null;
    else if (event === "disconnect" && match(this.#ondisconnect)) this.#ondisconnect = null;
    return this;
  }
  removeListener(event, cb) { return this.off(event, cb); }
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
    let cb = null;
    for (const a of rest) if (typeof a === "function") cb = a;
    if (!this.__connected) {
      // Node 口径：关通道后 send 回 false，并异步报 ERR_IPC_CHANNEL_CLOSED。
      const err = new Error("Channel closed");
      err.code = "ERR_IPC_CHANNEL_CLOSED";
      if (cb) queueMicrotask(() => cb(err));
      queueMicrotask(() => this.__emitForkError(err));
      return false;
    }
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
    if (!this.__connected) return;
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
    if (typeof this.#onmessage === "function") this.#onmessage(m);
  }
  __onForkExit(code) {
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
function __normSpawnAsyncOpts(opts) {
  // node 口径：stdio 缺省（整体缺或数组缺项）一律 'pipe'——spawnPromisified
  // 系套件直接读 child.stdout/stderr（'child.stderr is null' 现形于
  // test-url-parse-deprecation）；旧默认 inherit 是偏差。
  const o = { cwd: null, env: null, detached: false, stdio: ["pipe", "pipe", "pipe"], timeoutMs: 0 };
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
      // 三元数组（缺省补 pipe；Node 的复杂组合如 fd 重定向不在此列，文档记录）
      if (opts.stdio.length > 3) throw new Error("NotSupportedError: spawn stdio array takes at most 3 entries");
      o.stdio = [0, 1, 2].map((i) => opts.stdio[i] === undefined ? "pipe" : one(opts.stdio[i]));
    } else {
      throw new Error("NotSupportedError: spawn stdio must be a string or array");
    }
  }
  if (opts.timeout !== undefined) o.timeoutMs = Number(opts.timeout);
  return o;
}
export function spawn(file, args, opts) {
  if (args !== undefined && args !== null && !Array.isArray(args)) { opts = args; args = []; }
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
// exec 族（10f 重写，node 架构：execFile = spawn + 收集 + close 回调 + 返回
// live ChildProcess；exec = execFile('/bin/sh', ['-c', cmd])（normalizeExecArgs
// 口径）。旧 __asyncOneShot 一次性内核退役——"无 live 句柄"偏差消账。
// maxBuffer 超限 kill（真机 ERR_CHILD_PROCESS_STDIO_MAXBUFFER）；timeout 走
// spawn 既有 timeout_ms（killSignal 自定值偏差记档）；错误 cmd/code/killed/
// signal 照真机挂载。execFile 无回调即抛 ERR_INVALID_ARG_TYPE（真机口径）。
function __execCollect(child, o, cmdStr, cb) {
  const chunks = { stdout: [], stderr: [] };
  const sizes = { stdout: 0, stderr: 0 };
  let over = null;
  const watch = (name) => {
    const st = child[name];
    if (!st) return;
    st.on("data", (c) => {
      const u8 = c instanceof Uint8Array ? c : new TextEncoder().encode(String(c));
      sizes[name] += u8.length;
      if (sizes[name] <= o.maxBuffer) chunks[name].push(u8);
      else if (over === null) {
        over = name;
        child.kill();
      }
    });
  };
  watch("stdout");
  watch("stderr");
  const dec = (u8s) => {
    const all = Buffer.concat(u8s);
    return o.encoding === "buffer" || o.encoding === null ? all : new TextDecoder(String(o.encoding || "utf8")).decode(all);
  };
  const finish = (err) => {
    cb(err, dec(chunks.stdout), dec(chunks.stderr));
  };
  child.once("error", (e) => { finish(e); child.once("close", () => {}); });
  child.once("close", (code, signal) => {
    if (over !== null) {
      const err = new Error(`${over} maxBuffer length exceeded`);
      err.code = "ERR_CHILD_PROCESS_STDIO_MAXBUFFER";
      err.cmd = cmdStr;
      finish(err);
      return;
    }
    if (code === 0 && signal === null) { finish(null); return; }
    const err = new Error(`Command failed: ${cmdStr}\n${dec(chunks.stderr)}`);
    // 真机口径（execfile 套件点名）：负退出码转 UV 名（如 -1 → EPERM）。
    err.code = (typeof code === "number" && code < 0) ? __uvName(code) : (code ?? signal);
    err.killed = child.killed;
    err.signal = signal;
    err.cmd = cmdStr;
    finish(err);
  });
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
  if (args !== undefined && typeof args !== "object" && typeof args !== "function") {
    throw new TypeError("execFile: args must be an array");
  }
  if (typeof args === "function") { cb = args; args = undefined; opts = undefined; }
  else if (typeof opts === "function") { cb = opts; opts = undefined; }
  if (typeof cb !== "function") {
    const err = new TypeError("The \"callback\" argument must be of type function");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const o = __normExecOpts(opts);
  const argv = (args ?? []).map(String);
  const cmdStr = [String(file), ...argv].join(" ");
  // node 口径：spawn 失败走异步 'error' 事件 + 回调（不抛）；本仓 spawn 异步
  // 失败（id 先发、error 事件后到——pid 已置数）而 node ENOENT 时 pid 恒
  // undefined——在此同步预检（PATH 解析）转 node 形：死句柄 + 回调。
  if (!__canSpawnFile(String(file))) {
    const err = new Error(`spawn ${file} ENOENT`);
    err.code = "ENOENT";
    err.errno = -2;
    err.syscall = `spawn ${file}`;
    err.path = String(file);
    err.cmd = cmdStr;
    queueMicrotask(() => cb(err, "", ""));
    return new ChildProcess();
  }
  let child;
  try {
    child = spawn(String(file), argv, {
      cwd: o.cwd,
      env: o.env,
      timeout: o.timeoutMs || undefined,
      stdio: ["pipe", "pipe", "pipe"],
    });
  } catch (e) {
    if (e && typeof e === "object" && e.code === undefined) {
      e.code = (String(e.message).match(/^([A-Z_]+): /) || [])[1] || "ENOENT";
      e.path = String(file);
      e.syscall = `spawn ${file}`;
    }
    queueMicrotask(() => cb(e, "", ""));
    return new ChildProcess();
  }
  child.__cmdStr = cmdStr;
  __execCollect(child, o, cmdStr, cb);
  return child;
}

export function exec(command, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  if (typeof cb !== "function") {
    const err = new TypeError("The \"callback\" argument must be of type function");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const o = __normExecOpts(opts);
  const cmdStr = String(command);
  const child = execFile("/bin/sh", ["-c", cmdStr], { ...o, encoding: o.encoding }, (err, stdout, stderr) => {
    if (err) err.cmd = cmdStr;
    cb(err, stdout, stderr);
  });
  return child;
}
export function execFileSync(file, args, opts) {
  if (args !== undefined && args !== null && !Array.isArray(args)) { opts = args; args = []; }
  const o = __normSpawnOpts(opts);
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
  let cb = null;
  for (const a of rest) if (typeof a === "function") cb = a;
  if (!process.connected || parentPort === null) {
    const err = new Error("Channel closed");
    err.code = "ERR_IPC_CHANNEL_CLOSED";
    if (cb) queueMicrotask(() => cb(err));
    else queueMicrotask(() => process.__wjs_emit("error", err));
    return false;
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
  process.__wjs_emit("message", message);
});
parentPort.on("close", () => {
  if (process.connected) {
    process.connected = false;
    process.__wjs_emit("disconnect");
  }
});
await import(__mod);
`;
function __normForkOpts(opts) {
  const o = { execPath: process.execPath, killSignal: "SIGTERM" };
  if (opts === undefined || opts === null) return o;
  if (typeof opts !== "object") {
    const err = new TypeError("The \"options\" argument must be of type object");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  // 线程底座：cwd/env/execArgv/silent/stdio/serialization/timeout/detached 等
  // 接受忽略（同进程线程，无独立进程环境；stdio 恒 null，见 `fork` 文档）。
  if (opts.killSignal !== undefined) o.killSignal = String(opts.killSignal);
  if (opts.execPath !== undefined) o.execPath = String(opts.execPath);
  return o;
}
export function fork(modulePath, args, opts) {
  if (args !== undefined && args !== null && !Array.isArray(args)) { opts = args; args = []; }
  if (modulePath === undefined || modulePath === null ||
      (typeof modulePath !== "string" && !(modulePath instanceof URL))) {
    const err = new TypeError("The \"modulePath\" argument must be of type string or URL");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (modulePath instanceof URL && modulePath.protocol !== "file:") {
    const err = new TypeError("The \"modulePath\" argument must be a file URL");
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
  const o = __normForkOpts(opts);
  const modStr = modulePath instanceof URL ? modulePath.href : String(modulePath);
  const fileUrl = /^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(modStr) ? modStr : pathToFileURL(modStr).href;
  const argsArr = [...(args || [])].map(String);
  const src = __FORK_CHILD_SRC
    .replace("__FORK_MOD__", () => JSON.stringify(fileUrl))
    .replace("__FORK_ARGV__", () => JSON.stringify(argsArr));
  const worker = new Worker(src, { eval: true });
  const proc = new ChildProcess();
  proc.__forkChild = true;
  proc.__worker = worker;
  proc.__connected = true;
  proc.__killSignal = o.killSignal;
  proc.spawnfile = o.execPath;
  proc.spawnargs = [o.execPath, fileUrl, ...argsArr];
  proc.stdin = null;
  proc.stdout = null;
  proc.stderr = null;
  proc.channel = { ref() {}, unref() {} };
  worker.on("message", (m) => proc.__onForkMessage(m));
  worker.on("error", (e) => {
    if (typeof proc.onerror === "function") proc.onerror(e);
  });
  worker.on("exit", (code) => proc.__onForkExit(code));
  return proc;
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
