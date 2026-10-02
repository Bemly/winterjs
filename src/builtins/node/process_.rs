//! `process` 全局 + `node:process`（argv/env/cwd/exit/exitCode/stdio…）。
//! `exit()` 经 `__wjs2_exit:<code>` 哨兵错 unwind（`runtime` 转 `Error::Exit`）；
//! 同时记 `process_exited` 旗，哨兵被用户 catch 也在检查点照退（文档记录）。

use std::sync::OnceLock;

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::jsapi::JSObject;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{call_one, call_two, report_error, value_to_string, wrap_cx, Frame};
use crate::state;

pub use super::process_prelude::PROCESS_PRELUDE;

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

/// `__wjs2_process_getgroups()` → group id 数组（node 口径：缺 egid 即补）。
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

/// `__wjs2_next_tick(cb, args)` → undefined：nextTick 入原生队列（pump 在
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
    let Some(emit_v) = get_prop_value(cx, proc_obj, c"__wjs2_emit") else {
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
    // this 必须是 process：`__wjs2_emit` 读 `this.__wjs2_listeners`（§4.97 this 基）。
    // 修前以 global 为 this 调，取表即 TypeError 被吞——自然退出的 'exit' 监听从不触发，
    // node 套件 common 的 mustCall 退出核对形同虚设（假绿源，§4.126③）。
    rooted!(&in(cx) let proc_root: *mut JSObject = proc_obj);
    rooted!(&in(cx) let emit_root = emit_v);
    let _ = global;
    let _ = call_two(cx, proc_root.get(), emit_root.get(), kind_v.get(), code_v.get());
}

/// 事件循环排空：投递 process 'beforeExit'（JS 侧 nextTick 排队，下一轮 pump 派发）。
/// 返回是否已投递（无监听 / process 缺失 → false，调用方直接收尾）。前置：cx 已进 global realm。
pub fn queue_before_exit(cx: &mut mozjs::context::JSContext, global: *mut JSObject) -> bool {
    use crate::jsapi_glue::get_prop_value;
    let Some(proc_v) = get_prop_value(cx, global, c"process") else {
        return false;
    };
    if !proc_v.is_object() {
        return false;
    }
    rooted!(&in(cx) let proc_root: *mut JSObject = proc_v.to_object());
    let Some(f) = get_prop_value(cx, proc_root.get(), c"__wjs2_queueBeforeExit") else {
        return false;
    };
    if !f.is_object() {
        return false;
    }
    rooted!(&in(cx) let f_root = f);
    matches!(
        call_one(cx, proc_root.get(), f_root.get(), UndefinedValue()),
        Some(r) if r.is_boolean() && r.to_boolean()
    )
}

/// 收割 nextTick 原生队列（pump 专用：RunJobs 前后各一轮）。
/// 回调经 prelude `__wjs2_call(cb, args)` 展开；抛错走 uncaughtException 路由
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

/// `__wjs2_argv_json()` → argv 数组 JSON。
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

/// Node 兼容旗记录（CLI 解析前剥下的 node 运行时旗，见 `cli::strip_node_compat_args`）。
/// 用途：execArgv 保真 + 语义旗（insecure-http-parser/expose-gc）按需生效。
/// 进程级静态：真子进程（OS 级）记录为空；同进程线程会话共享父记录（记档）。
fn node_compat_store() -> &'static std::sync::Mutex<Vec<String>> {
    static STORE: OnceLock<std::sync::Mutex<Vec<String>>> = OnceLock::new();
    STORE.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// CLI 起点调用一次（main，引擎启动前；单线程）。
pub fn record_node_compat(flags: Vec<String>) {
    if let Ok(mut m) = node_compat_store().lock() {
        *m = flags;
    }
}

