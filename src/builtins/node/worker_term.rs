//! worker 线程生命期（spawn/terminate/boot/route；对齐 worker.rs；纯搬移）。

use mozjs::jsval::JSVal;
use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};
use crate::state;
use super::worker::WorkerEvent;
use std::sync::{Mutex, OnceLock};
use std::collections::HashMap;

// ── Worker 线程（9f-3）───────────────────────────────────────────────────

/// worker 线程 id 分配（进程级；主=0，worker 自 1 单调）。
static NEXT_THREAD_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

// ── terminate 中断（10f：busy JS 循环可被打断）──────────────────────────
// `terminate()` 原理：置共享旗 + `JS_RequestInterruptCallback`（Interrupt.h：
// any thread 可请求，JS 线程在解释器 CheckForInterrupt 处回调）。回调返回
// false 即引擎以 uncatchable interrupt 中止当前脚本——忙循环/微任务环/
// nextTick 环全被斩断，随后事件循环检查点照常退出（WExit{1}）。

/// 进程级终止槽（worker_id → 共享旗 + worker cx 裸指针；cx 经 §4.8 Runtime
/// 泄漏保活，指针终身有效）。
pub struct TerminateSlot {
    pub flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub cx: std::sync::atomic::AtomicUsize,
}

static TERMINATE_SLOTS: OnceLock<Mutex<HashMap<u64, std::sync::Arc<TerminateSlot>>>> = OnceLock::new();

fn term_slots() -> &'static Mutex<HashMap<u64, std::sync::Arc<TerminateSlot>>> {
    TERMINATE_SLOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

thread_local! {
    /// worker 线程自己的共享旗裸指针（interrupt 回调只读原子旗，禁碰 TLS
    /// RefCell/Arc 克隆——回调可能在任意 Rust 借用期内的 JS 执行点触发）。
    /// 存活由 TERM_FLAG_KEEP 的 Arc 保证（bind 一次 set，永不读）。
    static TERM_FLAG_PTR: std::cell::Cell<*const std::sync::atomic::AtomicBool> =
        const { std::cell::Cell::new(std::ptr::null()) };
    static TERM_FLAG_KEEP: std::cell::Cell<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>> =
        const { std::cell::Cell::new(None) };
}

/// worker 线程入口绑定旗（`run_worker_thread` 线程闭包内首调）。
pub fn term_tls_bind(flag: std::sync::Arc<std::sync::atomic::AtomicBool>) {
    TERM_FLAG_PTR.with(|t| t.set(std::sync::Arc::as_ptr(&flag)));
    TERM_FLAG_KEEP.with(|t| t.set(Some(flag)));
}

/// 本 worker 会话是否已被请求终止（WError 抑制判定）。
pub fn term_tls_flagged() -> bool {
    let p = TERM_FLAG_PTR.with(|t| t.get());
    !p.is_null() && unsafe { (*p).load(std::sync::atomic::Ordering::SeqCst) }
}

/// SAFETY-BOUNDARY（interrupt 回调）：仅读原子旗，无 JSAPI、无 TLS RefCell
/// 借用、无重入（Interrupt.h：回调内禁止再入引擎）。返回 false = 中止脚本。
/// 覆盖：`tests/node/worker.rs` terminate 生命周期黑盒。
unsafe extern "C" fn term_interrupt_cb(_cx: *mut mozjs::jsapi::JSContext) -> bool {
    !term_tls_flagged()
}

/// worker 会话装中断钩子（`init_session` worker 分支，realm 内、首段脚本前）。
pub fn term_install(raw_cx: *mut mozjs::jsapi::JSContext) {
    // SAFETY: JS 线程、会话初始化期；回调仅读原子旗（见 term_interrupt_cb）。
    unsafe {
        mozjs::jsapi::JS_AddInterruptCallback(raw_cx, Some(term_interrupt_cb));
    }
}

