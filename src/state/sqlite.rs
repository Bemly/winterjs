//! state/sqlite：bun/node sqlite worker 表。

use super::*;

// ── bun:sqlite worker 表（natives 阻塞往返，见 bun/sqlite.rs）───────────────

/// 登记 worker 并分配 id。
pub fn sqlite_add(worker: crate::builtins::bun::sqlite::SqliteWorker) -> u64 {
    with_plain(|p| {
        p.sqlite_next_id += 1;
        let id = p.sqlite_next_id;
        p.sqlite_workers.insert(id, worker);
        id
    })
}

/// 取 worker 端点（Clone 出用；channel 均可 Clone，不持 TLS 借用做阻塞 IO）。
pub fn sqlite_worker(id: u64) -> Option<crate::builtins::bun::sqlite::SqliteWorker> {
    with_plain(|p| p.sqlite_workers.get(&id).map(|w| crate::builtins::bun::sqlite::SqliteWorker {
        req_tx: w.req_tx.clone(),
        resp_rx: w.resp_rx.clone(),
    }))
}

/// 摘除 worker（close 调用；drop 掉的 req_tx 让线程自退）。
pub fn sqlite_remove(id: u64) {
    with_plain(|p| {
        p.sqlite_workers.remove(&id);
    });
}

/// 会话重置（`init_session` 调用）：上一会话的 worker 端点全数 drop，
/// 线程在 channel 断开后自退（PlainState 跨 run 复用，见本文件头注）。
pub fn sqlite_reset() {
    with_plain(|p| {
        p.sqlite_workers.clear();
        p.nsqlite_workers.clear();
    });
}

// ── node:sqlite worker 表（10d；natives 阻塞往返，见 node/sqlite.rs）───────

/// 登记 worker 并分配 id。
pub fn nsqlite_add(worker: crate::builtins::node::sqlite::NodeSqliteWorker) -> u64 {
    with_plain(|p| {
        p.nsqlite_next_id += 1;
        let id = p.nsqlite_next_id;
        p.nsqlite_workers.insert(id, worker);
        id
    })
}

/// 取 worker 端点（Clone 出用；channel 均可 Clone，不持 TLS 借用做阻塞 IO）。
pub fn nsqlite_worker(id: u64) -> Option<crate::builtins::node::sqlite::NodeSqliteWorker> {
    with_plain(|p| {
        p.nsqlite_workers.get(&id).map(|w| {
            crate::builtins::node::sqlite::NodeSqliteWorker {
                req_tx: w.req_tx.clone(),
                resp_rx: w.resp_rx.clone(),
            }
        })
    })
}

/// 摘除 worker（close 调用；drop 掉的 req_tx 让线程自退）。
pub fn nsqlite_remove(id: u64) {
    with_plain(|p| {
        p.nsqlite_workers.remove(&id);
    });
}
