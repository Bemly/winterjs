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

use mozjs::context::JSContext;
use mozjs::jsapi::JSObject;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;


use crate::jsapi_glue::{get_prop_value, report_error, value_to_string, wrap_cx, Frame};
use crate::state;

use super::quic_driver::{spawn_driver, spawn_h3_client, spawn_h3_server};
use super::quic_tls::{
    build_client_tls, build_server_tls, build_transport, handshake_info, ConnectOpts,
    ListenOpts,
};
pub use super::quic_driver::{QuicH3Cmd, QuicSessCmd, QuicStreamCmd};
pub use super::quic_api::{
    quic_h3_request, quic_h3_respond, quic_sess_max_dgram, quic_sess_open,
    quic_sess_send_dgram, quic_stream_attach, quic_stream_finish, quic_stream_reset,
    quic_stream_stop, quic_stream_write,
};


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
pub(crate) fn arg_id(cx: &mut JSContext, frame: &Frame, i: u32) -> Option<u64> {
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
pub(crate) fn arg_json(cx: &mut JSContext, frame: &Frame, i: u32, what: &str) -> Option<serde_json::Value> {
    if frame.argc() <= i {
        report_error(cx, &format!("TypeError: {what} needs options"));
        return None;
    }
    serde_json::from_str(&value_to_string(cx, frame.arg(i))).ok().or_else(|| {
        report_error(cx, &format!("TypeError: {what} options are not valid"));
        None
    })
}

pub(crate) fn sock_addr(host: &str, port: u16) -> Result<SocketAddr, String> {
    use std::net::ToSocketAddrs as _;
    let h = if host.is_empty() || host == "localhost" { "127.0.0.1" } else { host };
    format!("{h}:{port}")
        .to_socket_addrs()
        .map_err(|e| format!("TypeError: bad address {host}:{port} ({e})"))?
        .next()
        .ok_or_else(|| format!("TypeError: bad address {host}:{port} (unresolvable)"))
}

/// 起监听。`__wjs2_quic_listen(optsJson)` → endpoint id 串（bind 失败同步抛错）。
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

/// endpoint 本地地址。`__wjs2_quic_ep_addr(id)` → `"ip:port"`。
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
/// `__wjs2_quic_ep_close(id)` → undefined。
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

/// 登记 endpoint JS 目标。`__wjs2_quic_ep_attach(id, target)` → undefined。
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

/// 发起连接。`__wjs2_quic_connect(optsJson)` → session id 串（握手异步）。
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

/// 登记会话 JS 目标。`__wjs2_quic_sess_attach(id, target)` → undefined。
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

/// 会话信息。`__wjs2_quic_sess_info(id)` → JSON `{secure,local,remote,alpn,servername}`。
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

/// 会话统计（真值子集）。`__wjs2_quic_sess_stats(id)` → JSON。
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
/// `__wjs2_quic_sess_close(id, code)` → undefined。
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

/// 内嵌 ESM 源（`node:quic`；§0.9 按域分块：`quic.js` 全量，concat 字节恒等）。
pub const SOURCE: &str = include_str!("quic.js");
