//! napi 异步面（plan-napi M3）：async_work + TSFN，共用事件循环第 8 通道
//! `napi_rx`（fetch/net/worker 同构模式：OS 线程 → channel → JS 线程 dispatch）。
//!
//! - 通道 Sender 随记录走（create 时在 JS 线程从 `state::napi_tx` 捕获，存入
//!   rec）——**OS 线程侧禁经 TLS 取通道**（with_plain 是 JS 线程 TLS，§4.24）。
//! - async_work：queue 即 spawn OS 线程跑 `execute`（**禁 JSAPI**，Node 同约束）；
//!   完成经 `NapiEvent::AsyncDone` 回 JS 线程跑 `complete`（JSAPI 允许，包裹在
//!   槽位水位截断内——同 trampoline 语义）。pending 计数喂事件循环 keep-alive，
//!   queue ++ / dispatch --（cancel 只改状态不改计数，防止双减）。
//!   cancel = started 位竞速：赢者以 `napi_cancelled` 状态走 complete。
//! - TSFN：队列在 `Arc<TsfnShared>`（任意线程 push；JS 线程 dispatch 排空），
//!   `call_js_cb(env, js_callback, context, data)` 逐条在 JS 线程执行——
//!   js_callback 临时落 env 槽位（排空段截断回收）。closing/thread_count/
//!   loop_ref 全原子；loop_ref 喂 keep-alive，unref 后队列可残留（Node 口径：
//!   循环可先退）。thread_finalize_cb 在 closing+清空+0 线程时由 dispatch 触发。
//! - 偏差（记 plan-napi §4）：async_hooks/async_context 未接（async_resource
//!   收下不消费）。

use std::collections::VecDeque;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use mozjs::jsapi::Heap;
use mozjs::jsval::JSVal;

use crate::napi::api::{e, NAPI_GENERIC_FAILURE, NAPI_INVALID_ARG, NAPI_OK};
use crate::napi::sys;
use crate::napi::sys::{
    napi_async_complete_callback, napi_async_execute_callback, napi_async_work, napi_env,
    napi_finalize, napi_status, napi_threadsafe_function,
    napi_threadsafe_function_call_js, napi_threadsafe_function_call_mode,
    napi_threadsafe_function_release_mode, napi_value,
};

/// 第 8 通道事件（OS 线程 → JS 线程；仅 id/status，Send 安全）。
pub enum NapiEvent {
    /// async_work 的 execute 已在线程跑完（或被取消）；JS 线程跑 complete。
    /// complete/data 随事件携带——addon 可在 complete 前合法 delete（Node
    /// 语义：cancelled 的 complete 必达），按 id 查表会扑空丢回调。
    AsyncDone { status: napi_status, complete: napi_async_complete_callback, data_addr: usize },
    /// TSFN 队列有新数据（data 已入共享队列）/状态变化；JS 线程排空。
    TsfnDrain { id: u64 },
    /// ref/loop 计数变化（park 中的循环醒来重估 idle/退出）。
    Recount,
}

pub(crate) struct AsyncWorkRec {
    pub execute: napi_async_execute_callback,
    pub complete: napi_async_complete_callback,
    pub data: *mut c_void,
    /// execute 入口竞速位（cancel 与线程赛跑；赢者决定 complete 状态）。
    pub started: Arc<AtomicBool>,
    /// JS 线程捕获的通道发送端（线程回发 AsyncDone 用）。
    pub tx: tokio::sync::mpsc::UnboundedSender<NapiEvent>,
}

pub(crate) struct TsfnShared {
    queue: Mutex<VecDeque<*mut c_void>>,
    space: Condvar,
    max_queue: usize,
    closing: AtomicBool,
    thread_count: AtomicU32,
    /// 事件循环 keep-alive 引用数（ref/unref 面；与 thread_count 无关）。
    loop_ref: AtomicI32,
    /// addon context（create 后只读——任意线程经 Arc 读安全，Node 契约）。
    pub context: *mut c_void,
    /// 会话内 id（dispatch 反查 rec 用）。
    pub id: u64,
    /// 第 8 通道发送端（create 时捕获；任意线程 ping 零 TLS——§4.24）。
    pub tx: tokio::sync::mpsc::UnboundedSender<NapiEvent>,
}

pub(crate) struct TsfnRec {
    pub js_cb: Box<Heap<JSVal>>,
    pub call_js_cb: napi_threadsafe_function_call_js,
    pub shared: Arc<TsfnShared>,
    pub thread_finalize_data: *mut c_void,
    pub thread_finalize_cb: napi_finalize,
    pub finalized: bool,
}

