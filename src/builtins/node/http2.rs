//! `node:http2`：compat 面 Server/Client（hyper `server`+`http2` 直引，§h2 拍板口径）。
//! 事件模型与 net 同构：Rust task → net 通道（`H2Request`/`H2Stream`/
//! `H2SessionClose` 变体，零新 channel）→ `dispatch` 调 target 预绑定的 `__ev`。
//! 连接驱动与 hyper conn future 同 task（`select!` 兼收 `H2Respond`/`H2Open`/
//! `Close` 命令）；服务端应答经 oneshot 回 service_fn。
//! 偏差记档：
//! - h2c 仅 prior-knowledge（无 Upgrade/h1c 回落）；`allowHTTP1` 不做。
//! - 体整收（server 收齐才发 request，client 收齐才发 data/end；http 记档同款）。
//! - 服务端推送（pushStream）、trailer、优先级/流控调参、ping/settings 细 knob 不做。
//! - `session.socket`/`alpnProtocol` 等反射面仅 `encrypted` 布尔；`getPeerCertificate` 不做。
//! - 状态码/头非法（JS 侧）→ 500 兜底（不断连）。

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::context::JSContext;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::builtins::node::net::{NetCmd, NetEvent, NetKind};
use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};
use crate::state;

// ── hyper 胶水（零新 crate；Executor/Body/IO 手写小件）───────────────────────

/// tokio 执行器（hyper 文档口径；blanket 实现覆盖 client/server h2）。
#[derive(Clone, Copy, Debug)]
struct Exec;

impl<F> hyper::rt::Executor<F> for Exec
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    fn execute(&self, fut: F) {
        tokio::spawn(fut);
    }
}

/// 单块请求/响应体（整收口径；`http_body::Body` 手写 20 行，不另引 http-body-util）。
struct OneBody {
    data: Option<bytes::Bytes>,
}

impl http_body::Body for OneBody {
    type Data = bytes::Bytes;
    type Error = std::io::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        match self.get_mut().data.take() {
            Some(b) => Poll::Ready(Some(Ok(http_body::Frame::data(b)))),
            None => Poll::Ready(None),
        }
    }
}

/// tokio 流 → hyper IO（hyper-util 同款 recipe，本仓手写）。
#[derive(Debug)]
struct TokioIo<T>(T);

impl<T> TokioIo<T> {
    fn new(inner: T) -> Self {
        Self(inner)
    }
}

impl<T> hyper::rt::Read for TokioIo<T>
where
    T: tokio::io::AsyncRead + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut buf: hyper::rt::ReadBufCursor<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        // SAFETY: ReadBufCursor 的未填区即合法写区（hyper-util 同款 unsafe 用法）
        let uninit = unsafe { buf.as_mut() };
        let mut tbuf = tokio::io::ReadBuf::uninit(uninit);
        futures::ready!(tokio::io::AsyncRead::poll_read(
            Pin::new(&mut self.get_mut().0),
            cx,
            &mut tbuf
        ))?;
        let n = tbuf.filled().len();
        unsafe { buf.advance(n) };
        Poll::Ready(Ok(()))
    }
}

impl<T> hyper::rt::Write for TokioIo<T>
where
    T: tokio::io::AsyncWrite + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        tokio::io::AsyncWrite::poll_write(Pin::new(&mut self.get_mut().0), cx, buf)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        tokio::io::AsyncWrite::poll_flush(Pin::new(&mut self.get_mut().0), cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        tokio::io::AsyncWrite::poll_shutdown(Pin::new(&mut self.get_mut().0), cx)
    }
}

// ── 小件 ────────────────────────────────────────────────────────────────────

fn b64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn b64dec(s: &str) -> Result<Vec<u8>, String> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| format!("H2 protocol: bad base64 ({e})"))
}