/// `__wjs2_node_compat_json()` → 剥下的 node 兼容旗 JSON 数组（execArgv 底座）。
///
/// UNSAFE-BOUNDARY: 前置——引擎回调 cx 有效（调用约定）；覆盖测试——
/// `process_::tests::node_compat_json_empty`（零参）+ 黑盒 execArgv 回显。
pub unsafe extern "C" fn node_compat_json(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let json = node_compat_store()
        .lock()
        .ok()
        .and_then(|m| serde_json::to_string(&*m).ok())
        .unwrap_or_else(|| "[]".into());
    set_rval_str(&mut cx, &frame, &json);
    true
}

/// `__wjs2_env_get(k)` → 值串；缺失置 undefined。
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
    // 自 spawn 深度闸变量属宿主管线，对 JS 不可见（pitfalls 4.209）。
    match std::env::var_os(&key).filter(|_| key != crate::builtins::node::child::SELF_SPAWN_ENV) {
        Some(v) => set_rval_str(&mut cx, &frame, &v.to_string_lossy()),
        None => frame.set_rval(UndefinedValue()),
    }
    true
}

/// `__wjs2_env_set(k, v)`（`unsafe set_var`：JS 独占线程调用，见 SAFETY 内联注释）。
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

/// `__wjs2_env_del(k)`。
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

/// `__wjs2_env_keys()` → 键数组 JSON。
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
    let keys: Vec<String> = std::env::vars_os()
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .filter(|k| k != crate::builtins::node::child::SELF_SPAWN_ENV)
        .collect();
    set_rval_str(&mut cx, &frame, &serde_json::to_string(&keys).unwrap_or_else(|_| "[]".into()));
    true
}

/// `__wjs2_cwd()` → 当前目录（失败报，不吞）。
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

/// `__wjs2_chdir(dir)`（失败报）。
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

/// `__wjs2_process_exit(optCode)`：记旗 + 哨兵错 unwind（无参用 exitCode，无则 0）。
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
    tracing::info!(target: "winterjs2::process", code, "process.exit called");
    report_error(&mut cx, &format!("__wjs2_exit:{code}"));
    false
}

/// `__wjs2_exit_code_get()` → Int32（未设为 0）。
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

/// `__wjs2_exit_code_set(n)`（prelude 已校验整数）。
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

/// `__wjs2_exec_path()` → 可执行路径。
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
        .unwrap_or_else(|_| "winterjs2".into());
    set_rval_str(&mut cx, &frame, &exe);
    true
}

/// `__wjs2_pid()` → Int32。
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

/// `__wjs2_umask()` → 旧掩码 Int32；`__wjs2_umask(mask)` 置新掩码并回旧值。
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

/// `__wjs2_uptime()` → 启动至今秒（f64）。
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

/// `__wjs2_hrtime_ns()` → 启动至今纳秒串（prelude 包 `BigInt`，避 BigInt FFI）。
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

/// `__wjs2_memory_usage()` → `{rss, heapTotal: 0, heapUsed: 0, external: 0}` JSON。
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

/// `__wjs2_process_abort()`：SIGABRT 即死（node 口径；JS 侧为箭头函数，
/// 无 prototype，`new` 即 TypeError——见 process_prelude）。
pub unsafe extern "C" fn process_abort(
    _cx_raw: *mut mozjs::jsapi::JSContext,
    _argc: u32,
    _vp: *mut JSVal,
) -> bool {
    std::process::abort()
}

/// `__wjs2_available_memory()` → 可用内存字节（f64；经 sysinfo 真值）。
pub unsafe extern "C" fn available_memory(
    _cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 仅访问调用帧（无 cx 上的 JSAPI 调用）
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    frame.set_rval(mozjs::jsval::DoubleValue(sys.available_memory() as f64));
    true
}

/// `__wjs2_constrained_memory()` → 受限内存字节（f64；cgroup 上限不可读时回
/// 物理总量——node 无约束回 undefined，但套件要求 number，见黑盒注记）。
pub unsafe extern "C" fn constrained_memory(
    _cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    frame.set_rval(mozjs::jsval::DoubleValue(sys.total_memory() as f64));
    true
}

