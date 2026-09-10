//! mozjs 边界胶水：引擎初始化、Realm、prelude、脚本求值、事件循环。
//! 本模块是 §6 允许 `unsafe` 的唯一区域（rooting / AutoRealm / FFI），
//! builtins 里的 unsafe 仅限 JSNative 调用帧访问。
//!
//! 事件循环（Phase 1）：
//!   RunJobs 排空微任务（内部 job queue）→ 若有定时器则睡到最近触发时刻 →
//!   触发到期定时器 → 循环；两者皆空即退出（不饿死、不早退）。

use std::ffi::CString;
use std::ptr;

use mozjs::conversions::{ConversionResult, FromJSValConvertible as _};
use mozjs::gc::RootedGuard;
use mozjs::jsapi::{
    AddPromiseReactions, JS_ClearPendingException, JS_GetProperty, JSObject,
    OnNewGlobalHookOption, PromiseRejectionHandlingState, RunJobs,
    SetPromiseRejectionTrackerCallback,
};
use mozjs::jsval::UndefinedValue;
use mozjs::realm::AutoRealm;
use mozjs::rooted;
use mozjs::rust::{
    CompileOptionsWrapper, RealmOptions, SIMPLE_GLOBAL_CLASS, error_info_from_exception_stack,
    evaluate_script, JSEngine, Runtime,
};
use mozjs::rust::wrappers2::JS_NewGlobalObject;

use url::Url;

use crate::builtins;
use crate::builtins::timers;
use crate::modules;use crate::error::Error;
use crate::jsapi_glue::{exc_name_is, get_prop_string, get_prop_u32, pending_exception_error, raw_handle, raw_handle_mut, value_to_string};
use crate::state;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    /// 文件脚本：求值完成值，事件循环跑完 timers/microtasks 后退出
    Script,
    /// eval：先按普通脚本求值；仅当因顶层 await 语法失败时，用 async IIFE 重包
    Eval,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum WrapKind {
    /// `return (code);` —— 单表达式/以 await 表达式为主的场景，完成值 = await 值；行偏移 2
    Return,
    /// 任意语句序列；完成值丢失（文档注明）；行偏移 1
    Plain,
}

fn eval_wrap(code: &str, kind: WrapKind) -> String {
    // 包装行占用户代码之前的行 → 报错行号统一减 line_adjust
    let mut s = String::from(match kind {
        WrapKind::Return => "(async()=>{\nreturn (\n",
        WrapKind::Plain => "(async()=>{\n",
    });
    s.push_str(code);
    s.push_str(match kind {
        WrapKind::Return => "\n);\n",
        WrapKind::Plain => "\n",
    });
    s.push_str("})().then(v => { globalThis.__wjs_value = v; }, e => { globalThis.__wjs_error = e; });");
    s
}

fn eval_await_failure(msg: &str) -> bool {
    // SpiderMonkey 顶层 await 的 SyntaxError 文案
    msg.contains("await is only valid")
}

/// 文件入口是否走模块求值：`.ts/.tsx/.mts/.cts/.mjs` 强制；`.js/.jsx` 嗅探 ESM 语法。
/// 解析失败/未知后缀 → None（回落经典路径）。
fn sniff_module(filename: &str, source: &str) -> Option<Url> {
    let url = crate::loader::resolve::entry_url(std::path::Path::new(filename)).ok()?;
    if url.scheme() != "file" {
        return None;
    }
    let path = url.to_file_path().ok()?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    let forced = matches!(ext.as_deref(), Some("ts" | "mts" | "cts" | "tsx" | "mjs"));
    if !forced {
        if !matches!(ext.as_deref(), Some("js" | "jsx")) {
            return None;
        }
        let loaded = crate::loader::load_js(source, filename, &path).ok()?;
        if !loaded.is_module {
            return None;
        }
    }
    tracing::info!(target: "winterjs::runtime", url = url.as_str(), "module detected");
    Some(url)
}

