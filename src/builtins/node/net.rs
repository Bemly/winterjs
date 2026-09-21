//! `node:net`：TCP Socket/Server（tokio net 底座，dependencies2 §9d 口径）。
/// 事件模型与 child.rs 同构：task → channel → 事件循环 pump → `dispatch` 调
/// target 的 `__ev(kind, payload)` 钩子（prelude Socket/Server 翻译成 EventEmitter
/// 事件）。收尾单出口：半关旗（half_read/half_write）齐或读端出错才发 Close。
/// 偏差记档：write 错误经读端 EOF/错统一收尾（不单独 Error）；accept 瞬时错误
/// 不细分；write 回调无 flush 语义（即刻）。

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::context::JSContext;
use mozjs::jsapi::JSObject;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{get_prop_value, report_error, wrap_cx, Frame};
use crate::state;

/// 网络事件（socket 与 server 共用通道，`id` 区分）。
pub struct NetEvent {
    pub id: u64,
    pub kind: NetKind,
}

pub enum NetKind {
    /// 客户端已连接（payload 带 local 地址 JSON）。
    Connect { local: Option<std::net::SocketAddr> },
    Data { data_b64: String },
    /// 远端 FIN。
    End,
    Error { code: String, msg: String },
    /// socket 关闭（单次；派发后 target/entry 一并清除）。
    Close,
    /// server 开始监听。
    Listening { addr: String, port: u16 },
    /// UDS server 开始监听（payload = path 串）。
    ListeningUds { path: String },
    /// server 收到连接（conn 侧 entry/泵已就绪，等 JS attach target）。
    Connection {
        conn_id: u64,
        remote_addr: String,
        remote_port: u16,
        local_addr: String,
        local_port: u16,
    },
    /// UDS server 收到连接（conn 侧 entry/泵已就绪，地址全 undefined）。
    ConnectionUds { conn_id: u64 },
    ServerError { code: String, msg: String },
    ServerClose,
    /// dgram：绑定完成。
    DgramListening { addr: String, port: u16 },
    /// dgram：收到数据报。
    DgramMessage { data_b64: String, address: String, port: u16, family: u8 },
    /// dgram：connect 生效（task 已记默认远端；JS 侧置位并发 'connect'）。
    DgramConnect,
    /// dgram：send 失败（路由回 seq 对应的 send 回调；无回调 JS 侧转 error 事件）。
    /// node 口径：send 失败不杀 socket。
    DgramSendError { code: String, msg: String, seq: u64 },
    /// dgram：send 完成（回调 (null, bytes) 经此异步触发——uv_udp_send 完成回调同型）。
    DgramSendOk { seq: u64, bytes: usize },
    // ── http2（Phase 9d-7；与 net 共通道，零新 channel）─────────────────────
    /// h2 服务端收到完整请求（ev.id = server id；整收口径，http 记档同款）。
    /// 10f：authority/trailers/peer 面（compat 伪头合成 + trailers 事件）。
    H2Request {
        conn_id: u64,
        stream_id: u64,
        method: String,
        path: String,
        authority: String,
        headers: String,
        trailers_json: String,
        body_b64: String,
        peer: String,
    },
    /// h2 流事件（客户端 ev.id = session id：response/data/trailers/end/error/
    /// aborted；服务端 ev.id = server id：aborted）。
    H2Stream { stream_id: u64, what: String, payload: String },
    /// h2 客户端 session 终结（单次；派发后 purge）。
    H2SessionClose,
}

