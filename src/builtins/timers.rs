//! timers：setTimeout/setInterval/clearTimeout/clearInterval。
//! 注册表存于 state::RootedState（RootedTraceableBox 跨 GC 保活）；
//! prelude 已把实参打包成 JS 数组、回调/数组都是真实 JS 对象，无裸栈值跨越 GC。

use std::time::{Duration, Instant};

use mozjs::context::JSContext;
use mozjs::gc::ValueArray;
use mozjs::jsapi::{HandleValueArray, JS_CallFunctionValue, Heap, JSObject};
use mozjs::jsval::{Int32Value, JSVal, UndefinedValue};
use mozjs::rooted;

use crate::error::Error;
use crate::jsapi_glue::{Frame, pending_exception_error, raw_handle, raw_handle_mut, report_error, wrap_cx};
use crate::state;

/// Node 语义：0/负数/NaN → 1ms；上限 2^31-1。
fn clamp_delay(ms: f64) -> f64 {
    if !ms.is_finite() || ms < 1.0 {
        1.0
    } else {
        ms.min(2_147_483_647.0)
    }
}

fn register_timer(cx: &mut JSContext, frame: &Frame, interval: bool) -> bool {
    // 前置条件：prelude 已做类型检查并打包实参；此处防御式再查
    let cb = unsafe { frame.arg(0) };
    let ms = unsafe { frame.arg(1) };
    let args = unsafe { frame.arg(2) };
    if !cb.is_object() || !args.is_object() {
        report_error(cx, "TypeError: invalid timer arguments");
        return false;
    }
    let delay_ms = if ms.is_number() { ms.to_number() } else { 0.0 };
    let delay = Duration::from_secs_f64(clamp_delay(delay_ms) / 1e3);
    let id = state::next_timer_id();

    let mut callback = Heap::default();
    callback.set(cb);
    let mut args_heap = Heap::default();
    args_heap.set(args);

    state::with_rooted(|s| {
        s.timers.push(state::TimerEntry {
            id,
            callback,
            args: args_heap,
            at: Instant::now() + delay,
            interval: interval.then_some(delay),
        });
    });
    unsafe { frame.set_rval(Int32Value(id as i32)) };
    true
}

/// SAFETY: 由引擎以有效调用帧调用；prelude 保证 arg0=callback、arg1=ms、arg2=实参数组。
pub unsafe extern "C" fn set_timeout(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = wrap_cx(cx_raw);
    let frame = Frame { vp, argc };
    register_timer(&mut cx, &frame, false)
}

/// SAFETY: 同 set_timeout。
pub unsafe extern "C" fn set_interval(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = wrap_cx(cx_raw);
    let frame = Frame { vp, argc };
    register_timer(&mut cx, &frame, true)
}

/// SAFETY: 由引擎以有效调用帧调用；arg0 为数值 id。
pub unsafe extern "C" fn clear_timeout(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = wrap_cx(cx_raw);
    let frame = Frame { vp, argc };
    let _ = &mut cx;
    let id_v = if argc > 0 { frame.arg(0) } else { UndefinedValue() };
    let id = if id_v.is_number() {
        let f = id_v.to_number();
        if f.is_finite() && f >= 0.0 { f as u32 } else { 0 }
    } else {
        0
    };
    // 注册表里直接摘除；触发中的定时器不在注册表里，用 cleared_during_fire 记账，
    // fire 循环据此不重排 interval。未知 id 一并记账：id 单调不复用，残留无害。
    state::with_plain(|p| {
        state::with_rooted(|s| {
            let before = s.timers.len();
            s.timers.retain(|t| t.id != id);
            if s.timers.len() == before {
                p.cleared_during_fire.insert(id);
            }
        });
    });
    frame.set_rval(UndefinedValue());
    true
}

/// 事件循环里最近的触发时刻。
pub fn next_deadline() -> Option<Instant> {
    state::with_rooted(|s| s.timers.iter().map(|t| t.at).min())
}

/// 触发所有到期定时器。回调未捕获异常 → Error::Script（Node 式 fatal）。
/// 前置条件：cx 已进入 global 所属 realm。
pub fn fire_due(
    cx: &mut JSContext,
    global: *mut JSObject,
    source: &str,
    filename: &str,
) -> Result<(), Error> {
    // 快照到期 id（回调里可能再注册/清除，不能持借用调 JS）
    let due: Vec<u32> = state::with_rooted(|s| {
        let now = Instant::now();
        let mut ids: Vec<u32> = s
            .timers
            .iter()
            .filter(|t| t.at <= now)
            .map(|t| t.id)
            .collect();
        ids.sort();
        ids
    });

    for id in due {
        // 摘除条目（回调期间 clear 不必再摘），值复制进 rooted 栈槽
        let entry = state::with_rooted(|s| s.timers.iter().position(|t| t.id == id).map(|i| s.timers.remove(i)));
        let Some(entry) = entry else { continue };
        let (cb, args) = (entry.callback.get(), entry.args.get());
        let interval = entry.interval;
        let scheduled_at = entry.at;

        rooted!(&in(cx) let fun = cb);
        rooted!(&in(cx) let argv = ValueArray::new([cb, args]));
        rooted!(&in(cx) let mut rval = UndefinedValue());
        let args_array = HandleValueArray {
            length_: 2,
            // SAFETY: argv 为栈上 Rooted 槽，存活到调用返回，元素被 GC 追踪
            elements_: argv.as_ptr().cast(),
        };
        // SAFETY: cx/global/fun 均有效；rval 为 rooted 出参
        let ok = unsafe {
            JS_CallFunctionValue(
                cx.raw_cx(),
                raw_handle(&global),
                raw_handle(fun.as_ptr()),
                &args_array,
                raw_handle_mut(rval.as_ptr()),
            )
        };
        if !ok {
            // 清掉本轮回合的记账，进程即将退出
            state::with_plain(|p| p.cleared_during_fire.clear());
            return Err(pending_exception_error(cx, global, source, filename));
        }

        // interval：漂移校正重排（scheduled_at + interval）；触发期间被 clear 的不再排
        let cleared = state::with_plain(|p| p.cleared_during_fire.remove(&id));
        if interval.is_some() && !cleared {
            let iv = interval.expect("checked");
            state::with_rooted(|s| {
                s.timers.push(state::TimerEntry {
                    id,
                    callback: entry.callback,
                    args: entry.args,
                    at: scheduled_at + iv,
                    interval,
                });
            });
        }
    }
    Ok(())
}