/// 登记 worker cx 指针（boot 后纯 Rust 调，无 JSAPI）。
pub fn term_bind_cx(worker_id: u64, raw_cx: *mut mozjs::jsapi::JSContext) {
    if let Ok(m) = term_slots().lock() {
        if let Some(slot) = m.get(&worker_id) {
            slot.cx.store(raw_cx as usize, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

/// 注册终止槽（主线程 `run_worker_thread` 起线程前）。
pub fn term_slot_register(worker_id: u64) -> std::sync::Arc<TerminateSlot> {
    let slot = std::sync::Arc::new(TerminateSlot {
        flag: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        cx: std::sync::atomic::AtomicUsize::new(0),
    });
    if let Ok(mut m) = term_slots().lock() {
        m.insert(worker_id, slot.clone());
    }
    slot
}

/// 主线程请求终止：置共享旗 + 向 worker cx 请求中断（忙循环斩断）。
pub(crate) fn term_request(worker_id: u64) {
    let slot = term_slots().lock().ok().and_then(|m| m.get(&worker_id).cloned());
    if let Some(slot) = slot {
        slot.flag.store(true, std::sync::atomic::Ordering::SeqCst);
        let cx = slot.cx.load(std::sync::atomic::Ordering::SeqCst);
        if cx != 0 {
            // SAFETY: cx 指针由 worker 会话持有且 Runtime 刻意泄漏（§4.8），
            // 指针进程生命期有效；RequestInterruptCallback 线程安全
            //（Interrupt.h 注明 any thread；只置请求旗，不碰 GC/堆）。
            unsafe {
                mozjs::jsapi::JS_RequestInterruptCallback(cx as *mut mozjs::jsapi::JSContext);
            }
        }
    }
}

/// worker 线程 boot 包（spawn 线程 move 进去；`runtime` 经槽 transito）。
pub struct WorkerBoot {
    pub worker_id: u64,
    pub thread_id: u64,
    pub data_json: Option<String>,
    /// env 快照（JSON 对象串；None=继承真 env——主会话/SHARE_ENV，10f）。
    pub env_json: Option<String>,
    /// 构造 `options.name`（worker 线程 `threadName` 导出；10f）。
    pub name: Option<String>,
    /// fork 子进程（IPC 通道在，`process.send` 族不装 UNSUPPORTED 桩；10f）。
    pub is_fork: bool,
    pub main_inbox: tokio::sync::mpsc::UnboundedSender<WorkerEvent>,
    pub rendezvous: std::sync::mpsc::Sender<(
        tokio::sync::mpsc::UnboundedSender<WorkerEvent>,
        u64,
    )>,
}

/// boot 落地（`runtime::init_session` 内调；建 parentPort + 记主端点 + 权限继承）。
/// parentPort 对端地址是主会话的 worker_id（`port_post` 改道 `WMsg`）。
pub fn worker_boot_from_slot(boot: WorkerBoot) {
    let parent = state::port_alloc_cross(boot.worker_id, boot.main_inbox.clone());
    state::worker_boot(boot.thread_id, boot.data_json, boot.env_json, boot.name, boot.is_fork, parent);
    state::with_plain(|p| {
        p.worker_main_inbox = Some(boot.main_inbox);
        p.worker_rendezvous = Some(boot.rendezvous);
    });
}

/// boot 收尾（`init_session` 末调；主会话无操作）：回传 worker 收件箱 +
/// parentPort id（spawn 侧 rendezvous 等齐才返回），再发 `WOnline`。
pub fn worker_booted() {
    if state::worker_is_main() {
        return;
    }
    let (inbox, rendezvous, own_tx, parent) = state::with_plain(|p| {
        (
            p.worker_main_inbox.clone(),
            p.worker_rendezvous.take(),
            p.worker_tx.clone(),
            p.worker_parent_port,
        )
    });
    let (Some(inbox), Some(rendezvous), Some(own_tx), Some(parent)) = (inbox, rendezvous, own_tx, parent) else {
        return;
    };
    // worker_id 反查：parentPort 记录的 peer（alloc 时传入主会话的 worker_id）。
    let wid = state::with_rooted(|s| {
        s.worker_ports.iter().find(|pt| pt.id == parent).map(|pt| pt.peer)
    });
    let Some(wid) = wid else { return };
    let _ = rendezvous.send((own_tx, parent));
    let _ = inbox.send(WorkerEvent::WOnline { worker_id: wid });
}

/// 主→worker 投递寻址（句柄收件箱 + parentPort id）。
pub(crate) fn worker_route(worker_id: u64) -> Option<(tokio::sync::mpsc::UnboundedSender<WorkerEvent>, u64)> {
    let inbox = state::worker_inbox(worker_id)?;
    let parent = state::worker_parent_port_of(worker_id)?;
    Some((inbox, parent))
}

/// 起 worker。`__wjs2_worker_spawn(src, evalFlag, dataJson)` → `"workerId threadId"`。
/// `src` 为文件路径（evalFlag=0）或源码（evalFlag=1）；dataJson 为空即无 workerData。
pub unsafe extern "C" fn worker_spawn(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 port_pair
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: Worker needs a filename, eval flag and data");
        return false;
    }
    let src = value_to_string(&mut cx, frame.arg(0));
    let is_eval = value_to_string(&mut cx, frame.arg(1)) == "1";
    let data_raw = value_to_string(&mut cx, frame.arg(2));
    let data_json = if data_raw.is_empty() { None } else { Some(data_raw) };
    // 第 4 参：options.name（缺位/空串即无名；threadName 链路，10f）。
    let name_raw = if frame.argc() >= 4 {
        value_to_string(&mut cx, frame.arg(3))
    } else {
        String::new()
    };
    let name = if name_raw.is_empty() { None } else { Some(name_raw) };
    // 第 5 参：fork 子进程标记（IPC 面桩豁免，10f）。
    let is_fork = frame.argc() >= 5 && value_to_string(&mut cx, frame.arg(4)) == "1";
    // 第 6 参：env 快照 wire（空串=继承真 env；10f process-env 套件）。
    let env_raw = if frame.argc() >= 6 {
        let v = value_to_string(&mut cx, frame.arg(5));
        if v.is_empty() { None } else { Some(v) }
    } else {
        None
    };
    let main_inbox = match state::with_plain(|p| p.worker_tx.clone()) {
        Some(tx) => tx,
        None => {
            report_error(&mut cx, "OperationError: worker channel is not initialized");
            return false;
        }
    };
    let worker_id = state::with_plain(|p| {
        p.worker_next_id += 1;
        p.worker_next_id
    });
    let thread_id =
        NEXT_THREAD_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let (rendezvous_tx, rendezvous_rx) = std::sync::mpsc::channel();
    let boot = WorkerBoot {
        worker_id,
        thread_id,
        data_json,
        env_json: env_raw,
        name,
        is_fork,
        main_inbox: main_inbox.clone(),
        rendezvous: rendezvous_tx,
    };
    let spec = crate::runtime::WorkerThreadSpec::new(worker_id, src, is_eval, boot);
    crate::runtime::run_worker_thread(spec);
    // 等 worker 会话就绪（收件箱 + parentPort；超时即起线程失败）。
    match rendezvous_rx.recv_timeout(std::time::Duration::from_secs(30)) {
        Ok((inbox_tx, parent_port)) => {
            state::worker_handle_add(state::WorkerHandle {
                worker_id,
                thread_id,
                inbox_tx,
                parent_port,
                counted: true,
                exited: false,
            });
            use mozjs::conversions::ToJSValConvertible as _;
            format!("{worker_id} {thread_id}").to_jsval(&mut cx, frame.rval_mut());
            true
        }
        Err(_) => {
            report_error(&mut cx, "OperationError: worker thread failed to start");
            false
        }
    }
}
