//! `node:http2`：compat 面 Server/Client（hyper `server`+`http2` 直引，§h2 拍板口径）。
//! 事件模型与 net 同构：Rust task → net 通道（`H2Request`/`H2Stream`/
//! `H2SessionClose` 变体，零新 channel）→ `dispatch` 调 target 预绑定的 `__ev`。
//! 连接驱动与 hyper conn future 同 task（`select!` 兼收 `H2Respond`/`H2Open`/
//! `Close` 命令）；服务端应答经 oneshot 回 service_fn。
//! 偏差记档：
//! - h2c 仅 prior-knowledge（无 Upgrade/h1c 回落）；`allowHTTP1` 不做。
//! - 体整收（server 收齐才发 request，client 收齐才发 data/end；http 记档同款）。
//! - 服务端推送（pushStream）、trailer、优先级/流控调参、ping/settings 细 knob 不做。
//!   （10b-4 triage：push 系 Web 已死特性——Chrome 106+ 移除，Safari/Firefox
//!   从未发货，为死特性做兼容无意义；trailer 等 h2 流式切片（整收下半 baked，
//!   另案）；h1 Upgrade: h2c 浏览器不用（只走 prior-knowledge），且本仓 h1
//!   upgrade 管线归 ws，另案。）
//! - `session.socket`/`alpnProtocol` 等反射面仅 `encrypted` 布尔；`getPeerCertificate` 不做。
//! - 状态码/头非法（JS 侧）→ 500 兜底（不断连）。

use std::collections::{HashMap, HashSet};
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

/// 单块请求/响应体（整收口径；10f 流式化后由 ChanBody 接管，类型保留供文档对照）。
#[allow(dead_code)]
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

/// 服务端应答头（oneshot 负载；10f 流式化：体与头分离）。
struct H2Resp {
    status: u16,
    headers: Vec<(String, String)>,
}

/// 体通道消息（10f 流式应答/上传共用）。
/// End = 体尽即 EOS（无/含 trailer）；EndPending = 体尽但流不闭（等 trailer 命令）。
enum BodyMsg {
    Data(bytes::Bytes),
    End(Option<Vec<(String, String)>>),
    EndPending,
    Fail(String),
}

type Responders = Arc<tokio::sync::Mutex<HashMap<u64, tokio::sync::oneshot::Sender<H2Resp>>>>;
type BodyFeeds = Arc<tokio::sync::Mutex<HashMap<u64, tokio::sync::mpsc::UnboundedSender<BodyMsg>>>>;
/// 早到的体块暂存（头未应答前 data/end 先到；头应答时按序回放）。
type Pendings = Arc<tokio::sync::Mutex<HashMap<u64, Vec<BodyMsg>>>>;
/// 已显式 RST 的流（service 回 Err → hyper RST INTERNAL_ERROR）。
type DeadStreams = Arc<std::sync::Mutex<HashSet<u64>>>;
/// 已收尾（end 发出）的流——连接退出时不再报 aborted。
type EndedStreams = Arc<std::sync::Mutex<HashSet<u64>>>;

/// mpsc 供体 body（hyper `http_body::Body` 手写 30 行）。
/// 单任务 select 轮询：body Pending 不需 waker（cmd 臂推进后 conn 臂重轮询）。
struct ChanBody {
    rx: tokio::sync::mpsc::UnboundedReceiver<BodyMsg>,
    done: bool,
}

impl http_body::Body for ChanBody {
    type Data = bytes::Bytes;
    type Error = std::io::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        if self.done {
            return Poll::Ready(None);
        }
        // waker 关键：经 poll_recv 注册，体块后到即唤醒重轮询；
        // try_recv 空转 Pending 会饿死流式应答（首版坑）。
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(BodyMsg::Data(b))) => Poll::Ready(Some(Ok(http_body::Frame::data(b)))),
            Poll::Ready(Some(BodyMsg::End(trailers))) => {
                self.done = true;
                if let Some(t) = trailers {
                    let mut map = http::HeaderMap::new();
                    for (k, v) in t {
                        if let (Ok(name), Ok(val)) = (
                            http::header::HeaderName::from_bytes(k.as_bytes()),
                            http::header::HeaderValue::from_str(&v),
                        ) {
                            map.append(name, val);
                        }
                    }
                    if !map.is_empty() {
                        return Poll::Ready(Some(Ok(http_body::Frame::trailers(map))));
                    }
                }
                Poll::Ready(None)
            }
            Poll::Ready(Some(BodyMsg::EndPending)) => Poll::Pending,
            Poll::Ready(Some(BodyMsg::Fail(msg))) => Poll::Ready(Some(Err(std::io::Error::other(msg)))),
            Poll::Ready(None) => {
                self.done = true;
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.done
    }
}

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

/// 读全请求体（整收口径；trailer 一并收集。错即 Err）。
async fn read_body(body: hyper::body::Incoming) -> Result<(Vec<u8>, Vec<(String, String)>), String> {
    use http_body::Body as _;
    use std::future::poll_fn;
    let mut body = body;
    let mut out = Vec::new();
    let mut trailers: Vec<(String, String)> = Vec::new();
    loop {
        match poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await {
            None => return Ok((out, trailers)),
            Some(Ok(f)) => {
                if let Some(d) = f.data_ref() {
                    out.extend_from_slice(d);
                }
                if let Some(t) = f.trailers_ref() {
                    for (k, v) in t.iter() {
                        trailers.push((
                            k.as_str().to_owned(),
                            String::from_utf8_lossy(v.as_bytes()).into_owned(),
                        ));
                    }
                }
            }
            Some(Err(e)) => return Err(format!("H2 stream error: {e}")),
        }
    }
}

/// 头应答后向体通道注入一条消息（未应答则暂存 outbox）。
async fn feed_body(
    stream_id: u64,
    msg: BodyMsg,
    bodies: &BodyFeeds,
    pendings: &Pendings,
) {
    // Sender 可 Clone：先取克隆再发，避免 msg 双 move。
    let tx_opt = { bodies.lock().await.get(&stream_id).cloned() };
    if let Some(tx) = tx_opt {
        let _ = tx.send(msg);
    } else {
        pendings.lock().await.entry(stream_id).or_default().push(msg);
    }
}

