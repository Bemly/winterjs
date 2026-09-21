//! serve WS 面（T4 升级接管；与核心经 pump_req_body/serve_bridge 协作）。
//!
//! 本面全路径引用（axum/tungstenite 系），仅从父模块取 pump_req_body。

use super::pump_req_body;

/// WS upgrade 判定 + 校验（T4）：仅 H1 + `Upgrade: websocket` 为尝试；
/// 尝试而非法（非 GET/缺 key/错版本/Connection 无 upgrade）即 400 文案。
/// 返回 `Ok(Some(key))` = 合法升级；`Ok(None)` = 普通请求；`Err` = 非法升级。
fn ws_attempt(
    req: &axum::http::Request<axum::body::Body>,
) -> Result<Option<String>, &'static str> {
    if req.version() != axum::http::Version::HTTP_11 {
        return Ok(None);
    }
    let is_ws = req
        .headers()
        .get(axum::http::header::UPGRADE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim().eq_ignore_ascii_case("websocket")));
    if !is_ws {
        return Ok(None);
    }
    if req.method() != axum::http::Method::GET {
        return Err("WebSocket upgrade needs GET");
    }
    let conn_upgrade = req
        .headers()
        .get(axum::http::header::CONNECTION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim().eq_ignore_ascii_case("upgrade")));
    if !conn_upgrade {
        return Err("WebSocket upgrade needs Connection: Upgrade");
    }
    let Some(key) = req
        .headers()
        .get(axum::http::header::SEC_WEBSOCKET_KEY)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
    else {
        return Err("WebSocket upgrade needs Sec-WebSocket-Key");
    };
    let version_ok = req
        .headers()
        .get(axum::http::header::SEC_WEBSOCKET_VERSION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim() == "13");
    if !version_ok {
        return Err("WebSocket upgrade needs Sec-WebSocket-Version: 13");
    }
    Ok(Some(key))
}

/// `Sec-WebSocket-Accept`（RFC 6455 §1.3：base64(sha1(key + GUID))）。纯函数，单测覆盖。
pub fn ws_accept_key(key: &str) -> String {
    use base64::Engine as _;
    use sha1::Digest as _;
    let mut h = sha1::Sha1::new();
    h.update(key.as_bytes());
    h.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    base64::engine::general_purpose::STANDARD.encode(h.finalize())
}