/// HeaderMap → JSON `[[k, v], …]`（值按 latin1 语义 lossy 还原）。
fn headers_json(headers: &http::HeaderMap) -> String {
    let pairs: Vec<(String, String)> = headers
        .iter()
        .map(|(k, v)| (k.as_str().to_owned(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
        .collect();
    serde_json::to_string(&pairs).unwrap_or_else(|_| "[]".into())
}

/// JSON `[[k, v], …]` → 头表（名/值非法即跳过，调用方记档）。
fn parse_headers(json: &str) -> Vec<(String, String)> {
    let raw: Vec<Vec<String>> = serde_json::from_str(json).unwrap_or_default();
    raw.into_iter()
        .filter_map(|p| {
            let (k, v) = (p.first()?.to_owned(), p.get(1)?.to_owned());
            http::header::HeaderName::from_bytes(k.as_bytes()).ok()?;
            http::header::HeaderValue::from_str(&v).ok()?;
            Some((k.to_ascii_lowercase(), v))
        })
        .collect()
}

/// 服务端应答（oneshot 负载）。
struct H2Resp {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

type Responders = Arc<tokio::sync::Mutex<HashMap<u64, tokio::sync::oneshot::Sender<H2Resp>>>>;

fn set_rval_str(cx: &mut JSContext, frame: &Frame, s: &str) {
    rooted!(&in(cx) let mut v = UndefinedValue());
    s.to_jsval(cx, v.handle_mut());
    frame.set_rval(v.get());
}

fn opt_num(frame: &Frame, i: u32) -> Option<f64> {
    let v = frame.arg(i);
    if v.is_number() { Some(v.to_number()) } else { None }
}

fn ensure_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

// ── 服务端 ──────────────────────────────────────────────────────────────────

/// 读全请求体（整收口径；错即 Err）。
async fn read_body(body: hyper::body::Incoming) -> Result<Vec<u8>, String> {
    use http_body::Body as _;
    use std::future::poll_fn;
    let mut body = body;
    let mut out = Vec::new();
    loop {
        match poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await {
            None => return Ok(out),
            Some(Ok(f)) => {
                if let Some(d) = f.data_ref() {
                    out.extend_from_slice(d);
                }
            }
            Some(Err(e)) => return Err(format!("H2 stream error: {e}")),
        }
    }
}

/// 单连接驱动（`select!` 兼收应答命令；结束即静默 purge entry）。
async fn serve_conn<IO>(
    io: IO,
    server_id: u64,
    conn_id: u64,
    ev_tx: tokio::sync::mpsc::UnboundedSender<NetEvent>,
    mut cmd_rx: tokio::sync::mpsc::UnboundedReceiver<NetCmd>,
) where
    IO: hyper::rt::Read + hyper::rt::Write + Unpin + Send + 'static,
{
    let responders: Responders = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
    let seq = Arc::new(std::sync::atomic::AtomicU64::new(1));
    let svc_responders = responders.clone();
    let svc_seq = seq.clone();
    let svc_tx = ev_tx.clone();
    let svc = hyper::service::service_fn(move |req: http::Request<hyper::body::Incoming>| {
        let responders = svc_responders.clone();
        let seq = svc_seq.clone();
        let ev_tx = svc_tx.clone();
        async move {
            let stream_id = seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let method = req.method().to_string();
            let path = req
                .uri()
                .path_and_query()
                .map(|pq| pq.to_string())
                .unwrap_or_else(|| "/".into());
            let headers = headers_json(req.headers());
            let body = match read_body(req.into_body()).await {
                Ok(b) => b,
                Err(e) => {
                    let _ = ev_tx.send(NetEvent {
                        id: server_id,
                        kind: NetKind::Error {
                            code: "ERR_HTTP2_STREAM_ERROR".into(),
                            msg: e,
                        },
                    });
                    Vec::new()
                }
            };
            let (tx, rx) = tokio::sync::oneshot::channel();
            responders.lock().await.insert(stream_id, tx);
            let _ = ev_tx.send(NetEvent {
                id: server_id,
                kind: NetKind::H2Request {
                    conn_id,
                    stream_id,
                    method,
                    path,
                    headers,
                    body_b64: b64(&body),
                },
            });
            // JS 不应答即挂起（记档）；连接死亡则 oneshot 断开走 503 兜底
            let resp = match rx.await {
                Ok(r) => r,
                Err(_) => H2Resp { status: 503, headers: Vec::new(), body: Vec::new() },
            };
            let mut builder = http::Response::builder().status(resp.status);
            for (k, v) in &resp.headers {
                builder = builder.header(k, v);
            }
            let body = OneBody { data: Some(bytes::Bytes::from(resp.body)) };
            Ok::<_, anyhow::Error>(builder.body(body).unwrap_or_else(|_| {
                http::Response::builder()
                    .status(500)
                    .body(OneBody { data: None })
                    .expect("500 fallback builds")
            }))
        }
    });
    let conn = hyper::server::conn::http2::Builder::new(Exec).serve_connection(io, svc);
    tokio::pin!(conn);
    loop {
        tokio::select! {
            r = &mut conn => {
                let _ = r;
                break;
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(NetCmd::H2Respond { stream_id, status, headers, body_b64 }) => {
                        let sender = responders.lock().await.remove(&stream_id);
                        if let Some(tx) = sender {
                            let body = b64dec(&body_b64).unwrap_or_default();
                            let _ = tx.send(H2Resp { status, headers: parse_headers(&headers), body });
                        }
                    }
                    Some(NetCmd::Close) | None => break,
                    _ => {}
                }
            }
        }
    }
    state::net_purge(conn_id);
}

/// 监听选项 JSON：`{tls?: {cert, key}}`（h2c 缺省）。
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct ListenOpts {
    tls: Option<TlsServerOpts>,
}

#[derive(Debug, Default, serde::Deserialize, Clone)]
#[serde(default)]
struct TlsServerOpts {
    cert: Option<String>,
    key: Option<String>,
}

/// `__wjs_h2_listen(port, host, optsJson, target)` → server id。
pub unsafe extern "C" fn h2_listen(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 || !frame.arg(3).is_object() {
        report_error(&mut cx, "TypeError: h2 listen internals missing target");
        return false;
    }
    let Some(port) = opt_num(&frame, 0) else {
        report_error(&mut cx, "TypeError: h2 listen: port must be a number");
        return false;
    };
    let host = value_to_string(&mut cx, frame.arg(1));
    let opts: ListenOpts =
        serde_json::from_str(&value_to_string(&mut cx, frame.arg(2))).unwrap_or_default();
    let target = frame.arg(3);
    ensure_provider();
    // TLS 配置预检（fail fast；h2c 无配置）
    let tls_cfg: Option<std::sync::Arc<rustls::ServerConfig>> = match opts.tls {
        None => None,
        Some(t) => {
            let (Some(cert), Some(key)) = (t.cert, t.key) else {
                report_error(&mut cx, "TypeError: http2 secure server needs { key, cert }");
                return false;
            };
            match crate::builtins::node::tls::server_config_h2(&cert, &key) {
                Ok(c) => Some(std::sync::Arc::new(c)),
                Err(e) => {
                    report_error(&mut cx, &e);
                    return false;
                }
            }
        }
    };
    let Some((id, ev_tx)) = state::net_alloc() else {
        report_error(&mut cx, "OperationError: net driver not installed");
        return false;
    };
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        report_error(&mut cx, "OperationError: no async runtime for h2 listen");
        return false;
    };
    let mut cmd_rx = state::net_socket_add(id, target);
    set_rval_str(&mut cx, &frame, &id.to_string());
    handle.spawn(async move {
        let bound = tokio::net::TcpListener::bind((host.as_str(), port as u16)).await;
        let Ok(listener) = bound else {
            let e = bound.unwrap_err();
            let code = crate::builtins::node::fs::io_code(&e);
            let _ = ev_tx.send(NetEvent {
                id,
                kind: NetKind::ServerError { code: code.into(), msg: format!("{code}: {e}") },
            });
            let _ = ev_tx.send(NetEvent { id, kind: NetKind::ServerClose });
            return;
        };
        let local = listener
            .local_addr()
            .unwrap_or_else(|_| "0.0.0.0:0".parse::<std::net::SocketAddr>().expect("literal addr"));
        let _ = ev_tx.send(NetEvent {
            id,
            kind: NetKind::Listening { addr: local.ip().to_string(), port: local.port() },
        });
        // 存活连接表（server close 时逐个 Close；conn 退出经 done 通道摘除）
        let mut live: std::collections::HashSet<u64> = std::collections::HashSet::new();
        let (done_tx, mut done_rx) =
            tokio::sync::mpsc::unbounded_channel::<u64>();
        let acceptor = tls_cfg.map(tokio_rustls::TlsAcceptor::from);
        loop {
            tokio::select! {
                acc = listener.accept() => {
                    let Ok((stream, _peer)) = acc else { continue };
                    let (conn_id, conn_cmd_rx) = state::net_conn_add();
                    live.insert(conn_id);
                    let ev2 = ev_tx.clone();
                    let done2 = done_tx.clone();
                    if let Some(acc) = acceptor.clone() {
                        let fut = async move {
                            match acc.accept(stream).await {
                                Err(_) => {
                                    state::net_purge(conn_id);
                                    let _ = done2.send(conn_id);
                                }
                                Ok(tls) => {
                                    serve_conn(TokioIo::new(tls), id, conn_id, ev2, conn_cmd_rx).await;
                                    let _ = done2.send(conn_id);
                                }
                            }
                        };
                        tokio::spawn(fut);
                    } else {
                        let fut = async move {
                            serve_conn(TokioIo::new(stream), id, conn_id, ev2, conn_cmd_rx).await;
                            let _ = done2.send(conn_id);
                        };
                        tokio::spawn(fut);
                    }
                }
                done = done_rx.recv() => {
                    if let Some(cid) = done {
                        live.remove(&cid);
                    }
                }
                _ = cmd_rx.recv() => break,
            }
        }
        for cid in live {
            let _ = state::net_cmd(cid, NetCmd::Close);
        }
        let _ = ev_tx.send(NetEvent { id, kind: NetKind::ServerClose });
    });
    true
}

// ── 客户端 ──────────────────────────────────────────────────────────────────

/// 连接选项 JSON：`{tls?: {ca?, rejectUnauthorized?, servername?}}`。
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct ConnectOpts {
    tls: Option<TlsClientOpts>,
}

#[derive(Debug, Default, serde::Deserialize, Clone)]
#[serde(default)]
struct TlsClientOpts {
    #[serde(rename = "ca")]
    ca_pem: Option<String>,
    #[serde(rename = "rejectUnauthorized")]
    reject_unauthorized: Option<bool>,
    servername: Option<String>,
}

/// 开流请求 JSON：`{method, path, scheme, authority, headers: [[k,v]]}`。
#[derive(Debug, serde::Deserialize)]
struct OpenReq {
    method: String,
    path: String,
    #[serde(default = "default_scheme")]
    scheme: String,
    #[serde(default)]
    authority: String,
    #[serde(default)]
    headers: Vec<Vec<String>>,
}

fn default_scheme() -> String {
    "http".into()
}

/// 客户端 session 驱动（conn 与命令同 task `select!`；终结即 Close 事件）。
async fn drive_session<S>(
    io: TokioIo<S>,
    id: u64,
    ev_tx: tokio::sync::mpsc::UnboundedSender<NetEvent>,
    mut cmd_rx: tokio::sync::mpsc::UnboundedReceiver<NetCmd>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let hs = hyper::client::conn::http2::Builder::new(Exec).handshake::<_, OneBody>(io);
    let (sender, conn) = match hs.await {
        Ok(t) => t,
        Err(e) => {
            let _ = ev_tx.send(NetEvent {
                id,
                kind: NetKind::Error {
                    code: "ERR_HTTP2_CONNECT".into(),
                    msg: format!("ERR_HTTP2_CONNECT: {e}"),
                },
            });
            if state::net_close_once(id) {
                let _ = ev_tx.send(NetEvent { id, kind: NetKind::H2SessionClose });
            }
            return;
        }
    };
    let _ = ev_tx.send(NetEvent { id, kind: NetKind::Connect { local: None } });
    tokio::pin!(conn);
    loop {
        tokio::select! {
            r = &mut conn => {
                let _ = r;
                if state::net_close_once(id) {
                    let _ = ev_tx.send(NetEvent { id, kind: NetKind::H2SessionClose });
                }
                return;
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(NetCmd::H2Open { stream_id, headers, body_b64 }) => {
                        let req: OpenReq = match serde_json::from_str(&headers) {
                            Ok(r) => r,
                            Err(e) => {
                                let _ = ev_tx.send(NetEvent {
                                    id,
                                    kind: NetKind::H2Stream {
                                        stream_id,
                                        what: "error".into(),
                                        payload: serde_json::json!({
                                            "code": "ERR_HTTP2_STREAM_ERROR",
                                            "msg": format!("bad open params: {e}"),
                                        })
                                        .to_string(),
                                    },
                                });
                                continue;
                            }
                        };
                        let body = b64dec(&body_b64).unwrap_or_default();
                        // :authority 经完整 URI 带（hyper h2 客户端不合成伪头）
                        let uri = format!("{}://{}{}", req.scheme, req.authority, req.path);
                        let mut builder = http::Request::builder()
                            .method(req.method.as_str())
                            .uri(uri.as_str());
                        for (k, v) in parse_headers(&serde_json::to_string(&req.headers).unwrap_or_default()) {
                            builder = builder.header(k.as_str(), v.as_str());
                        }
                        let request = builder.body(OneBody { data: Some(bytes::Bytes::from(body)) });
                        let request = match request {
                            Ok(r) => r,
                            Err(e) => {
                                let _ = ev_tx.send(NetEvent {
                                    id,
                                    kind: NetKind::H2Stream {
                                        stream_id,
                                        what: "error".into(),
                                        payload: serde_json::json!({
                                            "code": "ERR_HTTP2_STREAM_ERROR",
                                            "msg": format!("bad request: {e}"),
                                        })
                                        .to_string(),
                                    },
                                });
                                continue;
                            }
                        };
                        let ev2 = ev_tx.clone();
                        let mut sender = sender.clone();
                        tokio::spawn(async move {
                            match sender.send_request(request).await {
                                Err(e) => {
                                    let _ = ev2.send(NetEvent {
                                        id,
                                        kind: NetKind::H2Stream {
                                            stream_id,
                                            what: "error".into(),
                                            payload: serde_json::json!({
                                                "code": "ERR_HTTP2_STREAM_ERROR",
                                                "msg": format!("{e}"),
                                            })
                                            .to_string(),
                                        },
                                    });
                                }
                                Ok(resp) => {
                                    let status = resp.status().as_u16();
                                    let head = headers_json(resp.headers());
                                    let _ = ev2.send(NetEvent {
                                        id,
                                        kind: NetKind::H2Stream {
                                            stream_id,
                                            what: "response".into(),
                                            payload: serde_json::json!({ "status": status, "headers": head }).to_string(),
                                        },
                                    });
                                    match read_body(resp.into_body()).await {
                                        Ok(b) => {
                                            if !b.is_empty() {
                                                let _ = ev2.send(NetEvent {
                                                    id,
                                                    kind: NetKind::H2Stream {
                                                        stream_id,
                                                        what: "data".into(),
                                                        payload: b64(&b),
                                                    },
                                                });
                                            }
                                            let _ = ev2.send(NetEvent {
                                                id,
                                                kind: NetKind::H2Stream {
                                                    stream_id,
                                                    what: "end".into(),
                                                    payload: String::new(),
                                                },
                                            });
                                        }
                                        Err(e) => {
                                            let _ = ev2.send(NetEvent {
                                                id,
                                                kind: NetKind::H2Stream {
                                                    stream_id,
                                                    what: "error".into(),
                                                    payload: serde_json::json!({
                                                        "code": "ERR_HTTP2_STREAM_ERROR",
                                                        "msg": e,
                                                    })
                                                    .to_string(),
                                                },
                                            });
                                        }
                                    }
                                }
                            }
                        });
                    }
                    Some(NetCmd::Close) | None => {
                        if state::net_close_once(id) {
                            let _ = ev_tx.send(NetEvent { id, kind: NetKind::H2SessionClose });
                        }
                        return;
                    }
                    _ => {}
                }
            }
        }
    }
}

/// `__wjs_h2_connect(host, port, optsJson, target)` → session id。
pub unsafe extern "C" fn h2_connect(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 || !frame.arg(3).is_object() {
        report_error(&mut cx, "TypeError: h2 connect internals missing target");
        return false;
    }
    let host = value_to_string(&mut cx, frame.arg(0));
    let Some(port) = opt_num(&frame, 1) else {
        report_error(&mut cx, "TypeError: h2 connect: port must be a number");
        return false;
    };
    let opts: ConnectOpts =
        serde_json::from_str(&value_to_string(&mut cx, frame.arg(2))).unwrap_or_default();
    let target = frame.arg(3);
    ensure_provider();
    let Some((id, ev_tx)) = state::net_alloc() else {
        report_error(&mut cx, "OperationError: net driver not installed");
        return false;
    };
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        report_error(&mut cx, "OperationError: no async runtime for h2 connect");
        return false;
    };
    let cmd_rx = state::net_socket_add(id, target);
    set_rval_str(&mut cx, &frame, &id.to_string());
    handle.spawn(async move {
        let tcp = match tokio::net::TcpStream::connect((host.as_str(), port as u16)).await {
            Ok(s) => s,
            Err(e) => {
                let code = crate::builtins::node::fs::io_code(&e);
                let _ = ev_tx.send(NetEvent {
                    id,
                    kind: NetKind::Error { code: code.into(), msg: format!("{code}: {e}") },
                });
                if state::net_close_once(id) {
                    let _ = ev_tx.send(NetEvent { id, kind: NetKind::H2SessionClose });
                }
                return;
            }
        };
        if let Some(t) = opts.tls {
            let reject = t.reject_unauthorized.unwrap_or(true);
            let cfg = match crate::builtins::node::tls::client_config_h2(t.ca_pem.as_deref(), reject) {
                Ok(c) => c,
                Err(e) => {
                    report_error_static(&ev_tx, id, &e);
                    return;
                }
            };
            let servername = t.servername.unwrap_or_else(|| host.clone());
            let name = match rustls::pki_types::ServerName::try_from(servername.clone()) {
                Ok(n) => n,
                Err(e) => {
                    report_error_static(
                        &ev_tx,
                        id,
                        &format!("ERR_TLS_HANDSHAKE: bad servername '{servername}': {e}"),
                    );
                    return;
                }
            };
            let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(cfg));
            match connector.connect(name, tcp).await {
                Err(e) => {
                    report_error_static(&ev_tx, id, &format!("ERR_TLS_HANDSHAKE: {e}"));
                }
                Ok(tls) => drive_session(TokioIo::new(tls), id, ev_tx, cmd_rx).await,
            }
        } else {
            drive_session(TokioIo::new(tcp), id, ev_tx, cmd_rx).await;
        }
    });
    true
}

