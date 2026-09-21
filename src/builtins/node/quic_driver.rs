//! quic 会话驱动域（H3 命令/头 helper、会话收尾、3 路 spawn 驱动；对齐 quic.rs；纯搬移）。

use crate::state;
use super::quic_tls::close_info;
use base64::Engine as _;
use super::quic::QuicEvent;

// ── 流与数据报（9g-2）────────────────────────────────────────────────────

// ── 流与数据报（9g-2）────────────────────────────────────────────────────

/// 会话命令（驱动任务持有接收端；open 流走此通道，写/读另有流级通道）。
#[derive(Debug)]
pub enum QuicSessCmd {
    OpenBidi { stream: u64 },
    OpenUni { stream: u64 },
}

/// 流级命令（写端任务收 `Write/Finish/Reset`，读端任务收 `Stop`）。
#[derive(Debug)]
pub enum QuicStreamCmd {
    Write(Vec<u8>),
    Finish,
    Reset(u64),
    Stop(u64),
}

/// H3 分支命令（9i-9；服务端驱动收 Respond，客户端服务任务收 Request）。
#[derive(Debug)]
pub enum QuicH3Cmd {
    Respond { stream: u64, status: u16, headers: Vec<(String, String)>, body: Vec<u8> },
    Request { stream: u64, method: String, path: String, headers: Vec<(String, String)>, body: Vec<u8> },
}

/// H3 请求/响应头（serde_json Map → 有序对；同名以 `, ` 连接，Node http 同款）。
pub(crate) fn h3_headers_json(value: serde_json::Value) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    if let serde_json::Value::Object(map) = value {
        for (k, v) in map {
            let vs = match v {
                serde_json::Value::String(sv) => sv,
                serde_json::Value::Array(items) => items
                    .iter()
                    .map(|x| x.as_str().unwrap_or_default().to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                other => other.to_string(),
            };
            out.push((k, vs));
        }
    }
    out
}

/// 会话收尾单出口（§4.52：先收名下流任务/表项，再发 `SessionClose`；done 旗防双发）。
pub(crate) fn sess_finish(
    inbox: &tokio::sync::mpsc::UnboundedSender<QuicEvent>,
    sess: u64,
    code: i64,
    reason: String,
    done: &mut bool,
) {
    if !*done {
        *done = true;
        for sid in state::quic_session_streams(sess) {
            state::quic_stream_finish(sid);
        }
        let _ = inbox.send(QuicEvent::SessionClose { id: sess, code, reason });
    }
}

/// H3 响应头 JSON 值（`http::HeaderMap` → Map；多值 `, ` 连接）。
fn h3_headers_value(headers: &http::HeaderMap) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (k, v) in headers.iter() {
        let key = k.as_str().to_string();
        let val = String::from_utf8_lossy(v.as_bytes()).into_owned();
        match map.get_mut(&key) {
            Some(serde_json::Value::String(prev)) => {
                let joined = format!("{prev}, {val}");
                map.insert(key, serde_json::Value::String(joined));
            }
            _ => {
                map.insert(key, serde_json::Value::String(val));
            }
        }
    }
    serde_json::Value::Object(map)
}