impl TsfnShared {
    fn new(
        max_queue: usize,
        initial_thread_count: u32,
        context: *mut c_void,
        id: u64,
        tx: tokio::sync::mpsc::UnboundedSender<NapiEvent>,
    ) -> Self {
        TsfnShared {
            queue: Mutex::new(VecDeque::new()),
            space: Condvar::new(),
            max_queue,
            closing: AtomicBool::new(false),
            thread_count: AtomicU32::new(initial_thread_count),
            loop_ref: AtomicI32::new(1),
            context,
            id,
            tx,
        }
    }
}

/// keep-alive 聚合（事件循环 idle 判定用；JS 线程读，state::napi_pending 转发）。
pub fn pending_count() -> usize {
    let Some(env_ptr) = crate::state::napi_env_ptr() else {
        return 0;
    };
    // SAFETY：JS 线程读（事件循环）；env 会话存续。
    let env = unsafe { &mut *env_ptr };
    env.async_pending
        + env
            .tsfns
            .values()
            // closing（0 线程 release/abort）即不再 keep-alive（Node 口径：
            // 关闭后的 TSFN 不挡事件循环退出）。
            .filter(|t| {
                !t.shared.closing.load(Ordering::SeqCst)
                    && t.shared.loop_ref.load(Ordering::SeqCst) > 0
            })
            .count()
}

// ── async_work ───────────────────────────────────────────────────────────

/// # Safety
/// N-API 约定（vendored node_api.h:157）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_async_work(
    env: napi_env,
    async_resource: napi_value,
    async_resource_name: napi_value,
    execute: napi_async_execute_callback,
    complete: napi_async_complete_callback,
    data: *mut c_void,
    result: *mut napi_async_work,
) -> napi_status {
    let _ = (async_resource, async_resource_name);
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(tx) = crate::state::with_plain(|p| p.napi_tx.clone()) else {
        return NAPI_GENERIC_FAILURE;
    };
    let env_ref = unsafe { e(env) };
    env_ref.next_napi_id += 1;
    let id = env_ref.next_napi_id;
    env_ref.async_works.insert(
        id,
        AsyncWorkRec {
            execute,
            complete,
            data,
            started: Arc::new(AtomicBool::new(false)),
            tx,
        },
    );
    // 句柄 = id（非零；napi_async_work 为不透明指针类型，按位承载 id）。
    // SAFETY：出参为 addon 提供的合法指针。
    unsafe { *result = id as napi_async_work };
    NAPI_OK
}

/// # Safety
/// N-API 约定（vendored node_api.h:169 附近；execute 侧禁 JSAPI——Node 同约束）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_queue_async_work(
    env: napi_env,
    work: napi_async_work,
) -> napi_status {
    let id = work as usize as u64;
    let env_ref = unsafe { e(env) };
    let Some(rec) = env_ref.async_works.get(&id) else {
        env_ref.set_last_error("invalid napi_async_work");
        return NAPI_INVALID_ARG;
    };
    let started = rec.started.clone();
    let execute = rec.execute;
    let rec_complete = rec.complete;
    let data_addr = rec.data as usize;
    let tx = rec.tx.clone();
    let env_addr = env as usize;
    env_ref.async_pending += 1;
    // SAFETY：线程闭包只持 fn 指针/地址位/原子位/通道（raw 指针按 usize 携带
    // 满足 Send）——env 指针随会话泄漏存续（§4.8 语义），execute 契约禁
    // JSAPI（Node 同）。
    std::thread::spawn(move || {
        if !started.swap(true, Ordering::SeqCst) {
            // SAFETY：execute 为 addon 函数指针（禁 JSAPI 契约，Node 同）。
            unsafe {
                if let Some(f) = execute {
                    f(env_addr as napi_env, data_addr as *mut c_void);
                }
            }
            let _ = tx.send(NapiEvent::AsyncDone {
                status: NAPI_OK,
                complete: rec_complete,
                data_addr,
            });
        }
    });
    NAPI_OK
}