/// 模块入口全流程：compile → load deps → link → evaluate → 事件循环 → 完成值打印。
async fn run_module(
    rt: &mut Runtime,
    global: &RootedGuard<'_, *mut JSObject>,
    url: &Url,
) -> Result<(), Error> {
    use mozjs::rust::wrappers2::{ModuleEvaluate, ModuleLink};

    tracing::info!(target: "winterjs::runtime", url = url.as_str(), "module run start");
    rooted!(&in(rt.cx()) let mut rval = UndefinedValue());
    {
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        rooted!(&in(&mut realm) let mut entry: *mut JSObject = std::ptr::null_mut());
        let record = modules::compile_entry(&mut realm, url)?;
        entry.set(record);
        modules::load_dependencies(&mut realm, entry.get())?;
        // SAFETY: entry 为有效 rooted 记录；realm 内同步 link/evaluate
        if !unsafe { ModuleLink(&mut realm, entry.handle()) } {
            return Err(modules::module_error(&mut realm, url.as_str()));
        }
        if !unsafe { ModuleEvaluate(&mut realm, entry.handle(), rval.handle_mut()) } {
            return Err(modules::module_error(&mut realm, url.as_str()));
        }
        // 入口 promise 先挂专用捕获（事件循环前挂载，否则收尾会被当未处理 rejection 误报）。
        if rval.is_object() {
            let obj = rval.to_object();
            rooted!(&in(&mut realm) let obj_root: *mut JSObject = obj);
            // SAFETY: obj_root 为有效 rooted 对象
            if unsafe { mozjs::jsapi::IsPromiseObject(raw_handle(obj_root.as_ptr())) } {
                let (fulfilled, rejected) = state::entry_native_values();
                rooted!(&in(&mut realm) let promise = obj_root.get());
                rooted!(&in(&mut realm) let ful_obj: *mut JSObject = fulfilled.to_object());
                rooted!(&in(&mut realm) let rej_obj: *mut JSObject = rejected.to_object());
                // SAFETY: promise/回调均为有效 rooted 函数对象；指针直拷标记位置
                unsafe {
                    let _ = AddPromiseReactions(
                        (&mut realm).raw_cx(),
                        raw_handle(promise.as_ptr()),
                        raw_handle(ful_obj.as_ptr()),
                        raw_handle(rej_obj.as_ptr()),
                    );
                }
                tracing::debug!(target: "winterjs::runtime", "entry promise capture attached");
            }
        }
    }

    event_loop(rt, global, ErrorSource::Module { url: url.as_str() }).await?;

    // 收割入口决议（事件循环的排空已驱动捕获回调）。
    let (fulfillment, rejection) =
        state::with_plain(|p| (p.entry_fulfillment.take(), p.entry_rejection.take()));
    if let Some(reason) = rejection {
        // 入口决议串自带位置（`file:line:col: message`）或为值串，直接上报
        return Err(Error::Other(reason));
    }
    if let Some(s) = fulfillment {
        // 脚本语义对齐：决议 undefined 不打印
        if s != "undefined" {
            println!("{s}");
        }
        return Ok(());
    }
    print_completion(rt, global, rval.get())
}

