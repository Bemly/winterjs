//! `process` 全局 + `node:process`（argv/env/cwd/exit/exitCode/stdio…）。
//! `exit()` 经 `__wjs_exit:<code>` 哨兵错 unwind（`runtime` 转 `Error::Exit`）；
//! 同时记 `process_exited` 旗，哨兵被用户 catch 也在检查点照退（文档记录）。

use std::sync::OnceLock;

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};
use crate::state;

/// 进程启动时刻（uptime/hrtime 基准）。
fn start() -> std::time::Instant {
    static T0: OnceLock<std::time::Instant> = OnceLock::new();
    *T0.get_or_init(std::time::Instant::now)
}

/// 字符串返回值（`os.rs` 同款小 helper，不跨模块引，保持单文件自洽）。
fn set_rval_str(cx: &mut mozjs::context::JSContext, frame: &Frame, s: &str) {
    rooted!(&in(cx) let mut v = UndefinedValue());
    s.to_jsval(cx, v.handle_mut());
    frame.set_rval(v.get());
}

/// `__wjs_argv_json()` → argv 数组 JSON。
pub unsafe extern "C" fn argv_json(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let json = state::with_plain(|p| serde_json::to_string(&p.argv).unwrap_or_else(|_| "[]".into()));
    set_rval_str(&mut cx, &frame, &json);
    true
}

/// `__wjs_env_get(k)` → 值串；缺失置 undefined。
pub unsafe extern "C" fn env_get(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: env needs a key");
        return false;
    }
    let key = value_to_string(&mut cx, frame.arg(0));
    match std::env::var_os(&key) {
        Some(v) => set_rval_str(&mut cx, &frame, &v.to_string_lossy()),
        None => frame.set_rval(UndefinedValue()),
    }
    true
}

/// `__wjs_env_set(k, v)`（`unsafe set_var`：JS 独占线程调用，见 SAFETY 内联注释）。
pub unsafe extern "C" fn env_set(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上（cx 构造）
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: env needs key and value");
        return false;
    }
    let (key, val) = (value_to_string(&mut cx, frame.arg(0)), value_to_string(&mut cx, frame.arg(1)));
    // SAFETY: 全进程环境表；写入只发生在 JS 独占线程，启动期配置读取早已完成，
    // 其余并发读（tokio 任务）与写不同 key；同 key 竞争语义与 Node 等价（后写赢）。
    unsafe { std::env::set_var(&key, &val) };
    frame.set_rval(UndefinedValue());
    true
}

/// `__wjs_env_del(k)`。
pub unsafe extern "C" fn env_del(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 env_set
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: env needs a key");
        return false;
    }
    let key = value_to_string(&mut cx, frame.arg(0));
    // SAFETY: 同上
    unsafe { std::env::remove_var(&key) };
    frame.set_rval(UndefinedValue());
    true
}

/// `__wjs_env_keys()` → 键数组 JSON。
pub unsafe extern "C" fn env_keys(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let keys: Vec<String> = std::env::vars_os().map(|(k, _)| k.to_string_lossy().into_owned()).collect();
    set_rval_str(&mut cx, &frame, &serde_json::to_string(&keys).unwrap_or_else(|_| "[]".into()));
    true
}

/// `__wjs_cwd()` → 当前目录（失败报，不吞）。
pub unsafe extern "C" fn cwd(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    match std::env::current_dir() {
        Ok(p) => {
            set_rval_str(&mut cx, &frame, &p.to_string_lossy());
            true
        }
        Err(e) => {
            report_error(&mut cx, &format!("OperationError: cannot get cwd: {e}"));
            false
        }
    }
}

/// `__wjs_chdir(dir)`（失败报）。
pub unsafe extern "C" fn chdir(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: chdir needs a directory");
        return false;
    }
    let dir = value_to_string(&mut cx, frame.arg(0));
    match std::env::set_current_dir(&dir) {
        Ok(()) => {
            frame.set_rval(UndefinedValue());
            true
        }
        Err(e) => {
            report_error(&mut cx, &format!("OperationError: cannot chdir: {e}"));
            false
        }
    }
}