/// rusage 取本进程用户/系统微秒（f64 二元；失败回零）。
#[cfg(unix)]
fn rusage_self_micros() -> (f64, f64) {
    // SAFETY: rusage 出参为栈上结构体指针，getrusage 同步写入后即读，无别名。
    let mut r: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut r) } != 0 {
        return (0.0, 0.0);
    }
    let tv = |t: libc::timeval| t.tv_sec as f64 * 1e6 + t.tv_usec as f64;
    (tv(r.ru_utime), tv(r.ru_stime))
}

/// `__wjs2_cpu_usage()` → `{user, system}` 微秒 JSON（node 口径真值）。
pub unsafe extern "C" fn cpu_usage(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    #[cfg(unix)]
    let (user, system) = rusage_self_micros();
    #[cfg(not(unix))]
    let (user, system) = (0.0, 0.0);
    let json = serde_json::json!({ "user": user, "system": system }).to_string();
    set_rval_str(&mut cx, &frame, &json);
    true
}

/// 本线程用户/系统微秒（macOS 经 thread_info 真值；Linux 经 RUSAGE_THREAD；
/// 其余回零并记档——Windows thread 面另案）。
// §6 三问：① libc 0.2 未导出 mach_port_deallocate（无 safe/现成可用），
// ② 收敛在本函数内、对外只暴露 (f64, f64)，③ 前置见 SAFETY 内联注释。
#[cfg(target_vendor = "apple")]
unsafe extern "C" {
    fn mach_port_deallocate(task: libc::mach_port_t, name: libc::mach_port_t) -> libc::kern_return_t;
}
#[cfg(target_vendor = "apple")]
fn thread_micros() -> (f64, f64) {
    // SAFETY: mach port 由 mach_thread_self 当场取得、用后即 deallocate；
    // thread_info 同步写入栈上 info，无别名；flavor/count 均为常量口径。
    unsafe {
        let port = libc::mach_thread_self();
        let mut info: libc::thread_basic_info = std::mem::zeroed();
        let mut count = libc::THREAD_BASIC_INFO_COUNT;
        let kr = libc::thread_info(
            port,
            libc::THREAD_BASIC_INFO as u32,
            &mut info as *mut _ as libc::thread_info_t,
            &mut count,
        );
        mach_port_deallocate(libc::mach_task_self(), port);
        if kr != libc::KERN_SUCCESS {
            return (0.0, 0.0);
        }
        let tv = |t: libc::time_value_t| t.seconds as f64 * 1e6 + t.microseconds as f64;
        (tv(info.user_time), tv(info.system_time))
    }
}
#[cfg(all(unix, not(target_vendor = "apple")))]
fn thread_micros() -> (f64, f64) {
    // SAFETY: 同 rusage_self_micros（Linux RUSAGE_THREAD）。
    let mut r: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_THREAD, &mut r) } != 0 {
        return (0.0, 0.0);
    }
    let tv = |t: libc::timeval| t.tv_sec as f64 * 1e6 + t.tv_usec as f64;
    (tv(r.ru_utime), tv(r.ru_stime))
}
#[cfg(not(unix))]
fn thread_micros() -> (f64, f64) {
    (0.0, 0.0)
}

/// `__wjs2_thread_cpu_usage()` → `{user, system}` 微秒 JSON（本线程真值）。
pub unsafe extern "C" fn thread_cpu_usage(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 cpu_usage
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (user, system) = thread_micros();
    let json = serde_json::json!({ "user": user, "system": system }).to_string();
    set_rval_str(&mut cx, &frame, &json);
    true
}

/// `__wjs2_stdout_write(s)` → boolean（直写 fd，绕 `console` 通道）。
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
        // Rust Stdout 块缓冲（管道时无换行即滞留，常驻进程输出永不到）——
        // Node 写无缓冲，逐次 flush（kill 套件：子进程 write('x') 后等 stdin，
        // 父收不到即双边 hang；exit 即刷才掩盖了它）。
        let mut out = std::io::stdout();
        out.write_all(s.as_bytes()).is_ok() && out.flush().is_ok()
    };
    frame.set_rval(mozjs::jsval::BooleanValue(ok));
    true
}

