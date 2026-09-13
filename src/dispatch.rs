//! SM 异步任务派发桥（`JS::Dispatchable` → 本线程事件循环）。
//!
//! `WebAssembly.compile/instantiate` 等 Promise API 需要 embedder 装配
//! `DispatchToEventLoopCallback`：SM 153 下 `offThreadPromiseState` 未初始化时
//! 直接报 "WebAssembly Promise APIs not supported in this runtime."
//! （`WasmJS.cpp::EnsurePromiseSupport`）。经 mozjs_sys glue
//! `SetUpEventLoopDispatch` 装配：SM 把 `UniquePtr<Dispatchable>` 裸指针
//! （`DispatchablePointer`）交给我们，事件循环每轮经 dispatch 通道收到的指针
//! 在 JS 线程上调 `DispatchableRun`（内部 resolve 对应 promise 并
//! `unregisterTask`）——结算只排 microtask，pump 的 RunJobs 随后排空（§4.18）。
//!
//! 编译在飞期间事件循环不能判 idle（否则进程提前退出丢 promise）：
//! started/finished 回调 = `OffThreadPromiseTask::registerTask/unregisterTask`
//! 的镜像计数（SM 在任务注册/注销时回调，与派发解耦），idle 判定读它。
//!
//! 线程模型：dispatch 回调来自 SM 辅助线程（wasm off-thread 编译完成时），
//! 禁走 TLS（§4.24）——sender 经 `closure` 直达（每会话 leaked Box，
//! 与 §4.8 Runtime 泄漏同寿命；SM 侧 `EventLoopCallbackData` 亦刻意泄漏，
//! 悬垂风险不存在）。
//!
//! closure 语义（glue `jsglue.cpp`，实测 2026-09-14）：dispatch 回调拿到的是
//! `data->closure`（我们的 DispatchClosure 指针），而 started/finished 拿到的是
//! glue 的 `EventLoopCallbackData*`（SM 只把 InitAsyncTaskCallbacks 的 closure
//! 传给它们）——两者不可混读；把后者当 DispatchClosure 解引用，pending（offset 8）
//! 恰好覆盖 `data->closure`，fetch_add 会把我们的指针 +1，后续 dispatch 即
//! 错位崩（实测 0x...2b0 → 0x...2b1 panic）。started/finished 一律经 bindgen
//! 结构读 `data.closure` 还原真指针。
//!
//! 已知限制：delayedDispatch 未装配（glue 固定传 null）——
//! `Atomics.waitAsync` 的超时路径会走空指针（`AtomicsObject.cpp:1186`），
//! 本运行时不支持 waitAsync，勿用（M5 记档）。

use std::sync::atomic::{AtomicUsize, Ordering};

use mozjs::glue::{DispatchablePointer, DispatchableRun, SetUpEventLoopDispatch};
use mozjs::jsapi::{Dispatchable_MaybeShuttingDown, JSContext};

/// 每会话闭包：sender（跨线程投递 `*mut DispatchablePointer`）+ 后台任务计数。
/// Box::into_raw 泄漏给进程（SM 辅助线程可能在会话收尾后才派发最后一棒，
/// free 即悬垂；量级为每会话一个 sender，可忽略）。
struct DispatchClosure {
    tx: tokio::sync::mpsc::UnboundedSender<usize>,
    pending: AtomicUsize,
}

/// 后台任务在飞数（本会话）。worker 会话各持各的闭包，互不可见。
pub fn pending() -> usize {
    crate::state::with_plain(|p| match p.dispatch_closure {
        Some(ptr) => {
            // SAFETY: closure 由 install 泄漏，进程存活期恒有效（见结构体注释）
            let c = unsafe { &*(ptr as *const DispatchClosure) };
            c.pending.load(Ordering::SeqCst)
        }
        None => 0,
    })
}

/// started/finished 的 closure 是 glue `EventLoopCallbackData*`（见模块注释）；
/// 经 bindgen 结构读出我们 install 时塞进 `data.closure` 的真指针。
///
/// SAFETY: closure 必须来自 SM 回调（SetUpEventLoopDispatch 装配的同一 data）；
/// 只读指针字段，无 GC。
unsafe fn unwrap_glue_data<'a>(closure: *mut std::ffi::c_void) -> &'a DispatchClosure {
    unsafe {
        let data = &*(closure as *const mozjs::glue::EventLoopCallbackData);
        &*(data.closure as *const DispatchClosure)
    }
}

/// SAFETY: SM 回调；可能来自辅助线程。只投递指针，无 GC/JSAPI/TLS。
unsafe extern "C" fn on_dispatch(
    closure: *mut std::ffi::c_void,
    ptr: *mut DispatchablePointer,
) -> bool {
    unsafe {
        let c = &*(closure as *const DispatchClosure);
        c.tx.send(ptr as usize).is_ok()
    }
}

/// SAFETY: SM 回调（JS 线程，registerTask 时）；只计数。
unsafe extern "C" fn on_started(
    closure: *mut std::ffi::c_void,
    _task: *mut mozjs::jsapi::Dispatchable,
) {
    unsafe {
        let c = unwrap_glue_data(closure);
        c.pending.fetch_add(1, Ordering::SeqCst);
    }
}

/// SAFETY: SM 回调（JS 线程，unregisterTask 时）；只计数。
unsafe extern "C" fn on_finished(
    closure: *mut std::ffi::c_void,
    _task: *mut mozjs::jsapi::Dispatchable,
) {
    unsafe {
        let c = unwrap_glue_data(closure);
        let prev = c.pending.fetch_sub(1, Ordering::SeqCst);
        debug_assert!(prev > 0, "dispatch task underflow");
    }
}

/// 会话装配（JS 线程，init_session 内 jobqueue 装配点同层）。
/// 返回接收端给事件循环；闭包指针存进 PlainState（pending 读取 + 身份归属）。
pub fn install(
    cx: *mut JSContext,
) -> (tokio::sync::mpsc::UnboundedReceiver<usize>, *mut std::ffi::c_void) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<usize>();
    let closure = Box::into_raw(Box::new(DispatchClosure {
        tx,
        pending: AtomicUsize::new(0),
    }));
    // SAFETY: cx 有效（调用方 realm 内）；装配回调与闭包。glue 内部转交
    // JS::InitAsyncTaskCallbacks（runtime 级状态，每 Runtime 一次；
    // 本 Runtime 刻意泄漏，无二次装配）。
    unsafe {
        SetUpEventLoopDispatch(&mut *cx, Some(on_dispatch), Some(on_started), Some(on_finished), closure.cast());
    }
    (rx, closure.cast())
}

/// 事件循环单轮：运行一个本轮到达的 Dispatchable（必须 JS 线程 + realm 内）。
/// 内部 resolve 对应 promise（结算只排 microtask）；pump 的 RunJobs 随后排空。
pub fn run_dispatchable(cx: *mut JSContext, ptr: usize) {
    // SAFETY: 指针来自 on_dispatch 入队（SM 所有权移交），此后归我们；
    // cx 有效（调用方 realm 内）；DispatchableRun 内部
    // `JS::Dispatchable::Run` + delete wrapper。
    unsafe {
        DispatchableRun(
            &mut *cx,
            ptr as *mut DispatchablePointer,
            Dispatchable_MaybeShuttingDown::NotShuttingDown,
        );
    }
}
