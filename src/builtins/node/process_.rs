//! `process` 全局 + `node:process`（argv/env/cwd/exit/exitCode/stdio…）。
//! `exit()` 经 `__wjs_exit:<code>` 哨兵错 unwind（`runtime` 转 `Error::Exit`）；
//! 同时记 `process_exited` 旗，哨兵被用户 catch 也在检查点照退（文档记录）。

use std::sync::OnceLock;

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::jsapi::JSObject;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{call_one, call_two, report_error, value_to_string, wrap_cx, Frame};
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

/// 身份族（10f：fs 套件 getuid()===0 守卫点名；libc 直查）。
macro_rules! ids_native {
    ($name:ident, $call:expr) => {
        pub unsafe extern "C" fn $name(
            cx_raw: *mut mozjs::jsapi::JSContext,
            argc: u32,
            vp: *mut JSVal,
        ) -> bool {
            // SAFETY: 引擎回调提供的 raw cx 有效
            let mut _cx = unsafe { wrap_cx(cx_raw) };
            let frame = unsafe { Frame::from_raw(vp, argc) };
            let _ = &mut _cx;
            let v: u32 = $call;
            frame.set_rval(mozjs::jsval::Int32Value(v as i32));
            true
        }
    };
}
ids_native!(getuid, unsafe { libc::getuid() });
ids_native!(getgid, unsafe { libc::getgid() });
ids_native!(geteuid, unsafe { libc::geteuid() });
ids_native!(getegid, unsafe { libc::getegid() });

/// `__wjs_process_getgroups()` → group id 数组（node 口径：缺 egid 即补）。
pub unsafe extern "C" fn getgroups(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let mut ids: Vec<i32> = Vec::new();
    #[cfg(unix)]
    {
        let n = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
        if n > 0 {
            let mut buf: Vec<libc::gid_t> = vec![0; n as usize];
            let n2 = unsafe { libc::getgroups(n, buf.as_mut_ptr()) };
            if n2 > 0 {
                ids.extend(buf[..n2 as usize].iter().map(|g| *g as i32));
            }
        }
        let egid = unsafe { libc::getegid() } as i32;
        if !ids.contains(&egid) {
            ids.push(egid);
        }
    }
    rooted!(&in(cx) let mut arr = UndefinedValue());
    ids.to_jsval(&mut cx, arr.handle_mut());
    frame.set_rval(arr.get());
    true
}

/// `__wjs_next_tick(cb, args)` → undefined：nextTick 入原生队列（pump 在
/// RunJobs 前后各收割一轮——node 口径 tick/微任务双层调度，10f stream 对拍）。
pub unsafe extern "C" fn next_tick_queue(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let _ = &mut cx;
    let (cb, args) = (frame.arg(0), frame.arg(1));
    state::with_rooted(|s| {
        s.next_ticks.push(state::NextTickEntry {
            cb: mozjs::jsapi::Heap::boxed(cb),
            args: mozjs::jsapi::Heap::boxed(args),
        });
    });
    frame.set_rval(UndefinedValue());
    true
}

/// 自然退出派发 process 'exit'（事件循环排空后；mustCall 结算点）。
/// 前置：cx 已进 global realm。异常一律吞（Node 口径：exit 监听抛错不改退出码）。
pub fn emit_exit(cx: &mut mozjs::context::JSContext, global: *mut JSObject) {
    use mozjs::conversions::ToJSValConvertible as _;
    use crate::jsapi_glue::{get_prop_value, set_prop_value};
    let Some(proc_v) = get_prop_value(cx, global, c"process") else {
        return;
    };
    if !proc_v.is_object() {
        return;
    }
    let proc_obj = proc_v.to_object();
    // _exiting = true（common.mustCall 在 exit 处理器内禁调，真机同）。
    rooted!(&in(cx) let mut flag_v = UndefinedValue());
    true.to_jsval(cx, flag_v.handle_mut());
    set_prop_value(cx, proc_obj, c"_exiting", flag_v.get());
    let Some(emit_v) = get_prop_value(cx, proc_obj, c"__wjs_emit") else {
        return;
    };
    if !emit_v.is_object() {
        return;
    }
    let code = state::exit_code().unwrap_or(0);
    rooted!(&in(cx) let mut code_v = UndefinedValue());
    (code as f64).to_jsval(cx, code_v.handle_mut());
    rooted!(&in(cx) let mut kind_v = UndefinedValue());
    "exit".to_jsval(cx, kind_v.handle_mut());
    let _ = call_two(cx, global, emit_v, kind_v.get(), code_v.get());
}