/// # Safety
/// N-API 约定（仅未起跑可取消；已起跑 = generic_failure）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_cancel_async_work(
    env: napi_env,
    work: napi_async_work,
) -> napi_status {
    let id = work as usize as u64;
    let env_ref = unsafe { e(env) };
    let Some(rec) = env_ref.async_works.get(&id) else {
        env_ref.set_last_error("invalid napi_async_work");
        return NAPI_INVALID_ARG;
    };
    let started = rec.started.clone();
    let tx = rec.tx.clone();
    let rec_complete = rec.complete;
    let rec_data_addr = rec.data as usize;
    // 原子位竞速：赢者代发 cancelled 完成（线程侧 swap 失败即跳过）。
    if started.swap(true, Ordering::SeqCst) {
        return NAPI_GENERIC_FAILURE;
    }
    // 计数由 dispatch 统一 --（cancel 只改状态，防双减）；
    // complete/data 随事件走（此后 addon delete 不影响必达）。
    let _ = tx.send(NapiEvent::AsyncDone {
        status: sys::napi_status_napi_cancelled,
        complete: rec_complete,
        data_addr: rec_data_addr,
    });
    NAPI_OK
}

/// # Safety
/// N-API 约定（complete 跑完或取消后删；提前删 = complete 不到但计数平衡）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_delete_async_work(
    env: napi_env,
    work: napi_async_work,
) -> napi_status {
    let id = work as usize as u64;
    let env_ref = unsafe { e(env) };
    if env_ref.async_works.remove(&id).is_none() {
        env_ref.set_last_error("invalid napi_async_work");
        return NAPI_INVALID_ARG;
    }
    NAPI_OK
}

// ── TSFN ─────────────────────────────────────────────────────────────────

/// # Safety
/// N-API 约定（vendored js_native_api.h TSFN 段）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_threadsafe_function(
    env: napi_env,
    func: napi_value,
    async_resource: napi_value,
    async_resource_name: napi_value,
    max_queue_size: usize,
    initial_thread_count: u32,
    thread_finalize_data: *mut c_void,
    thread_finalize_cb: napi_finalize,
    context: *mut c_void,
    call_js_cb: napi_threadsafe_function_call_js,
    result: *mut napi_threadsafe_function,
) -> napi_status {
    let _ = (async_resource, async_resource_name);
    // func（js_callback）可空：napi-rs 的 JsDeferred 恒传 null（call_js_cb 内
    // 直接 resolve deferred，不经 JS 函数——Node 同契约）。
    if result.is_null() || call_js_cb.is_none() {
        return NAPI_INVALID_ARG;
    }
    let v = if func.is_null() {
        mozjs::jsval::UndefinedValue()
    } else {
        let v = unsafe { e(env).get(func) };
        if !v.is_object() {
            return NAPI_INVALID_ARG;
        }
        v
    };
    let Some(tx) = crate::state::with_plain(|p| p.napi_tx.clone()) else {
        return NAPI_GENERIC_FAILURE;
    };
    let env_ref = unsafe { e(env) };
    env_ref.next_napi_id += 1;
    let id = env_ref.next_napi_id;
    let shared = Arc::new(TsfnShared::new(
        max_queue_size,
        initial_thread_count,
        context,
        id,
        tx,
    ));
    let rec = TsfnRec {
        // SAFETY：js 回调值进 Heap（env trace 追值，GC 安全）。
        js_cb: Heap::boxed(v),
        call_js_cb,
        shared: shared.clone(),
        thread_finalize_data,
        thread_finalize_cb,
        finalized: false,
    };
    // 句柄 = Arc 裸指针（自包含：任意线程 from_raw/into_raw 往返，零 TLS——
    // §4.24；句柄不回收，会话泄漏语义 §4.8）。
    let handle = Arc::into_raw(shared) as napi_threadsafe_function;
    // SAFETY：出参为 addon 提供的合法指针。
    unsafe { *result = handle };
    env_ref.tsfns.insert(id, rec);
    NAPI_OK
}

