//! `node:dgram`：UDP Socket（tokio net 底座，dependencies2 §9d 口径）。
/// 复用 net 的事件通道/状态机（NetCmd::SendTo + NetKind::Dgram* 变体、
/// net_open 计数、close_once 单发旗、dispatch Close 统一收尾，见 net.rs）。
/// 单 task `select!`（recv_from ↔ 命令通道）；UDP 数据报无连接语义，一次收发
/// 一事件。
/// 10a 全家：connect/disconnect（task 级默认远端，无内核过滤）+ 组播
/// （join/leave/setMulticastTTL/setMulticastLoopback）+ 广播/TTL +
/// ref 真计数（`__wjs_net_ref/unref`）。
/// 偏差记档：
/// - sockopt 系失败走异步 Error 事件（真机同步抛；socket 活在 task，JS 线程
///   阻塞等回包会死锁 runtime——fire-and-forget 是架构选择，不是偷懒）。
/// - connect 不做内核 connect（UDP 无连接；task 只记默认远端，send 照常可
///   显式覆盖）；connect 前的 DNS 只为展示 remoteAddress 解析一次，发送时由
///   task 再解（与真机 async lookup 时序差一拍，hermetic 下无感）。
/// - v6 接口名不支持（只收数字索引；名形走 Error 事件）；v6 组播 TTL 不支持
///  （tokio 未暴露 v6 版，走 ENOPROTOOPT 事件）。
/// - close 幂等（真机 26 二次 close 抛 ERR_SOCKET_DGRAM_NOT_RUNNING，本仓静默
///   no-op——复用 net 系关闭语义，不跟）。
/// - hermetic 组播配方：双端绑 0.0.0.0 + 默认接口加组（127.0.0.1 端在受限
///   沙箱收不到组播，真机同配方同样收不到，已对拍；见黑盒）。
/// - get/setRecvBufferSize 系未实现（需 socket2/nix socket 特性，另切片）。

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::context::JSContext;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::builtins::node::net::{NetCmd, NetEvent, NetKind};
use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};
use crate::state;

fn set_rval_str(cx: &mut JSContext, frame: &Frame, s: &str) {
    rooted!(&in(cx) let mut v = UndefinedValue());
    s.to_jsval(cx, v.handle_mut());
    frame.set_rval(v.get());
}

fn opt_num(frame: &Frame, i: u32) -> Option<f64> {
    let v = frame.arg(i);
    if v.is_number() { Some(v.to_number()) } else { None }
}

/// 带预选项的同步建套（bind 前落 SO_REUSEPORT/IPV6_V6ONLY；tokio from_std
/// 由调用方接管。复用：async bind 与 bindSync 共用）。
/// flags 位：1 = reusePort，2 = ipv6Only（仅 v6 有意义）。
fn std_bind_flags(addr: &str, port: u16, flags: u32) -> std::io::Result<std::net::UdpSocket> {
    use std::os::fd::{FromRawFd, OwnedFd};
    let raw = addr.strip_prefix('[').and_then(|s| s.strip_suffix(']')).unwrap_or(addr);
    let is_v6 = raw.contains(':');
    let reuse_port = flags & 1 != 0;
    let v6only = flags & 2 != 0;
    if !reuse_port && !v6only {
        return std::net::UdpSocket::bind((raw, port));
    }
    unsafe {
        let fd = libc::socket(
            if is_v6 { libc::AF_INET6 } else { libc::AF_INET },
            libc::SOCK_DGRAM,
            0,
        );
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let sock = OwnedFd::from_raw_fd(fd);
        let one: libc::c_int = 1;
        if reuse_port
            && libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_REUSEPORT,
                &one as *const _ as *const libc::c_void,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            ) != 0
        {
            return Err(std::io::Error::last_os_error());
        }
        if v6only && is_v6
            && libc::setsockopt(
                fd,
                libc::IPPROTO_IPV6,
                libc::IPV6_V6ONLY,
                &one as *const _ as *const libc::c_void,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            ) != 0
        {
            return Err(std::io::Error::last_os_error());
        }
        if is_v6 {
            let ip: std::net::Ipv6Addr = raw.parse().map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid address")
            })?;
            let mut sa: libc::sockaddr_in6 = std::mem::zeroed();
            sa.sin6_family = libc::AF_INET6 as _;
            sa.sin6_port = port.to_be();
            sa.sin6_addr = libc::in6_addr { s6_addr: ip.octets() };
            if libc::bind(
                fd,
                &sa as *const _ as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
            ) != 0
            {
                return Err(std::io::Error::last_os_error());
            }
        } else {
            let ip: std::net::Ipv4Addr = raw.parse().map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid address")
            })?;
            let mut sa: libc::sockaddr_in = std::mem::zeroed();
            sa.sin_family = libc::AF_INET as _;
            sa.sin_port = port.to_be();
            sa.sin_addr = libc::in_addr { s_addr: u32::from_ne_bytes(ip.octets()) };
            if libc::bind(
                fd,
                &sa as *const _ as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            ) != 0
            {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(std::net::UdpSocket::from(sock))
    }
}

fn v4_in_addr(a: &std::net::Ipv4Addr) -> libc::in_addr {
    // s_addr 存网序字节：from_ne_bytes 保版式（标准写法）。
    libc::in_addr { s_addr: u32::from_ne_bytes(a.octets()) }
}

/// SSM 加/退组（v4 走 IP_ADD/DROP_SOURCE_MEMBERSHIP；v6 走
/// MCAST_JOIN/LEAVE_SOURCE_GROUP；输入经 JS 侧预验，此处失败即 Error 事件）。
fn dgram_ssm(
    sock: &tokio::net::UdpSocket,
    multi: &str,
    iface: &str,
    source: &str,
    join: bool,
) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    let fd = sock.as_raw_fd();
    if let (Ok(m), Ok(i), Ok(s)) = (
        multi.parse::<std::net::Ipv4Addr>(),
        iface.parse::<std::net::Ipv4Addr>(),
        source.parse::<std::net::Ipv4Addr>(),
    ) {
        let mreq = libc::ip_mreq_source {
            imr_multiaddr: v4_in_addr(&m),
            imr_interface: v4_in_addr(&i),
            imr_sourceaddr: v4_in_addr(&s),
        };
        let opt = if join { libc::IP_ADD_SOURCE_MEMBERSHIP } else { libc::IP_DROP_SOURCE_MEMBERSHIP };
        let ret = unsafe {
            libc::setsockopt(
                fd,
                libc::IPPROTO_IP,
                opt,
                &mreq as *const _ as *const libc::c_void,
                std::mem::size_of::<libc::ip_mreq_source>() as libc::socklen_t,
            )
        };
        return if ret == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) };
    }
    // v6：RFC 3678 group_source_req（libc 未为 apple 导出，自定 ABI：
    // u32 interface + sockaddr_storage group/source；常量 macOS 82/83）。
    let (m, s) = match (multi.parse::<std::net::Ipv6Addr>(), source.parse::<std::net::Ipv6Addr>()) {
        (Ok(m), Ok(s)) => (m, s),
        _ => {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid address"));
        }
    };
    #[repr(C)]
    struct GroupSourceReq {
        interface: u32,
        group: libc::sockaddr_storage,
        source: libc::sockaddr_storage,
    }
    #[cfg(target_os = "macos")]
    const JOIN_SRC: libc::c_int = 82;
    #[cfg(target_os = "macos")]
    const LEAVE_SRC: libc::c_int = 83;
    #[cfg(not(target_os = "macos"))]
    const JOIN_SRC: libc::c_int = 46;
    #[cfg(not(target_os = "macos"))]
    const LEAVE_SRC: libc::c_int = 47;
    let ifindex: u32 = iface.parse().unwrap_or(0);
    let mut req: GroupSourceReq = unsafe { std::mem::zeroed() };
    req.interface = ifindex;
    let fill = |stor: &mut libc::sockaddr_storage, ip: &std::net::Ipv6Addr| {
        let mut sa: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
        sa.sin6_family = libc::AF_INET6 as _;
        sa.sin6_addr = libc::in6_addr { s6_addr: ip.octets() };
        unsafe { std::ptr::write(stor as *mut _ as *mut libc::sockaddr_in6, sa) };
    };
    fill(&mut req.group, &m);
    fill(&mut req.source, &s);
    let opt = if join { JOIN_SRC } else { LEAVE_SRC };
    let ret = unsafe {
        libc::setsockopt(
            fd,
            libc::IPPROTO_IPV6,
            opt,
            &req as *const _ as *const libc::c_void,
            std::mem::size_of::<GroupSourceReq>() as libc::socklen_t,
        )
    };
    if ret == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
}