/// 会话驱动任务：命令 + 双向/单向 accept + 数据报接收 + `closed()` 守望。
/// 任一终结条件先到即发 `SessionClose`（`done` 旗防双发）后退出。
pub fn spawn_driver(
    sess: u64,
    conn: quinn::Connection,
    inbox: tokio::sync::mpsc::UnboundedSender<QuicEvent>,
) -> tokio::task::AbortHandle {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<QuicSessCmd>();
    state::quic_sess_set_cmd(sess, cmd_tx);
    tokio::spawn(async move {
        let mut done = false;
        // 收尾：先收半端任务（abort 后无噪声 Error），再发 `SessionClose`。
        // 否则 teardown 引发的读写失败会误报成流错误（无监听即 fatal）。
        let finish = |inbox: &tokio::sync::mpsc::UnboundedSender<QuicEvent>,
                      sess: u64,
                      code: i64,
                      reason: String,
                      done: &mut bool| {
            if !*done {
                *done = true;
                for sid in state::quic_session_streams(sess) {
                    state::quic_stream_finish(sid);
                }
                let _ = inbox.send(QuicEvent::SessionClose { id: sess, code, reason });
            }
        };
        loop {
            tokio::select! {
                cmd = cmd_rx.recv() => {
                    match cmd {
                        None => break, // 会话已摘（发送端全 drop），退出
                        Some(QuicSessCmd::OpenBidi { stream }) => {
                            let inbox = inbox.clone();
                            let conn = conn.clone();
                            tokio::spawn(async move {
                                open_halves(sess, stream, state::QuicStreamDir::Bidi, inbox, conn, true).await;
                            });
                        }
                        Some(QuicSessCmd::OpenUni { stream }) => {
                            let inbox = inbox.clone();
                            let conn = conn.clone();
                            tokio::spawn(async move {
                                open_halves(sess, stream, state::QuicStreamDir::Send, inbox, conn, false).await;
                            });
                        }
                    }
                }
                acc = conn.accept_bi(), if !done => {
                    match acc {
                        Ok((send, recv)) => {
                            peer_halves(sess, state::QuicStreamDir::Bidi, inbox.clone(), Some(send), Some(recv)).await;
                        }
                        Err(_) => {
                            let (code, reason) = close_info(conn.closed().await);
                            finish(&inbox, sess, code, reason, &mut done);
                            break;
                        }
                    }
                }
                acc = conn.accept_uni(), if !done => {
                    match acc {
                        Ok(recv) => {
                            peer_halves(sess, state::QuicStreamDir::Recv, inbox.clone(), None, Some(recv)).await;
                        }
                        Err(_) => {
                            let (code, reason) = close_info(conn.closed().await);
                            finish(&inbox, sess, code, reason, &mut done);
                            break;
                        }
                    }
                }
                dg = conn.read_datagram(), if !done => {
                    match dg {
                        Ok(bytes) => {
                            let _ = inbox.send(QuicEvent::Datagram {
                                id: sess,
                                b64: base64::engine::general_purpose::STANDARD.encode(&bytes),
                            });
                        }
                        Err(_) => {
                            let (code, reason) = close_info(conn.closed().await);
                            finish(&inbox, sess, code, reason, &mut done);
                            break;
                        }
                    }
                }
                err = conn.closed() => {
                    let (code, reason) = close_info(err);
                    finish(&inbox, sess, code, reason, &mut done);
                    break;
                }
            }
        }
    })
    .abort_handle()
}

/// 本地发起开流：`open_bi/open_uni` → 登记半端 → `StreamOpened`（失败即 `StreamError`）。
async fn open_halves(
    sess: u64,
    stream: u64,
    dir: state::QuicStreamDir,
    inbox: tokio::sync::mpsc::UnboundedSender<QuicEvent>,
    conn: quinn::Connection,
    bidi: bool,
) {
    if bidi {
        match conn.open_bi().await {
            Ok((send, recv)) => {
                let qid = send.id().index();
                state::quic_stream_set_qid(stream, qid);
                spawn_halves(sess, stream, dir, inbox.clone(), Some(send), Some(recv)).await;
                let _ = inbox.send(QuicEvent::StreamOpened { id: stream, qid });
            }
            Err(e) => {
                let _ = inbox.send(QuicEvent::StreamError { id: stream, message: format!("ERR_QUIC_STREAM: open failed ({e})") });
                let _ = inbox.send(QuicEvent::StreamClosed { id: stream, code: -1 });
            }
        }
    } else {
        match conn.open_uni().await {
            Ok(send) => {
                let qid = send.id().index();
                state::quic_stream_set_qid(stream, qid);
                spawn_halves(sess, stream, dir, inbox.clone(), Some(send), None).await;
                let _ = inbox.send(QuicEvent::StreamOpened { id: stream, qid });
            }
            Err(e) => {
                let _ = inbox.send(QuicEvent::StreamError { id: stream, message: format!("ERR_QUIC_STREAM: open failed ({e})") });
                let _ = inbox.send(QuicEvent::StreamClosed { id: stream, code: -1 });
            }
        }
    }
}

