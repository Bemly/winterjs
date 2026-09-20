//! state/worker：worker 句柄/目标/线程身份。

use super::*;
use mozjs::jsval::JSVal;


// ── worker 驱动（MessagePort/Worker；channel → 事件循环，同 net 模型）────────

/// 会话线程身份初始化（`init_session` 对每个会话都调：主调 (true, 0)，9f-3 的
/// worker 线程起后改写 (false, id)；PlainState::default 全 false/0 不可直接用）。
pub fn worker_session_init(is_main: bool, thread_id: u64) {
    with_plain(|p| {
        p.worker_is_main = is_main;
        p.worker_thread_id = thread_id;
    });
}

pub fn worker_is_main() -> bool {
    with_plain(|p| p.worker_is_main)
}

pub fn worker_thread_id() -> u64 {
    with_plain(|p| p.worker_thread_id)
}

/// 存活计数（ref'd 端口 + 运行中 worker；事件循环退出条件用）。
pub fn worker_open() -> usize {
    with_plain(|p| p.worker_open)
}

// ── Worker 线程（9f-3；spawn/parentPort/workerData/exit/terminate）─────────

/// worker 线程起后初始化（`runtime::run_worker_thread` 经 boot 槽调；含权限继承）。
pub fn worker_boot(thread_id: u64, data_json: Option<String>, env_json: Option<String>, name: Option<String>, is_fork: bool, parent_port: u64) {
    with_plain(|p| {
        p.worker_is_main = false;
        p.worker_thread_id = thread_id;
        p.worker_data_json = data_json;
        p.worker_env_json = env_json;
        p.worker_name = name;
        p.worker_is_fork = is_fork;
        p.worker_parent_port = Some(parent_port);
        p.worker_terminated = false;
    });
    if let Some(perms) = crate::permissions::cli_snapshot() {
        crate::permissions::install(perms);
    }
}

pub fn worker_data_json() -> Option<String> {
    with_plain(|p| p.worker_data_json.clone())
}

/// worker 构造名（`threadName`；主会话恒 None）。
pub fn worker_name() -> Option<String> {
    with_plain(|p| p.worker_name.clone())
}

/// fork 子进程标记（主会话恒 false）。
pub fn worker_is_fork() -> bool {
    with_plain(|p| p.worker_is_fork)
}

pub fn worker_parent_port() -> Option<u64> {
    with_plain(|p| p.worker_parent_port)
}

/// 终止旗置位（`WTerminate` 分发时调；事件循环检查点见 `pump_once`）。
pub fn worker_terminate_flag() {
    with_plain(|p| p.worker_terminated = true);
}

pub fn worker_terminated() -> bool {
    with_plain(|p| p.worker_terminated)
}

/// 登记 worker 句柄（主会话；运行中计 1 存活，可 unref 摘）。
pub fn worker_handle_add(h: WorkerHandle) {
    with_plain(|p| {
        if h.counted {
            p.worker_open += 1;
        }
        p.worker_handles.insert(h.worker_id, h);
    });
}

/// 取 worker 发件端点（post/terminate 用；已退出即 None）。
pub fn worker_inbox(worker_id: u64) -> Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::node::worker::WorkerEvent>> {
    with_plain(|p| p.worker_handles.get(&worker_id).filter(|h| !h.exited).map(|h| h.inbox_tx.clone()))
}

/// 取 worker 的 parentPort 寻址（主→worker 投递用）。
pub fn worker_parent_port_of(worker_id: u64) -> Option<u64> {
    with_plain(|p| p.worker_handles.get(&worker_id).filter(|h| !h.exited).map(|h| h.parent_port))
}

/// worker 退出结算（计过数即减；标记 exited 防双减；返回是否首次）。
pub fn worker_exited(worker_id: u64) -> bool {
    with_plain(|p| match p.worker_handles.get_mut(&worker_id) {
        Some(h) if !h.exited => {
            h.exited = true;
            if h.counted {
                h.counted = false;
                p.worker_open = p.worker_open.saturating_sub(1);
            }
            true
        }
        _ => false,
    })
}

/// worker 取消息线程 id（`Worker.threadId` 用）。
pub fn worker_tid(worker_id: u64) -> Option<u64> {
    with_plain(|p| p.worker_handles.get(&worker_id).map(|h| h.thread_id))
}

/// worker ref/unref（unref 后不续命主循环）。
pub fn worker_set_ref(worker_id: u64, refed: bool) {
    with_plain(|p| {
        if let Some(h) = p.worker_handles.get_mut(&worker_id) {
            if !h.exited && h.counted != refed {
                h.counted = refed;
                if refed {
                    p.worker_open += 1;
                } else {
                    p.worker_open = p.worker_open.saturating_sub(1);
                }
            }
        }
    });
}

/// 登记 worker JS 目标（`Worker` 构造时 attach）。
pub fn worker_target_add(id: u64, target: JSVal) {
    with_rooted(|s| s.worker_targets.push(WorkerTarget { id, target: Heap::boxed(target) }));
}

pub fn worker_target(id: u64) -> Option<JSVal> {
    with_rooted(|s| s.worker_targets.iter().find(|t| t.id == id).map(|t| t.target.get()))
}

/// 摘除 worker JS 目标（Exit 派发后调；返回首次 true）。
pub fn worker_target_remove(id: u64) -> bool {
    with_rooted(|s| {
        let n0 = s.worker_targets.len();
        s.worker_targets.retain(|t| t.id != id);
        s.worker_targets.len() != n0
    })
}
