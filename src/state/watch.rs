//! state/watch：fs.watch 驱动/监听/seen 表 + fs 流计数。

use super::*;
use mozjs::jsval::JSVal;


// ── fs.watch 驱动（`notify` 线程 → channel → 事件循环，见 node/fs.rs）────────

/// 分配 watch id（调用方随后建 watcher；失败路径无需配套调用，尚未计数）。
pub fn watch_alloc() -> Option<(
    u64,
    tokio::sync::mpsc::UnboundedSender<crate::builtins::node::fs::WatchEvent>,
)> {
    with_plain(|p| {
        let tx = p.watch_tx.clone()?;
        p.watch_next_id += 1;
        Some((p.watch_next_id, tx))
    })
}

/// 登记驱动 + 监听（persistent 计存活；非 persistent 只收事件不续命）。
pub fn watch_add(
    id: u64,
    driver: notify::RecommendedWatcher,
    listener: JSVal,
    persistent: bool,
) {
    with_rooted(|s| s.watch_listeners.push(WatchCallback { id, listener: Heap::boxed(listener) }));
    with_plain(|p| {
        p.watch_drivers.insert(id, (driver, persistent));
        if persistent {
            p.watch_open += 1;
        }
    });
}

/// 取监听（分发用；保留注册，close 前一直有效）。
pub fn watch_listener(id: u64) -> Option<JSVal> {
    with_rooted(|s| s.watch_listeners.iter().find(|w| w.id == id).map(|w| w.listener.get()))
}

/// ref/unref 切换一路 watch 的续命位（node FSWatcher 句柄语义；幂等，
/// 不存在即 noop；watch_open 计数同步，事件循环退出条件用）。
pub fn watch_set_persistent(id: u64, persistent: bool) {
    with_plain(|p| {
        if let Some((_, flag)) = p.watch_drivers.get_mut(&id) {
            if *flag != persistent {
                *flag = persistent;
                if persistent {
                    p.watch_open += 1;
                } else {
                    p.watch_open = p.watch_open.saturating_sub(1);
                }
            }
        }
    });
}

/// 关闭一路 watch（幂等；残留事件落空）。
pub fn watch_remove(id: u64) {
    with_rooted(|s| {
        s.watch_listeners.retain(|w| w.id != id);
    });
    with_plain(|p| {
        if let Some((_, persistent)) = p.watch_drivers.remove(&id) {
            if persistent {
                p.watch_open = p.watch_open.saturating_sub(1);
            }
        }
        p.watch_seen.remove(&id);
    });
}

/// 存活 watch 数（persistent；事件循环退出条件用）。
pub fn watch_open() -> usize {
    with_plain(|p| p.watch_open)
}

/// fs 流续命（构造时 +1；close/终结时 -1，饱和减；幂等由调用方 __refed 旗保证）。
pub fn fs_stream_ref() {
    with_plain(|p| p.fs_stream_open += 1);
}

/// fs 流摘除（饱和减；事件循环退出条件用）。
pub fn fs_stream_unref() {
    with_plain(|p| p.fs_stream_open = p.fs_stream_open.saturating_sub(1));
}

/// 存活 fs 流数（事件循环退出条件用）。
pub fn fs_stream_open() -> usize {
    with_plain(|p| p.fs_stream_open)
}

/// 标记一路 watch 见过的文件（Create 去重用：见过的再 Create 即重写 artifact）。
pub fn watch_seen_mark(id: u64, file: &str) {
    with_plain(|p| {
        p.watch_seen.entry(id).or_default().insert(file.to_owned());
    });
}

/// 一路 watch 是否见过该文件。
pub fn watch_seen_has(id: u64, file: &str) -> bool {
    with_plain(|p| p.watch_seen.get(&id).is_some_and(|s| s.contains(file)))
}

/// 遗忘一路 watch 的文件（Remove 后重建即新文件）。
pub fn watch_seen_forget(id: u64, file: &str) {
    with_plain(|p| {
        if let Some(s) = p.watch_seen.get_mut(&id) {
            s.remove(file);
        }
    });
}