/// 出站组播接口（v4 用 IP_MULTICAST_IF + in_addr；v6 用 IPV6_MULTICAST_IF +
/// 接口索引，%scope 经 if_nametoindex，失败回 0 即系统选择——Node 口径）。
fn dgram_mcast_if(sock: &tokio::net::UdpSocket, addr: &str, is_v4: bool) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    let fd = sock.as_raw_fd();
    if is_v4 {
        let ip: std::net::Ipv4Addr = addr
            .parse()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid address"))?;
        let raw = v4_in_addr(&ip);
        let ret = unsafe {
            libc::setsockopt(
                fd,
                libc::IPPROTO_IP,
                libc::IP_MULTICAST_IF,
                &raw as *const _ as *const libc::c_void,
                std::mem::size_of::<libc::in_addr>() as libc::socklen_t,
            )
        };
        return if ret == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) };
    }
    let index: u32 = match addr.split_once('%') {
        Some((_, scope)) if !scope.is_empty() => {
            let name = std::ffi::CString::new(scope).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid scope")
            })?;
            unsafe { libc::if_nametoindex(name.as_ptr()) }
        }
        _ => 0,
    };
    let ret = unsafe {
        libc::setsockopt(
            fd,
            libc::IPPROTO_IPV6,
            libc::IPV6_MULTICAST_IF,
            &index as *const _ as *const libc::c_void,
            std::mem::size_of::<u32>() as libc::socklen_t,
        )
    };
    if ret == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
}

/// `__wjs_dgram_bind(port, address, target)` → id。bind 错误经 Error+Close 事件。
pub unsafe extern "C" fn dgram_bind(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 || !frame.arg(2).is_object() {
        report_error(&mut cx, "TypeError: dgram internals missing target");
        return false;
    }
    let Some(port) = opt_num(&frame, 0) else {
        report_error(&mut cx, "TypeError: bind: port must be a number");
        return false;
    };
    let address = value_to_string(&mut cx, frame.arg(1));
    let target = frame.arg(2);
    // 预选项（bit0 reusePort/bit1 ipv6Only）：有则走 std 预置后 from_std，
    // 无则沿旧 tokio 直绑快路（缺参容忍：旧 JS 只传 3 参时按 0 处理，禁越界取参）。
    let flags = if frame.argc() > 3 { opt_num(&frame, 3).unwrap_or(0.0) } else { 0.0 } as u32;
    let Some((id, ev_tx)) = state::net_alloc() else {
        report_error(&mut cx, "OperationError: net driver not installed");
        return false;
    };
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        report_error(&mut cx, "OperationError: no async runtime for dgram bind");
        return false;
    };
    let cmd_rx = state::net_socket_add(id, target);
    set_rval_str(&mut cx, &frame, &id.to_string());
    handle.spawn(async move {
        let bound: std::io::Result<tokio::net::UdpSocket> = if flags == 0 {
            tokio::net::UdpSocket::bind((address.as_str(), port as u16)).await
        } else {
            (|| {
                let std_sock = std_bind_flags(&address, port as u16, flags)?;
                std_sock.set_nonblocking(true)?;
                tokio::net::UdpSocket::from_std(std_sock)
            })()
        };
        let Ok(sock) = bound else {
            let e = bound.unwrap_err();
            let code = crate::builtins::node::fs::io_code(&e);
            let _ = ev_tx.send(NetEvent {
                id,
                kind: NetKind::Error { code: code.into(), msg: format!("{code}: {e}") },
            });
            if state::net_close_once(id) {
                let _ = ev_tx.send(NetEvent { id, kind: NetKind::Close });
            }
            return;
        };
        dgram_task(id, ev_tx, sock, cmd_rx, None).await;
    });
    true
}

/// 同步绑定 + 建 task（bindSync/connectSync 底座；失败走 rval 错误 JSON，
/// JS 侧整形成带码同步抛错——report_error 无码，不用）。
/// rval：成功 `{"id","addr","port"}`；失败 `{"error": code}`。
/// UNSAFE-BOUNDARY(dgram_bind_sync)：前置同 dgram_bind；新增 std 阻塞 bind
/// （UDP bind 即时返回，无死锁风险）；tokio::from_std 失败同一错误通道；
/// 覆盖：tests/node/dgram.rs bindSync 面黑盒。
pub unsafe extern "C" fn dgram_bind_sync(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调入参有效；target 为 JS 对象（task 表保活）。
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 || !frame.arg(3).is_object() {
        report_error(&mut cx, "TypeError: dgram internals missing target");
        return false;
    }
    let Some(port) = opt_num(&frame, 0) else {
        report_error(&mut cx, "TypeError: bind: port must be a number");
        return false;
    };
    let address = value_to_string(&mut cx, frame.arg(1));
    let remote = value_to_string(&mut cx, frame.arg(2));
    let target = frame.arg(3);
    // 缺参容忍：旧 JS 只传 4 参时 flags 按 0 处理，禁越界取参（Frame::arg 越界即 panic）。
    let flags = if frame.argc() > 4 { opt_num(&frame, 4).unwrap_or(0.0) } else { 0.0 } as u32;
    // 先同步 bind（失败直接回错误码，不占 id/表项，无需回滚）。
    let bound = std_bind_flags(&address, port as u16, flags);
    let Ok(std_sock) = bound else {
        let e = bound.unwrap_err();
        let code = crate::builtins::node::fs::io_code(&e);
        set_rval_str(&mut cx, &frame, &format!("{{\"error\":\"{code}\"}}"));
        return true;
    };
    if !remote.is_empty() {
        if let Err(e) = std_sock.connect(remote.as_str()) {
            let code = crate::builtins::node::fs::io_code(&e);
            set_rval_str(&mut cx, &frame, &format!("{{\"error\":\"{code}\"}}"));
            return true;
        }
    }
    // from_std 前必转 nonblocking（UDP 无握手，connect 不阻塞；tokio 拒收
    // blocking fd，否则 panic）。
    if let Err(e) = std_sock.set_nonblocking(true) {
        let code = crate::builtins::node::fs::io_code(&e);
        set_rval_str(&mut cx, &frame, &format!("{{\"error\":\"{code}\"}}"));
        return true;
    }
    let local = std_sock.local_addr().ok();
    let Ok(sock) = tokio::net::UdpSocket::from_std(std_sock) else {
        set_rval_str(&mut cx, &frame, "{\"error\":\"UNKNOWN\"}");
        return true;
    };
    let local = local.or_else(|| sock.local_addr().ok());
    let Some((id, ev_tx)) = state::net_alloc() else {
        report_error(&mut cx, "OperationError: net driver not installed");
        return false;
    };
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        report_error(&mut cx, "OperationError: no async runtime for dgram bind");
        return false;
    };
    let cmd_rx = state::net_socket_add(id, target);
    let (addr_s, port_n) = local
        .map(|a| (a.ip().to_string(), a.port()))
        .unwrap_or_else(|| ("0.0.0.0".to_string(), 0));
    set_rval_str(&mut cx, &frame, &format!("{{\"id\":{id},\"addr\":\"{addr_s}\",\"port\":{port_n}}}"));
    let preset = if remote.is_empty() { None } else { Some(remote) };
    handle.spawn(async move {
        dgram_task(id, ev_tx, sock, cmd_rx, preset).await;
    });
    true
}