/// 求值 `source`（名 `filename`）并打印完成值；事件循环排空 timers/microtasks。
pub async fn run(source: &str, filename: &str, mode: Mode) -> Result<(), Error> {
    tracing::info!(target: "winterjs::runtime", filename, source_len = source.len(), ?mode, "run start");
    // JS engine handle must outlive every Runtime.
    let engine = JSEngine::init().map_err(|_| Error::Other("failed to init JS engine".into()))?;
    let mut rt = Runtime::new(engine.handle());
    let _cx = rt.cx();
    // TLS 状态必须先于引擎销毁（见 state::shutdown 文档）
    let _state_guard = state::StateGuard;
    modules::install_hooks(&rt);

    // SAFETY: 引擎初始化后、首段脚本前启用内部 job queue（JS shell 同款），
    // Promise 微任务由此排队，RunJobs 排空。
    let options = RealmOptions::default();
    rooted!(&in(rt.cx()) let global = unsafe {
        JS_NewGlobalObject(
            rt.cx(),
            &SIMPLE_GLOBAL_CLASS,
            ptr::null_mut(),
            OnNewGlobalHookOption::FireOnNewGlobalHook,
            &*options,
        )
    });
    rooted!(&in(rt.cx()) let mut rval = UndefinedValue());

    // §4.1：进入 global realm 后再做 JSAPI 初始化（内建、prelude、rejection 追踪器）
    {
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        // SAFETY: realm 内启用内部 job queue（realm 外调用会 SEGV）；
        // 不启用则 RunJobs 无队列可用，同样 SEGV（AGENTS §4.7 记坑）。
        unsafe { crate::jobqueue::install((&mut realm).raw_cx()) };
        state::init(&mut realm);
        state::set_global(global.get());
        state::set_line_adjust(0);
        builtins::define_all(&mut realm, global.get())?;

        // SAFETY: realm 内；追踪器仅在 JS 线程被引擎回调
        unsafe {
            SetPromiseRejectionTrackerCallback(
                (&mut realm).raw_cx(),
                Some(rejection_tracker),
                ptr::null_mut(),
            );
        }

        let prelude_filename = CString::new("__wjs_prelude.js").expect("no NUL");
        let prelude_options = CompileOptionsWrapper::new(&realm, prelude_filename, 1);
        rooted!(&in(&mut realm) let mut prelude_rval = UndefinedValue());
        // prelude 是项目自带常量脚本，语法必然正确
        let ok = evaluate_script(
            &mut realm,
            global.handle(),
            builtins::PRELUDE,
            prelude_rval.handle_mut(),
            prelude_options,
        );
        if ok.is_err() {
            return Err(pending_error_in_realm(
                &mut realm,
                builtins::PRELUDE,
                "__wjs_prelude.js",
                0,
            ));
        }

        // 缓存 prelude 辅助函数值（timers 触发与 structuredClone 枚举要用）
        for (prop, idx) in [(c"__wjs_call", 0u8), (c"__wjs_entries", 1u8)] {
            rooted!(&in(&mut realm) let mut v = UndefinedValue());
            // SAFETY: global 为有效 rooted 对象
            let ok = unsafe {
                JS_GetProperty((&mut realm).raw_cx(), raw_handle(global.as_ptr()), prop.as_ptr(), raw_handle_mut(v.as_ptr()))
            };
            if !ok || !v.is_object() {
                return Err(Error::Other(format!(
                    "prelude helper {} missing",
                    prop.to_str().unwrap_or("?")
                )));
            }
            let got = v.get();
            state::with_rooted(|s| match idx {
                0 => s.call_fn.set(got),
                _ => s.entries_fn.set(got),
            });
        }
    }

    // 模块嗅探（仅 Script；Eval 保持经典语义，import 即 SyntaxError）。
    // 解析失败 → 回落经典（经典求值会给出它自己的报错）。
    if mode == Mode::Script {
        if let Some(url) = sniff_module(filename, source) {
            let r = run_module(&mut rt, &global, &url).await;
            // §4.8：跳过引擎/运行时析构
            forget_engine(rt, engine);
            return r;
        }
    }

    // 用户脚本求值
    {
        let c_filename = CString::new(filename).unwrap_or_else(|_| c"script.js".into());
        let options = CompileOptionsWrapper::new(rt.cx(), c_filename, 1);
        // evaluate_script 内部自进 realm；rval 为 rooted 出参，跨事件循环存活
        let res = evaluate_script(rt.cx(), global.handle(), source, rval.handle_mut(), options);
        if res.is_err() {
            if mode == Mode::Eval {
                let r = eval_syntax_fallback(&mut rt, &global, source, filename).await;
                // §4.8：跳过引擎/运行时析构（StoreBuffer 悬垂边在 destroyRuntime 的小 GC 里 SEGV）
                forget_engine(rt, engine);
                return r;
            }
            let err = pending_exception_error(rt.cx(), global.get(), source, filename);
            // TLA 重试：顶层 await 的脚本改走模块求值（Node 同行为；嗅探覆盖不到纯 await 文件）
            let retry_module = matches!(&err, Error::Script { message, .. } if eval_await_failure(message));
            if retry_module {
                if let Ok(url) = crate::loader::resolve::entry_url(std::path::Path::new(filename)) {
                    tracing::info!(target: "winterjs::runtime", url = url.as_str(), "top-level await, retrying as module");
                    let r = run_module(&mut rt, &global, &url).await;
                    forget_engine(rt, engine);
                    return r;
                }
            }
            forget_engine(rt, engine);
            return Err(err);
        }
    }

    // 未包装成功的场景（含全部 Script 与无顶层 await 的 Eval）：
    // 完成值就是 rval（老行为）；仅 async IIFE 包装路径才读 __wjs_value。
    event_loop(&mut rt, &global, ErrorSource::Script { source, filename }).await?;
    let r = print_completion(&mut rt, &global, rval.get());
    // §4.8：跳过引擎/运行时析构（带 timer 的路径在 JS_DestroyContext 里 SEGV）。
    // CLI 进程即将退出，内存由 OS 回收；见 AGENTS §4.8。
    forget_engine(rt, engine);
    r
}

