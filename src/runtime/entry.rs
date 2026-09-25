//! runtime/entry：入口求值形态（经典/模块/TLA 重试）+ 报错回映射。

use super::*;
use mozjs::jsapi::JSObject;
use mozjs::jsval::UndefinedValue;
use mozjs::realm::AutoRealm;
use mozjs::rooted;
use mozjs::rust::RootedGuard;
use mozjs::rust::Runtime;
use url::Url;
use std::ffi::CString;
use crate::error::Error;

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
    // rejection 处理器重抛（`return Promise.reject(e)`）：链式 promise 保持
    // rejected，eval 路径据此挂 entry reactions（见 eval_syntax_fallback）——
    // 否则失败只落 `__wjs_error`、循环尾才读，开着的句柄（子进程/socket）会让
    // 循环永不 idle 即 hang（§4.70 姊妹案，spawn stdin 套件现形）。
    s.push_str("})().then(v => { globalThis.__wjs_value = v; }, e => { globalThis.__wjs_error = e; return Promise.reject(e); });");
    s
}

/// 文件入口是否走模块求值：`.ts/.tsx/.mts/.cts/.mjs` 强制；`.js/.jsx` 嗅探 ESM 语法。
/// 解析失败/未知后缀 → None（回落经典路径）。
pub(crate) fn sniff_module(filename: &str, source: &str) -> Option<Url> {
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
        // `.js` 跟最近 package.json type（`require()` 同口径）；无扩展名同理
        //（`type: module` 包的 extensionless bin，如 oxlint；无 type 即经典）。
        let check_type = matches!(ext.as_deref(), Some("js" | "jsx") | None);
        if !check_type {
            return None;
        }
        if crate::builtins::node::require::nearest_pkg_type(&path).as_deref() == Some("module") {
            tracing::info!(target: "winterjs::runtime", url = url.as_str(), "module detected (package.json type)");
            return Some(url);
        }
        if ext.is_none() {
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

/// `process.exit` 哨兵错识别（native 报 `__wjs_exit:<code>`，见 `node/process_.rs`）。
/// 覆盖裸消息与 `unhandled rejection: …` 单因包装。
pub fn exit_code_from_message(msg: &str) -> Option<i32> {
    if let Some(code) = msg.strip_prefix("__wjs_exit:") {
        return code.trim().parse().ok();
    }
    if let Some(reason) = msg.strip_prefix("unhandled rejection: ") {
        return exit_code_from_message(reason.trim());
    }
    None
}

/// 哨兵优先：错误消息带哨兵即转静默退出（`process.exit` 被 catch 也由旗兜底，见 `run`）。
pub(crate) fn map_exit_sentinel(err: Error) -> Error {
    let message = match &err {
        Error::Script { message, .. } => Some(message.clone()),
        Error::Other(message) => Some(message.clone()),
        _ => None,
    };
    match message.and_then(|m| exit_code_from_message(&m)) {
        Some(code) => Error::Exit(code),
        None => err,
    }
}


/// eval 首次求值失败：若为 SyntaxError，用 async IIFE 重包一次。
/// （触发条件放宽到一切 SyntaxError：`await` 在参数位置报的不是 await 错，见 §4.17；
/// 包装也解不出的真语法错误回落原始报错。）
pub(crate) async fn eval_syntax_fallback(
    rt: &mut Runtime,
    global: &RootedGuard<'_, *mut JSObject>,
    source: &str,
    filename: &str,
    fetch_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::fetch::FetchMsg>,
    ws_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::ws::WsEvent>,
    watch_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::fs::WatchEvent>,
    child_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::child::ChildEvent>,
    net_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::net::NetEvent>,
    worker_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::worker::WorkerEvent>,
    quic_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::quic::QuicEvent>,
    napi_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::napi::asyncwork::NapiEvent>,
    dispatch_rx: &mut tokio::sync::mpsc::UnboundedReceiver<usize>,
) -> Result<(), Error> {
    let original = {
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        rooted!(&in(&mut realm) let mut exc = UndefinedValue());
        // realm 内读取 pending exception（会消费异常值）
        let info = { let i = error_info_from_exception_stack(&mut realm, exc.handle_mut()); crate::jsapi_glue::fill_message(&mut realm, i, exc.get()) };
        let exc_kind = exc_name(&mut realm, exc.get());
        let is_syntax = exc_kind.as_deref() == Some("SyntaxError");
        if !is_syntax {
            // 非语法错误：直接用已捕获的信息报错（异常已被消费，勿再取）
            return match info {
                Some(info) => Err(Error::script_with_kind(
                    filename,
                    source,
                    info.line.saturating_sub(state::line_adjust()).max(1),
                    info.col,
                    info.message,
                    exc_kind,
                )),
                None => Err(Error::Other("uncaught JS exception (no stack info)".into())),
            };
        }
        // SAFETY: 首次失败发生在解析期（无副作用），清除后重跑包装版
        unsafe { JS_ClearPendingException((&mut realm).raw_cx()) };
        info.map(|info| {
            Error::script_with_kind(
                filename,
                source,
                info.line.saturating_sub(state::line_adjust()).max(1),
                info.col,
                info.message,
                exc_kind,
            )
        })
        .unwrap_or_else(|| Error::Other("uncaught JS exception (no stack info)".into()))
    };

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
            // 包装 promise（含 rejection 重抛）挂 entry 捕获：失败经 pump 检查点
            // 跳出循环（§4.15/§4.70 同型——事件循环前挂载，收尾不进 unhandled 表）。
            {
                let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
                if wrapped_rval.is_object() {
                    let obj = wrapped_rval.to_object();
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
                        tracing::debug!(target: "winterjs::runtime", "eval wrapped entry capture attached");
                    }
                }
            }
            event_loop(rt, global, ErrorSource::Script { source, filename }, fetch_rx, ws_rx, watch_rx, child_rx, net_rx, worker_rx, quic_rx, napi_rx, dispatch_rx).await?;
            let r = extract_eval_result(rt, global, source, filename);
            // engine/rt 由外层 run() 统一 forget（见 §4.8）
            return r;
        }
        // 语法错误 → 换下一种包装；运行期错误 → 直接上报（勿重跑）
        let (info, is_syntax, exc_kind) = {
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            rooted!(&in(&mut realm) let mut exc = UndefinedValue());
            // realm 内读取 pending exception（消费异常值）
            let info = { let i = error_info_from_exception_stack(&mut realm, exc.handle_mut()); crate::jsapi_glue::fill_message(&mut realm, i, exc.get()) };
            let exc_kind = exc_name(&mut realm, exc.get());
            let is_syntax = exc_kind.as_deref() == Some("SyntaxError");
            if is_syntax {
                // SAFETY: 解析期失败无副作用，清除后重试
                unsafe { mozjs::jsapi::JS_ClearPendingException((&mut realm).raw_cx()) };
            }
            (info, is_syntax, exc_kind)
        };
        if !is_syntax {
            return match info {
                Some(info) => Err(Error::script_with_kind(
                    filename,
                    source,
                    info.line.saturating_sub(adjust).max(1),
                    info.col,
                    info.message,
                    exc_kind,
                )),
                None => Err(Error::Other("uncaught JS exception (no stack info)".into())),
            };
        }
    }
    // 包装也解不出：回落首次的原始报错（而非 exhausted，保住定位）。
    Err(original)
}

