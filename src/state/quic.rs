//! state/quic：QUIC endpoint/会话/流表。

use super::*;
use mozjs::jsval::JSVal;


// ── QUIC 驱动（endpoint/会话；task → channel → 事件循环，同 net 模型）─────

/// QUIC 事件端点（接收端由事件循环持有；无即会话外，不分配）。
pub fn quic_tx_clone() -> Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::node::quic::QuicEvent>> {
    with_plain(|p| p.quic_tx.clone())
}

/// 分配 QUIC id（endpoint/会话共享单调空间）。
pub fn quic_alloc_id() -> u64 {
    with_plain(|p| {
        p.quic_next_id += 1;
        p.quic_next_id
    })
}

/// 登记 endpoint（监听中计 1 存活）。
pub fn quic_ep_insert(id: u64, ep: quinn::Endpoint, accept_task: tokio::task::AbortHandle) {
    with_plain(|p| {
        p.quic_endpoints.insert(id, QuicEndpointEntry { ep, accept_task, closing: false });
        p.quic_open += 1;
    });
}

/// 开始关闭（置 closing + abort accept 环；返回 endpoint 供发 Close 帧）。
/// 重复关即 None（防双发 `EndpointClosed`）。
pub fn quic_ep_begin_close(id: u64) -> Option<quinn::Endpoint> {
    with_plain(|p| match p.quic_endpoints.get_mut(&id) {
        Some(e) if !e.closing => {
            e.closing = true;
            e.accept_task.abort();
            Some(e.ep.clone())
        }
        _ => None,
    })
}

pub fn quic_ep_get(id: u64) -> Option<quinn::Endpoint> {
    with_plain(|p| p.quic_endpoints.get(&id).map(|e| e.ep.clone()))
}

/// 摘除 endpoint（abort accept 任务；计过数即减；返回是否首次）。
pub fn quic_ep_remove(id: u64) -> bool {
    with_plain(|p| match p.quic_endpoints.remove(&id) {
        Some(e) => {
            e.accept_task.abort();
            p.quic_open = p.quic_open.saturating_sub(1);
            true
        }
        None => false,
    })
}

/// 登记会话（存活计 1；conn/cmd 建好后补，见 `quic_sess_set_conn`）。
pub fn quic_sess_insert(id: u64, local: String, remote: String) {
    with_plain(|p| {
        p.quic_sessions.insert(id, QuicSessionEntry { conn: None, driver: None, local, remote, cmd_tx: None, h3_tx: None, client_ep: None });
        p.quic_open += 1;
    });
}

/// 补登记驱动任务句柄（spawn 后；重复登记 abort 旧柄）。
pub fn quic_sess_set_driver(id: u64, driver: tokio::task::AbortHandle) {
    with_plain(|p| {
        if let Some(e) = p.quic_sessions.get_mut(&id) {
            if let Some(old) = e.driver.replace(driver) {
                old.abort();
            }
        }
    });
}

/// 会话寻址（info 用；conn 建好前也能读）。
pub fn quic_sess_addrs(id: u64) -> Option<(String, String)> {
    with_plain(|p| p.quic_sessions.get(&id).map(|e| (e.local.clone(), e.remote.clone())))
}

/// 补登记连接句柄（握手成功后；驱动任务等 `closed()` 用不上它，info/stats/close 用）。
pub fn quic_sess_set_conn(id: u64, conn: quinn::Connection) {
    with_plain(|p| {
        if let Some(e) = p.quic_sessions.get_mut(&id) {
            e.conn = Some(conn);
        }
    });
}

pub fn quic_sess_conn(id: u64) -> Option<quinn::Connection> {
    with_plain(|p| p.quic_sessions.get(&id).and_then(|e| e.conn.clone()))
}

/// 摘除会话（abort 驱动；名下流由分发侧收尾；自有 endpoint 关后释放；
/// 计过数即减；返回是否首次）。
pub fn quic_sess_remove(id: u64) -> bool {
    with_plain(|p| match p.quic_sessions.remove(&id) {
        Some(e) => {
            if let Some(d) = e.driver {
                d.abort();
            }
            if let Some(ep) = e.client_ep {
                ep.close(0u32.into(), b"bye");
            }
            p.quic_open = p.quic_open.saturating_sub(1);
            true
        }
        None => false,
    })
}

