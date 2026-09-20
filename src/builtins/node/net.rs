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

use crate::jsapi_glue::{get_prop_value, report_error, value_to_string, wrap_cx, Frame};
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

fn b64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// 数字实参（缺省/非数 → None）。
fn opt_num(frame: &Frame, i: u32) -> Option<f64> {
    let v = frame.arg(i);
    if v.is_number() { Some(v.to_number()) } else { None }
}

fn set_rval_str(cx: &mut JSContext, frame: &Frame, s: &str) {
    rooted!(&in(cx) let mut v = UndefinedValue());
    s.to_jsval(cx, v.handle_mut());
    frame.set_rval(v.get());
}
// ── 泵（connect 与 accept 共用；收尾单出口 = 读端 task）────────────────────
/// 泛型拆分半部：TCP（`OwnedReadHalf/OwnedWriteHalf`）与 TLS（`tokio::io` split 半部）
/// 共用（Phase 9d-6 `node:tls` 复用，零重复实现；行为与单态版一致）。
pub(crate) fn spawn_pumps<R, W>(
    id: u64,
    r: R,
    w: W,
    ev_tx: tokio::sync::mpsc::UnboundedSender<NetEvent>,
    mut cmd_rx: tokio::sync::mpsc::UnboundedReceiver<NetCmd>,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let handle = tokio::runtime::Handle::current();
    let mut r = r;
    let mut w = w;
    let ev_w = ev_tx.clone();
    // 写端 task：消费 cmd；退出（drop w）→ 读端见 EOF/错，走统一收尾。
    handle.spawn(async move {
        use tokio::io::AsyncWriteExt as _;
        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                NetCmd::Write(bytes) => {
                    if w.write_all(&bytes).await.is_err() {
                        break;
                    }
                }
                NetCmd::End => {
                    let _ = w.shutdown().await;
                    if state::net_half_write(id) && state::net_close_once(id) {
                        // 读端早已 EOF：本端是最后一个半关者，负责收尾
                        let _ = ev_w.send(NetEvent { id, kind: NetKind::Close });
                    }
                    break;
                }
                NetCmd::Close => {
                    // 显式 shutdown 再退（10b https 保活案：tokio-rustls 写半部
                    // drop 不发 close_notify，对端读端永 block，双边死锁；
                    // TCP 写半部 drop 自带 FIN，故 9d 从未暴露）。
                    let _ = w.shutdown().await;
                    break;
                },
                NetCmd::SendTo { .. } => {} // dgram 专用（net socket 不产生）
                // 10a dgram sockopt 全家（同上，net socket 不产生）
                NetCmd::DgramBroadcast(_)
                | NetCmd::DgramMulticastLoop(_)
                | NetCmd::DgramMulticastTtl(_)
                | NetCmd::DgramTtl(_)
                | NetCmd::DgramJoin { .. }
                | NetCmd::DgramLeave { .. }
                | NetCmd::DgramJoinSource { .. }
                | NetCmd::DgramLeaveSource { .. }
                | NetCmd::DgramMulticastInterface { .. }
                | NetCmd::DgramConnect { .. }
                | NetCmd::DgramDisconnect => {}
                // http2 命令走 h2 conn/session task（本泵不产生，见 http2.rs）
                NetCmd::H2Respond { .. }
                | NetCmd::H2RespondData { .. }
                | NetCmd::H2RespondEnd { .. }
                | NetCmd::H2RespondReset { .. }
                | NetCmd::H2Open { .. }
                | NetCmd::H2OpenTrailers { .. } => {}
            }
        }
        state::net_writer_exit(id);
        // 读端已先走（EOF/错后 break）：写端是最后一个退出者，补发 Close
        // （10b https 保活案：destroy 后读端见 FIN 先退，写端退出时无人收尾）。
        if state::net_reader_gone(id) && state::net_close_once(id) {
            let _ = ev_w.send(NetEvent { id, kind: NetKind::Close });
        }
    });
    // 读端 task：EOF → End（两侧半关齐则 Close 收尾）；错 → Error + Close。
    handle.spawn(async move {
        use tokio::io::AsyncReadExt as _;
        let mut buf = vec![0u8; 65536];
        loop {
            match r.read(&mut buf).await {
                Ok(0) => {
                    let _ = ev_tx.send(NetEvent { id, kind: NetKind::End });
                    let _ = state::net_half_read(id);
                    // writer 已死（destroy 路径）：End 命令无人消费，此处直接收尾
                    if state::net_writer_dead(id) && state::net_close_once(id) {
                        let _ = ev_tx.send(NetEvent { id, kind: NetKind::Close });
                    }
                    state::net_reader_done(id);
                    break;
                }
                Ok(n) => {
                    let _ = ev_tx.send(NetEvent {
                        id,
                        kind: NetKind::Data { data_b64: b64(&buf[..n]) },
                    });
                }
                Err(e) => {
                    let code = crate::builtins::node::fs::io_code(&e);
                    let _ = ev_tx.send(NetEvent {
                        id,
                        kind: NetKind::Error { code: code.into(), msg: format!("{code}: {e}") },
                    });
                    state::net_reader_done(id);
                    if state::net_close_once(id) {
                        let _ = ev_tx.send(NetEvent { id, kind: NetKind::Close });
                    }
                    break;
                }
            }
        }
    });
}

// ── natives ─────────────────────────────────────────────────────────────────

/// `__wjs_net_connect(host, port, target)` → id。target 为 prelude Socket 对象。
pub unsafe extern "C" fn net_connect(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 || !frame.arg(2).is_object() {
        report_error(&mut cx, "TypeError: connect internals missing target");
        return false;
    }
    // path 形 = unix domain socket（首参非数即 path；port 槽给空串）。
    // win 无 UnixStream：走 Error 事件面（io_code 无映射 → UNKNOWN，不抛 native 错）。
    let path_uds: Option<String> = if frame.arg(0).is_string() && opt_num(&frame, 1).is_none() {
        let p = value_to_string(&mut cx, frame.arg(0));
        if p.is_empty() { None } else { Some(p) }
    } else {
        None
    };
    let host = value_to_string(&mut cx, frame.arg(0));
    let port = opt_num(&frame, 1).unwrap_or(0.0);
    let target = frame.arg(2);
    let Some((id, ev_tx)) = state::net_alloc() else {
        report_error(&mut cx, "OperationError: net driver not installed");
        return false;
    };
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        report_error(&mut cx, "OperationError: no async runtime for connect");
        return false;
    };
    let cmd_rx = state::net_socket_add(id, target);
    set_rval_str(&mut cx, &frame, &id.to_string());
    // 第4参 noDelay（http agent 默认 true；Node net 默认 false；tokio set_nodelay 零依赖）。
    let no_delay = frame.argc() > 3 && frame.arg(3).is_boolean() && frame.arg(3).to_boolean();
    // client-adopt 源 path（frame.arg(3) 非空串即 bind 本端；frame.arg(4) 布尔 noDelay 占位后移）。
    // 真机语义：new Socket({handle: srcBound}) + connect({path: dst}) 时本端 bind src（SO_REUSEADDR 式覆盖）。
    let src_bind: Option<String> = if frame.argc() > 3 && frame.arg(3).is_string() {
        let p = value_to_string(&mut cx, frame.arg(3));
        if p.is_empty() { None } else { Some(p) }
    } else {
        None
    };
    if let Some(sock_path) = path_uds {
        // UDS：仅 unix；win 下 tokio UnixStream 不可用 → 直接 Error+Close 事件。
        #[cfg(unix)]
        handle.spawn(async move {
            // 本端源 bind（adopt 面；UnixSocket::new_stream → bind(src) → connect(dst)，
            // 真机 client-adopt 语义；残留先清，bind 失败不致命走直连）。
            if let Some(src) = src_bind {
                let _ = tokio::fs::remove_file(src.as_str()).await;
                let bound_conn: Result<tokio::net::UnixStream, String> = (|| async {
                    let sock = tokio::net::UnixSocket::new_stream()
                        .map_err(|e| format!("SRCBIND-NEWSOCK {e}"))?;
                    if let Err(e) = sock.bind(src.as_str()) {
                        let code = crate::builtins::node::fs::io_code(&e);
                        return Err(format!("SRCBIND {code}: {e}"));
                    }
                    sock.connect(sock_path.as_str()).await.map_err(|e| format!("DSTCONN {e}"))
                })()
                .await;
                if let Err(tagged) = &bound_conn {
                    let (code, msg) = if let Some(rest) = tagged.strip_prefix("SRCBIND ") {
                        let c = rest.split_whitespace().next().unwrap_or("UNKNOWN");
                        (c.to_string(), format!("{c}: {tagged}"))
                    } else if let Some(rest) = tagged.strip_prefix("SRCBIND-NEWSOCK ") {
                        (String::from("UNKNOWN"), format!("UNKNOWN: {rest}"))
                    } else {
                        let e2 = tagged.clone();
                        (String::from("UNKNOWN"), format!("UNKNOWN: {e2}"))
                    };
                    let _ = ev_tx.send(NetEvent {
                        id,
                        kind: NetKind::Error { code, msg },
                    });
                    let _ = ev_tx.send(NetEvent { id, kind: NetKind::Close });
                    return;
                }
                match bound_conn {
                    Err(_) => unreachable!(),
                    Ok(stream) => {
                        let _ = ev_tx.send(NetEvent { id, kind: NetKind::Connect { local: None } });
                        let (r, w) = stream.into_split();
                        spawn_pumps(id, r, w, ev_tx, cmd_rx);
                        return;
                    }
                }
            }
            match tokio::net::UnixStream::connect(sock_path.as_str()).await {
                Err(e) => {
                    let code = crate::builtins::node::fs::io_code(&e);
                    let _ = ev_tx.send(NetEvent {
                        id,
                        kind: NetKind::Error { code: code.into(), msg: format!("{code}: {e}") },
                    });
                    let _ = ev_tx.send(NetEvent { id, kind: NetKind::Close });
                }
                Ok(stream) => {
                    let _ = ev_tx.send(NetEvent { id, kind: NetKind::Connect { local: None } });
                    let (r, w) = stream.into_split();
                    spawn_pumps(id, r, w, ev_tx, cmd_rx);
                }
            }
        });
        #[cfg(not(unix))]
        handle.spawn(async move {
            let _ = ev_tx.send(NetEvent {
                id,
                kind: NetKind::Error { code: "ENOTSUP".into(), msg: "ENOTSUP: unix socket not supported".into() },
            });
            let _ = ev_tx.send(NetEvent { id, kind: NetKind::Close });
        });
        return true;
    }
    handle.spawn(async move {
        match tokio::net::TcpStream::connect((host.as_str(), port as u16)).await {
            Err(e) => {
                let code = crate::builtins::node::fs::io_code(&e);
                let _ = ev_tx.send(NetEvent {
                    id,
                    kind: NetKind::Error { code: code.into(), msg: format!("{code}: {e}") },
                });
                let _ = ev_tx.send(NetEvent { id, kind: NetKind::Close });
            }
            Ok(stream) => {
                let _ = stream.set_nodelay(no_delay);
                let local = stream.local_addr().ok();
                let _ = ev_tx.send(NetEvent { id, kind: NetKind::Connect { local } });
                let (r, w) = stream.into_split();
                spawn_pumps(id, r, w, ev_tx, cmd_rx);
            }
        }
    });
    true
}

/// `__wjs_net_isip(s)` → "0"|"4"|"6"（`net.isIP` 底座；std::net 解析）。
pub unsafe extern "C" fn net_isip(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 || !frame.arg(0).is_string() {
        set_rval_str(&mut cx, &frame, "0");
        return true;
    }
    let s = value_to_string(&mut cx, frame.arg(0));
    let n = if s.parse::<std::net::Ipv4Addr>().is_ok() {
        4
    } else if s.parse::<std::net::Ipv6Addr>().is_ok() {
        6
    } else {
        0
    };
    set_rval_str(&mut cx, &frame, &n.to_string());
    true
}