/// 对端发起流：直接登记半端 → `StreamAccepted`（读/写任务即起）。
async fn peer_halves(
    sess: u64,
    dir: state::QuicStreamDir,
    inbox: tokio::sync::mpsc::UnboundedSender<QuicEvent>,
    send: Option<quinn::SendStream>,
    recv: Option<quinn::RecvStream>,
) {
    let qid = send.as_ref().map(|s| s.id().index()).or_else(|| recv.as_ref().map(|r| r.id().index())).unwrap_or(0);
    let stream = state::quic_alloc_id();
    state::quic_stream_insert(stream, sess, dir);
    state::quic_stream_set_qid(stream, qid);
    spawn_halves(sess, stream, dir, inbox.clone(), send, recv).await;
    let dir_s = match dir {
        state::QuicStreamDir::Bidi => "bidi",
        state::QuicStreamDir::Send => "send",
        state::QuicStreamDir::Recv => "receive",
    };
    let _ = inbox.send(QuicEvent::StreamAccepted { id: stream, qid, dir: dir_s.into() });
}

/// 起读写半端任务（有半端才起；任一半终结即整流 `StreamClosed`，单出口）。
async fn spawn_halves(
    sess: u64,
    stream: u64,
    _dir: state::QuicStreamDir,
    inbox: tokio::sync::mpsc::UnboundedSender<QuicEvent>,
    send: Option<quinn::SendStream>,
    recv: Option<quinn::RecvStream>,
) {
    let _ = sess;
    if let Some(mut send) = send {
        let (wtx, mut wrx) = tokio::sync::mpsc::unbounded_channel::<QuicStreamCmd>();
        let inbox = inbox.clone();
        let wtask = tokio::spawn(async move {
            loop {
                match wrx.recv().await {
                    None => break, // 流已收尾
                    Some(QuicStreamCmd::Write(bytes)) => {
                        if let Err(e) = send.write_all(&bytes).await {
                            let _ = inbox.send(QuicEvent::StreamError {
                                id: stream,
                                message: format!("ERR_QUIC_STREAM: write failed ({e})"),
                            });
                            let _ = inbox.send(QuicEvent::StreamClosed { id: stream, code: -1 });
                            break;
                        }
                    }
                    Some(QuicStreamCmd::Finish) => {
                        let _ = send.finish();
                        let _ = inbox.send(QuicEvent::StreamWriteDone { id: stream });
                        break;
                    }
                    Some(QuicStreamCmd::Reset(code)) => {
                        if let Ok(v) = quinn::VarInt::from_u64(code) {
                            let _ = send.reset(v);
                        }
                        let _ = inbox.send(QuicEvent::StreamClosed { id: stream, code: code as i64 });
                        break;
                    }
                    Some(QuicStreamCmd::Stop(_)) => {} // 读端命令，写端忽略
                }
            }
        });
        state::quic_stream_set_ends(stream, Some(wtx), Some(wtask.abort_handle()), None, None);
    }
    if let Some(mut recv) = recv {
        let (rtx, mut rrx) = tokio::sync::mpsc::unbounded_channel::<QuicStreamCmd>();
        let inbox = inbox.clone();
        let rtask = tokio::spawn(async move {
            loop {
                tokio::select! {
                    cmd = rrx.recv() => {
                        match cmd {
                            Some(QuicStreamCmd::Stop(code)) => {
                                if let Ok(v) = quinn::VarInt::from_u64(code) {
                                    let _ = recv.stop(v);
                                }
                            }
                            _ => break, // 写端命令/通道关闭即退
                        }
                    }
                    chunk = recv.read_chunk(65536, true) => {
                        match chunk {
                            Ok(Some(c)) => {
                                let _ = inbox.send(QuicEvent::StreamData {
                                    id: stream,
                                    b64: base64::engine::general_purpose::STANDARD.encode(&c.bytes),
                                });
                            }
                            Ok(None) => {
                                let _ = inbox.send(QuicEvent::StreamEnd { id: stream });
                                break;
                            }
                            Err(quinn::ReadError::Reset(code)) => {
                                let _ = inbox.send(QuicEvent::StreamClosed { id: stream, code: code.into_inner() as i64 });
                                break;
                            }
                            Err(e) => {
                                let _ = inbox.send(QuicEvent::StreamError {
                                    id: stream,
                                    message: format!("ERR_QUIC_STREAM: read failed ({e})"),
                                });
                                let _ = inbox.send(QuicEvent::StreamClosed { id: stream, code: -1 });
                                break;
                            }
                        }
                    }
                }
            }
        });
        state::quic_stream_set_ends(stream, None, None, Some(rtx), Some(rtask.abort_handle()));
    }
}