/// 收割 nextTick 原生队列（pump 专用：RunJobs 前后各一轮）。
/// 回调经 prelude `__wjs_call(cb, args)` 展开；抛错走 uncaughtException 路由
/// （有监听分发即吞，无监听保持 pending 走 fatal——fire_due 同款）。
/// **逐条摘取立即 rooting**（fire_due 同款纪律）：批内裸 JSVal 横跨回调即
/// 悬垂——回调可触发 GC（§4.80；实测 batch 形即 SIGSEGV）。node 语义核心：
/// 微任务期入队的 tick 必须**等整轮微任务排空**后才跑（V8 checkpoint 原子性）
/// ——queueMicrotask 同队列 FIFO 做不到，此即原生队列的存在理由
/// （compose/pipeline post-loop throw 全族对拍现形）。
pub fn drain_next_ticks(
    cx: &mut mozjs::context::JSContext,
    global: *mut JSObject,
    err: crate::runtime::ErrorSource<'_>,
) -> Result<(), crate::error::Error> {
    loop {
        let next = state::with_rooted(|s| {
            if s.next_ticks.is_empty() {
                None
            } else {
                Some(s.next_ticks.remove(0))
            }
        });
        let Some(entry) = next else { return Ok(()) };
        {
            // 条目已摘离队列（Box 定址随移动稳定），值先 rooted 再调
            rooted!(&in(cx) let cb_root = entry.cb.get());
            rooted!(&in(cx) let args_root = entry.args.get());
            drop(entry);
            let call_fn_v = state::with_rooted(|s| s.call_fn.get());
            if call_two(cx, global, call_fn_v, cb_root.get(), args_root.get()).is_none() {
                // 未捕获异常：Node 口径先探 process 'uncaughtException' 监听器；
                // 无监听保持 pending 原样走 fatal（错误信息/栈不降级）。
                let count_fn = state::with_rooted(|s| s.uncaught_count_fn.get());
                let count = call_one(cx, global, count_fn, UndefinedValue())
                    .and_then(|v| if v.is_number() { Some(v.to_number() as usize) } else { None })
                    .unwrap_or(0);
                let handled = if count > 0 {
                    match crate::jsapi_glue::take_pending_exception(cx) {
                        Some(err_v) => {
                            rooted!(&in(cx) let err_root = err_v);
                            let uncaught_fn = state::with_rooted(|s| s.uncaught_fn.get());
                            matches!(
                                call_two(cx, global, uncaught_fn, err_root.get(), UndefinedValue()),
                                Some(r) if r.is_boolean() && r.to_boolean()
                            )
                        }
                        None => false,
                    }
                } else {
                    false
                };
                if !handled {
                    return Err(match err {
                        crate::runtime::ErrorSource::Script { source, filename } => {
                            crate::jsapi_glue::pending_exception_error(cx, global, source, filename)
                        }
                        crate::runtime::ErrorSource::Module { url } => {
                            crate::modules::module_error(cx, url)
                        }
                    });
                }
            }
        }
    }
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
    if let Err(msg) = crate::permissions::check_env(&key) {
        report_error(&mut cx, &msg);
        return false;
    }
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
    if let Err(msg) = crate::permissions::check_env(&key) {
        report_error(&mut cx, &msg);
        return false;
    }
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
    if let Err(msg) = crate::permissions::check_env(&key) {
        report_error(&mut cx, &msg);
        return false;
    }
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
    if let Err(msg) = crate::permissions::check_env_keys() {
        report_error(&mut cx, &msg);
        return false;
    }
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
    // node 口径：exit 即终结（try 内调用不触发 catch）；哨兵是可抛 JS 值，
    // 用户 catch 吞掉后仍以后续 exit 覆盖——首个码赢（realpath-pipe 套件：
    // try{exit(2)}catch{exit(1)} 必须 rc=2）。
    state::with_plain(|p| {
        if p.process_exited.is_none() {
            p.process_exited = Some(code);
        }
    });
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

/// `__wjs_umask()` → 旧掩码 Int32；`__wjs_umask(mask)` 置新掩码并回旧值。
/// 10f：unix 经 libc 真改（test/common load 期置 0o22，fs 模式测试依赖）；
/// 非 unix 回 0o22 常量（记档）。
pub unsafe extern "C" fn umask(
    _cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 仅访问调用帧（无 cx 上的 JSAPI 调用）
    let frame = unsafe { Frame::from_raw(vp, argc) };
    #[cfg(unix)]
    {
        let set = frame.argc() >= 1 && frame.arg(0).is_number();
        let mask = if set { frame.arg(0).to_number() as u16 } else { 0 };
        // libc umask 回旧值：读时先取后恢复，两路都回调用前的值。
        //（10f 修：旧写法回的是第二次调用前的值，读恒得 0。）
        let prev = unsafe { libc::umask(if set { mask } else { 0 }) };
        if !set {
            unsafe { libc::umask(prev) };
        }
        frame.set_rval(mozjs::jsval::Int32Value(prev as i32));
        true
    }
    #[cfg(not(unix))]
    {
        let _ = argc;
        frame.set_rval(mozjs::jsval::Int32Value(0o22));
        true
    }
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

/// `__wjs_stdio_istty(fd)` → boolean（0=stdin，1=stdout，2=stderr；其余 false）。
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
    // 10c-1 勘误：旧实现把 fd 0 也按 stdout 查；其余 fd 按 stdout 回（应 false）。
    let tty = match fd {
        0 => std::io::IsTerminal::is_terminal(&std::io::stdin()),
        1 => std::io::IsTerminal::is_terminal(&std::io::stdout()),
        2 => std::io::IsTerminal::is_terminal(&std::io::stderr()),
        _ => false,
    };
    frame.set_rval(mozjs::jsval::BooleanValue(tty));
    true
}

/// 启动期全局 `process`（`NODE_PRELUDE` 经 `runtime` 在主 PRELUDE 后求值）。
pub const PROCESS_PRELUDE: &str = r#"
// stdout/stderr 造形（10f stream 对拍）：node Socket 形——写直通 fd + EE 全表面
//（on/once/off/addListener/prependListener/removeAllListeners/listenerCount/
// listeners/emit/end/destroy）。写完成回调 microtask 异步回（§4.74）。
function __wjs_stdio_stream(fd) {
  return {
    __wjs_fd: fd,
    write(s, ...rest) {
      const r = fd === 1 ? __wjs_stdout_write(String(s)) : __wjs_stderr_write(String(s));
      const cb = rest.find((a) => typeof a === "function");
      if (cb) queueMicrotask(() => cb());
      return r;
    },
    get isTTY() { return __wjs_stdio_istty(fd); },
    clearLine() { return __wjs_stdio_istty(fd); },
    cursorTo() { return __wjs_stdio_istty(fd); },
    getColorDepth() { return __wjs_stdio_istty(fd) ? 8 : 1; },
    __wjs_listeners: {},
    on(type, cb) {
      if (typeof cb !== "function") throw new TypeError("stdio.on: listener must be a function");
      (this.__wjs_listeners[String(type)] ??= []).push(cb);
      return this;
    },
    addListener(type, cb) { return this.on(type, cb); },
    once(type, cb) {
      const self = this;
      const wrapped = (...a) => { self.off(type, wrapped); cb(...a); };
      wrapped.__wjs_orig = cb;
      return self.on(type, wrapped);
    },
    prependListener(type, cb) {
      if (typeof cb !== "function") throw new TypeError("stdio.prependListener: listener must be a function");
      (this.__wjs_listeners[String(type)] ??= []).unshift(cb);
      return this;
    },
    off(type, cb) {
      const list = this.__wjs_listeners[String(type)];
      if (list) {
        let i = list.findIndex((l) => l === cb || l.__wjs_orig === cb);
        while (i >= 0) { list.splice(i, 1); i = list.findIndex((l) => l === cb || l.__wjs_orig === cb); }
      }
      return this;
    },
    removeListener(type, cb) { return this.off(type, cb); },
    removeAllListeners(type) {
      if (type === undefined) this.__wjs_listeners = {};
      else delete this.__wjs_listeners[String(type)];
      return this;
    },
    listenerCount(type) { return (this.__wjs_listeners[String(type)] || []).length; },
    listeners(type) { return (this.__wjs_listeners[String(type)] || []).slice(); },
    emit(type, ...args) {
      const list = (this.__wjs_listeners[String(type)] || []).slice();
      for (const l of list) l(...args);
      return list.length > 0;
    },
    end(...rest) {
      const cb = rest.find((a) => typeof a === "function");
      if (cb) queueMicrotask(() => cb());
      return this;
    },
    destroy() { return this; },
    __wjs_maxListeners: 10,
    getMaxListeners() { return this.__wjs_maxListeners; },
    setMaxListeners(n) { this.__wjs_maxListeners = Number(n); return this; },
  };
}

globalThis.process = {
  argv: JSON.parse(__wjs_argv_json()),
  // 真机口径：argv0 缺省即 argv[0]（spawn-argv0 套件点名自举回显）。
  argv0: JSON.parse(__wjs_argv_json())[0] ?? __wjs_exec_path(),
  env: (() => {
    // 10f 对拍：worker 会话带 env 快照（创建时复制或自定义对象）——读写全落
    // 本地 store，不碰进程级 env（process-env 套件隔离/快照断言）；主会话与
    // SHARE_ENV 会话走真 env native（原语义不变）。
    const snap = __wjs_worker_env_snapshot();
    if (snap === undefined) {
      return new Proxy({}, {
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
      });
    }
    const store = JSON.parse(snap);
    return new Proxy({}, {
      get(_, k) { return typeof k === "string" ? store[k] : undefined; },
      set(_, k, v) { store[k] = String(v); return true; },
      deleteProperty(_, k) { delete store[k]; return true; },
      has(_, k) { return typeof k === "string" && k in store; },
      ownKeys() { return Object.keys(store); },
      getOwnPropertyDescriptor(_, k) {
        if (typeof k !== "string" || !(k in store)) return undefined;
        return { value: store[k], writable: true, enumerable: true, configurable: true };
      },
      // node 口径：env 只收 configurable+writable+enumerable 齐备的数据描述符
      //（process-env 套件 defineProperty {value:42} 即抛，message 逐字）。
      defineProperty(_, k, desc) {
        if (desc !== null && typeof desc === "object" &&
            desc.writable === true && desc.enumerable === true && desc.configurable === true &&
            !("get" in desc) && !("set" in desc)) {
          store[k] = String(desc.value);
          return true;
        }
        const e = new TypeError("'process.env' only accepts a configurable, writable, and enumerable data descriptor");
        e.code = "ERR_INVALID_OBJECT_DEFINE_PROPERTY";
        throw e;
      },
    });
  })(),
  cwd() { return __wjs_cwd(); },
  chdir(d) { __wjs_chdir(String(d)); },
  exit(code) {
    // node 口径：'exit' 监听同步派发后再 unwind（mustCall 计数在监听内结算；
    // _exiting 置位，监听内再 mustCall 即抛，真机同）。
    this._exiting = true;
    try { this.__wjs_emit("exit", code === undefined ? (this.exitCode || 0) : Number(code)); } catch {}
    __wjs_process_exit(code === undefined ? undefined : Number(code));
  },
  // node 口径：退出中标志（common.mustCall 在 exit 处理器内禁调；真机 process._exiting）。
  // 本仓 exit 经哨兵错 unwind：设旗后抛，'exit' 监听在 unwind 前同步派发（见下）。
  _exiting: false,
  // 存活句柄表（assert-leaks 套件：`process._getActiveHandles()` 数组；
  // 本仓收录 watch 句柄（fs 侧登记/摘除），其余底座另案记档）。
  _getActiveHandles() { return [...(globalThis.__wjsFsHandles ?? [])]; },
  get exitCode() { return __wjs_exit_code_get(); },
  set exitCode(v) {
    const n = Number(v);
    if (!Number.isInteger(n)) throw new TypeError("process.exitCode must be an integer");
    __wjs_exit_code_set(n);
  },
  get platform() { return __wjs_os_platform(); },
  get arch() { return __wjs_os_arch(); },
  version: "v26.9.13",
  // versions.node = Node API 兼容水位（Bun 同哲学：process.version 是自家版本，
  // versions.node 报兼容等级）。22.12 = vite 8 的最低地板（22 && minor>=12），
  // 22.x 大版本保 `^22` caret 区间可用；22.0.0 过不了 vite checkNodeVersion。
  // openssl/sqlite 为兼容水位（套件门控 `hasCrypto/hasSQLite` 用；TLS 底座实为
  // rustls/ring、DB 实为 turso，引擎差异见模块头注；10f 跑 test/common 前置）。
  versions: { node: "22.12.0", winterjs: "26.9.13", mozjs: "153", openssl: "3.6.4", sqlite: "3.53.4" },
  // 构建配置（10f 跑 test/common 前置；键集按套件读取面收敛，非全量 115 键）。
  config: {
    target_defaults: { default_configuration: "Release" },
    variables: {
      asan: 0,
      node_shared: false,
      node_use_ffi: false,
      v8_enable_i18n_support: 1,
      v8_enable_temporal_support: 1,
      v8_use_perfetto: false,
    },
  },
  // 特性门控（套件 hasInspector/hasQuic 等用；inspector 本仓为薄层故 false，
  // quic 真机 26 亦 false；10f 前置）。
  features: {
    inspector: false, debug: false, uv: true, ipv6: true,
    tls: true, tls_alpn: true, tls_sni: true, tls_ocsp: true,
    cached_builtins: true, require_module: true, quic: false,
  },
  execPath: __wjs_exec_path(),
  // node 选项透传（M5 vitest 牵引：本仓无 node 旗标，恒空数组，真机口径）。
  execArgv: [],
  pid: __wjs_pid(),
  // 文件创建掩码（10f：读无参回当前，置数回旧值；真机口径）。
  umask(mask) {
    if (mask === undefined) return __wjs_umask();
    return __wjs_umask(Number(mask));
  },
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
  // Node 22.3+（vite 用 getBuiltinModule('node:module').Module 做互操作）；
  // 裸名（'module'）与 'node:module' 双形均收（Node 口径），非内置走 require
  // 的可读报错；require 的 ESM-default 口径（node:module default 导出带 Module 类）。
  getBuiltinModule(id) {
    const spec = String(id);
    return globalThis.require(spec.startsWith("node:") ? spec : `node:${spec}`);
  },
  // stdout/stderr 富流（真 node 是 Socket；10f 起 helper 造形：直写 fd +
  // EE 全表面——pipe 的 dest.on/emit('pipe')/close/finish 登记接得住；
  // 事件面空转（无 data/end 发射）偏差记档）。clearLine/cursorTo/getColorDepth
  // 非 TTY no-op（vite dev；TTY 下调用方自写 ANSI）。stdin：监听登记 +
  // isTTY + EOF read()（偏差记档：stdin EOF/data 不投递、信号不投递——
  // 注册表只收不发，SIGTERM 默认行为不变（OS 默认终止））。
  stdout: __wjs_stdio_stream(1),
  stderr: __wjs_stdio_stream(2),
  stdin: {
    get isTTY() { return __wjs_stdio_istty(0); },
    __wjs_listeners: {},
    on(type, cb) {
      if (typeof cb !== "function") throw new TypeError("stdin.on: listener must be a function");
      (this.__wjs_listeners[String(type)] ??= []).push(cb);
      return this;
    },
    once(type, cb) { return this.on(type, cb); },
    off(type, cb) {
      const list = this.__wjs_listeners[String(type)];
      if (list) {
        const i = list.indexOf(cb);
        if (i >= 0) list.splice(i, 1);
      }
      return this;
    },
    removeListener(type, cb) { return this.off(type, cb); },
    read() { return null; },
    pause() { return this; },
    resume() { return this; },
    setRawMode() { return this; },
    unref() { return this; },
    ref() { return this; },
  },
  getuid() { return __wjs_process_getuid(); },
  getgid() { return __wjs_process_getgid(); },
  geteuid() { return __wjs_process_geteuid(); },
  getegid() { return __wjs_process_getegid(); },
  getgroups() { return __wjs_process_getgroups(); },
  nextTick(cb, ...args) {
    if (typeof cb !== "function") throw new TypeError("nextTick: callback must be a function");
    // 原生队列（node 口径）：tick 由 pump 在 RunJobs 前后收割——同步期入队的
    // tick 先于微任务、微任务期入队的等整轮微任务排空（V8 checkpoint 原子性）。
    // 回调抛错经 drain 侧 uncaughtException 路由（destroy/emitErrorNT 等内建
    // 全走 nextTick，throw 落成 rejection 即全族套件反红）。
    __wjs_next_tick(cb, args);
  },
  // Phase 9a（node:events MaxListenersExceededWarning 路径）：warning 监听 + emitWarning。
  // Node 语义收敛：string → 包 Error（name=type||'Warning'，code/detail 挂载）；
  // Error 原样；第二参可 string（type）或 { type, code, detail }；有监听走监听，
  // 否则 stderr 默认打印 `(node:<pid>) [code] Name: message`。
  __wjs_warningListeners: [],
  // 通用监听表（warning 沿旧径；signal/stdin 等只登记不投递——偏差记档，
  // SIGTERM 默认行为不变）。emit 供未来事件循环接信号投递。
  // 方法一律走 `this`（套件 process-tampering：node common 载入期捕获
  // `const process = globalThis.process`，之后全局被换也不经它读表）。
  __wjs_listeners: {},
  on(type, cb) {
    if (type === "warning" && typeof cb === "function") this.__wjs_warningListeners.push(cb);
    if (typeof cb !== "function") throw new TypeError("process.on: listener must be a function");
    (this.__wjs_listeners[String(type)] ??= []).push(cb);
    return this;
  },
  once(type, cb) {
    if (typeof cb !== "function") throw new TypeError("process.once: listener must be a function");
    const self = this;
    const wrapped = (...args) => { self.off(type, wrapped); cb(...args); };
    wrapped.__wjs_orig = cb;
    return self.on(type, wrapped);
  },
  off(type, cb) {
    const list = this.__wjs_listeners[String(type)];
    if (list) {
      let i = list.findIndex((l) => l === cb || l.__wjs_orig === cb);
      while (i >= 0) { list.splice(i, 1); i = list.findIndex((l) => l === cb || l.__wjs_orig === cb); }
    }
    return this;
  },
  removeListener(type, cb) { return this.off(type, cb); },
  // node process 即 EventEmitter（套件 promises-scheduler：process.addListener/
  // process.emit 直用）；emit 返回是否命中监听（node 口径）。
  addListener(type, cb) { return this.on(type, cb); },
  emit(type, ...args) { return this.__wjs_emit(type, ...args) > 0; },
  removeAllListeners(type) {
    if (type === undefined) this.__wjs_listeners = {};
    else delete this.__wjs_listeners[String(type)];
    return this;
  },
  listenerCount(type) { return (this.__wjs_listeners[String(type)] ?? []).length; },
  // EventEmitter 读表（M5 vitest 牵引：init 链 `process.listeners(..).bind(..)`）。
  listeners(type) { return [...(this.__wjs_listeners[String(type)] ?? [])]; },
  rawListeners(type) { return this.listeners(type); },
  eventNames() { return Object.keys(this.__wjs_listeners); },
  __wjs_emit(type, ...args) {
    const list = [...(this.__wjs_listeners[String(type)] ?? [])];
    for (const l of list) {
      try { l.call(this, ...args); } catch {}
    }
    return list.length;
  },
  emitWarning(warning, typeOrOptions, code, _ctor) {
    let type, detail;
    if (typeof typeOrOptions === "object" && typeOrOptions !== null) {
      type = typeOrOptions.type; code = typeOrOptions.code; detail = typeOrOptions.detail;
    } else {
      type = typeOrOptions;
    }
    if (typeof warning === "string") {
      warning = new Error(warning);
      warning.name = String(type || "Warning");
      if (code) warning.code = String(code);
      if (detail) warning.detail = String(detail);
    } else if (warning !== null && typeof warning === "object") {
      if (type && !warning.name) warning.name = String(type);
      if (code && !warning.code) warning.code = String(code);
    } else {
      throw new TypeError("warning must be a string or an Error");
    }
    // node 口径：warning 异步派发（nextTick）——emitWarning 同步返回后
    // 调用方才挂 'warning' 监听（套件"先 parse 后 expectWarning"的时序
    // 依赖此，10f url DEP0169 现形）；§4.74 同源教训。
    queueMicrotask(() => {
      const listeners = this.__wjs_warningListeners;
      if (listeners.length > 0) {
        for (const l of listeners) {
          try { l.call(this, warning); } catch {}
        }
      } else {
        const codePart = warning.code ? `[${warning.code}] ` : "";
        const line = `(node:${__wjs_pid()}) ${codePart}${warning.name}: ${warning.message}`;
        __wjs_stderr_write(line + "\n");
        if (warning.detail) __wjs_stderr_write(warning.detail + "\n");
      }
    });
  },
};
// 真机口径：process[Symbol.toStringTag] = "process"（不可枚举，实测 getter 面），
// String(process) → '[object process]'（vm basic 套件 / util.inspect 点名）。
Object.defineProperty(globalThis.process, Symbol.toStringTag, { value: "process" });
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
export const config = p.config;
export const features = p.features;
export const execPath = p.execPath;
export const execArgv = p.execArgv;
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