/// task 内错误上报（Error + 单次 Close；native 已返回，无 Frame 可用）。
fn report_error_static(
    ev_tx: &tokio::sync::mpsc::UnboundedSender<NetEvent>,
    id: u64,
    msg: &str,
) {
    let code = msg.split(':').next().unwrap_or("ERR_HTTP2_CONNECT");
    let _ = ev_tx.send(NetEvent {
        id,
        kind: NetKind::Error { code: code.into(), msg: msg.into() },
    });
    if state::net_close_once(id) {
        let _ = ev_tx.send(NetEvent { id, kind: NetKind::H2SessionClose });
    }
}

/// `__wjs_h2_open(sessionId, streamId, reqJson, bodyB64)`。
pub unsafe extern "C" fn h2_open(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(sid), Some(stid)) = (opt_num(&frame, 0), opt_num(&frame, 1)) else {
        report_error(&mut cx, "TypeError: h2 open: ids must be numbers");
        return false;
    };
    let headers = value_to_string(&mut cx, frame.arg(2));
    let body_b64 = value_to_string(&mut cx, frame.arg(3));
    if !state::net_cmd(
        sid as u64,
        NetCmd::H2Open { stream_id: stid as u64, headers, body_b64 },
    ) {
        report_error(&mut cx, "ERR_HTTP2_ERROR: h2 open: session is gone");
        return false;
    }
    true
}

