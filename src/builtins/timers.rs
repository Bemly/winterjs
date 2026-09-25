//! timers：setTimeout/setInterval/clearTimeout/clearInterval。
//! 注册表存于 state::RootedState（RootedTraceableBox 跨 GC 保活）；
//! prelude 已把实参打包成 JS 数组、回调/数组都是真实 JS 对象，无裸栈值跨越 GC。

use std::time::{Duration, Instant};

use mozjs::context::JSContext;
use mozjs::jsapi::{Heap, JSObject};
use mozjs::jsval::{Int32Value, JSVal, UndefinedValue};
use mozjs::rooted;

use crate::error::Error;
use crate::jsapi_glue::{Frame, call_one, call_two, pending_exception_error, report_error, wrap_cx};
use crate::state;

/// Node 语义：`!(after >= 1 && after <= 2^31-1)` 一律钳 1——溢出同样走 1
/// （套件 test-timers.js：>2^31-1 按下一跳跑，非钳上限）。JS 面已做同口径
/// 钳制并发警告；此处对内建直接调用者（setImmediate 的 0ms 等）保底。
fn clamp_delay(ms: f64) -> f64 {
    if ms >= 1.0 && ms <= 2_147_483_647.0 {
        ms
    } else {
        1.0
    }
}

fn register_timer(cx: &mut JSContext, frame: &Frame, interval: bool) -> bool {
    // 前置条件：prelude 已做类型检查并打包实参；此处防御式再查
    let cb = frame.arg(0);
    let ms = frame.arg(1);
    let args = frame.arg(2);
    if !cb.is_object() || !args.is_object() {
        report_error(cx, "TypeError: invalid timer arguments");
        return false;
    }
    let delay_ms = if ms.is_number() { ms.to_number() } else { 0.0 };
    // 恰 0 且非 interval = setImmediate（JS 面 setTimeout 已钳 ≥1，唯一 0 来源）：不钳 1ms，
    // 本轮到期、下一轮 pump 触发（fire_due 快照到期集，回调内新排的 immediate 顺延一轮，
    // 同 node check 相）。修前每个 immediate 至少 1ms，读流/链式 immediate 被定时器甩开
    //（read-stream-pos 套件：写端 1ms interval 追加，读端每块 1ms 永远追不上）。
    let delay = if delay_ms == 0.0 && !interval {
        Duration::ZERO
    } else {
        Duration::from_secs_f64(clamp_delay(delay_ms) / 1e3)
    };
    let id = state::next_timer_id();

    // `Heap::boxed` 定址（set 后禁移动，见 §4.40；Vec push/interval 重排会搬运）。
    state::with_rooted(|s| {
        s.timers.push(state::TimerEntry {
            id,
            callback: Heap::boxed(cb),
            args: Heap::boxed(args),
            at: Instant::now() + delay,
            interval: interval.then_some(delay),
            period: delay,
            unrefed: false,
        });
    });
    tracing::debug!(target: "winterjs::timers", id, delay_ms, interval, "timer registered");
    frame.set_rval(Int32Value(id as i32));
    true
}

