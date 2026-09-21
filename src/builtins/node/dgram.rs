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
/// flags 位：1 = reusePort，2 = ipv6Only（仅 v6 有意义），4 = reuseAddr
/// （SO_REUSEADDR；macOS 双绑另需 SO_REUSEPORT，libuv 同款双落）。
fn std_bind_flags(addr: &str, port: u16, flags: u32) -> std::io::Result<std::net::UdpSocket> {
    use std::os::fd::{FromRawFd, OwnedFd};
    let raw = addr.strip_prefix('[').and_then(|s| s.strip_suffix(']')).unwrap_or(addr);
    let is_v6 = raw.contains(':');
    let reuse_port = flags & 1 != 0;
    let v6only = flags & 2 != 0;
    let reuse_addr = flags & 4 != 0;
    if !reuse_port && !v6only && !reuse_addr {
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
        if reuse_addr
            && libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_REUSEADDR,
                &one as *const _ as *const libc::c_void,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            ) != 0
        {
            return Err(std::io::Error::last_os_error());
        }
        if (reuse_port || reuse_addr)
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
    // 预选项（bit0 reusePort/bit1 ipv6Only/bit2 reuseAddr）：有则走 std 预置后 from_std，
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

/// 内嵌 ESM 源（`node:dgram`；§0.9 按域分块：`dgram.js` 全量，concat 字节恒等）。
pub const SOURCE: &str = include_str!("dgram.js");