/// `__wjs_h2_respond(connId, streamId, status, headersJson, bodyB64)`。
pub unsafe extern "C" fn h2_respond(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(cid), Some(stid), Some(status)) =
        (opt_num(&frame, 0), opt_num(&frame, 1), opt_num(&frame, 2))
    else {
        report_error(&mut cx, "TypeError: h2 respond: ids/status must be numbers");
        return false;
    };
    let headers = value_to_string(&mut cx, frame.arg(3));
    let body_b64 = value_to_string(&mut cx, frame.arg(4));
    // 连接已收尾即静默 true（竞态：客户端先走；Node 口径不抛）
    let _ = state::net_cmd(
        cid as u64,
        NetCmd::H2Respond { stream_id: stid as u64, status: status as u16, headers, body_b64 },
    );
    true
}

/// 内嵌 ESM 源（`node:http2`；compat 子集，见头注偏差）。
pub const SOURCE: &str = r#"
import { EventEmitter } from "node:events";
const Buffer = globalThis.Buffer;

function __b64dec(s) {
  const bin = atob(s);
  const u8 = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) u8[i] = bin.charCodeAt(i);
  return u8;
}
function __b64enc(u8) {
  let s = "";
  for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000));
  return btoa(s);
}
function __toU8(data, what) {
  if (typeof data === "string") return new TextEncoder().encode(data);
  if (data instanceof Uint8Array) return data;
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  throw new TypeError(`${what}: data must be string or BufferSource`);
}
function __h2Err(code, msg) {
  const e = new Error(msg);
  e.code = code;
  return e;
}
function __pairsToObj(pairs) {
  const out = Object.create(null);
  for (const [k, v] of pairs) {
    const lk = String(k).toLowerCase();
    out[lk] = out[lk] === undefined ? String(v) : `${out[lk]}, ${v}`;
  }
  return out;
}

