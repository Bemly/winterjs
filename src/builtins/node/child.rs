//! `node:child_process` 同步子集（`execSync`/`spawnSync`；`std::process` 阻塞跑）。
//! 超时只杀直系子进程（孙进程组杀树顺延 4d，文档记录）；shell 拼串经 `shlex::quote`。
//! 结果走 JSON 桥（二进制 base64）；错误形状由 prelude 组装（`.status/.signal/`…
//! 见 SOURCE）。

use std::io::{Read as _, Write as _};
use std::time::{Duration, Instant};

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};

/// spawn 选项（prelude 传 JSON；`None` 表缺省）。
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct SpawnOpts {
    cwd: Option<String>,
    /// 全量替换环境（None=继承；`{}`=清空，Node 同语义）。
    env: Option<std::collections::HashMap<String, String>>,
    /// 超时毫秒（0/缺省=无限；超时杀直系，`signal="SIGKILL"`）。
    timeout_ms: u64,
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
            // 超时杀直系（组杀顺延 4d；见头注）。
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

/// `__wjs_cp_spawn(fileStr, argsJson, optsJson)` → 结果 JSON。
/// `shell:true` 时经 `shlex::quote` 拼串（unix；win 按空格拼，文档记录）。
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
export default { execSync, spawnSync };
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
                let mut c = std::process::Command::new("definitely-missing-binary-xyz");
                c
            },
            &SpawnOpts::default(),
            None,
        );
        assert!(out["spawnErr"].as_str().unwrap_or("").contains("ENOENT"));
        assert!(out["status"].is_null());
    }
}