/// socket 命令（写/半关/硬关；写端 task 消费。SendTo 为 dgram 专用）。
pub enum NetCmd {
    Write(Vec<u8>),
    End,
    Close,
    SendTo { data: Vec<u8>, addr: String, seq: u64 },
    // ── dgram 10a（组播/广播/TTL/connect 全家；task 内同步 setsockopt，
    // 失败走 Error 事件——真机同步抛的偏差记档，见 dgram.rs）────────────────
    /// SO_BROADCAST 开关。
    DgramBroadcast(bool),
    /// 组播环回开关。
    DgramMulticastLoop(bool),
    /// 组播 TTL（0-255，JS 侧已验范围）。
    DgramMulticastTtl(u8),
    /// 单播 TTL（1-255，JS 侧已验范围）。
    DgramTtl(u32),
    /// 加组播组（点分十进制串；v6 用索引串，task 内分流）。
    DgramJoin { multi: String, iface: String },
    /// 退组播组。
    DgramLeave { multi: String, iface: String },
    /// SSM 加组（源+组；v6 走 MCAST_JOIN_SOURCE_GROUP）。
    DgramJoinSource { multi: String, iface: String, source: String },
    /// SSM 退组。
    DgramLeaveSource { multi: String, iface: String, source: String },
    /// 出站组播接口（v4 地址串 / v6 索引或 %scope 形）。
    DgramMulticastInterface { addr: String },
    /// 记默认远端（task 级 connect，无内核过滤，记档）。
    DgramConnect { addr: String },
    /// 清默认远端。
    DgramDisconnect,
    // ── http2 ─────────────────────────────────────────────────────────────
    /// 服务端应答头（发往 conn id；stream_id 由 H2Request 事件给出）。
    /// 10f 流式化：头与体分离，体经 H2RespondData/H2RespondEnd 增量下发。
    H2Respond {
        stream_id: u64,
        status: u16,
        headers: String,
    },
    /// 服务端应答体块（须在 H2Respond 之后；task 侧早到则暂存 outbox）。
    H2RespondData { stream_id: u64, data_b64: String },
    /// 服务端应答收尾（trailer 可空 `[]`）。
    H2RespondEnd {
        stream_id: u64,
        trailers_json: String,
    },
    /// 服务端 RST_STREAM（code: 0=NO_ERROR 干净关，2=INTERNAL_ERROR）。
    H2RespondReset { stream_id: u64, code: u32 },
    /// 客户端开流（发往 session id；stream_id 由 JS 侧会话内分配）。
    H2Open {
        stream_id: u64,
        headers: String,
        body_b64: String,
    },
    /// 客户端上传 trailer 帧（waitForTrailers 口径；补发后 EOS）。
    H2OpenTrailers { stream_id: u64, trailers_json: String },
}

pub(crate) use super::net_pumps::spawn_pumps;
use super::net_pumps::{opt_num, set_rval_str};
pub use super::net_pumps::{
    net_attach, net_bind, net_connect, net_destroy, net_end, net_isip, net_listen,
    net_write,
};


/// SO_REUSEPORT TCP bind（BoundSocket `reusePort` 选项；boundsocket 套件双绑定点名，
/// 真机 macOS/Linux 均支持）。UNSAFE-BOUNDARY：libc socket FFI——前置条件：
/// `socket()` 返回的 fd 由本函数独占管理（成功路径经 `FromRawFd` 接管为
/// `TcpListener`，任一步失败即 `close(fd)` 回收后再返回）；setsockopt/bind/listen
/// 参数全为栈上值、无别名。覆盖：`tests/node/net.rs::phase10f_net_validators_family`
/// reusePort 双绑 + 主流平台 setsockopt 恒成功（macOS/Linux ≥3.9）。
#[cfg(unix)]
pub(crate) fn bind_tcp_reuseport(addr_str: &str) -> std::io::Result<std::net::TcpListener> {
    use std::net::{IpAddr, TcpListener};
    use std::os::fd::FromRawFd;
    let addr: std::net::SocketAddr = addr_str.parse().map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid address")
    })?;
    let mut sa4: libc::sockaddr_in;
    let mut sa6: libc::sockaddr_in6;
    let (domain, ptr, slen): (libc::c_int, *const libc::sockaddr, libc::socklen_t) = match addr.ip() {
        IpAddr::V4(v4) => {
            sa4 = unsafe { std::mem::zeroed() };
            sa4.sin_family = libc::AF_INET as libc::sa_family_t;
            sa4.sin_port = addr.port().to_be();
            sa4.sin_addr.s_addr = u32::from(v4).to_be();
            (libc::AF_INET, &sa4 as *const _ as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t)
        }
        IpAddr::V6(v6) => {
            sa6 = unsafe { std::mem::zeroed() };
            sa6.sin6_family = libc::AF_INET6 as libc::sa_family_t;
            sa6.sin6_port = addr.port().to_be();
            sa6.sin6_addr.s6_addr = v6.octets();
            (libc::AF_INET6, &sa6 as *const _ as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t)
        }
    };
    unsafe {
        let fd = libc::socket(domain, libc::SOCK_STREAM, 0);
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let one: libc::c_int = 1;
        let mut ok = libc::setsockopt(
            fd, libc::SOL_SOCKET, libc::SO_REUSEPORT,
            &one as *const libc::c_int as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        ) == 0;
        if ok {
            ok = libc::bind(fd, ptr, slen) == 0;
        }
        if ok {
            ok = libc::listen(fd, 511) == 0;
        }
        if !ok {
            let err = std::io::Error::last_os_error();
            libc::close(fd);
            return Err(err);
        }
        Ok(TcpListener::from_raw_fd(fd))
    }
}