// ── 服务端 ──────────────────────────────────────────────────────────────
class Http2ServerRequest extends EventEmitter {
  constructor(info) {
    super();
    this.method = info.method;
    this.url = info.path;
    this.headers = __pairsToObj(JSON.parse(info.headers));
    this.rawHeaders = JSON.parse(info.headers).flat();
    this.httpVersion = "2.0";
    this.complete = false;
    this.__conn = Number(info.connId);
    this.__stream = Number(info.streamId);
  }
  __feed(b64body) {
    const u8 = __b64dec(b64body);
    if (u8.length > 0) this.emit("data", Buffer.from(u8));
    this.complete = true;
    this.emit("end");
  }
}
class Http2ServerResponse extends EventEmitter {
  constructor(req) {
    super();
    this.__conn = req.__conn;
    this.__stream = req.__stream;
    this.statusCode = 200;
    this.__headers = Object.create(null);
    this.headersSent = false;
    this.__done = false;
  }
  setHeader(n, v) { this.__headers[String(n).toLowerCase()] = String(v); return this; }
  getHeader(n) { return this.__headers[String(n).toLowerCase()]; }
  writeHead(status, obj) {
    this.statusCode = status;
    if (obj && typeof obj === "object") {
      for (const [k, v] of Object.entries(obj)) this.__headers[String(k).toLowerCase()] = String(v);
    }
    return this;
  }
  end(chunk) {
    if (this.__done) return this;
    this.__done = true;
    this.headersSent = true;
    const body = chunk === undefined || chunk === null ? new Uint8Array(0) : __toU8(chunk, "end");
    __wjs_h2_respond(this.__conn, this.__stream, this.statusCode,
      JSON.stringify(Object.entries(this.__headers)), __b64enc(body));
    this.emit("finish");
    this.emit("close");
    return this;
  }
}

