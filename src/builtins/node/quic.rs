//! `node:quic`：Endpoint + 会话（9g-1，QUIC 落地；流/数据报见 9g-2）。
//!
//! 落法：quinn 0.11 直驱（`runtime-tokio` 特性跑在本会话 current-thread
//! tokio 上；TLS 走 rustls/ring，自签与 tls.rs 同口径）。事件模型与 net 同构
//! （task → `quic_rx` → 事件循环 `dispatch` → 预绑定的 `__ev`；收尾单出口，
//! checklist §4.36）：endpoint accept 任务逐个握手派 `EndpointSession`；
//! 客户端 connect 任务派 `SessionSecure` 后兼 `closed()` 守望派 `SessionClose`。
//! 计数：监听中 endpoint + 存活会话（`quic_open`，退出条件）。
//!
//! 握手失败：客户端→ `SessionError`（`ERR_QUIC_HANDSHAKE` 具名错）+ `SessionClose{-1}`；
//! 服务端 accept/握手失败静默丢弃（扫描流量不能杀服务，记档）。
//!
//! 偏差记档（9g-1）：
//! - ALPN 必选（QUIC 无"无 ALPN"情形，Node 同款强制）；服务端数组、客户端单串
//!   （单元素数组宽容为该串）。
//! - 服务端 key/cert PEM 必给（fail fast TypeError）；客户端证书不支持；
//!   `ca` 缺省走系统 roots（rustls-native-certs），`rejectUnauthorized:false`
//!   跳校验（NoVerifier，tls.rs 同款）；`servername` 缺省取 host。
//! - 无 `ping()`（quinn 0.11 无 PING 原语）；`stats()` 给真值子集
//!  （rttMs/udp 收发字节/包 + 路径 cwnd/loss，余下 Node 字段不列）；
//!   endpoint 无 stats；`updateKey`/`setSNIContexts`/qlog/keylog/ticket/token/
//!   0-RTT/early-data/headers/trailers/priority/goaway 全不支持。
//! - `close()` 同步调、异步到（`'close'` 事件为准）；`destroy()` 同 `close()`。
//! - `DEFAULT_CIPHERS`/`DEFAULT_GROUPS` 不导出（套件选择不支持，无真值可给）；
//!   `CC_ALGO_*` 三档真映射（reno/cubic/bbr）；`idleTimeout` 毫秒（0/缺省=不限）。
//! 偏差记档（9i-9 H3 分支）：
//! - 真机 node 26.8.2 无 node:quic 模块（`--experimental-quic` 亦无），headers 面
//!   为**本仓自定 API**：ALPN 含 "h3" 的会话走 H3 驱动——服务端 `sess.on("request",
//!   (req) => req.respond({status, headers, body}))`，客户端 `sess.request(opts)`
//!   回一次性 Promise；非 h3 会话 request() 即 ERR_INVALID_PROTOCOL。
//! - H3 会话独占连接：无裸流/datagram 事件；请求/响应**串行**处理（v1 记档）；
//!   响应头经 serde_json Map（键字典序）；请求体服务端整收后才发 request 事件
//!   （§4.35 同口径）。会话关闭时未决请求 promise 拒绝（ERR_QUIC_SESSION_CLOSED）。

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use mozjs::context::JSContext;
use mozjs::jsapi::JSObject;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use base64::Engine as _;

use crate::jsapi_glue::{get_prop_value, report_error, value_to_string, wrap_cx, Frame};
use crate::state;

/// 监听选项（JS 侧校验形态，Rust 侧解析；alpn 非空由 JS 保证，Rust 再断言）。
#[derive(serde::Deserialize)]
struct ListenOpts {
    host: String,
    port: u16,
    alpn: Vec<String>,
    key_pem: String,
    cert_pem: String,
    #[serde(default)]
    idle_timeout_ms: Option<u64>,
    #[serde(default)]
    cc: Option<String>,
}

/// 连接选项（alpn 单串；servername 缺省取 host）。
#[derive(serde::Deserialize)]
struct ConnectOpts {
    host: String,
    port: u16,
    alpn: String,
    #[serde(default)]
    servername: Option<String>,
    #[serde(default)]
    ca_pem: Option<String>,
    #[serde(default)]
    reject_unauthorized: Option<bool>,
    #[serde(default)]
    idle_timeout_ms: Option<u64>,
    #[serde(default)]
    cc: Option<String>,
}

/// 验签跳过（tls.rs `NoVerifier` 同款；`rejectUnauthorized:false` 用）。
#[derive(Debug)]
struct NoVerifier;

impl rustls::client::danger::ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider().signature_verification_algorithms.supported_schemes()
    }
}

/// 传输配置（idle 超时 + 拥塞控制三档；`cc` 非法即 Err）。
fn build_transport(idle_timeout_ms: Option<u64>, cc: Option<&str>) -> Result<Arc<quinn::TransportConfig>, String> {
    let mut t = quinn::TransportConfig::default();
    match idle_timeout_ms {
        Some(0) | None => {}
        Some(ms) => {
            let to = quinn::IdleTimeout::try_from(Duration::from_millis(ms))
                .map_err(|_| "TypeError: options.idleTimeout out of range".to_string())?;
            t.max_idle_timeout(Some(to));
        }
    }
    match cc {
        None => {}
        Some("reno") => {
            t.congestion_controller_factory(Arc::new(quinn::congestion::NewRenoConfig::default()));
        }
        Some("cubic") => {
            t.congestion_controller_factory(Arc::new(quinn::congestion::CubicConfig::default()));
        }
        Some("bbr") => {
            t.congestion_controller_factory(Arc::new(quinn::congestion::BbrConfig::default()));
        }
        Some(other) => return Err(format!("TypeError: options.cc must be reno/cubic/bbr (got {other})")),
    }
    Ok(Arc::new(t))
}

/// 服务端 TLS（PEM 双件；零证书/坏 key 即 fail fast，§4.38 同口径）。
fn build_server_tls(key_pem: &str, cert_pem: &str, alpn: &[Vec<u8>]) -> Result<rustls::ServerConfig, String> {
    if key_pem.is_empty() || cert_pem.is_empty() {
        return Err("TypeError: listen needs options.key and options.cert (PEM)".into());
    }
    let certs: Vec<rustls::pki_types::CertificateDer<'static>> =
        rustls_pemfile::certs(&mut cert_pem.as_bytes())
            .collect::<Result<_, _>>()
            .map_err(|_| "TypeError: options.cert is not valid PEM".to_string())?;
    if certs.is_empty() {
        return Err("TypeError: options.cert has no certificate".into());
    }
    let key = rustls_pemfile::private_key(&mut key_pem.as_bytes())
        .map_err(|_| "TypeError: options.key is not valid PEM".to_string())?
        .ok_or_else(|| "TypeError: options.key has no private key".to_string())?;
    let mut cfg = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| format!("TypeError: bad key/cert pair ({e})"))?;
    cfg.alpn_protocols = alpn.to_vec();
    Ok(cfg)
}