/// # Safety / 泄漏说明
/// 刻意泄漏 Runtime 与 JSEngine（不含 `Drop` 清理），见 §4.8。
fn forget_engine(rt: Runtime, engine: JSEngine) {
    std::mem::forget(rt);
    std::mem::forget(engine);
}


/// eval 首次求值失败：若为顶层 await 的 SyntaxError，用 async IIFE 重包一次。
async fn eval_syntax_fallback(
    rt: &mut Runtime,
    global: &RootedGuard<'_, *mut JSObject>,
    source: &str,
    filename: &str,
) -> Result<(), Error> {
    {
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        rooted!(&in(&mut realm) let mut exc = UndefinedValue());
        // realm 内读取 pending exception（会消费异常值）
        let info = error_info_from_exception_stack(&mut realm, exc.handle_mut());
        let is_await = info
            .as_ref()
            .map(|i| eval_await_failure(&i.message))
            .unwrap_or(false);
        if !is_await {
            // 非 await 错误：直接用已捕获的信息报错（异常已被消费，勿再取）
            return match info {
                Some(info) => Err(Error::script(
                    filename,
                    source,
                    info.line.saturating_sub(state::line_adjust()).max(1),
                    info.col,
                    info.message,
                )),
                None => Err(Error::Other("uncaught JS exception (no stack info)".into())),
            };
        }
        // SAFETY: 首次失败发生在解析期（无副作用），清除后重跑包装版
        unsafe { JS_ClearPendingException((&mut realm).raw_cx()) };
    }

    // 先试 return 包装（保住完成值），纯语句序列再退普通包装
    for (kind, adjust) in [(WrapKind::Return, 2u32), (WrapKind::Plain, 1u32)] {
        tracing::debug!(target: "winterjs::runtime", ?kind, adjust, "eval fallback trying wrap");
        state::set_line_adjust(adjust);
        let wrapped = eval_wrap(source, kind);
        let c_filename = CString::new(filename).unwrap_or_else(|_| c"eval.js".into());
        let options = CompileOptionsWrapper::new(rt.cx(), c_filename, 1);
        rooted!(&in(rt.cx()) let mut wrapped_rval = UndefinedValue());
        // 同 run()；包装版行号偏移经 line_adjust 校正
        let res = evaluate_script(rt.cx(), global.handle(), &wrapped, wrapped_rval.handle_mut(), options);
        if res.is_ok() {
            event_loop(rt, global, ErrorSource::Script { source, filename }).await?;
            let r = extract_eval_result(rt, global, source, filename);
            // engine/rt 由外层 run() 统一 forget（见 §4.8）
            return r;
        }
        // 语法错误 → 换下一种包装；运行期错误 → 直接上报（勿重跑）
        let (info, is_syntax) = {
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            rooted!(&in(&mut realm) let mut exc = UndefinedValue());
            // realm 内读取 pending exception（消费异常值）
            let info = error_info_from_exception_stack(&mut realm, exc.handle_mut());
            let is_syntax = exc_name_is(&mut realm, exc.get(), "SyntaxError");
            if is_syntax {
                // SAFETY: 解析期失败无副作用，清除后重试
                unsafe { mozjs::jsapi::JS_ClearPendingException((&mut realm).raw_cx()) };
            }
            (info, is_syntax)
        };
        if !is_syntax {
            return match info {
                Some(info) => Err(Error::script(
                    filename,
                    source,
                    info.line.saturating_sub(adjust).max(1),
                    info.col,
                    info.message,
                )),
                None => Err(Error::Other("uncaught JS exception (no stack info)".into())),
            };
        }
    }
    Err(Error::Other("eval fallback exhausted".into()))
}