/// `__wjs_net_listen(port, host, target)` → id。bind 错误经 ServerError 事件。
pub unsafe extern "C" fn net_listen(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 || !frame.arg(2).is_object() {
        report_error(&mut cx, "TypeError: listen internals missing target");
        return false;
    }
    let Some(port) = opt_num(&frame, 0) else {
        report_error(&mut cx, "TypeError: listen: port must be a number");
        return false;
    };
    let host = value_to_string(&mut cx, frame.arg(1));
    let target = frame.arg(2);
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        report_error(&mut cx, "OperationError: no async runtime for listen");
        return false;
    };
    // listen path 形：frame.arg(1) 恒 "UDS:<path>[\n<modeBits>]" 标记（JS __doListen 约定；
    // modeBits 含 r 即 readableAll（0044），含 w 即 writableAll（0022），基 0600）。
    if let Some(rest) = host.strip_prefix("UDS:") {
        let (sock_path, mode_bits) = match rest.split_once('\n') {
            Some((p, m)) => (p.to_string(), m.to_string()),
            None => (rest.to_string(), String::new()),
        };
        let Some((id2, ev_tx2)) = state::net_alloc() else {
            report_error(&mut cx, "OperationError: net driver not installed");
            return false;
        };
        set_rval_str(&mut cx, &frame, &id2.to_string());
        let mut cmd_rx2 = state::net_socket_add(id2, target);
        #[cfg(unix)]
        {
            // node 口径：pipe bind 在 listen() 返回前同步落定——紧随其后的
            // 同步 cp 必须已见 socket 文件（cp-socket 套件；旧 task 内异步绑
            // 必现 ENOENT 竞态）。探活/清残留/bind/chmod 全同步（本地 syscall，
            // 无 I/O 等待），task 只接管已绑定的 listener 跑 accept。
            use std::os::unix::net::{UnixListener, UnixStream};
            enum SyncOut {
                Bound(UnixListener),
                Failed(String, String),
            }
            let sync_out: SyncOut = (|| {
                if std::fs::metadata(sock_path.as_str()).is_ok() {
                    // 探活：能连上即真占用 → EADDRINUSE；连不上即残留 → 清后重绑。
                    if UnixStream::connect(sock_path.as_str()).is_ok() {
                        return SyncOut::Failed("EADDRINUSE".into(), format!("listen EADDRINUSE: address already in use {sock_path}"));
                    }
                    if let Err(e) = std::fs::remove_file(sock_path.as_str()) {
                        let code = crate::builtins::node::fs::io_code(&e);
                        return SyncOut::Failed(code.to_string(), format!("{code}: {e}"));
                    }
                }
                match UnixListener::bind(sock_path.as_str()) {
                    Ok(l) => SyncOut::Bound(l),
                    Err(e) => {
                        let code = crate::builtins::node::fs::io_code(&e);
                        SyncOut::Failed(code.to_string(), format!("{code}: {e}"))
                    }
                }
            })();
            let listener = match sync_out {
                SyncOut::Failed(code, msg) => {
                    let _ = ev_tx2.send(NetEvent {
                        id: id2,
                        kind: NetKind::ServerError { code: code.into(), msg },
                    });
                    let _ = ev_tx2.send(NetEvent { id: id2, kind: NetKind::ServerClose });
                    return true;
                }
                SyncOut::Bound(l) => l,
            };
            // chmod：基 0600 + readableAll 0044 + writableAll 0022（真机 pipe chmod 语义）。
            {
                use std::os::unix::fs::PermissionsExt as _;
                let mut mode: u32 = 0o600;
                if mode_bits.contains('r') { mode |= 0o044; }
                if mode_bits.contains('w') { mode |= 0o022; }
                let _ = std::fs::set_permissions(sock_path.as_str(), std::fs::Permissions::from_mode(mode));
            }
            let bound_path = sock_path.clone();
            handle.spawn(async move {
                // from_std 前必 nonblocking（否则 tokio panic）。
                let _ = listener.set_nonblocking(true);
                let Ok(listener) = tokio::net::UnixListener::from_std(listener) else {
                    let _ = ev_tx2.send(NetEvent {
                        id: id2,
                        kind: NetKind::ServerError { code: "UNKNOWN".into(), msg: "UNKNOWN: from_std failed".into() },
                    });
                    let _ = ev_tx2.send(NetEvent { id: id2, kind: NetKind::ServerClose });
                    return;
                };
                let _ = ev_tx2.send(NetEvent {
                    id: id2,
                    kind: NetKind::ListeningUds { path: bound_path },
                });
                loop {
                    tokio::select! {
                        acc = listener.accept() => {
                            match acc {
                                Err(_) => continue,
                                Ok((stream, _peer)) => {
                                    let (conn_id, conn_cmd_rx) = state::net_conn_add();
                                    let (r, w) = stream.into_split();
                                    spawn_pumps(conn_id, r, w, ev_tx2.clone(), conn_cmd_rx);
                                    let _ = ev_tx2.send(NetEvent {
                                        id: id2,
                                        kind: NetKind::ConnectionUds { conn_id },
                                    });
                                }
                            }
                        }
                        _ = cmd_rx2.recv() => break,
                    }
                }
                let _ = ev_tx2.send(NetEvent { id: id2, kind: NetKind::ServerClose });
            });
            return true;
        }
        #[cfg(not(unix))]
        handle.spawn(async move {
            let _ = ev_tx2.send(NetEvent {
                id: id2,
                kind: NetKind::ServerError { code: "ENOTSUP".into(), msg: "ENOTSUP: unix socket not supported".into() },
            });
            let _ = ev_tx2.send(NetEvent { id: id2, kind: NetKind::ServerClose });
        });
        return true;
    }
    let Some((id, ev_tx)) = state::net_alloc() else {
        report_error(&mut cx, "OperationError: net driver not installed");
        return false;
    };
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        report_error(&mut cx, "OperationError: no async runtime for listen");
        return false;
    };
    let mut cmd_rx = state::net_socket_add(id, target);
    set_rval_str(&mut cx, &frame, &id.to_string());
    // 第 4 参 "1"：SO_REUSEPORT 重绑（BoundSocket adopt 路径；host 含 ':' 走
    // [v6]:port 括号形）。非 reuse 路径维持 tokio tuple bind。
    let reuse = frame.argc() > 3 && frame.arg(3).is_string()
        && value_to_string(&mut cx, frame.arg(3)) == "1";
    let accept_handle = handle.clone();
    accept_handle.spawn(async move {
        let bound = if reuse {
            let addr_str = if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") };
            bind_tcp_reuseport(addr_str.as_str()).and_then(|l| {
                use std::os::fd::{FromRawFd, IntoRawFd};
                l.set_nonblocking(true)?;
                // SAFETY(tokio 契约)：std listener 非阻塞 + 本 runtime 线程内接管。
                let std_listener = unsafe { std::net::TcpListener::from_raw_fd(l.into_raw_fd()) };
                tokio::net::TcpListener::from_std(std_listener)
            })
        } else {
            tokio::net::TcpListener::bind((host.as_str(), port as u16)).await
        };
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
        loop {
            tokio::select! {
                acc = listener.accept() => {
                    match acc {
                        Err(_) => continue, // 瞬时 accept 错误（记档：不细分）
                        Ok((stream, peer)) => {
                            // 服务端 accept 的 socket 默认 noDelay（Node _http_server 口径）。
                            let _ = stream.set_nodelay(true);
                            let conn_local = stream
                                .local_addr()
                                .unwrap_or_else(|_| "0.0.0.0:0".parse::<std::net::SocketAddr>().expect("literal addr"));
                            let (conn_id, conn_cmd_rx) = state::net_conn_add();
                            let (r, w) = stream.into_split();
                            spawn_pumps(conn_id, r, w, ev_tx.clone(), conn_cmd_rx);
                            let _ = ev_tx.send(NetEvent {
                                id,
                                kind: NetKind::Connection {
                                    conn_id,
                                    remote_addr: peer.ip().to_string(),
                                    remote_port: peer.port(),
                                    local_addr: conn_local.ip().to_string(),
                                    local_port: conn_local.port(),
                                },
                            });
                        }
                    }
                }
                _ = cmd_rx.recv() => break, // Close 命令（或写端掉光）→ 停止 accept
            }
        }
        let _ = ev_tx.send(NetEvent { id, kind: NetKind::ServerClose });
    });
    true
}

/// `__wjs_net_attach(connId, target)`：server 连接的 target 事后登记。
pub unsafe extern "C" fn net_attach(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 || !frame.arg(1).is_object() {
        report_error(&mut cx, "TypeError: attach internals missing target");
        return false;
    }
    let Some(id) = opt_num(&frame, 0) else {
        report_error(&mut cx, "TypeError: attach: id must be a number");
        return false;
    };
    state::net_target_add(id as u64, frame.arg(1));
    true
}

/// `__wjs_net_write(id, dataBytes)`。
pub unsafe extern "C" fn net_write(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = opt_num(&frame, 0) else {
        report_error(&mut cx, "TypeError: write: id must be a number");
        return false;
    };
    let data = if frame.arg(1).is_string() {
        value_to_string(&mut cx, frame.arg(1)).into_bytes()
    } else {
        match crate::jsapi_glue::view_bytes(&mut cx, frame.arg(1), "write data") {
            Some(b) => b,
            None => return false,
        }
    };
    if !state::net_cmd(id as u64, NetCmd::Write(data)) {
        report_error(&mut cx, "ERR_STREAM_DESTROYED: write: socket is gone");
        return false;
    }
    true
}

/// `__wjs_net_end(id)`：半关写端（FIN）。
pub unsafe extern "C" fn net_end(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = opt_num(&frame, 0) else {
        report_error(&mut cx, "TypeError: end: id must be a number");
        return false;
    };
    state::net_cmd(id as u64, NetCmd::End);
    true
}

/// `__wjs_net_destroy(id)`：硬关（socket 或 server 通用）。
pub unsafe extern "C" fn net_destroy(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = opt_num(&frame, 0) else {
        report_error(&mut cx, "TypeError: destroy: id must be a number");
        return false;
    };
    state::net_cmd(id as u64, NetCmd::Close);
    true
}

/// `__wjs_net_bind(host, port, path[, reuse])` → "port"（BoundSocket 同步 bind 底座）。
/// 成功回绑定端口串；失败抛带 code/syscall 的 Error（EADDRINUSE/EACCES/EADDRNOTAVAIL/EINVAL）。
/// path 非空即 UDS bind（返回 path 回显标记 "UDS:<path>"）；第 4 参 "1" 即
/// SO_REUSEPORT bind（reusePort 选项；平台不支持时 setsockopt/bind 失败按既有错误面抛）。
pub unsafe extern "C" fn net_bind(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    use std::net::TcpListener as StdTcp;
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let host = value_to_string(&mut cx, frame.arg(0));
    let port = opt_num(&frame, 1).unwrap_or(0.0) as u16;
    let path = if frame.argc() > 2 && frame.arg(2).is_string() {
        value_to_string(&mut cx, frame.arg(2))
    } else {
        String::new()
    };
    // UDS bind（同步 std；残留先清；超长 path 即 EINVAL）。
    // 真机语义：BoundSocket 只预留 path（文件占位），connect/listen 时重 bind；
    // 故此处 bind 成功即 drop（关 listener 留文件作预留标记）。
    if !path.is_empty() {
        #[cfg(unix)]
        {
            use std::os::unix::net::UnixListener as StdUds;
            let _ = std::fs::remove_file(path.as_str());
            match StdUds::bind(path.as_str()) {
                Ok(_l) => {
                    drop(_l);
                    set_rval_str(&mut cx, &frame, &format!("UDS:{path}"));
                    return true;
                }
                Err(e) => {
                    let code = crate::builtins::node::fs::io_code(&e);
                    let code = if code == "UNKNOWN" { "EINVAL" } else { code };
                    report_error(&mut cx, &format!("BINDFAIL {code}: bind {code} {path}"));
                    return false;
                }
            }
        }
        #[cfg(not(unix))]
        {
            report_error(&mut cx, "ENOTSUP: bind ENOTSUP unix socket not supported");
            return false;
        }
    }
    // TCP bind（同步 std；port 0 由 OS 分配；err → code + syscall=bind）。
    // 占位 listener 由 Rust 侧持有（net_hold_add），close/adopt/listen 消费时释放——
    // 真机 fd 复用语义的次优：端口在占位期内真被占用（冲突/EADDRINUSE 全真），
    // adopt 预置端口恒有效（p19 51446/51447 漂移案）。fd() 仍回 -1 桩（不跨 JS 暴露）。
    let reuse = argc > 3
        && frame.arg(3).is_string()
        && value_to_string(&mut cx, frame.arg(3)) == "1";
    // IPv6 主机名走 [v6]:port 括号形（'::1:0' 会被当 v6 字面量误解析）。
    let addr = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let bound_listener = if reuse {
        bind_tcp_reuseport(&addr)
    } else {
        StdTcp::bind(addr.as_str())
    };
    match bound_listener {
        Ok(l) => {
            let p = l.local_addr().map(|a| a.port()).unwrap_or(port);
            let token = state::net_hold_add();
            state::net_hold_put(token, l);
            set_rval_str(&mut cx, &frame, &format!("{p}:{token}"));
            true
        }
        Err(e) => {
            let code = crate::builtins::node::fs::io_code(&e);
            let code = if code == "UNKNOWN" { "EADDRINUSE" } else { code };
            report_error(&mut cx, &format!("BINDFAIL {code}: bind {code} {addr}"));
            false
        }
    }
}

/// SO_REUSEPORT TCP bind（BoundSocket `reusePort` 选项；boundsocket 套件双绑定点名，
/// 真机 macOS/Linux 均支持）。UNSAFE-BOUNDARY：libc socket FFI——前置条件：
/// `socket()` 返回的 fd 由本函数独占管理（成功路径经 `FromRawFd` 接管为
/// `TcpListener`，任一步失败即 `close(fd)` 回收后再返回）；setsockopt/bind/listen
/// 参数全为栈上值、无别名。覆盖：`tests/node/net.rs::phase10f_net_validators_family`
/// reusePort 双绑 + 主流平台 setsockopt 恒成功（macOS/Linux ≥3.9）。
#[cfg(unix)]
fn bind_tcp_reuseport(addr_str: &str) -> std::io::Result<std::net::TcpListener> {
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
fn bind_tcp_reuseport(addr_str: &str) -> std::io::Result<std::net::TcpListener> {
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
pub const SOURCE: &str = r#"
import { EventEmitter } from "node:events";
import { StringDecoder } from "node:string_decoder";
import { __etAdd, __etRemove } from "node:internal/events/abort_listener";
const Buffer = globalThis.Buffer;

function __b64dec(s) {
  const bin = atob(s);
  const u8 = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) u8[i] = bin.charCodeAt(i);
  return u8;
}
function __chunkU8(chunk) {
  // 真机逐字（write-arguments 套件）：'The "chunk" argument must be of type string
  // or an instance of Buffer, TypedArray, or DataView.' + invalidArgTypeHelper。
  if (typeof chunk === "string") return new TextEncoder().encode(chunk);
  if (typeof Buffer !== "undefined" && Buffer.isBuffer(chunk)) return chunk;
  if (ArrayBuffer.isView(chunk) && !(chunk instanceof DataView) || chunk instanceof DataView) {
    if (chunk instanceof DataView) return new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength);
    return chunk;
  }
  if (chunk instanceof ArrayBuffer) return new Uint8Array(chunk);
  let __got;
  if (chunk === null) __got = "null";
  else if (chunk === undefined) __got = "undefined";
  else if (typeof chunk === "object") __got = `an instance of ${chunk.constructor?.name ?? "Object"}`;
  else __got = `type ${typeof chunk} (${String(chunk)})`;
  const e = new TypeError(`The "chunk" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received ${__got}`);
  e.code = "ERR_INVALID_ARG_TYPE"; throw e;
}
function __toU8(data, what) {
  if (typeof data === "string") return new TextEncoder().encode(data);
  if (data instanceof Uint8Array) return data;
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  throw new TypeError(`${what}: data must be string or BufferSource`);
}
function __netErr(code, msg) {
  const e = new Error(msg);
  e.code = code;
  return e;
}