/// 单连接驱动（`select!` 兼收应答命令；结束即静默 purge entry）。
/// 10f 流式化：H2Respond 发头（oneshot），体经 data/end 命令增量喂 ChanBody；
/// 连接退出时对未收尾流补发 aborted 事件（§4.52：先静默子域再终结）。
async fn serve_conn<IO>(
    io: IO,
    server_id: u64,
    conn_id: u64,
    peer: std::net::SocketAddr,
    ev_tx: tokio::sync::mpsc::UnboundedSender<NetEvent>,
    mut cmd_rx: tokio::sync::mpsc::UnboundedReceiver<NetCmd>,
) where
    IO: hyper::rt::Read + hyper::rt::Write + Unpin + Send + 'static,
{
    let responders: Responders = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
    let bodies: BodyFeeds = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
    let pendings: Pendings = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
    let dead: DeadStreams = Arc::new(std::sync::Mutex::new(HashSet::new()));
    let dead_clean: DeadStreams = Arc::new(std::sync::Mutex::new(HashSet::new()));
    let ended: EndedStreams = Arc::new(std::sync::Mutex::new(HashSet::new()));
    let seq = Arc::new(std::sync::atomic::AtomicU64::new(1));
    let svc = {
        let responders = responders.clone();
        let bodies = bodies.clone();
        let pendings = pendings.clone();
        let dead = dead.clone();
        let dead_clean = dead_clean.clone();
        let seq = seq.clone();
        let ev_tx = ev_tx.clone();
        hyper::service::service_fn(move |req: http::Request<hyper::body::Incoming>| {
            let responders = responders.clone();
            let bodies = bodies.clone();
            let pendings = pendings.clone();
            let dead = dead.clone();
            let dead_clean = dead_clean.clone();
            let seq = seq.clone();
            let ev_tx = ev_tx.clone();
            async move {
                let stream_id = seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let method = req.method().to_string();
                let _is_connect = method == "CONNECT";
                let path = req
                    .uri()
                    .path_and_query()
                    .map(|pq| pq.to_string())
                    .unwrap_or_else(|| "/".into());
                // authority：hyper 只在客户端真发过 `:authority` 伪头时才有
                // （host-only 请求保持 host 常规头——host 回落套件依赖此区分）。
                let authority = req
                    .uri()
                    .authority()
                    .map(|a| a.as_str().to_owned())
                    .unwrap_or_default();
                let headers = headers_json(req.headers());
                let (body, trailers) = match read_body(req.into_body()).await {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = ev_tx.send(NetEvent {
                            id: server_id,
                            kind: NetKind::Error {
                                code: "ERR_HTTP2_STREAM_ERROR".into(),
                                msg: e,
                            },
                        });
                        (Vec::new(), Vec::new())
                    }
                };
                let trailers_json = serde_json::to_string(&trailers).unwrap_or_else(|_| "[]".into());
                let (tx, rx) = tokio::sync::oneshot::channel();
                responders.lock().await.insert(stream_id, tx);
                let _ = ev_tx.send(NetEvent {
                    id: server_id,
                    kind: NetKind::H2Request {
                        conn_id,
                        stream_id,
                        method,
                        path,
                        authority,
                        headers,
                        trailers_json,
                        body_b64: b64(&body),
                        peer: peer.to_string(),
                    },
                });
                // JS 不应答即挂起（记档）；reset 已发则 service 回 Err（hyper RST），
                // 干净关（NO_ERROR）回 200 空体（偏差记档：body API 无法 RST NO_ERROR）。
                let head = match rx.await {
                    Ok(r) => r,
                    Err(_) => {
                        let was_dead = dead.lock().unwrap().remove(&stream_id);
                        if was_dead {
                            return Err::<http::Response<ChanBody>, anyhow::Error>(anyhow::anyhow!(
                                "h2 stream reset"
                            ));
                        }
                        let _ = dead_clean.lock().unwrap().remove(&stream_id);
                        return Ok(http::Response::builder()
                            .status(200)
                            .body(ChanBody { rx: tokio::sync::mpsc::unbounded_channel().1, done: true })
                            .expect("200 fallback builds"));
                    }
                };
                let (btx, brx) = tokio::sync::mpsc::unbounded_channel::<BodyMsg>();
                bodies.lock().await.insert(stream_id, btx.clone());
                // 回放早到的体块（data/end 在头前到达的竞态）
                let early = pendings.lock().await.remove(&stream_id);
                if let Some(msgs) = early {
                    for m in msgs {
                        let _ = btx.send(m);
                    }
                }
                let mut builder = http::Response::builder().status(head.status);
                for (k, v) in &head.headers {
                    builder = builder.header(k, v);
                }
                Ok(builder
                    .body(ChanBody { rx: brx, done: false })
                    .unwrap_or_else(|_| {
                        http::Response::builder()
                            .status(500)
                            .body(ChanBody { rx: tokio::sync::mpsc::unbounded_channel().1, done: true })
                            .expect("500 fallback builds")
                    }))
            }
        })
    };
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
                    Some(NetCmd::H2Respond { stream_id, status, headers }) => {
                        let sender = responders.lock().await.remove(&stream_id);
                        if let Some(tx) = sender {
                            let _ = tx.send(H2Resp { status, headers: parse_headers(&headers) });
                        }
                    }
                    Some(NetCmd::H2RespondData { stream_id, data_b64 }) => {
                        let data = b64dec(&data_b64).unwrap_or_default();
                        feed_body(stream_id, BodyMsg::Data(bytes::Bytes::from(data)), &bodies, &pendings).await;
                    }
                    Some(NetCmd::H2RespondEnd { stream_id, trailers_json }) => {
                        let trailers: Vec<(String, String)> =
                            serde_json::from_str(&trailers_json).unwrap_or_default();
                        ended.lock().unwrap().insert(stream_id);
                        feed_body(stream_id, BodyMsg::End(Some(trailers)), &bodies, &pendings).await;
                    }
                    Some(NetCmd::H2RespondReset { stream_id, code }) => {
                        ended.lock().unwrap().insert(stream_id);
                        if code == 0 {
                            dead_clean.lock().unwrap().insert(stream_id);
                        } else {
                            dead.lock().unwrap().insert(stream_id);
                        }
                        // 头未应答：摘 oneshot 触发 service 分支；已应答：体通道注错/RST。
                        let sender = responders.lock().await.remove(&stream_id);
                        drop(sender);
                        if code == 0 {
                            feed_body(stream_id, BodyMsg::End(None), &bodies, &pendings).await;
                        } else {
                            feed_body(
                                stream_id,
                                BodyMsg::Fail("h2 stream reset".into()),
                                &bodies,
                                &pendings,
                            )
                            .await;
                        }
                    }
                    Some(NetCmd::Close) | None => break,
                    _ => {}
                }
            }
        }
    }
    // 连接退出：未收尾的流对 server 补发 aborted（target 在 server close 前仍在，
    // 时序竞态由 JS 侧 server close 处理器兜底）。
    // std MutexGuard 非 Send：先拷贝快照再 await，禁跨 await 持锁。
    let outstanding: Vec<u64> = {
        let ended_ids: HashSet<u64> = { ended.lock().unwrap().iter().cloned().collect() };
        let mut ids: Vec<u64> = Vec::new();
        for id in responders.lock().await.keys() {
            if !ended_ids.contains(id) {
                ids.push(*id);
            }
        }
        let bodies = bodies.lock().await;
        for id in bodies.keys() {
            if !ended_ids.contains(id) {
                ids.push(*id);
            }
        }
        ids.sort_unstable();
        ids.dedup();
        ids
    };
    for stream_id in outstanding {
        let _ = ev_tx.send(NetEvent {
            id: server_id,
            kind: NetKind::H2Stream {
                stream_id,
                what: "aborted".into(),
                payload: String::new(),
            },
        });
    }
    // 会话终结通知（借 H2Stream 通道：what="connClose"，payload 带 conn_id；
    // JS 侧 server 收到后收尾对应 Http2Session 并发 'close'）。
    let _ = ev_tx.send(NetEvent {
        id: server_id,
        kind: NetKind::H2Stream {
            stream_id: 0,
            what: "connClose".into(),
            payload: conn_id.to_string(),
        },
    });
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
                    let Ok((stream, peer)) = acc else { continue };
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
                                    serve_conn(TokioIo::new(tls), id, conn_id, peer, ev2, conn_cmd_rx).await;
                                    let _ = done2.send(conn_id);
                                }
                            }
                        };
                        tokio::spawn(fut);
                    } else {
                        let fut = async move {
                            serve_conn(TokioIo::new(stream), id, conn_id, peer, ev2, conn_cmd_rx).await;
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

/// 开流请求 JSON：`{method, path, scheme, authority, waitTrailers, headers: [[k,v]]}`。
#[derive(Debug, serde::Deserialize)]
struct OpenReq {
    method: String,
    #[serde(default)]
    path: String,
    #[serde(default = "default_scheme")]
    scheme: String,
    #[serde(default)]
    authority: String,
    #[serde(rename = "waitTrailers", default)]
    wait_trailers: bool,
    #[serde(default)]
    headers: Vec<Vec<String>>,
}

fn default_scheme() -> String {
    "http".into()
}

/// 客户端 session 驱动（conn 与命令同 task `select!`；终结即 Close 事件）。
/// 10f：response 事件带真 flags（END_STREAM→5）、trailers 事件、session 终结时
/// 未完结流补 aborted（先于 close，§4.52 顺序）。
async fn drive_session<S>(
    io: TokioIo<S>,
    id: u64,
    ev_tx: tokio::sync::mpsc::UnboundedSender<NetEvent>,
    mut cmd_rx: tokio::sync::mpsc::UnboundedReceiver<NetCmd>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let hs = hyper::client::conn::http2::Builder::new(Exec).handshake::<_, ChanBody>(io);
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
    // 开流登记：stream_id → 响应体是否已完结（session 死时未完结者报 aborted）。
    let open: Arc<std::sync::Mutex<HashMap<u64, bool>>> =
        Arc::new(std::sync::Mutex::new(HashMap::new()));
    // 上传体通道（waitTrailers 悬置流；trailer 命令回注）。
    let uploads: BodyFeeds = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
    tokio::pin!(conn);
    loop {
        tokio::select! {
            r = &mut conn => {
                let _ = r;
                if state::net_close_once(id) {
                    for (stream_id, done) in open.lock().unwrap().iter() {
                        if !*done {
                            let _ = ev_tx.send(NetEvent {
                                id,
                                kind: NetKind::H2Stream {
                                    stream_id: *stream_id,
                                    what: "aborted".into(),
                                    payload: String::new(),
                                },
                            });
                        }
                    }
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
                        let parsed_headers =
                            parse_headers(&serde_json::to_string(&req.headers).unwrap_or_default());
                        // :authority 经完整 URI 带（hyper h2 客户端不合成伪头）；
                        // authority 为空回落 host 头（host-only 请求，host 回落套件）。
                        let authority = if req.authority.is_empty() {
                            parsed_headers
                                .iter()
                                .find(|(k, _)| k == "host")
                                .map(|(_, v)| v.clone())
                                .unwrap_or_default()
                        } else {
                            req.authority.clone()
                        };
                        let uri = format!("{}://{}{}", req.scheme, authority, req.path);
                        let mut builder = http::Request::builder()
                            .method(req.method.as_str())
                            .uri(uri.as_str());
                        for (k, v) in parsed_headers {
                            builder = builder.header(k.as_str(), v.as_str());
                        }
                        // 请求体经 ChanBody：空体 done=true → is_end_stream → END_STREAM
                        // 随头出线（GET 系即时开流）；非体先 Data 后 End；
                        // waitTrailers 悬置 EndPending 等 trailer 命令再闭流。
                        let (utx, urx) = tokio::sync::mpsc::unbounded_channel::<BodyMsg>();
                        // done=true 仅空体无 trailer（END_STREAM 随头）；有体即 false，
                        // 否则 poll_frame 首轮即 None，Data 永不到线。
                        let mut done = true;
                        if !body.is_empty() {
                            let _ = utx.send(BodyMsg::Data(bytes::Bytes::from(body)));
                            done = false;
                            if req.wait_trailers {
                                let _ = utx.send(BodyMsg::EndPending);
                            } else {
                                let _ = utx.send(BodyMsg::End(None));
                            }
                        } else if req.wait_trailers {
                            let _ = utx.send(BodyMsg::EndPending);
                            done = false;
                        }
                        if req.wait_trailers {
                            uploads.lock().await.insert(stream_id, utx);
                        }
                        let request = builder.body(ChanBody { rx: urx, done });
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
                        let open2 = open.clone();
                        open.lock().unwrap().insert(stream_id, false);
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
                                    // flags：hyper Incoming 无 END_STREAM 预判，一律 4；
                                    // 空体仍走 data(无)→end，JS 侧 `?? 4` 兜底一致。
                                    let flags = 4;
                                    let _ = ev2.send(NetEvent {
                                        id,
                                        kind: NetKind::H2Stream {
                                            stream_id,
                                            what: "response".into(),
                                            payload: serde_json::json!({ "status": status, "flags": flags, "headers": head }).to_string(),
                                        },
                                    });
                                    match read_body(resp.into_body()).await {
                                        Ok((b, trailers)) => {
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
                                            if !trailers.is_empty() {
                                                let _ = ev2.send(NetEvent {
                                                    id,
                                                    kind: NetKind::H2Stream {
                                                        stream_id,
                                                        what: "trailers".into(),
                                                        payload: serde_json::to_string(&trailers)
                                                            .unwrap_or_else(|_| "[]".into()),
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
                                            open2.lock().unwrap().insert(stream_id, true);
                                        }
                                        Err(e) => {
                                            // read_body 回 String（无 h2 Reason 携带）：
                                            // NO_ERROR 干净收尾按子串判，其余按流错误上报。
                                            if e.contains("NO_ERROR") {
                                                let _ = ev2.send(NetEvent {
                                                    id,
                                                    kind: NetKind::H2Stream {
                                                        stream_id,
                                                        what: "end".into(),
                                                        payload: String::new(),
                                                    },
                                                });
                                                open2.lock().unwrap().insert(stream_id, true);
                                            } else {
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
                                                open2.lock().unwrap().insert(stream_id, true);
                                            }
                                        }
                                    }
                                }
                            }
                        });
                    }
                    Some(NetCmd::H2OpenTrailers { stream_id, trailers_json }) => {
                        let trailers: Vec<(String, String)> =
                            serde_json::from_str(&trailers_json).unwrap_or_default();
                        let tx = uploads.lock().await.remove(&stream_id);
                        if let Some(tx) = tx {
                            let _ = tx.send(BodyMsg::End(Some(trailers)));
                        }
                    }
                    Some(NetCmd::Close) | None => {
                        if state::net_close_once(id) {
                            for (stream_id, done) in open.lock().unwrap().iter() {
                                if !*done {
                                    let _ = ev_tx.send(NetEvent {
                                        id,
                                        kind: NetKind::H2Stream {
                                            stream_id: *stream_id,
                                            what: "aborted".into(),
                                            payload: String::new(),
                                        },
                                    });
                                }
                            }
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

/// `__wjs_h2_open_trailers(sessionId, streamId, trailersJson)`——上传 trailer 帧。
pub unsafe extern "C" fn h2_open_trailers(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(sid), Some(stid)) = (opt_num(&frame, 0), opt_num(&frame, 1)) else {
        report_error(&mut cx, "TypeError: h2 open trailers: ids must be numbers");
        return false;
    };
    let trailers_json = value_to_string(&mut cx, frame.arg(2));
    let _ = state::net_cmd(
        sid as u64,
        NetCmd::H2OpenTrailers { stream_id: stid as u64, trailers_json },
    );
    true
}

/// `__wjs_h2_respond(connId, streamId, status, headersJson)`——应答头（10f 流式化：
/// 体经 `__wjs_h2_data`/`__wjs_h2_end` 增量下发；status 0 表无头直 RST）。
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
    // 连接已收尾即静默 true（竞态：客户端先走；Node 口径不抛）
    let _ = state::net_cmd(
        cid as u64,
        NetCmd::H2Respond { stream_id: stid as u64, status: status as u16, headers },
    );
    true
}

/// `__wjs_h2_data(connId, streamId, dataB64)`——应答体块。
pub unsafe extern "C" fn h2_data(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(cid), Some(stid)) = (opt_num(&frame, 0), opt_num(&frame, 1)) else {
        report_error(&mut cx, "TypeError: h2 data: ids must be numbers");
        return false;
    };
    let data_b64 = value_to_string(&mut cx, frame.arg(2));
    let _ = state::net_cmd(cid as u64, NetCmd::H2RespondData { stream_id: stid as u64, data_b64 });
    true
}

/// `__wjs_h2_end(connId, streamId, trailersJson)`——应答收尾（trailer 可空）。
pub unsafe extern "C" fn h2_end(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(cid), Some(stid)) = (opt_num(&frame, 0), opt_num(&frame, 1)) else {
        report_error(&mut cx, "TypeError: h2 end: ids must be numbers");
        return false;
    };
    let trailers_json = value_to_string(&mut cx, frame.arg(2));
    let _ = state::net_cmd(
        cid as u64,
        NetCmd::H2RespondEnd { stream_id: stid as u64, trailers_json },
    );
    true
}

/// `__wjs_h2_reset(connId, streamId, code)`——RST_STREAM（0=NO_ERROR 干净，2=INTERNAL）。
pub unsafe extern "C" fn h2_reset(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(cid), Some(stid), Some(code)) = (opt_num(&frame, 0), opt_num(&frame, 1), opt_num(&frame, 2))
    else {
        report_error(&mut cx, "TypeError: h2 reset: ids/code must be numbers");
        return false;
    };
    let _ = state::net_cmd(cid as u64, NetCmd::H2RespondReset { stream_id: stid as u64, code: code as u32 });
    true
}

/// 内嵌 ESM 源（`node:http2`；compat 子集，见头注偏差）。
pub const SOURCE: &str = r#"
import { EventEmitter } from "node:events";
import { Readable, Writable, Duplex } from "node:stream";
import { codes } from "node:internal/errors";
import * as net from "node:net";
import * as fs from "node:fs";
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
function __h2Err(code, msg, name = "Error") {
  const e = new Error(msg);
  e.code = code;
  e.name = name;
  return e;
}
// http2 专属错误码（errors.rs 内核表外补；消息逐字对齐 node/lib/internal/errors.js）
const __codes = {
  ERR_HTTP2_STATUS_INVALID: (s) => __h2Err("ERR_HTTP2_STATUS_INVALID", `Invalid status code: ${s}`),
  ERR_HTTP2_INVALID_INFO_STATUS: (s) => __h2Err("ERR_HTTP2_INVALID_INFO_STATUS", `Invalid informational status code: ${s}`),
  ERR_HTTP2_INVALID_PSEUDOHEADER: (s) => __h2Err("ERR_HTTP2_INVALID_PSEUDOHEADER", `"${s}" is an invalid pseudoheader or is used incorrectly`, "TypeError"),
  ERR_HTTP2_PSEUDOHEADER_NOT_ALLOWED: () => __h2Err("ERR_HTTP2_PSEUDOHEADER_NOT_ALLOWED", "Cannot set HTTP/2 pseudo headers after regular headers", "TypeError"),
  ERR_HTTP2_HEADER_SINGLE_VALUE: (s) => __h2Err("ERR_HTTP2_HEADER_SINGLE_VALUE", `Header field "${s}" must only have a single value`, "TypeError"),
  ERR_HTTP2_TRAILERS_CANNOT_BE_SENT: () => __h2Err("ERR_HTTP2_TRAILERS_CANNOT_BE_SENT", "Trailers cannot be sent at this stage."),
  ERR_HTTP2_TRAILERS_ALREADY_SENT: () => __h2Err("ERR_HTTP2_TRAILERS_ALREADY_SENT", "Trailers has already been sent."),
  ERR_HTTP2_PUSH_DISABLED: () => __h2Err("ERR_HTTP2_PUSH_DISABLED", "Push streams are not enabled on this session."),
  ERR_HTTP2_NESTED_PUSH: () => __h2Err("ERR_HTTP2_NESTED_PUSH", "A push stream cannot be initiated from within a push stream."),
  ERR_HTTP2_GOAWAY_SESSION: () => __h2Err("ERR_HTTP2_GOAWAY_SESSION", "New streams cannot be created after receiving a GOAWAY."),
  ERR_HTTP2_SESSION_ERROR: (n) => __h2Err("ERR_HTTP2_SESSION_ERROR", `Session closed with error code ${n}`),
  ERR_HTTP2_MAX_PENDING_SETTINGS_ACK: () => __h2Err("ERR_HTTP2_MAX_PENDING_SETTINGS_ACK", "Maximum concurrent SETTINGS frames not acknowledged"),
  ERR_HTTP2_INVALID_SETTING_VALUE: (name, v) => __h2Err("ERR_HTTP2_INVALID_SETTING_VALUE", `Invalid value for setting "${name}": ${v}`, "RangeError"),
  ERR_HTTP2_PAYLOAD_FORBIDDEN: (s) => __h2Err("ERR_HTTP2_PAYLOAD_FORBIDDEN", `Responses with ${s} status must not have a payload`),
  ERR_HTTP2_NO_PAYLOAD: () => __h2Err("ERR_HTTP2_NO_PAYLOAD", "No payload supplied"),
  ERR_HTTP2_INVALID_PACKED_SETTINGS_LENGTH: () => __h2Err("ERR_HTTP2_INVALID_PACKED_SETTINGS_LENGTH", "Packed settings length must be a multiple of six"),
  ERR_HTTP2_INVALID_PROTOCOL: (v, a) => __h2Err("ERR_HTTP2_INVALID_PROTOCOL", `Protocol "${v}" does not contain "${a}"`),
  ERR_HTTP2_SEND_FILE: () => __h2Err("ERR_HTTP2_SEND_FILE", "Filename passed to sendFile must be absolute"),
  ERR_HTTP2_SEND_FILE_NOSEEK: () => __h2Err("ERR_HTTP2_SEND_FILE_NOSEEK", "Offset or length can only be specified for regular files"),
  ERR_HTTP2_PING_CANCEL: () => __h2Err("ERR_HTTP2_PING_CANCEL", "Ping canceled"),
  ERR_HTTP2_TOO_MANY_INVALID_FRAMES: (s) => __h2Err("ERR_HTTP2_TOO_MANY_INVALID_FRAMES", `Too many invalid HTTP/2 frames: ${s}`),
  ERR_HTTP2_FRAME_ERROR: (s) => __h2Err("ERR_HTTP2_FRAME_ERROR", `HTTP/2 frame error: ${s}`),
  ERR_HTTP2_STREAM_CLOSED: () => __h2Err("ERR_HTTP2_STREAM_CLOSED", "The stream has been destroyed"),
  ERR_HTTP2_INVALID_SESSION: () => __h2Err("ERR_HTTP2_INVALID_SESSION", "The session has been destroyed"),
  ERR_HTTP2_SOCKET_UNBOUND: () => __h2Err("ERR_HTTP2_SOCKET_UNBOUND", "The socket has been unbound from the session."),
  ERR_HTTP2_OUT_OF_BUFFERS: () => __h2Err("ERR_HTTP2_OUT_OF_BUFFERS", "Out of buffers"),
  ERR_HTTP2_HEADERS_OBJECT: () => __h2Err("ERR_HTTP2_HEADERS_OBJECT", "Headers must be an object"),
  ERR_HTTP2_HEADERS_AFTER_RESPOND: () => __h2Err("ERR_HTTP2_HEADERS_AFTER_RESPOND", "Cannot specify additional headers after response has initiated"),
  ERR_HTTP2_INVALID_HEADER_VALUE: (v, n) => __h2Err("ERR_HTTP2_INVALID_HEADER_VALUE", `Invalid header value: "${v}" for header "${n}"`),
  ERR_HTTP2_NO_SOCKET_MANIPULATION: () => __h2Err("ERR_HTTP2_NO_SOCKET_MANIPULATION", "HTTP/2 sockets should not be directly manipulated (e.g. read and written)"),
  ERR_HTTP2_STREAM_SELF_DEPENDENCY: () => __h2Err("ERR_HTTP2_STREAM_SELF_DEPENDENCY", "A stream cannot depend on itself"),
  ERR_HTTP2_INVALID_STREAM: () => __h2Err("ERR_HTTP2_INVALID_STREAM", "The stream has been destroyed"),
  ERR_HTTP2_HEADERS_SENT: () => __h2Err("ERR_HTTP2_HEADERS_SENT", "Response has already been initiated."),
  ERR_HTTP2_STREAM_CANCEL: (s) => __h2Err("ERR_HTTP2_STREAM_CANCEL", typeof s === "string" && s ? s : "The stream was aborted"),
};
function __code(name, ...args) {
  const f = codes[name];
  // E() 工厂是 class（必须 new；返回对象的工厂 new 后同样取返回对象）
  if (typeof f === "function") { try { return new f(...args); } catch { /* fallthrough */ } }
  const g = __codes[name];
  if (typeof g === "function") return g(...args);
  return __h2Err(name, args.length ? String(args[0]) : name);
}
function __pairsToObj(pairs) {
  const out = Object.create(null);
  for (const [k, v] of pairs) {
    const lk = String(k).toLowerCase();
    out[lk] = out[lk] === undefined ? String(v) : `${out[lk]}, ${v}`;
  }
  return out;
}
// node checkIsHttpToken 同款（header 名校验）
const __TOKEN_RE = /^[\^_`a-zA-Z\-0-9!#$%&'*+.|~]+$/;
function __validateHeaderName(name) {
  if (typeof name !== "string") {
    throw __code("ERR_INVALID_ARG_TYPE", "name", "string", name);
  }
  if (!__TOKEN_RE.test(name)) {
    throw __code("ERR_INVALID_HTTP_TOKEN", name);
  }
}
function __validateHeaderValue(name, value) {
  if (value === undefined || value === null) {
    throw __code("ERR_HTTP2_INVALID_HEADER_VALUE", value, name);
  }
  if (Array.isArray(value)) {
    for (const v of value) __validateHeaderValue(name, v);
    return;
  }
  if (typeof value !== "string" && typeof value !== "number" && typeof value !== "boolean") {
    throw __code("ERR_INVALID_ARG_TYPE", `header "${name}"`, "string|number|boolean", value);
  }
}
function __headerToWire(value) {
  if (Array.isArray(value)) return value.map((v) => String(v));
  return String(value);
}

// ── internal/http2/util 同款校验（套件直接可对齐；10g 欠账 G10）──────────────
const __PSEUDO_RE = /^:[a-zA-Z0-9_]+$/;
function assertValidPseudoHeader(key) {
  if (!__PSEUDO_RE.test(key)) throw __code("ERR_HTTP2_INVALID_PSEUDOHEADER", key);
}
function assertIsObject(val, name, type) {
  if (val === undefined || val === null || (typeof val !== "object" && typeof val !== "function") || Array.isArray(val)) {
    throw __code("ERR_INVALID_ARG_TYPE", name, type ?? "Object", val);
  }
}
function assertWithinRange(name, value, min, max) {
  if (typeof value !== "number" || !Number.isInteger(value) || value < min || value > max) {
    throw __code("ERR_HTTP2_INVALID_SETTING_VALUE", name, value);
  }
}
// connection-specific 头（RFC 7540 §8.1.2.2，node isConnectionSpecificHeader 同表）
const __CONN_HEADERS = new Set(["connection", "upgrade", "http2-settings", "te", "transfer-encoding", "keep-alive", "proxy-connection"]);
// node validateH2Headers：伪头白名单（respond 只允许 :status；trailers 全禁）+ 单值
function __validateH2Headers(headers, allowedPseudo = []) {
  if (headers === null || typeof headers !== "object") throw __code("ERR_HTTP2_HEADERS_OBJECT");
  for (const key of Object.keys(headers)) {
    const lk = String(key).toLowerCase();
    if (lk.startsWith(":")) {
      if (!allowedPseudo.includes(lk)) throw __code("ERR_HTTP2_INVALID_PSEUDOHEADER", lk);
      const v = headers[key];
      if (Array.isArray(v)) {
        if (v.length !== 1) throw __code("ERR_HTTP2_HEADER_SINGLE_VALUE", key);
      } else if (v === undefined) {
        throw __code("ERR_HTTP2_INVALID_HEADER_VALUE", v, key);
      }
    } else {
      __validateHeaderName(String(key));
      __validateHeaderValue(String(key), headers[key]);
    }
  }
}
// 设置项取值/校验（node updateSettingsInternal 同口径；customSettings 放行）
const __SETTING_RANGES = {
  headerTableSize: [0, 0xffffffff],
  enablePush: "boolean",
  initialWindowSize: [0, 0xffffffff],
  maxFrameSize: [16384, 16777215],
  maxConcurrentStreams: [0, 0xffffffff],
  maxHeaderListSize: [0, 0xffffffff],
  maxHeaderSize: [0, 0xffffffff],
  enableConnectProtocol: "boolean",
};
function __validateSettings(settings) {
  if (settings === null || typeof settings !== "object") {
    throw __code("ERR_INVALID_ARG_TYPE", "settings", "object", settings);
  }
  const out = {};
  for (const key of Object.keys(settings)) {
    const v = settings[key];
    const spec = __SETTING_RANGES[key];
    if (spec === undefined) {
      if (key === "customSettings") {
        out[key] = { ...v };
        continue;
      }
      continue; // 未知设置项忽略（node 静默忽略未知名——non-critical）
    }
    if (spec === "boolean") {
      if (typeof v !== "boolean") throw __code("ERR_HTTP2_INVALID_SETTING_VALUE", key, v);
      out[key] = v;
    } else if (typeof v !== "number" || !Number.isInteger(v) || v < spec[0] || v > spec[1]) {
      throw __code("ERR_HTTP2_INVALID_SETTING_VALUE", key, v);
    } else {
      out[key] = v;
    }
  }
  return out;
}
function __applySettings(base, extra) {
  return { ...base, ...extra };
}
const __DEFAULT_SETTINGS = {
  headerTableSize: 4096,
  enablePush: true,
  initialWindowSize: 65535,
  maxFrameSize: 16384,
  maxConcurrentStreams: 4294967295,
  maxHeaderSize: 65535,
  maxHeaderListSize: 65535,
  enableConnectProtocol: false,
};

// ── socket 代理（node socketProxyPair 口径：net.Socket 假面 + 流委派）────────
const __SOCK_MANIP_KEYS = ["read", "write", "pause", "resume"];
function __socketManipErr() {
  return __code("ERR_HTTP2_NO_SOCKET_MANIPULATION");
}
function __mkSocketProxy(stream, server, peerObj) {
  const base = Object.create(net.Socket.prototype);
  Object.defineProperty(base, "connecting", { value: false, writable: true, configurable: true });
  const handler = {
    get(t, prop) {
      if (prop === "readable" || prop === "writable") return stream[prop];
      if (prop === "destroyed") return stream.__destroyed;
      if (__SOCK_MANIP_KEYS.includes(prop)) throw __socketManipErr();
      if (prop === "on" || prop === "once" || prop === "emit" ||
          prop === "end") {
        return stream[prop].bind(stream);
      }
      if (prop === "destroy") {
        // node：socket.destroy() 杀整个连接（stream.session.destroy 同收尾）
        return (...args) => {
          stream.destroy(...args);
          stream.session.destroy();
        };
      }
      if (prop === "setTimeout") return stream.session.setTimeout.bind(stream.session);
      if (prop === "address") return () => server.address();
      if (prop === "remoteAddress") return peerObj.addr;
      if (prop === "remotePort") return peerObj.port;
      if (prop === "localAddress") return server.__listening ? server.__listening.address : undefined;
      if (prop === "localPort") return server.__listening ? server.__listening.port : undefined;
      if (prop === "_server") return server;
      const v = Reflect.get(t, prop);
      if (v !== undefined || prop in t) return v;
      return server.__sockBag[prop];
    },
    set(t, prop, value) {
      if (prop === "readable") { stream.readable = !!value; return true; }
      if (prop === "writable") { stream.writable = !!value; return true; }
      if (prop === "destroyed") { stream.__destroyed = !!value; return true; }
      if (__SOCK_MANIP_KEYS.includes(prop)) throw __socketManipErr();
      if (prop === "on" || prop === "once" || prop === "emit" ||
          prop === "end" || prop === "destroy") {
        stream[prop] = value;
        return true;
      }

      if (prop === "setTimeout") { stream.session.setTimeout = value; return true; }
      t[prop] = value;
      server.__sockBag[prop] = value;
      return true;
    },
  };
  return new Proxy(base, handler);
}

// ── 会话级 socket（session.socket：EventEmitter 假面 + peer 反射）────────────
function __mkSessionSocket(session, peerObj, localObj) {
  const sock = new EventEmitter();
  sock.remoteAddress = peerObj.addr;
  sock.remotePort = peerObj.port;
  sock.localAddress = localObj?.address;
  sock.localPort = localObj?.port;
  sock.remoteFamily = peerObj.addr?.includes(":") ? "IPv6" : "IPv4";
  sock.connecting = false;
  sock.destroyed = false;
  sock.readable = true;
  sock.writable = true;
  sock.destroy = (err) => {
    if (sock.destroyed) return;
    sock.destroyed = true;
    if (err) sock.emit("error", err);
    session.destroy();
  };
  sock.end = () => { session.close(); return sock; };
  sock.write = () => true;
  setTimeout(() => { if (!session.destroyed) sock.emit("connect"); }, 0);
  return sock;
}

// ── Http2Session（服务端每连接一个；'session' 事件载体）──────────────────────
let __h2SessionSeq = 1;
class Http2Session extends EventEmitter {
  constructor(server, connId, peerObj) {
    super();
    this.__id = __h2SessionSeq++;
    this.__server = server;
    this.__conn = connId;
    this.type = 0; // NGHTTP2_SESSION_SERVER
    this.encrypted = !!server.__secure;
    this.connecting = false;
    this.destroyed = false;
    this.closed = false;
    this.__peer = peerObj ?? { addr: undefined, port: 0 };
    this.__settings = { ...__DEFAULT_SETTINGS, ...(server.__opts?.settings ?? {}) };
    this.__remoteSettings = { ...__DEFAULT_SETTINGS };
    this.__pendingSettingsAck = false;
    this.__outstandingSettings = 0;
    this.__maxOutstandingSettings = server.__opts?.maxOutstandingSettings ?? Infinity;
    this.__streams = new Map();
    this.state = {
      effectiveLocalWindowSize: 65535,
      effectiveRemoteWindowSize: 65535,
      localWindowSize: 65535,
      remoteWindowSize: 65535,
      outboundQueueSize: 0,
      deflateDynamicTableSize: 4096,
      inflateDynamicTableSize: 4096,
    };
    this.__ev = this.__ev.bind(this);
  }
  get socket() {
    if (this.__socket === undefined) {
      this.__socket = __mkSessionSocket(this, this.__peer, this.__server.__listening);
    }
    return this.__socket;
  }
  get localSettings() { return this.__settings; }
  get remoteSettings() { return this.__remoteSettings; }
  get pendingSettingsAck() { return this.__pendingSettingsAck; }
  get originSet() { return this.encrypted ? undefined : undefined; }
  get alpnProtocol() { return this.encrypted ? "h2" : false; }
  get unrefed() { return false; }
  setTimeout(msecs, callback) {
    if (typeof callback === "function") this.once("timeout", callback);
    const ms = Number(msecs) || 0;
    if (this.__timeoutTimer !== undefined) clearTimeout(this.__timeoutTimer);
    if (ms > 0 && !this.closed && !this.destroyed) {
      this.__timeoutTimer = setTimeout(() => {
        this.__timeoutTimer = undefined;
        this.emit("timeout");
      }, ms);
    }
    return this;
  }
  ref() { if (this.__conn) __wjs_net_ref(this.__conn); return this; }
  unref() { if (this.__conn) __wjs_net_unref(this.__conn); return this; }
  settings(settings, cb) {
    const validated = __validateSettings(settings);
    if (this.destroyed) throw __code("ERR_HTTP2_INVALID_SESSION");
    this.__pendingSettingsAck = true;
    this.__outstandingSettings++;
    if (Number.isFinite(this.__maxOutstandingSettings) &&
        this.__outstandingSettings >= this.__maxOutstandingSettings) {
      this.__outstandingSettings = 0;
      this.__pendingSettingsAck = false;
      process.nextTick(() => {
        this.emit("error", __code("ERR_HTTP2_MAX_PENDING_SETTINGS_ACK"));
        this.destroy();
      });
      return this;
    }
    setTimeout(() => {
      this.__outstandingSettings = Math.max(0, this.__outstandingSettings - 1);
      if (this.__outstandingSettings === 0) this.__pendingSettingsAck = false;
      this.__settings = __applySettings(this.__settings, validated);
      if (!this.destroyed) {
        this.emit("localSettings", this.__settings);
        if (typeof cb === "function") cb();
      }
    }, 1);
    return this;
  }
  updateSettings(settings) {
    const validated = __validateSettings(settings);
    this.__settings = __applySettings(this.__settings, validated);
    return this;
  }
  ping(cb, payload) {
    if (this.destroyed) throw __code("ERR_HTTP2_INVALID_SESSION");
    let buf = null;
    if (payload !== undefined && payload !== null) {
      const u8 = payload instanceof Uint8Array ? payload : __toU8(payload, "ping");
      if (u8.length > 8) throw __code("ERR_OUT_OF_RANGE", "payload", u8.length);
      buf = u8;
    }
    if (typeof cb !== "function") {
      throw __code("ERR_INVALID_ARG_TYPE", "callback", "function", cb);
    }
    const ret = Buffer.alloc(8);
    if (buf) Buffer.from(buf).copy(ret);
    setTimeout(() => {
      if (this.destroyed) { cb(__code("ERR_HTTP2_PING_CANCEL")); return; }
      cb(null, 0.5, ret);
    }, 1);
    return true;
  }
  goaway(code = 0, lastStreamID = 0, opaqueData) {
    if (this.destroyed) throw __code("ERR_HTTP2_INVALID_SESSION");
    if (typeof code === "object" && code !== null) {
      // goaway(options) 形（node：{errorCode, lastStreamID, opaqueData}）
      const o = code;
      code = o.errorCode ?? 0;
      lastStreamID = o.lastStreamID ?? 0;
      opaqueData = o.opaqueData;
    }
    this.__goaway = { code, lastStreamID, opaqueData };
    // 底座无 GOAWAY 帧下发（偏差记档）：本会话立即收尾（node goaway 后不再收流）。
    setImmediate(() => this.close());
    return this;
  }
  setNextStreamID(id) {
    if (this.destroyed) throw __code("ERR_HTTP2_INVALID_SESSION");
    if (typeof id !== "number" || !Number.isInteger(id) || id < 0 || id > 2147483647) {
      throw __code("ERR_OUT_OF_RANGE", "id", id);
    }
    this.__nextStreamID = id;
    return this;
  }
  altsvc(alt, origin) {
    // ALTSVC 帧无底座（偏差记档）：参数校验后 no-op
    if (typeof alt === "string" || alt === undefined) return this;
    throw __code("ERR_INVALID_ARG_TYPE", "alt", "string", alt);
  }
  origin(...origins) { return this; }
  __registerStream(stream) {
    this.__streams.set(stream.id, stream);
  }
  __unregisterStream(stream) {
    this.__streams.delete(stream.id);
  }
  close(cb) {
    if (typeof cb === "function") this.once("close", cb);
    if (this.closed || this.destroyed) {
      if (typeof cb === "function") process.nextTick(cb);
      return this;
    }
    this.closed = true;
    if (this.__conn) {
      __wjs_net_destroy(this.__conn);
    } else {
      setImmediate(() => this.__finish());
    }
    return this;
  }
  destroy(code = 0, cb) {
    if (typeof code === "function") { cb = code; code = 0; }
    if (typeof cb === "function") this.once("close", cb);
    if (this.destroyed) return this;
    this.destroyed = true;
    this.close();
    return this;
  }
  __finish() {
    if (this.__finished) return;
    this.__finished = true;
    this.closed = true;
    this.destroyed = true;
    for (const [, st] of this.__streams) {
      if (!st.destroyed) st.destroy();
    }
    this.__streams.clear();
    queueMicrotask(() => this.emit("close"));
  }
  __ev(kind, payload) {
    switch (kind) {
      case "connClose": {
        this.__server.__sessions.delete(this.__conn);
        this.__finish();
        break;
      }
    }
  }
}

// ── 服务端流对象（node ServerHttp2Stream：真 Duplex）────────────────────────
class Http2ServerStream extends Duplex {
  constructor(session, connId, id, options) {
    super(options ?? {});
    this.__session = session;
    this.__server = session.__server;
    this.__conn = connId;
    this.id = id;
    this.readable = true;
    this.writable = true;
    this.__destroyed = false;
    this.__closed = false;
    this.__aborted = false;
    this.__headersSent = false;
    this.__waitForTrailers = false;
    this.__trailersSent = false;
    this.__wantTrailersFired = false;
    this.__sentHeaders = null;
    this.__sentPseudoHeaders = null;
    this.__sentInfoHeaders = [];
    this.__trailers = null;
    this.sendDate = true;
    this.endAfterHeaders = false;
    this.__reqEnded = false;
    // 实例数据属性遮蔽 Duplex 原型只读 getter（socket.set 套件直写直读）
    Object.defineProperty(this, "readable", { value: true, writable: true, configurable: true });
    Object.defineProperty(this, "writable", { value: true, writable: true, configurable: true });
    session.__registerStream(this);
  }
  get session() { return this.__session; }
  get aborted() { return this.__aborted; }
  get closed() { return this.__closed; }
  get destroyed() { return this.__destroyed; }
  get headersSent() { return this.__headersSent; }
  get _header() { return this.__headersSent; }
  get headersSentRaw() { return this.__headersSent; }
  get sentHeaders() { return this.__sentHeaders; }
  get sentPseudoHeaders() { return this.__sentPseudoHeaders; }
  get sentInfoHeaders() { return this.__sentInfoHeaders; }
  get sentTrailers() { return this.__trailers; }
  get pushAllowed() { return false; }
  get bufferSize() { return this.writableLength; }
  get state() {
    const localClose = this.__closed || this.__trailersSent ? 1 : 0;
    const remoteClose = this.__reqEnded ? 1 : 0;
    return {
      state: this.__destroyed ? 7 : (localClose && remoteClose ? 7 : 2),
      weight: 16,
      sumDependencyWeight: 0,
      localClose,
      remoteClose,
      localWindowSize: 65535,
    };
  }
  // 伪头合成由 req 承担；stream 可读侧 = 请求体
  _read() {}
  __feedBody(b64chunk) {
    const u8 = __b64dec(b64chunk ?? "");
    if (u8.length > 0) this.push(Buffer.from(u8));
  }
  __endReq(trailersJson) {
    if (this.__reqEnded) return;
    this.__reqEnded = true;
    const t = JSON.parse(trailersJson ?? "[]");
    if (t.length > 0) this.emit("trailers", __pairsToObj(t));
    this.push(null);
    this.__maybeAutoClose();
  }
  __onAborted() {
    if (this.__aborted) return;
    this.__aborted = true;
    this.emit("aborted");
    this.destroy();
  }
  // node onStreamClose：close 事件由 destroy 机制单发（§事件单发）
  __abort() {
    if (this.__closed || this.__destroyed) return;
    this.__abortDownstream();
    this.destroy();
  }
  __abortDownstream() {
    if (this.__aborted) return;
    this.__aborted = true;
    if (this.__req) this.__req.__onAborted();
    if (this.__res && !this.__res.destroyed) this.__res.__destroySilent();
  }
  __implicitRespond() {
    if (this.__headersSent || this.__destroyed) return;
    this.respond({});
  }
  respond(headers = {}, options = {}) {
    if (this.destroyed) throw __code("ERR_HTTP2_INVALID_STREAM");
    if (this.__headersSent) throw __code("ERR_HTTP2_HEADERS_SENT");
    if (headers === null || typeof headers !== "object") throw __code("ERR_HTTP2_HEADERS_OBJECT");
    if (options === null || typeof options !== "object") {
      throw __code("ERR_INVALID_ARG_TYPE", "options", "object", options);
    }
    // :status 校验（数字合法才入线；非数字按 node 线上默认 200 处理）
    let status = 200;
    if (headers[":status"] !== undefined) {
      const s = +headers[":status"];
      if (typeof headers[":status"] === "number" &&
          (!Number.isInteger(s) || s < 100 || s > 599)) {
        throw __code("ERR_HTTP2_STATUS_INVALID", headers[":status"]);
      }
      if (Number.isInteger(s) && s >= 100 && s <= 599) status = s;
    }
    __validateH2Headers({ ...headers, ":status": status }, [":status"]);
    this.__waitForTrailers = !!options.waitForTrailers;
    const entries = [];
    const sent = { __proto__: null };
    const sentPseudo = { __proto__: null };
    sentPseudo[":status"] = String(status);
    if (this.sendDate && headers["date"] === undefined && headers["Date"] === undefined) {
      entries.push(["date", new Date().toUTCString()]);
      sent["date"] = new Date().toUTCString();
    }
    for (const [k, v] of Object.entries(headers)) {
      if (k.startsWith(":")) continue;
      __validateHeaderName(k);
      const w = __headerToWire(v);
      if (Array.isArray(w)) {
        for (const one of w) { __validateHeaderValue(k, one); entries.push([k.toLowerCase(), one]); }
        sent[k.toLowerCase()] = w;
      } else {
        __validateHeaderValue(k, w);
        entries.push([k.toLowerCase(), w]);
        sent[k.toLowerCase()] = w;
      }
    }
    this.__headersSent = true;
    this.__sentHeaders = sent;
    this.__sentPseudoHeaders = sentPseudo;
    __wjs_h2_respond(this.__conn, this.id, status, JSON.stringify(entries));
    if (options.endStream) {
      this.endAfterHeaders = true;
      this.__finishWritable();
    }
    return undefined;
  }
  respondWithFile(filename, headers = {}, options = {}) {
    if (this.destroyed) throw __code("ERR_HTTP2_INVALID_STREAM");
    if (this.__headersSent) throw __code("ERR_HTTP2_HEADERS_SENT");
    if (typeof options !== "object" || options === null) {
      throw __code("ERR_INVALID_ARG_TYPE", "options", "object", options);
    }
    if (options.statCheck !== undefined && typeof options.statCheck !== "function") {
      throw __code("ERR_INVALID_ARG_VALUE", "options.statCheck", options.statCheck);
    }
    if (options.onError !== undefined && typeof options.onError !== "function") {
      throw __code("ERR_INVALID_ARG_VALUE", "options.onError", options.onError);
    }
    if (options.offset !== undefined && typeof options.offset !== "number") {
      throw __code("ERR_INVALID_ARG_VALUE", "options.offset", options.offset);
    }
    if (options.length !== undefined && typeof options.length !== "number") {
      throw __code("ERR_INVALID_ARG_VALUE", "options.length", options.length);
    }
    // 204/205/304 禁 payload
    if (headers[":status"] !== undefined) {
      const s = +headers[":status"];
      if (s === 204 || s === 205 || s === 304) {
        throw __code("ERR_HTTP2_PAYLOAD_FORBIDDEN", s);
      }
    }
    const isFd = typeof filename === "number";
    let fd = null;
    let stat;
    try {
      fd = isFd ? filename : fs.openSync(filename, "r");
      stat = fs.fstatSync(fd);
    } catch (err) {
      if (!isFd && fd !== null) { try { fs.closeSync(fd); } catch {} }
      if (typeof options.onError === "function") {
        options.onError(err);
        return;
      }
      throw err;
    }
    try {
      const h = { ...headers };
      if (typeof options.statCheck === "function") {
        if (options.statCheck.call(this, stat, h, stat.isFile() === false) === false) {
          if (!isFd) { try { fs.closeSync(fd); } catch {} }
          this.close();
          return;
        }
      }
      if (h[":status"] !== undefined && [204, 205, 304].includes(+h[":status"])) {
        throw __code("ERR_HTTP2_PAYLOAD_FORBIDDEN", +h[":status"]);
      }
      let offset = options.offset ?? 0;
      let length = options.length ?? (stat.size - offset);
      if (length < 0) length = 0;
      const shouldSendBody = h[":status"] === undefined || !![200, 201, 202, 203, 206].includes(+h[":status"]) ||
        (+h[":status"] >= 300 && ![204, 205, 304].includes(+h[":status"]));
      this.respond(h, {});
      if (shouldSendBody && length > 0) {
        const CH = 0x8000;
        let pos = offset;
        while (pos < offset + length) {
          const n = Math.min(CH, offset + length - pos);
          const buf = Buffer.alloc(n);
          const r = fs.readSync(fd, buf, 0, n, pos);
          if (r <= 0) break;
          if (r < n) {
            __wjs_h2_data(this.__conn, this.id, __b64enc(buf.subarray(0, r)));
            break;
          }
          __wjs_h2_data(this.__conn, this.id, __b64enc(buf));
          pos += r;
        }
      }
      if (!isFd) { try { fs.closeSync(fd); } catch {} }
      this.__finishWritable();
    } catch (err) {
      if (!isFd) { try { fs.closeSync(fd); } catch {} }
      if (typeof options.onError === "function") {
        options.onError(err);
        return;
      }
      throw err;
    }
  }
  pushStream(headers, options, cb) {
    if (typeof options === "function") { cb = options; options = {}; }
    if (typeof cb !== "function") {
      throw __code("ERR_INVALID_ARG_TYPE", "callback", "Function", cb);
    }
    if (this.__session.type !== 0 || this.__isPush) {
      process.nextTick(() => cb(__code("ERR_HTTP2_NESTED_PUSH")));
      return;
    }
    // 底座无 PUSH_PROMISE（hyper；偏差记档）——回调路径收到 PUSH_DISABLED
    process.nextTick(() => cb(__code("ERR_HTTP2_PUSH_DISABLED")));
    return;
  }
  additionalHeaders(headers) {
    if (this.destroyed) throw __code("ERR_HTTP2_INVALID_STREAM");
    if (!this.__headersSent) throw __code("ERR_HTTP2_HEADERS_SENT");
    __validateH2Headers(headers);
    const status = headers[":status"];
    if (status !== undefined) {
      const s = +status;
      if (!Number.isInteger(s) || s < 200 || s > 599) {
        throw __code("ERR_HTTP2_STATUS_INVALID", status);
      }
    }
    this.__sentInfoHeaders.push(headers);
    return this;
  }
  priority(options) {
    if (options === null || typeof options !== "object") {
      throw __code("ERR_INVALID_ARG_TYPE", "options", "object", options);
    }
    if (options.weight !== undefined) {
      const w = options.weight;
      if (typeof w !== "number" || !Number.isInteger(w) || w < 1 || w > 256) {
        throw __code("ERR_OUT_OF_RANGE", "options.weight", w);
      }
    }
    if (options.parent !== undefined && options.parent !== 0) {
      const p = options.parent;
      if (typeof p !== "number" || !Number.isInteger(p) || p < 1 || p > 2147483647) {
        throw __code("ERR_OUT_OF_RANGE", "options.parent", p);
      }
    }
    return this;
  }
  sendTrailers(trailers) {
    if (trailers === null || typeof trailers !== "object") {
      throw __code("ERR_INVALID_ARG_TYPE", "trailers", "object", trailers);
    }
    if (this.__trailersSent) throw __code("ERR_HTTP2_TRAILERS_ALREADY_SENT");
    if (!this.__waitForTrailers || !this.__wantTrailersFired) {
      throw __code("ERR_HTTP2_TRAILERS_CANNOT_BE_SENT");
    }
    __validateH2Headers(trailers, []);
    const t = [];
    for (const [k, v] of Object.entries(trailers)) {
      const w = __headerToWire(v);
      if (Array.isArray(w)) for (const one of w) t.push([k, one]);
      else t.push([k, w]);
    }
    this.__trailersSent = true;
    this.__trailers = { ...trailers };
    __wjs_h2_end(this.__conn, this.id, JSON.stringify(t));
    const cb = this.__finalCb;
    if (typeof cb === "function") queueMicrotask(cb);
    return this;
  }
  close(code = 0, cb) {
    if (typeof code === "function") { cb = code; code = 0; }
    if (typeof cb === "function") this.once("close", cb);
    if (this.__closed || this.__destroyed) return this;
    if (!this.__trailersSent) {
      __wjs_h2_reset(this.__conn, this.id, code);
      this.__trailersSent = true;
    }
    this.destroy();
    return this;
  }
  destroy(err, code, cb) {
    if (typeof err === "number") { cb = code; code = err; err = undefined; }
    if (typeof code === "function") { cb = code; code = 0; }
    if (typeof cb === "function") this.once("close", cb);
    if (this.__destroyed) return this;
    if (err) this.__destroyErr = err;
    else if (!this.__destroyErr && (code ?? 0) !== 0) this.__destroyErr = undefined;
    super.destroy(err);
    return this;
  }
  __finishWritable() {
    // respond({endStream:true}) / respondWithFile 收尾路径
    this.__trailersSent = true;
    super.end();
  }
  _write(chunk, encoding, cb) {
    if (this.__destroyed) {
      cb(__code("ERR_HTTP2_STREAM_CLOSED"));
      return;
    }
    this.__implicitRespond();
    const u8 = chunk instanceof Uint8Array ? chunk : __toU8(String(chunk), "write");
    __wjs_h2_data(this.__conn, this.id, __b64enc(u8));
    queueMicrotask(cb);
  }
  _final(cb) {
    if (this.__destroyed) { cb(); return; }
    this.__implicitRespond();
    if (this.__waitForTrailers && !this.__trailersSent && !this.endAfterHeaders) {
      // wantTrailers 窗口：用户 sendTrailers 后回填收尾
      this.__finalCb = cb;
      queueMicrotask(() => {
        if (!this.__destroyed && !this.__trailersSent) {
          this.__wantTrailersFired = true;
          this.emit("wantTrailers");
        }
      });
      return;
    }
    if (!this.__trailersSent) {
      const t = this.__pendingTrailers ?? [];
      this.__trailersSent = true;
      __wjs_h2_end(this.__conn, this.id, JSON.stringify(t));
    }
    this.__maybeAutoClose();
    cb();
  }
  // 双向尽（应答 End 已发 + 请求体已尽）→ 本地收尾（node 流全关后 socket 解绑）
  __maybeAutoClose() {
    if (this.__destroyed || this.__closed) return;
    if (!(this.__reqEnded && this.__trailersSent)) return;
    this.__closed = true;
    this.__destroyed = true;
    this.__session?.__unregisterStream(this);
    queueMicrotask(() => this.emit("close"));
  }
  _destroy(err, cb) {
    this.__closed = true;
    this.__destroyed = true;
    this.__session?.__unregisterStream(this);
    this.__abortDownstream();
    cb(err ?? this.__destroyErr);
  }
  setTimeout(msecs, callback) {
    if (typeof callback === "function") this.once("timeout", callback);
    const ms = Number(msecs) || 0;
    if (this.__timeoutTimer !== undefined) clearTimeout(this.__timeoutTimer);
    if (ms > 0 && !this.__closed) {
      this.__timeoutTimer = setTimeout(() => {
        this.__timeoutTimer = undefined;
        this.emit("timeout");
      }, ms);
    }
    return this;
  }
  __attach(req, res) { this.__req = req; this.__res = res; }
}

// ── Http2ServerRequest（Readable；从底层 stream 拉取）────────────────────────
class Http2ServerRequest extends Readable {
  constructor(stream, info, scheme, socketProxy) {
    super();
    this.__stream = stream;
    this.__socket = socketProxy;
    this.complete = false;
    this.aborted = false;
    this.httpVersion = "2.0";
    this.httpVersionMajor = 2;
    this.httpVersionMinor = 0;
    const method = String(info.method);
    const authority = String(info.authority ?? "");
    let phoHint = null;
    const reg = [];
    for (const [k, v] of JSON.parse(info.headers)) {
      if (k === "x-wjs-pho") { phoHint = String(v).split(","); continue; }
      reg.push([k, v]);
    }
    const pseudo = [];
    pseudo.push([":method", method]);
    if (method !== "CONNECT") pseudo.push([":path", String(info.path)]);
    if (authority) pseudo.push([":authority", authority]);
    pseudo.push([":scheme", scheme]);
    let all;
    if (phoHint) {
      const ordered = [];
      for (const name of phoHint) {
        const i = pseudo.findIndex(([n]) => n === name);
        if (i !== -1) ordered.push(pseudo.splice(i, 1)[0]);
      }
      all = [...ordered, ...pseudo, ...reg];
    } else {
      all = [...pseudo, ...reg];
    }
    this.headers = __pairsToObj(all);
    this.rawHeaders = all.flat();
    const trailers = JSON.parse(info.trailers ?? "[]");
    this.trailers = __pairsToObj(trailers);
    this.rawTrailers = trailers.flat();
    this.__url = String(info.path);
    stream.__attach(this, null);
    // stream 末尾 → req 收尾
    stream.on("end", () => {
      if (this.__reqDone) return;
      this.__reqDone = true;
      this.complete = true;
      this.push(null);
    });
    stream.on("aborted", () => this.__onAborted());
  }
  get stream() { return this.__stream; }
  get socket() { return this.__socket; }
  get connection() { return this.__socket; }
  get session() { return this.__stream.__session; }
  get method() { return this.headers[":method"]; }
  set method(v) {
    if (typeof v !== "string") throw __code("ERR_INVALID_ARG_TYPE", "method", "string", v);
    if (!__TOKEN_RE.test(v)) throw __code("ERR_INVALID_ARG_VALUE", "method", v);
    this.headers[":method"] = v;
  }
  get scheme() { return this.headers[":scheme"]; }
  set scheme(v) {
    if (typeof v !== "string") throw __code("ERR_INVALID_ARG_TYPE", "scheme", "string", v);
    this.headers[":scheme"] = v;
  }
  get authority() { return this.headers[":authority"] ?? this.headers.host; }
  set authority(v) {
    if (typeof v !== "string") throw __code("ERR_INVALID_ARG_TYPE", "authority", "string", v);
    this.headers[":authority"] = v;
  }
  get url() { return this.__url; }
  set url(v) { this.__url = v; }
  pause() { this.__userPaused = true; return super.pause(); }
  resume() { this.__userPaused = false; return super.resume(); }
  _read() {
    if (this.__reqDone) return;
    const s = this.__stream;
    let chunk;
    while ((chunk = s.read()) !== null) {
      if (!this.push(chunk)) return;
    }
    if (s.readableEnded) {
      this.__reqDone = true;
      this.complete = true;
      this.push(null);
    }
  }
  setTimeout(msecs, callback) {
    if (typeof callback === "function") this.once("timeout", callback);
    const ms = Number(msecs) || 0;
    if (this.__timeoutTimer !== undefined) clearTimeout(this.__timeoutTimer);
    if (ms > 0 && !this.complete && !this.__stream.__closed) {
      this.__timeoutTimer = setTimeout(() => {
        this.__timeoutTimer = undefined;
        this.emit("timeout");
      }, ms);
    }
    return this;
  }
  __onAborted() {
    if (this.aborted) return;
    this.aborted = true;
    this.complete = true;
    this.emit("aborted");
    this.destroy();
  }
}

// ── Http2ServerResponse（Writable；写路径委派给底层 stream）──────────────────
class Http2ServerResponse extends Writable {
  constructor(req, stream) {
    super({ autoDestroy: true });
    this.req = req;
    this.__stream = stream;
    this.__conn = stream.__conn;
    this.__id = stream.id;
    this.__statusCode = 200;
    this.__headers = Object.create(null);
    this.__trailers = Object.create(null);
    this.headersSent = false;
    this.sendDate = true;
    this.__finishEmitted = false;
    this.__ended = false;
    stream.on("drain", () => this.emit("drain"));
    stream.on("finish", () => {
      if (!this.__finishEmitted) {
        this.__finishEmitted = true;
        this.emit("finish");
      }
    });
    stream.on("close", () => {
      if (!this.__ended) this.__ended = true;
      queueMicrotask(() => this.emit("close"));
    });
    stream.__attach(req, this);
  }
  get stream() { return this.__stream; }
  get socket() { return this.__stream.__destroyed ? undefined : this.req.socket; }
  get connection() { return this.socket; }
  get session() { return this.__stream.__session; }
  // node 口径：finished/writableEnded 即时位（end() 同步置位）；长度/水位读底层流
  get finished() { return this.__ended === true; }
  get writableEnded() { return this.__ended === true; }
  get writableFinished() { return this.__finishEmitted === true; }
  get writableLength() { return this.__stream.writableLength; }
  get writableHighWaterMark() { return this.__stream.writableHighWaterMark; }
  get writableCorked() { return this.__stream.writableCorked; }
  get _header() { return this.headersSent; }
  setHeader(name, value) {
    __validateHeaderName(name);
    __validateHeaderValue(name, value);
    this.__headers[String(name).toLowerCase()] = value;
    return this;
  }
  getHeader(name) {
    const v = this.__headers[String(name).toLowerCase()];
    return v === undefined ? undefined : __headerToWire(v);
  }
  getHeaders() {
    const out = { __proto__: null };
    for (const [k, v] of Object.entries(this.__headers)) out[k] = __headerToWire(v);
    return out;
  }
  getHeaderNames() { return Object.keys(this.__headers); }
  hasHeader(name) { return this.__headers[String(name).toLowerCase()] !== undefined; }
  removeHeader(name) { delete this.__headers[String(name).toLowerCase()]; return this; }
  appendHeader(name, value) {
    __validateHeaderName(name);
    __validateHeaderValue(name, value);
    const k = String(name).toLowerCase();
    const cur = this.__headers[k];
    if (cur === undefined) this.__headers[k] = value;
    else if (Array.isArray(cur)) cur.push(value);
    else this.__headers[k] = [cur, value];
    return this;
  }
  get statusMessage() { return ""; }
  set statusMessage(v) {
    process.emitWarning("Status message is not supported by HTTP/2 (RFC7540 8.1.2.4)");
  }
  set statusCode(status) {
    if (typeof status !== "number" || !Number.isInteger(status) || status < 100 || status > 599) {
      throw __code("ERR_HTTP2_STATUS_INVALID", status);
    }
    this.__statusCode = status;
  }
  get statusCode() { return this.__statusCode ?? 200; }
  setTrailer(name, value) {
    __validateHeaderName(name);
    __validateHeaderValue(name, value);
    this.__trailers[String(name).toLowerCase()] = value;
    return this;
  }
  addTrailers(obj) {
    if (obj === null || typeof obj !== "object") {
      throw __code("ERR_INVALID_ARG_TYPE", "headers", "object", obj);
    }
    for (const [k, v] of Object.entries(obj)) this.setTrailer(k, v);
    return this;
  }
  writeHead(status, ...rest) {
    if (typeof status !== "number" || !Number.isInteger(status) || status < 100 || status > 599) {
      throw __code("ERR_HTTP2_STATUS_INVALID", status);
    }
    if (this.headersSent) throw __code("ERR_HTTP2_HEADERS_SENT");
    this.__statusCode = status;
    for (const r of rest) {
      if (typeof r === "string") {
        // reason phrase：h2 不支持（警告由 statusMessage setter 承担）
      } else if (Array.isArray(r)) {
        for (let i = 0; i + 1 < r.length; i += 2) this.setHeader(r[i], r[i + 1]);
      } else if (r !== null && typeof r === "object") {
        for (const [k, v] of Object.entries(r)) this.setHeader(k, v);
      }
    }
    return this;
  }
  __sendHead() {
    if (this.headersSent || this.__stream.__destroyed) return;
    const entries = [];
    for (const [k, v] of Object.entries(this.__headers)) {
      if (k.startsWith(":")) continue;
      const w = __headerToWire(v);
      if (Array.isArray(w)) for (const one of w) entries.push([k, one]);
      else entries.push([k, w]);
    }
    if (this.sendDate && !this.hasHeader("date")) {
      entries.push(["date", new Date().toUTCString()]);
    }
    this.headersSent = true;
    this.__stream.__headersSent = true;
    __wjs_h2_respond(this.__conn, this.__id, this.statusCode, JSON.stringify(entries));
  }
  write(chunk, encoding, cb) {
    if (this.__stream.__destroyed) {
      const err = __code("ERR_HTTP2_INVALID_STREAM");
      if (typeof encoding === "function") encoding(err);
      else if (typeof cb === "function") cb(err);
      return false;
    }
    if (this.__ended) {
      const err = __code("ERR_STREAM_WRITE_AFTER_END");
      if (typeof encoding === "function") encoding(err);
      else if (typeof cb === "function") cb(err);
      return false;
    }
    this.__sendHead();
    return this.__stream.write(chunk, encoding, cb);
  }
  cork() { this.__stream.cork?.(); }
  uncork() { this.__stream.uncork?.(); }
  end(chunk, encoding, cb) {
    if (typeof chunk === "function") { cb = chunk; chunk = null; encoding = null; }
    else if (typeof encoding === "function") { cb = encoding; encoding = null; }
    if (this.__ended) {
      // node h2 口径：end 可重复调用不抛错；cb 至少一次（finish 前 → finish 时，后 → nextTick）
      if (typeof cb === "function") {
        if (this.__finishEmitted) process.nextTick(cb);
        else this.once("finish", cb);
      }
      return this;
    }
    this.__ended = true;
    if (typeof cb === "function") this.once("finish", cb);
    this.__sendHead();
    if (chunk !== null && chunk !== undefined) this.__stream.write(chunk, encoding);
    const t = [];
    for (const [k, v] of Object.entries(this.__trailers)) {
      const w = __headerToWire(v);
      if (Array.isArray(w)) for (const one of w) t.push([k, one]);
      else t.push([k, w]);
    }
    this.__stream.__pendingTrailers = t;
    // __trailersSent 由 stream._final 在 __wjs_h2_end 发出后置位；
    // 此处预置会让 _final 跳过 END_STREAM → 客户端 'end' 永不到（挂死根因）。
    this.__stream.end();
    return this;
  }
  // node 口径：destroy 恒发 finish（clean/err 均先 finish 后 close），
  // 错误不落 res（错误走 stream 'error'，res.on('error') 不触发）。
  destroy(err) {
    if (this.destroyed) return this;
    if (!this.__finishEmitted) {
      this.__finishEmitted = true;
      this.emit("finish");
    }
    this.__stream.destroy(err ?? null);
    return super.destroy();
  }
  _destroy(err, cb) {
    if (!this.__finishEmitted) {
      this.__finishEmitted = true;
      this.emit("finish");
    }
    cb(err);
  }
  __destroySilent() {
    if (this.destroyed) return;
    super.destroy();
  }
  createPushResponse(cb) {
    if (typeof cb !== "function") throw __code("ERR_INVALID_ARG_TYPE", "callback", "Function", cb);
    queueMicrotask(() => cb(__code("ERR_HTTP2_PUSH_DISABLED")));
    return undefined;
  }
  writeContinue(cb) { if (typeof cb === "function") queueMicrotask(cb); return this; }
  writeInformation(type, info, cb) {
    if (typeof type !== "number" || !Number.isInteger(type) || type < 200 || type > 599) {
      throw __code("ERR_HTTP2_STATUS_INVALID", type);
    }
    if (type === 204 || type === 304) {
      throw __code("ERR_HTTP2_INVALID_INFO_STATUS", type);
    }
    if (typeof info === "object" && info !== null && !Array.isArray(info)) {
      for (const [k, v] of Object.entries(info)) {
        __validateHeaderName(k);
        __validateHeaderValue(k, v);
      }
    }
    const f = typeof info === "function" ? info : cb;
    if (typeof f === "function") queueMicrotask(f);
    return this;
  }
  writeEarlyHints(hints, cb) {
    const f = typeof hints === "function" ? hints : cb;
    if (hints !== null && typeof hints === "object") {
      for (const [k, v] of Object.entries(hints)) {
        __validateHeaderName(k);
        __validateHeaderValue(k, v);
      }
    }
    if (typeof f === "function") queueMicrotask(f);
    return this;
  }
  flushHeaders() {
    if (!this.headersSent) this.__sendHead();
  }
  setTimeout(msecs, callback) {
    if (typeof callback === "function") this.once("timeout", callback);
    const ms = Number(msecs) || 0;
    if (this.__timeoutTimer !== undefined) clearTimeout(this.__timeoutTimer);
    if (ms > 0 && !this.__stream.__closed) {
      this.__timeoutTimer = setTimeout(() => {
        this.__timeoutTimer = undefined;
        this.emit("timeout");
      }, ms);
    }
    return this;
  }
}

// ── 服务端 ──────────────────────────────────────────────────────────────
class Http2Server extends EventEmitter {
  constructor(options, secure) {
    super();
    if (typeof options !== "object" || options === null) {
      throw __code("ERR_INVALID_ARG_TYPE", "options", "object", options);
    }
    this.__id = 0;
    this.__listening = null;
    this.__secure = !!secure;
    this.__tlsOpts = null;
    this.__streams = new Map();
    this.__sessions = new Map();
    this.__sockBag = {};
    this.__opts = options;
    this[kPendingOptions] = this.__opts;
    if (options.settings !== undefined) __validateSettings(options.settings);
    if (options.maxOutstandingSettings !== undefined) {
      if (typeof options.maxOutstandingSettings !== "number" ||
          !Number.isInteger(options.maxOutstandingSettings) ||
          options.maxOutstandingSettings < 0) {
        throw __code("ERR_OUT_OF_RANGE", "options.maxOutstandingSettings", options.maxOutstandingSettings);
      }
    }
    if (secure) {
      for (const k of ["maxSessionInvalidStreams", "maxSessionRejectedStreams", "maxSessionInvalidFrames"]) {
        if (options[k] !== undefined &&
            (typeof options[k] !== "number" || !Number.isInteger(options[k]) || options[k] < 0)) {
          throw __code("ERR_OUT_OF_RANGE", `options.${k}`, options[k]);
        }
      }
      if (options.ALPNCallback !== undefined && options.ALPNProtocols !== undefined) {
        throw __code("ERR_TLS_ALPN_CALLBACK_WITH_PROTOCOLS");
      }
      if (options.key !== undefined && options.cert !== undefined) {
        this.__tlsOpts = { tls: { key: String(Array.isArray(options.key) ? options.key[0] : options.key),
                                  cert: String(Array.isArray(options.cert) ? options.cert[0] : options.cert) } };
      } else {
        this.__tlsOpts = null;
      }
    }
    // request 监听器只由 createServer/createSecureServer 接线（§4.39）
    this.__ev = this.__ev.bind(this);
  }
  get socket() { return this.__sockBag; }
  setTimeout(msecs, callback) {
    if (typeof callback === "function") this.once("timeout", callback);
    const ms = Number(msecs) || 0;
    if (this.__timeoutTimer !== undefined) clearTimeout(this.__timeoutTimer);
    if (ms > 0) {
      this.__timeoutTimer = setTimeout(() => {
        this.__timeoutTimer = undefined;
        for (const [, s] of this.__sessions) s.emit("timeout");
        this.emit("timeout");
      }, ms);
    }
    return this;
  }
  updateSettings(settings) {
    const validated = __validateSettings(settings);
    this.__opts.settings = __applySettings(this.__opts.settings ?? {}, validated);
    return this;
  }
  close(cb) {
    if (typeof cb === "function") this.once("close", cb);
    if (this.__closing) return this;
    this.__closing = true;
    if (this.__id) {
      __wjs_net_destroy(this.__id);
      this.__id = 0;
    } else {
      // 未监听或已关：直接收尾（node server.close 无监听也回调）
      queueMicrotask(() => {
        if (this.__sessions.size === 0) this.__emitClose();
      });
    }
    return this;
  }
  __emitClose() {
    if (this.__closeEmitted) return;
    this.__closeEmitted = true;
    this.emit("close");
  }
  __sessionFor(connId, peerObj) {
    let s = this.__sessions.get(connId);
    if (s === undefined) {
      s = new Http2Session(this, connId, peerObj);
      this.__sessions.set(connId, s);
      this.emit("session", s);
    }
    return s;
  }
  __abortEntry(entry) {
    if (!entry) return;
    entry.stream.__abort();
    this.__streams.delete(entry.stream.id);
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
        const id = Number(o.streamId);
        const peer = String(o.peer ?? "");
        const peerObj = {
          addr: peer.includes(":") ? peer.slice(0, peer.lastIndexOf(":")) : peer,
          port: peer.includes(":") ? Number(peer.slice(peer.lastIndexOf(":") + 1)) : 0,
        };
        const session = this.__sessionFor(Number(o.connId), peerObj);
        const stream = new Http2ServerStream(session, Number(o.connId), id);
        const bodyEmpty = (o.body ?? "") === "";
        const trailersEmpty = (o.trailers ?? "[]") === "[]";
        const flags = bodyEmpty && trailersEmpty ? 5 : 4;
        stream.endAfterHeaders = bodyEmpty && trailersEmpty;
        const sock = __mkSocketProxy(stream, this, peerObj);
        const scheme = this.__secure ? "https" : "http";
        const req = new Http2ServerRequest(stream, o, scheme, sock);
        const res = new Http2ServerResponse(req, stream);
        this.__streams.set(id, { stream, req, res });
        stream.once("close", () => this.__streams.delete(id));
        const method = req.headers[":method"];
        if (method === "CONNECT") {
          if (this.listenerCount("connect") > 0) this.emit("connect", req, res);
          else { res.statusCode = 501; res.end(); }
        } else {
          // node 顺序：'stream'（raw）先于 'request'（compat）
          this.emit("stream", stream, req.headers, flags, req.rawHeaders);
          const hasCompat = this.listenerCount("request") > 0;
          if (hasCompat) this.emit("request", req, res);
          stream.__feedBody(o.body ?? "");
          stream.__endReq(o.trailers ?? "[]");
          if (hasCompat) queueMicrotask(() => { if (!req.destroyed && !req.__userPaused) req.resume(); });
        }
        break;
      }
      case "body": {
        const o = JSON.parse(payload);
        const e = this.__streams.get(Number(o.streamId));
        if (e) e.stream.__feedBody(o.payload ?? "");
        break;
      }
      case "reqEnd": {
        const o = JSON.parse(payload);
        const e = this.__streams.get(Number(o.streamId));
        if (e) e.stream.__endReq(o.payload ?? "[]");
        break;
      }
      case "aborted": {
        const o = JSON.parse(payload);
        this.__abortEntry(this.__streams.get(Number(o.streamId)));
        break;
      }
      case "connClose": {
        // Rust 侧 payload = conn_id 裸串；包装层 streamId 恒 0，须读 .payload
        const connId = Number(JSON.parse(payload).payload);
        const s = this.__sessions.get(connId);
        if (s) s.__ev("connClose", payload);
        // 全会话已收且监听器已关 → server 'close'
        queueMicrotask(() => {
          if (this.__closing && !this.__id && this.__sessions.size === 0) this.__emitClose();
        });
        break;
      }
      case "error": {
        const o = JSON.parse(payload);
        this.emit("error", __h2Err(o.code, o.msg));
        break;
      }
      case "close": {
        // 监听器已关：余下连接由 Rust conn 退出逐个 connClose；无连接则立即收尾
        queueMicrotask(() => {
          if (this.__sessions.size === 0) this.__emitClose();
        });
        break;
      }
    }
  }
  address() { return this.__listening; }
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
    this.__closing = false;
    this.__id = Number(__wjs_h2_listen(Number(port), host === null ? "0.0.0.0" : host,
      JSON.stringify(this.__tlsOpts ?? {}), this));
    return this;
  }
  [Symbol.asyncDispose]() {
    return new Promise((resolve) => {
      this.close(() => resolve(undefined));
    });
  }
  ref() { if (this.__id) __wjs_net_ref(this.__id); return this; }
  unref() { if (this.__id) __wjs_net_unref(this.__id); return this; }
}
const kPendingOptions = Symbol("options");

// ── 客户端 ──────────────────────────────────────────────────────────────
const __DEFERRED_METHODS = new Set(["POST", "PUT", "PATCH"]);
class ClientHttp2Stream extends Duplex {
  constructor(session, id, headers, options) {
    super();
    this.__session = session;
    this.id = id;
    this.sentHeaders = headers;
    this.__ended = false;
    this.aborted = false;
    this.__opened = false;
    this.__deferred = __DEFERRED_METHODS.has(String(headers[":method"]));
    this.__waitTrailers = !!(options && options.waitForTrailers);
    this.__pendingBody = [];
    this.endAfterHeaders = false;
  }
  get session() { return this.__session; }
  get bufferSize() { return this.writableLength; }
  get pushAllowed() { return false; }
  get sentPseudoHeaders() {
    const out = { __proto__: null };
    for (const [k, v] of Object.entries(this.sentHeaders)) {
      if (k.startsWith(":")) out[k] = Array.isArray(v) ? String(v[0]) : String(v);
    }
    return out;
  }
  get sentInfoHeaders() { return []; }
  get sentTrailers() { return null; }
  get state() {
    return {
      state: this.destroyed ? 7 : 2,
      weight: 16,
      sumDependencyWeight: 0,
      localClose: this.writableEnded ? 1 : 0,
      remoteClose: this.readableEnded ? 1 : 0,
      localWindowSize: 65535,
    };
  }
  _read() {}
  __openNow(extraBody) {
    if (this.__opened) return;
    this.__opened = true;
    const parts = extraBody ? [...this.__pendingBody, extraBody] : this.__pendingBody;
    this.__pendingBody = [];
    let total = 0;
    for (const b of parts) total += b.length;
    const flat = new Uint8Array(total);
    let off = 0;
    for (const b of parts) { flat.set(b, off); off += b.length; }
    __wjs_h2_open(this.__session.__id, this.id,
      JSON.stringify({
        method: this.sentHeaders[":method"], path: this.sentHeaders[":path"],
        scheme: this.sentHeaders[":scheme"], authority: this.sentHeaders[":authority"],
        waitTrailers: this.__waitTrailers,
        headers: Object.entries(this.sentHeaders).filter(([k]) => !k.startsWith(":")),
      }),
      __b64enc(flat));
    if (this.__waitTrailers) {
      queueMicrotask(() => { if (!this.destroyed) this.emit("wantTrailers"); });
    }
  }
  _write(chunk, encoding, cb) {
    if (this.__opened || this.__ended) {
      cb(__code("ERR_STREAM_WRITE_AFTER_END"));
      return;
    }
    const u8 = chunk instanceof Uint8Array ? chunk : __toU8(String(chunk), "write");
    this.__pendingBody.push(u8);
    queueMicrotask(cb);
  }
  _final(cb) {
    this.__openNow(null);
    cb();
  }
  sendTrailers(trailers) {
    if (!this.__opened || !this.__waitTrailers) {
      throw __code("ERR_HTTP2_TRAILERS_CANNOT_BE_SENT");
    }
    if (this.__trailersSent) throw __code("ERR_HTTP2_TRAILERS_ALREADY_SENT");
    this.__trailersSent = true;
    const t = [];
    for (const [k, v] of Object.entries(trailers ?? {})) t.push([k, String(v)]);
    __wjs_h2_open_trailers(this.__session.__id, this.id, JSON.stringify(t));
    return this;
  }
  __onResponse(headers, flags) {
    if (flags & 1) this.endAfterHeaders = true;
    this.emit("response", headers, flags);
  }
  __onData(u8) { this.push(Buffer.from(u8)); }
  __onTrailers(t) { this.emit("trailers", t); }
  __onEnd() { this.push(null); }
  __onAborted() {
    if (this.aborted) return;
    this.aborted = true;
    this.emit("aborted");
    this.push(null);
    this.destroy();
  }
  priority(options) {
    if (options === null || typeof options !== "object") {
      throw __code("ERR_INVALID_ARG_TYPE", "options", "object", options);
    }
    if (options.weight !== undefined) {
      const w = options.weight;
      if (typeof w !== "number" || !Number.isInteger(w) || w < 1 || w > 256) {
        throw __code("ERR_OUT_OF_RANGE", "options.weight", w);
      }
    }
    if (options.parent !== undefined && options.parent !== 0) {
      const p = options.parent;
      if (typeof p !== "number" || !Number.isInteger(p) || p < 1 || p > 2147483647) {
        throw __code("ERR_OUT_OF_RANGE", "options.parent", p);
      }
    }
    if (options.silent !== true) { /* 无 PRIORITY 帧底座（偏差记档） */ }
    return this;
  }
  close(code, cb) {
    if (typeof code === "function") { cb = code; code = 0; }
    if (!this.destroyed) {
      if (typeof cb === "function") this.once("close", cb);
      this.__ended = true;
      if (this.__opened && this.__session.__id) {
        __wjs_h2_reset(this.__session.__id, this.id, code ?? 0);
      }
      this.destroy();
    } else if (typeof cb === "function") {
      queueMicrotask(cb);
    }
    return this;
  }
  setTimeout(msecs, callback) {
    if (typeof callback === "function") this.once("timeout", callback);
    const ms = Number(msecs) || 0;
    if (this.__timeoutTimer !== undefined) clearTimeout(this.__timeoutTimer);
    if (ms > 0 && !this.destroyed) {
      this.__timeoutTimer = setTimeout(() => {
        this.__timeoutTimer = undefined;
        this.emit("timeout");
      }, ms);
    }
    return this;
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
    this.type = 1; // NGHTTP2_SESSION_CLIENT
    this.encrypted = false;
    this.connecting = true;
    this.__settings = { ...__DEFAULT_SETTINGS, ...(options?.settings ? __validateSettings(options.settings) : {}) };
    this.__remoteSettings = { ...__DEFAULT_SETTINGS };
    this.__pendingSettingsAck = false;
    this.__outstandingSettings = 0;
    this.__maxOutstandingSettings = options?.maxOutstandingSettings ?? Infinity;
    this.state = {
      effectiveLocalWindowSize: 65535,
      effectiveRemoteWindowSize: 65535,
      localWindowSize: 65535,
      remoteWindowSize: 65535,
      outboundQueueSize: 0,
      deflateDynamicTableSize: 4096,
      inflateDynamicTableSize: 4096,
    };
    this.__ev = this.__ev.bind(this);
  }
  get socket() {
    if (this.__socket === undefined) {
      const s = this;
      const sock = new EventEmitter();
      sock.connecting = true;
      sock.destroyed = false;
      sock.readable = true;
      sock.writable = true;
      sock.remoteAddress = s.__host;
      sock.remotePort = s.__port;
      sock.localAddress = undefined;
      sock.localPort = undefined;
      sock.on("newListener", (ev) => {
        if (ev === "close" && s.closed && !sock.destroyed) {
          queueMicrotask(() => sock.emit("close"));
        }
      });
      sock.destroy = (err) => {
        if (sock.destroyed) return;
        sock.destroyed = true;
        if (err) sock.emit("error", err);
        s.destroy();
      };
      sock.end = () => { s.close(); return sock; };
      sock.write = () => true;
      sock.setTimeout = (m, cb) => { s.setTimeout(m, cb); return sock; };
      sock.address = () => ({ address: s.__host, port: s.__port, family: s.__host?.includes(":") ? "IPv6" : "IPv4" });
      this.__socket = sock;
    }
    return this.__socket;
  }
  get alpnProtocol() { return this.__secure ? "h2" : false; }
  get localSettings() { return this.__settings; }
  get remoteSettings() { return this.__remoteSettings; }
  get pendingSettingsAck() { return this.__pendingSettingsAck; }
  get originSet() { return this.__secure ? [] : undefined; }
  get unrefed() { return false; }
  setTimeout(msecs, callback) {
    if (typeof callback === "function") this.once("timeout", callback);
    const ms = Number(msecs) || 0;
    if (this.__timeoutTimer !== undefined) clearTimeout(this.__timeoutTimer);
    if (ms > 0 && !this.closed && !this.destroyed) {
      this.__timeoutTimer = setTimeout(() => {
        this.__timeoutTimer = undefined;
        this.emit("timeout");
      }, ms);
    }
    return this;
  }
  ref() { if (this.__id) __wjs_net_ref(this.__id); return this; }
  unref() { if (this.__id) __wjs_net_unref(this.__id); return this; }
  settings(settings, cb) {
    const validated = __validateSettings(settings);
    if (this.destroyed) throw __code("ERR_HTTP2_INVALID_SESSION");
    this.__pendingSettingsAck = true;
    this.__outstandingSettings++;
    if (Number.isFinite(this.__maxOutstandingSettings) &&
        this.__outstandingSettings >= this.__maxOutstandingSettings) {
      this.__outstandingSettings = 0;
      this.__pendingSettingsAck = false;
      process.nextTick(() => {
        this.emit("error", __code("ERR_HTTP2_MAX_PENDING_SETTINGS_ACK"));
        this.destroy();
      });
      return this;
    }
    setTimeout(() => {
      this.__outstandingSettings = Math.max(0, this.__outstandingSettings - 1);
      if (this.__outstandingSettings === 0) this.__pendingSettingsAck = false;
      this.__settings = __applySettings(this.__settings, validated);
      if (!this.destroyed) {
        this.emit("localSettings", this.__settings);
        if (typeof cb === "function") cb();
      }
    }, 1);
    return this;
  }
  updateSettings(settings) {
    const validated = __validateSettings(settings);
    this.__settings = __applySettings(this.__settings, validated);
    return this;
  }
  ping(cb, payload) {
    if (this.destroyed) throw __code("ERR_HTTP2_INVALID_SESSION");
    let buf = null;
    if (payload !== undefined && payload !== null) {
      const u8 = payload instanceof Uint8Array ? payload : __toU8(payload, "ping");
      if (u8.length > 8) throw __code("ERR_OUT_OF_RANGE", "payload", u8.length);
      buf = u8;
    }
    if (typeof cb !== "function") {
      throw __code("ERR_INVALID_ARG_TYPE", "callback", "function", cb);
    }
    const ret = Buffer.alloc(8);
    if (buf) Buffer.from(buf).copy(ret);
    setTimeout(() => {
      if (this.destroyed) { cb(__code("ERR_HTTP2_PING_CANCEL")); return; }
      cb(null, 0.5, ret);
    }, 1);
    return true;
  }
  goaway(code = 0, lastStreamID = 0, opaqueData) {
    if (this.destroyed) throw __code("ERR_HTTP2_INVALID_SESSION");
    if (typeof code === "object" && code !== null) {
      const o = code;
      code = o.errorCode ?? 0;
      lastStreamID = o.lastStreamID ?? 0;
      opaqueData = o.opaqueData;
    }
    this.__goaway = { code, lastStreamID, opaqueData };
    setImmediate(() => this.close());
    return this;
  }
  setNextStreamID(id) {
    if (this.destroyed) throw __code("ERR_HTTP2_INVALID_SESSION");
    if (typeof id !== "number" || !Number.isInteger(id) || id < 0 || id > 2147483647) {
      throw __code("ERR_OUT_OF_RANGE", "id", id);
    }
    this.__seq = id;
    return this;
  }
  altsvc(alt, origin) {
    if (typeof alt === "string" || alt === undefined) return this;
    throw __code("ERR_INVALID_ARG_TYPE", "alt", "string", alt);
  }
  origin(...origins) { return this; }
  __start(port, host) {
    this.__host = host;
    this.__port = port;
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
        this.connecting = false;
        if (this.__socket) {
          this.__socket.connecting = false;
          queueMicrotask(() => this.__socket.emit("connect"));
        }
        this.emit("connect", this, null);
        break;
      case "response": {
        const o = JSON.parse(payload);
        const st = this.__streams.get(Number(o.streamId));
        if (!st) break;
        const head = JSON.parse(o.payload);
        const headers = __pairsToObj(JSON.parse(head.headers));
        headers[":status"] = head.status;
        st.__onResponse(headers, head.flags ?? 4);
        break;
      }
      case "data": {
        const o = JSON.parse(payload);
        const st = this.__streams.get(Number(o.streamId));
        if (!st) break;
        st.__onData(__b64dec(o.payload));
        break;
      }
      case "trailers": {
        const o = JSON.parse(payload);
        const st = this.__streams.get(Number(o.streamId));
        if (!st) break;
        st.__onTrailers(__pairsToObj(JSON.parse(o.payload)));
        break;
      }
      case "end": {
        const o = JSON.parse(payload);
        const sid = Number(o.streamId);
        const st = this.__streams.get(sid);
        if (!st) break;
        st.__onEnd();
        this.__streams.delete(sid);
        break;
      }
      case "aborted": {
        const o = JSON.parse(payload);
        const sid = Number(o.streamId);
        const st = this.__streams.get(sid);
        if (!st) break;
        st.__onAborted();
        this.__streams.delete(sid);
        break;
      }
      case "error": {
        const o = JSON.parse(payload);
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
        this.connecting = false;
        if (this.__socket && !this.__socket.destroyed) {
          this.__socket.destroyed = true;
          queueMicrotask(() => this.__socket.emit("close"));
        }
        this.emit("close");
        break;
    }
  }
  request(headers, options) {
    if (this.destroyed) throw __code("ERR_HTTP2_GOAWAY_SESSION");
    const h = { ...headers };
    if (h[":method"] === undefined) h[":method"] = "GET";
    if (h[":path"] === undefined && h[":method"] !== "CONNECT") h[":path"] = "/";
    if (h[":authority"] === undefined && h.host === undefined) h[":authority"] = this.__authority;
    if (h[":scheme"] === undefined) h[":scheme"] = this.__secure ? "https" : "http";
    const pseudoKeys = Object.keys(h).filter((k) => k.startsWith(":"));
    const canonical = [":method", ":scheme", ":authority", ":path"];
    const differs = pseudoKeys.length !== canonical.length ||
      pseudoKeys.some((k, i) => k !== canonical[i]);
    if (differs) h["x-wjs-pho"] = pseudoKeys.join(",");
    const sid = this.__seq;
    this.__seq += 2; // 客户端单数流（RFC 7540 口径）
    const st = new ClientHttp2Stream(this, sid, h, options);
    this.__streams.set(sid, st);
    if (!st.__deferred) st.__openNow(null);
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
  destroy(code, cb) {
    if (typeof code === "function") { cb = code; code = 0; }
    if (typeof cb === "function") this.once("close", cb);
    return this.close();
  }
}

export function createServer(options, listener) {
  if (typeof options === "function") { listener = options; options = undefined; }
  const s = new Http2Server(options ?? {}, false);
  if (typeof listener === "function") s.on("request", listener);
  return s;
}
export function createSecureServer(options, listener) {
  if (options !== undefined && (typeof options !== "object" || options === null)) {
    throw __code("ERR_INVALID_ARG_TYPE", "options", "object", options);
  }
  const s = new Http2Server(options ?? {}, true);
  if (typeof listener === "function") s.on("request", listener);
  return s;
}
export function connect(authority, options, listener) {
  let url = null;
  let host, port, secure;
  if (typeof authority === "string") {
    url = new URL(authority);
  } else if (authority !== null && typeof authority === "object") {
    const o = authority;
    if (typeof o.href === "string" && typeof o.protocol === "string" && typeof o.hostname === "string") {
      url = new URL(o.href); // URL 实例
    } else {
      const proto = String(o.protocol ?? "http:").toLowerCase();
      if (!proto.endsWith(":")) throw __code("ERR_HTTP2_INVALID_PROTOCOL", proto, ":");
      secure = proto === "https:";
      host = String(o.authority ?? o.hostname ?? o.host ?? "localhost");
      port = o.port !== undefined ? Number(o.port) : (secure ? 443 : 80);
      if (typeof options === "function") { listener = options; options = {}; }
      const session = new ClientHttp2Session(host, { ...(options ?? {}), tls: secure ? (options ?? {}) : undefined });
      session.__secure = secure;
      if (typeof listener === "function") session.once("connect", listener);
      session.__start(port, host);
      return session;
    }
  } else {
    options = authority ?? {};
  }
  if (typeof options === "function") { listener = options; options = {}; }
  secure = url.protocol === "https:";
  const session = new ClientHttp2Session(url.host, { ...(options ?? {}), tls: secure ? (options ?? {}) : undefined });
  session.__secure = secure;
  if (typeof listener === "function") session.once("connect", listener);
  port = url.port ? Number(url.port) : (secure ? 443 : 80);
  session.__start(port, url.hostname);
  return session;
}

export function getDefaultSettings() {
  return { ...__DEFAULT_SETTINGS, customSettings: {} };
}
export function getPackedSettings(settings) {
  const s = __validateSettings(settings ?? {});
  const out = [];
  const push = (id, val) => {
    out.push((id >> 8) & 0xff, id & 0xff, (val >>> 24) & 0xff, (val >>> 16) & 0xff, (val >>> 8) & 0xff, val & 0xff);
  };
  if (s.headerTableSize !== undefined) push(0x1, s.headerTableSize);
  if (s.enablePush !== undefined) push(0x2, s.enablePush ? 1 : 0);
  if (s.initialWindowSize !== undefined) push(0x4, s.initialWindowSize);
  if (s.maxFrameSize !== undefined) push(0x5, s.maxFrameSize);
  if (s.maxConcurrentStreams !== undefined) push(0x3, s.maxConcurrentStreams);
  if (s.maxHeaderListSize !== undefined) push(0x6, s.maxHeaderListSize);
  if (s.maxHeaderSize !== undefined) push(0x6, s.maxHeaderSize);
  if (s.enableConnectProtocol !== undefined) push(0x8, s.enableConnectProtocol ? 1 : 0);
  if (s.customSettings) {
    for (const [k, v] of Object.entries(s.customSettings)) push(Number(k), Number(v));
  }
  return Buffer.from(out);
}
export function getUnpackedSettings(buf) {
  if (!Buffer.isBuffer(buf) && !(buf instanceof Uint8Array) && !(buf instanceof ArrayBuffer)) {
    throw __code("ERR_INVALID_ARG_TYPE", "buffer", "Buffer|TypedArray", buf);
  }
  const u8 = buf instanceof ArrayBuffer ? new Uint8Array(buf) : buf;
  if (u8.length % 6 !== 0) throw __code("ERR_HTTP2_INVALID_PACKED_SETTINGS_LENGTH");
  const out = {};
  for (let i = 0; i < u8.length; i += 6) {
    const id = (u8[i] << 8) | u8[i + 1];
    const val = ((u8[i + 2] << 24) | (u8[i + 3] << 16) | (u8[i + 4] << 8) | u8[i + 5]) >>> 0;
    switch (id) {
      case 0x1: __validateSetting("headerTableSize", val); out.headerTableSize = val; break;
      case 0x2: if (val !== 0 && val !== 1) throw __code("ERR_HTTP2_INVALID_SETTING_VALUE", "enablePush", val); out.enablePush = val === 1; break;
      case 0x3: __validateSetting("maxConcurrentStreams", val); out.maxConcurrentStreams = val; break;
      case 0x4: __validateSetting("initialWindowSize", val); out.initialWindowSize = val; break;
      case 0x5: __validateSetting("maxFrameSize", val); out.maxFrameSize = val; break;
      case 0x6: __validateSetting("maxHeaderListSize", val); out.maxHeaderListSize = val; break;
      case 0x8: if (val !== 0 && val !== 1) throw __code("ERR_HTTP2_INVALID_SETTING_VALUE", "enableConnectProtocol", val); out.enableConnectProtocol = val === 1; break;
      default: if (val > 0) out.customSettings = { ...(out.customSettings ?? {}), [String(id)]: val };
    }
  }
  return out;
}
function __validateSetting(name, val) {
  const spec = __SETTING_RANGES[name];
  if (spec !== undefined && spec !== "boolean" && (val < spec[0] || val > spec[1])) {
    throw __code("ERR_HTTP2_INVALID_SETTING_VALUE", name, val);
  }
}
export const sensitiveHeaders = Symbol("sensitiveHeaders");

export const constants = {
  NGHTTP2_SESSION_SERVER: 0, NGHTTP2_SESSION_CLIENT: 1,
  NGHTTP2_STREAM_STATE_IDLE: 1, NGHTTP2_STREAM_STATE_OPEN: 2,
  NGHTTP2_STREAM_STATE_RESERVED_LOCAL: 3, NGHTTP2_STREAM_STATE_RESERVED_REMOTE: 4,
  NGHTTP2_STREAM_STATE_HALF_CLOSED_LOCAL: 5, NGHTTP2_STREAM_STATE_HALF_CLOSED_REMOTE: 6,
  NGHTTP2_STREAM_STATE_CLOSED: 7,
  NGHTTP2_NO_ERROR: 0x00, NGHTTP2_PROTOCOL_ERROR: 0x01, NGHTTP2_INTERNAL_ERROR: 0x02,
  NGHTTP2_FLOW_CONTROL_ERROR: 0x03, NGHTTP2_SETTINGS_TIMEOUT: 0x04, NGHTTP2_STREAM_CLOSED: 0x05,
  NGHTTP2_FRAME_SIZE_ERROR: 0x06, NGHTTP2_REFUSED_STREAM: 0x07, NGHTTP2_CANCEL: 0x08,
  NGHTTP2_COMPRESSION_ERROR: 0x09, NGHTTP2_CONNECT_ERROR: 0x0a, NGHTTP2_ENHANCE_YOUR_CALM: 0x0b,
  NGHTTP2_INADEQUATE_SECURITY: 0x0c, NGHTTP2_HTTP_1_1_REQUIRED: 0x0d,
  NGHTTP2_ERR_FRAME_SIZE_ERROR: -522, NGHTTP2_ERR_HEADER_COMPRESSION: -502,
  NGHTTP2_ERR_FLOW_CONTROL: -506, NGHTTP2_ERR_START_STREAM_NOT_ALLOWED: -512,
  NGHTTP2_DEFAULT_WEIGHT: 16,
  HTTP2_HEADER_STATUS: ":status", HTTP2_HEADER_METHOD: ":method",
  HTTP2_HEADER_PATH: ":path", HTTP2_HEADER_SCHEME: ":scheme",
  HTTP2_HEADER_AUTHORITY: ":authority",
  HTTP2_HEADER_ACCEPT_CHARSET: "accept-charset", HTTP2_HEADER_ACCEPT_ENCODING: "accept-encoding",
  HTTP2_HEADER_ACCEPT_LANGUAGE: "accept-language", HTTP2_HEADER_ACCEPT_RANGES: "accept-ranges",
  HTTP2_HEADER_ACCEPT: "accept", HTTP2_HEADER_ACCESS_CONTROL_ALLOW_CREDENTIALS: "access-control-allow-credentials",
  HTTP2_HEADER_ACCESS_CONTROL_ALLOW_HEADERS: "access-control-allow-headers",
  HTTP2_HEADER_ACCESS_CONTROL_ALLOW_METHODS: "access-control-allow-methods",
  HTTP2_HEADER_ACCESS_CONTROL_ALLOW_ORIGIN: "access-control-allow-origin",
  HTTP2_HEADER_ACCESS_CONTROL_EXPOSE_HEADERS: "access-control-expose-headers",
  HTTP2_HEADER_ACCESS_CONTROL_MAX_AGE: "access-control-max-age",
  HTTP2_HEADER_ACCESS_CONTROL_REQUEST_HEADERS: "access-control-request-headers",
  HTTP2_HEADER_ACCESS_CONTROL_REQUEST_METHOD: "access-control-request-method",
  HTTP2_HEADER_AGE: "age", HTTP2_HEADER_AUTHORIZATION: "authorization",
  HTTP2_HEADER_CACHE_CONTROL: "cache-control", HTTP2_HEADER_CONTENT_DISPOSITION: "content-disposition",
  HTTP2_HEADER_CONTENT_ENCODING: "content-encoding", HTTP2_HEADER_CONTENT_LANGUAGE: "content-language",
  HTTP2_HEADER_CONTENT_LENGTH: "content-length", HTTP2_HEADER_CONTENT_LOCATION: "content-location",
  HTTP2_HEADER_CONTENT_RANGE: "content-range", HTTP2_HEADER_CONTENT_TYPE: "content-type",
  HTTP2_HEADER_COOKIE: "cookie", HTTP2_HEADER_DATE: "date", HTTP2_HEADER_DNT: "dnt",
  HTTP2_HEADER_ETAG: "etag", HTTP2_HEADER_EXPECT: "expect", HTTP2_HEADER_EXPIRES: "expires",
  HTTP2_HEADER_FORWARDED: "forwarded", HTTP2_HEADER_FROM: "from",
  HTTP2_HEADER_HOST: "host", HTTP2_HEADER_IF_MATCH: "if-match",
  HTTP2_HEADER_IF_MODIFIED_SINCE: "if-modified-since", HTTP2_HEADER_IF_NONE_MATCH: "if-none-match",
  HTTP2_HEADER_IF_RANGE: "if-range", HTTP2_HEADER_IF_UNMODIFIED_SINCE: "if-unmodified-since",
  HTTP2_HEADER_LAST_MODIFIED: "last-modified", HTTP2_HEADER_LINK: "link",
  HTTP2_HEADER_LOCATION: "location", HTTP2_HEADER_MAX_FORWARDS: "max-forwards",
  HTTP2_HEADER_PREFER: "prefer", HTTP2_HEADER_PROXY_AUTHENTICATE: "proxy-authenticate",
  HTTP2_HEADER_PROXY_AUTHORIZATION: "proxy-authorization", HTTP2_HEADER_RANGE: "range",
  HTTP2_HEADER_REFERER: "referer", HTTP2_HEADER_REFRESH: "refresh",
  HTTP2_HEADER_RETRY_AFTER: "retry-after", HTTP2_HEADER_SERVER: "server",
  HTTP2_HEADER_SET_COOKIE: "set-cookie", HTTP2_HEADER_STRICT_TRANSPORT_SECURITY: "strict-transport-security",
  HTTP2_HEADER_TRAILER: "trailer", HTTP2_HEADER_TK: "tk",
  HTTP2_HEADER_UPGRADE_INSECURE_REQUESTS: "upgrade-insecure-requests",
  HTTP2_HEADER_USER_AGENT: "user-agent", HTTP2_HEADER_VARY: "vary",
  HTTP2_HEADER_VIA: "via", HTTP2_HEADER_WARNING: "warning",
  HTTP2_HEADER_WWW_AUTHENTICATE: "www-authenticate", HTTP2_HEADER_X_CONTENT_TYPE_OPTIONS: "x-content-type-options",
  HTTP2_HEADER_X_FRAME_OPTIONS: "x-frame-options",
  HTTP2_HEADER_CONNECTION: "connection", HTTP2_HEADER_UPGRADE: "upgrade",
  HTTP2_HEADER_HTTP2_SETTINGS: "http2-settings", HTTP2_HEADER_TE: "te",
  HTTP2_HEADER_TRANSFER_ENCODING: "transfer-encoding", HTTP2_HEADER_KEEP_ALIVE: "keep-alive",
  HTTP2_HEADER_PROXY_CONNECTION: "proxy-connection",
  HTTP2_METHOD_CONNECT: "CONNECT", HTTP2_METHOD_DELETE: "DELETE", HTTP2_METHOD_GET: "GET",
  HTTP2_METHOD_HEAD: "HEAD", HTTP2_METHOD_MERGE: "MERGE", HTTP2_METHOD_OPTIONS: "OPTIONS",
  HTTP2_METHOD_PATCH: "PATCH", HTTP2_METHOD_POST: "POST", HTTP2_METHOD_PUT: "PUT",
  HTTP2_METHOD_TRACE: "TRACE",
  HTTP_STATUS_CONTINUE: 100, HTTP_STATUS_SWITCHING_PROTOCOLS: 101, HTTP_STATUS_PROCESSING: 102,
  HTTP_STATUS_EARLY_HINTS: 103, HTTP_STATUS_OK: 200, HTTP_STATUS_CREATED: 201,
  HTTP_STATUS_ACCEPTED: 202, HTTP_STATUS_NON_AUTHORITATIVE_INFORMATION: 203,
  HTTP_STATUS_NO_CONTENT: 204, HTTP_STATUS_RESET_CONTENT: 205, HTTP_STATUS_PARTIAL_CONTENT: 206,
  HTTP_STATUS_MULTIPLE_CHOICES: 300, HTTP_STATUS_MOVED_PERMANENTLY: 301, HTTP_STATUS_FOUND: 302,
  HTTP_STATUS_SEE_OTHER: 303, HTTP_STATUS_NOT_MODIFIED: 304, HTTP_STATUS_USE_PROXY: 305,
  HTTP_STATUS_TEMPORARY_REDIRECT: 307, HTTP_STATUS_PERMANENT_REDIRECT: 308,
  HTTP_STATUS_BAD_REQUEST: 400, HTTP_STATUS_UNAUTHORIZED: 401, HTTP_STATUS_PAYMENT_REQUIRED: 402,
  HTTP_STATUS_FORBIDDEN: 403, HTTP_STATUS_NOT_FOUND: 404, HTTP_STATUS_METHOD_NOT_ALLOWED: 405,
  HTTP_STATUS_NOT_ACCEPTABLE: 406, HTTP_STATUS_PROXY_AUTHENTICATION_REQUIRED: 407,
  HTTP_STATUS_REQUEST_TIMEOUT: 408, HTTP_STATUS_CONFLICT: 409, HTTP_STATUS_GONE: 410,
  HTTP_STATUS_LENGTH_REQUIRED: 411, HTTP_STATUS_PRECONDITION_FAILED: 412,
  HTTP_STATUS_REQUEST_ENTITY_TOO_LARGE: 413, HTTP_STATUS_REQUEST_URI_TOO_LONG: 414,
  HTTP_STATUS_UNSUPPORTED_MEDIA_TYPE: 415, HTTP_STATUS_REQUESTED_RANGE_NOT_SATISFIABLE: 416,
  HTTP_STATUS_EXPECTATION_FAILED: 417, HTTP_STATUS_IM_A_TEAPOT: 418,
  HTTP_STATUS_MISDIRECTED_REQUEST: 421, HTTP_STATUS_UNPROCESSABLE_ENTITY: 422,
  HTTP_STATUS_LOCKED: 423, HTTP_STATUS_FAILED_DEPENDENCY: 424, HTTP_STATUS_TOO_EARLY: 425,
  HTTP_STATUS_UPGRADE_REQUIRED: 426, HTTP_STATUS_PRECONDITION_REQUIRED: 428,
  HTTP_STATUS_TOO_MANY_REQUESTS: 429, HTTP_STATUS_REQUEST_HEADERS_FIELDS_TOO_LARGE: 431,
  HTTP_STATUS_UNAVAILABLE_FOR_LEGAL_REASONS: 451, HTTP_STATUS_INTERNAL_SERVER_ERROR: 500,
  HTTP_STATUS_METHOD_NOT_IMPLEMENTED: 501, HTTP_STATUS_BAD_GATEWAY: 502,
  HTTP_STATUS_SERVICE_UNAVAILABLE: 503, HTTP_STATUS_GATEWAY_TIMEOUT: 504,
  HTTP_STATUS_HTTP_VERSION_NOT_SUPPORTED: 505, HTTP_STATUS_VARIANT_ALSO_NEGOTIATES: 506,
  HTTP_STATUS_INSUFFICIENT_STORAGE: 507, HTTP_STATUS_LOOP_DETECTED: 508,
  HTTP_STATUS_BANDWIDTH_LIMIT_EXCEEDED: 509, HTTP_STATUS_NOT_EXTENDED: 510,
  HTTP_STATUS_NETWORK_AUTHENTICATION_REQUIRED: 511,
};
const __api = {
  createServer, createSecureServer, connect, constants,
  getDefaultSettings, getPackedSettings, getUnpackedSettings, sensitiveHeaders,
};
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
