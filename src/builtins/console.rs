//! console 内建（Phase 1 子集）：log/info/warn/error/debug/trace/dir、assert、
//! count/countReset、time/timeLog/timeEnd、group/groupEnd、clear。
//! 对象走 ToString（"[object Object]"），Node 式 inspect 与 %-格式化留待后续 Phase。

use std::io::Write as _;

use mozjs::context::JSContext;
use mozjs::jsval::JSVal;
use mozjs::jsval::UndefinedValue;

use crate::jsapi_glue::{Frame, value_to_string, wrap_cx};
use crate::state;

fn emit(stderr: bool, msg: &str) {
    let indent = state::console_state(|p| p.console_indent);
    let prefix = "  ".repeat(indent.min(16));
    let line = format!("{prefix}{msg}");
    // REPL TTY 会话：读行线程 raw mode 期间裸 `\n` 不回车（阶梯右移），
    // 用户输出统一 CRLF 化（crate::repl::REPL_TTY_OUTPUT 注记）。
    if crate::repl::tty_output_enabled() {
        let body = crate::repl::crlf(&line);
        if stderr {
            eprint!("{body}\r\n");
        } else {
            print!("{body}\r\n");
            let _ = std::io::stdout().flush();
        }
        return;
    }
    if stderr {
        let mut err = std::io::stderr().lock();
        let _ = writeln!(err, "{line}");
    } else {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{line}");
    }
}

fn join_args(cx: &mut JSContext, frame: &Frame) -> String {
    let mut parts = Vec::with_capacity(frame.argc() as usize);
    for i in 0..frame.argc() {
        let v = frame.arg(i);
        parts.push(value_to_string(cx, v));
    }
    parts.join(" ")
}

/// SAFETY: 由引擎以有效调用帧调用（JSNative 约定）。
macro_rules! console_sink {
    ($name:ident, $stderr:expr) => {
        pub unsafe extern "C" fn $name(cx_raw: *mut mozjs::jsapi::JSContext, argc: u32, vp: *mut JSVal) -> bool { unsafe {
            let mut cx = wrap_cx(cx_raw);
            let frame = Frame::from_raw(vp, argc);
            emit($stderr, &join_args(&mut cx, &frame));
            frame.set_rval(UndefinedValue());
            true
        }}
    };
}

console_sink!(log, false);
console_sink!(info, false);
console_sink!(debug, false);
console_sink!(dir, false);
console_sink!(warn, true);
console_sink!(error, true);

/// console.trace：Phase 1 无栈信息，退化为 stderr 输出（文档已注明）。
pub unsafe extern "C" fn trace(cx_raw: *mut mozjs::jsapi::JSContext, argc: u32, vp: *mut JSVal) -> bool { unsafe {
    let mut cx = wrap_cx(cx_raw);
    let frame = Frame::from_raw(vp, argc);
    let joined = join_args(&mut cx, &frame);
    emit(true, &format!("Trace: {joined}"));
    frame.set_rval(UndefinedValue());
    true
}}

/// JS 真值判定（console.assert 用）。
fn truthy(cx: &mut JSContext, v: JSVal) -> bool {
    if v.is_null_or_undefined() {
        false
    } else if v.is_boolean() {
        v.to_boolean()
    } else if v.is_number() {
        let n = v.to_number();
        n != 0.0 && !n.is_nan()
    } else if v.is_string() {
        !value_to_string(cx, v).is_empty()
    } else {
        true
    }
}

/// console.assert(cond, ...rest)：条件为假时输出（不抛，规范行为）。
pub unsafe extern "C" fn assert(cx_raw: *mut mozjs::jsapi::JSContext, argc: u32, vp: *mut JSVal) -> bool { unsafe {
    let mut cx = wrap_cx(cx_raw);
    let frame = Frame::from_raw(vp, argc);
    let cond = if argc > 0 { frame.arg(0) } else { UndefinedValue() };
    if !truthy(&mut cx, cond) {
        let rest = if argc > 1 {
            value_to_string(&mut cx, frame.arg(1))
        } else {
            String::new()
        };
        if rest.is_empty() {
            emit(true, "Assertion failed:");
        } else {
            emit(true, &format!("Assertion failed: {rest}"));
        }
    }
    frame.set_rval(UndefinedValue());
    true
}}