/// 客户端 TLS（`ca` 缺省走系统 roots；`rejectUnauthorized:false` 跳校验）。
fn build_client_tls(
    ca_pem: Option<&str>,
    reject_unauthorized: bool,
    alpn: Vec<u8>,
) -> Result<rustls::ClientConfig, String> {
    let mut roots = rustls::RootCertStore::empty();
    if let Some(pem) = ca_pem {
        let mut added = 0usize;
        for cert in rustls_pemfile::certs(&mut pem.as_bytes()) {
            match cert {
                Ok(c) => {
                    roots.add(c).map_err(|e| format!("TypeError: bad ca cert ({e})"))?;
                    added += 1;
                }
                Err(_) => return Err("TypeError: options.ca is not valid PEM".into()),
            }
        }
        if added == 0 {
            return Err("TypeError: options.ca has no certificate".into());
        }
    } else {
        // tls.rs 同款：系统 roots 逐个装，坏的跳过；零命中即错。
        let loaded = rustls_native_certs::load_native_certs();
        let mut added = 0usize;
        for cert in loaded.certs {
            if roots.add(cert).is_ok() {
                added += 1;
            }
        }
        if added == 0 {
            return Err(format!(
                "OperationError: no system roots ({} load errors)",
                loaded.errors.len()
            ));
        }
    }
    let mut cfg = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    if !reject_unauthorized {
        cfg.dangerous().set_certificate_verifier(Arc::new(NoVerifier));
    }
    cfg.alpn_protocols = vec![alpn];
    Ok(cfg)
}

/// 握手信息（真协商值；取不到即空串，不编造）。
fn handshake_info(conn: &quinn::Connection) -> (String, String) {
    let mut alpn = String::new();
    let mut servername = String::new();
    if let Some(data) = conn.handshake_data() {
        if let Ok(h) = data.downcast::<quinn::crypto::rustls::HandshakeData>() {
            if let Some(p) = h.protocol {
                alpn = String::from_utf8_lossy(&p).into_owned();
            }
            if let Some(s) = h.server_name {
                servername = s;
            }
        }
    }
    (alpn, servername)
}

/// 关闭原因映射（应用码透出；本地关按 0；其余 -1 + 文案）。
fn close_info(err: quinn::ConnectionError) -> (i64, String) {
    match err {
        quinn::ConnectionError::ApplicationClosed(app) => {
            (app.error_code.into_inner() as i64, String::from_utf8_lossy(&app.reason).into_owned())
        }
        quinn::ConnectionError::LocallyClosed => (0, String::new()),
        quinn::ConnectionError::ConnectionClosed(frame) => {
            (-1, format!("closed by peer: {frame}"))
        }
        other => (-1, other.to_string()),
    }
}

/// 会话收件箱事件（方向按收件会话定）。
#[derive(Debug)]
pub enum QuicEvent {
    /// 服务端新会话（endpoint 目标收；含寻址信息，JS 侧建 `QuicSession`）。
    EndpointSession { ep: u64, sess: u64, remote: String, alpn: String, servername: String },
    /// 客户端握手成功（会话目标收）。
    SessionSecure { id: u64, alpn: String, servername: String },
    /// 握手失败（会话目标收；随后必跟 `SessionClose{-1}`）。
    SessionError { id: u64, message: String },
    /// 会话结束（会话目标收；派发后摘除 + purge）。
    SessionClose { id: u64, code: i64, reason: String },
    /// endpoint accept 环结束（endpoint 目标收；派发后摘除 + purge）。
    EndpointClosed { ep: u64 },
    /// 本地开流就绪（流目标收；`qid` 为 quic 流号）。
    StreamOpened { id: u64, qid: u64 },
    /// 对端开流（会话目标收；JS 侧建 `QuicStream`）。
    StreamAccepted { id: u64, qid: u64, dir: String },
    /// 流数据（流目标收；`b64` 载荷，JS 侧转 Buffer）。
    StreamData { id: u64, b64: String },
    /// 读端 FIN（流目标收）。
    StreamEnd { id: u64 },
    /// 写端 finish 落定（流目标收；与 `StreamEnd` 配对成 `close`）。
    StreamWriteDone { id: u64 },
    /// 流终结（流目标收；派发后收尾 + 摘除 + purge）。
    StreamClosed { id: u64, code: i64 },
    /// 流读写错（流目标收；随后必跟 `StreamClosed{-1}`）。
    StreamError { id: u64, message: String },
    /// 数据报到达（会话目标收；`b64` 载荷）。
    Datagram { id: u64, b64: String },
    /// H3 请求到达（会话目标收；9i-9 服务端 H3 分支；headers 为 JSON 值，body b64）。
    H3Request { sess: u64, stream: u64, method: String, path: String, headers: serde_json::Value, body: String },
    /// H3 响应就绪（流目标收；9i-9 客户端 H3 分支；headers 为 JSON 值，body b64）。
    H3Response { stream: u64, status: u16, headers: serde_json::Value, body: String },
}

fn with_str_args(
    cx: &mut JSContext,
    global: *mut JSObject,
    fun: JSVal,
    kind: &str,
    payload: &str,
) -> Option<JSVal> {
    use mozjs::conversions::ToJSValConvertible as _;
    rooted!(&in(cx) let mut a = UndefinedValue());
    rooted!(&in(cx) let mut b = UndefinedValue());
    kind.to_jsval(cx, a.handle_mut());
    payload.to_jsval(cx, b.handle_mut());
    crate::jsapi_glue::call_two(cx, global, fun, a.get(), b.get())
}

