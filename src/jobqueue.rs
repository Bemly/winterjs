//! Promise 微任务队列（SM 153 需要 embedding 提供 JS::JobQueue）。
//!
//! 为什么不用 `js::UseInternalJobQueues`：实测（2026-09-10）在最小 Runtime 下调用即 SEGV
//! （realm 内外皆崩，原因未深究——AGENTS §4.7 记坑）。改用 mozjs_sys 自带的 RustJobQueue
//! glue（servo 同款）：traps 的 runJobs 由 Rust 用 MicroTask 朋友 API 排空
//! （PeekNextMicroTask / DequeueNextMicroTask / RunJSMicroTask）。

use mozjs::glue::{CallValueTracer, CreateJobQueue, JobQueueTraps};
use mozjs::jsapi::{
    GetExecutionGlobalFromJSMicroTask, IsJSMicroTask, JSObject, JSTracer, PeekNextMicroTask,
    RunJSMicroTask, SetJobQueue, ToUnwrappedJSMicroTask,
};
use mozjs::jsval::JSVal;
use mozjs::realm::AutoRealm;
use mozjs::rooted;

use crate::jsapi_glue::{raw_handle, wrap_cx};

/// SAFETY: 引擎回调；只读陷阱，无 GC。
unsafe extern "C" fn get_host_defined_data(
    _cx: *mut mozjs::jsapi::JSContext,
    _incumbent_global: mozjs::jsapi::MutableHandle<*mut JSObject>,
    _optional_host_defined_data: mozjs::jsapi::MutableHandle<*mut JSObject>,
) -> bool {
    // Phase 1 不提供 host defined data；引擎对未设置的处理走默认路径
    true
}

/// SAFETY: 同上。
unsafe extern "C" fn get_host_defined_global(
    _cx: *mut mozjs::jsapi::JSContext,
    _data: mozjs::jsapi::MutableHandle<*mut JSObject>,
) -> bool {
    true
}

/// SAFETY: 引擎在 JS::RunJobs 里回调；此处排空 microtask 队列直到为空。
unsafe extern "C" fn run_jobs(cx: *mut mozjs::jsapi::JSContext) { unsafe {
    let mut drained: u64 = 0;
    let mut swallowed: u64 = 0;
    loop {
        let task: JSVal = PeekNextMicroTask(cx);
        if task.is_null_or_undefined() {
            break;
        }
        let task: JSVal = mozjs::jsapi::DequeueNextRegularMicroTask(cx);
        if IsJSMicroTask(&task) {
            let entry = ToUnwrappedJSMicroTask(&task);
            // SAFETY: entry 摘自队列后立即 rooted，RunJSMicroTask 期间 GC 安全
            rooted!(in(cx) let entry_root: *mut JSObject = entry);
            // SM 内部队 runJobs 同款：先取任务执行 global 并 AutoRealm 进去再跑。
            // RunJSMicroTask 的 DEBUG assert（execution global == cx->global()）
            // 要求调用方先进任务 realm——vm 等 cross-compartment 微任务在主域
            // 排空时漏这步即 assert abort（exit 139，test-vm-script-after-evaluate
            // 现形）；无执行 global 的任务 SM 内部队同款 continue 跳过。
            let eg = GetExecutionGlobalFromJSMicroTask(entry_root.get());
            if eg.is_null() {
                continue;
            }
            rooted!(in(cx) let eg_root: *mut JSObject = eg);
            let mut cxw = wrap_cx(cx);
            let mut realm = AutoRealm::new(&mut cxw, std::ptr::NonNull::new(eg).unwrap());
            // SAFETY: 任务与执行 global 均 rooted，realm 内调用
            let ok = RunJSMicroTask(realm.raw_cx(), raw_handle(entry_root.as_ptr()));
            if !ok {
                // 微任务内抛异常：状态留在 pending exception，由上层 rejection/错误
                // 路径处理；继续排空避免队列阻塞。
                mozjs::jsapi::JS_ClearPendingException(cx);
                swallowed += 1;
            }
            drained += 1;
        }
    }
    tracing::trace!(target: "winterjs2::jobqueue", drained, swallowed, "microtasks drained");
}}

/// SAFETY: 引擎 GC 时回调；追踪队列里以 JS::Value 存放的非 JS microtask。
unsafe extern "C" fn trace_non_gc_thing_micro_task(trc: *mut JSTracer, value_ptr: *mut JSVal) { unsafe {
    // CallValueTracer 的 C++ 形参是 JS::Heap<Value>*；glue 侧 vtable 传的是同一槽位的
    // JS::Value*（布局一致），按 servo 的用法原地转交
    let heap_ptr = value_ptr as *mut mozjs::jsapi::Heap<JSVal>;
    CallValueTracer(trc, heap_ptr, c"winterjs2-microtask".as_ptr());
}}

static JOB_QUEUE_TRAPS: JobQueueTraps = JobQueueTraps {
    getHostDefinedData: Some(get_host_defined_data),
    getHostDefinedGlobal: Some(get_host_defined_global),
    runJobs: Some(run_jobs),
    traceNonGCThingMicroTask: Some(trace_non_gc_thing_micro_task),
};

/// 创建并安装 RustJobQueue。返回的指针故意泄漏（CLI 进程级单例，与 Runtime 同生命周期）。
///
/// # Safety
/// cx 必须是当前线程活跃的引擎 context，且首段脚本求值前调用。
pub unsafe fn install(cx: *mut mozjs::jsapi::JSContext) { unsafe {
    let queue = CreateJobQueue(&JOB_QUEUE_TRAPS);
    assert!(!queue.is_null(), "CreateJobQueue failed");
    SetJobQueue(cx, queue as *mut mozjs::jsapi::JobQueue);
}}