/// dgram 收发 task（bind 成功后接管；默认远端可预置——connectSync 底座）。
async fn dgram_task(
    id: u64,
    ev_tx: tokio::sync::mpsc::UnboundedSender<NetEvent>,
    sock: tokio::net::UdpSocket,
    mut cmd_rx: tokio::sync::mpsc::UnboundedReceiver<NetCmd>,
    default_remote: Option<String>,
) {
        let local = sock
            .local_addr()
            .unwrap_or_else(|_| "0.0.0.0:0".parse::<std::net::SocketAddr>().expect("literal addr"));
        let _ = ev_tx.send(NetEvent {
            id,
            kind: NetKind::DgramListening { addr: local.ip().to_string(), port: local.port() },
        });
        let sock = std::sync::Arc::new(sock);
        // fd 登记给同步 bufsize native（task 收尾摘除，防悬垂查表）
        {
            use std::os::fd::AsRawFd;
            dgram_fd_add(id, sock.as_raw_fd());
        }
        let is_v4 = local.is_ipv4();
        // task 级 connect 的默认远端（无内核过滤，记档；disconnect 清除；
        // connectSync 预置经参数传入）。
        let mut default_remote = default_remote;
        // 单 task：收数据报 ↔ 命令（SendTo/Close/10a sockopt 全家）
        let rsock = sock.clone();
        let mut buf = vec![0u8; 65536];
        // sockopt 失败走 Error 事件（真机同步抛的偏差记档：socket 活在 task，
        // JS 线程阻塞等回包会死锁 runtime，见 10a-6）。
        let sockopt_fail = |syscall: &str, e: &std::io::Error| NetEvent {
            id,
            kind: NetKind::Error {
                code: crate::builtins::node::fs::io_code(e).into(),
                msg: format!("{} {}", syscall, crate::builtins::node::fs::io_code(e)),
            },
        };
        loop {
            tokio::select! {
                r = rsock.recv_from(&mut buf) => {
                    match r {
                        Ok((n, peer)) => {
                            let _ = ev_tx.send(NetEvent {
                                id,
                                kind: NetKind::DgramMessage {
                                    data_b64: {
                                        use base64::Engine as _;
                                        base64::engine::general_purpose::STANDARD.encode(&buf[..n])
                                    },
                                    address: peer.ip().to_string(),
                                    port: peer.port(),
                                    family: if peer.is_ipv4() { 4 } else { 6 },
                                },
                            });
                        }
                        Err(e) => {
                            let code = crate::builtins::node::fs::io_code(&e);
                            let _ = ev_tx.send(NetEvent {
                                id,
                                kind: NetKind::Error { code: code.into(), msg: format!("{code}: {e}") },
                            });
                            break;
                        }
                    }
                }
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(NetCmd::SendTo { data, addr, seq }) => {
                            let target = if addr.is_empty() { default_remote.clone() } else { Some(addr) };
                            let Some(target) = target else {
                                let _ = ev_tx.send(NetEvent {
                                    id,
                                    kind: NetKind::DgramSendError {
                                        code: "ERR_SOCKET_DGRAM_NOT_CONNECTED".into(),
                                        msg: "Not connected".into(),
                                        seq,
                                    },
                                });
                                continue;
                            };
                            if let Err(e) = rsock.send_to(&data, target.as_str()).await {
                                // node 口径：send 失败不杀 socket（原 break 即杀任务，错），
                                // 错误路由回 send 回调（msgsize 套件：EMSGSIZE + "send …" 文案）。
                                let code = crate::builtins::node::fs::io_code(&e);
                                let _ = ev_tx.send(NetEvent {
                                    id,
                                    kind: NetKind::DgramSendError {
                                        code: code.into(),
                                        msg: format!("send {code} {target}"),
                                        seq,
                                    },
                                });
                                continue;
                            }
                            let _ = ev_tx.send(NetEvent {
                                id,
                                kind: NetKind::DgramSendOk { seq, bytes: data.len() },
                            });
                        }
                        Some(NetCmd::DgramBroadcast(v)) => {
                            if let Err(e) = rsock.set_broadcast(v) {
                                let _ = ev_tx.send(sockopt_fail("setBroadcast", &e));
                            }
                        }
                        Some(NetCmd::DgramMulticastLoop(v)) => {
                            let r = if is_v4 {
                                rsock.set_multicast_loop_v4(v)
                            } else {
                                rsock.set_multicast_loop_v6(v)
                            };
                            if let Err(e) = r {
                                let _ = ev_tx.send(sockopt_fail("setMulticastLoopback", &e));
                            }
                        }
                        Some(NetCmd::DgramMulticastTtl(v)) => {
                            // tokio 只暴露 v4 版；v6 显式 ENOPROTOOPT（记档，
                            // 不用 errno 数字——ENOPROTOOPT mac/Linux 码不同）。
                            if !is_v4 {
                                let _ = ev_tx.send(NetEvent {
                                    id,
                                    kind: NetKind::Error {
                                        code: "ENOPROTOOPT".into(),
                                        msg: "ENOPROTOOPT setMulticastTTL".into(),
                                    },
                                });
                                continue;
                            }
                            if let Err(e) = rsock.set_multicast_ttl_v4(v as u32) {
                                let _ = ev_tx.send(sockopt_fail("setMulticastTTL", &e));
                            }
                        }
                        Some(NetCmd::DgramTtl(v)) => {
                            if let Err(e) = rsock.set_ttl(v) {
                                let _ = ev_tx.send(sockopt_fail("setTTL", &e));
                            }
                        }
                        Some(NetCmd::DgramJoin { multi, iface }) => {
                            let r = if is_v4 {
                                match (multi.parse::<std::net::Ipv4Addr>(), iface.parse::<std::net::Ipv4Addr>()) {
                                    (Ok(m), Ok(i)) => rsock.join_multicast_v4(m, i),
                                    _ => Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid address")),
                                }
                            } else {
                                match (multi.parse::<std::net::Ipv6Addr>(), iface.parse::<u32>()) {
                                    (Ok(m), Ok(i)) => rsock.join_multicast_v6(&m, i),
                                    _ => Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid address")),
                                }
                            };
                            if let Err(e) = r {
                                let _ = ev_tx.send(sockopt_fail("addMembership", &e));
                            }
                        }
                        Some(NetCmd::DgramLeave { multi, iface }) => {
                            let r = if is_v4 {
                                match (multi.parse::<std::net::Ipv4Addr>(), iface.parse::<std::net::Ipv4Addr>()) {
                                    (Ok(m), Ok(i)) => rsock.leave_multicast_v4(m, i),
                                    _ => Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid address")),
                                }
                            } else {
                                match (multi.parse::<std::net::Ipv6Addr>(), iface.parse::<u32>()) {
                                    (Ok(m), Ok(i)) => rsock.leave_multicast_v6(&m, i),
                                    _ => Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid address")),
                                }
                            };
                            if let Err(e) = r {
                                let _ = ev_tx.send(sockopt_fail("dropMembership", &e));
                            }
                        }
                        Some(NetCmd::DgramJoinSource { multi, iface, source }) => {
                            if let Err(e) = dgram_ssm(&rsock, &multi, &iface, &source, true) {
                                let _ = ev_tx.send(sockopt_fail("addSourceSpecificMembership", &e));
                            }
                        }
                        Some(NetCmd::DgramLeaveSource { multi, iface, source }) => {
                            if let Err(e) = dgram_ssm(&rsock, &multi, &iface, &source, false) {
                                let _ = ev_tx.send(sockopt_fail("dropSourceSpecificMembership", &e));
                            }
                        }
                        Some(NetCmd::DgramMulticastInterface { addr }) => {
                            if let Err(e) = dgram_mcast_if(&rsock, &addr, is_v4) {
                                let _ = ev_tx.send(sockopt_fail("setMulticastInterface", &e));
                            }
                        }
                        Some(NetCmd::DgramConnect { addr }) => {
                            default_remote = Some(addr);
                            let _ = ev_tx.send(NetEvent { id, kind: NetKind::DgramConnect });
                        }
                        Some(NetCmd::DgramDisconnect) => {
                            default_remote = None;
                        }
                        _ => break, // Close 或写端掉光
                    }
                }
            }
        }
        dgram_fd_remove(id);
        if state::net_close_once(id) {
            // purge 由 dispatch 派发 Close 之后统一做（§4.34 症状三：先清后派发即丢事件）
            let _ = ev_tx.send(NetEvent { id, kind: NetKind::Close });
        }
}

/// `__wjs_dgram_send(id, dataBytes, addrStr)`（addr = "ip:port"）。
pub unsafe extern "C" fn dgram_send(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = opt_num(&frame, 0) else {
        report_error(&mut cx, "TypeError: send: id must be a number");
        return false;
    };
    let data = if frame.arg(1).is_string() {
        value_to_string(&mut cx, frame.arg(1)).into_bytes()
    } else {
        match crate::jsapi_glue::view_bytes(&mut cx, frame.arg(1), "send data") {
            Some(b) => b,
            None => return false,
        }
    };
    let addr = value_to_string(&mut cx, frame.arg(2));
    let seq = opt_num(&frame, 3).unwrap_or(0.0) as u64;
    if !state::net_cmd(id as u64, NetCmd::SendTo { data, addr, seq }) {
        report_error(&mut cx, "ERR_SOCKET_DGRAM_NOT_RUNNING: send: socket is gone");
        return false;
    }
    true
}

/// `__wjs_dgram_sockopt(id, opJson)`（10a 组播/广播/TTL/connect 全家；
/// opJson 如 `{"op":"setBroadcast","v":true}`；fire-and-forget，失败走 Error 事件）。
pub unsafe extern "C" fn dgram_sockopt(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = opt_num(&frame, 0) else {
        report_error(&mut cx, "TypeError: sockopt: id must be a number");
        return false;
    };
    let op_str = value_to_string(&mut cx, frame.arg(1));
    let Ok(op) = serde_json::from_str::<serde_json::Value>(&op_str) else {
        report_error(&mut cx, "TypeError: sockopt: op must be JSON");
        return false;
    };
    let str_field = |k: &str| op.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let cmd = match op.get("op").and_then(|v| v.as_str()) {
        Some("setBroadcast") => NetCmd::DgramBroadcast(op.get("v").and_then(|v| v.as_bool()).unwrap_or(false)),
        Some("setMulticastLoop") => {
            NetCmd::DgramMulticastLoop(op.get("v").and_then(|v| v.as_bool()).unwrap_or(false))
        }
        Some("setMulticastTtl") => NetCmd::DgramMulticastTtl(
            op.get("v").and_then(|v| v.as_u64()).unwrap_or(1).min(255) as u8,
        ),
        Some("setTtl") => NetCmd::DgramTtl(op.get("v").and_then(|v| v.as_u64()).unwrap_or(64) as u32),
        Some("join") => NetCmd::DgramJoin { multi: str_field("multi"), iface: str_field("iface") },
        Some("joinSource") => NetCmd::DgramJoinSource {
            multi: str_field("multi"),
            iface: str_field("iface"),
            source: str_field("source"),
        },
        Some("leaveSource") => NetCmd::DgramLeaveSource {
            multi: str_field("multi"),
            iface: str_field("iface"),
            source: str_field("source"),
        },
        Some("multicastInterface") => NetCmd::DgramMulticastInterface { addr: str_field("addr") },
        Some("leave") => NetCmd::DgramLeave { multi: str_field("multi"), iface: str_field("iface") },
        Some("connect") => NetCmd::DgramConnect { addr: str_field("addr") },
        Some("disconnect") => NetCmd::DgramDisconnect,
        _ => {
            report_error(&mut cx, "TypeError: sockopt: unknown op");
            return false;
        }
    };
    if !state::net_cmd(id as u64, cmd) {
        report_error(&mut cx, "ERR_SOCKET_DGRAM_NOT_RUNNING: sockopt: socket is gone");
        return false;
    }
    true
}

