//! JS 线程私有状态：timers 注册表、console 计数、未处理 rejection 清单、内部辅助函数值。
//! 只允许在 JS 独占线程访问（AGENTS §6）。所有持 JS 值的字段集中在 `RootedState`，
//! 经 `RootedTraceableBox` 整体跨 GC 保活；`PlainState` 不含 GC 指针。

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use mozjs::context::JSContext;
use mozjs::gc::{RootedTraceableBox, Traceable};
use mozjs::jsapi::{Heap, JSFunction, JS_GetFunctionObject, JS_NewFunction, JSObject, JSTracer};
use mozjs::jsval::{JSVal, ObjectValue, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{Frame, value_to_string};

/// 一个已注册的定时器。`at` 为触发时刻（interval 为上次触发 + 间隔，漂移校正）。
pub struct TimerEntry {
    pub id: u32,
    pub callback: Heap<JSVal>,
    pub args: Heap<JSVal>, // JS 数组，由 prelude 打包
    pub at: Instant,
    pub interval: Option<Duration>,
}

// SAFETY: 只追踪 GC 字段；Instant/Duration/u32 无 GC 指针。
unsafe impl Traceable for TimerEntry {
    unsafe fn trace(&self, trc: *mut JSTracer) {
        self.callback.trace(trc);
        self.args.trace(trc);
    }
}

/// 全部跨 GC 存活的 JS 值。
#[derive(Default)]
pub struct RootedState {
    pub timers: Vec<TimerEntry>,
    pub unhandled: Vec<Heap<*mut JSObject>>, // 未处理 rejection 的 promise
    pub call_fn: Heap<JSVal>,                // prelude 的 __wjs_call(cb, args)
    pub entries_fn: Heap<JSVal>,             // prelude 的 __wjs_entries(v)
    pub on_fulfilled: Heap<JSVal>,           // rejection 捕获用 native
    pub on_rejected: Heap<JSVal>,
}

// SAFETY: 同 TimerEntry，全字段 Traceable 或无 GC 指针。
unsafe impl Traceable for RootedState {
    unsafe fn trace(&self, trc: *mut JSTracer) {
        self.timers.trace(trc);
        self.unhandled.trace(trc);
        self.call_fn.trace(trc);
        self.entries_fn.trace(trc);
        self.on_fulfilled.trace(trc);
        self.on_rejected.trace(trc);
    }
}

/// 不含 GC 指针的状态。
#[derive(Default)]
pub struct PlainState {
    pub next_timer_id: u32,
    pub cleared_during_fire: HashSet<u32>,
    pub console_counts: HashMap<String, u32>,
    pub console_times: HashMap<String, Instant>,
    pub console_indent: usize,
    pub rejection_reasons: Vec<String>,
    /// eval 包装（async IIFE）引入的行偏移，报错行号统一校正。
    pub line_adjust: u32,
    /// 全局对象裸指针。前置条件：run() 里的 rooted! global 活过整个事件循环，
    /// 本指针只是它的借用副本，绝不在此之外解引用。
    pub global: *mut JSObject,
}

thread_local! {
    static ROOTED: RefCell<Option<RootedTraceableBox<RefCell<RootedState>>>> = RefCell::new(None);
    static PLAIN: RefCell<PlainState> = RefCell::new(PlainState::default());
}

pub fn line_adjust() -> u32 {
    PLAIN.with(|p| p.borrow().line_adjust)
}

pub fn set_line_adjust(n: u32) {
    PLAIN.with(|p| p.borrow_mut().line_adjust = n);
}

pub fn set_global(global: *mut JSObject) {
    PLAIN.with(|p| p.borrow_mut().global = global);
}

pub fn global() -> *mut JSObject {
    PLAIN.with(|p| p.borrow().global)
}

/// 引擎初始化后、首段脚本前调用一次。创建跨 GC 根与 rejection 捕获 natives。
pub fn init(cx: &mut JSContext) {
    ROOTED.with(|r| {
        let mut slot = r.borrow_mut();
        if slot.is_some() {
            return;
        }
        let boxed = RootedTraceableBox::new(RefCell::new(RootedState::default()));
        // SAFETY: cx 为引擎当前线程的活跃 context；raw 调用不触发 GC
        unsafe {
            let rcx = cx.raw_cx();
            let on_fulfilled = JS_NewFunction(
                rcx,
                Some(on_fulfilled_native),
                0,
                0,
                c"__wjs_onFulfilled".as_ptr(),
            );
            let on_rejected = JS_NewFunction(
                rcx,
                Some(on_rejected_native),
                1,
                0,
                c"__wjs_onRejected".as_ptr(),
            );
            assert!(!on_fulfilled.is_null() && !on_rejected.is_null(), "capture natives");
            {
                let mut s = boxed.borrow_mut();
                s.on_fulfilled.set(ObjectValue(JS_GetFunctionObject(on_fulfilled)));
                s.on_rejected.set(ObjectValue(JS_GetFunctionObject(on_rejected)));
            }
        }
        *slot = Some(boxed);
    });
}

/// 必须在引擎仍存活时调用（run() 结束前，经 StateGuard）：把 RootedTraceableBox
/// 从 thread_local 摘除并就地销毁，避免 TLS 析构晚于引擎导致的 SEGV/abort。
pub fn shutdown() {
    // 刻意泄漏 RootedState：Runtime 的 StoreBuffer 记有指向这些 Heap 槽位的边，
    // 若在引擎销毁前 drop 槽位，destroyRuntime 的小 GC 会解引用悬垂边而 SEGV。
    // 进程退出时由 OS 回收（AGENTS §4.8）。
    ROOTED.with(|r| {
        if let Some(boxed) = r.borrow_mut().take() {
            std::mem::forget(boxed);
        }
    });
}

/// run() 作用域守卫：无论正常返回还是 `?` 提前返回，都在引擎销毁前拆除 TLS 状态。
pub struct StateGuard;

impl Drop for StateGuard {
    fn drop(&mut self) {
        shutdown();
    }
}

pub fn with_rooted<R>(f: impl FnOnce(&mut RootedState) -> R) -> R {
    ROOTED.with(|r| {
        let slot = r.borrow_mut();
        let box_ref = slot.as_ref().expect("state::init not called");
        let mut s = box_ref.borrow_mut();
        f(&mut s)
    })
}

pub fn with_plain<R>(f: impl FnOnce(&mut PlainState) -> R) -> R {
    PLAIN.with(|p| f(&mut p.borrow_mut()))
}

pub fn next_timer_id() -> u32 {
    with_plain(|p| {
        p.next_timer_id += 1;
        p.next_timer_id
    })
}

// ── rejection 捕获 natives ─────────────────────────────────────────────

/// SAFETY: 由引擎以有效调用帧调用（JSNative 约定）。
unsafe extern "C" fn on_fulfilled_native(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper（JS 线程单实例）
    let mut cx = JSContext::from_ptr(std::ptr::NonNull::new_unchecked(cx_raw));
    let frame = unsafe { Frame::from_raw(vp, argc) };
    frame.set_rval(UndefinedValue());
    let _ = &mut cx;
    true
}

/// SAFETY: 同上；arg0 为 rejection reason。
unsafe extern "C" fn on_rejected_native(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = JSContext::from_ptr(std::ptr::NonNull::new_unchecked(cx_raw));
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let reason = if argc > 0 { frame.arg(0) } else { UndefinedValue() };
    let s = value_to_string(&mut cx, reason);
    with_plain(|p| p.rejection_reasons.push(s));
    frame.set_rval(UndefinedValue());
    true
}

/// 供 runtime 在事件循环收尾把捕获 natives 挂到未处理 promise 上。
pub fn capture_native_values() -> (JSVal, JSVal) {
    with_rooted(|s| (s.on_fulfilled.get(), s.on_rejected.get()))
}

/// `JSFunction*` → JS 值（调试/初始化辅助）。
pub fn function_value(fun: *mut JSFunction) -> JSVal {
    // SAFETY: fun 来自 JS_NewFunction 的有效返回
    unsafe { ObjectValue(JS_GetFunctionObject(fun)) }
}

/// console 计数等纯 Rust 状态访问（builtins 用）。
pub fn console_state<R>(f: impl FnOnce(&mut PlainState) -> R) -> R {
    with_plain(f)
}