#[cfg(not(unix))]
pub(crate) fn bind_tcp_reuseport(addr_str: &str) -> std::io::Result<std::net::TcpListener> {
    // 非 unix 无 SO_REUSEPORT（win 记档，同 node）：直接 std bind。
    std::net::TcpListener::bind(addr_str)
}

/// `__wjs_net_unhold(token)`：释放 BoundSocket TCP 占位 listener（close/adopt；未知 token 静默）。
pub unsafe extern "C" fn net_unhold(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(token) = opt_num(&frame, 0) else {
        report_error(&mut cx, "TypeError: unhold: token must be a number");
        return false;
    };
    state::net_hold_take(token as u64);
    true
}

/// `__wjs_net_fd(token)` → fd 串（BoundSocket fd() 真 fd 面；未知回 "-1"）。
pub unsafe extern "C" fn net_fd(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(token) = opt_num(&frame, 0) else {
        report_error(&mut cx, "TypeError: fd: token must be a number");
        return false;
    };
    set_rval_str(&mut cx, &frame, &state::net_hold_fd(token as u64).to_string());
    true
}

/// `__wjs_net_ref(id)` / `__wjs_net_unref(id)`：ref 真计数（10a；未知 id 静默，
/// Node 口径 ref/unref 不抛）。
pub unsafe extern "C" fn net_ref(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = opt_num(&frame, 0) else {
        report_error(&mut cx, "TypeError: ref: id must be a number");
        return false;
    };
    state::net_set_ref(id as u64, true);
    true
}

/// SAFETY: 同 net_ref。
pub unsafe extern "C" fn net_unref(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = opt_num(&frame, 0) else {
        report_error(&mut cx, "TypeError: unref: id must be a number");
        return false;
    };
    state::net_set_ref(id as u64, false);
    true
}

// ── 事件循环派发 ────────────────────────────────────────────────────────────