/// 寄存发起侧 endpoint（socket 保活到会话收尾；任务结束即 move 进来）。
pub fn quic_sess_set_client_ep(id: u64, ep: quinn::Endpoint) {
    with_plain(|p| {
        if let Some(e) = p.quic_sessions.get_mut(&id) {
            e.client_ep = Some(ep);
        }
    });
}

/// 登记会话命令端点（驱动任务持有接收端；open 流/关会话走此通道）。
pub fn quic_sess_set_cmd(
    id: u64,
    cmd_tx: tokio::sync::mpsc::UnboundedSender<crate::builtins::node::quic::QuicSessCmd>,
) {
    with_plain(|p| {
        if let Some(e) = p.quic_sessions.get_mut(&id) {
            e.cmd_tx = Some(cmd_tx);
        }
    });
}

/// 发会话命令（会话已摘即 false）。
pub fn quic_sess_cmd(id: u64, cmd: crate::builtins::node::quic::QuicSessCmd) -> bool {
    with_plain(|p| {
        p.quic_sessions.get(&id).and_then(|e| e.cmd_tx.clone()).is_some_and(|tx| tx.send(cmd).is_ok())
    })
}

/// 登记 H3 分支命令端点（9i-9；h3 驱动/服务任务持有接收端）。
pub fn quic_sess_set_h3_cmd(
    id: u64,
    tx: tokio::sync::mpsc::UnboundedSender<crate::builtins::node::quic::QuicH3Cmd>,
) {
    with_plain(|p| {
        if let Some(e) = p.quic_sessions.get_mut(&id) {
            e.h3_tx = Some(tx);
        }
    });
}

/// 取 H3 命令端点（会话已摘/非 H3 即 None）。
pub fn quic_sess_h3_cmd(id: u64) -> Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::node::quic::QuicH3Cmd>> {
    with_plain(|p| p.quic_sessions.get(&id).and_then(|e| e.h3_tx.clone()))
}

/// 登记 endpoint JS 目标（`QuicEndpoint` 构造时 attach）。
pub fn quic_ep_target_add(id: u64, target: JSVal) {
    with_rooted(|s| s.quic_ep_targets.push(QuicTarget { id, target: Heap::boxed(target) }));
}

pub fn quic_ep_target(id: u64) -> Option<JSVal> {
    with_rooted(|s| s.quic_ep_targets.iter().find(|t| t.id == id).map(|t| t.target.get()))
}

/// 摘除 endpoint JS 目标（Close 派发后调；返回首次 true）。
pub fn quic_ep_target_remove(id: u64) -> bool {
    with_rooted(|s| {
        let n0 = s.quic_ep_targets.len();
        s.quic_ep_targets.retain(|t| t.id != id);
        s.quic_ep_targets.len() != n0
    })
}

/// 登记会话 JS 目标（`QuicSession` 构造时 attach）。
pub fn quic_sess_target_add(id: u64, target: JSVal) {
    with_rooted(|s| s.quic_sess_targets.push(QuicTarget { id, target: Heap::boxed(target) }));
}

pub fn quic_sess_target(id: u64) -> Option<JSVal> {
    with_rooted(|s| s.quic_sess_targets.iter().find(|t| t.id == id).map(|t| t.target.get()))
}

/// 摘除会话 JS 目标（Close 派发后调；返回首次 true）。
pub fn quic_sess_target_remove(id: u64) -> bool {
    with_rooted(|s| {
        let n0 = s.quic_sess_targets.len();
        s.quic_sess_targets.retain(|t| t.id != id);
        s.quic_sess_targets.len() != n0
    })
}

/// 存活数（监听中 endpoint + 存活会话；事件循环退出条件用）。
pub fn quic_open() -> usize {
    with_plain(|p| p.quic_open)
}

// ── QUIC 流（9g-2；半端任务各持一端，任一半终结即整流收尾）────────────────