/// dgram fd 表（id → 原生 fd）：bind 任务成功后登记、task 收尾摘除。
/// `__wjs_dgram_bufsize` 同步 get/setsockopt 用——fd 归 task 所有，但 sockopt
/// 系统调用只按号操作、跨线程安全；摘除后查表即 miss（JS 侧转
/// ERR_SOCKET_BUFFER_SIZE），无悬垂窗口（§4.48：有收尾的进程内状态才可 static）。
static DGRAM_FDS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<u64, i32>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn dgram_fd_add(id: u64, fd: i32) {
    DGRAM_FDS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(id, fd);
}

fn dgram_fd_remove(id: u64) {
    DGRAM_FDS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&id);
}

fn dgram_fd_get(id: u64) -> Option<i32> {
    DGRAM_FDS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&id)
        .copied()
}

/// `__wjs_dgram_bufsize(id, "recv"|"send"[, size])` → 当前值串（§4.33 字符串返回，
/// JS 侧 Number() 包装）。带 size 即 setsockopt，否则 getsockopt（SO_RCVBUF/SO_SNDBUF）。
/// fd 查表 miss 或 syscall 失败回空串（JS 侧转 ERR_SOCKET_BUFFER_SIZE，真机
/// "Could not get or set buffer size: uv_recv_buffer_size returned EBADF …" 口径）。
pub unsafe extern "C" fn dgram_bufsize(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let out = (|| -> Option<i32> {
        let id = opt_num(&frame, 0)? as u64;
        let kind = value_to_string(&mut cx, frame.arg(1));
        let opt = match kind.as_str() {
            "recv" => libc::SO_RCVBUF,
            "send" => libc::SO_SNDBUF,
            _ => return None,
        };
        let fd = dgram_fd_get(id)?;
        if let Some(sz) = opt_num(&frame, 2) {
            let v = sz as libc::c_int;
            let rc = unsafe {
                libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    opt,
                    &v as *const _ as *const libc::c_void,
                    std::mem::size_of::<libc::c_int>() as libc::socklen_t,
                )
            };
            if rc != 0 { None } else { Some(v) }
        } else {
            let mut val: libc::c_int = 0;
            let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
            let rc = unsafe {
                libc::getsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    opt,
                    &mut val as *mut _ as *mut libc::c_void,
                    &mut len,
                )
            };
            if rc != 0 { None } else { Some(val) }
        }
    })();
    match out {
        Some(v) => set_rval_str(&mut cx, &frame, &v.to_string()),
        None => set_rval_str(&mut cx, &frame, ""),
    }
    true
}

/// 内嵌 ESM 源（`node:dgram`；net 底座）。
pub const SOURCE: &str = r#"
import { EventEmitter } from "node:events";
import errors from 'node:internal/errors';
import { validatePort } from 'node:internal/validators';
import __dnsDefault from "node:dns";
const Buffer = globalThis.Buffer;

const {
  codes: {
    ERR_MISSING_ARGS,
  },
} = errors;