/// `__wjs_process_exit(optCode)`：记旗 + 哨兵错 unwind（无参用 exitCode，无则 0）。
pub unsafe extern "C" fn process_exit(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let code = if frame.argc() > 0 && frame.arg(0).is_number() {
        frame.arg(0).to_number() as i32
    } else {
        state::exit_code().unwrap_or(0)
    };
    state::with_plain(|p| p.process_exited = Some(code));
    tracing::info!(target: "winterjs::process", code, "process.exit called");
    report_error(&mut cx, &format!("__wjs_exit:{code}"));
    false
}

/// `__wjs_exit_code_get()` → Int32（未设为 0）。
pub unsafe extern "C" fn exit_code_get(
    _cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 仅访问调用帧（无 cx 上的 JSAPI 调用）
    let frame = unsafe { Frame::from_raw(vp, argc) };
    frame.set_rval(mozjs::jsval::Int32Value(state::exit_code().unwrap_or(0)));
    true
}

/// `__wjs_exit_code_set(n)`（prelude 已校验整数）。
pub unsafe extern "C" fn exit_code_set(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 || !frame.arg(0).is_number() {
        report_error(&mut cx, "TypeError: exitCode needs a number");
        return false;
    }
    state::set_exit_code(frame.arg(0).to_number() as i32);
    frame.set_rval(UndefinedValue());
    true
}

/// `__wjs_exec_path()` → 可执行路径。
pub unsafe extern "C" fn exec_path(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "winterjs".into());
    set_rval_str(&mut cx, &frame, &exe);
    true
}

/// `__wjs_pid()` → Int32。
pub unsafe extern "C" fn pid(
    _cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 仅访问调用帧（无 cx 上的 JSAPI 调用）
    let frame = unsafe { Frame::from_raw(vp, argc) };
    frame.set_rval(mozjs::jsval::Int32Value(std::process::id() as i32));
    true
}

/// `__wjs_uptime()` → 启动至今秒（f64）。
pub unsafe extern "C" fn uptime(
    _cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 仅访问调用帧（无 cx 上的 JSAPI 调用）
    let frame = unsafe { Frame::from_raw(vp, argc) };
    frame.set_rval(mozjs::jsval::DoubleValue(start().elapsed().as_secs_f64()));
    true
}

/// `__wjs_hrtime_ns()` → 启动至今纳秒串（prelude 包 `BigInt`，避 BigInt FFI）。
pub unsafe extern "C" fn hrtime_ns(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    set_rval_str(&mut cx, &frame, &start().elapsed().as_nanos().to_string());
    true
}

/// `__wjs_memory_usage()` → `{rss, heapTotal: 0, heapUsed: 0, external: 0}` JSON。
/// 偏差：堆三数未接 SpiderMonkey GC 统计，恒 0（文档记录）；rss 经 sysinfo 实测。
pub unsafe extern "C" fn memory_usage(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let rss = sysinfo::get_current_pid()
        .ok()
        .and_then(|pid| sys.process(pid))
        .map(|p| p.memory())
        .unwrap_or(0);
    let json = serde_json::json!({
        "rss": rss,
        "heapTotal": 0,
        "heapUsed": 0,
        "external": 0,
        "arrayBuffers": 0,
    })
    .to_string();
    set_rval_str(&mut cx, &frame, &json);
    true
}

/// `__wjs_stdout_write(s)` → boolean（直写 fd，绕 `console` 通道）。
pub unsafe extern "C" fn stdout_write(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: stdout.write needs a string");
        return false;
    }
    let s = value_to_string(&mut cx, frame.arg(0));
    let ok = {
        use std::io::Write as _;
        std::io::stdout().write_all(s.as_bytes()).is_ok()
    };
    frame.set_rval(mozjs::jsval::BooleanValue(ok));
    true
}

/// `__wjs_stderr_write(s)` → boolean。
pub unsafe extern "C" fn stderr_write(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: stderr.write needs a string");
        return false;
    }
    let s = value_to_string(&mut cx, frame.arg(0));
    let ok = {
        use std::io::Write as _;
        std::io::stderr().write_all(s.as_bytes()).is_ok()
    };
    frame.set_rval(mozjs::jsval::BooleanValue(ok));
    true
}