/// 事件分发（事件循环 `pump_once` 内调用；目标已摘即丢弃；收尾单出口）。
pub fn dispatch(
    cx: &mut JSContext,
    global: *mut JSObject,
    ev: QuicEvent,
    err: crate::runtime::ErrorSource<'_>,
) -> Result<(), crate::error::Error> {
    let failed = |cx: &mut JSContext| match err {
        crate::runtime::ErrorSource::Script { source, filename } => {
            crate::jsapi_glue::pending_exception_error(cx, global, source, filename)
        }
        crate::runtime::ErrorSource::Module { url } => crate::modules::module_error(cx, url),
    };
    let emit_to = |cx: &mut JSContext,
                   target: JSVal,
                   kind: &str,
                   payload: &str|
     -> Result<(), crate::error::Error> {
        if !target.is_object() {
            return Ok(());
        }
        rooted!(&in(cx) let t: *mut JSObject = target.to_object());
        let Some(fun) = get_prop_value(cx, t.get(), c"__ev") else {
            return Err(failed(cx));
        };
        if with_str_args(cx, global, fun, kind, payload).is_none() {
            return Err(failed(cx));
        }
        Ok(())
    };
    match ev {
        QuicEvent::EndpointSession { ep, sess, remote, alpn, servername } => {
            let Some(target) = state::quic_ep_target(ep) else { return Ok(()) };
            let payload = serde_json::json!({
                "sessionId": sess.to_string(), "remote": remote,
                "alpn": alpn, "servername": servername,
            })
            .to_string();
            emit_to(cx, target, "session", &payload)
        }
        QuicEvent::SessionSecure { id, alpn, servername } => {
            let Some(target) = state::quic_sess_target(id) else { return Ok(()) };
            let payload = serde_json::json!({ "alpn": alpn, "servername": servername }).to_string();
            emit_to(cx, target, "secure", &payload)
        }
        QuicEvent::SessionError { id, message } => {
            let Some(target) = state::quic_sess_target(id) else { return Ok(()) };
            emit_to(cx, target, "error", &message)
        }
        QuicEvent::SessionClose { id, code, reason } => {
            if let Some(target) = state::quic_sess_target(id) {
                let payload = serde_json::json!({ "code": code, "reason": reason }).to_string();
                emit_to(cx, target, "close", &payload)?;
            }
            // 名下流一并收尾（任务 abort + 记录摘 + 目标摘；事件不再补，记档）。
            for sid in state::quic_session_streams(id) {
                state::quic_stream_finish(sid);
                state::quic_stream_remove(sid);
                state::quic_stream_target_remove(sid);
            }
            // purge 一律放派发之后（§4.36 checklist）。
            state::quic_sess_remove(id);
            state::quic_sess_target_remove(id);
            Ok(())
        }
        QuicEvent::EndpointClosed { ep } => {
            if let Some(target) = state::quic_ep_target(ep) {
                emit_to(cx, target, "close", "")?;
            }
            state::quic_ep_remove(ep);
            state::quic_ep_target_remove(ep);
            Ok(())
        }
        QuicEvent::StreamOpened { id, qid } => {
            let Some(target) = state::quic_stream_target(id) else { return Ok(()) };
            emit_to(cx, target, "opened", &qid.to_string())
        }
        QuicEvent::StreamAccepted { id, qid, dir } => {
            // 对端流挂到会话目标下（JS 侧建 `QuicStream` 再 attach）。
            let Some(starget) = state::quic_sess_target_by_stream(id) else { return Ok(()) };
            let payload = serde_json::json!({ "streamId": id.to_string(), "qid": qid, "dir": dir }).to_string();
            emit_to(cx, starget, "stream", &payload)
        }
        QuicEvent::StreamData { id, b64 } => {
            let Some(target) = state::quic_stream_target(id) else { return Ok(()) };
            emit_to(cx, target, "data", &b64)
        }
        QuicEvent::StreamEnd { id } => {
            let Some(target) = state::quic_stream_target(id) else { return Ok(()) };
            emit_to(cx, target, "end", "")
        }
        QuicEvent::StreamWriteDone { id } => {
            let Some(target) = state::quic_stream_target(id) else { return Ok(()) };
            emit_to(cx, target, "writedone", "")
        }
        QuicEvent::StreamClosed { id, code } => {
            if let Some(target) = state::quic_stream_target(id) {
                emit_to(cx, target, "closed", &code.to_string())?;
            }
            state::quic_stream_finish(id);
            state::quic_stream_remove(id);
            state::quic_stream_target_remove(id);
            Ok(())
        }
        QuicEvent::StreamError { id, message } => {
            let Some(target) = state::quic_stream_target(id) else { return Ok(()) };
            emit_to(cx, target, "error", &message)
        }
        QuicEvent::Datagram { id, b64 } => {
            let Some(target) = state::quic_sess_target(id) else { return Ok(()) };
            emit_to(cx, target, "datagram", &b64)
        }
        QuicEvent::H3Request { sess, stream, method, path, headers, body } => {
            let Some(target) = state::quic_sess_target(sess) else { return Ok(()) };
            let payload = serde_json::json!({
                "streamId": stream.to_string(), "method": method, "path": path,
                "headers": headers, "body": body,
            })
            .to_string();
            emit_to(cx, target, "request", &payload)
        }
        QuicEvent::H3Response { stream, status, headers, body } => {
            let Some(target) = state::quic_stream_target(stream) else { return Ok(()) };
            let payload = serde_json::json!({ "status": status, "headers": headers, "body": body }).to_string();
            emit_to(cx, target, "response", &payload)
        }
    }
}

/// id 实参（字符串形态数字，§4.33 约定）。
fn arg_id(cx: &mut JSContext, frame: &Frame, i: u32) -> Option<u64> {
    if frame.argc() <= i {
        report_error(cx, "TypeError: quic call needs an id");
        return None;
    }
    value_to_string(cx, frame.arg(i)).parse::<u64>().ok().or_else(|| {
        report_error(cx, "TypeError: quic id must be an id string");
        None
    })
}

/// JSON 实参（JS 侧已拼好；解析失败即 TypeError）。
fn arg_json(cx: &mut JSContext, frame: &Frame, i: u32, what: &str) -> Option<serde_json::Value> {
    if frame.argc() <= i {
        report_error(cx, &format!("TypeError: {what} needs options"));
        return None;
    }
    serde_json::from_str(&value_to_string(cx, frame.arg(i))).ok().or_else(|| {
        report_error(cx, &format!("TypeError: {what} options are not valid"));
        None
    })
}

fn sock_addr(host: &str, port: u16) -> Result<SocketAddr, String> {
    use std::net::ToSocketAddrs as _;
    let h = if host.is_empty() || host == "localhost" { "127.0.0.1" } else { host };
    format!("{h}:{port}")
        .to_socket_addrs()
        .map_err(|e| format!("TypeError: bad address {host}:{port} ({e})"))?
        .next()
        .ok_or_else(|| format!("TypeError: bad address {host}:{port} (unresolvable)"))
}