function __b64dec(s) {
  const bin = atob(s);
  const u8 = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) u8[i] = bin.charCodeAt(i);
  return u8;
}
function __toU8(data) {
  // node 口径：send 首参收 string/Buffer/视图/**数组**（逐段拼接，空数组即 0 字节，
  // send-callback-multi-buffer 系套件）。
  if (Array.isArray(data)) {
    const parts = data.map((m) => {
      if (typeof m === "string") return new TextEncoder().encode(m);
      if (m instanceof Uint8Array) return m;
      if (m instanceof ArrayBuffer) return new Uint8Array(m);
      if (ArrayBuffer.isView(m)) return new Uint8Array(m.buffer, m.byteOffset, m.byteLength);
      throw new TypeError("send: list items must be string or BufferSource");
    });
    const total = parts.reduce((n, p) => n + p.length, 0);
    const out = new Uint8Array(total);
    let at = 0;
    for (const p of parts) { out.set(p, at); at += p.length; }
    return out;
  }
  if (typeof data === "string") return new TextEncoder().encode(data);
  if (data instanceof Uint8Array) return data;
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  throw new TypeError("send: data must be string or BufferSource");
}
function __netErr(code, msg) {
  const e = new Error(msg);
  e.code = code;
  return e;
}
// validators 口径 Received 形（ttl 校验逐字点名）。
function __dgramReceived(v) {
  if (v === null) return "null";
  if (v === undefined) return "undefined";
  const t = typeof v;
  if (t === "string") return `type string ('${v}')`;
  if (t === "boolean") return `type boolean (${v})`;
  if (t === "number") return `type number (${v})`;
  if (t === "object") return Array.isArray(v) ? "an instance of Array" : `an instance of ${v.constructor?.name ?? "Object"}`;
  if (t === "function") return `function ${v.name}`;
  return `type ${t} (${String(v)})`;
}
function __dgramTtl(name, ttl, min, max) {
  if (typeof ttl !== "number") {
    const e = new TypeError(`The "ttl" argument must be of type number. Received ${__dgramReceived(ttl)}`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  if (!Number.isInteger(ttl) || ttl < min || ttl > max) throw __sysErr(name, 'EINVAL');
  return ttl;
}
// 系统调用失败形（真机 `EINVAL: addMembership EINVAL` 口径；task 侧失败走
// Error 事件见模块头注，此处只做 JS 侧可同步判定的格式校验）。
function __sysErr(syscall, code) {
  const e = new Error(`${syscall} ${code}`);
  e.code = code;
  return e;
}
function __parseIPv4(s) {
  const parts = String(s).split('.');
  if (parts.length !== 4) return null;
  const nums = [];
  for (const p of parts) {
    if (!/^\d+$/.test(p)) return null;
    const n = Number(p);
    if (n > 255) return null;
    nums.push(n);
  }
  return nums;
}
// v6 字面量粗验（够 setMulticastInterface 非法形判定；route 用原文）。
// 注：手写 hex 判定，不用正则字面量（本模块内该位置正则字面量曾伴随
// require 链 crash 高频现形；根因为 require_cjs_file 的 make_fn 裸值窗口，
// 已修，此处保守形态保留）。
function __parseIPv6Hex(g) {
  if (g.length < 1 || g.length > 4) return false;
  for (let i = 0; i < g.length; i++) {
    const c = g.charCodeAt(i);
    const dig = c >= 48 && c <= 57;
    const lo = c >= 97 && c <= 102;
    const hi = c >= 65 && c <= 70;
    if (!dig && !lo && !hi) return false;
  }
  return true;
}
function __parseIPv6(s) {
  if (typeof s !== "string" || !s.includes(":")) return false;
  const chk = (arr) => {
    for (const g of arr) { if (!__parseIPv6Hex(g)) return false; }
    return true;
  };
  if (s.includes("::")) {
    if (s.indexOf("::") !== s.lastIndexOf("::")) return false;
    const parts = s.split("::");
    const lg = parts[0] === "" ? [] : parts[0].split(":");
    const rg = parts[1] === "" ? [] : parts[1].split(":");
    if (lg.length + rg.length > 7) return false;
    return chk(lg) && chk(rg);
  }
  const gs = s.split(":");
  return gs.length === 8 && chk(gs);
}
// 组播地址校验（v4 限 224/4，v6 限 ff00::/8；跨类型错配即 EINVAL，真机口径）。
function __membershipAddrs(multi, iface, type, syscall) {
  if (multi === undefined) throw new ERR_MISSING_ARGS('multicastAddress');
  const v4 = (typeof multi === 'string') ? __parseIPv4(multi) : null;
  let v6 = false;
  if (v4) {
    if (v4[0] < 224 || v4[0] > 239) throw __sysErr(syscall, 'EINVAL');
  } else if (typeof multi === 'string' && multi.includes(':')) {
    v6 = true;
    if (!/^ff/i.test(multi)) throw __sysErr(syscall, 'EINVAL');
  } else {
    throw __sysErr(syscall, 'EINVAL');
  }
  if (type === 'udp4' && v6) throw __sysErr(syscall, 'EINVAL');
  if (type === 'udp6' && !v6) throw __sysErr(syscall, 'EINVAL');
  return { multi: String(multi), iface: iface === undefined ? (v6 ? '0' : '0.0.0.0') : String(iface) };
}

class Socket extends EventEmitter {
  constructor(typeOrOptions, cb) {
    super();
    let type;
    if (typeof typeOrOptions === "string") type = typeOrOptions;
    else if (typeOrOptions && typeof typeOrOptions === "object") type = typeOrOptions.type;
    if (type !== "udp4" && type !== "udp6") {
      const e = new TypeError(`Bad socket type specified. Valid types are: udp4, udp6`);
      e.code = "ERR_SOCKET_BAD_TYPE";
      throw e;
    }
    this.type = type;
    this.__id = 0;
    this.__bound = false;
    // 构造缓冲选项校验（createSocket-type 套件：非 number 即 ARG_TYPE）。
    if (typeOrOptions && typeof typeOrOptions === "object") {
      for (const k of ["recvBufferSize", "sendBufferSize"]) {
        const v = typeOrOptions[k];
        if (v !== undefined && typeof v !== "number") {
          const e = new TypeError(`The "${k}" argument must be of type number. Received ${__dgramReceived(v)}`);
          e.code = "ERR_INVALID_ARG_TYPE"; throw e;
        }
      }
    }
    // 连接/关闭状态（connect 连接机 + close 后 NOT_RUNNING 门，见下）。
    this.__connecting = false;
    this.__closed = false;
    // 绑定窗口旗（bind 错误走 ExceptionWithHostPort 形，见 __evErrorBind）。
    this.__binding = false;
    // 绑定失败旗（bind-error-repeat：错误处理器内可立即重绑；陈旧 Close
    // 按"重绑与否"分流，见 __ev close）。
    this.__bindFailed = false;
    this.__opts = (typeOrOptions && typeof typeOrOptions === "object") ? typeOrOptions : null;
    this.__addr = null;
    this.__connected = false;
    this.__remote = null;
    this.__pending = [];
    this.__blockList = (typeOrOptions && typeof typeOrOptions === "object") ? (typeOrOptions.sendBlockList ?? null) : null;
    this.__recvBlockList = (typeOrOptions && typeof typeOrOptions === "object") ? (typeOrOptions.receiveBlockList ?? null) : null;
    // 自定义 DNS（custom-lookup 套件）：非函数即同步 ARG_TYPE。
    if (typeOrOptions && typeof typeOrOptions === "object" && typeOrOptions.lookup !== undefined) {
      if (typeof typeOrOptions.lookup !== "function") {
        const e = new TypeError(`The "lookup" argument must be of type function. Received ${__dgramReceived(typeOrOptions.lookup)}`);
        e.code = "ERR_INVALID_ARG_TYPE"; throw e;
      }
      this.__lookup = typeOrOptions.lookup;
    } else {
      this.__lookup = null;
    }
    this.__sendSeq = 0;
    this.__sendCbs = new Map();
    this.__sendTargets = new Map();
    // signal 面（close-signal 套件）：非法即同步 ARG_TYPE；abort 即关；
    // 预 abort 即微任务关（close 事件仍异步到）。
    if (typeOrOptions && typeof typeOrOptions === "object" && typeOrOptions.signal !== undefined) {
      const sig = typeOrOptions.signal;
      if (!sig || typeof sig !== "object" || typeof sig.aborted !== "boolean" || typeof sig.addEventListener !== "function") {
        const e = new TypeError(`The "options.signal" property must be of type AbortSignal. Received ${String(sig)}`);
        e.code = "ERR_INVALID_ARG_TYPE";
        throw e;
      }
      if (sig.aborted) {
        queueMicrotask(() => this.close());
      } else {
        sig.addEventListener("abort", () => this.close(), { once: true });
      }
    }
    if (typeof cb === "function") this.on("message", cb);
    // 派发钩子预绑定（dispatch 以 global 为 this 调用，§4.34 坑一）
    this.__ev = this.__ev.bind(this);
  }
  bind(...args) {
    // node 口径（test-dgram-bind）：已绑（含绑定窗口）再 bind 同步抛
    // ERR_SOCKET_ALREADY_BOUND "Socket is already bound"；成功返回 this。
    // 失败后重绑（bind-error-repeat）：上一代 task 已死，清旧旗旧柄再起
    //（含 __binding 窗口旗，否则失败后重绑被窗口门误拦报 ALREADY_BOUND）。
    if (this.__bindFailed) {
      this.__bindFailed = false;
      this.__binding = false;
      this.__closed = false;
      this.__id = 0;
    }
    if (this.__id || this.__bound || this.__binding) {
      throw __netErr("ERR_SOCKET_ALREADY_BOUND", "Socket is already bound");
    }
    let port = 0, address = null, cb = null;
    if (typeof args[0] === "object" && args[0] !== null) {
      port = args[0].port ?? 0;
      address = args[0].address ?? null;
      cb = typeof args[1] === "function" ? args[1] : null;
    } else {
      // bind([port][, address][, cb])——缺省位依次前移
      let i = 0;
      if (typeof args[i] === "number") { port = args[i]; i++; }
      if (typeof args[i] === "string") { address = args[i]; i++; }
      if (typeof args[i] === "function") cb = args[i];
    }
    if (cb) this.once("listening", cb);
    this.__binding = true;
    // 地址解析（真机 handle.lookup 口径：自定义 lookup 必经，默认走同步族匹配；
    // 通配符在自定义 lookup 下同样过一遍，custom-lookup 套件点名）。
    const rawAddr = address === null ? (this.type === "udp6" ? "::" : "0.0.0.0") : String(address);
    const finishBind = (err, ip) => {
      // 关后即弃（真机 handle 置空同口径），并复位窗口旗以免卡死重绑。
      if (this.__closed) { this.__binding = false; return; }
      if (err) {
        this.__binding = false;
        this.__evError(err);
        return;
      }
      // 族匹配解析（node 口径：udp4 socket bind('localhost') 落 127.0.0.1——tokio
      // 直接 bind('localhost') 会挑 ::1，family 错乱即后续 send EINVAL）。
      this.__addr = this.__resolveAddr(ip, false);
      // 用户原文地址（bind 错误形 e.address 用；归一化串在字面量下同值）。
      this.__bindAddr = address === null ? null : String(address);
      this.__id = Number(__wjs_dgram_bind(Number(port), this.__addr, this, this.__bindFlags()));
    };
    if (this.__lookup) {
      try {
        this.__lookup.call(this, rawAddr, this.type === "udp4" ? 4 : 6, (e, ip) => {
          if (e) finishBind(e);
          else finishBind(null, ip);
        });
      } catch (e) { finishBind(e); }
    } else if (this.__isNumericIP(rawAddr)) {
      finishBind(null, rawAddr);
    } else {
      // 默认经 JS dns.lookup（custom-lookup 第二块：全局 mock 必经；
      // Node handle.lookup 异步口径；数字 IP 上已直通免一跳）。
      let dnsLookup = null;
      try { dnsLookup = __dnsDefault.lookup; } catch {}
      if (typeof dnsLookup !== "function") {
        finishBind(null, rawAddr);
      } else {
        try {
          dnsLookup.call(this, rawAddr, this.type === "udp4" ? 4 : 6, (e, ip) => {
            if (e) finishBind(e);
            else finishBind(null, ip);
          });
        } catch (e) { finishBind(e); }
      }
    }
    return this;
  }
  // 绑定预选项位（bit0 reusePort/bit1 ipv6Only；随 bind 进内核）。
  __bindFlags() {
    const o = this.__opts;
    return ((o && o.reusePort ? 1 : 0) | (o && o.ipv6Only ? 2 : 0));
  }
  // 数字 IP 判定（bindSync/connectSync 不做 DNS；v6 允许 %scope 后缀）。
  __isNumericIP(s) {
    if (typeof s !== "string") return false;
    if (__parseIPv4(s)) return true;
    return __parseIPv6(s.split("%")[0]);
  }
  bindSync(...args) {
    // 校验先于状态门（已绑 socket 传非法参仍报参数错，bind-sync 套件点名）；
    // 校验失败不落状态、可重调。
    let port = 0, address = null;
    if (args[0] !== undefined) {
      const o = args[0];
      if (!o || typeof o !== "object" || Array.isArray(o)) {
        const e = new TypeError(`The "options" argument must be of type object. Received ${__dgramReceived(o)}`);
        e.code = "ERR_INVALID_ARG_TYPE"; throw e;
      }
      port = o.port ?? 0;
      address = o.address ?? null;
    }
    if (address !== null && typeof address !== "string") {
      const e = new TypeError(`The "address" argument must be of type string. Received ${__dgramReceived(address)}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    // bind 端口容 0（临时端口；-1 照旧 BAD_PORT）。
    validatePort(port, 'Port', true);
    if (address !== null && !this.__isNumericIP(address)) {
      const e = new TypeError(`The argument 'address' must be a numeric IP address. Received '${address}'`);
      e.code = "ERR_INVALID_ARG_VALUE"; throw e;
    }
    // 已绑（含绑定窗口）再绑即 ALREADY_BOUND。
    if (this.__id || this.__bound) {
      throw __netErr("ERR_SOCKET_ALREADY_BOUND", "Socket is already bound");
    }
    const bindAddr = address ?? (this.type === "udp6" ? "::" : "0.0.0.0");
    const res = JSON.parse(__wjs_dgram_bind_sync(Number(port), bindAddr, "", this, this.__bindFlags()));
    if (res.error) {
      const e = new Error(`${res.error}: ${bindAddr}`);
      e.code = res.error;
      e.syscall = 'bind';
      throw e;
    }
    this.__id = res.id;
    this.__bound = true;
    this.__binding = false;
    this.__closed = false;
    this.__rinfo = { address: res.addr, port: res.port };
    // 挂起队列留待 task listening 事件刷出（单刷，不与本函数双发）；
    // 'listening' 同样由该事件派发（仍异步；close 抢先即被 __closed 门吞）。
    return { address: res.addr, family: res.addr.includes(":") ? "IPv6" : "IPv4", port: res.port };
  }
  connectSync(port, address) {
    validatePort(port, 'Port', false);
    if (this.__connected || this.__connecting) {
      throw __netErr('ERR_SOCKET_DGRAM_IS_CONNECTED', 'Already connected');
    }
    if (address === undefined || address === null) {
      address = this.type === "udp4" ? "127.0.0.1" : "::1";
    }
    if (typeof address !== "string") {
      const e = new TypeError(`The "address" argument must be of type string. Received ${__dgramReceived(address)}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    if (!this.__isNumericIP(address)) {
      const e = new TypeError(`The argument 'address' must be a numeric IP address. Received '${address}'`);
      e.code = "ERR_INVALID_ARG_VALUE"; throw e;
    }
    if (this.__blockList) {
      const fam = address.includes(":") ? "ipv6" : "ipv4";
      let blocked = false;
      try { blocked = this.__blockList.check(address, fam); } catch {}
      if (blocked) {
        const e = new Error(`connect ${address} blocked`);
        e.code = "ERR_IP_BLOCKED"; throw e;
      }
    }
    if (!this.__id && !this.__bound) this.bindSync();
    else if (!this.__bound) throw __netErr("ERR_SOCKET_ALREADY_BOUND", "Socket is already bound");
    const family = address.includes(":") ? "IPv6" : "IPv4";
    this.__remote = { address, port, family };
    this.__connecting = false;
    this.__connected = true;
    __wjs_dgram_sockopt(this.__id, JSON.stringify({ op: 'connect', addr: `${address}:${port}` }));
    // 'connect' 仍递延（先关即抑制，与 bindSync listening 同口径）。
    queueMicrotask(() => { if (!this.__closed) this.emit("connect"); });
  }
  send(...args) {
    // 尾函数恒为回调（各形态通用；Node 位置舞步的等价形）。
    let cb = null;
    if (typeof args[args.length - 1] === "function") cb = args.pop();
    const msg = args[0];
    if (this.__connected) {
      // 已连接：(msg[, offset, length])——port/address 禁止（IS_CONNECTED）。
      let off = 0, len;
      if (args.length >= 2) {
        if (typeof args[1] === "function") { /* (msg, cb)——cb 已摘 */ }
        else if (args.length === 2) { /* (msg, 误放回调)——Node 忽略 */ }
        else { off = args[1] ?? 0; len = args[2]; }
      }
      if ((args[3] !== undefined && args[3] !== null) || (args[4] !== undefined && args[4] !== null)) {
        throw __netErr('ERR_SOCKET_DGRAM_IS_CONNECTED', 'Already connected');
      }
      const u8 = __toU8(msg);
      const body = (off || len !== undefined) ? u8.subarray(off, len === undefined ? u8.length : off + len) : u8;
      if (!this.__bound) {
        this.__pending.push({ __send: true, msg: body, port: undefined, address: undefined, cb });
        if (!this.__id && !this.__binding) this.bind();
        return this;
      }
      return this.__doSend(body, undefined, undefined, cb);
    }
    // 未连接：位置 3/4 判形（Node 原文规则）——有 port/address 即
    // (msg, offset, length, port, address)，否则 (msg, port, address)。
    let off, len, port, address;
    // Node 原文真值规则：address(pos4) 真，或 port(pos3) 真且非函数 → 五元形。
    if (args[4] || (args[3] && typeof args[3] !== "function")) {
      off = args[1]; len = args[2]; port = args[3]; address = args[4];
    } else {
      port = args[1]; address = args[2];
    }
    if (typeof address === "function") address = undefined;
    let body = __toU8(msg);
    if (off !== undefined || len !== undefined) {
      const o = off ?? 0, l = len === undefined ? body.length - o : len;
      body = body.subarray(o, o + l);
    }
    if (address !== undefined && address !== null && typeof address !== "string") {
      const e = new TypeError(`The "address" argument must be of type string. Received ${typeof address} (${String(address)})`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    // node 口径：无目标且未 connect → validatePort(undefined) 先炸（真机 26
    // 实测 ERR_SOCKET_BAD_PORT）；隐式绑定只发生在"有目标"的 send 上。
    if (port === undefined && address === undefined && !this.__connected) {
      validatePort(port, 'Port', false);
    }
    if (!this.__bound) {
      // node/libuv 口径：未绑 socket send 即隐式 bind（port 0），send 参数挂起
      // 到 listening 刷出（真机：cb 后 address().port > 0）。绑定窗口内（__id
      // 已有）直接挂起不重复 bind。
      this.__pending.push({ __send: true, msg: body, port, address, cb });
      if (!this.__id && !this.__binding) this.bind();
      return this;
    }
    return this.__doSend(body, port, address, cb);
  }
  // legacy sendto（严格六元形；校验逐字，sendto 套件点名）。
  sendto(buffer, offset, length, port, address, callback) {
    const needNum = (name, v) => {
      if (typeof v !== "number") {
        const e = new TypeError(`The "${name}" argument must be of type number. Received ${__dgramReceived(v)}`);
        e.code = "ERR_INVALID_ARG_TYPE"; throw e;
      }
    };
    needNum("offset", offset);
    needNum("length", length);
    needNum("port", port);
    if (typeof address !== "string") {
      const e = new TypeError(`The "address" argument must be of type string. Received ${__dgramReceived(address)}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    return this.send(buffer, offset, length, port, address, callback);
  }
  __doSend(msg, port, address, cb) {
    let target;
    if (port === undefined && address === undefined) {
      // connect 后的无地址发送走默认远端；未 connect 即端口校验错（真机口径）。
      if (!this.__connected) validatePort(port, 'Port', false);
      target = "";
    } else {
      validatePort(port, 'Port', false);
      target = `${this.__resolveAddr(address ?? 'localhost')}:${port}`;
    }
    // 发送黑名单（blocklist 套件：错经回调/事件异步到，不抛同步）。
    {
      const tip = target === "" ? this.__remote?.address : String(address ?? 'localhost');
      let dip = String(tip ?? "");
      try {
        const entries = JSON.parse(__wjs_dns_lookup(dip));
        if (Array.isArray(entries) && entries.length) dip = entries[0].address;
      } catch {}
      const clean = dip.startsWith("[") && dip.endsWith("]") ? dip.slice(1, -1) : dip;
      if (this.__blockList && typeof this.__blockList.check === "function") {
        let blocked = false;
        try { blocked = this.__blockList.check(clean, clean.includes(":") ? "ipv6" : "ipv4"); } catch {}
        if (blocked) {
          const err = __netErr('ERR_IP_BLOCKED', `send ${clean} blocked`);
          if (cb) queueMicrotask(() => cb(err));
          else this.__evError(err);
          return this;
        }
      }
    }
    const u8 = __toU8(msg);
    // send 失败按 seq 路由回本回调（无回调才走 error 事件，node 口径）；
    // 目标随 seq 记录（senderr 的 e.address/e.port 回填）。
    const seq = ++this.__sendSeq;
    this.__sendTargets.set(seq, { address: address ?? this.__remote?.address, port: port ?? this.__remote?.port });
    __wjs_dgram_send(this.__id, u8, target, seq);
    if (cb) this.__sendCbs.set(seq, cb);
    return this;
  }
  // fire-and-forget sockopt（失败走 Error 事件）。未 bind（__id 未分配）即
  // 同步 EBADF（真机口径 `setMulticastLoopback EBADF`；静默挂起是偏差——
  // 绑定窗口内 __id 已有，照常发 task）。文案用 JS 侧方法名。
  __sockopt(op) {
    this.__healthCheck();
    if (!this.__id) {
      const name = { setBroadcast: 'setBroadcast', setTtl: 'setTTL', setMulticastTtl: 'setMulticastTTL', setMulticastLoop: 'setMulticastLoopback', join: 'addMembership', leave: 'dropMembership', connect: 'connect', disconnect: 'disconnect' }[op.op] ?? op.op;
      throw __sysErr(name, 'EBADF');
    }
    __wjs_dgram_sockopt(this.__id, JSON.stringify(op));
  }
  connect(...args) {
    let port, address = 'localhost', cb = null;
    if (args.length === 1 && typeof args[0] === 'object' && args[0] !== null) {
      port = args[0].port;
      if (args[0].address !== undefined) address = args[0].address;
    } else {
      port = args[0];
      if (args[1] !== undefined) {
        if (typeof args[1] === 'function') cb = args[1];
        else address = args[1];
      }
      if (typeof args[2] === 'function') cb = args[2];
    }
    validatePort(port, 'Port', false);
    // node 口径（lib/dgram.js）：非 DISCONNECTED 即 IS_CONNECTED（含 CONNECTING 窗口）。
    if (this.__connected || this.__connecting) {
      throw __netErr('ERR_SOCKET_DGRAM_IS_CONNECTED', 'Already connected');
    }
    // 地址解析后即查发送黑名单（blocklist 套件：错经回调/事件异步到，不抛同步）。
    let dispAddr = String(address);
    let family = dispAddr.includes(':') ? 'IPv6' : 'IPv4';
    try {
      const entries = JSON.parse(__wjs_dns_lookup(dispAddr));
      if (Array.isArray(entries) && entries.length) {
        dispAddr = entries[0].address;
        family = entries[0].family === 6 ? 'IPv6' : 'IPv4';
      }
    } catch { /* keep verbatim */ }
    if (this.__blockList && typeof this.__blockList.check === "function") {
      let blocked = false;
      try { blocked = this.__blockList.check(dispAddr, family === 'IPv6' ? 'ipv6' : 'ipv4'); } catch {}
      if (blocked) {
        const err = __netErr('ERR_IP_BLOCKED', `connect ${dispAddr} blocked`);
        if (cb) queueMicrotask(() => cb(err));
        else this.__evError(err);
        return;
      }
    }
    this.__connecting = true;
    if (cb) this.once('connect', cb);
    if (!this.__id && !this.__binding) this.bind();
    // 展示用远端（发送时 task 再解，见模块头注；解析已在黑名单检查前完成）。
    this.__remote = { address: dispAddr, port, family };
    // 窗口期内（lookup 未归）排队，listening 刷出；不直调（__id 未落）。
    if (this.__id) __wjs_dgram_sockopt(this.__id, JSON.stringify({ op: 'connect', addr: `${this.__resolveAddr(String(address))}:${port}` }));
    else this.__pending.push({ op: 'connect', addr: `${this.__resolveAddr(String(address))}:${port}` });
  }
  disconnect() {
    if (!this.__connected) {
      throw __netErr('ERR_SOCKET_DGRAM_NOT_CONNECTED', 'Not connected');
    }
    this.__connected = false;
    this.__connecting = false;
    this.__remote = null;
    if (this.__id) __wjs_dgram_sockopt(this.__id, JSON.stringify({ op: 'disconnect' }));
  }
  remoteAddress() {
    if (!this.__connected || !this.__remote) {
      throw __netErr('ERR_SOCKET_DGRAM_NOT_CONNECTED', 'Not connected');
    }
    return { ...this.__remote };
  }
  // close 后调用即 NOT_RUNNING（membership 套件点名；close 本体幂等静默系既有记档）。
  __healthCheck() {
    if (this.__closed) throw __netErr('ERR_SOCKET_DGRAM_NOT_RUNNING', 'Not running');
  }
  addMembership(multicastAddress, multicastInterface) {
    this.__healthCheck();
    const { multi, iface } = __membershipAddrs(multicastAddress, multicastInterface, this.type, 'addMembership');
    if (!this.__id && !this.__binding) this.bind();
    if (this.__id) __wjs_dgram_sockopt(this.__id, JSON.stringify({ op: 'join', multi, iface }));
    else this.__pending.push({ op: 'join', multi, iface });
  }
  dropMembership(multicastAddress, multicastInterface) {
    this.__healthCheck();
    const { multi, iface } = __membershipAddrs(multicastAddress, multicastInterface, this.type, 'dropMembership');
    if (!this.__id && !this.__binding) this.bind();
    if (this.__id) __wjs_dgram_sockopt(this.__id, JSON.stringify({ op: 'leave', multi, iface }));
    else this.__pending.push({ op: 'leave', multi, iface });
  }
  // SSM 入组/退组（membership 套件点名校验；成功路径走 tasksetsockopt）。
  __ssmAddrs(sourceAddress, groupAddress, syscall) {
    if (typeof sourceAddress !== "string") {
      const e = new TypeError(`The "sourceAddress" argument must be of type string. Received ${__dgramReceived(sourceAddress)}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    if (typeof groupAddress !== "string") {
      const e = new TypeError(`The "groupAddress" argument must be of type string. Received ${__dgramReceived(groupAddress)}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    // 组须为本族组播地址（'0' 等非组播即 EINVAL，真机口径）。
    const v4 = __parseIPv4(groupAddress);
    let v6 = false;
    if (v4) {
      if (v4[0] < 224 || v4[0] > 239) throw __sysErr(syscall, 'EINVAL');
    } else if (groupAddress.includes(':')) {
      v6 = true;
      if (!/^ff/i.test(groupAddress)) throw __sysErr(syscall, 'EINVAL');
    } else {
      throw __sysErr(syscall, 'EINVAL');
    }
    if (this.type === 'udp4' && v6) throw __sysErr(syscall, 'EINVAL');
    if (this.type === 'udp6' && !v6) throw __sysErr(syscall, 'EINVAL');
    return { source: sourceAddress, group: groupAddress, v6 };
  }
  addSourceSpecificMembership(sourceAddress, groupAddress, interfaceAddress) {
    this.__healthCheck();
    const { source, group, v6 } = this.__ssmAddrs(sourceAddress, groupAddress, 'addSourceSpecificMembership');
    if (!this.__id && !this.__binding) this.bind();
    if (this.__id) __wjs_dgram_sockopt(this.__id, JSON.stringify({ op: 'joinSource', source, group, iface: interfaceAddress ?? (v6 ? '0' : '0.0.0.0') }));
    else this.__pending.push({ op: 'joinSource', source, group, iface: interfaceAddress ?? (v6 ? '0' : '0.0.0.0') });
  }
  dropSourceSpecificMembership(sourceAddress, groupAddress, interfaceAddress) {
    this.__healthCheck();
    const { source, group, v6 } = this.__ssmAddrs(sourceAddress, groupAddress, 'dropSourceSpecificMembership');
    if (!this.__id && !this.__binding) this.bind();
    if (this.__id) __wjs_dgram_sockopt(this.__id, JSON.stringify({ op: 'leaveSource', source, group, iface: interfaceAddress ?? (v6 ? '0' : '0.0.0.0') }));
    else this.__pending.push({ op: 'leaveSource', source, group, iface: interfaceAddress ?? (v6 ? '0' : '0.0.0.0') });
  }
  setMulticastInterface(interfaceAddress) {
    this.__healthCheck();
    if (typeof interfaceAddress !== "string") {
      const e = new TypeError(`The "interfaceAddress" argument must be of type string. Received ${__dgramReceived(interfaceAddress)}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    // 族错配同步 EINVAL（udp4 配 v6 形；真机平台三态之一，此处取确定态）。
    // 其余非法形（''/'undefined'/非组播）同样 EINVAL；合法交 task。
    const s = interfaceAddress;
    const v6 = s.includes(':');
    if (this.type === 'udp4' && v6) throw __sysErr('setMulticastInterface', 'EINVAL');
    if (this.type === 'udp4') {
      const v = __parseIPv4(s);
      if (!v || v[0] >= 224) throw __sysErr('setMulticastInterface', 'EINVAL');
    } else {
      const bare = s.split('%')[0];
      if (bare === '' || !__parseIPv6(bare)) throw __sysErr('setMulticastInterface', 'EINVAL');
    }
    this.__sockopt({ op: 'multicastInterface', addr: s });
  }
  // buffer size 四方法（node 口径：未绑即 ERR_SOCKET_BUFFER_SIZE，文案
  // "Could not get or set buffer size: uv_recv/send_buffer_size returned
  // EBADF (bad file descriptor)" 逐字；绑后 get/setsockopt 同步直调）。
  __bufSizeErr(kind) {
    return __netErr("ERR_SOCKET_BUFFER_SIZE", `Could not get or set buffer size: uv_${kind}_buffer_size returned EBADF (bad file descriptor)`);
  }
  __bufSize(kind, size) {
    if (!this.__bound) throw this.__bufSizeErr(kind);
    const v = __wjs_dgram_bufsize(this.__id, kind, size);
    if (v === "") throw this.__bufSizeErr(kind);
    return Number(v);
  }
  getRecvBufferSize() { return this.__bufSize("recv"); }
  setRecvBufferSize(size) { this.__bufSize("recv", size); }
  getSendBufferSize() { return this.__bufSize("send"); }
  setSendBufferSize(size) { this.__bufSize("send", size); }
  setBroadcast(flag) {
    this.__sockopt({ op: 'setBroadcast', v: Boolean(flag) });
  }
  setTTL(ttl) {
    __dgramTtl('setTTL', ttl, 1, 255);
    this.__sockopt({ op: 'setTtl', v: ttl });
    return ttl;
  }
  setMulticastTTL(v) {
    __dgramTtl('setMulticastTTL', v, 0, 255);
    this.__sockopt({ op: 'setMulticastTtl', v });
    return v;
  }
  setMulticastLoopback(flag) {
    // 真机回显原值（16→16，非 Boolean 归一；loopback 套件点名）。
    this.__sockopt({ op: 'setMulticastLoop', v: Boolean(flag) });
    return flag;
  }
  // 事件循环派发钩子（Rust dispatch 以 global 为 this 调用，须预绑定）
  __ev(kind, payload) {
    switch (kind) {
      case "listening": {
        // close 后到的迟到事件丢弃（close-is-not-callback：send 即关，
        // 绑定完成事件后到，此时挂起队列早随 task 消亡，刷出即对空表抛错）。
        if (this.__closed) break;
        this.__binding = false;
        const o = JSON.parse(payload);
        this.__bound = true;
        this.__rinfo = { address: o.addr, port: o.port };
        for (const p of this.__pending) {
          if (p.__send) this.__doSend(p.msg, p.port, p.address, p.cb);
          else __wjs_dgram_sockopt(this.__id, JSON.stringify(p));
        }
        this.__pending = [];
        // 构造选项 buffer sizes（node：选项在 handle 创建期生效；此处绑后即设，
        // macOS getsockopt 精确回读，Linux 回读翻倍记平台差）。
        if (this.__opts) {
          if (this.__opts.recvBufferSize !== undefined) __wjs_dgram_bufsize(this.__id, "recv", this.__opts.recvBufferSize);
          if (this.__opts.sendBufferSize !== undefined) __wjs_dgram_bufsize(this.__id, "send", this.__opts.sendBufferSize);
        }
        this.emit("listening");
        break;
      }
      case "connect": {
        if (this.__closed) break;
        this.__connecting = false;
        this.__connected = true;
        this.emit("connect");
        break;
      }
      case "message": {
        const o = JSON.parse(payload);
        // 接收黑名单：命中即静默丢弃（blocklist 套件；check 副作用由调用方承载）。
        if (this.__recvBlockList && typeof this.__recvBlockList.check === "function") {
          let blocked = false;
          try { blocked = this.__recvBlockList.check(o.address, o.family === 6 ? "ipv6" : "ipv4"); } catch {}
          if (blocked) break;
        }
        const msg = Buffer.from(__b64dec(o.data));
        const rinfo = { address: o.address, port: o.port, family: o.family, size: msg.length };
        this.emit("message", msg, rinfo);
        break;
      }
      case "error": {
        const o = JSON.parse(payload);
        this.__evErrorBind(o.code, o.msg);
        break;
      }
      case "sendok": {
        // send 完成：回调 (null, bytes) 异步触发（§4.74——同步完成也走微任务）。
        const o = JSON.parse(payload);
        const cb = this.__sendCbs.get(o.seq);
        if (cb) {
          this.__sendCbs.delete(o.seq);
          queueMicrotask(() => cb(null, o.bytes));
        }
        this.__sendTargets.delete(o.seq);
        break;
      }
      case "senderr": {
        // send 失败：有回调走回调（msgsize 套件 EMSGSIZE 形），无回调走 error 事件
        //（node 口径）；e.address/e.port 由 JS 侧按 seq 记录的发送目标回填。
        const o = JSON.parse(payload);
        const rec = this.__sendTargets.get(o.seq);
        const e = __netErr(o.code, o.msg);
        if (rec) {
          e.address = rec.address;
          e.port = rec.port;
        }
        const cb = this.__sendCbs.get(o.seq);
        if (cb) {
          this.__sendCbs.delete(o.seq);
          queueMicrotask(() => cb(e));
        } else {
          this.__evError(e);
        }
        this.__sendTargets.delete(o.seq);
        break;
      }
      case "close": {
        // 陈旧代 Close（失败后已重绑）：跳过状态改写与派发，不吞新代；
        // purge 照旧由分发侧按 id 做（见 net dispatch）。
        if (this.__binding && !this.__bindFailed) break;
        this.__bound = false;
        this.__binding = false;
        this.__bindFailed = false;
        this.__connected = false;
        this.__connecting = false;
        this.__closed = true;
        this.__remote = null;
        this.emit("close");
        break;
      }
    }
  }
  // 目标地址归一：主机名经 DNS 取本 socket 族匹配的地址（node 口径——udp4 socket
  // 发 'localhost' 落 127.0.0.1，macOS localhost 首选 ::1，不归一即 EINVAL）；
  // v6 字面量加方括号（tokio ToSocketAddrs 需 "[::1]:port" 形）。
  __resolveAddr(addr, bracket = true) {
    let s = String(addr);
    try {
      const entries = JSON.parse(__wjs_dns_lookup(s));
      if (Array.isArray(entries) && entries.length) {
        const fam = this.type === "udp4" ? 4 : 6;
        const hit = entries.find((e) => e.family === fam) ?? entries[0];
        s = hit.address;
      }
    } catch { /* 字面量/解析失败保留原文 */ }
    // bind 路径禁括号：tokio ("[::1]", port) 元组经 ToSocketAddrs 走 DNS 而非
    // 字面量，即 EADDRNOTAVAIL；裸 "::"/"::1" 才直解（address 套件现形）。
    // send/connect 目标串仍要括号（"ip:port" 拼接口径）。
    if (!bracket) return s;
    return s.includes(":") && !s.startsWith("[") ? `[${s}]` : s;
  }
  address() {
    // node 口径（test-dgram-address 末块）：未绑 address() 即
    // `Error EBADF "getsockname EBADF"`（uv getsockname 直透，非 NOT_RUNNING）；
    // 已关闭（曾绑）即 NOT_RUNNING（async-dispose 套件点名）。
    if (this.__closed) throw __netErr('ERR_SOCKET_DGRAM_NOT_RUNNING', 'Not running');
    if (!this.__bound) {
      const e = __netErr("EBADF", "getsockname EBADF");
      throw e;
    }
    return { address: this.__rinfo.address, port: this.__rinfo.port, family: String(this.__rinfo.address).includes(":") ? "IPv6" : "IPv4" };
  }
  // error 事件无监听时经 nextTick 抛（运行时 uncaughtException 探针收敛；
  // 分发内直抛绕过监听走 fatal，bind-error-callback 套件点名）。
  __evError(e) {
    if (this.listenerCount("error") > 0) { this.emit("error", e); return; }
    process.nextTick(() => { throw e; });
  }
  // 绑定期错误整形（ExceptionWithHostPort 口径 `bind CODE addr` + address/port
  // 属性，error-message-address 套件逐字点名；非绑定期错误原样）。
  // 失败旗为重绑留门（bind-error-repeat）。
  __evErrorBind(code, msg) {
    if (this.__binding) {
      this.__bindFailed = true;
      const e = new Error(`bind ${code} ${this.__bindAddr ?? this.__addr}`);
      e.code = code;
      e.syscall = 'bind';
      e.address = this.__bindAddr ?? this.__addr;
      e.port = undefined;
      this.__evError(e);
      return;
    }
    this.__evError(__netErr(code, msg));
  }
  // 显式处置（async-dispose 套件）：关后决议；重复处置照决议（幂等）。
  async [Symbol.asyncDispose]() {
    this.close();
  }
  [Symbol.dispose]() {
    this.close();
  }
  close(cb) {
    if (typeof cb === "function") this.once("close", cb);
    // 同步落关闭旗（后继 addMembership/connect 等健康检查即时生效，不等 task）。
    this.__closed = true;
    this.__binding = false;
    // 从未绑定（无 task）即微任务派 close（真机未绑 close 仍异步派发；
    // 有 task 走 task Close 事件独派，不双发）。
    if (!this.__id) queueMicrotask(() => this.emit("close"));
    else __wjs_net_destroy(this.__id);
    return this;
  }
  ref() {
    if (this.__id) __wjs_net_ref(this.__id);
    return this;
  }
  unref() {
    if (this.__id) __wjs_net_unref(this.__id);
    return this;
  }
}

export function createSocket(options, cb) {
  return new Socket(options, cb);
}
export { Socket };
const __api = { Socket, createSocket };
export default __api;
"#;