// ── 9i-9 H3 分支（headers 面；`quinn` 特性组内 h3/h3-quinn）──────────────
// 本仓自定面（真机 node:quic 模块不存在，26.8.2 实测）：ALPN 含 "h3" 的会话走
// H3 驱动——服务端 accept 循环发 `request` 事件（respond 单发）；客户端
// `request()` 经 SendRequest 串行发请求、`response` 事件回结果。H3 会话无
// 裸流/datagram 事件（h3 独占连接，记档）。

/// 服务端 H3 驱动：accept 循环 → `H3Request`；Respond 命令回响应；
/// `closed()`/accept 结束即收尾（先收名下流，§4.52 顺序）。
pub fn spawn_h3_server(
    sess: u64,
    conn: quinn::Connection,
    inbox: tokio::sync::mpsc::UnboundedSender<QuicEvent>,
) -> tokio::task::AbortHandle {
    let (h3tx, mut h3rx) = tokio::sync::mpsc::unbounded_channel::<QuicH3Cmd>();
    state::quic_sess_set_h3_cmd(sess, h3tx);
    tokio::spawn(async move {
        let mut done = false;
        let mut h3_conn: h3::server::Connection<h3_quinn::Connection, bytes::Bytes> =
            match h3::server::Connection::new(h3_quinn::Connection::new(conn.clone())).await {
                Ok(c) => c,
                Err(e) => {
                    let _ = inbox.send(QuicEvent::SessionError { id: sess, message: format!("ERR_QUIC_H3: {e}") });
                    sess_finish(&inbox, sess, -1, "h3 init failed".into(), &mut done);
                    return;
                }
            };
        let mut streams: std::collections::HashMap<
            u64,
            h3::server::RequestStream<h3_quinn::BidiStream<bytes::Bytes>, bytes::Bytes>,
        > = std::collections::HashMap::new();
        loop {
            tokio::select! {
                cmd = h3rx.recv() => {
                    match cmd {
                        None => break,
                        Some(QuicH3Cmd::Respond { stream, status, headers, body }) => {
                            let Some(mut st) = streams.remove(&stream) else { continue };
                            let mut builder = http::Response::builder().status(status);
                            for (k, v) in &headers {
                                builder = builder.header(k, v);
                            }
                            if let Ok(resp) = builder.body(()) {
                                if st.send_response(resp).await.is_ok() {
                                    if !body.is_empty() {
                                        let _ = st.send_data(bytes::Bytes::from(body)).await;
                                    }
                                    let _ = st.finish().await;
                                }
                            }
                        }
                        Some(QuicH3Cmd::Request { .. }) => {} // 服务端无此命令
                    }
                }
                acc = h3_conn.accept(), if !done => {
                    match acc {
                        Ok(Some(resolver)) => {
                            if let Ok((req, mut stream)) = resolver.resolve_request().await {
                                // 体先备齐再发事件（§4.35 同口径；顺序处理 v1 记档）。
                                let mut body_acc = Vec::new();
                                loop {
                                    match stream.recv_data().await {
                                        Ok(Some(chunk)) => {
                                            use bytes::Buf as _;
                                            body_acc.extend_from_slice(chunk.chunk());
                                        }
                                        Ok(None) => break,
                                        Err(_) => break,
                                    }
                                }
                                let sid = state::quic_alloc_id();
                                state::quic_stream_insert(sid, sess, state::QuicStreamDir::Bidi);
                                streams.insert(sid, stream);
                                let _ = inbox.send(QuicEvent::H3Request {
                                    sess,
                                    stream: sid,
                                    method: req.method().as_str().to_string(),
                                    path: req.uri().path().to_string(),
                                    headers: h3_headers_value(req.headers()),
                                    body: base64::engine::general_purpose::STANDARD.encode(&body_acc),
                                });
                            }
                        }
                        // Ok(None)/Err：h3 连接终结 → 等待 QUIC 关闭原因后收尾
                        _ => {
                            let err = conn.closed().await;
                            let (code, reason) = close_info(err);
                            sess_finish(&inbox, sess, code, reason, &mut done);
                            break;
                        }
                    }
                }
                err = conn.closed(), if !done => {
                    let (code, reason) = close_info(err);
                    sess_finish(&inbox, sess, code, reason, &mut done);
                    break;
                }
            }
        }
    })
    .abort_handle()
}