/// 登记流（半端任务句柄随后补；`done` 防双重收尾）。
pub fn quic_stream_insert(id: u64, sess: u64, _dir: QuicStreamDir) {
    with_plain(|p| {
        p.quic_streams.insert(
            id,
            QuicStreamEntry {
                sess,
                quic_id: None,
                write_tx: None,
                read_tx: None,
                write_task: None,
                read_task: None,
                done: false,
            },
        );
    });
}

/// 补登记 quic 流 id（`StreamOpened/Accepted` 后）。
pub fn quic_stream_set_qid(id: u64, qid: u64) {
    with_plain(|p| {
        if let Some(e) = p.quic_streams.get_mut(&id) {
            e.quic_id = Some(qid);
        }
    });
}

/// 补登记半端（写端/读端任务各调一次）。
pub fn quic_stream_set_ends(
    id: u64,
    write_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::node::quic::QuicStreamCmd>>,
    write_task: Option<tokio::task::AbortHandle>,
    read_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::builtins::node::quic::QuicStreamCmd>>,
    read_task: Option<tokio::task::AbortHandle>,
) {
    with_plain(|p| {
        if let Some(e) = p.quic_streams.get_mut(&id) {
            if write_tx.is_some() {
                e.write_tx = write_tx;
                e.write_task = write_task;
            }
            if read_tx.is_some() {
                e.read_tx = read_tx;
                e.read_task = read_task;
            }
        }
    });
}

/// 发流写命令（写端已摘即 false）。
pub fn quic_stream_write_cmd(id: u64, cmd: crate::builtins::node::quic::QuicStreamCmd) -> bool {
    with_plain(|p| {
        p.quic_streams.get(&id).and_then(|e| e.write_tx.clone()).is_some_and(|tx| tx.send(cmd).is_ok())
    })
}

/// 发流读命令（目前仅 `Stop`；读端已摘即 false）。
pub fn quic_stream_read_cmd(id: u64, cmd: crate::builtins::node::quic::QuicStreamCmd) -> bool {
    with_plain(|p| {
        p.quic_streams.get(&id).and_then(|e| e.read_tx.clone()).is_some_and(|tx| tx.send(cmd).is_ok())
    })
}

/// 流收尾（abort 半端任务；`done` 置位防双收；返回是否首次）。
pub fn quic_stream_finish(id: u64) -> bool {
    with_plain(|p| match p.quic_streams.get_mut(&id) {
        Some(e) if !e.done => {
            e.done = true;
            if let Some(t) = e.write_task.take() {
                t.abort();
            }
            if let Some(t) = e.read_task.take() {
                t.abort();
            }
            e.write_tx = None;
            e.read_tx = None;
            true
        }
        _ => false,
    })
}

/// 摘除流记录（收尾后调；会话摘除时顺带清其流——任务已 abort，无泄漏）。
pub fn quic_stream_remove(id: u64) {
    with_plain(|p| {
        p.quic_streams.remove(&id);
    });
}

/// 会话名下全流 id（会话收尾时逐个 `quic_stream_finish` 用）。
pub fn quic_session_streams(sess: u64) -> Vec<u64> {
    with_plain(|p| p.quic_streams.iter().filter(|(_, e)| e.sess == sess).map(|(id, _)| *id).collect())
}

/// 登记流 JS 目标（`QuicStream` 构造时 attach）。
pub fn quic_stream_target_add(id: u64, target: JSVal) {
    with_rooted(|s| s.quic_stream_targets.push(QuicTarget { id, target: Heap::boxed(target) }));
}

pub fn quic_stream_target(id: u64) -> Option<JSVal> {
    with_rooted(|s| s.quic_stream_targets.iter().find(|t| t.id == id).map(|t| t.target.get()))
}

/// 流所属会话的 JS 目标（对端开流事件挂到会话下用）。
pub fn quic_sess_target_by_stream(stream: u64) -> Option<JSVal> {
    let sess = with_plain(|p| p.quic_streams.get(&stream).map(|e| e.sess))?;
    quic_sess_target(sess)
}

/// 摘除流 JS 目标（Close 派发后调；返回首次 true）。
pub fn quic_stream_target_remove(id: u64) -> bool {
    with_rooted(|s| {
        let n0 = s.quic_stream_targets.len();
        s.quic_stream_targets.retain(|t| t.id != id);
        s.quic_stream_targets.len() != n0
    })
}
