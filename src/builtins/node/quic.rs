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

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use mozjs::context::JSContext;
use mozjs::jsapi::JSObject;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

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

/// 会话收件箱事件（9g-2 加流/数据报变体）。
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
                        let watch_conn = conn.clone();
                        let watch_inbox = inbox.clone();
                        let watcher = tokio::spawn(async move {
                            let (code, reason) = close_info(watch_conn.closed().await);
                            let _ = watch_inbox.send(QuicEvent::SessionClose { id: sess, code, reason });
                        });
                        let local = ep_local;
                        state::quic_sess_insert(sess, watcher.abort_handle(), local, remote.clone());
                        state::quic_sess_set_conn(sess, conn);
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
    let handle = tokio::spawn(async move {
        match endpoint.connect(addr, &servername) {
            Ok(connecting) => match connecting.await {
                Ok(conn) => {
                    state::quic_sess_set_conn(id, conn.clone());
                    // 发起侧 handshake_data 的 server_name 恒 None（quinn 口径），
                    // 用请求时的 servername（TLS 失败即无握手，无此事件）。
                    let _ = inbox.send(QuicEvent::SessionSecure {
                        id,
                        alpn: alpn_report,
                        servername,
                    });
                    let (code, reason) = close_info(conn.closed().await);
                    let _ = inbox.send(QuicEvent::SessionClose { id, code, reason });
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
        endpoint.close(0u32.into(), b"bye");
    });
    state::quic_sess_insert(id, handle.abort_handle(), local_report, remote_report);
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

function __quicErr(e, dflt = "ERR_QUIC_ERROR") {
  const m = String((e && e.message) || e);
  const err = new QuicError(m, dflt);
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
      this.emit("error", new QuicError(String(payload), "ERR_QUIC_HANDSHAKE"));
    } else if (kind === "close") {
      if (this.__closed) return;
      this.__closed = true;
      const info = JSON.parse(String(payload));
      this.emit("close", info.code, info.reason);
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

const __api = { listen, connect, QuicEndpoint, QuicSession, QuicError, CC_ALGO_RENO, CC_ALGO_CUBIC, CC_ALGO_BBR };
export default __api;
"#;
