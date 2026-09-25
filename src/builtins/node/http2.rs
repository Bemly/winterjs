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
pub(crate) struct Exec;

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
pub(crate) struct TokioIo<T>(T);

impl<T> TokioIo<T> {
    pub(crate) fn new(inner: T) -> Self {
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

pub(crate) fn b64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub(crate) fn b64dec(s: &str) -> Result<Vec<u8>, String> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| format!("H2 protocol: bad base64 ({e})"))
}

/// HeaderMap → JSON `[[k, v], …]`（值按 latin1 语义 lossy 还原）。
pub(crate) fn headers_json(headers: &http::HeaderMap) -> String {
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
pub(crate) struct H2Resp {
    status: u16,
    headers: Vec<(String, String)>,
}

/// 体通道消息（10f 流式应答/上传共用）。
/// End = 体尽即 EOS（无/含 trailer）；EndPending = 体尽但流不闭（等 trailer 命令）。
pub(crate) enum BodyMsg {
    Data(bytes::Bytes),
    End(Option<Vec<(String, String)>>),
    EndPending,
    Fail(String),
}

type Responders = Arc<tokio::sync::Mutex<HashMap<u64, tokio::sync::oneshot::Sender<H2Resp>>>>;
pub(crate) type BodyFeeds = Arc<tokio::sync::Mutex<HashMap<u64, tokio::sync::mpsc::UnboundedSender<BodyMsg>>>>;
/// 早到的体块暂存（头未应答前 data/end 先到；头应答时按序回放）。
type Pendings = Arc<tokio::sync::Mutex<HashMap<u64, Vec<BodyMsg>>>>;
/// 已显式 RST 的流（service 回 Err → hyper RST INTERNAL_ERROR）。
type DeadStreams = Arc<std::sync::Mutex<HashSet<u64>>>;
/// 已收尾（end 发出）的流——连接退出时不再报 aborted。
type EndedStreams = Arc<std::sync::Mutex<HashSet<u64>>>;

/// mpsc 供体 body（hyper `http_body::Body` 手写 30 行）。
/// 单任务 select 轮询：body Pending 不需 waker（cmd 臂推进后 conn 臂重轮询）。
pub(crate) struct ChanBody {
    pub(crate) rx: tokio::sync::mpsc::UnboundedReceiver<BodyMsg>,
    pub(crate) done: bool,
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
            // 悬置标记：继续轮询以登记 waker（直接回 Pending 而不再 poll_recv 就没人唤醒——
            // 有体 + waitForTrailers 时 trailer 永不出线，两端互等挂死）。
            Poll::Ready(Some(BodyMsg::EndPending)) => self.poll_frame(cx),
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

pub(crate) fn set_rval_str(cx: &mut JSContext, frame: &Frame, s: &str) {
    rooted!(&in(cx) let mut v = UndefinedValue());
    s.to_jsval(cx, v.handle_mut());
    frame.set_rval(v.get());
}

pub(crate) fn opt_num(frame: &Frame, i: u32) -> Option<f64> {
    let v = frame.arg(i);
    if v.is_number() { Some(v.to_number()) } else { None }
}

pub(crate) fn ensure_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// h2 Reason → node nghttp2 常量名（错误消息用）。
fn nghttp2_reason_name(r: h2::Reason) -> &'static str {
    match r {
        h2::Reason::NO_ERROR => "NGHTTP2_NO_ERROR",
        h2::Reason::PROTOCOL_ERROR => "NGHTTP2_PROTOCOL_ERROR",
        h2::Reason::INTERNAL_ERROR => "NGHTTP2_INTERNAL_ERROR",
        h2::Reason::FLOW_CONTROL_ERROR => "NGHTTP2_FLOW_CONTROL_ERROR",
        h2::Reason::SETTINGS_TIMEOUT => "NGHTTP2_SETTINGS_TIMEOUT",
        h2::Reason::STREAM_CLOSED => "NGHTTP2_STREAM_CLOSED",
        h2::Reason::FRAME_SIZE_ERROR => "NGHTTP2_FRAME_SIZE_ERROR",
        h2::Reason::REFUSED_STREAM => "NGHTTP2_REFUSED_STREAM",
        h2::Reason::CANCEL => "NGHTTP2_CANCEL",
        h2::Reason::COMPRESSION_ERROR => "NGHTTP2_COMPRESSION_ERROR",
        h2::Reason::CONNECT_ERROR => "NGHTTP2_CONNECT_ERROR",
        h2::Reason::ENHANCE_YOUR_CALM => "NGHTTP2_ENHANCE_YOUR_CALM",
        h2::Reason::INADEQUATE_SECURITY => "NGHTTP2_INADEQUATE_SECURITY",
        h2::Reason::HTTP_1_1_REQUIRED => "NGHTTP2_HTTP_1_1_REQUIRED",
        _ => "NGHTTP2_INTERNAL_ERROR",
    }
}

/// hyper/h2 错误 → node 口径消息 + RST 原因码（node 客户端流 `rstCode` 需要）。
pub(crate) fn h2_err_msg_rst(e: &hyper::Error) -> (String, Option<u32>) {
    let mut src: Option<&(dyn std::error::Error + 'static)> = Some(e);
    while let Some(err) = src {
        if let Some(h2e) = err.downcast_ref::<h2::Error>() {
            if let Some(reason) = h2e.reason() {
                return (
                    format!("Stream closed with error code {}", nghttp2_reason_name(reason)),
                    Some(u32::from(reason)),
                );
            }
        }
        src = err.source();
    }
    (format!("{e}"), None)
}

// ── 服务端 ──────────────────────────────────────────────────────────────────

/// 读全请求体（整收口径；trailer 一并收集。错即 Err）。
pub(crate) async fn read_body(
    body: hyper::body::Incoming,
) -> Result<(Vec<u8>, Vec<(String, String)>), (String, Option<u32>)> {
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
            Some(Err(e)) => return Err(h2_err_msg_rst(&e)),
        }
    }
}