pub unsafe extern "C" fn count(cx_raw: *mut mozjs::jsapi::JSContext, argc: u32, vp: *mut JSVal) -> bool { unsafe {
    let mut cx = wrap_cx(cx_raw);
    let frame = Frame::from_raw(vp, argc);
    let label = if argc > 0 {
        value_to_string(&mut cx, frame.arg(0))
    } else {
        "default".into()
    };
    let n = state::console_state(|p| {
        let e = p.console_counts.entry(label.clone()).or_insert(0);
        *e += 1;
        *e
    });
    emit(false, &format!("{label}: {n}"));
    frame.set_rval(UndefinedValue());
    true
}}

pub unsafe extern "C" fn count_reset(cx_raw: *mut mozjs::jsapi::JSContext, argc: u32, vp: *mut JSVal) -> bool { unsafe {
    let mut cx = wrap_cx(cx_raw);
    let frame = Frame::from_raw(vp, argc);
    let label = if argc > 0 {
        value_to_string(&mut cx, frame.arg(0))
    } else {
        "default".into()
    };
    state::console_state(|p| {
        p.console_counts.remove(&label);
    });
    frame.set_rval(UndefinedValue());
    true
}}

pub unsafe extern "C" fn time(cx_raw: *mut mozjs::jsapi::JSContext, argc: u32, vp: *mut JSVal) -> bool { unsafe {
    let mut cx = wrap_cx(cx_raw);
    let frame = Frame::from_raw(vp, argc);
    let label = if argc > 0 {
        value_to_string(&mut cx, frame.arg(0))
    } else {
        "default".into()
    };
    state::console_state(|p| {
        p.console_times.insert(label, std::time::Instant::now());
    });
    frame.set_rval(UndefinedValue());
    true
}}

pub unsafe extern "C" fn time_log(cx_raw: *mut mozjs::jsapi::JSContext, argc: u32, vp: *mut JSVal) -> bool { unsafe {
    let mut cx = wrap_cx(cx_raw);
    let frame = Frame::from_raw(vp, argc);
    let label = if argc > 0 {
        value_to_string(&mut cx, frame.arg(0))
    } else {
        "default".into()
    };
    let started = state::console_state(|p| p.console_times.get(&label).copied());
    match started {
        Some(t) => emit(false, &format!("{label}: {:.3}ms", t.elapsed().as_secs_f64() * 1e3)),
        None => emit(true, &format!("Timer '{label}' does not exist")),
    }
    frame.set_rval(UndefinedValue());
    true
}}

pub unsafe extern "C" fn time_end(cx_raw: *mut mozjs::jsapi::JSContext, argc: u32, vp: *mut JSVal) -> bool { unsafe {
    let mut cx = wrap_cx(cx_raw);
    let frame = Frame::from_raw(vp, argc);
    let label = if argc > 0 {
        value_to_string(&mut cx, frame.arg(0))
    } else {
        "default".into()
    };
    let started = state::console_state(|p| p.console_times.remove(&label));
    match started {
        Some(t) => emit(false, &format!("{label}: {:.3}ms", t.elapsed().as_secs_f64() * 1e3)),
        None => emit(true, &format!("Timer '{label}' does not exist")),
    }
    frame.set_rval(UndefinedValue());
    true
}}

pub unsafe extern "C" fn group(cx_raw: *mut mozjs::jsapi::JSContext, argc: u32, vp: *mut JSVal) -> bool { unsafe {
    let mut cx = wrap_cx(cx_raw);
    let frame = Frame::from_raw(vp, argc);
    let joined = join_args(&mut cx, &frame);
    if !joined.is_empty() {
        emit(false, &joined);
    }
    state::console_state(|p| p.console_indent += 1);
    frame.set_rval(UndefinedValue());
    true
}}

pub unsafe extern "C" fn group_end(cx_raw: *mut mozjs::jsapi::JSContext, argc: u32, vp: *mut JSVal) -> bool {
    let _ = cx_raw;
    let frame = unsafe { Frame::from_raw(vp, argc) };
    state::console_state(|p| p.console_indent = p.console_indent.saturating_sub(1));
    frame.set_rval(UndefinedValue());
    true
}

pub unsafe extern "C" fn clear(cx_raw: *mut mozjs::jsapi::JSContext, argc: u32, vp: *mut JSVal) -> bool {
    let _ = cx_raw;
    let frame = unsafe { Frame::from_raw(vp, argc) };
    frame.set_rval(UndefinedValue());
    true
}
