//! state/child：异步子进程表。

use super::*;
use mozjs::jsval::JSVal;


/// 分配子进程 id（调用方随后 spawn + 登记；失败路径无需配套调用）。
pub fn child_alloc() -> Option<(
    u64,
    tokio::sync::mpsc::UnboundedSender<crate::builtins::node::child::ChildEvent>,
)> {
    with_plain(|p| {
        let tx = p.child_tx.clone()?;
        p.child_next_id += 1;
        Some((p.child_next_id, tx))
    })
}

/// 登记进程本体 + JS 目标（target 为 prelude ChildProcess 对象；pipe 时带 stdin 通道
/// 与期望落定的 pipe 泵数）。
pub fn child_add(
    id: u64,
    child: tokio::process::Child,
    detached: bool,
    target: JSVal,
    stdin_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::node::child::StdinCmd>>,
    pipes_expected: u8,
) {
    with_rooted(|s| s.child_targets.push(ChildTarget { id, target: Heap::boxed(target) }));
    with_plain(|p| {
        p.child_procs.insert(id, ChildEntry { child, detached, stdin_tx, pipes_expected, pipes_done: 0, timed_out: false });
        p.child_open += 1;
    });
}

/// stdin 写/关（pipe 专用；子进程已走即 false，调用方转流错误）。
pub fn child_stdin_send(id: u64, cmd: crate::builtins::node::child::StdinCmd) -> bool {
    with_plain(|p| {
        p.child_procs
            .get(&id)
            .and_then(|e| e.stdin_tx.as_ref())
            .is_some_and(|tx| tx.send(cmd).is_ok())
    })
}

/// pipe 泵落定记数（EOF/错皆记；返回是否全部落定）。泵 task 退出路径必调。
pub fn child_pipe_done(id: u64) -> bool {
    with_plain(|p| {
        let Some(e) = p.child_procs.get_mut(&id) else {
            return true;
        };
        e.pipes_done = e.pipes_done.saturating_add(1);
        e.pipes_done >= e.pipes_expected
    })
}

/// pipe 是否全部落定（等待 task 发 Exited 前查；表项已摘除也算落定）。
pub fn child_pipes_flushed(id: u64) -> bool {
    with_plain(|p| {
        p.child_procs
            .get(&id)
            .is_none_or(|e| e.pipes_done >= e.pipes_expected)
    })
}

/// 取 JS 目标（分发用；保留注册，终态时摘除）。
pub fn child_target(id: u64) -> Option<JSVal> {
    with_rooted(|s| s.child_targets.iter().find(|c| c.id == id).map(|c| c.target.get()))
}

/// 终态记账（target + 本体移除 + 存活减一；kill 后残留事件落空）。
pub fn child_remove(id: u64) {
    with_rooted(|s| {
        s.child_targets.retain(|c| c.id != id);
    });
    with_plain(|p| {
        p.child_procs.remove(&id);
        p.child_open = p.child_open.saturating_sub(1);
    });
}

/// 超时杀作用记号（timeout task 杀前置位；Exited 事件取走）。
pub fn child_mark_timeout(id: u64) {
    with_plain(|p| {
        if let Some(e) = p.child_procs.get_mut(&id) {
            e.timed_out = true;
        }
    });
}

/// 取走超时杀记号（发 Exited 前读，消费即复位语义由摘除路径承担）。
pub fn child_take_timed_out(id: u64) -> bool {
    with_plain(|p| {
        p.child_procs
            .get(&id)
            .is_some_and(|e| e.timed_out)
    })
}

/// 发信号（数字串/`SIGKILL`/`SIGTERM`/`KILL`/`TERM`；detached 走组杀，unix；
/// win 直接杀）。数字串口径：JS 侧经 os.signals 表归一后传 signo（killSignal
/// 可为任意信号，10f）。返回是否作用到存活进程（未知 id/已退出为 false）。
pub fn child_kill(id: u64, sig: &str) -> bool {
    with_plain(|p| {
        let Some(entry) = p.child_procs.get_mut(&id) else {
            return false;
        };
        #[cfg(unix)]
        {
            use nix::sys::signal::{kill, Signal};
            use nix::unistd::Pid;
            let pid = entry.child.id().unwrap_or(0) as i32;
            if pid <= 0 {
                return false;
            }
            let raw = sig.trim();
            let target = if entry.detached { Pid::from_raw(-pid) } else { Pid::from_raw(pid) };
            // 信号 0 为存在性检查（kill 套件：只验活，不发信号；数字 0 无对应
            // Signal 枚举值，try_from 落空即往 SIGKILL 误杀——此处短路）。
            if raw == "0" {
                return kill(target, None).is_ok();
            }
            let signal = if let Ok(n) = raw.parse::<i32>() {
                Signal::try_from(n).unwrap_or(Signal::SIGKILL)
            } else {
                match raw.to_ascii_uppercase().as_str() {
                    "SIGKILL" | "KILL" | "9" => Signal::SIGKILL,
                    "SIGTERM" | "TERM" | "15" => Signal::SIGTERM,
                    _ => Signal::SIGTERM,
                }
            };
            if kill(target, signal).is_ok() {
                return true;
            }
            // 组杀失败回退直杀（如已非组长）。
            if entry.detached && kill(Pid::from_raw(pid), signal).is_ok() {
                return true;
            }
            // 同步杀不动则置异步杀（task 侧收尾；此处报 false 由调用方定）。
            entry.child.start_kill().is_ok()
        }
        #[cfg(not(unix))]
        {
            let _ = sig;
            entry.child.start_kill().is_ok()
        }
    })
}

/// 存活子进程数（事件循环退出条件用）。
pub fn child_open() -> usize {
    with_plain(|p| p.child_open)
}

/// 非阻塞收尸（`try_wait` 到即收，无僵尸；返回原始状态，映射由调用方做）。
pub fn child_try_wait(id: u64) -> Option<std::process::ExitStatus> {
    with_plain(|p| p.child_procs.get_mut(&id)?.child.try_wait().ok()?)
}
