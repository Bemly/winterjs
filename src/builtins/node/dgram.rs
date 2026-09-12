//! `node:dgram`：UDP Socket（tokio net 底座，dependencies2 §9d 口径）。
/// 复用 net 的事件通道/状态机（NetCmd::SendTo + NetKind::Dgram* 变体、
/// net_open 计数、close_once 单发旗、dispatch Close 统一收尾，见 net.rs）。
/// 单 task `select!`（recv_from ↔ 命令通道）；UDP 数据报无连接语义，一次收发
/// 一事件。偏差记档：无 connect/disconnect、组播/广播选项、setTTL 系（子集）。

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
        // 单 task：收数据报 ↔ 命令（SendTo/Close）
        let rsock = sock.clone();
        let mut buf = vec![0u8; 65536];
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
                            if rsock.send_to(&data, addr.as_str()).await.is_err() {
                                break;
                            }
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

/// 内嵌 ESM 源（`node:dgram`；net 底座）。
pub const SOURCE: &str = r#"
import { EventEmitter } from "node:events";
const Buffer = globalThis.Buffer;

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
    this.__addr = address === null ? "0.0.0.0" : address;
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
    __wjs_dgram_send(this.__id, __toU8(msg), `${address}:${port}`);
    if (cb) queueMicrotask(cb);
    return this;
  }
  // 事件循环派发钩子（Rust dispatch 以 global 为 this 调用，须预绑定）
  __ev(kind, payload) {
    switch (kind) {
      case "listening": {
        const o = JSON.parse(payload);
        this.__bound = true;
        this.__rinfo = { address: o.addr, port: o.port };
        this.emit("listening");
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
  ref() { return this; }
  unref() { return this; }
}

export function createSocket(options, cb) {
  return new Socket(options, cb);
}
export { Socket };
const __api = { Socket, createSocket };
export default __api;
"#;