class Http2Server extends EventEmitter {
  constructor(options, secure) {
    super();
    this.__id = 0;
    this.__listening = null;
    this.__secure = !!secure;
    this.__tlsOpts = null;
    if (options && typeof options === "object") {
      if (secure) {
        if (options.key === undefined || options.cert === undefined) {
          throw new TypeError("http2.createSecureServer needs { key, cert } PEM strings");
        }
        this.__tlsOpts = { tls: { key: String(options.key), cert: String(options.cert) } };
      }
    }
    // request 监听器只由 createServer/createSecureServer 接线（构造器不再重复注册）
    // 派发钩子预绑定（dispatch 以 global 为 this）
    this.__ev = this.__ev.bind(this);
  }
  listen(...args) {
    let port, host = null, cb = null;
    if (typeof args[0] === "object" && args[0] !== null) {
      port = args[0].port;
      host = args[0].host ?? null;
      cb = typeof args[1] === "function" ? args[1] : null;
    } else {
      port = args[0];
      for (let i = 1; i < args.length; i++) {
        if (typeof args[i] === "string" && host === null) host = args[i];
        else if (typeof args[i] === "function") cb = args[i];
      }
    }
    if (cb) this.once("listening", cb);
    this.__port = Number(port);
    this.__id = Number(__wjs_h2_listen(Number(port), host === null ? "0.0.0.0" : host,
      JSON.stringify(this.__tlsOpts ?? {}), this));
    return this;
  }
  __ev(kind, payload) {
    switch (kind) {
      case "listening": {
        const o = JSON.parse(payload);
        this.__listening = { address: o.addr, port: o.port, family: String(o.addr).includes(":") ? "IPv6" : "IPv4" };
        this.emit("listening");
        break;
      }
      case "request": {
        const o = JSON.parse(payload);
        const req = new Http2ServerRequest(o);
        const res = new Http2ServerResponse(req);
        this.emit("request", req, res);
        this.emit("stream", req, res);
        req.__feed(o.body);
        break;
      }
      case "error": {
        const o = JSON.parse(payload);
        this.emit("error", __h2Err(o.code, o.msg));
        break;
      }
      case "close": this.emit("close"); break;
    }
  }
  address() { return this.__listening; }
  close(cb) {
    if (typeof cb === "function") this.once("close", cb);
    if (this.__id) __wjs_net_destroy(this.__id);
    return this;
  }
  ref() { return this; }
  unref() { return this; }
}

