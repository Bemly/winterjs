//! net 泵域（TCP 半部泵 + b64；connect/accept/tls 共用；对齐 net.rs；纯搬移）。

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;
use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};
use crate::state;
use super::net::{bind_tcp_reuseport, NetCmd, NetEvent, NetKind};
use mozjs::context::JSContext;

fn b64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// 数字实参（缺省/非数 → None）。
pub(crate) fn opt_num(frame: &Frame, i: u32) -> Option<f64> {
    let v = frame.arg(i);
    if v.is_number() { Some(v.to_number()) } else { None }
}

pub(crate) fn set_rval_str(cx: &mut JSContext, frame: &Frame, s: &str) {
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
                    // G11 keep-alive-timeout 案：destroy 后对端半开永不 FIN，
                    // 本端读端在 read() 永等不到 EOF，此处不等读端即发 Close
                    //（close_once 防与读端 EOF/错路径双发；读端后到的 EOF 只
                    // 发 End，不再补 Close）。
                    if state::net_close_once(id) {
                        let _ = ev_w.send(NetEvent { id, kind: NetKind::Close });
                    }
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
    // 源 bind 在场时 noDelay 后移到第 5 参（client-adopt/源地址面既有约定）。
    let no_delay = (frame.argc() > 3 && frame.arg(3).is_boolean() && frame.arg(3).to_boolean())
        || (frame.argc() > 4 && frame.arg(4).is_boolean() && frame.arg(4).to_boolean());
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
        match tcp_connect_resolved(host.as_str(), port as u16, src_bind.as_deref()).await {
            Err((code, msg)) => {
                let _ = ev_tx.send(NetEvent {
                    id,
                    kind: NetKind::Error { code: code.into(), msg },
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

/// TCP 建连（DNS 先解 + 逐地址试连；node getaddrinfo 口径；供 `net::tls` 共用）。
/// 成功回 stream；失败回 `(code, msg)`（DNS 解不出即 `ENOTFOUND` +
/// `getaddrinfo ENOTFOUND <host>`，不再经 `io_code` 落 UNKNOWN）。
pub(crate) async fn tcp_connect_resolved(
    host: &str,
    port: u16,
    src: Option<&str>,
) -> Result<tokio::net::TcpStream, (String, String)> {
    // DNS 先解（dns-error 套件）：阻塞调用走 spawn_blocking。
    let resolved: Vec<std::net::SocketAddr> = match tokio::task::spawn_blocking({
        let host = host.to_owned();
        move || {
            use std::net::ToSocketAddrs as _;
            (host.as_str(), port).to_socket_addrs().map(|it| it.collect::<Vec<_>>())
        }
    })
    .await
    {
        Ok(Ok(addrs)) if !addrs.is_empty() => addrs,
        _ => {
            let code = "ENOTFOUND";
            return Err((code.to_string(), format!("getaddrinfo {code} {host}")));
        }
    };
    // 逐个试连（首个成功即停；全败取末错——tokio connect 同语义）。
    // 源地址 bind（localaddress 套件）：有 src 即按目标族建 sock 预 bind，
    // 族不配即跳过该地址；bind 失败按连接错出。
    let src_ip: Option<std::net::IpAddr> = src.and_then(|s| s.parse().ok());
    let mut last_err: Option<std::io::Error> = None;
    for addr in resolved {
        let res = if let Some(src_ip) = src_ip {
            use tokio::net::TcpSocket as _;
            let v6 = matches!(src_ip, std::net::IpAddr::V6(_));
            let tv6 = matches!(addr.ip(), std::net::IpAddr::V6(_));
            if v6 != tv6 {
                continue;
            }
            let sock = if v6 {
                tokio::net::TcpSocket::new_v6()
            } else {
                tokio::net::TcpSocket::new_v4()
            };
            match sock {
                Ok(s) => match s.bind(std::net::SocketAddr::new(src_ip, 0)) {
                    Ok(()) => s.connect(addr).await.map_err(|e| e),
                    Err(e) => Err(e),
                },
                Err(e) => Err(e),
            }
        } else {
            tokio::net::TcpStream::connect(addr).await
        };
        match res {
            Ok(stream) => return Ok(stream),
            Err(e) => {
                last_err = Some(e);
            }
        }
    }
    // addrs 非空，必有末错。
    let e = last_err.expect("non-empty addrs always yield a result");
    let code = crate::builtins::node::fs::io_code(&e);
    Err((code.to_string(), format!("{code}: {e}")))
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
            // 落到外层统一 return（Failed 分支已早返，此处不直接 return，
            // 否则外层 return 在 unix 下恒不可达）。
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