class Socket extends EventEmitter {
  constructor(options) {
    super();
    // node Socket 构造（socket-constructor 套件）：number 形即 {fd: options}；
    // fd 校验 validateInt32(fd, 'fd', 0) 逐字——'foo' → ARG_TYPE、-1 → ERR_OUT_OF_RANGE。
    if (typeof options === "number") options = { fd: options };
    if (options !== null && typeof options === "object" && options.fd !== undefined) {
      if (typeof options.fd !== "number") {
        const e = new TypeError(`The "fd" argument must be of type number. Received ${__netGot(options.fd)}`);
        e.code = "ERR_INVALID_ARG_TYPE"; throw e;
      }
      if (!Number.isInteger(options.fd) || options.fd < 0 || options.fd > 2147483647) {
        const e = new RangeError(`The value of "fd" is out of range. It must be >= 0 && <= 2147483647. Received ${options.fd}`);
        e.code = "ERR_OUT_OF_RANGE"; throw e;
      }
    }
    // node 口径：new Socket({ handle: bound }) 消费 BoundSocket（adopt）。
    if (options && typeof options === "object" && options.handle !== undefined) {
      const h = options.handle;
      if (h && typeof h === "object" && typeof h.address === "function" && h.__boundPort !== undefined) {
        if (h.__adopted) {
          const e = new Error("The bound socket has already been adopted by a server or socket");
          e.code = "ERR_SOCKET_HANDLE_ADOPTED"; throw e;
        }
        h.__adopted = true;
        if (h.__udsPath !== undefined) __boundPaths.delete(h.__udsPath);
        if (h.__holdToken) { try { __wjs_net_unhold(h.__holdToken); } catch {} }
        this.__adoptHost = h.__boundHost ?? null;
        this.__adoptPort = h.__boundPort ?? 0;
        this.__adoptUds = h.__isPipe ? h.__udsPath : null;
        options = { ...options };
        delete options.handle;
      }
    }
    this.__id = 0;
    this.__enc = null;
    this.__dec = null;
    this.__peerFin = false;
    // signal 选项（abort-controller 套件 testConstructor* 三形）：构造即 aborted
    // → 异步 destroy(AbortError)（once('close') 以 error reject）；live → 注册
    // abort→destroy（直调 addEventListener 须入侧表供 events.listenerCount 读）。
    if (options !== null && typeof options === "object" && options.signal !== undefined) {
      const __sig = options.signal;
      if (__sig.aborted) {
        queueMicrotask(() => {
          const e = new Error("The operation was aborted"); e.name = "AbortError"; e.code = "ABORT_ERR";
          try { this.destroy(e); } catch {}
        });
      } else {
        const __sigHandler = () => {
          __etRemove(__sig, "abort", __sigHandler);
          const e = new Error("The operation was aborted"); e.name = "AbortError"; e.code = "ABORT_ERR";
          // 同 connect 侧：套件在 abort 之后才挂 once('close')，destroy 推 microtask。
          queueMicrotask(() => { try { this.destroy(e); } catch {} });
        };
        __sig.addEventListener("abort", __sigHandler, { once: true });
        __etAdd(__sig, "abort", __sigHandler);
      }
    }
    // node 口径（remote-address 双套件点名）：连接完成前 remote* 全 undefined
    // （够不上 null；发布点在 __ev-connect，不在 __realConnect）。
    this.remoteAddress = undefined;
    this.remotePort = undefined;
    this.remoteFamily = undefined;
    this.localAddress = null;
    this.localPort = null;
    this.readable = true;
    this.writable = true;
    this.destroyed = false;
    this.bytesWritten = 0;
    this.bytesRead = 0;
    this.__connected = false;   // 完成连接（connect/attach 后 true）
    this.__pendW = [];          // 连接完成前的缓冲写（node write 语义）
    // Node 默认 allowHalfOpen=false：收到远端 FIN（'end'）后自动 end 本端
    this.allowHalfOpen = !!(options && options.allowHalfOpen);
    // node 口径：_handle 只在连接存活期非空（构造时/close 后恒 null，真机 26 实测；
    // after-close 套件点名 `c._handle === null`）。连接建立（__realConnect/__attach*）
    // 时建桩，destroy/__ev-close 置空。setNoDelay/setKeepAlive 恒可调（无柄只缓存）。
    this._handle = null;
    this.__tos = 0;            // getTypeOfService 缓存（真机默认 0；连接前设置同样缓存）
    this.__kaState = null;     // setKeepAlive 去重缓存 [enable, delaySec, intervalSec, count]
    this.__hadError = false;   // close(hadError) 口径：error 发过即 true
    // transfer-guards 套件：Socket 不可经 MessagePort transfer（node kTransferList
    // 断言族的最保守近似：任何 Socket 在 transfer list 即 ERR_WORKER_HANDLE_NOT_
    // TRANSFERABLE；worker 侧 __normTransfer 认领，成功转移面本就另案）。
    try { (globalThis.__wjs_netXfer ??= new Map()).set(this, "net.Socket"); } catch {}
    this.__handleClosed = false;
    // node _handle 表面（10f：套件直接打补丁观测 setNoDelay/setKeepAlive 调用；
    // write-after-close 套件点名 _handle.close()；unref-timer 套件点名 _unrefTimer）。
    this.__makeHandle = () => {
      const self = this;
      return {
        setNoDelay: (enable) => { self.__noDelayApplied = enable; },
        setKeepAlive: (enable, delay, interval, count) => { self.__keepAliveApplied = [enable, delay, interval, count]; },
        close: () => { self.__handleClosed = true; queueMicrotask(() => self.destroy()); },
      };
    };
    if (options && typeof options === "object") {
      if (options.readable !== undefined) this.readable = !!options.readable;
      if (options.writable !== undefined) this.writable = !!options.writable;
    }
    // node 写背压：write 返回值 = 未超 highWaterMark（默认 16KB；hwm 0 恒 false）
    this.__hwm = options && options.highWaterMark !== undefined ? Number(options.highWaterMark) || 0 : 16384;
    this.__pendBytes = 0;
    // node 口径：bufferSize = 待刷写字节（本仓同步写队列，连接中缓冲计入，完成即 0）。
    Object.defineProperty(this, "bufferSize", { get: () => this.__pendBytes, enumerable: true });
    // 事件循环派发钩子：dispatch 以 global 为 this 调用，须预绑定（self 语义）
    this.__ev = this.__ev.bind(this);
    // Node Writable/Readable 内部面（ws 等 npm 库直接翻字段/调用）：
    // cork/uncork no-op（JS 层写本就不聚合，行为等价）；setNoDelay/
    // setKeepAlive no-op（tokio 写半直通，无 Nagle 可关）；_readableState
    // 最小桩（socketOnClose/socketOnEnd 读 endEmitted/length 判收尾路径）；
    // pause/resume no-op（读流无 JS 侧缓冲，整包即达）；
    // read 恒 null（数据已全经 data 事件投递，无缓冲可取——M5 dev 实测
    // `stream.resume is not a function`，缺桩即 TypeError）。
    // setTimeout 真实现见下（10f timers 对拍）。
    this.cork = () => this;
    this.uncork = () => this;
    // _handle 为空（未连接/已关闭）时 no-op 只缓存（after-close 套件：close 后调不抛）。
    this.setNoDelay = (enable) => { if (this._handle && typeof this._handle.setNoDelay === "function") { try { this._handle.setNoDelay(enable !== false); } catch {} } else { this.__noDelayApplied = enable !== false; } return this; };
    // node 口径（真机 26 实测）：setKeepAlive(enable, initialDelay, interval, count) /
    // setKeepAlive({enable, initialDelay, interval, count})；ms→s 下取整转发
    // （5000→5），缺省 interval/count 转发 undefined（JSON 呈 null，typeof 仍 undefined）；
    // 与上次四元组全同即跳过转发（server-keepalive 套件：同值首调被吞）。
    this.setKeepAlive = (enable, initialDelay, interval, count) => {
      if (enable !== null && typeof enable === "object") {
        const o = enable;
        enable = o.enable; initialDelay = o.initialDelay; interval = o.interval; count = o.count;
      }
      enable = enable === undefined ? false : !!enable;
      const toSec = (ms) => ms === undefined ? undefined : Math.floor(Number(ms) / 1000);
      const dSec = initialDelay === undefined ? 0 : toSec(initialDelay);
      const iSec = toSec(interval);
      const st = [enable, dSec, iSec, count];
      const pv = this.__kaState;
      const same = !!pv && pv[0] === st[0] && pv[1] === st[1] && pv[2] === st[2] && pv[3] === st[3];
      this.__kaState = st;
      if (!same && this._handle && typeof this._handle.setKeepAlive === "function") {
        try { this._handle.setKeepAlive(enable, dSec, iSec, count); } catch {}
      }
      return this;
    };
    // node 口径（真机 26 实测）：setTypeOfService 校验逐字（invalidArgTypeHelper 形/
    // OUT_OF_RANGE 双文案：非整数 "must be an integer"、越界 "must be >= 0 && <= 255"），
    // 链式返回自身；getTypeOfService 读缓存（连接前设置同样生效，tos 套件 2a 项）。
    this.setTypeOfService = (tos) => {
      if (typeof tos !== "number" || Number.isNaN(tos)) {
        const got = typeof tos === "string" ? `type string ('${tos}')` : `type ${typeof tos} (${String(tos)})`;
        const e = new TypeError(`The "tos" argument must be of type number. Received ${got}`);
        e.code = "ERR_INVALID_ARG_TYPE"; throw e;
      }
      if (!Number.isInteger(tos)) {
        const e = new RangeError(`The value of "tos" is out of range. It must be an integer. Received ${tos}`);
        e.code = "ERR_OUT_OF_RANGE"; throw e;
      }
      if (tos < 0 || tos > 255) {
        const e = new RangeError(`The value of "tos" is out of range. It must be >= 0 && <= 255. Received ${tos}`);
        e.code = "ERR_OUT_OF_RANGE"; throw e;
      }
      this.__tos = tos;
      return this;
    };
    this.getTypeOfService = () => this.__tos ?? 0;
    // 最小 pipe 面（write-connect-write 套件：server 侧 socket.pipe(socket) 回显）；
    // unpipe/_unrefTimer 桩（_parent 链安全，unref-timer 套件点名不抛）。
    this.pipe = (dest, options) => {
      this.on("data", (chunk) => { try { dest.write(chunk); } catch {} });
      if (!options || options.end !== false) this.on("end", () => { try { dest.end(); } catch {} });
      return dest;
    };
    this.unpipe = (dest) => this;
    this._unrefTimer = () => {};
    // pause/resume 真语义（server-pause-on-connect 套件）：paused 期 data 分节
    // 缓存（bytesRead 不进），resume 即冲刷。
    this.__paused = false;
    this.__pauseBuf = [];
    this.pause = () => { this.__paused = true; return this; };
    this.resume = () => {
      this.__paused = false;
      // node 流语义：resume 异步续流（同步冲刷会抢在调用方 resume 之后的
      // 语句前发 data——pause-on-connect 套件 stopped 旗现形）。
      queueMicrotask(() => {
        const buf = this.__pauseBuf;
        this.__pauseBuf = [];
        for (const u8 of buf) {
          this.bytesRead += u8.length;
          this.emit("data", this.__dec ? this.__dec.write(u8) : Buffer.from(u8));
        }
      });
      return this;
    };
    // 10f timers 对拍：setTimeout(ms[, cb]) 真实现——单发内部 timer 到期
    // emit('timeout')（Node 口径：不关连接、不杀 socket；cb 注册为 once 监听；
    // 0/负值 = 解除）。内部 timer 恒 unref：连接生死不归它管，socket 在场时
    // 事件循环照常泵到点（fire 不因 unrefed 豁免）。活动重置（node 收包即重置
    // idle 计时）未做——整收口径记档。
    this.setTimeout = (ms, cb) => {
      const delay = Number(ms) || 0;
      if (this.__wjs_stimer) { clearTimeout(this.__wjs_stimer); this.__wjs_stimer = null; }
      if (delay > 0) {
        const t = setTimeout(() => { this.__wjs_stimer = null; this.emit("timeout"); }, delay);
        t.unref();
        this.__wjs_stimer = t;
      }
      if (typeof cb === "function") this.once("timeout", cb);
      return this;
    };
    this.read = () => null;
    this._readableState = { endEmitted: false, length: 0 };
  }
  connect(...args) {
    if (args.length === 0 || (args.length === 1 && typeof args[0] === "object" && args[0] !== null && args[0].port === undefined && args[0].path === undefined)) {
      // node ERR_MISSING_ARGS（connect-no-arg 套件逐字）
      const e = new TypeError('The "options" or "port" or "path" argument must be specified');
      e.code = "ERR_MISSING_ARGS"; throw e;
    }
    // node 口径（Socket.connect destroyed 分支 + initSocketHandle._undestroy）：
    // destroyed 后 connect 即整流复位（destroyed/ending/errored 全清）——
    // boundsocket reconnect-after-destroy 块：close → connect → end 必须可用。
    if (this.destroyed) {
      this.destroyed = false;
      this.readable = true; this.writable = true;
      this.__connected = false;
      this.__ended = false; this.__finSent = false; this.__endAfterFlush = false;
      this.__hadError = false; this.__handleClosed = false; this.__peerFin = false;
      this._handle = null;
      this.__pendW = []; this.__pendBytes = 0;
      this.__id = 0;
    }
    let port, host, cb, __noDelay, signal, sockPath = null, __blockList = null, __lookup = null, __halfOpen, __famOpt = 0;
    if (typeof args[0] === "object" && args[0] !== null) {
      if (args[0].fd !== undefined) {
        // node 口径：listen({fd}) 非法 fd 即异步 EINVAL（error 事件；真机实证）。
        const cbFd = typeof args[1] === "function" ? args[1] : null;
        if (cbFd) this.once("listening", cbFd);
        queueMicrotask(() => {
          const e = new Error(`listen EINVAL: invalid argument`);
          e.code = "EINVAL"; e.syscall = "listen"; e.errno = -4071;
          this.emit("error", e);
        });
        return this;
      }
      if (args[0].path !== undefined) {
        // node 口径：{path} 非串 → ERR_INVALID_ARG_TYPE（逐字形）；{path} 形走 unix socket。
        if (typeof args[0].path !== "string") {
          const __got = args[0].path === null ? "null" : (Array.isArray(args[0].path) ? "an instance of Array" : (typeof args[0].path === "object" ? `an instance of ${args[0].path.constructor?.name ?? "Object"}` : `type ${typeof args[0].path} (${String(args[0].path)})`));
          const e = new TypeError(`The "options.path" property must be of type string. Received ${__got}`);
          e.code = "ERR_INVALID_ARG_TYPE"; throw e;
        }
        sockPath = String(args[0].path); ({ noDelay: __noDelay, signal } = args[0]);
        cb = typeof args[1] === "function" ? args[1] : undefined;
      } else {
        ({ port, host = "127.0.0.1", family: __famOpt, noDelay: __noDelay, signal, blockList: __blockList, lookup: __lookup, allowHalfOpen: __halfOpen } = args[0]);
        // autoSelectFamily 校验（HE 校验族套件真机口径）：非 boolean → ARG_TYPE；
        // attemptTimeout 仅在生效 autoSelectFamily 下验 int [1,60000] → OUT_OF_RANGE。
        if (args[0].autoSelectFamily !== undefined && typeof args[0].autoSelectFamily !== "boolean") {
          const e = new TypeError(`The "options.autoSelectFamily" property must be of type boolean. Received type ${typeof args[0].autoSelectFamily} (${String(args[0].autoSelectFamily)})`);
          e.code = "ERR_INVALID_ARG_TYPE"; throw e;
        }
        if ((args[0].autoSelectFamily ?? __autoSelectFamily) && args[0].autoSelectFamilyAttemptTimeout !== undefined) {
          const __att = args[0].autoSelectFamilyAttemptTimeout;
          if (typeof __att !== "number" || !Number.isInteger(__att) || __att < 1 || __att > 60000) {
            const e = new RangeError(`The value of "options.autoSelectFamilyAttemptTimeout" is out of range. It must be an integer >= 1 && <= 60000. Received ${String(__att)}`);
            e.code = "ERR_OUT_OF_RANGE"; throw e;
          }
        }
        // node 口径：connect(server.address()) 形——address 对象（{address/family/port}）
        // 直作 options，host 缺省时取 address 键（ready-without-cb 套件点名）。
        if ((args[0].host === undefined || args[0].host === null) && typeof args[0].address === "string") host = args[0].address;
        if (__halfOpen !== undefined) this.allowHalfOpen = !!__halfOpen;
        // host 校验（真机逐字）：非串→ARG_TYPE（Array 显实例形）；含 \0→ARG_VALUE。
        if (host !== undefined && typeof host !== "string") {
          const __got = Array.isArray(host) ? "an instance of Array" : (host !== null && typeof host === "object" ? `an instance of ${host.constructor?.name ?? "Object"}` : `type ${typeof host} (${String(host)})`);
          const e = new TypeError(`The "options.host" property must be of type string. Received ${__got}`);
          e.code = "ERR_INVALID_ARG_TYPE"; throw e;
        }
        if (typeof host === "string" && host.includes("\0")) {
          const e = new TypeError(`The property 'options.host' must be a string without null bytes. Received '${host.replaceAll("\0", "\\x00")}'`);
          e.code = "ERR_INVALID_ARG_VALUE"; throw e;
        }
        // 不支持键（真机逐字；lib/net.js 黑名单）。
        for (const __k of ["objectMode", "readableObjectMode", "writableObjectMode"]) {
          if (args[0][__k] !== undefined) {
            const e = new TypeError(`The property 'options.${__k}' is not supported. Received ${String(args[0][__k])}`);
            e.code = "ERR_INVALID_ARG_VALUE"; throw e;
          }
        }
        cb = typeof args[1] === "function" ? args[1] : undefined;
      }
    } else if (typeof args[0] === "string" && (typeof args[1] !== "string" || args[1] === "")) {
      // connect(path[, cb])：首参串 + 次参非 host 串即 path 形。
      sockPath = args[0];
      cb = typeof args[1] === "function" ? args[1] : (typeof args[2] === "function" ? args[2] : undefined);
    } else {
      port = args[0];
      if (typeof args[1] === "string") { host = args[1]; cb = typeof args[2] === "function" ? args[2] : undefined; }
      else { host = "127.0.0.1"; cb = typeof args[1] === "function" ? args[1] : undefined; }
    }
    // adopt-UDS + connect({path}) 恒走 UDS（真机口径：path 在即 pipe，不看 adopt）。
    if (sockPath === null) {
      // node lookupAndConnect 校验序（localerror/boundsocket 套件真机逐字）：
      // adopt 门 → localAddress(isIP) → localPort(number) → port(type/range)。
      if (this.__adoptPort !== undefined &&
          (args[0].localAddress !== undefined || args[0].localPort !== undefined)) {
        const e = new TypeError(`The argument 'options' is invalid. localAddress and localPort cannot be used with an adopted bound socket. Received ${__netInspect(args[0])}`);
        e.code = "ERR_INVALID_ARG_VALUE"; throw e;
      }
      const __la = args[0].localAddress, __lp = args[0].localPort;
      if (__la && !isIP(__la)) {
        const e = new TypeError(`Invalid IP address: ${__la}`);
        e.code = "ERR_INVALID_IP_ADDRESS"; throw e;
      }
      if (__lp && typeof __lp !== "number") {
        const e = new TypeError(`The "options.localPort" property must be of type number. Received ${__netGot(__lp)}`);
        e.code = "ERR_INVALID_ARG_TYPE"; throw e;
      }
      if (port !== undefined) { __vPortType(port); __vPort(port); }
      // node：host 非 IP 才走 lookup 系校验（IP 捷径跳过 dns 全链）——
      // lookup 函数型（options-lookup 套件）+ hints 掩码（connect-options-port）。
      // node 缺省 host = options.host || 'localhost'（connect({port}) 无 host 也走
      // 校验）；本仓底层连接面维持 127.0.0.1 缺省（remote* 表面记档），仅校验门
      // 按 node 有效 host 判定。掩码 1024|2048|256 与 dns 模块同值。
      const __effHost = (args[0].host === undefined || args[0].host === null || args[0].host === "")
        ? "localhost" : host;
      if (!isIP(__effHost)) {
        if (__lookup !== null && __lookup !== undefined && typeof __lookup !== "function") {
          const e = new TypeError(`The "options.lookup" property must be of type function. Received ${__netGot(__lookup)}`);
          e.code = "ERR_INVALID_ARG_TYPE"; throw e;
        }
        const __hv = args[0].hints || 0;
        if ((__hv & ~(1024 | 2048 | 256)) !== 0) {
          const e = new TypeError(`The argument 'hints' is invalid. Received ${Number(__hv) || 0}`);
          e.code = "ERR_INVALID_ARG_VALUE"; throw e;
        }
      }
    }
    if (cb) this.once("connect", cb);
    if (signal) {
      if (typeof signal.addEventListener !== "function") {
        const e = new TypeError("The 'signal' option must be an AbortSignal-like object");
        e.code = "ERR_INVALID_ARG_TYPE"; throw e;
      }
      if (signal.aborted) {
        const e = new Error("The operation was aborted"); e.name = "AbortError"; e.code = "ABORT_ERR";
        queueMicrotask(() => this.destroy(e));
        return this;
      }
      {
        // 直调 addEventListener 须入侧表（abort-controller 套件 listenerCount 口径；
        // 原生忽略 once，handler 自摘）。
        const __connAbort = () => {
          __etRemove(signal, "abort", __connAbort);
          const e = new Error("The operation was aborted"); e.name = "AbortError"; e.code = "ABORT_ERR";
          // postAbort 形：套件在 abort 之后才挂 once('close')——destroy 的
          // error/close 必须推 microtask（node destroy 发射为 nextTick）。
          queueMicrotask(() => this.destroy(e));
        };
        signal.addEventListener("abort", __connAbort, { once: true });
        __etAdd(signal, "abort", __connAbort);
      }
    }
    // node 口径：blockList 命中即 ERR_IP_BLOCKED（connect 前，不建连接）；
    // lookup 形：自定义解析（(host, opts, cb)；cb(null, addr[, family] | [{address, family}])）。
    const __doConnect = (finalHost) => {
      if (__blockList && typeof __blockList.check === "function" && finalHost !== null && __blockList.check(finalHost)) {
        const e = new Error(`IP(${finalHost}) is blocked by net.BlockList`);
        e.code = "ERR_IP_BLOCKED"; e.syscall = "connect";
        // HE 链中：blockList 命中即该地址尝试失败（不走 task，无 close 事件），
        // 直接推进下一地址；末位命中由 __heAdvance 收口 error+close。
        if (this.__heOnErr) { this.__heLast = e; this.__heAdvance(); return this; }
        queueMicrotask(() => this.destroy(e));
        return this;
      }
      this.__realConnect(finalHost, port, cb, __noDelay, signal, sockPath);
      return this;
    };
    if (sockPath !== null) return __doConnect(null);
    if (typeof __lookup === "function") {
      let called = false;
      // autoSelectFamily 生效时以 all:true 拉全地址（autoselectfamily-default
      // 套件的 mocked lookup 只在 all:true 下给数组）→ 多地址走 __heTry 串行回落。
      const __heAll = (args[0].autoSelectFamily ?? __autoSelectFamily) === true;
      try {
        __lookup(String(host), { family: __famOpt || 0, hints: 0, all: __heAll }, (err, addr, family) => {
          if (called) return; called = true;
          if (err) { queueMicrotask(() => this.destroy(err)); return; }
          // node onlookup：family ∉ {4,6} → ERR_INVALID_ADDRESS_FAMILY（异步 error 事件，
          // 错误带 host/port 属性；options-lookup 套件 message 逐字）。
          const fam = Array.isArray(addr) ? addr[0].family : family;
          if (fam !== 4 && fam !== 6) {
            const e = new RangeError(`Invalid address family: ${fam} ${host}:${port}`);
            e.code = "ERR_INVALID_ADDRESS_FAMILY"; e.host = host; e.port = port;
            queueMicrotask(() => this.destroy(e)); return;
          }
          const first = Array.isArray(addr) ? addr[0].address : addr;
          if (__heAll && Array.isArray(addr) && addr.length > 1) {
            this.__heStart(addr, __doConnect, cb);
            return;
          }
          __doConnect(String(first));
        });
      } catch (e) { queueMicrotask(() => this.destroy(e)); return this; }
      return this;
    }
    return __doConnect(String(host));
  }
  // Happy Eyeballs 串行回落（autoSelectFamily-default 套件）：按 lookup 数组序
  // 逐地址尝试，中间失败（error/close）被 __heOnErr 钩吞掉，close 后复位重试
  // 下一地址；connect 成功即拆钩。记档：attemptTimeout 竞速未实现（回环
  // ECONNREFUSED 即时失败，套件不经超时路径）。
  __heStart(addrs, doConnect, cb) {
    this.__heSeq = { addrs, doConnect, cb, i: 0 };
    this.__heOnErr = () => {};
    this.__heTry();
  }
  __heTry() {
    const seq = this.__heSeq;
    // 统一走 __doConnect 闭包：blockList 校验每地址都生效（blocklist 套件
    // 多 IP 全屏蔽形——直接 __realConnect 会绕过拦截并停摆回落链）。
    seq.doConnect(seq.addrs[seq.i].address);
  }
  __heReset() {
    // 与 connect() 的 destroyed 复位分支同款（boundsocket reconnect-after-destroy 口径），
    // 但保留 __pendW——HE 失败尝试期间的用户写要带到最终连接（default 套件
    // write('request') 先于 connect 的缓冲形）。
    this.destroyed = false;
    this.readable = true; this.writable = true;
    this.__connected = false;
    this.__ended = false; this.__finSent = false; this.__endAfterFlush = false;
    this.__hadError = false; this.__handleClosed = false; this.__peerFin = false;
    this._handle = null;
    this.__id = 0;
  }
  __heAdvance() {
    const seq = this.__heSeq;
    seq.i++;
    if (seq.i < seq.addrs.length) {
      this.__heReset();
      this.__heOnErr = () => {};
      this.__heTry();
    } else {
      const last = this.__heLast;
      this.__heSeq = null; this.__heOnErr = null;
      this.destroyed = true; this._handle = null;
      queueMicrotask(() => { this.emit("error", last); this.emit("close", true); });
    }
  }
  // 真连接段（blockList/lookup 前置之后；adopt 预置 local 面）。
  __realConnect(finalHost, port, cb, __noDelay, signal, sockPath) {
    // UDS 面：remoteAddress/localAddress 恒 undefined（真机实证），address() 回 {}。
    // adopt 面：localAddress/localPort 预置 bound 值（connect 前后一致，真机实证）。
    // remote* 不在此发布（连接完成前恒 undefined，见构造注）；目标另存供报错整形。
    this.__targetHost = sockPath !== null ? null : String(finalHost);
    this.__targetPort = sockPath !== null ? null : Number(port);
    this.remoteAddress = undefined; this.remotePort = undefined; this.remoteFamily = undefined;
    if (this.__adoptPort !== undefined && sockPath === null) {
      this.localAddress = this.__adoptUds || this.__adoptHost;
      this.localPort = this.__adoptPort;
    }
    // node 口径：connect 即读写可达（write 缓冲至连接完成）
    this.readable = true; this.writable = true;
    // noDelay 经 native 直达 setsockopt（http agent 默认 true；Node net 默认 false）。
    // adopt-UDS：本端源 path 透 native 预 bind（localAddress 预置源 path）。
    if (sockPath !== null) this.__udsTarget = sockPath;
    // 建柄（_handle 存活期起点；连接前缓存的 keepAlive 随建即直通新柄）。
    this.__handleClosed = false;
    this._handle = this.__makeHandle();
    if (this.__kaState) { const [ke, kd, ki, kc] = this.__kaState; try { this._handle.setKeepAlive(ke, kd, ki, kc); } catch {} }
    if (sockPath !== null && this.__adoptUds) {
      this.localAddress = this.__adoptUds;
      this.__id = Number(__wjs_net_connect(sockPath, "", this, this.__adoptUds));
    } else this.__id = sockPath !== null
      ? Number(__wjs_net_connect(sockPath, "", this, false))
      : Number(__wjs_net_connect(this.__targetHost, this.__targetPort, this, __noDelay === true));
  }
  // 事件循环派发钩子（Rust dispatch 调用；kind/data 均为字符串）
  __ev(kind, payload) {
    switch (kind) {
      case "connect": {
        try {
          const o = JSON.parse(payload || "{}");
          // serde SocketAddr → "ip:port"（IPv6 为 "[ip]:port"）
          // adopt-TCP 不回填（真机 fd 复用：local 恒为 bound 值；OS 重分漂移时以预置为准）。
          if (typeof o.local === "string" && this.__adoptPort === undefined) {
            const m = o.local.match(/^\[?([^\]]+?)\]?:(\d+)$/);
            if (m) { this.localAddress = m[1]; this.localPort = Number(m[2]); }
          }
          if (this.localAddress !== undefined && this.localAddress !== null)
            this.localFamily = String(this.localAddress).includes(":") ? "IPv6" : "IPv4";
        } catch {}
        // 远端面在此发布（UDS 恒 undefined；TCP 取 __realConnect 存的目标）。
        this.remoteAddress = this.__targetHost ?? undefined;
        this.remotePort = this.__targetPort ?? undefined;
        this.remoteFamily = this.remoteAddress === undefined ? undefined
          : (String(this.remoteAddress).includes(":") ? "IPv6" : "IPv4");
        this.__connected = true;
        this.readable = true; this.writable = true;
        const pend = this.__pendW; this.__pendW = [];
        this.__pendBytes = 0;
        for (const [u8, cb2] of pend) {
          __wjs_net_write(this.__id, u8);
          if (cb2) queueMicrotask(cb2);
        }
        if (this.__endAfterFlush) {
          this.__endAfterFlush = false;
          if (this.__id) __wjs_net_end(this.__id);
        }
        // HE 成功：拆回落钩（此后 close 走正常路径）。
        this.__heOnErr = null; this.__heSeq = null;
        this.emit("connect");
        // 注：真机另序发 'ready'（connect → ready，已接受端不发），但本仓暂不发射——
        // 同步/microtask 发射在并行负载下与静默进程死亡（exit -10，无崩溃报告）强相关，
        // 根因未定（疑 dispatch 侧存活期/GC 时序，见 AGENTS §4.126）；且本仓不执行
        // common mustCall 的 exit 钩，ready-without-cb 套件靠退出码无法证伪，
        // 发射与否不影响对拍计数。待引擎侧根因闭环后再补。
        break;
      }
      case "data": {
        const u8 = __b64dec(payload);
        if (this.__paused) { this.__pauseBuf.push(u8); break; }
        this.bytesRead += u8.length;
        this.emit("data", this.__dec ? this.__dec.write(u8) : Buffer.from(u8));
        break;
      }
      case "end": {
        this.readable = false;
        this.__peerFin = true;
        // 池化空闲 socket 见 FIN 即销毁（半关不可复用；否则写端永活、条目永泄，
        // 10b https 保活案；Node 同样把 end 掉的 socket 踢出池）。
        if (this.__inPool) {
          this.destroy();
          break;
        }
        // setEncoding 残余字节 flush（分包切断的多字节尾在 end 前补齐）
        if (this.__dec) {
          const rest = this.__dec.end();
          if (rest) this.emit("data", rest);
        }
        this.emit("end");
        // Node 口径：非 allowHalfOpen 时收 FIN 即自动回 FIN（'close' 随后）
        if (!this.allowHalfOpen && this.__id) __wjs_net_end(this.__id);
        break;
      }
      case "error": {
        const o = JSON.parse(payload);
        const se = __netErr(o.code, o.msg);
        this.__hadError = true;
        // 已销毁 socket 的迟到 teardown 噪声不 chạm 用户监听（真机口径：destroy 后
        // 底层 RST/EOF 竞速错不再派发；write-after-close 套件双块并发下必现 flaky）。
        if (this.destroyed) break;
        // node connect 系标配：syscall + errno（uv 负值；ENOENT=-2/EACCES=-13/ECONNREFUSED=-61
        // /ENOTSOCK=-38/EADDRNOTAVAIL=-49；未知 -4094）。
        se.syscall = "connect";
        se.errno = { ENOENT: -2, EACCES: -13, ECONNREFUSED: -61, ENOTSOCK: -38, EADDRNOTAVAIL: -49, EINVAL: -22, EADDRINUSE: -48 }[o.code] ?? -4094;
        // node connect 错误消息形："connect CODE <target>"（target=host:port 或 path）。
        // native msg 已是 "CODE: <os>"，此处按目标重塑（expectsError 逐字断言面）。
        if (typeof o.msg === "string" && !o.msg.startsWith("connect ") && !o.msg.startsWith("IP(")) {
          const rh = this.__targetHost ?? this.remoteAddress;
          const rp = this.__targetPort ?? this.remotePort;
          const tgt = this.__udsTarget ?? ((rh !== undefined && rh !== null && rp !== undefined && rp !== null) ? `${rh}:${rp}` : null);
          if (tgt) se.message = `connect ${o.code} ${tgt}`;
        }
        // HE 串行回落：中间地址的连接失败被钩吞（不落用户监听），close 后重试。
        if (this.__heOnErr) { this.__heLast = se; break; }
        this.emit("error", se);
        break;
      }
      case "close":
        // HE：失败尝试的 close → 推进下一地址（或末位失败收口）。
        if (this.__heOnErr) { this.__heAdvance(); break; }
        this.destroyed = true; this._handle = null; this.emit("close", this.__hadError === true); break;
    }
  }
  // node 口径：pending = 尚无可用句柄——连接中 true、连接完成 false、
  // close 后**仍为 true**（test-net-connect-buffer 'close' 处理器点名）。
  get pending() { return !this.__connected || this.destroyed; }
  get connecting() { return !this.__connected && !this.destroyed && this.__id > 0; }
  get readyState() {
    if (this.destroyed) return "closed";
    // 真机：new Socket() 未连接即 "open"（构造 readable/writable 初始真；connect 前即 open）。
    // 连接中（__id 已发）才 "opening"。
    if (!this.__connected && this.__id) return "opening";
    if (this.readable && this.writable) return "open";
    return this.readable ? "readOnly" : "writeOnly";
  }
  // node Socket async iterable（for await over data；end/close 终结、error 拒绝）
  [Symbol.asyncIterator]() {
    const st = { q: [], wake: null, done: false, err: null };
    const push = (fn, v) => { st[fn] && 0; };
    const onData = (c) => { st.q.push({ v: c }); if (st.wake) { st.wake(); st.wake = null; } };
    const onDone = () => { st.done = true; if (st.wake) { st.wake(); st.wake = null; } };
    const onErr = (e) => { st.err = e; st.done = true; if (st.wake) { st.wake(); st.wake = null; } };
    this.on("data", onData);
    this.on("end", onDone);
    this.on("close", onDone);
    this.once("error", onErr);
    const cleanup = () => { this.off("data", onData); this.off("end", onDone); this.off("close", onDone); this.off("error", onErr); };
    return {
      next: () => new Promise((resolve, reject) => {
        const step = () => {
          if (st.q.length) resolve({ value: st.q.shift().v, done: false });
          else if (st.err) { const e = st.err; st.err = null; cleanup(); reject(e); }
          else if (st.done) { cleanup(); resolve({ done: true }); }
          else st.wake = step;
        };
        step();
      }),
      return: () => { cleanup(); this.destroy(); return Promise.resolve({ done: true }); },
      throw: (e) => { cleanup(); this.destroy(); return Promise.reject(e); },
    };
  }
  // node 口径（lib/net.js writeGeneric + stream Writable.write）：
  // destroyed/!writable → 有 cb 走 cb(err)+error 事件（返回 false），无 cb 才同步抛；
  // err 形状：write after end / connect 未完成 → ERR_STREAM_WRITE_AFTER_END（writableLength 0），
  // 其余 destroyed → ERR_STREAM_DESTROYED。§4.119 同源：包装/状态检查不吞场景口径。
  __writeErr(cb2) {
    const ended = this.__ended === true || this.__finSent === true;
    const code = ended ? "ERR_STREAM_WRITE_AFTER_END" : "ERR_STREAM_DESTROYED";
    const e = __netErr(code, ended ? "write after end" : "Cannot call write after a stream was destroyed");
    if (typeof cb2 === "function") { queueMicrotask(() => { try { cb2.call(this, e); } catch {} }); return false; }
    throw e;
  }
  write(data, enc, cb) {
    const cb2 = typeof enc === "function" ? enc : cb;
    // net 口径（write-after-end-nt 套件真机形）：本地已 end 且对端已 FIN 后再写
    // → Error EPIPE 'This socket has been ended by the other party'——cb 与 error
    // 事件都下一 tick。两条件缺一不可：仅对端 FIN（writable 套件 'end' 后写）
    // 与仅本地 end（G7 write-after-end 形 STREAM_WRITE_AFTER_END）都不走此路。
    if (this.__peerFin === true && this.__ended === true && this.allowHalfOpen !== true) {
      const e = new Error("This socket has been ended by the other party");
      e.code = "EPIPE"; e.errno = 32; e.syscall = "write";
      if (typeof cb2 === "function") queueMicrotask(() => { try { cb2.call(this, e); } catch {} });
      queueMicrotask(() => this.emit("error", e));
      return false;
    }
    // 真机逐字（writable.js _write）：仅 null → ERR_STREAM_NULL_VALUES（undefined 落
    // ARG_TYPE 'Received undefined'）；chunk 类型校验先于 after-end/destroyed 状态检查。
    if (data === null) {
      const e = new TypeError("May not write null values to stream");
      e.code = "ERR_STREAM_NULL_VALUES";
      if (typeof cb2 === "function") { queueMicrotask(() => { try { cb2.call(this, e); } catch {} }); return false; }
      throw e;
    }
    const u8 = __chunkU8(data);
    if (this.destroyed || !this.writable) return this.__writeErr(cb2);
    // node 口径（write-after-close 套件双形，真机 26 实测均为异步 error 事件非同步抛）：
    // 已连接但 _handle 被置空后写 → ERR_SOCKET_CLOSED('Socket is closed')；
    // _handle.close() 后（柄关而对象在）写 → Error('write EBADF'，win 系 EPIPE)。
    if (this.__connected && this._handle === null) {
      const e2 = new Error("Socket is closed"); e2.code = "ERR_SOCKET_CLOSED";
      if (typeof cb2 === "function") queueMicrotask(() => { try { cb2.call(this, e2); } catch {} });
      queueMicrotask(() => this.emit("error", e2));
      return false;
    }
    if (this.__handleClosed === true) {
      const e = new Error(`write ${typeof process !== "undefined" && process.platform === "win32" ? "EPIPE" : "EBADF"}`);
      if (typeof cb2 === "function") queueMicrotask(() => { try { cb2.call(this, e); } catch {} });
      queueMicrotask(() => this.emit("error", e));
      return false;
    }
    this.bytesWritten += u8.length;
    if (!this.__connected) {
      // node 口径：连接完成前 write 缓冲（connect 完成时按序冲刷）
      this.__pendW.push([u8, cb2]);
      this.__pendBytes += u8.length;
      return u8.length + this.__pendBytes - u8.length <= this.__hwm;
    }
    __wjs_net_write(this.__id, u8);
    // 记档：底层同步写队列，无 flush 语义，回调即刻
    if (cb2) queueMicrotask(cb2);
    return u8.length <= this.__hwm;
  }
  end(data, enc, cb) {
    // node 语义：end([chunk][, enc][, cb])——首参函数即 cb（async-iter 套件
    // `end(resolve)` 形；不识别则回调被当 chunk 落校验 TypeError）。
    if (typeof data === "function") { cb = data; data = undefined; enc = undefined; }
    else if (typeof enc === "function") { cb = enc; enc = undefined; }
    if (data !== undefined && data !== null) this.write(data, typeof enc === "string" ? enc : undefined);
    const cb2 = cb;
    this.writable = false; this.__ended = true;
    if (this.__id && this.__connected) __wjs_net_end(this.__id);
    else this.__endAfterFlush = true; // node 口径：FIN 排队到连接完成+缓冲写冲刷之后
    // node 口径：写侧刷完即 'finish'（早于 close；bytes-stats/bytes-read 套件点名）。
    // 本仓同步写队列：FIN 已发即 microtask 派发 finish。
    queueMicrotask(() => this.emit("finish"));
    // node 流语义：end 的回调挂 'finish'（非 close——半开对端不回 FIN 时
    // close 永不来，async-iter 套件 `end(resolve)` 卡死）。
    if (cb2) this.once("finish", cb2);
    return this;
  }
  // node 口径：resetAndDestroy() = RST 硬关（本端无 error 即 close；
  // 对端读侧 ECONNRESET）。本仓 TCP 无 RST 面：本端走 destroy 无 error，
  // 对端侧由传输 FIN 收尾（ECONNRESET 偏离，见 bun-parity net 节）。
  resetAndDestroy() { return this.destroy(); }
  // node 口径：error 事件只在 destroy(err) 带参时发（显式 destroy() 无参不发）。
  // 校验/状态 write 失败走 cb（__writeErr），不进 error 事件——lib/net.js 原文口径。
  destroy(err) {
    if (!this.destroyed) {
      this.destroyed = true;
      this.writable = false; this.readable = false;
      this._handle = null;
      if (this.__id) __wjs_net_destroy(this.__id);
      if (err !== undefined && err !== null) { this.__hadError = true; this.emit("error", err); }
    }
    return this;
  }
  address() {
    // UDS：真机 address() 回 {}（local/remote 全 undefined）。
    if (this.localAddress === null || this.localAddress === undefined) {
      return this.remoteAddress === undefined && this.__connected ? {} : null;
    }
    return { address: this.localAddress, port: this.localPort, family: String(this.localAddress).includes(":") ? "IPv6" : "IPv4" };
  }
  setEncoding(enc) {
    this.__enc = enc === null || enc === undefined ? null : String(enc);
    // 持久解码器（large-string 套件：分包多字节必须跨 chunk 保态——
    // 每 chunk 新建 TextDecoder 会把切断的序列各吐一个 U+FFFD）。
    this.__dec = this.__enc ? new StringDecoder(this.__enc) : null;
    return this;
  }
  // 10a：ref 真计数（net/dgram 共用 natives；__id 为 0 时静默 no-op）。
  ref() { if (this.__id) __wjs_net_ref(this.__id); return this; }
  unref() { if (this.__id) __wjs_net_unref(this.__id); return this; }
}