/// 事件循环错误源：脚本（源码直给）或模块（按异常文件名查调试信息回映射）。
#[derive(Clone, Copy)]
pub(crate) enum ErrorSource<'a> {
    Script { source: &'a str, filename: &'a str },
    Module { url: &'a str },
}

/// 事件循环：RunJobs 排空微任务 → 睡到最近定时器 → 触发到期定时器，直到两者皆空。
async fn event_loop(
    rt: &mut Runtime,
    global: &RootedGuard<'_, *mut JSObject>,
    err: ErrorSource<'_>,
) -> Result<(), Error> {
    let mut iterations: u64 = 0;
    let mut timers_fired: usize = 0;
    loop {
        {
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            // SAFETY: realm 内排空内部 job queue
            unsafe { RunJobs((&mut realm).raw_cx()) };
        }
        iterations += 1;

        let Some(at) = timers::next_deadline() else {
            break;
        };
        let tokio_at = tokio::time::Instant::from_std(at);
        if tokio_at > tokio::time::Instant::now() {
            tokio::time::sleep_until(tokio_at).await;
        }

        {
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            timers_fired += timers::fire_due(&mut realm, global.get(), err)?;
        }
    }
    tracing::info!(target: "winterjs::runtime", iterations, timers_fired, "event loop drained");

    // 未处理 rejection 收尾上报（Node 式 fatal）：挂捕获 reactions → 再排空一轮
    let unhandled = state::with_rooted(|s| {
        s.unhandled
            .iter()
            .map(|h| h.get())
            .collect::<Vec<*mut JSObject>>()
    });
    if !unhandled.is_empty() {
        tracing::warn!(target: "winterjs::runtime", count = unhandled.len(), "unhandled rejections detected");
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        let (on_fulfilled, on_rejected) = state::capture_native_values();
        for promise_obj in unhandled {
            rooted!(&in(&mut realm) let promise = promise_obj);
            rooted!(&in(&mut realm) let ful_obj: *mut JSObject = on_fulfilled.to_object());
            rooted!(&in(&mut realm) let rej_obj: *mut JSObject = on_rejected.to_object());
            // SAFETY: promise/回调均为有效 rooted 函数对象；指针直拷标记位置
            unsafe {
                let _ = AddPromiseReactions(
                    (&mut realm).raw_cx(),
                    raw_handle(promise.as_ptr()),
                    raw_handle(ful_obj.as_ptr()),
                    raw_handle(rej_obj.as_ptr()),
                );
            }
        }
        // SAFETY: realm 内排空捕获反应，on_rejected_native 记录 reason
        unsafe { RunJobs((&mut realm).raw_cx()) };
        let reasons = state::with_plain(|p| std::mem::take(&mut p.rejection_reasons));
        if !reasons.is_empty() {
            return Err(Error::Other(format!(
                "unhandled rejection: {}",
                reasons.join("; ")
            )));
        }
    }
    Ok(())
}