// ── 客户端 ──────────────────────────────────────────────────────────────
class ClientHttp2Stream extends EventEmitter {
  constructor(session, id, headers, body) {
    super();
    this.__session = session;
    this.id = id;
    this.sentHeaders = headers;
    this.__buf = [];
    this.__ended = false;
    this.closed = false;
    this.destroyed = false;
    // 开流（整收上传口径：write 缓冲，end 下发；http ClientRequest 同款）
    this.__pendingBody = body ? [body] : [];
  }
  write(chunk) {
    if (this.__ended) throw __h2Err("ERR_STREAM_WRITE_AFTER_END", "write after end");
    this.__pendingBody.push(__toU8(chunk, "write"));
    return true;
  }
  end(chunk) {
    if (this.__ended) return this;
    if (chunk !== undefined && chunk !== null) this.__pendingBody.push(__toU8(chunk, "end"));
    this.__ended = true;
    let total = 0;
    for (const b of this.__pendingBody) total += b.length;
    const flat = new Uint8Array(total);
    let off = 0;
    for (const b of this.__pendingBody) { flat.set(b, off); off += b.length; }
    this.__pendingBody = [];
    __wjs_h2_open(this.__session.__id, this.id,
      JSON.stringify({
        method: this.sentHeaders[":method"], path: this.sentHeaders[":path"],
        scheme: this.sentHeaders[":scheme"], authority: this.sentHeaders[":authority"],
        headers: Object.entries(this.sentHeaders).filter(([k]) => !k.startsWith(":")),
      }),
      __b64enc(flat));
    return this;
  }
  close() {
    if (!this.closed) { this.closed = true; this.emit("close"); }
  }
}