/// 句柄（Arc 裸指针）→ shared 克隆（call/acquire/release/ref/unref 共用；
/// 任意线程，零 TLS——§4.24）。
///
/// # Safety
/// `tsfn` 须为本 crate `Arc::into_raw` 发出的句柄（from_raw/into_raw 严格
/// 配对往返；Arc 强引用计数经此恢复，泄漏语义见 create 注）。
unsafe fn tsfn_shared(tsfn: napi_threadsafe_function) -> Option<Arc<TsfnShared>> {
    let ptr = tsfn as *const TsfnShared;
    if ptr.is_null() {
        return None;
    }
    // SAFETY：from_raw 恢复所有权 → clone → into_raw 归还（计数守恒）。
    unsafe {
        let shared = Arc::from_raw(ptr);
        let out = shared.clone();
        // into_raw 归还所有权（泄漏语义见 create 注；#[must_use] 显式丢弃）。
        let _ = Arc::into_raw(shared);
        Some(out)
    }
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_threadsafe_function_context(
    tsfn: napi_threadsafe_function,
    result: *mut *mut c_void,
) -> napi_status {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(shared) = (unsafe { tsfn_shared(tsfn) }) else {
        return NAPI_INVALID_ARG;
    };
    // SAFETY：出参为 addon 提供的合法指针；context create 后只读（任意线程）。
    unsafe { *result = shared.context };
    NAPI_OK
}

/// # Safety
/// N-API 约定（任意线程；blocking 模式队列满时挂起等排空）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_call_threadsafe_function(
    tsfn: napi_threadsafe_function,
    data: *mut c_void,
    mode: napi_threadsafe_function_call_mode,
) -> napi_status {
    let Some(shared) = (unsafe { tsfn_shared(tsfn) }) else {
        return NAPI_INVALID_ARG;
    };
    if shared.closing.load(Ordering::SeqCst) {
        return sys::napi_status_napi_closing;
    }
    {
        let mut q = shared.queue.lock().expect("tsfn queue mutex");
        if shared.max_queue > 0 {
            while q.len() >= shared.max_queue {
                if mode == sys::napi_threadsafe_function_call_mode_napi_tsfn_nonblocking {
                    return sys::napi_status_napi_queue_full;
                }
                q = shared.space.wait(q).expect("tsfn condvar");
                if shared.closing.load(Ordering::SeqCst) {
                    return sys::napi_status_napi_closing;
                }
            }
        }
        q.push_back(data);
    }
    shared.space.notify_all();
    ping_drain(&shared);
    NAPI_OK
}

/// drain ping（经 shared.tx——任意线程零 TLS；会话收尾后发送失败即静默，
/// §4.49 parked 收件箱语义）。
fn ping_drain(shared: &TsfnShared) {
    let _ = shared.tx.send(NapiEvent::TsfnDrain { id: shared.id });
}

/// # Safety
/// N-API 约定（closing 后返回 napi_closing）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_acquire_threadsafe_function(
    tsfn: napi_threadsafe_function,
) -> napi_status {
    let Some(shared) = (unsafe { tsfn_shared(tsfn) }) else {
        return NAPI_INVALID_ARG;
    };
    if shared.closing.load(Ordering::SeqCst) {
        return sys::napi_status_napi_closing;
    }
    shared.thread_count.fetch_add(1, Ordering::SeqCst);
    NAPI_OK
}

/// # Safety
/// N-API 约定（release → 0 线程即 closing；abort 即时关闭并清队列）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_release_threadsafe_function(
    tsfn: napi_threadsafe_function,
    mode: napi_threadsafe_function_release_mode,
) -> napi_status {
    let Some(shared) = (unsafe { tsfn_shared(tsfn) }) else {
        return NAPI_INVALID_ARG;
    };
    let prev = shared.thread_count.fetch_sub(1, Ordering::SeqCst);
    if prev == 0 {
        // Node 口径：0 上再 release = invalid_arg（计数回补）。
        shared.thread_count.fetch_add(1, Ordering::SeqCst);
        return NAPI_INVALID_ARG;
    }
    if mode == sys::napi_threadsafe_function_release_mode_napi_tsfn_abort {
        shared.closing.store(true, Ordering::SeqCst);
        shared.queue.lock().expect("tsfn queue mutex").clear();
        shared.space.notify_all();
    } else if shared.thread_count.load(Ordering::SeqCst) == 0 {
        shared.closing.store(true, Ordering::SeqCst);
        shared.space.notify_all();
    }
    ping_drain(&shared); // 触发终化检查/排空
    NAPI_OK
}