class __ServerClass extends EventEmitter {
  constructor(options, cb) {
    super();
    // node Server（server-options 套件逐字）：function 即 connectionListener；
    // null/undefined 走 {}；其余非对象（0/'path'/true）→ ARG_TYPE validateObject 形。
    if (options !== undefined && options !== null && typeof options !== "object" && typeof options !== "function") {
      const e = new TypeError(`The "options" argument must be of type object. Received ${__netGot(options)}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    this.__id = 0;
    this.__listening = null;
    this.allowHalfOpen = !!(options && typeof options === "object" && options.allowHalfOpen);
    // node 口径（server-keepalive 套件点名）：keepAlive/keepAliveInitialDelay 存自身
    // （默认 false/0，真机 26 实测）；_handle 在 listen 成功路径建桩（含 onconnection）。
    this.keepAlive = !!(options && typeof options === "object" && options.keepAlive);
    this.keepAliveInitialDelay = (options && typeof options === "object" && options.keepAliveInitialDelay !== undefined) ? options.keepAliveInitialDelay : 0;
    // server 选项面（blocklist/drop-connections/pause-on-connect 套件）
    this.pauseOnConnect = !!(options && typeof options === "object" && options.pauseOnConnect);
    this.__blockList = (options && typeof options === "object" && options.blockList) || null;
    this.__connsSet = new Set();
    this._handle = null;
    this.__pendingConnPayload = null;
    // transfer-guards：Server 同 Socket 不可 transfer（成功转移面另案，见 Socket 注）。
    try { (globalThis.__wjs_netXfer ??= new Map()).set(this, "net.Server"); } catch {}
    if (typeof options === "function") { cb = options; options = undefined; }
    if (typeof cb === "function") this.on("connection", cb);
    // 派发钩子预绑定（同 Socket 注）
    this.__ev = this.__ev.bind(this);
  }
  get listening() { return this.__listening !== null; }
  // _handle 建桩：onconnection 为箭头函数（构造/方法 this 捕获 server 本体，
  // 用户 `.call(handle, …)` 改绑不影响）；连接创建逻辑收敛 __acceptConn，
  // __ev connection 经此进入（套件包装 onconnection 即被调用）。
  __setupHandle() {
    this._handle = { onconnection: (err, clientHandle) => {
      const payload = this.__pendingConnPayload; this.__pendingConnPayload = null;
      if (err) { this.emit("error", err); return; }
      if (this.keepAlive && clientHandle && typeof clientHandle.setKeepAlive === "function") {
        try { clientHandle.setKeepAlive(true, this.keepAliveInitialDelay); } catch {}
      }
      this.__acceptConn(payload);
    } };
  }
  __acceptConn(payload) {
    const o = JSON.parse(payload);
    const s = new Socket();
    if (o.uds) s.__attachUds(o);
    else s.__attachConn(o);
    // 真机：server 侧 socket.server 全等 server 本体；本端地址族取监听地址。
    s.server = this;
    s.allowHalfOpen = !!this.allowHalfOpen;
    // server keepAlive 落已接受 socket（去重缓存预置，首个同值显式调用被吞，
    // server-keepalive 套件三调只进二）。
    if (this.keepAlive) { try { s.setKeepAlive(true, this.keepAliveInitialDelay); } catch {} }
    if (this.__listening && typeof this.__listening === "object") {
      s.localAddress = this.__listening.address;
      s.localPort = this.__listening.port;
      s.localFamily = this.__listening.family;
    }
    // blockList 拒绝：静默销毁，不进 connection（node 口径，server-blocklist 套件）。
    if (this.__blockList && s.remoteAddress && typeof this.__blockList.check === "function" && this.__blockList.check(s.remoteAddress)) {
      s.destroy();
      return;
    }
    // 超限：'drop' 事件（五元组）+ 销毁，不进 connection。node 语义：
    // maxConnections=0 即全拒（dormantServer 用例）；undefined = 不限。
    if (this.maxConnections !== undefined && this.maxConnections !== null &&
        (this.__conns ?? 0) >= this.maxConnections) {
      this.emit("drop", {
        localAddress: s.localAddress, localPort: s.localPort,
        remoteAddress: s.remoteAddress, remotePort: s.remotePort, remoteFamily: s.remoteFamily,
      });
      s.destroy();
      return;
    }
    if (this.pauseOnConnect) s.__paused = true;
    this.__conns = (this.__conns ?? 0) + 1;
    this.__connsSet.add(s);
    s.once("close", () => { this.__conns = Math.max(0, (this.__conns ?? 1) - 1); this.__connsSet.delete(s); });
    this.emit("connection", s);
  }
  // node：销毁全部已接受连接（server-drop-connections 套件）。
  dropConnections() {
    for (const s of [...this.__connsSet]) {
      try { s.destroy(); } catch {}
    }
  }
  __doListen(port, host, cb, reusePort) {
    // relisten 必须清 close-during-listen 窗口旗（close() 置位后柄已清，
    // 重听的 listening 派发不再属"bind 窗口内 close"——残留 true 会吞掉
    // 新一轮 listening 派发，listening 回调永不触发）。
    this.__closing = false;
    if (port && typeof port === "object" && typeof port.address === "function" && port.__boundPort !== undefined) {
      // listen(bound)：adopt（旧柄失效；server 地址 = bound 地址）。
      if (port.__adopted) {
        const e = new Error("The bound socket has already been adopted by a server or socket");
        e.code = "ERR_SOCKET_HANDLE_ADOPTED"; throw e;
      }
      port.__adopted = true;
      if (port.__udsPath !== undefined) __boundPaths.delete(port.__udsPath);
      if (port.__holdToken) { try { __wjs_net_unhold(port.__holdToken); } catch {} }
      if (cb) this.once("listening", cb);
      this.__setupHandle();
      if (port.__isPipe) {
        this.__port = 0; this.__udsPath = port.__udsPath;
        this.__id = Number(__wjs_net_listen(0, "UDS:" + port.__udsPath, this));
      } else {
        this.__port = port.__boundPort;
        // reusePort 占位柄释放后重绑仍须带 SO_REUSEPORT（boundsocket reusePort
        // 双 listen 块；native 第 4 参 "1" 即开）。
        this.__id = Number(__wjs_net_listen(port.__boundPort, port.__boundHost, this, port.__reusePort === true ? "1" : ""));
      }
      return this;
    }
    if (typeof port === "string" || (port !== null && typeof port === "object")) {
      // node normalizeArgs：可解析为有限数的非空字符串 = TCP 端口（listen("0")
      // → 通配，真机 26 实证 address 回 port；旧实现一律当 UDS 路径，listen("0")
      // 建出名为 "0" 的套接字文件，listen-options 套件二次绑定即 EADDRINUSE）。
      // 其余字符串才是 IPC 路径（"abc" → path，真机同）。
      if (typeof port === "string" && port.trim() !== "" && Number.isFinite(Number(port))) {
        port = Number(port);
      } else {
        const p = typeof port === "string" ? port : port.path;
        let modeBits = "";
        if (port !== null && typeof port === "object") {
          if (port.readableAll) modeBits += "r";
          if (port.writableAll) modeBits += "w";
        }
        if (cb) this.once("listening", cb);
        this.__port = 0; this.__udsPath = String(p);
        this.__setupHandle();
        this.__id = Number(__wjs_net_listen(0, "UDS:" + String(p) + "\n" + modeBits, this));
        return this;
      }
    }
    if (port === undefined || port === null) port = 0;
    __vPort(port, "options.port");
    if (cb) this.once("listening", cb);
    this.__port = Number(port);
    this.__setupHandle();
    // reusePort 直通 native 第 4 参（child reuseport 套件：fork 共享端口；
    // BoundSocket-adopt 路径早有同款，此处 direct 路径补齐）。
    this.__id = Number(__wjs_net_listen(Number(port), host === null ? "0.0.0.0" : host, this, reusePort === true ? "1" : ""));
    return this;
  }
  // node 口径：listen(cb)/listen()/listen(null) 即 listen(0)；listen(port[, host][, cb])
  // 全形态（port 缺省 0；cb 可在任意位置）。
  listen(...args) {
    // node：listening 期间再 listen 即同步抛（真机 26：`Error ERR_SERVER_ALREADY_LISTEN
    // "Listen method has been called more than once without closing."`；close 同步
    // 清柄故 close 后可再听——call-listen-multiple 套件三段全覆盖）。
    if (this._handle) {
      const e = new Error("Listen method has been called more than once without closing.");
      e.code = "ERR_SERVER_ALREADY_LISTEN";
      throw e;
    }
    let port, host = null, cb = null;
    if (typeof args[0] === "function") return this.__doListen(0, null, args[0]);
    if (args[0] === undefined || args[0] === null) {
      for (let i = 1; i < args.length; i++) {
        if (typeof args[i] === "string" && host === null) host = args[i];
        else if (typeof args[i] === "function") cb = args[i];
      }
      return this.__doListen(0, host, cb);
    }
    if (args[0] && typeof args[0] === "object" && typeof args[0].address === "function" && args[0].__boundPort !== undefined)
      return this.__doListen(args[0], null, typeof args[1] === "function" ? args[1] : null);
    if (typeof args[0] === "object" && args[0] !== null) {
      const o = args[0];
      const cb0 = typeof args[1] === "function" ? args[1] : null;
      // node addServerAbortSignalOption：port/path 分派前校验（ARG_TYPE 逐字）+
      // abort 即 close（pre-aborted 走 nextTick 同位微任务）。
      if (o.signal !== undefined) {
        if (o.signal === null || typeof o.signal !== "object" || !("aborted" in o.signal)) {
          const e = new TypeError(`The "options.signal" property must be an instance of AbortSignal. Received ${__netGot(o.signal)}`);
          e.code = "ERR_INVALID_ARG_TYPE"; throw e;
        }
        if (o.signal.aborted) queueMicrotask(() => { try { this.close(); } catch {} });
        else {
          // abort-controller 套件：直调 addEventListener 的监听须入侧表
          // （events.listenerCount 对 EventTarget 读侧表；原生忽略 once，自摘）。
          const __sigHandler = () => { __etRemove(o.signal, "abort", __sigHandler); try { this.close(); } catch {} };
          o.signal.addEventListener("abort", __sigHandler, { once: true });
          __etAdd(o.signal, "abort", __sigHandler);
        }
      }
      if (typeof o.fd === "number" && o.fd >= 0) {
        // node 口径：listen({fd}) 非法 fd 即异步 EINVAL（error 事件；真机实证）；
        // 负数/非数值 fd 落 node 尾 throw（{fd:-1} → 'must have the property'）。
        if (cb0) this.once("listening", cb0);
        queueMicrotask(() => {
          const e = new Error(`listen EINVAL: invalid argument`);
          e.code = "EINVAL"; e.syscall = "listen"; e.errno = -4071;
          this.emit("error", e);
        });
        return this;
      }
      if (("port" in o) && (o.port === undefined || o.port === null)) {
        // node：port 显式 undefined/null 即 0（listen({port}) 通配）。
        return this.__doListen(0, o.host ?? null, cb0, o.reusePort);
      }
      if (typeof o.port === "number" || typeof o.port === "string") {
        // node：port 分支先于 path（{port:-1, path} 点名 BAD_PORT 先抛）。
        __vPort(o.port, "options.port");
        return this.__doListen(o.port, o.host ?? null, cb0, o.reusePort);
      }
      if (o.path && typeof o.path === "string") {
        const o0 = { path: String(o.path) };
        if (o.readableAll !== undefined) o0.readableAll = !!o.readableAll;
        if (o.writableAll !== undefined) o0.writableAll = !!o.writableAll;
        return this.__doListen(o0, null, cb0);
      }
      // node 尾两 throw：无 port/path 键（{}/fd 负数）→ 'must have the property'；
      // 有键但不合格（{port:false}/{path:-1}）→ 'is invalid'（均 ARG_VALUE inspect 形）。
      if (!("port" in o) && !("path" in o)) {
        const e = new TypeError(`The argument 'options' must have the property "port" or "path". Received ${__netInspect(o)}`);
        e.code = "ERR_INVALID_ARG_VALUE"; throw e;
      }
      const e = new TypeError(`The argument 'options' is invalid. Received ${__netInspect(o)}`);
      e.code = "ERR_INVALID_ARG_VALUE"; throw e;
    } else {
      port = args[0];
      // node normalizeArgs：非对象/非 pipe 首参（含 boolean）进 options.port，
      // listen(true/false) 落尾 throw ARG_VALUE('options')（inspect { port: true } 形）。
      if (typeof port === "boolean") {
        const e = new TypeError(`The argument 'options' is invalid. Received ${__netInspect({ port })}`);
        e.code = "ERR_INVALID_ARG_VALUE"; throw e;
      }
      for (let i = 1; i < args.length; i++) {
        // host 位：首参为数字（含数字字符串，node normalizeArgs 同判）才轮到 host。
        const portNum = typeof port === "number" || (typeof port === "string" && port.trim() !== "" && Number.isFinite(Number(port)));
        if (typeof args[i] === "string" && host === null && portNum) host = args[i];
        else if (typeof args[i] === "function") cb = args[i];
      }
    }
    return this.__doListen(port, host, cb);
  }
  __ev(kind, payload) {
    switch (kind) {
      case "listening": {
        // bind 窗口内 close()（listen-close-server 套件）：listening 派发吞掉，
        // listening 回调永不触发；'close' 由 destroy 路径照常发。
        if (this.__closing) break;
        const o = JSON.parse(payload);
        if (o.uds) {
          this.__listening = o.path;
          this._connectionKey = `unix:${o.path}`;
          this.emit("listening");
          break;
        }
        this.__listening = { address: o.addr, port: o.port, family: String(o.addr).includes(":") ? "IPv6" : "IPv4" };
        // node Server._connectionKey：'<family>:<host>:<请求端口>'（listen(0) 键含 '0'）
        this._connectionKey = `${String(o.addr).includes(":") ? 6 : 4}:${o.addr}:${this.__port}`;
        this.emit("listening");
        break;
      }
      case "connection": {
        // 经 _handle.onconnection 进入（套件可包装观测；缺桩回落直建）。
        this.__pendingConnPayload = payload;
        const h = this._handle;
        if (h && typeof h.onconnection === "function") h.onconnection(null, { setKeepAlive: (en, ms) => {} });
        else this.__acceptConn(payload);
        break;
      }
      case "error": {
        const o = JSON.parse(payload);
        const e = __netErr(o.code, o.msg);
        e.port = this.__listening ? this.__listening.port : this.__port;
        // node bind 系标配：syscall + errno（EADDRINUSE=-4091/-48；EACCES=-4092/-13）。
        e.syscall = "listen";
        e.errno = o.code === "EADDRINUSE" ? -4091 : (o.code === "EACCES" ? -4092 : -4094);
        // listen 失败柄即清：error 后可立即重听（node 口径，call-listen-multiple
        // 第一段；ALREADY_LISTEN 守卫读 `_handle`，不清即卡死重听）。
        this._handle = null; this.__id = 0;
        this.emit("error", e);
        break;
      }
      case "close": this.__listening = null; this.emit("close"); break;
    }
  }
  address() { return this.__listening; }
  // node 口径：getConnections(cb) 异步回现存连接数；无 cb 直回数（真机同）。
  // 计数位由 connection/+socket-close 维护（server 侧 socket close 即减）。
  getConnections(cb) {
    const n = this.__conns ?? 0;
    if (typeof cb === "function") { queueMicrotask(() => { try { cb(null, n); } catch {} }); return this; }
    return n;
  }
  close(cb) {
    if (typeof cb === "function") this.once("close", cb);
    if (this.__id) {
      __wjs_net_destroy(this.__id);
      // 柄同步即清（node 口径：close 后 listen 立即可用，call-listen-multiple 第三段）。
      // __closing 旗拦 bind 窗口内已就绪的 Listening 派发（listen-close-server
      // 套件：close() 后 listening 回调必须永不触发）。
      this.__closing = true;
      this.__id = 0;
      this._handle = null;
      this.__listening = undefined;
    }
    return this;
  }
  // 10a：ref 真计数（同 Socket）。
  ref() { if (this.__id) __wjs_net_ref(this.__id); return this; }
  unref() { if (this.__id) __wjs_net_unref(this.__id); return this; }
}

Socket.prototype.__attachConn = function (info) {
  this.__id = Number(info.connId);
  this.__connected = true;
  this.__handleClosed = false;
  this._handle = this.__makeHandle();
  this.remoteAddress = info.remoteAddress;
  this.remotePort = info.remotePort;
  this.remoteFamily = String(info.remoteAddress).includes(":") ? "IPv6" : "IPv4";
  if (info.serverId !== undefined) this.server = info.serverId;
  this.localAddress = info.localAddress;
  this.localPort = info.localPort;
  this.readable = true; this.writable = true;
  __wjs_net_attach(this.__id, this);
};
// Node Socket.unshift：字节塞回读流头部（ws setSocket 对升级残留用）；
// 本仓读流无 JS 侧缓冲，以 data 事件回灌近似（先于后续 pump chunk——
// microtask 时序，残余错序仅限升级瞬间的罕见重叠帧）。
Socket.prototype.__attachUds = function (info) {
  this.__id = Number(info.connId);
  this.__connected = true;
  this.__handleClosed = false;
  this._handle = this.__makeHandle();
  this.remoteAddress = undefined; this.remotePort = undefined;
  this.localAddress = undefined; this.localPort = undefined;
  this.remoteFamily = undefined;
  this.readable = true; this.writable = true;
  __wjs_net_attach(this.__id, this);
};
Socket.prototype.unshift = function (chunk) {
  if (chunk && chunk.length > 0) queueMicrotask(() => this.emit("data", chunk));
  return this;
};

export function createServer(options, cb) {
  return new Server(options, cb);
}
// Node 口径：Server/Socket 裸调用返回新实例（lib/net.js 原文
// `if (!(this instanceof Server)) return new Server(...)`）。
function Server(...args) {
  if (!(this instanceof __ServerClass)) return new __ServerClass(...args);
  return Reflect.construct(__ServerClass, args, new.target ?? __ServerClass);
}
Object.setPrototypeOf(Server, __ServerClass);
Server.prototype = __ServerClass.prototype;
export function createConnection(...args) { return new Socket().connect(...args); }
export const connect = createConnection;
// node BoundSocket（同步 bind 句柄；adopt 即迁入 server/socket，旧柄失效）。
// 底座：TCP 占位 bind（地址/冲突语义真，fd 桩 -1 记档）；UDS 真 bind（path 串）。
// 校验族对 lib/net.js：非对象 → ERR_INVALID_ARG_TYPE；host 非法串（localhost 等
// 不可 bind 名）→ ERR_INVALID_ARG_VALUE；bind 失败 → code+syscall=bind。
class BoundSocket {
  constructor(options) {
    // node 口径：无参即 {}（0.0.0.0:0 通配）；null/数组/非对象才 ARG_TYPE。
    if (options === undefined) options = {};
    if (options === null || typeof options !== "object" || Array.isArray(options)) {
      const e = new TypeError(`The "options" argument must be of type object. Received ${options === null ? "null" : Array.isArray(options) ? "an instance of Array" : typeof options}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    const { host, port, path, ipv6Only, reusePort } = options;
    // path 与 TCP 族互斥（真机 ERR_INVALID_ARG_VALUE）；path 非串 → ARG_TYPE。
    if (path !== undefined) {
      if (typeof path !== "string") {
        const e = new TypeError(`The "options.path" property must be of type string. Received type ${typeof path}`);
        e.code = "ERR_INVALID_ARG_TYPE"; throw e;
      }
      for (const k of ["host", "port", "ipv6Only", "reusePort"]) {
        if (options[k] !== undefined) {
          const e = new TypeError(`The "options.path" property cannot be used with "options.${k}"`);
          e.code = "ERR_INVALID_ARG_VALUE"; throw e;
        }
      }
    }
    if (host !== undefined && typeof host !== "string") {
      const e = new TypeError(`The "options.host" property must be of type string. Received type ${typeof host}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    if (host !== undefined && (host === "localhost" || host === "")) {
      const e = new TypeError(`The "options.host" property must be a valid IP address or hostname. Received '${host}'`);
      e.code = "ERR_INVALID_ARG_VALUE"; throw e;
    }
    this.__adopted = false;
    this.__isPipe = path !== undefined;
    if (path !== undefined) {
      // 抽象地址（首字节 \0）仅 Linux 支持；其余平台同步 ERR_INVALID_ARG_VALUE（真机口径）。
      if (String(path).charCodeAt(0) === 0) {
        let isLinux = false;
        try { isLinux = process.platform === "linux"; } catch {}
        if (!isLinux) {
          const e = new TypeError(`The "options.path" property must be a valid path. Received '${path}'`);
          e.code = "ERR_INVALID_ARG_VALUE"; throw e;
        }
      }
      // 路径形态预检（同步抛，不进 native）：超长 → EINVAL；缺父目录 → EACCES。
      // （真机：uv_pipe_bind + UV_PIPE_NO_TRUNCATE；libuv ENOENT→EACCES 跨平台对齐。）
      // 同 path 重复 bind → EADDRINUSE（真机 pipe bind 同步语义；抽象地址走内存集合）。
      if (__boundPaths.has(String(path))) {
        const e = new Error(`bind EADDRINUSE ${String(path)}`);
        e.code = "EADDRINUSE"; e.syscall = "bind"; e.errno = -4091; throw e;
      }
      if (String(path).charCodeAt(0) !== 0) {
        // 缺父目录 → 同步 EACCES（真机实证：ENOENT→EACCES 跨平台对齐；__bs_cut 误判曾疑此，
        // 实为探针路径非法——node 同路径同 EACCES，检查无辜）。
        // 相对路径先绝对化再取父（与 kernel bind 同解析；cwd 即进程 cwd）。
        if (String(path).length > 100) {
          const e = new Error(`bind EINVAL ${String(path).slice(0, 40)}...`);
          e.code = "EINVAL"; e.syscall = "bind"; e.errno = -4071; throw e;
        }
        {
          const abs = String(path).startsWith("/") ? String(path) : (process.cwd() + "/" + String(path));
          const norm = abs.split("/").filter((x) => x !== "" && x !== ".").join("/");
          const parts = norm.split("/");
          parts.pop();
          const parent = "/" + parts.join("/");
          let parentExists = false;
          try { __wjs_fs_stat(parent, true); parentExists = true; } catch { parentExists = false; }
          if (!parentExists) {
            const e = new Error(`bind EACCES ${String(path)}`);
            e.code = "EACCES"; e.syscall = "bind"; e.errno = -4092; throw e;
          }
        }
      }
      let bound;
      try { bound = __wjs_net_bind("", 0, String(path)); }
      catch (e) { throw __bindErr(String((e && e.message) || e)); }
      this.__udsPath = String(path);
      this.__boundPort = 0;
      __boundPaths.add(String(path));
    } else {
      // node：ipv6Only 即绑定 IPv6 通配 '::'（boundsocket IPv6 块：address '::'/IPv6）。
      const h = host !== undefined ? host : (ipv6Only === true ? "::" : "0.0.0.0");
      const p = port !== undefined ? Number(port) : 0;
      let bound;
      // reusePort → native 走 SO_REUSEPORT bind（真机 macOS/Linux 同支持；
      // 平台不支持由 native setsockopt 失败即 Err，套件 probe 落 first=null 跳过）。
      try { bound = __wjs_net_bind(h, p, "", reusePort === true ? "1" : ""); }
      catch (e) { throw __bindErr(String((e && e.message) || e)); }
      // native 回 "port:token"（占位保活；close/adopt 时 __wjs_net_unhold(token) 释放）。
      const parts = String(bound).split(":");
      this.__boundHost = h;
      this.__boundPort = Number(parts[0]);
      this.__holdToken = Number(parts[1] || 0);
    }
    this.__ipv6Only = !!ipv6Only; this.__reusePort = !!reusePort;
  }
  get isPipe() { return this.__isPipe; }
  address() {
    if (this.__adopted) {
      const e = new Error("The bound socket has already been adopted by a server or socket");
      e.code = "ERR_SOCKET_HANDLE_ADOPTED"; throw e;
    }
    if (this.__isPipe) return this.__udsPath;
    const h = this.__boundHost;
    return { address: h, port: this.__boundPort, family: h.includes(":") ? "IPv6" : "IPv4" };
  }
  fd() {
    if (this.__adopted) {
      const e = new Error("The bound socket has already been adopted by a server or socket");
      e.code = "ERR_SOCKET_HANDLE_ADOPTED"; throw e;
    }
    // 真 fd：占位 listener dup（unix；win 回 -1，套件 win 侧只断类型）。
    if (this.__holdToken) return Number(__wjs_net_fd(this.__holdToken));
    return -1;
  }
  close() {
    if (this.__adopted) {
      const e = new Error("The bound socket has already been adopted by a server or socket");
      e.code = "ERR_SOCKET_HANDLE_ADOPTED"; throw e;
    }
    if (this.__udsPath !== undefined) __boundPaths.delete(this.__udsPath);
    if (this.__holdToken) { try { __wjs_net_unhold(this.__holdToken); } catch {} }
    this.__adopted = true; // close 即失效（真机二次 close 同 ADOPTED 口径）
  }
}
// native BINDFAIL 文本 → code + syscall=bind 整形（node "bind CODE addr" 形）。
function __bindErr(msg) {
  const m = /^BINDFAIL (\S+): bind (\S+) (.*)$/.exec(msg);
  const code = m ? m[1] : "EADDRINUSE";
  const e = new Error(m ? `bind ${m[2]} ${m[3]}` : msg);
  e.code = code; e.syscall = "bind"; e.errno = -4078;
  return e;
}
export { Socket, Server, BoundSocket };
// node legacy：net.Stream(...) 无 new 可调（lib/net.js `Stream.Stream = Stream` 族）
export const Stream = new Proxy(Socket, {
  apply(_t, _this, args) { return new Socket(...args); },
});
// Node `net.isIP/isIPv4/isIPv6`（vite 请求路径 host 校验用；std 解析对齐语义）
// ── 10f net 对拍：校验器 + IP 解析纯 JS（std 拒前导零，node 收）─────────
// node `invalidArgTypeHelper` 口径（ARG_TYPE 助记形：`Received type string ('x')`/
// `Received null`/`Received an instance of Array`；ARG_VALUE 走 inspect 形，§4.114）。
function __netGot(v) {
  if (v === null) return "null";
  if (v === undefined) return "undefined";
  if (typeof v === "string") return `type string ('${v}')`;
  if (typeof v === "object") return `an instance of ${v.constructor?.name ?? "Object"}`;
  return `type ${typeof v} (${String(v)})`;
}
// node inspect 简形（ARG_VALUE 收尾 `Received { port: false }` 形；listen-options 套件
// 断言到正则 `Received .+`，浅对象逐键 node 形即可，深结构记档）。
function __netInspect(v) {
  if (v === null || v === undefined) return String(v);
  if (typeof v === "string") return `'${v}'`;
  if (typeof v === "number" || typeof v === "boolean" || typeof v === "bigint") return String(v);
  if (typeof v !== "object") return `[${typeof v}]`;
  if (Array.isArray(v)) return "[Array]";
  try {
    const ks = Object.keys(v);
    if (ks.length === 0) return "{}";
    return `{ ${ks.map((k) => `${k}: ${__netInspect(v[k])}`).join(", ")} }`;
  } catch { return "{}"; }
}
// node validatePort（lib/internal/validators.js 逐字语义）：number/string、串 trim 非空、
// `+p === (+p >>> 0)`、`p <= 0xFFFF`（'0x10' 数值线名同收——connect-options-port
// canConnect('0x..') 点名；123.456/-1/65536/NaN/±Infinity 拒）。
// name：connect 系缺省 'Port'；listen 系传 'options.port'（真机两口径）。
function __vPort(p, name = "Port") {
  if ((typeof p !== "number" && typeof p !== "string") ||
      (typeof p === "string" && p.trim().length === 0) ||
      +p !== (+p >>> 0) ||
      p > 0xFFFF) {
    const e = new RangeError(`${name} should be >= 0 and < 65536. Received ${__netGot(p)}.`);
    e.code = "ERR_SOCKET_BAD_PORT"; throw e;
  }
  return p | 0;
}
// connect(options.port) ARG_TYPE 门（lookupAndConnect：类型不合先于 BAD_PORT；
// 真机文案 `must be one of type number or string`，name 恒 'options.port'）。
function __vPortType(p) {
  if (typeof p !== "number" && typeof p !== "string") {
    const e = new TypeError(`The "options.port" property must be one of type number or string. Received ${__netGot(p)}`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
}
function __isIPv4(s) {
  if (typeof s !== "string") return false;
  const parts = s.split(".");
  if (parts.length !== 4) return false;
  for (const p of parts) {
    if (!/^[0-9]{1,3}$/.test(p)) return false;
    if (p.length > 1 && p.charCodeAt(0) === 48) return false;
    if (Number(p) > 255) return false;
  }
  return true;
}
function __v6Groups(head, tail) {
  const group = (g) => /^[0-9A-Fa-f]{1,4}$/.test(g);
  if (tail === null) return head.length === 8 && head.every(group);
  return head.length + tail.length <= 7 && head.every(group) && tail.every(group);
}
function __isIPv6(s) {
  if (typeof s !== "string" || s === "") return false;
  // node 口径：尾部 %zone 可选（'fe80::2008%eth0' → 6；'@' 等非法字符拒）
  const zi = s.indexOf("%");
  if (zi !== -1) {
    const zone = s.slice(zi + 1);
    if (zone === "" || !/^[A-Za-z0-9._~-]+$/.test(zone)) return false;
    s = s.slice(0, zi);
  }
  let head = s, tail = null;
  const i = s.indexOf("::");
  if (i !== -1) {
    if (s.indexOf("::", i + 1) !== -1) return false;
    head = s.slice(0, i);
    tail = s.slice(i + 2);
  }
  let hp = head === "" ? [] : head.split(":");
  let tp = tail === null ? null : (tail === "" ? [] : tail.split(":"));
  if (tp !== null && tp.length > 0 && tp[tp.length - 1].includes(".")) {
    const v4 = tp.pop();
    if (!__isIPv4(v4)) return false;
    tp.push("0", "0");
  } else if (tp === null && hp.length > 0 && hp[hp.length - 1].includes(".")) {
    const v4 = hp.pop();
    if (!__isIPv4(v4)) return false;
    hp.push("0", "0");
  }
  return __v6Groups(hp, tp);
}
export function isIP(input) {
  // node 口径：对象经 String() 转换（{ toString: () => '127.0.0.1' } → 4）
  const s = (input && typeof input === "object") ? String(input) : input;
  if (typeof s !== "string") return 0;
  if (__isIPv4(s)) return 4;
  if (__isIPv6(s)) return 6;
  return 0;
}
// 已占 pipe 路径集合（BoundSocket 同步 EADDRINUSE 判重；close 即释放）。
// 文件形 bind 失败（残留 vs 真占用）由 native 探活区分；此处集合只管抽象地址 + 文件形已占标记。
const __boundPaths = new Set();
// net.BlockList（10f，node 口径：v4/v6 BigInt 二元比较；规则 = address/subnet/range）
class BlockList {
  #rules = [];
  addAddress(ip, type) {
    const t = type ?? (isIP(ip) === 4 ? "ipv4" : isIP(ip) === 6 ? "ipv6" : null);
    const one = __blOne(ip, t);
    one.type = "address";
    this.#rules.push(one);
    return this;
  }
  addSubnet(net, prefix, type) {
    const t = type ?? (isIP(net) === 4 ? "ipv4" : isIP(net) === 6 ? "ipv6" : null);
    const bits = t === "ipv4" ? 32 : 128;
    const p = Number(prefix);
    if (!(Number.isInteger(p) && p >= 0 && p <= bits)) {
      const e = new RangeError(`The "prefix" argument must be >= 0 and <= ${bits}. Received ${prefix}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
    const n = __blOne(net, t);
    this.#rules.push({ t, net: n.ip & __blMask(n.ip, p, bits), mask: __blMask(n.ip, p, bits), type: "subnet", prefix: p });
    return this;
  }
  addRange(start, end, type) {
    let t = type ?? (isIP(start) === 4 ? "ipv4" : isIP(start) === 6 ? "ipv6" : null);
    const a = __blOne(start, t), b = __blOne(end, t);
    if (a.t !== b.t) { const e = new TypeError("IP addresses must be of the same family"); e.code = "ERR_INVALID_ARG_TYPE"; throw e; }
    this.#rules.push({ t, min: a.ip < b.ip ? a.ip : b.ip, max: a.ip < b.ip ? b.ip : a.ip, type: "range" });
    return this;
  }
  check(ip, type) {
    const t = type ?? (isIP(ip) === 4 ? "ipv4" : isIP(ip) === 6 ? "ipv6" : null);
    let one;
    try { one = __blOne(ip, t); } catch { return false; }
    return this.#rules.some((r) => r.t === t && (
      r.type === "address" ? r.ip === one.ip
      : r.type === "subnet" ? (one.ip & r.mask) === r.net
      : one.ip >= r.min && one.ip <= r.max));
  }
  get size() { return this.#rules.length; }
}
function __blMask(ip, prefix, bits) {
  return prefix === 0 ? 0n : ((1n << BigInt(prefix)) - 1n) << BigInt(bits - prefix);
}
function __blOne(ip, t) {
  if (t === "ipv4") {
    if (!__isIPv4(ip)) { const e = new TypeError(`The "ip" argument must be a valid IPv4 address. Received '${ip}'`); e.code = "ERR_INVALID_ARG_VALUE"; throw e; }
    const p = ip.split(".");
    let n = 0n;
    for (const x of p) n = (n << 8n) | BigInt(Number(x));
    return { ip: n, t };
  }
  if (t === "ipv6") {
    if (!__isIPv6(ip)) { const e = new TypeError(`The "ip" argument must be a valid IPv6 address. Received '${ip}'`); e.code = "ERR_INVALID_ARG_VALUE"; throw e; }
    let head = ip, tail = null;
    const i = ip.indexOf("::");
    if (i !== -1) { head = ip.slice(0, i); tail = ip.slice(i + 2); }
    let hp = head === "" ? [] : head.split(":");
    let tp = tail === null ? null : (tail === "" ? [] : tail.split(":"));
    if (tp !== null && tp.length > 0 && tp[tp.length - 1].includes(".")) {
      const v4 = tp.pop();
      const p4 = v4.split(".");
      const hi = (Number(p4[0]) << 8) | Number(p4[1]);
      const lo = (Number(p4[2]) << 8) | Number(p4[3]);
      tp.push(hi.toString(16), lo.toString(16));
    } else if (tp === null && hp.length > 0 && hp[hp.length - 1].includes(".")) {
      const v4 = hp.pop();
      const p4 = v4.split(".");
      const hi = (Number(p4[0]) << 8) | Number(p4[1]);
      const lo = (Number(p4[2]) << 8) | Number(p4[3]);
      hp.push(hi.toString(16), lo.toString(16));
    }
    const fill = tp === null ? 0 : 8 - hp.length - tp.length;
    const all = tp === null ? hp : [...hp, ...Array(fill).fill("0"), ...tp];
    let n = 0n;
    for (const g of all) n = (n << 16n) | BigInt(parseInt(g, 16));
    return { ip: n, t };
  }
  const e = new TypeError(`The "type" argument must be 'ipv4' or 'ipv6'. Received '${t}'`);
  e.code = "ERR_INVALID_ARG_TYPE"; throw e;
}
Object.defineProperty(BlockList.prototype, Symbol.toStringTag, { value: "BlockList" });
BlockList.ValidateString = (ip, type) => { __blOne(ip, type); return true; };
export { BlockList };
export function isIPv4(input) { return isIP(input) === 4; }
export function isIPv6(input) { return isIP(input) === 6; }
// Happy-eyeballs 超时存值（10f：test/common 前置；连接侧暂不实现自动族选择，记档）。
let __autoSelectTimeout = 500;
// 默认自动族选择开关（真机 26 默认 true，实测）；连接侧 Happy Eyeballs 未实现，记档。
let __autoSelectFamily = true;
export function getDefaultAutoSelectFamily() { return __autoSelectFamily; }
export function setDefaultAutoSelectFamily(value) { __autoSelectFamily = !!value; }
export function getDefaultAutoSelectFamilyAttemptTimeout() { return __autoSelectTimeout; }
export function setDefaultAutoSelectFamilyAttemptTimeout(value) {
  // 真机口径（HE 校验族套件）：int [1,60000] 之外 OUT_OF_RANGE；
  // 存取钳 [10,60000]（1/9 → getDefault 10，套件逐项）。
  if (typeof value !== "number" || Number.isNaN(value)) {
    const err = new TypeError("timeout must be a number");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (!Number.isFinite(value) || !Number.isInteger(value) || value < 1 || value > 60000) {
    const err = new RangeError(`The value of "autoSelectFamilyAttemptTimeout" is out of range. It must be an integer >= 1 && <= 60000. Received ${String(value)}`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  __autoSelectTimeout = Math.min(Math.max(value, 10), 60000);
}
const __api = { Socket, Server, BlockList, BoundSocket, createServer, createConnection, connect, Stream, isIP, isIPv4, isIPv6, getDefaultAutoSelectFamily, setDefaultAutoSelectFamily, getDefaultAutoSelectFamilyAttemptTimeout, setDefaultAutoSelectFamilyAttemptTimeout };
export default __api;
"#;

#[cfg(test)]
mod tests {
    #[test]
    fn uds_marker_splits_path_and_mode() {
        // "UDS:<path>[\n<modeBits>]" 约定：无 \n 即无 modeBits。
        let (p, m) = match "UDS:/tmp/x.sock\nrw".strip_prefix("UDS:") {
            Some(rest) => match rest.split_once('\n') {
                Some((pp, mm)) => (pp.to_string(), mm.to_string()),
                None => (rest.to_string(), String::new()),
            },
            None => unreachable!(),
        };
        assert_eq!(p, "/tmp/x.sock");
        assert_eq!(m, "rw");
        let (p2, m2) = match "UDS:/tmp/y.sock".strip_prefix("UDS:") {
            Some(rest) => match rest.split_once('\n') {
                Some((pp, mm)) => (pp.to_string(), mm.to_string()),
                None => (rest.to_string(), String::new()),
            },
            None => unreachable!(),
        };
        assert_eq!(p2, "/tmp/y.sock");
        assert_eq!(m2, "");
    }

    #[test]
    fn bind_fail_marker_shapes_listen_message() {
        // ServerError 整形："CODE: <os>" 取冒号后接 "listen CODE:" 全形；已有 listen 头透传。
        let shape = |code: &str, msg: &str| {
            if msg.starts_with("listen ") {
                msg.to_string()
            } else if let Some(rest) = msg.split_once(':') {
                format!("listen {code}:{}", rest.1)
            } else {
                format!("listen {code}: {msg}")
            }
        };
        assert_eq!(
            shape("EADDRINUSE", "EADDRINUSE: Address already in use (os error 48)"),
            "listen EADDRINUSE: Address already in use (os error 48)"
        );
        assert_eq!(
            shape("EADDRINUSE", "listen EADDRINUSE: address already in use /tmp/x.sock"),
            "listen EADDRINUSE: address already in use /tmp/x.sock"
        );
    }

    #[test]
    fn hold_token_roundtrips_port() {
        // net_bind 回 "port:token"：port 解析取冒号前，token 取冒号后。
        let reply = "51618:7";
        let mut parts = reply.split(':');
        assert_eq!(parts.next(), Some("51618"));
        assert_eq!(parts.next(), Some("7"));
    }
}