/// 服务端 WS 会话（`tokio-tungstenite` 直通；读写映射与 ws.rs `run_socket` 同构）。
/// Opened 先行（JS onopen）；终结经 dispatch 清表计数；握手失败走 Failed。
async fn run_server_socket(
    ws_id: u64,
    ev_tx: tokio::sync::mpsc::UnboundedSender<crate::builtins::ws::WsEvent>,
    mut out_rx: tokio::sync::mpsc::UnboundedReceiver<crate::builtins::ws::WsOut>,
    on_up: hyper::upgrade::OnUpgrade,
) {
    use crate::builtins::ws::{WsEvent, WsKind, WsOut};
    use futures::{SinkExt as _, StreamExt as _};
    let up = match on_up.await {
        Ok(u) => u,
        Err(e) => {
            let _ = ev_tx.send(WsEvent { id: ws_id, kind: WsKind::Failed(format!("websocket error: {e}")) });
            return;
        }
    };
    // 注意：hyper 已做完 HTTP 握手，此处必须 `from_raw_socket` 直接接管；
    // `accept_async` 会重读一次握手（等一个永不到的 HTTP 请求）而永挂。
    // `from_raw_socket` 为 infallible（async 仅为构造，无握手失败面）。
    let stream = tokio_tungstenite::WebSocketStream::from_raw_socket(
        hyper_util::rt::TokioIo::new(up),
        tokio_tungstenite::tungstenite::protocol::Role::Server,
        None,
    )
    .await;
    let _ = ev_tx.send(WsEvent { id: ws_id, kind: WsKind::Opened { protocol: String::new() } });
    tracing::debug!(target: "winterjs::serve", ws_id, "WS opened");
    let (mut sink, mut stream) = stream.split();
    loop {
        tokio::select! {
            msg = stream.next() => {
                match msg {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Text(t))) => {
                        tracing::trace!(target: "winterjs::serve", ws_id, bytes = t.len(), "WS text in");
                        let _ = ev_tx.send(WsEvent { id: ws_id, kind: WsKind::Text(t.to_string()) });
                    }
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(b))) => {
                        tracing::trace!(target: "winterjs::serve", ws_id, bytes = b.len(), "WS bin in");
                        let _ = ev_tx.send(WsEvent { id: ws_id, kind: WsKind::Bin(b.to_vec()) });
                    }
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Close(frame))) => {
                        let (code, reason) = frame
                            .as_ref()
                            .map(|f| (f.code.into(), f.reason.to_string()))
                            .unwrap_or((1005, String::new()));
                        // tungstenite 已在内部排队回 Close（自动应答），此处 flush 推出再结算；
                        // 手动再发会被状态机拒绝（ClosedByPeer）；见 tungstenite protocol/mod.rs。
                        // 排空策略与 tests/ws.rs stub 同族（回帧再收尾，避免 RST 竞态）。
                        let flushed = sink.flush().await.is_ok();
                        tracing::debug!(target: "winterjs::serve", ws_id, code, flushed, "WS peer close");
                        let _ = ev_tx.send(WsEvent { id: ws_id, kind: WsKind::Closed { code, reason, clean: true } });
                        break;
                    }
                    Some(Ok(_)) => {} // Ping/Pong/Frame：tungstenite 自动回 pong
                    Some(Err(e)) => {
                        let _ = ev_tx.send(WsEvent { id: ws_id, kind: WsKind::Closed { code: 1006, reason: e.to_string(), clean: false } });
                        break;
                    }
                    None => {
                        let _ = ev_tx.send(WsEvent { id: ws_id, kind: WsKind::Closed { code: 1006, reason: String::new(), clean: false } });
                        break;
                    }
                }
            }
            out = out_rx.recv() => {
                match out {
                    Some(WsOut::Text(t)) => {
                        if sink.send(tokio_tungstenite::tungstenite::Message::Text(t.into())).await.is_err() {
                            let _ = ev_tx.send(WsEvent { id: ws_id, kind: WsKind::Closed { code: 1006, reason: String::new(), clean: false } });
                            break;
                        }
                    }
                    Some(WsOut::Bin(b)) => {
                        if sink.send(tokio_tungstenite::tungstenite::Message::Binary(b.into())).await.is_err() {
                            let _ = ev_tx.send(WsEvent { id: ws_id, kind: WsKind::Closed { code: 1006, reason: String::new(), clean: false } });
                            break;
                        }
                    }
                    Some(WsOut::Close { code, reason }) => {
                        let frame = (code != 1005).then(|| tokio_tungstenite::tungstenite::protocol::frame::CloseFrame {
                            code: code.into(),
                            reason: reason.clone().into(),
                        });
                        let _ = sink.send(tokio_tungstenite::tungstenite::Message::Close(frame)).await;
                        // 等对端回 close，上限 5s（对端已关则直接结算）
                        match tokio::time::timeout(std::time::Duration::from_secs(5), stream.next()).await {
                            Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Close(frame)))) => {
                                let (code, reason) = frame
                                    .map(|f| (f.code.into(), f.reason.to_string()))
                                    .unwrap_or((code, reason));
                                let _ = ev_tx.send(WsEvent { id: ws_id, kind: WsKind::Closed { code, reason, clean: true } });
                            }
                            _ => {
                                let _ = ev_tx.send(WsEvent { id: ws_id, kind: WsKind::Closed { code, reason, clean: false } });
                            }
                        }
                        break;
                    }
                    None => break,
                }
            }
        }
    }
}