/// SAFETY: rejection 追踪器由引擎在 JS 线程回调。
pub(crate) unsafe extern "C" fn rejection_tracker(
    _cx: *mut mozjs::jsapi::JSContext,
    _muted_errors: bool,
    promise: mozjs::jsapi::Handle<*mut JSObject>,
    state_: PromiseRejectionHandlingState,
    _data: *mut std::ffi::c_void,
) {
    match state_ {
        PromiseRejectionHandlingState::Unhandled => {
            // `Heap::boxed` 定址（set 后禁移动，见 §4.40）。
            tracing::debug!(target: "winterjs::promise", promise = ?promise.get(), "rejection unhandled");
            state::with_rooted(|s| s.unhandled.push(mozjs::jsapi::Heap::boxed(promise.get())));
        }
        PromiseRejectionHandlingState::Handled => {
            tracing::trace!(target: "winterjs::promise", promise = ?promise.get(), "rejection handled");
            state::with_rooted(|s| s.unhandled.retain(|h| h.get() != promise.get()));
        }
    }
}

/// 在已持有的 realm 里把 pending exception 转成 Error（返回前异常被消费）。
pub(crate) fn pending_error_in_realm(
    realm: &mut AutoRealm<'_>,
    source: &str,
    filename: &str,
    line_adjust: u32,
) -> Error {
    rooted!(&in(realm) let mut exc = UndefinedValue());
    // realm 内读取 pending exception
    match { let i = error_info_from_exception_stack(realm, exc.handle_mut()); crate::jsapi_glue::fill_message(realm, i, exc.get()) } {
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
pub(crate) fn print_completion(
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
    // 同上：REPL 管道下完成值行立即刷出（`run`/`eval` 单次进程无感，顺手）。
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    Ok(())
}
