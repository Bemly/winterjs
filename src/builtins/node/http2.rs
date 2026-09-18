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
// node checkIsHttpToken 同款（header 名校验）
const __TOKEN_RE = /^[\^_`a-zA-Z\-0-9!#$%&'*+.|~]+$/;
function __validateHeaderName(name) {
  if (typeof name !== "string") {
    throw new codes.ERR_INVALID_ARG_TYPE("name", "string", name);
  }
  if (!__TOKEN_RE.test(name)) {
    throw new codes.ERR_INVALID_HTTP_TOKEN(name);
  }
}
function __validateHeaderValue(name, value) {
  if (value === undefined || value === null) {
    throw new codes.ERR_HTTP2_INVALID_HEADER_VALUE(String(value), name);
  }
  if (Array.isArray(value)) {
    for (const v of value) __validateHeaderValue(name, v);
    return;
  }
  if (typeof value !== "string" && typeof value !== "number" && typeof value !== "boolean") {
    throw new codes.ERR_INVALID_ARG_TYPE(`header "${name}"`, "string|number|boolean", value);
  }
}
function __headerToWire(value) {
  if (Array.isArray(value)) return value.map((v) => String(v));
  return String(value);
}

// ── socket 代理（node socketProxyPair 口径：net.Socket 假面 + 流委派）────────
// read/write/pause/resume 及其 setter 抛 ERR_HTTP2_NO_SOCKET_MANIPULATION；
// on/once/emit/end/destroy 转发给流；setTimeout 转发给 session；未知属性落
// 会话级 bag（session.socket._isProcessing 直写直读，socket-set 套件口径）。
const __SOCK_MANIP_KEYS = ["read", "write", "pause", "resume"];
function __socketManipErr() {
  return __h2Err("ERR_HTTP2_NO_SOCKET_MANIPULATION",
    "HTTP/2 sockets should not be directly manipulated (e.g. read and written)");
}
function __mkSocketProxy(stream, server, peerObj) {
  const base = Object.create(net.Socket.prototype);
  // net.Socket 原型 connecting 为只读 getter：自有数据属性遮蔽（可写）。
  Object.defineProperty(base, "connecting", { value: false, writable: true, configurable: true });
  const handler = {
    get(t, prop) {
      if (prop === "readable" || prop === "writable") return stream[prop];
      if (prop === "destroyed") return stream.__destroyed;
      if (__SOCK_MANIP_KEYS.includes(prop)) throw __socketManipErr();
      if (prop === "on" || prop === "once" || prop === "emit" ||
          prop === "end" || prop === "destroy") {
        return stream[prop].bind(stream);
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

// ── 服务端流对象（node ServerHttp2Stream 子集）──────────────────────────────
class Http2ServerStream extends EventEmitter {
  constructor(server, connId, id) {
    super();
    this.__server = server;
    this.__conn = connId;
    this.id = id;
    this.session = server;
    this.readable = true;
    this.writable = true;
    this.__destroyed = false;
    this.__closed = false;
    this.__aborted = false;
    this.__req = null;
    this.__res = null;
  }
  get destroyed() { return this.__destroyed; }
  get aborted() { return this.__aborted; }
  __attach(req, res) { this.__req = req; this.__res = res; }
  __closeOnce() {
    if (this.__closed) return;
    this.__closed = true;
    this.readable = false;
    this.writable = false;
    this.__destroyed = true;
    queueMicrotask(() => this.emit("close"));
  }
  // 下游 abort：req 'aborted' + res 静默销毁（§4.52：先子域后终结）
  __abortDownstream() {
    if (this.__aborted) return;
    this.__aborted = true;
    if (this.__req) this.__req.__onAborted();
    if (this.__res && !this.__res.destroyed) this.__res.__destroySilent();
  }
  // 会话终结/连接死亡路径（Rust aborted 事件、server close）
  __abort() {
    if (this.__closed) return;
    this.__abortDownstream();
    this.__closeOnce();
  }
  close(code = 0, cb) {
    if (typeof code === "function") { cb = code; code = 0; }
    if (this.__closed) {
      if (typeof cb === "function") queueMicrotask(cb);
      return this;
    }
    __wjs_h2_reset(this.__conn, this.id, code);
    if (typeof cb === "function") this.once("close", cb);
    this.__closeOnce();
    return this;
  }
  destroy(err) {
    if (this.__closed) return this;
    if (err && this.listenerCount("error") > 0) this.emit("error", err);
    __wjs_h2_reset(this.__conn, this.id, err ? 2 : 0);
    this.__abortDownstream();
    this.__closeOnce();
    return this;
  }
}

// ── Http2ServerRequest（Readable；auto-flow node 口径）──────────────────────
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
    this.__paused = false;
    // 伪头合成：method/path/authority/scheme；顺序 = 客户端提示头 > node 口径
    //（method, path, authority, scheme）。提示头由本仓客户端在伪头序≠hyper
    // 线序时内联（x-wjs-pho），服务端摘除不入 headers/rawHeaders。
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
    // node compat：无 data 监听也消费（'end' 必发）；用户已显式 pause 则尊重。
    // 微任务延迟到 handler 同步段之后（真机 req.readableFlowing 入口为 null）。
    queueMicrotask(() => { if (!this.__paused) this.resume(); });
  }
  get stream() { return this.__stream; }
  get socket() { return this.__socket; }
  get connection() { return this.__socket; }
  get method() { return this.headers[":method"]; }
  set method(v) {
    if (typeof v !== "string") throw new codes.ERR_INVALID_ARG_TYPE("method", "string", v);
    if (!__TOKEN_RE.test(v)) throw new codes.ERR_INVALID_ARG_VALUE("method", v);
    this.headers[":method"] = v;
  }
  get scheme() { return this.headers[":scheme"]; }
  set scheme(v) {
    if (typeof v !== "string") throw new codes.ERR_INVALID_ARG_TYPE("scheme", "string", v);
    this.headers[":scheme"] = v;
  }
  get authority() { return this.headers[":authority"] ?? this.headers.host; }
  set authority(v) {
    if (typeof v !== "string") throw new codes.ERR_INVALID_ARG_TYPE("authority", "string", v);
    this.headers[":authority"] = v;
  }
  get url() { return this.__url; }
  set url(v) { this.__url = v; }
  _read() {}
  pause() { this.__paused = true; return super.pause(); }
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
  __complete() {
    if (this.complete) return;
    this.complete = true;
    this.push(null);
  }
  __onBody(b64chunk) {
    const u8 = __b64dec(b64chunk ?? "");
    if (u8.length > 0) this.push(Buffer.from(u8));
  }
  __onReqEnd(trailersJson) {
    const t = JSON.parse(trailersJson ?? "[]");
    this.trailers = __pairsToObj(t);
    this.rawTrailers = t.flat();
    this.__complete();
  }
  __onAborted() {
    if (this.aborted) return;
    this.aborted = true;
    this.complete = true;
    this.emit("aborted");
    this.destroy();
  }
}

// ── Http2ServerResponse（Writable；体经 native 增量下发）────────────────────
class Http2ServerResponse extends Writable {
  constructor(req, stream) {
    super({ autoDestroy: true });
    this.req = req;
    this.__stream = stream;
    this.__conn = stream.__conn;
    this.__id = stream.id;
    this.statusCode = 200;
    this.statusMessage = undefined;
    this.__headers = Object.create(null);
    this.__trailers = Object.create(null);
    this.headersSent = false;
    this.sendDate = true;
    this.__finishEmitted = false;
    this.once("finish", () => { this.__finishEmitted = true; });
    stream.__attach(req, this);
  }
  get stream() { return this.__stream; }
  get socket() { return this.__stream.__destroyed ? undefined : this.req.socket; }
  get connection() { return this.socket; }
  get finished() { return this.writableEnded; }
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
  setTrailer(name, value) {
    __validateHeaderName(name);
    __validateHeaderValue(name, value);
    this.__trailers[String(name).toLowerCase()] = value;
    return this;
  }
  addTrailers(obj) {
    for (const [k, v] of Object.entries(obj ?? {})) this.setTrailer(k, v);
    return this;
  }
  writeHead(status, ...rest) {
    if (this.headersSent) throw __h2Err("ERR_HTTP2_HEADERS_SENT", "Response has already been initiated.");
    if (typeof status !== "number" || !Number.isInteger(status) || status < 100 || status > 599) {
      throw new codes.ERR_INVALID_ARG_VALUE("status", status, "is not a valid HTTP status code");
    }
    this.statusCode = status;
    for (const r of rest) {
      if (typeof r === "string") this.statusMessage = r;
      else if (Array.isArray(r)) {
        for (let i = 0; i + 1 < r.length; i += 2) this.setHeader(r[i], r[i + 1]);
      } else if (r !== null && typeof r === "object") {
        for (const [k, v] of Object.entries(r)) this.setHeader(k, v);
      }
    }
    return this;
  }
  flushHeaders() {
    if (this.headersSent || this.__stream.__destroyed) return;
    this.__sendHead();
  }
  __sendHead() {
    if (this.headersSent || this.__stream.__destroyed) return;
    this.headersSent = true;
    const entries = [];
    for (const [k, v] of Object.entries(this.__headers)) {
      if (k.startsWith(":")) continue; // 伪头不入线（:status 由 status 承载）
      const w = __headerToWire(v);
      if (Array.isArray(w)) for (const one of w) entries.push([k, one]);
      else entries.push([k, w]);
    }
    __wjs_h2_respond(this.__conn, this.__id, this.statusCode, JSON.stringify(entries));
  }
  _write(chunk, encoding, cb) {
    if (this.__stream.__destroyed) {
      // node 口径：首个 write 回 ERR_HTTP2_INVALID_STREAM，后续 falsy + 无错 cb
      if (!this.__writeErrored) {
        this.__writeErrored = true;
        if (typeof cb === "function") cb(__h2Err("ERR_HTTP2_INVALID_STREAM", "The stream has been destroyed."));
      } else if (typeof cb === "function") {
        cb();
      }
      return false;
    }
    const u8 = chunk instanceof Uint8Array ? chunk : __toU8(String(chunk), "write");
    this.__sendHead();
    __wjs_h2_data(this.__conn, this.__id, __b64enc(u8));
    if (typeof cb === "function") cb();
    return true;
  }
  _final(cb) {
    if (!this.__stream.__destroyed) {
      this.__sendHead();
      const t = [];
      for (const [k, v] of Object.entries(this.__trailers)) {
        const w = __headerToWire(v);
        if (Array.isArray(w)) for (const one of w) t.push([k, one]);
        else t.push([k, w]);
      }
      __wjs_h2_end(this.__conn, this.__id, JSON.stringify(t));
      this.__stream.__closeOnce();
    }
    cb();
  }
  // node 口径：destroy 恒发 finish（真机实测：clean/err 均先 finish 后 close），
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
  __destroySilent() {
    if (this.destroyed) return;
    super.destroy();
  }
  createPushResponse(cb) {
    if (typeof cb !== "function") throw new codes.ERR_INVALID_ARG_TYPE("callback", "Function", cb);
    queueMicrotask(() => cb(__h2Err("ERR_HTTP2_PUSH_DISABLED", "Push streams are not enabled.")));
    return undefined;
  }
  // 1xx/informational：本仓 h2 无 informational 帧（偏差记档）——校验后吞掉。
  writeContinue(cb) { if (typeof cb === "function") queueMicrotask(cb); return this; }
  writeInformation(type, info, cb) {
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
    this.__id = 0;
    this.__listening = null;
    this.__secure = !!secure;
    this.__tlsOpts = null;
    this.__streams = new Map();
    this.__sockBag = {};
    if (options && typeof options === "object") {
      if (secure) {
        if (options.key === undefined || options.cert === undefined) {
          throw new TypeError("http2.createSecureServer needs { key, cert } PEM strings");
        }
        this.__tlsOpts = { tls: { key: String(options.key), cert: String(options.cert) } };
      }
    }
    // request 监听器只由 createServer/createSecureServer 接线（构造器不再重复注册，
    // §4.39）；派发钩子预绑定（dispatch 以 global 为 this，§4.34）
    this.__ev = this.__ev.bind(this);
  }
  get socket() { return this.__sockBag; }
  setTimeout(msecs, callback) {
    if (typeof callback === "function") this.once("timeout", callback);
    return this;
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
        const stream = new Http2ServerStream(this, Number(o.connId), id);
        const sock = __mkSocketProxy(stream, this, peerObj);
        const scheme = this.__secure ? "https" : "http";
        const req = new Http2ServerRequest(stream, o, scheme, sock);
        const res = new Http2ServerResponse(req, stream);
        this.__streams.set(id, { stream, req, res });
        stream.once("close", () => this.__streams.delete(id));
        const method = req.headers[":method"];
        if (method === "CONNECT") {
          // node：CONNECT 走 'connect' 事件（不入 request）；无监听 501 兜底防挂起
          //（node RST REFUSED_STREAM 口径偏差记档）
          if (this.listenerCount("connect") > 0) this.emit("connect", req, res);
          else { res.statusCode = 501; res.end(); }
        } else {
          // node 顺序：'stream'（raw）先于 'request'（compat）
          this.emit("stream", stream, req.headers, 5, req.rawHeaders);
          this.emit("request", req, res);
          req.__onBody(o.body ?? "");
          req.__onReqEnd(o.trailers ?? "[]");
        }
        break;
      }
      case "body": {
        const o = JSON.parse(payload);
        const e = this.__streams.get(Number(o.streamId));
        if (e) e.req.__onBody(o.payload ?? "");
        break;
      }
      case "reqEnd": {
        const o = JSON.parse(payload);
        const e = this.__streams.get(Number(o.streamId));
        if (e) e.req.__onReqEnd(o.payload ?? "[]");
        break;
      }
      case "aborted": {
        const o = JSON.parse(payload);
        this.__abortEntry(this.__streams.get(Number(o.streamId)));
        break;
      }
      case "error": {
        const o = JSON.parse(payload);
        this.emit("error", __h2Err(o.code, o.msg));
        break;
      }
      case "close": {
        // 先静默摘除全部在途流（req aborted + res destroy），再发 server 'close'
        //（§4.52 顺序；Rust conn aborted 事件竞态由 __abortEntry 幂等兜底）
        for (const [, entry] of this.__streams) entry.stream.__abort();
        this.__streams.clear();
        this.emit("close");
        break;
      }
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
// 开流时机：无体语义方法（GET/HEAD 等）在 request() 即开（空体 → END_STREAM
// 随头出线，服务端请求事件即时）；POST/PUT/PATCH 整收上传口径——end() 才下发
// 全量体（偏差记档：node 头即刻出线、体流式）。waitForTrailers → end 后发
// 'wantTrailers'，sendTrailers 补 trailer 帧。
const __DEFERRED_METHODS = new Set(["POST", "PUT", "PATCH"]);
class ClientHttp2Stream extends Duplex {
  constructor(session, id, headers, options) {
    super();
    this.__session = session;
    this.id = id;
    this.sentHeaders = headers;
    this.__ended = false;
    // Duplex 基类 closed/destroyed 为只读 getter（state 位图），禁直接赋值；
    // 自有 aboard 旗用 __ 前缀（服务端 Http2ServerStream 同口径）。
    this.aborted = false;
    this.__opened = false;
    this.__deferred = __DEFERRED_METHODS.has(String(headers[":method"]));
    this.__waitTrailers = !!(options && options.waitForTrailers);
    this.__pendingBody = [];
  }
  get session() { return this.__session; }
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
      // 即时开流方法（GET 系）END_STREAM 已随头出线，写即错（偏差记档）
      cb(__h2Err("ERR_STREAM_WRITE_AFTER_END", "write after end"));
      return;
    }
    const u8 = chunk instanceof Uint8Array ? chunk : __toU8(String(chunk), "write");
    this.__pendingBody.push(u8);
    cb();
  }
  _final(cb) {
    this.__openNow(null);
    cb();
  }
  sendTrailers(trailers) {
    if (!this.__opened || !this.__waitTrailers) {
      throw __h2Err("ERR_HTTP2_TRAILERS_CANNOT_BE_SENT", "Trailers cannot be sent at this stage.");
    }
    const t = [];
    for (const [k, v] of Object.entries(trailers ?? {})) t.push([k, String(v)]);
    __wjs_h2_open_trailers(this.__session.__id, this.id, JSON.stringify(t));
    return this;
  }
  __onResponse(headers, flags) { this.emit("response", headers, flags); }
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
  close(code, cb) {
    if (typeof code === "function") { cb = code; code = 0; }
    // closed/destroyed 只读：以 destroyed 判幂等，destroy() 置底层位。
    if (!this.destroyed) {
      if (typeof cb === "function") this.once("close", cb);
      this.destroy();
    } else if (typeof cb === "function") {
      queueMicrotask(cb);
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
    // 派发钩子预绑定（§4.34）
    this.__ev = this.__ev.bind(this);
  }
  get socket() { return undefined; }
  get alpnProtocol() { return null; }
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
    if (h[":path"] === undefined && h[":method"] !== "CONNECT") h[":path"] = "/";
    if (h[":authority"] === undefined && h.host === undefined) h[":authority"] = this.__authority;
    if (h[":scheme"] === undefined) h[":scheme"] = this.__secure ? "https" : "http";
    // 伪头序提示：hyper 线序固定（method,scheme,authority,path），用户序≠线序时
    // 内联提示头供本仓服务端还原（node 客户端保对象序，跨服务端偏差记档）
    const pseudoKeys = Object.keys(h).filter((k) => k.startsWith(":"));
    const canonical = [":method", ":scheme", ":authority", ":path"];
    const differs = pseudoKeys.length !== canonical.length ||
      pseudoKeys.some((k, i) => k !== canonical[i]);
    if (differs) h["x-wjs-pho"] = pseudoKeys.join(",");
    const id = this.__seq;
    this.__seq += 2; // 客户端单数流（RFC 7540 口径）
    const st = new ClientHttp2Stream(this, id, h, options);
    this.__streams.set(id, st);
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
  NGHTTP2_NO_ERROR: 0, NGHTTP2_PROTOCOL_ERROR: 1, NGHTTP2_INTERNAL_ERROR: 2,
  NGHTTP2_FLOW_CONTROL_ERROR: 3, NGHTTP2_SETTINGS_TIMEOUT: 4, NGHTTP2_STREAM_CLOSED: 5,
  NGHTTP2_FRAME_SIZE_ERROR: 6, NGHTTP2_REFUSED_STREAM: 7, NGHTTP2_CANCEL: 8,
  NGHTTP2_COMPRESSION_ERROR: 9, NGHTTP2_CONNECT_ERROR: 10, NGHTTP2_ENHANCE_YOUR_CALM: 11,
  NGHTTP2_INADEQUATE_SECURITY: 12, NGHTTP2_HTTP_1_1_REQUIRED: 13,
  HTTP2_HEADER_STATUS: ":status", HTTP2_HEADER_METHOD: ":method",
  HTTP2_HEADER_PATH: ":path", HTTP2_HEADER_SCHEME: ":scheme",
  HTTP2_HEADER_AUTHORITY: ":authority", HTTP2_HEADER_CONTENT_TYPE: "content-type",
  HTTP2_HEADER_DATE: "date", HTTP2_HEADER_USER_AGENT: "user-agent",
  HTTP2_HEADER_ACCEPT: "accept", HTTP2_HEADER_HOST: "host",
  HTTP2_HEADER_CONTENT_LENGTH: "content-length",
  HTTP2_HEADER_SET_COOKIE: "set-cookie", HTTP2_HEADER_COOKIE: "cookie",
  HTTP2_HEADER_AUTHORIZATION: "authorization", HTTP2_HEADER_LOCATION: "location",
  HTTP2_HEADER_CONNECTION: "connection", HTTP2_HEADER_TRANSFER_ENCODING: "transfer-encoding",
  HTTP2_HEADER_UPGRADE: "upgrade", HTTP2_HEADER_KEEP_ALIVE: "keep-alive",
  HTTP2_HEADER_PROXY_CONNECTION: "proxy-connection", HTTP2_HEADER_TE: "te",
  HTTP2_HEADER_TRAILER: "trailer", HTTP2_HEADER_HTTP2_SETTINGS: "http2-settings",
  HTTP2_HEADER_ACCEPT_ENCODING: "accept-encoding", HTTP2_HEADER_ACCEPT_LANGUAGE: "accept-language",
  HTTP2_HEADER_ACCEPT_CHARSET: "accept-charset", HTTP2_HEADER_CACHE_CONTROL: "cache-control",
  HTTP2_HEADER_ETAG: "etag", HTTP2_HEADER_EXPIRES: "expires", HTTP2_HEADER_RETRY_AFTER: "retry-after",
  HTTP2_HEADER_VIA: "via", HTTP2_HEADER_WWW_AUTHENTICATE: "www-authenticate",
  HTTP2_METHOD_GET: "GET", HTTP2_METHOD_POST: "POST", HTTP2_METHOD_HEAD: "HEAD",
  HTTP2_METHOD_PUT: "PUT", HTTP2_METHOD_DELETE: "DELETE", HTTP2_METHOD_PATCH: "PATCH",
  HTTP2_METHOD_OPTIONS: "OPTIONS", HTTP2_METHOD_CONNECT: "CONNECT", HTTP2_METHOD_TRACE: "TRACE",
  HTTP_STATUS_CONTINUE: 100, HTTP_STATUS_OK: 200, HTTP_STATUS_NO_CONTENT: 204,
  HTTP_STATUS_RESET_CONTENT: 205, HTTP_STATUS_NOT_MODIFIED: 304,
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