/// `__wjs2_stderr_write(s)` → boolean。
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
        // stderr 恒无缓冲，flush 为对称 no-op（与 stdout 同形，免后人误抄回旧形）。
        let mut out = std::io::stderr();
        out.write_all(s.as_bytes()).is_ok() && out.flush().is_ok()
    };
    frame.set_rval(mozjs::jsval::BooleanValue(ok));
    true
}

/// `__wjs2_stdio_istty(fd)` → boolean（0=stdin，1=stdout，2=stderr；其余 false）。
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

/// `__wjs2_stdin_poll()` → `"D"+b64（有数据）/ `"E"`（EOF 或 fd 坏）/ `""`（暂无）。
/// 非阻塞读 fd 0（首调置 O_NONBLOCK，幂等；读写经 nix safe 封装）。
/// 唯一 unsafe 表达式是 `BorrowedFd::borrow_raw(0)`：fd 0 为进程生命期
/// 标准输入，本仓永不关闭它；若宿主关了它，fcntl/read 只回 EBADF（→"E"），
/// 不解引用，只是整数传递，无实际 UB 风险。
/// JS 侧 setInterval 轮询（refed，EOF 自停）——无运行时改动，供 stdin data/end。
pub unsafe extern "C" fn stdin_poll(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效（仅帧/rval 操作）
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    static ARMED: std::sync::Once = std::sync::Once::new();
    // SAFETY: fd 0 为进程生命期标准输入（见函数头注）；仅整数传递给
    // 要求 AsFd 的 nix 封装，失败只回 errno，不解引用。
    let fd = unsafe { std::os::fd::BorrowedFd::borrow_raw(0) };
    ARMED.call_once(|| {
        use nix::fcntl::{fcntl, FcntlArg, OFlag};
        // 取出现有 flags 后或入 O_NONBLOCK（只改本进程 fd 0；子进程继承见文档）。
        if let Ok(flags) = fcntl(fd, FcntlArg::F_GETFL) {
            let flags = OFlag::from_bits_retain(flags) | OFlag::O_NONBLOCK;
            let _ = fcntl(fd, FcntlArg::F_SETFL(flags));
        }
    });
    let mut buf = [0u8; 65536];
    match nix::unistd::read(fd, &mut buf) {
        Ok(0) => set_rval_str(&mut cx, &frame, "E"),
        Ok(n) => {
            // base64 小 helper（避开额外依赖；b64 字母表内建）。
            const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut s = String::with_capacity((n + 2) / 3 * 4 + 1);
            s.push('D');
            for c in buf[..n].chunks(3) {
                let b0 = c[0] as u32;
                let b1 = *c.get(1).unwrap_or(&0) as u32;
                let b2 = *c.get(2).unwrap_or(&0) as u32;
                let v = (b0 << 16) | (b1 << 8) | b2;
                s.push(B64[((v >> 18) & 63) as usize] as char);
                s.push(B64[((v >> 12) & 63) as usize] as char);
                s.push(if c.len() > 1 { B64[((v >> 6) & 63) as usize] as char } else { '=' });
                s.push(if c.len() > 2 { B64[(v & 63) as usize] as char } else { '=' });
            }
            set_rval_str(&mut cx, &frame, &s);
        }
        Err(e) if e == nix::errno::Errno::EAGAIN || e == nix::errno::Errno::EWOULDBLOCK => {
            set_rval_str(&mut cx, &frame, "")
        }
        Err(_) => set_rval_str(&mut cx, &frame, "E"),
    }
    true
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn node_compat_record_roundtrip() {
        // UNSAFE-BOUNDARY 覆盖（node_compat_json 的纯侧）：记录→读回一致；
        // 空记录回空数组（黑盒 execArgv 回显另行覆盖）。
        record_node_compat(vec!["--expose-internals".into()]);
        let back = node_compat_store().lock().unwrap().clone();
        assert_eq!(back, ["--expose-internals"]);
        record_node_compat(Vec::new());
        let back = node_compat_store().lock().unwrap().clone();
        assert!(back.is_empty());
    }
}
