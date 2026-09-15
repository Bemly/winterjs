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
    let Some((id, ev_tx)) = state::net_alloc() else {
        report_error(&mut cx, "OperationError: net driver not installed");
        return false;
    };
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        report_error(&mut cx, "OperationError: no async runtime for dgram bind");
        return false;
    };
    let mut cmd_rx = state::net_socket_add(id, target);
    set_rval_str(&mut cx, &frame, &id.to_string());
    handle.spawn(async move {
        let bound = tokio::net::UdpSocket::bind((address.as_str(), port as u16)).await;
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
        let local = sock
            .local_addr()
            .unwrap_or_else(|_| "0.0.0.0:0".parse::<std::net::SocketAddr>().expect("literal addr"));
        let _ = ev_tx.send(NetEvent {
            id,
            kind: NetKind::DgramListening { addr: local.ip().to_string(), port: local.port() },
        });
        let sock = std::sync::Arc::new(sock);
        let is_v4 = local.is_ipv4();
        // task 级 connect 的默认远端（无内核过滤，记档；disconnect 清除）。
        let mut default_remote: Option<String> = None;
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
                        Some(NetCmd::SendTo { data, addr }) => {
                            let target = if addr.is_empty() { default_remote.clone() } else { Some(addr) };
                            let Some(target) = target else {
                                let _ = ev_tx.send(NetEvent {
                                    id,
                                    kind: NetKind::Error {
                                        code: "ERR_SOCKET_DGRAM_NOT_CONNECTED".into(),
                                        msg: "Not connected".into(),
                                    },
                                });
                                continue;
                            };
                            if rsock.send_to(&data, target.as_str()).await.is_err() {
                                break;
                            }
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
        if state::net_close_once(id) {
            // purge 由 dispatch 派发 Close 之后统一做（§4.34 症状三：先清后派发即丢事件）
            let _ = ev_tx.send(NetEvent { id, kind: NetKind::Close });
        }
    });
    true
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
    if !state::net_cmd(id as u64, NetCmd::SendTo { data, addr }) {
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

/// 内嵌 ESM 源（`node:dgram`；net 底座）。
pub const SOURCE: &str = r#"
import { EventEmitter } from "node:events";
import errors from 'node:internal/errors';
import { validatePort } from 'node:internal/validators';
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
    this.__addr = null;
    this.__connected = false;
    this.__remote = null;
    this.__pending = [];
    if (typeof cb === "function") this.on("message", cb);
    // 派发钩子预绑定（dispatch 以 global 为 this 调用，§4.34 坑一）
    this.__ev = this.__ev.bind(this);
  }
  bind(...args) {
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
    this.__addr = address === null ? (this.type === "udp6" ? "::" : "0.0.0.0") : address;
    this.__id = Number(__wjs_dgram_bind(Number(port), this.__addr, this));
    return this;
  }
  send(...args) {
    let msg, cb = null, port, address;
    if (typeof args[args.length - 1] === "function") cb = args.pop();
    if (args.length >= 4) {
      // (msg, offset, length, port, address)
      const [m, offset, length, p, a] = args;
      const u8 = __toU8(m);
      msg = u8.subarray(offset ?? 0, (offset ?? 0) + (length ?? u8.length - (offset ?? 0)));
      port = p; address = a;
    } else {
      // (msg, port, address)
      msg = args[0]; port = args[1]; address = args[2];
    }
    if (!this.__bound) throw __netErr("ERR_SOCKET_DGRAM_NOT_RUNNING", "send: socket not bound");
    let target;
    if (port === undefined && address === undefined) {
      // connect 后的无地址发送走默认远端；未 connect 即端口校验错（真机口径）。
      if (!this.__connected) validatePort(port, 'Port', false);
      target = "";
    } else {
      validatePort(port, 'Port', false);
      target = `${address ?? 'localhost'}:${port}`;
    }
    __wjs_dgram_send(this.__id, __toU8(msg), target);
    if (cb) queueMicrotask(cb);
    return this;
  }
  // fire-and-forget sockopt（失败走 Error 事件；未 bind 先挂起，listening 刷出）。
  __sockopt(op) {
    if (!this.__id) {
      this.__pending.push(op);
      return;
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
    if (cb) this.once('connect', cb);
    if (!this.__id) this.bind();
    // 展示用远端（发送时 task 再解，见模块头注）。
    let dispAddr = String(address);
    let family = dispAddr.includes(':') ? 'IPv6' : 'IPv4';
    try {
      const entries = JSON.parse(__wjs_dns_lookup(dispAddr));
      if (Array.isArray(entries) && entries.length) {
        dispAddr = entries[0].address;
        family = entries[0].family === 6 ? 'IPv6' : 'IPv4';
      }
    } catch { /* keep verbatim */ }
    this.__remote = { address: dispAddr, port, family };
    __wjs_dgram_sockopt(this.__id, JSON.stringify({ op: 'connect', addr: `${address}:${port}` }));
  }
  disconnect() {
    if (!this.__connected) {
      throw __netErr('ERR_SOCKET_DGRAM_NOT_CONNECTED', 'Not connected');
    }
    this.__connected = false;
    this.__remote = null;
    if (this.__id) __wjs_dgram_sockopt(this.__id, JSON.stringify({ op: 'disconnect' }));
  }
  remoteAddress() {
    if (!this.__connected || !this.__remote) {
      throw __netErr('ERR_SOCKET_DGRAM_NOT_CONNECTED', 'Not connected');
    }
    return { ...this.__remote };
  }
  addMembership(multicastAddress, multicastInterface) {
    const { multi, iface } = __membershipAddrs(multicastAddress, multicastInterface, this.type, 'addMembership');
    if (!this.__id) this.bind();
    __wjs_dgram_sockopt(this.__id, JSON.stringify({ op: 'join', multi, iface }));
  }
  dropMembership(multicastAddress, multicastInterface) {
    const { multi, iface } = __membershipAddrs(multicastAddress, multicastInterface, this.type, 'dropMembership');
    if (!this.__id) this.bind();
    __wjs_dgram_sockopt(this.__id, JSON.stringify({ op: 'leave', multi, iface }));
  }
  setBroadcast(flag) {
    this.__sockopt({ op: 'setBroadcast', v: Boolean(flag) });
  }
  setTTL(ttl) {
    if (!Number.isInteger(ttl) || ttl < 1 || ttl > 255) throw __sysErr('setTTL', 'EINVAL');
    this.__sockopt({ op: 'setTtl', v: ttl });
    return ttl;
  }
  setMulticastTTL(v) {
    if (!Number.isInteger(v) || v < 0 || v > 255) throw __sysErr('setMulticastTTL', 'EINVAL');
    this.__sockopt({ op: 'setMulticastTtl', v });
    return v;
  }
  setMulticastLoopback(flag) {
    const b = Boolean(flag);
    this.__sockopt({ op: 'setMulticastLoop', v: b });
    return b;
  }
  // 事件循环派发钩子（Rust dispatch 以 global 为 this 调用，须预绑定）
  __ev(kind, payload) {
    switch (kind) {
      case "listening": {
        const o = JSON.parse(payload);
        this.__bound = true;
        this.__rinfo = { address: o.addr, port: o.port };
        for (const op of this.__pending) {
          __wjs_dgram_sockopt(this.__id, JSON.stringify(op));
        }
        this.__pending = [];
        this.emit("listening");
        break;
      }
      case "connect": {
        this.__connected = true;
        this.emit("connect");
        break;
      }
      case "message": {
        const o = JSON.parse(payload);
        const msg = Buffer.from(__b64dec(o.data));
        const rinfo = { address: o.address, port: o.port, family: o.family, size: msg.length };
        this.emit("message", msg, rinfo);
        break;
      }
      case "error": {
        const o = JSON.parse(payload);
        this.emit("error", __netErr(o.code, o.msg));
        break;
      }
      case "close": {
        this.__bound = false;
        this.__connected = false;
        this.__remote = null;
        this.emit("close");
        break;
      }
    }
  }
  address() {
    if (!this.__bound) throw __netErr("ERR_SOCKET_DGRAM_NOT_RUNNING", "address: socket not bound");
    return { address: this.__rinfo.address, port: this.__rinfo.port, family: String(this.__rinfo.address).includes(":") ? "IPv6" : "IPv4" };
  }
  close(cb) {
    if (typeof cb === "function") this.once("close", cb);
    if (this.__id) __wjs_net_destroy(this.__id);
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