/// 客户端 H3 服务任务：h3 driver 后台轮询 + Request 命令串行处理
/// （send → [body] → recv_response → recv_data 全量）→ `H3Response`；
/// `closed()` 守望收尾（driver abort 后发 `SessionClose`，§4.52 顺序）。
pub fn spawn_h3_client(
    sess: u64,
    conn: quinn::Connection,
    inbox: tokio::sync::mpsc::UnboundedSender<QuicEvent>,
) -> tokio::task::AbortHandle {
    let (h3tx, mut h3rx) = tokio::sync::mpsc::unbounded_channel::<QuicH3Cmd>();
    state::quic_sess_set_h3_cmd(sess, h3tx);
    tokio::spawn(async move {
        let mut done = false;
        let (mut driver, mut send_request) =
            match h3::client::new(h3_quinn::Connection::new(conn.clone())).await {
                Ok(pair) => pair,
                Err(e) => {
                    let _ = inbox.send(QuicEvent::SessionError { id: sess, message: format!("ERR_QUIC_H3: {e}") });
                    sess_finish(&inbox, sess, -1, "h3 init failed".into(), &mut done);
                    return;
                }
            };
        let driver_task = tokio::spawn(async move {
            driver.wait_idle().await;
        });
        loop {
            tokio::select! {
                cmd = h3rx.recv() => {
                    match cmd {
                        None => break,
                        Some(QuicH3Cmd::Request { stream, method, path, headers, body }) => {
                            let authority = state::quic_sess_addrs(sess)
                                .map(|(_, remote)| remote)
                                .unwrap_or_default();
                            let mut builder = http::Request::builder()
                                .method(method.as_str())
                                .uri(format!("https://{authority}{path}"));
                            for (k, v) in &headers {
                                builder = builder.header(k, v);
                            }
                            let req = match builder.body(()) {
                                Ok(r) => r,
                                Err(e) => {
                                    let _ = inbox.send(QuicEvent::StreamError { id: stream, message: format!("ERR_QUIC_H3: bad request ({e})") });
                                    let _ = inbox.send(QuicEvent::StreamClosed { id: stream, code: -1 });
                                    continue;
                                }
                            };
                            match send_request.send_request(req).await {
                                Ok(mut st) => {
                                    if !body.is_empty() {
                                        let _ = st.send_data(bytes::Bytes::from(body)).await;
                                    }
                                    let _ = st.finish().await;
                                    match st.recv_response().await {
                                        Ok(resp) => {
                                            let status = resp.status().as_u16();
                                            let headers = h3_headers_value(resp.headers());
                                            let mut body_acc = Vec::new();
                                            loop {
                                                match st.recv_data().await {
                                                    Ok(Some(chunk)) => {
                                                        use bytes::Buf as _;
                                                        body_acc.extend_from_slice(chunk.chunk());
                                                    }
                                                    Ok(None) => break,
                                                    Err(_) => break,
                                                }
                                            }
                                            let _ = inbox.send(QuicEvent::H3Response {
                                                stream,
                                                status,
                                                headers,
                                                body: base64::engine::general_purpose::STANDARD.encode(&body_acc),
                                            });
                                        }
                                        Err(e) => {
                                            let _ = inbox.send(QuicEvent::StreamError { id: stream, message: format!("ERR_QUIC_H3: {e}") });
                                            let _ = inbox.send(QuicEvent::StreamClosed { id: stream, code: -1 });
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = inbox.send(QuicEvent::StreamError { id: stream, message: format!("ERR_QUIC_H3: {e}") });
                                    let _ = inbox.send(QuicEvent::StreamClosed { id: stream, code: -1 });
                                }
                            }
                        }
                        Some(QuicH3Cmd::Respond { .. }) => {} // 客户端无此命令
                    }
                }
                err = conn.closed(), if !done => {
                    driver_task.abort();
                    let (code, reason) = close_info(err);
                    sess_finish(&inbox, sess, code, reason, &mut done);
                    break;
                }
            }
        }
    })
    .abort_handle()
}