/// 起监听。`__wjs_quic_listen(optsJson)` → endpoint id 串（bind 失败同步抛错）。
pub unsafe extern "C" fn quic_listen(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let v = match arg_json(&mut cx, &frame, 0, "quic listen") {
        Some(v) => v,
        None => return false,
    };
    let opts: ListenOpts = match serde_json::from_value(v) {
        Ok(o) => o,
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: bad listen options ({e})"));
            return false;
        }
    };
    if opts.alpn.is_empty() {
        report_error(&mut cx, "TypeError: listen needs options.alpn (QUIC has no no-ALPN case)");
        return false;
    }
    let alpn: Vec<Vec<u8>> = opts.alpn.iter().map(|s| s.as_bytes().to_vec()).collect();
    let transport = match build_transport(opts.idle_timeout_ms, opts.cc.as_deref()) {
        Ok(t) => t,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let tls = match build_server_tls(&opts.key_pem, &opts.cert_pem, &alpn) {
        Ok(t) => t,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let mut server_cfg = match quinn::crypto::rustls::QuicServerConfig::try_from(tls) {
        Ok(c) => quinn::ServerConfig::with_crypto(Arc::new(c)),
        Err(e) => {
            report_error(&mut cx, &format!("OperationError: quic server crypto failed ({e:?})"));
            return false;
        }
    };
    server_cfg.transport_config(transport);
    let addr = match sock_addr(&opts.host, opts.port) {
        Ok(a) => a,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let endpoint = match quinn::Endpoint::server(server_cfg, addr) {
        Ok(ep) => ep,
        Err(e) => {
            report_error(&mut cx, &format!("OperationError: quic bind failed ({e})"));
            return false;
        }
    };
    let inbox = match state::quic_tx_clone() {
        Some(tx) => tx,
        None => {
            report_error(&mut cx, "OperationError: quic channel is not initialized");
            return false;
        }
    };
    let id = state::quic_alloc_id();
    let ep_clone = endpoint.clone();
    let ep_local = endpoint.local_addr().map(|a| a.to_string()).unwrap_or_default();
    let handle = tokio::spawn(async move {
        // accept 环：逐个握手（并发孵化），环断即 endpoint 关。
        while let Some(incoming) = ep_clone.accept().await {
            let inbox = inbox.clone();
            let ep_local = ep_local.clone();
            tokio::spawn(async move {
                let remote = incoming.remote_address().to_string();
                match incoming.await {
                    Ok(conn) => {
                        let (alpn, servername) = handshake_info(&conn);
                        let sess = state::quic_alloc_id();
                        state::quic_sess_insert(sess, ep_local, remote.clone());
                        state::quic_sess_set_conn(sess, conn.clone());
                        // ALPN "h3" 走 H3 驱动（accept 循环 + Respond 命令）；其余裸流。
                        let driver = if alpn == "h3" {
                            spawn_h3_server(sess, conn.clone(), inbox.clone())
                        } else {
                            spawn_driver(sess, conn, inbox.clone())
                        };
                        state::quic_sess_set_driver(sess, driver);
                        // 服务端会话握手已成：先报会话（建目标），再报 secure（与客户端对称）。
                        let _ = inbox.send(QuicEvent::EndpointSession {
                            ep: id,
                            sess,
                            remote,
                            alpn: alpn.clone(),
                            servername: servername.clone(),
                        });
                        let _ = inbox.send(QuicEvent::SessionSecure { id: sess, alpn, servername });
                    }
                    Err(_) => {
                        // 服务端握手失败静默丢弃（扫描流量不能杀服务，记档）。
                    }
                }
            });
        }
        let _ = inbox.send(QuicEvent::EndpointClosed { ep: id });
    });
    state::quic_ep_insert(id, endpoint, handle.abort_handle());
    use mozjs::conversions::ToJSValConvertible as _;
    id.to_string().to_jsval(&mut cx, frame.rval_mut());
    true
}

/// endpoint 本地地址。`__wjs_quic_ep_addr(id)` → `"ip:port"`。
pub unsafe extern "C" fn quic_ep_addr(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_listen
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    use mozjs::conversions::ToJSValConvertible as _;
    match state::quic_ep_get(id).and_then(|ep| ep.local_addr().ok()) {
        Some(addr) => {
            addr.to_string().to_jsval(&mut cx, frame.rval_mut());
            true
        }
        None => {
            report_error(&mut cx, "OperationError: quic endpoint is gone");
            false
        }
    }
}

/// 关 endpoint（abort accept 环 + 发 `EndpointClosed`；派发后摘除）。
/// `__wjs_quic_ep_close(id)` → undefined。
pub unsafe extern "C" fn quic_ep_close(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_listen
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    // 重复关/已摘除即无操作（目标在派发后摘除前仍可达，begin_close 防双发）。
    let ep = match state::quic_ep_begin_close(id) {
        Some(ep) => ep,
        None => {
            frame.set_rval(UndefinedValue());
            return true;
        }
    };
    ep.close(0u32.into(), b"bye");
    if let Some(tx) = state::quic_tx_clone() {
        let _ = tx.send(QuicEvent::EndpointClosed { ep: id });
    }
    frame.set_rval(UndefinedValue());
    true
}

/// 登记 endpoint JS 目标。`__wjs_quic_ep_attach(id, target)` → undefined。
pub unsafe extern "C" fn quic_ep_attach(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_listen
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    if frame.argc() < 2 || !frame.arg(1).is_object() {
        report_error(&mut cx, "TypeError: quic endpoint attach needs a target object");
        return false;
    }
    state::quic_ep_target_add(id, frame.arg(1));
    frame.set_rval(UndefinedValue());
    true
}

/// 发起连接。`__wjs_quic_connect(optsJson)` → session id 串（握手异步）。
pub unsafe extern "C" fn quic_connect(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_listen
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let v = match arg_json(&mut cx, &frame, 0, "quic connect") {
        Some(v) => v,
        None => return false,
    };
    let opts: ConnectOpts = match serde_json::from_value(v) {
        Ok(o) => o,
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: bad connect options ({e})"));
            return false;
        }
    };
    if opts.alpn.is_empty() {
        report_error(&mut cx, "TypeError: connect needs options.alpn");
        return false;
    }
    let transport = match build_transport(opts.idle_timeout_ms, opts.cc.as_deref()) {
        Ok(t) => t,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let tls = match build_client_tls(
        opts.ca_pem.as_deref(),
        opts.reject_unauthorized.unwrap_or(true),
        opts.alpn.as_bytes().to_vec(),
    ) {
        Ok(t) => t,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let mut client_cfg = match quinn::crypto::rustls::QuicClientConfig::try_from(tls) {
        Ok(c) => quinn::ClientConfig::new(Arc::new(c)),
        Err(e) => {
            report_error(&mut cx, &format!("OperationError: quic client crypto failed ({e:?})"));
            return false;
        }
    };
    client_cfg.transport_config(transport);
    let addr = match sock_addr(&opts.host, opts.port) {
        Ok(a) => a,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let servername = opts.servername.clone().unwrap_or_else(|| opts.host.clone());
    let mut endpoint = match quinn::Endpoint::client("0.0.0.0:0".parse().unwrap()) {
        Ok(ep) => ep,
        Err(e) => {
            report_error(&mut cx, &format!("OperationError: quic client bind failed ({e})"));
            return false;
        }
    };
    endpoint.set_default_client_config(client_cfg);
    let inbox = match state::quic_tx_clone() {
        Some(tx) => tx,
        None => {
            report_error(&mut cx, "OperationError: quic channel is not initialized");
            return false;
        }
    };
    let id = state::quic_alloc_id();
    let alpn_report = opts.alpn.clone();
    let remote_report = addr.to_string();
    let local_report = endpoint.local_addr().map(|a| a.to_string()).unwrap_or_default();
    state::quic_sess_insert(id, local_report, remote_report);
    let handle = tokio::spawn(async move {
        match endpoint.connect(addr, &servername) {
            Ok(connecting) => match connecting.await {
                Ok(conn) => {
                    state::quic_sess_set_conn(id, conn.clone());
                    // 发起侧 endpoint 移交 entry 保活（任务结束即 drop 会怎样未可知，
                    // 实测 task 尾 close 会杀会话；收尾时才关，见 quic_sess_remove）。
                    state::quic_sess_set_client_ep(id, endpoint.clone());
                    // ALPN "h3" 走 H3 服务任务（driver 轮询 + Request 命令）；其余裸流。
                    let driver = if alpn_report == "h3" {
                        spawn_h3_client(id, conn, inbox.clone())
                    } else {
                        spawn_driver(id, conn, inbox.clone())
                    };
                    state::quic_sess_set_driver(id, driver);
                    // 发起侧 handshake_data 的 server_name 恒 None（quinn 口径），
                    // 用请求时的 servername（TLS 失败即无握手，无此事件）。
                    let _ = inbox.send(QuicEvent::SessionSecure {
                        id,
                        alpn: alpn_report,
                        servername,
                    });
                }
                Err(e) => {
                    let _ = inbox.send(QuicEvent::SessionError {
                        id,
                        message: format!("ERR_QUIC_HANDSHAKE: {e}"),
                    });
                    let _ = inbox.send(QuicEvent::SessionClose { id, code: -1, reason: "handshake failed".into() });
                }
            },
            Err(e) => {
                let _ = inbox.send(QuicEvent::SessionError {
                    id,
                    message: format!("ERR_QUIC_HANDSHAKE: {e}"),
                });
                let _ = inbox.send(QuicEvent::SessionClose { id, code: -1, reason: "connect failed".into() });
            }
        }
        // endpoint 已移交 entry 保活，此处不再 close（task 尾 close 会杀活会话）。
    });
    // 连接任务自生自灭（连上即孵化驱动后结束；失败发 Error+Close）；柄 detach。
    drop(handle);
    use mozjs::conversions::ToJSValConvertible as _;
    id.to_string().to_jsval(&mut cx, frame.rval_mut());
    true
}

/// 登记会话 JS 目标。`__wjs_quic_sess_attach(id, target)` → undefined。
pub unsafe extern "C" fn quic_sess_attach(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_listen
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    if frame.argc() < 2 || !frame.arg(1).is_object() {
        report_error(&mut cx, "TypeError: quic session attach needs a target object");
        return false;
    }
    state::quic_sess_target_add(id, frame.arg(1));
    frame.set_rval(UndefinedValue());
    true
}

/// 会话信息。`__wjs_quic_sess_info(id)` → JSON `{secure,local,remote,alpn,servername}`。
pub unsafe extern "C" fn quic_sess_info(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_listen
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    use mozjs::conversions::ToJSValConvertible as _;
    let (local, remote, secure, alpn, servername) = match state::quic_sess_conn(id) {
        Some(conn) => {
            let (alpn, servername) = handshake_info(&conn);
            let (local, remote) = state::quic_sess_addrs(id).unwrap_or_default();
            (local, remote, true, alpn, servername)
        }
        None => {
            let (local, remote) = state::quic_sess_addrs(id).unwrap_or_default();
            (local, remote, false, String::new(), String::new())
        }
    };
    serde_json::json!({
        "secure": secure, "local": local, "remote": remote,
        "alpn": alpn, "servername": servername,
    })
    .to_string()
    .to_jsval(&mut cx, frame.rval_mut());
    true
}

/// 会话统计（真值子集）。`__wjs_quic_sess_stats(id)` → JSON。
pub unsafe extern "C" fn quic_sess_stats(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_listen
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    use mozjs::conversions::ToJSValConvertible as _;
    match state::quic_sess_conn(id) {
        Some(conn) => {
            let st = conn.stats();
            serde_json::json!({
                "rttMs": conn.rtt().as_secs_f64() * 1000.0,
                "udpRxBytes": st.udp_rx.bytes, "udpTxBytes": st.udp_tx.bytes,
                "udpRxDatagrams": st.udp_rx.datagrams, "udpTxDatagrams": st.udp_tx.datagrams,
                "cwnd": st.path.cwnd, "lostPackets": st.path.lost_packets,
            })
            .to_string()
            .to_jsval(&mut cx, frame.rval_mut());
            true
        }
        None => {
            serde_json::json!({
                "rttMs": 0.0, "udpRxBytes": 0, "udpTxBytes": 0,
                "udpRxDatagrams": 0, "udpTxDatagrams": 0, "cwnd": 0, "lostPackets": 0,
            })
            .to_string()
            .to_jsval(&mut cx, frame.rval_mut());
            true
        }
    }
}

/// 关会话（`conn.close`；`SessionClose` 事件到后摘除）。
/// `__wjs_quic_sess_close(id, code)` → undefined。
pub unsafe extern "C" fn quic_sess_close(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_listen
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    let code = if frame.argc() >= 2 {
        value_to_string(&mut cx, frame.arg(1)).parse::<u64>().unwrap_or(0)
    } else {
        0
    };
    if let Some(conn) = state::quic_sess_conn(id) {
        if let Ok(v) = quinn::VarInt::from_u64(code) {
            conn.close(v, b"bye");
        }
    }
    frame.set_rval(UndefinedValue());
    true
}

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
fn h3_headers_json(value: serde_json::Value) -> Vec<(String, String)> {
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
fn sess_finish(
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

/// `__wjs_quic_h3_respond(sessId, streamId, json)` → undefined（服务端回 H3 响应；
/// json `{status, headers, body(b64)}`；会话已摘/非 H3 即 ERR_INVALID_STATE）。
pub unsafe extern "C" fn quic_h3_respond(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(sess) = arg_id(&mut cx, &frame, 0) else {
        return false;
    };
    let Some(stream) = arg_id(&mut cx, &frame, 1) else {
        return false;
    };
    let v = match arg_json(&mut cx, &frame, 2, "quic h3 respond") {
        Some(v) => v,
        None => return false,
    };
    let status = v.get("status").and_then(|x| x.as_u64()).unwrap_or(200).min(599) as u16;
    let headers = h3_headers_json(v.get("headers").cloned().unwrap_or(serde_json::Value::Object(Default::default())));
    let body = v.get("body").and_then(|x| x.as_str()).unwrap_or("");
    let body = match base64::engine::general_purpose::STANDARD.decode(body) {
        Ok(b) => b,
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: quic h3 respond body is not base64 ({e})"));
            return false;
        }
    };
    let Some(tx) = state::quic_sess_h3_cmd(sess) else {
        report_error(&mut cx, "ERR_INVALID_STATE: quic h3 session is gone");
        return false;
    };
    let _ = tx.send(QuicH3Cmd::Respond { stream, status, headers, body });
    frame.set_rval(UndefinedValue());
    true
}

/// `__wjs_quic_h3_request(sessId, json)` → 流 id 串（客户端发 H3 请求；
/// json `{method, path, headers, body(b64)}`；响应经 `H3Response` 到流目标）。
pub unsafe extern "C" fn quic_h3_request(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_h3_respond
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(sess) = arg_id(&mut cx, &frame, 0) else {
        return false;
    };
    let v = match arg_json(&mut cx, &frame, 1, "quic h3 request") {
        Some(v) => v,
        None => return false,
    };
    let method = v.get("method").and_then(|x| x.as_str()).unwrap_or("GET").to_string();
    let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("/").to_string();
    let headers = h3_headers_json(v.get("headers").cloned().unwrap_or(serde_json::Value::Object(Default::default())));
    let body = v.get("body").and_then(|x| x.as_str()).unwrap_or("");
    let body = match base64::engine::general_purpose::STANDARD.decode(body) {
        Ok(b) => b,
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: quic h3 request body is not base64 ({e})"));
            return false;
        }
    };
    let Some(tx) = state::quic_sess_h3_cmd(sess) else {
        report_error(&mut cx, "ERR_INVALID_STATE: quic h3 session is gone");
        return false;
    };
    let stream = state::quic_alloc_id();
    state::quic_stream_insert(stream, sess, state::QuicStreamDir::Bidi);
    let _ = tx.send(QuicH3Cmd::Request { stream, method, path, headers, body });
    use mozjs::conversions::ToJSValConvertible as _;
    stream.to_string().to_jsval(&mut cx, frame.rval_mut());
    true
}

/// 本地开流。`__wjs_quic_sess_open(sessId, "bidi"|"uni")` → 流 id 串
/// （就绪经 `StreamOpened` 事件；会话已摘即 `ERR_INVALID_STATE` 错）。
pub unsafe extern "C" fn quic_sess_open(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_listen
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let sess = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    let dir = if frame.argc() >= 2 { value_to_string(&mut cx, frame.arg(1)) } else { String::new() };
    let bidi = match dir.as_str() {
        "bidi" => true,
        "uni" => false,
        _ => {
            report_error(&mut cx, "TypeError: direction must be bidi or uni");
            return false;
        }
    };
    let dir_enum =
        if bidi { state::QuicStreamDir::Bidi } else { state::QuicStreamDir::Send };
    let stream = state::quic_alloc_id();
    state::quic_stream_insert(stream, sess, dir_enum);
    let cmd = if bidi {
        QuicSessCmd::OpenBidi { stream }
    } else {
        QuicSessCmd::OpenUni { stream }
    };
    if !state::quic_sess_cmd(sess, cmd) {
        state::quic_stream_remove(stream);
        report_error(&mut cx, "ERR_INVALID_STATE: Session is closed. New streams cannot be opened.");
        return false;
    }
    use mozjs::conversions::ToJSValConvertible as _;
    stream.to_string().to_jsval(&mut cx, frame.rval_mut());
    true
}

/// 登记流 JS 目标。`__wjs_quic_stream_attach(id, target)` → undefined。
pub unsafe extern "C" fn quic_stream_attach(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_listen
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    if frame.argc() < 2 || !frame.arg(1).is_object() {
        report_error(&mut cx, "TypeError: quic stream attach needs a target object");
        return false;
    }
    state::quic_stream_target_add(id, frame.arg(1));
    frame.set_rval(UndefinedValue());
    true
}

/// 流写。`__wjs_quic_stream_write(id, uint8)` → boolean（流已收尾即 false）。
pub unsafe extern "C" fn quic_stream_write(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_listen
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: quic stream write needs data");
        return false;
    }
    let bytes = match crate::jsapi_glue::view_bytes(&mut cx, frame.arg(1), "quic stream write") {
        Some(b) => b,
        None => return false,
    };
    frame.set_rval(mozjs::jsval::BooleanValue(
        state::quic_stream_write_cmd(id, QuicStreamCmd::Write(bytes)),
    ));
    true
}

/// 写端 finish。`__wjs_quic_stream_finish(id)` → undefined（已收尾即无操作）。
pub unsafe extern "C" fn quic_stream_finish(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_listen
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    state::quic_stream_write_cmd(id, QuicStreamCmd::Finish);
    frame.set_rval(UndefinedValue());
    true
}

/// 码实参（number/bigint 形态；非法即 `ERR_OUT_OF_RANGE` 错，None）。
fn arg_code(cx: &mut JSContext, frame: &Frame, i: u32) -> Option<u64> {
    if frame.argc() <= i {
        return Some(0);
    }
    let v = frame.arg(i);
    let n = if v.is_number() {
        value_to_string(cx, v).parse::<i64>().ok()?
    } else if v.is_bigint() {
        value_to_string(cx, v).trim_end_matches('n').parse::<i64>().ok()?
    } else {
        return None;
    };
    if n < 0 || n > 0xFFFF_FFFF {
        return None;
    }
    Some(n as u64)
}

/// 写端 reset。`__wjs_quic_stream_reset(id, code)` → undefined。
pub unsafe extern "C" fn quic_stream_reset(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_listen
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    let code = match arg_code(&mut cx, &frame, 1) {
        Some(c) => c,
        None => {
            report_error(&mut cx, "ERR_OUT_OF_RANGE: reset code must be an integer in range");
            return false;
        }
    };
    state::quic_stream_write_cmd(id, QuicStreamCmd::Reset(code));
    frame.set_rval(UndefinedValue());
    true
}

/// 读端 stop。`__wjs_quic_stream_stop(id, code)` → undefined。
pub unsafe extern "C" fn quic_stream_stop(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_listen
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    let code = match arg_code(&mut cx, &frame, 1) {
        Some(c) => c,
        None => {
            report_error(&mut cx, "ERR_OUT_OF_RANGE: stop code must be an integer in range");
            return false;
        }
    };
    state::quic_stream_read_cmd(id, QuicStreamCmd::Stop(code));
    frame.set_rval(UndefinedValue());
    true
}

/// 发数据报（超限静默丢弃，Node 同款）。`__wjs_quic_sess_send_dgram(id, uint8)` → boolean。
pub unsafe extern "C" fn quic_sess_send_dgram(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_listen
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: sendDatagram needs data");
        return false;
    }
    let bytes = match crate::jsapi_glue::view_bytes(&mut cx, frame.arg(1), "sendDatagram") {
        Some(b) => b,
        None => return false,
    };
    let ok = match state::quic_sess_conn(id) {
        Some(conn) => conn.send_datagram(bytes.into()).is_ok(),
        None => false,
    };
    frame.set_rval(mozjs::jsval::BooleanValue(ok));
    true
}

/// 数据报上限（禁收发即 0，Node 同款）。`__wjs_quic_sess_max_dgram(id)` → 数字串。
pub unsafe extern "C" fn quic_sess_max_dgram(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_listen
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    use mozjs::conversions::ToJSValConvertible as _;
    let max = state::quic_sess_conn(id).and_then(|c| c.max_datagram_size()).unwrap_or(0);
    max.to_string().to_jsval(&mut cx, frame.rval_mut());
    true
}

/// 内嵌 ESM 源（`node:quic`，9g-1：Endpoint + 会话）。
pub const SOURCE: &str = r#"
import { EventEmitter } from "node:events";

export const CC_ALGO_RENO = "reno";
export const CC_ALGO_CUBIC = "cubic";
export const CC_ALGO_BBR = "bbr";

export class QuicError extends Error {
  constructor(message, code = "ERR_QUIC_ERROR") {
    super(message);
    this.name = "QuicError";
    this.code = code;
  }
}

function __parseCoded(m, dflt) {
  const mm = String(m).match(/^(ERR_[A-Z0-9_]+): ([\s\S]*)$/);
  if (mm) return { code: mm[1], message: mm[2] };
  return { code: dflt, message: String(m) };
}
function __quicErr(e, dflt = "ERR_QUIC_ERROR") {
  const m = String((e && e.message) || e);
  const { code, message } = __parseCoded(m, dflt);
  const err = new QuicError(message, code);
  throw err;
}
function __callNative(fn, dflt) {
  try {
    return fn();
  } catch (e) {
    __quicErr(e, dflt);
  }
}
function __needStr(v, what) {
  if (typeof v !== "string") {
    const err = new TypeError(`The "${what}" argument must be of type string. Received type ${typeof v}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  return v;
}
function __needPort(v, what) {
  if (typeof v !== "number" || !Number.isInteger(v) || v < 0 || v > 65535) {
    const err = new RangeError(`The "${what}" argument must be an integer in range 0..65535. Received ${String(v)}`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  return v;
}
function __normAlpn(v, what, single) {
  const list = Array.isArray(v) ? v.slice() : [v];
  if (list.length === 0 || list.some((s) => typeof s !== "string" || s.length === 0)) {
    const err = new TypeError(`The "${what}" argument must be a non-empty string or array of non-empty strings.`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (single && list.length !== 1) {
    const err = new TypeError(`The "${what}" argument must be a single protocol string.`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  return single ? list[0] : list;
}
function __normCc(v) {
  if (v === undefined) return undefined;
  if (v !== "reno" && v !== "cubic" && v !== "bbr") {
    const err = new TypeError(`The "options.cc" property must be one of reno/cubic/bbr.`);
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
  return v;
}
function __normIdle(v) {
  if (v === undefined) return undefined;
  if (typeof v !== "number" || !Number.isInteger(v) || v < 0) {
    const err = new RangeError(`The "options.idleTimeout" property must be a non-negative integer.`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  return v;
}
function __parseAddr(addr) {
  if (typeof addr === "string") {
    let host, port;
    if (addr.startsWith("[")) {
      const rb = addr.indexOf("]");
      if (rb < 0) throw new TypeError(`Invalid address string.`);
      host = addr.slice(1, rb);
      const rest = addr.slice(rb + 1);
      if (!rest.startsWith(":")) throw new TypeError(`Invalid address string.`);
      port = Number(rest.slice(1));
    } else {
      const i = addr.lastIndexOf(":");
      if (i < 0) throw new TypeError(`Invalid address string.`);
      host = addr.slice(0, i);
      port = Number(addr.slice(i + 1));
    }
    if (host === "") host = "127.0.0.1";
    return { host, port: __needPort(port, "address port") };
  }
  if (addr !== null && typeof addr === "object") {
    const host = addr.address ?? addr.host ?? "127.0.0.1";
    const port = addr.port;
    __needStr(String(host), "address");
    return { host: String(host), port: __needPort(port, "address.port") };
  }
  const err = new TypeError(`The "address" argument must be of type string or object.`);
  err.code = "ERR_INVALID_ARG_TYPE";
  throw err;
}

export class QuicEndpoint extends EventEmitter {
  constructor(__id) {
    super();
    if (typeof __id !== "string") {
      const err = new TypeError(`QuicEndpoint needs an internal endpoint id.`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    this.__id = __id;
    this.__closed = false;
    this.__ev = this.__ev.bind(this);
    __wjs_quic_ep_attach(__id, this);
  }
  __ev(kind, payload) {
    if (kind === "session") {
      const info = JSON.parse(String(payload));
      const sess = new QuicSession(info.sessionId, {
        local: "", remote: info.remote, alpn: info.alpn, servername: info.servername,
      });
      this.emit("session", sess);
    } else if (kind === "close") {
      if (this.__closed) return;
      this.__closed = true;
      this.emit("close");
    }
  }
  address() {
    const s = __callNative(() => __wjs_quic_ep_addr(this.__id));
    const { host, port } = __parseAddr(String(s));
    return { address: host, port, family: host.includes(":") ? "IPv6" : "IPv4" };
  }
  close() {
    if (this.__closed) return;
    __callNative(() => __wjs_quic_ep_close(this.__id));
  }
}

export class QuicSession extends EventEmitter {
  constructor(__id, __peer = {}) {
    super();
    if (typeof __id !== "string") {
      const err = new TypeError(`QuicSession needs an internal session id.`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    this.__id = __id;
    this.__peer = __peer;
    this.__secure = false;
    this.__closed = false;
    this.__h3Pending = new Set();
    this.__ev = this.__ev.bind(this);
    __wjs_quic_sess_attach(__id, this);
  }
  __ev(kind, payload) {
    if (kind === "secure") {
      const info = JSON.parse(String(payload));
      this.__secure = true;
      this.__peer.alpn = info.alpn;
      this.__peer.servername = info.servername;
      this.emit("secure", info.servername, info.alpn);
    } else if (kind === "error") {
      const { code, message } = __parseCoded(String(payload), "ERR_QUIC_HANDSHAKE");
      this.emit("error", new QuicError(message, code));
    } else if (kind === "close") {
      if (this.__closed) return;
      this.__closed = true;
      const info = JSON.parse(String(payload));
      // H3 未决请求随会话关闭全部失败（避免 promise 悬挂）。
      for (const fail of this.__h3Pending) fail();
      this.__h3Pending.clear();
      this.emit("close", info.code, info.reason);
    } else if (kind === "request") {
      const info = JSON.parse(String(payload));
      const req = {
        id: info.streamId,
        method: info.method,
        path: info.path,
        headers: info.headers,
        body: Buffer.from(String(info.body || ""), "base64"),
        respond: ({ status = 200, headers = {}, body } = {}) => {
          const b64 = body === undefined || body === null ? ""
            : Buffer.isBuffer(body) ? body.toString("base64")
            : Buffer.from(body).toString("base64");
          __callNative(() => __wjs_quic_h3_respond(this.__id, info.streamId,
            JSON.stringify({ status, headers, body: b64 })));
        },
      };
      this.emit("request", req);
    } else if (kind === "stream") {
      const info = JSON.parse(String(payload));
      const stream = new QuicStream(info.streamId, { dir: info.dir, qid: info.qid });
      this.emit("stream", stream);
    } else if (kind === "datagram") {
      this.emit("datagram", Buffer.from(String(payload), "base64"));
    }
  }
  get encrypted() { return this.__secure; }
  get alpnProtocol() {
    if (!this.__secure) return null;
    if (this.__peer.alpn) return this.__peer.alpn;
    const info = JSON.parse(__callNative(() => __wjs_quic_sess_info(this.__id)));
    return info.alpn || null;
  }
  get servername() {
    if (!this.__secure) return null;
    // 发起侧 handshake 无 SNI 回显（quinn 口径恒 None），用 secure 事件缓存值。
    if (this.__peer.servername) return this.__peer.servername;
    const info = JSON.parse(__callNative(() => __wjs_quic_sess_info(this.__id)));
    return info.servername || null;
  }
  get localAddress() { return this.__addrOf("local"); }
  get remoteAddress() { return this.__addrOf("remote"); }
  __addrOf(which) {
    if (which === "remote" && this.__peer.remote) {
      const { host, port } = __parseAddr(this.__peer.remote);
      return { address: host, port, family: host.includes(":") ? "IPv6" : "IPv4" };
    }
    const info = JSON.parse(__callNative(() => __wjs_quic_sess_info(this.__id)));
    const raw = which === "local" ? info.local : info.remote;
    if (!raw) return null;
    const { host, port } = __parseAddr(raw);
    return { address: host, port, family: host.includes(":") ? "IPv6" : "IPv4" };
  }
  stats() {
    return JSON.parse(__callNative(() => __wjs_quic_sess_stats(this.__id)));
  }
  close(code = 0) {
    if (typeof code !== "number" || !Number.isInteger(code) || code < 0) {
      const err = new RangeError(`The "code" argument must be a non-negative integer.`);
      err.code = "ERR_OUT_OF_RANGE";
      throw err;
    }
    __callNative(() => __wjs_quic_sess_close(this.__id, String(code)));
  }
  destroy(err) {
    void err;
    this.close();
  }
  request({ method = "GET", path = "/", headers = {}, body } = {}) {
    // 9i-9 H3 面（自定）：仅 ALPN "h3" 会话；响应走一次性 promise。
    if (this.__peer.alpn !== "h3") {
      const err = new Error(`Session ALPN must be "h3" for request(). Received ${JSON.stringify(this.__peer.alpn)}`);
      err.code = "ERR_INVALID_PROTOCOL";
      throw err;
    }
    const b64 = body === undefined || body === null ? ""
      : Buffer.isBuffer(body) ? body.toString("base64")
      : Buffer.from(body).toString("base64");
    const sid = __callNative(() => __wjs_quic_h3_request(this.__id,
      JSON.stringify({ method, path, headers, body: b64 })));
    const target = new EventEmitter();
    target.__ev = (kind, payload) => {
      if (kind === "response") {
        const info = JSON.parse(String(payload));
        this.__h3Pending.delete(fail);
        target.emit("response", { status: info.status, headers: info.headers, body: Buffer.from(String(info.body || ""), "base64") });
      } else if (kind === "error") {
        this.__h3Pending.delete(fail);
        const { code, message } = __parseCoded(String(payload), "ERR_QUIC_H3");
        target.emit("error", new QuicError(message, code));
      } else if (kind === "closed") {
        this.__h3Pending.delete(fail);
        target.emit("error", new QuicError("session closed before response", "ERR_QUIC_SESSION_CLOSED"));
      }
    };
    const fail = () => target.emit("closed");
    this.__h3Pending.add(fail);
    __wjs_quic_stream_attach(String(sid), target);
    return new Promise((resolve, reject) => {
      target.once("response", resolve);
      target.once("error", reject);
    });
  }
  createBidirectionalStream() {
    return this.__openStream("bidi");
  }
  createUnidirectionalStream() {
    return this.__openStream("uni");
  }
  __openStream(dir) {
    const sid = __callNative(() => __wjs_quic_sess_open(this.__id, dir));
    const stream = new QuicStream(String(sid), { dir: dir === "bidi" ? "bidi" : "send", qid: null });
    return new Promise((resolve, reject) => {
      stream.__pendingOpen = { resolve, reject };
    });
  }
  sendDatagram(data) {
    const buf = Buffer.isBuffer(data) ? data : Buffer.from(String(data ?? ""));
    return Boolean(__callNative(() => __wjs_quic_sess_send_dgram(this.__id, buf)));
  }
  get maxDatagramSize() {
    return Number(__callNative(() => __wjs_quic_sess_max_dgram(this.__id)));
  }
}

function __normStreamCode(v, what) {
  if (typeof v === "bigint") v = Number(v);
  if (typeof v !== "number" || !Number.isInteger(v) || v < 0 || v > 4294967295) {
    const err = new RangeError(`The "${what}" argument must be an integer in range 0..2^32-1.`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  return v;
}

export class QuicStream extends EventEmitter {
  constructor(__id, { dir, qid }) {
    super();
    if (typeof __id !== "string") {
      const err = new TypeError(`QuicStream needs an internal stream id.`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    this.__id = __id;
    this.__dir = dir;
    this.__qid = qid ?? null;
    // 半流：不存在的方向视为已 done（单出口 close 配对才成立）。
    this.__readDone = (dir === "send");
    this.__writeDone = (dir === "receive");
    // end()/close() 调后同步落旗（writedone 事件异步到，期间再写必须同步抛）。
    this.__ended = (dir === "receive");
    this.__closed = false;
    this.__closeCode = 0;
    this.__pendingOpen = null;
    this.__ev = this.__ev.bind(this);
    __wjs_quic_stream_attach(__id, this);
  }
  __ev(kind, payload) {
    if (kind === "opened") {
      this.__qid = Number(payload);
      if (this.__pendingOpen) {
        const { resolve } = this.__pendingOpen;
        this.__pendingOpen = null;
        resolve(this);
      }
    } else if (kind === "data") {
      this.emit("data", Buffer.from(String(payload), "base64"));
    } else if (kind === "end") {
      this.__readDone = true;
      this.emit("end");
      this.__maybeClose();
    } else if (kind === "writedone") {
      this.__writeDone = true;
      this.emit("finish");
      this.__maybeClose();
    } else if (kind === "closed") {
      this.__forceClose(Number(payload));
    } else if (kind === "error") {
      const { code, message } = __parseCoded(String(payload), "ERR_QUIC_STREAM");
      if (this.__pendingOpen) {
        const { reject } = this.__pendingOpen;
        this.__pendingOpen = null;
        reject(new QuicError(message, code));
      } else {
        this.emit("error", new QuicError(message, code));
      }
    }
  }
  __maybeClose() {
    if (this.__closed || !this.__readDone || !this.__writeDone) return;
    this.__closed = true;
    this.emit("close", this.__closeCode);
  }
  __forceClose(code) {
    if (this.__closed) return;
    this.__closed = true;
    this.__readDone = true;
    this.__writeDone = true;
    this.__closeCode = code;
    this.emit("close", code);
  }
  get id() { return this.__qid; }
  get direction() { return this.__dir; }
  write(chunk, encoding) {
    if (this.__dir === "receive") {
      const err = new Error("Cannot write to receive-only stream.");
      err.code = "ERR_INVALID_STATE";
      throw err;
    }
    if (this.__writeDone || this.__closed || this.__ended) {
      const err = new Error("Write after end.");
      err.code = "ERR_STREAM_WRITE_AFTER_END";
      throw err;
    }
    const buf = Buffer.isBuffer(chunk) ? chunk : Buffer.from(String(chunk ?? ""), encoding ?? "utf8");
    const ok = __callNative(() => __wjs_quic_stream_write(this.__id, buf));
    if (!ok) {
      const err = new Error("Write after end.");
      err.code = "ERR_STREAM_WRITE_AFTER_END";
      throw err;
    }
    return true;
  }
  end(data, encoding) {
    if (data !== undefined) this.write(data, encoding);
    this.__ended = true;
    __callNative(() => __wjs_quic_stream_finish(this.__id));
    return this;
  }
  close() {
    this.__ended = true;
    __callNative(() => __wjs_quic_stream_finish(this.__id));
  }
  destroy(err) {
    __callNative(() => __wjs_quic_stream_reset(this.__id, 0));
    __callNative(() => __wjs_quic_stream_stop(this.__id, 0));
    if (err !== undefined && err !== null) {
      this.emit("error", err instanceof Error ? err : new Error(String(err)));
    }
    this.__forceClose(0);
  }
  stopSending(code = 0) {
    const c = __normStreamCode(code, "code");
    __callNative(() => __wjs_quic_stream_stop(this.__id, c));
  }
  resetStream(code = 0) {
    const c = __normStreamCode(code, "code");
    __callNative(() => __wjs_quic_stream_reset(this.__id, c));
  }
}

export async function listen(callback, options = {}) {
  if (typeof callback !== "function") {
    const err = new TypeError(`The "callback" argument must be of type function.`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (options === null || typeof options !== "object") {
    const err = new TypeError(`The "options" argument must be of type object.`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const { host, port } = __parseAddr({ address: options.address ?? options.host, port: options.port });
  const alpn = __normAlpn(options.alpn, "options.alpn", false);
  if (options.key === undefined || options.cert === undefined) {
    const err = new TypeError(`listen needs options.key and options.cert (PEM).`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  // 校验提在 native 调用之外（包进 __callNative 会被重包成 ERR_QUIC_ERROR）。
  const idleMs = __normIdle(options.idleTimeout);
  const ccName = __normCc(options.cc);
  const id = __callNative(() => __wjs_quic_listen(JSON.stringify({
    host, port, alpn,
    key_pem: String(options.key), cert_pem: String(options.cert),
    idle_timeout_ms: idleMs, cc: ccName,
  })));
  const ep = new QuicEndpoint(String(id));
  ep.on("session", callback);
  return ep;
}

export async function connect(address, options = {}) {
  const { host, port } = __parseAddr(address);
  if (options === null || typeof options !== "object") {
    const err = new TypeError(`The "options" argument must be of type object.`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const alpn = __normAlpn(options.alpn, "options.alpn", true);
  // 校验提在 native 调用之外（包进 __callNative 会被重包成 ERR_QUIC_ERROR）。
  const servername = options.servername === undefined ? undefined : __needStr(options.servername, "options.servername");
  const caPem = options.ca === undefined ? undefined : String(options.ca);
  const idleMs = __normIdle(options.idleTimeout);
  const ccName = __normCc(options.cc);
  const id = __callNative(() => __wjs_quic_connect(JSON.stringify({
    host, port, alpn,
    servername,
    ca_pem: caPem,
    reject_unauthorized: options.rejectUnauthorized,
    idle_timeout_ms: idleMs, cc: ccName,
  })));
  return new QuicSession(String(id), {});
}

const __api = { listen, connect, QuicEndpoint, QuicSession, QuicStream, QuicError, CC_ALGO_RENO, CC_ALGO_CUBIC, CC_ALGO_BBR };
export default __api;
"#;