class ClientHttp2Session extends EventEmitter {
  constructor(authority, options) {
    super();
    this.__id = 0;
    this.__seq = 1;
    this.__streams = new Map();
    this.__authority = authority;
    this.__options = options ?? {};
    this.destroyed = false;
    this.closed = false;
    // 派发钩子预绑定
    this.__ev = this.__ev.bind(this);
  }
  __start(port, host) {
    const wire = {};
    if (this.__options.tls !== undefined || this.__secure) {
      const t = this.__options.tls ?? this.__options;
      wire.tls = {};
      if (t.ca !== undefined) wire.tls.ca = String(t.ca);
      if (t.rejectUnauthorized !== undefined) wire.tls.rejectUnauthorized = !!t.rejectUnauthorized;
      if (t.servername !== undefined) wire.tls.servername = String(t.servername);
    }
    this.__id = Number(__wjs_h2_connect(host, port, JSON.stringify(wire), this));
    return this;
  }
  __ev(kind, payload) {
    switch (kind) {
      case "connect":
        this.emit("connect", this, null);
        break;
      case "response": {
        const o = JSON.parse(payload);
        const st = this.__streams.get(Number(o.streamId));
        if (!st) break;
        const head = JSON.parse(o.payload);
        const headers = __pairsToObj(JSON.parse(head.headers));
        headers[":status"] = head.status;
        st.emit("response", headers, 0);
        break;
      }
      case "data": {
        const o = JSON.parse(payload);
        const st = this.__streams.get(Number(o.streamId));
        if (!st) break;
        st.emit("data", Buffer.from(__b64dec(o.payload)));
        break;
      }
      case "end": {
        const o = JSON.parse(payload);
        const st = this.__streams.get(Number(o.streamId));
        if (!st) break;
        st.emit("end");
        st.close();
        this.__streams.delete(Number(o.streamId));
        break;
      }
      case "error": {
        const o = JSON.parse(payload);
        // 会话级错误（TCP 拒连/握手失败）载荷即 {code, msg}；流级错误包在 o.payload 内层
        const inner = o.payload === undefined ? o : JSON.parse(o.payload);
        const sid = o.streamId === undefined ? undefined : Number(o.streamId);
        const err = __h2Err(inner.code ?? "ERR_HTTP2_STREAM_ERROR", inner.msg ?? "");
        const st = sid !== undefined ? this.__streams.get(sid) : undefined;
        if (st && st.listenerCount("error") > 0) st.emit("error", err);
        else this.emit("error", err);
        break;
      }
      case "close":
        this.closed = true;
        this.emit("close");
        break;
    }
  }
  request(headers, options) {
    const h = { ...headers };
    if (h[":method"] === undefined) h[":method"] = "GET";
    if (h[":path"] === undefined) h[":path"] = "/";
    if (h[":authority"] === undefined) h[":authority"] = this.__authority;
    if (h[":scheme"] === undefined) h[":scheme"] = this.__secure ? "https" : "http";
    const id = this.__seq;
    this.__seq += 2; // 客户端单数流（RFC 7540 口径）
    const st = new ClientHttp2Stream(this, id, h, null);
    this.__streams.set(id, st);
    // 整收上传口径：end() 才下发（Node 也要求 end 收尾；http ClientRequest 同款）
    return st;
  }
  close(cb) {
    if (typeof cb === "function") this.once("close", cb);
    if (this.__id && !this.destroyed) {
      this.destroyed = true;
      __wjs_net_destroy(this.__id);
    }
    return this;
  }
  destroy() { return this.close(); }
  ref() { return this; }
  unref() { return this; }
}

export function createServer(options, listener) {
  const s = new Http2Server(options ?? {}, false);
  if (typeof options === "function") s.on("request", options);
  else if (typeof listener === "function") s.on("request", listener);
  return s;
}
export function createSecureServer(options, listener) {
  const s = new Http2Server(options ?? {}, true);
  if (typeof options === "function") s.on("request", options);
  else if (typeof listener === "function") s.on("request", listener);
  return s;
}
export function connect(authority, options, listener) {
  let url;
  if (typeof authority === "string") url = new URL(authority);
  else {
    url = new URL("http://127.0.0.1");
    options = authority ?? {};
  }
  if (typeof options === "function") { listener = options; options = {}; }
  const secure = url.protocol === "https:";
  const session = new ClientHttp2Session(url.host, { ...(options ?? {}), tls: secure ? (options ?? {}) : undefined });
  session.__secure = secure;
  if (typeof listener === "function") session.once("connect", listener);
  const port = url.port ? Number(url.port) : (secure ? 443 : 80);
  session.__start(port, url.hostname);
  return session;
}

export const constants = {
  NGHTTP2_NO_ERROR: 0, NGHTTP2_PROTOCOL_ERROR: 1, NGHTTP2_FLOW_CONTROL_ERROR: 3,
  NGHTTP2_SETTINGS_TIMEOUT: 4, NGHTTP2_STREAM_CLOSED: 5, NGHTTP2_REFUSED_STREAM: 7,
  NGHTTP2_CANCEL: 8,
  HTTP2_HEADER_STATUS: ":status", HTTP2_HEADER_METHOD: ":method",
  HTTP2_HEADER_PATH: ":path", HTTP2_HEADER_SCHEME: ":scheme",
  HTTP2_HEADER_AUTHORITY: ":authority",
  HTTP2_HEADER_CONTENT_LENGTH: "content-length", HTTP2_HEADER_CONTENT_TYPE: "content-type",
  HTTP2_METHOD_GET: "GET", HTTP2_METHOD_POST: "POST",
};
const __api = { createServer, createSecureServer, connect, constants };
export default __api;
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2_headers_roundtrip() {
        let mut map = http::HeaderMap::new();
        map.insert("content-type", "text/plain".parse().unwrap());
        map.insert("x-n", "v".parse().unwrap());
        let json = headers_json(&map);
        let back = parse_headers(&json);
        assert!(back.contains(&("content-type".into(), "text/plain".into())));
        assert!(back.contains(&("x-n".into(), "v".into())));
        // 非法头名跳过
        assert!(parse_headers(r#"[["bad name", "v"], ["ok", "1"]]"#) == vec![("ok".into(), "1".into())]);
    }

    #[test]
    fn h2_b64_roundtrip() {
        let data = b"\x00\xffbinary\x01probe".to_vec();
        assert_eq!(b64dec(&b64(&data)).unwrap(), data);
        assert!(b64dec("!!!not-b64!!!").is_err());
    }
}