/// # Safety
/// N-API 约定（unref 后事件循环可先退，队列残留待会话收尾丢弃）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_unref_threadsafe_function(
    env: napi_env,
    tsfn: napi_threadsafe_function,
) -> napi_status {
    let _ = env;
    let Some(shared) = (unsafe { tsfn_shared(tsfn) }) else {
        return NAPI_INVALID_ARG;
    };
    shared.loop_ref.fetch_sub(1, Ordering::SeqCst);
    ping_recount(&shared);
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_ref_threadsafe_function(
    env: napi_env,
    tsfn: napi_threadsafe_function,
) -> napi_status {
    let _ = env;
    let Some(shared) = (unsafe { tsfn_shared(tsfn) }) else {
        return NAPI_INVALID_ARG;
    };
    shared.loop_ref.fetch_add(1, Ordering::SeqCst);
    ping_recount(&shared);
    NAPI_OK
}

/// Recount ping（unref/ref 后唤醒 park 中的循环重估退出条件；经 shared.tx）。
fn ping_recount(shared: &TsfnShared) {
    let _ = shared.tx.send(NapiEvent::Recount);
}

// ── dispatch（JS 线程；事件循环第 8 通道结算点）─────────────────────────

/// 通道结算（pump 调；签名对齐 net/worker dispatch）。
pub fn dispatch(
    cx: &mut mozjs::context::JSContext,
    global: *mut mozjs::jsapi::JSObject,
    ev: NapiEvent,
    err: crate::runtime::ErrorSource<'_>,
) -> Result<(), crate::error::Error> {
    // TSFN/async_work 回调抛错即 pending 异常——不收敛则污染后续一切 JSAPI
    // （M4-③ loader register 同类：遗留 pending 必转可读错，不可静默 Ok）。
    let failed = |cx: &mut mozjs::context::JSContext| match err {
        crate::runtime::ErrorSource::Script { source, filename } => {
            crate::jsapi_glue::pending_exception_error(cx, global, source, filename)
        }
        crate::runtime::ErrorSource::Module { url } => crate::modules::module_error(cx, url),
    };
    // SAFETY：JS 线程；JS_IsExceptionPending 读 pending 位无副作用。
    let pending = |cx: &mut mozjs::context::JSContext| unsafe {
        mozjs::jsapi::JS_IsExceptionPending(cx.raw_cx())
    };
    let Some(env_ptr) = crate::state::napi_env_ptr() else {
        return Ok(());
    };
    match ev {
        NapiEvent::AsyncDone { status, complete, data_addr } => {
            // SAFETY：JS 线程；complete 调用包裹槽位水位截断（同 trampoline）。
            // complete 随事件携带（Node 语义：cancelled 的 complete 必达，
            // addon 在 complete 前合法 delete 不影响）。
            unsafe {
                let env = &mut *env_ptr;
                env.async_pending = env.async_pending.saturating_sub(1);
                if let Some(f) = complete {
                    let mark = env.slots.len();
                    let escape_base = env.escape_slots.len();
                    f(env_ptr as napi_env, status, data_addr as *mut c_void);
                    let env = &mut *env_ptr;
                    env.slots.truncate(mark);
                    crate::napi::scope::escape_truncate_to(env_ptr as napi_env, escape_base);
                    if pending(cx) {
                        return Err(failed(cx));
                    }
                }
            }
            Ok(())
        }
        NapiEvent::TsfnDrain { id } => {
            // SAFETY：JS 线程；逐条 call_js_cb 包裹槽位水位截断。
            unsafe {
                let env = &mut *env_ptr;
                let Some(rec) = env.tsfns.get(&id) else {
                    return Ok(());
                };
                let shared = rec.shared.clone();
                let call_js_cb = match rec.call_js_cb {
                    Some(f) => f,
                    None => return Ok(()),
                };
                let context = shared.context;
                let cb_v = rec.js_cb.get();
                let finalize_data = rec.thread_finalize_data;
                let finalize_cb = rec.thread_finalize_cb;
                let mut mark = env.slots.len();
                let escape_base = env.escape_slots.len();
                loop {
                    let item = {
                        let mut q = shared.queue.lock().expect("tsfn queue mutex");
                        q.pop_front()
                    };
                    shared.space.notify_all();
                    let Some(data) = item else { break };
                    let cb_val = env.put(cb_v);
                    call_js_cb(env_ptr as napi_env, cb_val, context, data);
                    let env = &mut *env_ptr;
                    env.slots.truncate(mark);
                    crate::napi::scope::escape_truncate_to(env_ptr as napi_env, escape_base);
                    if pending(cx) {
                        return Err(failed(cx));
                    }
                    mark = env.slots.len();
                }
                // 终化检查：closing + 队列空 + 0 线程 → thread_finalize_cb 一次。
                if shared.closing.load(Ordering::SeqCst)
                    && shared.thread_count.load(Ordering::SeqCst) == 0
                    && shared.queue.lock().expect("tsfn queue mutex").is_empty()
                {
                    let env = &mut *env_ptr;
                    if let Some(rec) = env.tsfns.get_mut(&id) {
                        if !rec.finalized {
                            rec.finalized = true;
                            if let Some(f) = finalize_cb {
                                f(env_ptr as napi_env, finalize_data, ptr::null_mut());
                            }
                        }
                    }
                }
            }
            Ok(())
        }
        NapiEvent::Recount => Ok(()), // 仅唤醒循环重估 idle
    }
}