/// WS 拦截中间件（最内层，Router 之前）：升级尝试优先于静态/handler（T4 路由序）；
/// 非法即 400；合法交 JS 决策（101 接管 / Decline 走普通管线）。
/// Offer 只承载空体（非 GET 已 400）：Decline 重建空体重入管线，体泵照常先行。
pub(crate) async fn ws_upgrade_intercept(
    axum::extract::State(scheme): axum::extract::State<&'static str>,
    mut req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response<axum::body::Body> {
    fn bad(msg: &str) -> axum::response::Response<axum::body::Body> {
        axum::response::Response::builder()
            .status(axum::http::StatusCode::BAD_REQUEST)
            .body(axum::body::Body::from(msg.to_owned()))
            .unwrap_or_else(|_| {
                axum::response::Response::builder()
                    .status(axum::http::StatusCode::BAD_REQUEST)
                    .body(axum::body::Body::empty())
                    .expect("static 400 builds")
            })
    }
    fn down() -> axum::response::Response<axum::body::Body> {
        axum::response::Response::builder()
            .status(axum::http::StatusCode::SERVICE_UNAVAILABLE)
            .body(axum::body::Body::empty())
            .expect("static 503 builds")
    }
    let key = match ws_attempt(&req) {
        Ok(None) => return next.run(req).await,
        Ok(Some(k)) => k,
        Err(msg) => return bad(msg),
    };
    // 先占 upgrade future（101 后用），再拆头泵体交 JS 决策。
    let on_up = hyper::upgrade::on(&mut req);
    let (parts, body) = req.into_parts();
    let host = parts
        .headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .or_else(|| parts.uri.host().map(str::to_owned))
        .unwrap_or_else(|| "localhost".to_owned());
    let target = parts.uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    let url = format!("{scheme}://{host}{target}");
    let headers: Vec<(String, String)> = parts
        .headers
        .iter()
        .map(|(k, v)| (k.as_str().to_owned(), v.to_str().unwrap_or("").to_owned()))
        .collect();
    let id = crate::serve_bridge::next_serve_id();
    let Some(tx) = crate::serve_bridge::serve_tx_global() else {
        return down();
    };
    let (dec_tx, dec_rx) = tokio::sync::oneshot::channel();
    // 决策期无头/体泵（Decline 走普通管线重建）；体泵照常先行供 offer 期读体。
    let (body_tx, _body_rx) = tokio::sync::mpsc::unbounded_channel();
    let resp = crate::serve_bridge::ServeRespTx { head_tx: None, body_tx, upgrade_tx: Some(dec_tx) };
    if tx
        .send(crate::serve_bridge::ServeEvent::Head {
            head: crate::serve_bridge::ServeReqHead {
                id,
                method: parts.method.to_string(),
                url,
                headers,
                upgrade: true,
            },
            resp,
        })
        .is_err()
    {
        return down();
    }
    pump_req_body(tx, id, body);
    // 决策 30s 未到即 Decline（handler 挂起不连累升级空转）。
    let decision = tokio::time::timeout(std::time::Duration::from_secs(30), dec_rx).await;
    let crate::serve_bridge::ServeUpgrade::Accept { ws_id, ev_tx, out_rx } = (match decision {
        Ok(Ok(d)) => d,
        _ => crate::serve_bridge::ServeUpgrade::Decline,
    }) else {
        // Decline/超时/会话走：重建空体重入普通管线（静态 → handler）。
        let req = axum::http::Request::from_parts(parts, axum::body::Body::empty());
        return next.run(req).await;
    };
    let accept = ws_accept_key(&key);
    tokio::spawn(run_server_socket(ws_id, ev_tx, out_rx, on_up));
    axum::response::Response::builder()
        .status(axum::http::StatusCode::SWITCHING_PROTOCOLS)
        .header(axum::http::header::UPGRADE, "websocket")
        .header(axum::http::header::CONNECTION, "Upgrade")
        .header(axum::http::header::SEC_WEBSOCKET_ACCEPT, accept)
        .body(axum::body::Body::empty())
        .unwrap_or_else(|_| down())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_accept_key_rfc_vector() {
        // RFC 6455 §1.3 标准向量：key `dGhlIHNhbXBsZSBub25jZQ==` → `s3pPLMBiTxaQ9kYGzzhZRbK+xOo=`。
        assert_eq!(
            ws_accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }
}