/// SAFETY: rejection 追踪器由引擎在 JS 线程回调。
unsafe extern "C" fn rejection_tracker(
    _cx: *mut mozjs::jsapi::JSContext,
    _muted_errors: bool,
    promise: mozjs::jsapi::Handle<*mut JSObject>,
    state_: PromiseRejectionHandlingState,
    _data: *mut std::ffi::c_void,
) {
    match state_ {
        PromiseRejectionHandlingState::Unhandled => {
            let heap = mozjs::jsapi::Heap::default();
            heap.set(promise.get());
            tracing::debug!(target: "winterjs::promise", promise = ?promise.get(), "rejection unhandled");
            state::with_rooted(|s| s.unhandled.push(heap));
        }
        PromiseRejectionHandlingState::Handled => {
            tracing::trace!(target: "winterjs::promise", promise = ?promise.get(), "rejection handled");
            state::with_rooted(|s| s.unhandled.retain(|h| h.get() != promise.get()));
        }
    }
}

/// 在已持有的 realm 里把 pending exception 转成 Error（返回前异常被消费）。
fn pending_error_in_realm(
    realm: &mut AutoRealm<'_>,
    source: &str,
    filename: &str,
    line_adjust: u32,
) -> Error {
    rooted!(&in(realm) let mut exc = UndefinedValue());
    // realm 内读取 pending exception
    match error_info_from_exception_stack(realm, exc.handle_mut()) {
        Some(info) => Error::script(
            filename,
            source,
            info.line.saturating_sub(line_adjust).max(1),
            info.col,
            info.message,
        ),
        None => Error::Other("uncaught JS exception (no stack info)".into()),
    }
}

/// eval 结果：__wjs_error 优先（格式与未捕获异常一致），否则打印 __wjs_value。
fn extract_eval_result(
    rt: &mut Runtime,
    global: &RootedGuard<'_, *mut JSObject>,
    source: &str,
    filename: &str,
) -> Result<(), Error> {
    let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
    rooted!(&in(&mut realm) let mut err = UndefinedValue());
    // SAFETY: realm 内读全局属性；raw 调用不触发 GC
    let ok = unsafe {
        JS_GetProperty((&mut realm).raw_cx(), raw_handle(global.as_ptr()), c"__wjs_error".as_ptr(), raw_handle_mut(err.as_ptr()))
    };
    if ok && !err.is_undefined() {
        // Error 对象读 message/lineNumber/columnNumber；非对象值退化为 ToString
        let (message, line, col) = if err.is_object() {
            let obj = err.to_object();
            rooted!(&in(&mut realm) let obj_root: *mut JSObject = obj);
            let message = get_prop_string(&mut realm, obj_root.get(), c"message")
                .unwrap_or_default();
            let line = get_prop_u32(&mut realm, obj_root.get(), c"lineNumber")
                .map(|l| l.saturating_sub(state::line_adjust()))
                .unwrap_or(1);
            let col = get_prop_u32(&mut realm, obj_root.get(), c"columnNumber").unwrap_or(1);
            (message, line, col)
        } else {
            (value_to_string(&mut realm, err.get()), 1, 1)
        };
        let message = if message.is_empty() {
            value_to_string(&mut realm, err.get())
        } else {
            message
        };
        return Err(Error::script(filename, source, line.max(1), col, message));
    }

    rooted!(&in(&mut realm) let mut val = UndefinedValue());
    // SAFETY: realm 内读全局属性；raw 调用不触发 GC
    let ok = unsafe {
        JS_GetProperty((&mut realm).raw_cx(), raw_handle(global.as_ptr()), c"__wjs_value".as_ptr(), raw_handle_mut(val.as_ptr()))
    };
    if ok && !val.is_undefined() {
        println!("{}", value_to_string(&mut realm, val.get()));
    }
    Ok(())
}

/// Script 模式完成值打印（与既有行为一致：undefined 不打印）。
fn print_completion(
    rt: &mut Runtime,
    global: &RootedGuard<'_, *mut JSObject>,
    rval: mozjs::jsval::JSVal,
) -> Result<(), Error> {
    if rval.is_undefined() {
        return Ok(());
    }
    let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
    rooted!(&in(&mut realm) let rv = rval);
    match String::from_jsval(&mut realm, rv.handle(), ()) {
        Ok(ConversionResult::Success(s)) => println!("{s}"),
        _ => println!("<non-stringifiable result>"),
    }
    Ok(())
}