/// 网络事件派发：调 target 的 `__ev(kind, payload)`（payload 空串或 JSON）。
/// 前置条件：cx 已进入 global 所属 realm（事件循环上下文）。
pub fn dispatch(
    cx: &mut JSContext,
    global: *mut JSObject,
    ev: NetEvent,
    err: crate::runtime::ErrorSource<'_>,
) -> Result<(), crate::error::Error> {
    let failed = |cx: &mut JSContext| match err {
        crate::runtime::ErrorSource::Script { source, filename } => {
            crate::jsapi_glue::pending_exception_error(cx, global, source, filename)
        }
        crate::runtime::ErrorSource::Module { url } => crate::modules::module_error(cx, url),
    };
    // connection 事件：server target 必须在；conn target 由 JS attach（此处不读）。
    // UDS：地址全 undefined（真机实证：remote/local 全 undefined，address() 回 {}）。
    if let NetKind::ConnectionUds { conn_id } = &ev.kind {
        let Some(target) = state::net_target(ev.id) else {
            state::net_purge(*conn_id);
            return Ok(());
        };
        rooted!(&in(cx) let target_r = target); // §4.80：拷贝值立即入槽防 GC 搬移
        let target = target_r.get();
        if !target.is_object() {
            state::net_purge(*conn_id);
            return Ok(());
        }
        rooted!(&in(cx) let t: *mut JSObject = target.to_object());
        let Some(fun) = get_prop_value(cx, t.get(), c"__ev") else {
            return Err(failed(cx));
        };
        rooted!(&in(cx) let fun_r = fun); // §4.80：裸 JSVal 跨 json!/to_jsval 分配即悬垂
        let json = serde_json::json!({ "connId": conn_id, "uds": true }).to_string();
        if with_str_args(cx, global, fun_r.get(), "connection", &json).is_none() {
            return Err(failed(cx));
        }
        return Ok(());
    }
    if let NetKind::Connection { conn_id, remote_addr, remote_port, local_addr, local_port } =
        &ev.kind
    {
        let Some(target) = state::net_target(ev.id) else {
            state::net_purge(*conn_id); // server 已 gone：丢弃连接防泄漏
            return Ok(());
        };
        rooted!(&in(cx) let target_r = target); // §4.80：拷贝值立即入槽防 GC 搬移
        let target = target_r.get();
        if !target.is_object() {
            state::net_purge(*conn_id);
            return Ok(());
        }
        rooted!(&in(cx) let t: *mut JSObject = target.to_object());
        let Some(fun) = get_prop_value(cx, t.get(), c"__ev") else {
            return Err(failed(cx));
        };
        rooted!(&in(cx) let fun_r = fun); // §4.80：裸 JSVal 跨 json!/to_jsval 分配即悬垂
        let json = serde_json::json!({
            "connId": conn_id,
            "remoteAddress": remote_addr, "remotePort": remote_port,
            "localAddress": local_addr, "localPort": local_port,
        })
        .to_string();
        if with_str_args(cx, global, fun_r.get(), "connection", &json).is_none() {
            return Err(failed(cx));
        }
        return Ok(());
    }
    let Some(target) = state::net_target(ev.id) else {
        if matches!(ev.kind, NetKind::Close | NetKind::ServerClose | NetKind::H2SessionClose) {
            state::net_purge(ev.id);
        }
        return Ok(());
    };
    rooted!(&in(cx) let target_r = target); // §4.80：拷贝值立即入槽防 GC 搬移
    let target = target_r.get();
    if !target.is_object() {
        if matches!(ev.kind, NetKind::Close | NetKind::ServerClose | NetKind::H2SessionClose) {
            state::net_purge(ev.id);
        }
        return Ok(());
    }
    rooted!(&in(cx) let t: *mut JSObject = target.to_object());
    let Some(fun) = get_prop_value(cx, t.get(), c"__ev") else {
        if matches!(ev.kind, NetKind::Close | NetKind::ServerClose | NetKind::H2SessionClose) {
            state::net_purge(ev.id);
        }
        return Err(failed(cx));
    };
    rooted!(&in(cx) let fun_r = fun); // §4.80：裸 JSVal 跨 json!/to_jsval 分配即悬垂
    let is_conn_close = matches!(&ev.kind, NetKind::H2Stream { what, .. } if what == "connClose");
    let (kind, payload): (&str, String) = match &ev.kind {
        NetKind::Connect { local } => {
            ("connect", serde_json::json!({ "local": local }).to_string())
        }
        NetKind::Data { data_b64 } => ("data", data_b64.clone()),
        NetKind::End => ("end", String::new()),
        NetKind::Error { code, msg } => {
            ("error", serde_json::json!({ "code": code, "msg": msg }).to_string())
        }
        NetKind::Close => ("close", String::new()),
        NetKind::DgramSendError { code, msg, seq } => (
            "senderr",
            serde_json::json!({ "code": code, "msg": msg, "seq": seq }).to_string(),
        ),
        NetKind::DgramSendOk { seq, bytes } => (
            "sendok",
            serde_json::json!({ "seq": seq, "bytes": bytes }).to_string(),
        ),
        NetKind::Listening { addr, port } => {
            ("listening", serde_json::json!({ "addr": addr, "port": port }).to_string())
        }
        NetKind::ListeningUds { path } => {
            ("listening", serde_json::json!({ "uds": true, "path": path }).to_string())
        }
        NetKind::ServerError { code, msg } => {
            // node 形："listen EADDRINUSE: address already in use <path>"（已有 listen 头透传；
            // TCP bind 形 "CODE: <os msg>"（如 "EADDRINUSE: Address already in use"）取冒号后接全形）。
            let full = if msg.starts_with("listen ") {
                msg.clone()
            } else if let Some(rest) = msg.split_once(':') {
                format!("listen {code}:{}", rest.1)
            } else {
                format!("listen {code}: {msg}")
            };
            ("error", serde_json::json!({ "code": code, "msg": full }).to_string())
        }
        NetKind::ServerClose => ("close", String::new()),
        NetKind::DgramListening { addr, port } => {
            ("listening", serde_json::json!({ "addr": addr, "port": port }).to_string())
        }
        NetKind::DgramMessage { data_b64, address, port, family } => (
            "message",
            serde_json::json!({ "data": data_b64, "address": address, "port": port, "family": family })
                .to_string(),
        ),
        NetKind::DgramConnect => ("connect", String::new()),
        // http2：request 派发给 server target；stream/session 派发给 session target
        NetKind::H2Request { conn_id, stream_id, method, path, authority, headers, trailers_json, body_b64, peer } => (
            "request",
            serde_json::json!({
                "connId": conn_id, "streamId": stream_id,
                "method": method, "path": path, "authority": authority,
                "headers": headers, "trailers": trailers_json, "body": body_b64,
                "peer": peer,
            })
            .to_string(),
        ),
        NetKind::H2Stream { stream_id, what, payload } => (
            what.as_str(),
            serde_json::json!({ "streamId": stream_id, "payload": payload }).to_string(),
        ),
        NetKind::H2SessionClose => ("close", String::new()),
        NetKind::Connection { .. } | NetKind::ConnectionUds { .. } => unreachable!(),
    };
    let ok = with_str_args(cx, global, fun_r.get(), kind, &payload);
    let closed = matches!(
        ev.kind,
        NetKind::Close | NetKind::ServerClose | NetKind::H2SessionClose
    );
    if closed || is_conn_close {
        // connClose：连 net_target 一起清（serve_conn 尾部不再自 purge，
        // 保证本事件派发时 target 仍在）
        state::net_purge(ev.id);
    }
    if ok.is_none() {
        return Err(failed(cx));
    }
    Ok(())
}

/// 字符串双参调用（`__ev(kind, payload)`；值先 rooted 再进 call_two）。
fn with_str_args(
    cx: &mut JSContext,
    global: *mut JSObject,
    fun: JSVal,
    kind: &str,
    payload: &str,
) -> Option<JSVal> {
    rooted!(&in(cx) let g = global); // global 裸指针入 GC 槽（§4.80）
    rooted!(&in(cx) let f = fun); // fun 裸 JSVal 入槽
    rooted!(&in(cx) let mut a = UndefinedValue());
    rooted!(&in(cx) let mut b = UndefinedValue());
    kind.to_jsval(cx, a.handle_mut());
    payload.to_jsval(cx, b.handle_mut());
    crate::jsapi_glue::call_two(cx, g.get(), f.get(), a.get(), b.get())
}

/// 内嵌 ESM 源（`node:net`；Socket/Server 建立在 node:events 之上）。

/// 内嵌 ESM 源（`node:net`；§0.9 按域分块：`net_socket.js` Socket 类 +
/// `net_server.js` Server/BoundSocket/校验族，concat 字节恒等）。
pub const SOURCE: &str = concat!(
    include_str!("net_socket.js"),
    include_str!("net_server.js"),
);