/// SAFETY: 由引擎以有效调用帧调用；prelude 保证 arg0=callback、arg1=ms、arg2=实参数组。
pub unsafe extern "C" fn set_timeout(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool { unsafe {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = wrap_cx(cx_raw);
    let frame = Frame::from_raw(vp, argc);
    register_timer(&mut cx, &frame, false)
}}

/// SAFETY: 同 set_timeout。
pub unsafe extern "C" fn set_interval(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool { unsafe {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = wrap_cx(cx_raw);
    let frame = Frame::from_raw(vp, argc);
    register_timer(&mut cx, &frame, true)
}}

/// SAFETY: 由引擎以有效调用帧调用；arg0 为数值 id。
pub unsafe extern "C" fn clear_timeout(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool { unsafe {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = wrap_cx(cx_raw);
    let frame = Frame::from_raw(vp, argc);
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
    let removed = state::with_plain(|p| {
        state::with_rooted(|s| {
            let before = s.timers.len();
            s.timers.retain(|t| t.id != id);
            if s.timers.len() == before {
                p.cleared_during_fire.insert(id);
                false
            } else {
                true
            }
        })
    });
    tracing::debug!(target: "winterjs::timers", id, removed, "timer cleared");
    frame.set_rval(UndefinedValue());
    true
}}

/// 事件循环里最近的触发时刻（unrefed 项不计——Node 口径：未 ref 的定时器
/// 不为事件循环续命；循环因他者存活时仍照常触发）。
pub fn next_deadline() -> Option<Instant> {
    state::with_rooted(|s| s.timers.iter().filter(|t| !t.unrefed).map(|t| t.at).min())
}

/// park 唤醒目标：含 unrefed 项。unrefed 定时器到点必须唤醒事件循环去触发
/// （存活判定/idle 仍走 refed-only 的 `next_deadline`；两者分离——套件
/// unrefd-interval-still-fires：refed 看门 1s、unrefed 1ms interval 须逐跳醒）。
pub fn next_wake() -> Option<Instant> {
    state::with_rooted(|s| s.timers.iter().map(|t| t.at).min())
}

/// 触发所有到期定时器（返回 (触发总数, 其中 unrefed 数)）。回调未捕获异常
/// → 先走 uncaughtException 分发，无监听 → Error::Script（Node 式 fatal）。
/// 前置条件：cx 已进入 global 所属 realm。
pub fn fire_due(
    cx: &mut JSContext,
    global: *mut JSObject,
    err: crate::runtime::ErrorSource<'_>,
    allow_unrefed: bool,
) -> Result<(usize, usize), Error> {
    // 快照到期 id（回调里可能再注册/清除，不能持借用调 JS）
    let due: Vec<u32> = state::with_rooted(|s| {
        let now = Instant::now();
        let mut ids: Vec<u32> = s
            .timers
            .iter()
            .filter(|t| t.at <= now && (allow_unrefed || !t.unrefed))
            .map(|t| t.id)
            .collect();
        ids.sort();
        ids
    });

    let mut fired: usize = 0;
    let mut unrefed_fired: usize = 0;
    for id in due {
        // 摘除条目（回调期间 clear 不必再摘），值复制进 rooted 栈槽
        let entry = state::with_rooted(|s| s.timers.iter().position(|t| t.id == id).map(|i| s.timers.remove(i)));
        let Some(entry) = entry else { continue };
        tracing::debug!(target: "winterjs::timers", id, "timer fired");
        let (cb, args) = (entry.callback.get(), entry.args.get());
        let interval = entry.interval;
        let scheduled_at = entry.at;
        if entry.unrefed {
            unrefed_fired += 1;
        }

        // 经 prelude `__wjs_call(cb, args)` 展开实参（10a 修：此前直调
        // `fun(cb, args数组)`，定时器实参从未展开——旧用例全用闭包故未暴露；
        // 复现 `tests/builtins.rs::phase10a_immediate_and_timeout_class`）。
        // 10f起回调为 prelude 闭包（this=Timeout 实例/ALS 恢复在 JS 侧闭环），
        // 返回布尔 false 表示 interval 不再重排（node processTimers 门）。
        let call_fn_v = state::with_rooted(|s| s.call_fn.get());
        let (ok, rval) = match call_two(cx, global, call_fn_v, cb, args) {
            Some(v) => (true, Some(v)),
            None => (false, None),
        };
        if !ok {
            // 未捕获异常：Node 口径先探 process 'uncaughtException' 监听器；
            // 无监听保持 pending 原样走既有 fatal（错误信息/栈不降级），
            // 有监听则取走异常逐个调用，已处理即吞掉继续（interval 照常重排）。
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
                // 清掉本轮回合的记账，进程即将退出
                state::with_plain(|p| p.cleared_during_fire.clear());
                return Err(match err {
                    crate::runtime::ErrorSource::Script { source, filename } => {
                        pending_exception_error(cx, global, source, filename)
                    }
                    crate::runtime::ErrorSource::Module { url } => {
                        crate::modules::module_error(cx, url)
                    }
                });
            }
        }
        fired += 1;

        // interval：漂移校正重排（scheduled_at + interval）；触发期间被 clear
        // 的不再排；JS step 闭包显式返回 false（_onTimeout 失效/_repeat 清失/
        // legacy `_idleTimeout = -1` unenroll）也不再排——异常路径无返回值，
        // 按 node finally 口径照旧重排（套件 interval-throw）。
        let cleared = state::with_plain(|p| p.cleared_during_fire.remove(&id));
        let rearm = match rval {
            Some(v) if v.is_boolean() => v.to_boolean(),
            _ => true,
        };
        if interval.is_some() && !cleared && rearm {
            let iv = interval.expect("checked");
            // unref 旗以 plain 侧账为准（触发期 unref：entry 已摘表，旗在
            // unrefed_ids——套件 unrefed-in-callback 的 callback 内 unref）。
            let unrefed_now =
                entry.unrefed || state::with_plain(|p| p.unrefed_ids.contains(&id));
            state::with_rooted(|s| {
                s.timers.push(state::TimerEntry {
                    id,
                    callback: entry.callback,
                    args: entry.args,
                    at: scheduled_at + iv,
                    interval,
                    period: entry.period,
                    unrefed: unrefed_now,
                });
            });
            tracing::trace!(target: "winterjs::timers", id, "interval rescheduled");
        }
    }
    Ok((fired, unrefed_fired))
}

/// SAFETY: 由引擎以有效调用帧调用；arg0=定时器 id（number）、arg1=refed 布尔。
/// 未知 id（已触发摘除等）no-op——node 对已 destroyed 句柄同样宽容。
pub unsafe extern "C" fn timer_ref(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool { unsafe {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = wrap_cx(cx_raw);
    let frame = Frame::from_raw(vp, argc);
    let _ = &mut cx;
    let id_v = if argc > 0 { frame.arg(0) } else { UndefinedValue() };
    let refed = if argc > 1 && frame.arg(1).is_boolean() {
        frame.arg(1).to_boolean()
    } else {
        true
    };
    if id_v.is_number() {
        let f = id_v.to_number();
        if f.is_finite() && f >= 0.0 {
            let id = f as u32;
            state::with_rooted(|s| {
                if let Some(t) = s.timers.iter_mut().find(|t| t.id == id) {
                    t.unrefed = !refed;
                }
            });
            // 触发期 unref 的侧账：entry 此刻已摘表（fire 中），重排在即，
            // 旗必须留在 plain 侧让重排读到（套件 unrefed-in-callback）。
            state::with_plain(|p| {
                if refed {
                    p.unrefed_ids.remove(&id);
                } else {
                    p.unrefed_ids.insert(id);
                }
            });
            tracing::debug!(target: "winterjs::timers", id, refed, "timer ref flag");
        }
    }
    frame.set_rval(UndefinedValue());
    true
}}

/// SAFETY: 由引擎以有效调用帧调用；arg0=定时器 id（number）。
/// node refresh 语义：起算点挪到现在；条目已触发摘除则 no-op（refresh-after-fire
/// 不重臂——偏差记档，套件 refresh 件走内部面不点名）。
pub unsafe extern "C" fn timer_refresh(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool { unsafe {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = wrap_cx(cx_raw);
    let frame = Frame::from_raw(vp, argc);
    let _ = &mut cx;
    let id_v = if argc > 0 { frame.arg(0) } else { UndefinedValue() };
    if id_v.is_number() {
        let f = id_v.to_number();
        if f.is_finite() && f >= 0.0 {
            let id = f as u32;
            state::with_rooted(|s| {
                if let Some(t) = s.timers.iter_mut().find(|t| t.id == id) {
                    t.at = Instant::now() + t.period;
                }
            });
            tracing::debug!(target: "winterjs::timers", id, "timer refreshed");
        }
    }
    frame.set_rval(UndefinedValue());
    true
}}

#[cfg(test)]
mod tests {
    use super::clamp_delay;

    #[test]
    fn clamp_delay_node_semantics() {
        // node lib/internal/timers.js 口径：`!(after >= 1 && after <= 2^31-1)`
        // 一律钳 1（套件 test-timers.js 注释：所有非法值按 1ms 跑）。
        assert_eq!(clamp_delay(0.0), 1.0);
        assert_eq!(clamp_delay(-10.0), 1.0);
        assert_eq!(clamp_delay(-0.5), 1.0);
        assert_eq!(clamp_delay(0.1), 1.0);
        assert_eq!(clamp_delay(f64::NAN), 1.0);
        assert_eq!(clamp_delay(f64::INFINITY), 1.0);
        assert_eq!(clamp_delay(f64::NEG_INFINITY), 1.0);
        assert_eq!(clamp_delay(1.0), 1.0);
        assert_eq!(clamp_delay(100.0), 100.0);
        assert_eq!(clamp_delay(2_147_483_647.0), 2_147_483_647.0);
        // 溢出钳 1 而非上限（test-timers.js：>2^31-1 按下一跳跑）。
        assert_eq!(clamp_delay(2_147_483_648.0), 1.0);
        assert_eq!(clamp_delay(12_345_678_901_234.0), 1.0);
    }
}