/// `__wjs_stdio_istty(fd)` → boolean（fd: 1=out, 2=err）。
pub unsafe extern "C" fn stdio_istty(
    _cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 仅访问调用帧（无 cx 上的 JSAPI 调用）
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let fd = if frame.argc() > 0 && frame.arg(0).is_number() {
        frame.arg(0).to_number() as i32
    } else {
        1
    };
    let tty = if fd == 2 {
        std::io::IsTerminal::is_terminal(&std::io::stderr())
    } else {
        std::io::IsTerminal::is_terminal(&std::io::stdout())
    };
    frame.set_rval(mozjs::jsval::BooleanValue(tty));
    true
}

/// 启动期全局 `process`（`NODE_PRELUDE` 经 `runtime` 在主 PRELUDE 后求值）。
pub const PROCESS_PRELUDE: &str = r#"
globalThis.process = {
  argv: JSON.parse(__wjs_argv_json()),
  env: new Proxy({}, {
    get(_, k) {
      if (typeof k !== "string") return undefined;
      const v = __wjs_env_get(k);
      return v === undefined ? undefined : v;
    },
    set(_, k, v) { __wjs_env_set(String(k), String(v)); return true; },
    deleteProperty(_, k) { __wjs_env_del(String(k)); return true; },
    has(_, k) { return __wjs_env_get(String(k)) !== undefined; },
    ownKeys() { return JSON.parse(__wjs_env_keys()); },
    getOwnPropertyDescriptor(_, k) {
      const v = __wjs_env_get(String(k));
      if (v === undefined) return undefined;
      return { value: v, writable: true, enumerable: true, configurable: true };
    },
  }),
  cwd() { return __wjs_cwd(); },
  chdir(d) { __wjs_chdir(String(d)); },
  exit(code) { __wjs_process_exit(code === undefined ? undefined : Number(code)); },
  get exitCode() { return __wjs_exit_code_get(); },
  set exitCode(v) {
    const n = Number(v);
    if (!Number.isInteger(n)) throw new TypeError("process.exitCode must be an integer");
    __wjs_exit_code_set(n);
  },
  get platform() { return __wjs_os_platform(); },
  get arch() { return __wjs_os_arch(); },
  version: "v26.9.0",
  versions: { node: "22.0.0", winterjs: "26.9.0", mozjs: "153" },
  execPath: __wjs_exec_path(),
  pid: __wjs_pid(),
  uptime() { return __wjs_uptime(); },
  hrtime: Object.assign(
    (t) => {
      const now = BigInt(__wjs_hrtime_ns());
      if (t === undefined) {
        const s = now / 1000000000n;
        return [Number(s), Number(now - s * 1000000000n)];
      }
      const base = BigInt(t[0]) * 1000000000n + BigInt(t[1]);
      const d = now - base;
      return [Number(d / 1000000000n), Number(d % 1000000000n)];
    },
    { bigint: () => BigInt(__wjs_hrtime_ns()) },
  ),
  memoryUsage() { return JSON.parse(__wjs_memory_usage()); },
  stdout: { write(s) { return __wjs_stdout_write(String(s)); }, get isTTY() { return __wjs_stdio_istty(1); } },
  stderr: { write(s) { return __wjs_stderr_write(String(s)); }, get isTTY() { return __wjs_stdio_istty(2); } },
  nextTick(cb, ...args) {
    if (typeof cb !== "function") throw new TypeError("nextTick: callback must be a function");
    queueMicrotask(() => cb(...args));
  },
};
"#;

/// `node:process` 模块源（默认导出即全局 process，具名按需取）。
pub const SOURCE: &str = r#"
const p = globalThis.process;
export default p;
export const argv = p.argv;
export const env = p.env;
export const pid = p.pid;
export const platform = p.platform;
export const arch = p.arch;
export const version = p.version;
export const versions = p.versions;
export const execPath = p.execPath;
export function cwd() { return p.cwd(); }
export function chdir(d) { return p.chdir(d); }
export function exit(c) { return p.exit(c); }
export function uptime() { return p.uptime(); }
export function hrtime(t) { return p.hrtime(t); }
export function memoryUsage() { return p.memoryUsage(); }
export function nextTick(cb, ...args) { return p.nextTick(cb, ...args); }
export const stdout = p.stdout;
export const stderr = p.stderr;
"#;
