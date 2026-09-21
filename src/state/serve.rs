//! state/serve：独立 serve 在飞请求计数 + 响应通道表（plan4）。

use super::*;

// ── 独立 serve（plan4 T1）：在飞请求计数 + 响应通道表 ──
// 配对铁律：Head 分发时 `serve_head`（+1 并落表）；End/Fail 终结时 `serve_take`
// （-1 并摘表）。重复终结（未知 id）静默丢弃，不重复减。

/// 在飞请求数（含常驻服务 +1；优雅停机排空用）。
pub fn serve_open() -> usize {
    with_plain(|p| p.serve_open)
}

/// 常驻服务计数（serve 会话持有：循环不等 idle 退出，只认停机旗）。
pub fn serve_hold_server() {
    with_plain(|p| p.serve_open += 1);
}

/// 常驻释放（会话收尾；配套 `serve_hold_server`）。
pub fn serve_release_server() {
    with_plain(|p| p.serve_open = p.serve_open.saturating_sub(1));
}

/// Head 落账（+1 并存通道；同 id 重复 Head 即覆盖，计数不重复加）。
pub fn serve_head(id: u64, resp: crate::serve_bridge::ServeRespTx) {
    with_plain(|p| {
        if p.serve_resps.insert(id, resp).is_none() {
            p.serve_open += 1;
        }
    });
}

/// 取响应头发送端（投递后 head 通道即消费；表项与计数保留给体）。
pub fn serve_take_head(
    id: u64,
) -> Option<tokio::sync::oneshot::Sender<crate::serve_bridge::ServeRespHead>> {
    with_plain(|p| p.serve_resps.get_mut(&id).and_then(|r| r.head_tx.take()))
}

/// 体发送端克隆（未知 id 即过期响应，调用方静默成功）。
pub fn serve_body_tx(
    id: u64,
) -> Option<tokio::sync::mpsc::UnboundedSender<crate::serve_bridge::ServeBodyMsg>> {
    with_plain(|p| p.serve_resps.get(&id).map(|r| r.body_tx.clone()))
}

/// 取升级决策发送端（投递后通道即消费；表项与计数保留给后续头/体）。
pub fn serve_take_upgrade(
    id: u64,
) -> Option<tokio::sync::oneshot::Sender<crate::serve_bridge::ServeUpgrade>> {
    with_plain(|p| p.serve_resps.get_mut(&id).and_then(|r| r.upgrade_tx.take()))
}

/// 终结摘表（-1 并取走通道；未知 id 回 None，调用方静默丢弃）。
pub fn serve_take(id: u64) -> Option<crate::serve_bridge::ServeRespTx> {
    with_plain(|p| {
        let r = p.serve_resps.remove(&id);
        if r.is_some() {
            p.serve_open = p.serve_open.saturating_sub(1);
        }
        r
    })
}