/// 服务端请求体泵：逐帧转 `body`（b64 块）/ `reqEnd`（trailers JSON）流事件；
/// 对端 RST 等中断转 `aborted`（payload 为 RST 码，JS 侧落 `stream.rstCode`）。
async fn pump_request_body(
    body: hyper::body::Incoming,
    ev_tx: tokio::sync::mpsc::UnboundedSender<NetEvent>,
    server_id: u64,
    stream_id: u64,
) {
    use http_body::Body as _;
    use std::future::poll_fn;
    let mut body = body;
    let send = |what: &str, payload: String| {
        let _ = ev_tx.send(NetEvent {
            id: server_id,
            kind: NetKind::H2Stream { stream_id, what: what.into(), payload },
        });
    };
    let mut trailers: Vec<(String, String)> = Vec::new();
    loop {
        match poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await {
            None => break,
            Some(Ok(f)) => {
                if let Some(d) = f.data_ref() {
                    if !d.is_empty() {
                        send("body", b64(d));
                    }
                }
                if let Some(t) = f.trailers_ref() {
                    for (k, v) in t.iter() {
                        trailers.push((k.as_str().to_owned(), String::from_utf8_lossy(v.as_bytes()).into_owned()));
                    }
                }
            }
            Some(Err(e)) => {
                let (_msg, rst) = h2_err_msg_rst(&e);
                send("aborted", rst.map(|c| c.to_string()).unwrap_or_default());
                return;
            }
        }
    }
    send("reqEnd", serde_json::to_string(&trailers).unwrap_or_else(|_| "[]".into()));
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
pub(crate) async fn serve_conn<IO>(
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
    // 连接死亡广播：service 的应答等待在 conn 亡后即退出（否则 hyper 等待
    // 未完成响应 → conn 永不结束 → 客户端死连接泄漏，服务端流 'close' 不到）
    let (_conn_dead_tx, conn_dead_rx) = tokio::sync::watch::channel(false);
    let svc = {
        let responders = responders.clone();
        let bodies = bodies.clone();
        let pendings = pendings.clone();
        let dead = dead.clone();
        let dead_clean = dead_clean.clone();
        let seq = seq.clone();
        let ev_tx = ev_tx.clone();
        let conn_dead_base = conn_dead_rx.clone();
        hyper::service::service_fn(move |req: http::Request<hyper::body::Incoming>| {
            let responders = responders.clone();
            let bodies = bodies.clone();
            let pendings = pendings.clone();
            let dead = dead.clone();
            let dead_clean = dead_clean.clone();
            let seq = seq.clone();
            let ev_tx = ev_tx.clone();
            let mut conn_dead = conn_dead_base.clone();
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
                // P1（2026-09-25）：请求体流式——头到即派发 'stream'/'request'，体帧逐块
                // 以 `body`/`reqEnd` 流事件跟进（修前整收：读完整个请求体才派发，客户端
                // 等服务端应答/wantTrailers 再收尾的交互形全挂死）。头带 END_STREAM 的
                // 空体请求照旧一次派发（flags/endAfterHeaders 口径不变）。
                let body_in = req.into_body();
                let streaming = !http_body::Body::is_end_stream(&body_in);
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
                        trailers_json: "[]".into(),
                        body_b64: String::new(),
                        peer: peer.to_string(),
                        streaming,
                    },
                });
                if streaming {
                    let ev_body = ev_tx.clone();
                    tokio::spawn(pump_request_body(body_in, ev_body, server_id, stream_id));
                }
                // JS 不应答即挂起（记档）；reset 已发则 service 回 Err（hyper RST），
                // 干净关（NO_ERROR）回 200 空体（偏差记档：body API 无法 RST NO_ERROR）。
                let head = match tokio::select! {
                    r = rx => r,
                    _ = conn_dead.changed() => {
                        return Err::<http::Response<ChanBody>, anyhow::Error>(anyhow::anyhow!("h2 connection closed"));
                    }
                } {
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
                        // 通知 server 侧流被 RST（rstCode 语义；JS 侧据此置 rstCode）
                        let _ = ev_tx.send(NetEvent {
                            id: server_id,
                            kind: NetKind::H2Stream {
                                stream_id,
                                what: "aborted".into(),
                                payload: code.to_string(),
                            },
                        });
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
                    Some(NetCmd::Close) | None => {
                        tracing::debug!(target: "winterjs::http2", conn_id, "serve_conn: Close cmd");
                        break;
                    }
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
            id: conn_id,
            kind: NetKind::H2Stream {
                stream_id,
                what: "aborted".into(),
                payload: String::new(),
            },
        });
    }
    // 会话终结通知（借 H2Stream 通道：what="connClose"，payload 带 conn_id；
    // JS 侧 server 收到后收尾对应 Http2Session 并发 'close'）。
    tracing::debug!(target: "winterjs::http2", conn_id, "tail target_present={}", state::net_target(conn_id).is_some());
    let _ = ev_tx.send(NetEvent {
        id: conn_id,
        kind: NetKind::H2Stream {
            stream_id: 0,
            what: "connClose".into(),
            payload: conn_id.to_string(),
        },
    });
    // conn 的 net_purge 由 net.rs 在 connClose 派发后执行（此 purge 会连
    // net_target 一起删——先删则排队的 connClose/aborted 事件全部丢路由）
}

pub use super::http2_client::h2_connect;
pub use super::http2_server::h2_listen;


/// task 内错误上报（Error + 单次 Close；native 已返回，无 Frame 可用）。
pub(crate) fn report_error_static(
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

/// 内嵌 ESM 源（`node:http2`；§0.9 按域分块：`h2_head.js` 基础 +
/// `h2_server.js` 服务端 + `h2_client.js` 客户端，concat 字节恒等）。
pub const SOURCE: &str = concat!(
    include_str!("h2_head.js"),
    include_str!("h2_server.js"),
    include_str!("h2_server_http2server.js"),
    include_str!("h2_client.js"),
);

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
