//! state/ws：WebSocket 驱动端点/发送端表/存活计数。

use super::*;

/// 分配 WebSocket id（失败路径无需配套调用，尚未计数）。
pub fn ws_alloc() -> Option<(u64, tokio::sync::mpsc::UnboundedSender<crate::builtins::ws::WsEvent>)> {
    with_plain(|p| {
        let tx = p.ws_tx.clone()?;
        p.ws_next_id += 1;
        let id = p.ws_next_id;
        p.ws_open += 1;
        Some((id, tx))
    })
}

/// 登记发送端。
pub fn ws_add_sink(id: u64, tx: tokio::sync::mpsc::UnboundedSender<crate::builtins::ws::WsOut>) {
    with_plain(|p| {
        p.ws_sinks.insert(id, tx);
    });
}

/// 发出消息（未知 id 返回 false）；close 同通道（未知 id 静默成功，幂等）。
pub fn ws_send(id: u64, msg: crate::builtins::ws::WsOut) -> bool {
    with_plain(|p| p.ws_sinks.get(&id).map(|tx| tx.send(msg).is_ok()).unwrap_or(false))
}

pub fn ws_close(id: u64, code: u16, reason: String) {
    with_plain(|p| {
        if let Some(tx) = p.ws_sinks.get(&id) {
            let _ = tx.send(crate::builtins::ws::WsOut::Close { code, reason });
        }
    });
}

/// 清理发送端 + target + 存活计数（close/error 结算时调用）。
pub fn ws_remove(id: u64) {
    with_plain(|p| {
        p.ws_sinks.remove(&id);
        p.ws_open = p.ws_open.saturating_sub(1);
    });
}

/// 存活 WebSocket 数（事件循环退出条件用）。
pub fn ws_open() -> usize {
    with_plain(|p| p.ws_open)
}
