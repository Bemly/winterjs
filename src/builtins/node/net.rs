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
    /// server 收到连接（conn 侧 entry/泵已就绪，等 JS attach target）。
    Connection {
        conn_id: u64,
        remote_addr: String,
        remote_port: u16,
        local_addr: String,
        local_port: u16,
    },
    ServerError { code: String, msg: String },
    ServerClose,
    /// dgram：绑定完成。
    DgramListening { addr: String, port: u16 },
    /// dgram：收到数据报。
    DgramMessage { data_b64: String, address: String, port: u16, family: u8 },
}

/// socket 命令（写/半关/硬关；写端 task 消费。SendTo 为 dgram 专用）。
pub enum NetCmd {
    Write(Vec<u8>),
    End,
    Close,
    SendTo { data: Vec<u8>, addr: String },
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
                NetCmd::Close => break,
                NetCmd::SendTo { .. } => {} // dgram 专用（net socket 不产生）
            }
        }
        state::net_writer_exit(id);
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
    let host = value_to_string(&mut cx, frame.arg(0));
    let Some(port) = opt_num(&frame, 1) else {
        report_error(&mut cx, "TypeError: connect: port must be a number");
        return false;
    };
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
                let local = stream.local_addr().ok();
                let _ = ev_tx.send(NetEvent { id, kind: NetKind::Connect { local } });
                let (r, w) = stream.into_split();
                spawn_pumps(id, r, w, ev_tx, cmd_rx);
            }
        }
    });
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
    let accept_handle = handle.clone();
    accept_handle.spawn(async move {
        let bound = tokio::net::TcpListener::bind((host.as_str(), port as u16)).await;
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

// ── 事件循环派发 ────────────────────────────────────────────────────────────

/// 网络事件派发：调 target 的 `__ev(kind, payload)`（payload 空串或 JSON）。
/// 前置条件：cx 已进入 global 所属 realm（事件循环上下文）。
pub fn dispatch(
    cx: &mut JSContext,
    global: *mut JSObject,
    ev: NetEvent,
    err: crate::runtime::ErrorSource<'_>,
) -> Result<(), crate::error::Error> {
    use crate::jsapi_glue::call_two;
    let failed = |cx: &mut JSContext| match err {
        crate::runtime::ErrorSource::Script { source, filename } => {
            crate::jsapi_glue::pending_exception_error(cx, global, source, filename)
        }
        crate::runtime::ErrorSource::Module { url } => crate::modules::module_error(cx, url),
    };
    // connection 事件：server target 必须在；conn target 由 JS attach（此处不读）。
    if let NetKind::Connection { conn_id, remote_addr, remote_port, local_addr, local_port } =
        &ev.kind
    {
        let Some(target) = state::net_target(ev.id) else {
            state::net_purge(*conn_id); // server 已 gone：丢弃连接防泄漏
            return Ok(());
        };
        if !target.is_object() {
            state::net_purge(*conn_id);
            return Ok(());
        }
        rooted!(&in(cx) let t: *mut JSObject = target.to_object());
        let Some(fun) = get_prop_value(cx, t.get(), c"__ev") else {
            return Err(failed(cx));
        };
        let json = serde_json::json!({
            "connId": conn_id,
            "remoteAddress": remote_addr, "remotePort": remote_port,
            "localAddress": local_addr, "localPort": local_port,
        })
        .to_string();
        if with_str_args(cx, global, fun, "connection", &json).is_none() {
            return Err(failed(cx));
        }
        return Ok(());
    }
    let Some(target) = state::net_target(ev.id) else {
        if matches!(ev.kind, NetKind::Close | NetKind::ServerClose) {
            state::net_purge(ev.id);
        }
        return Ok(());
    };
    if !target.is_object() {
        if matches!(ev.kind, NetKind::Close | NetKind::ServerClose) {
            state::net_purge(ev.id);
        }
        return Ok(());
    }
    rooted!(&in(cx) let t: *mut JSObject = target.to_object());
    let Some(fun) = get_prop_value(cx, t.get(), c"__ev") else {
        if matches!(ev.kind, NetKind::Close | NetKind::ServerClose) {
            state::net_purge(ev.id);
        }
        return Err(failed(cx));
    };
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
        NetKind::Listening { addr, port } => {
            ("listening", serde_json::json!({ "addr": addr, "port": port }).to_string())
        }
        NetKind::ServerError { code, msg } => {
            ("error", serde_json::json!({ "code": code, "msg": msg }).to_string())
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
        NetKind::Connection { .. } => unreachable!(),
    };
    let ok = with_str_args(cx, global, fun, kind, &payload);
    let closed = matches!(ev.kind, NetKind::Close | NetKind::ServerClose);
    if closed {
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
    rooted!(&in(cx) let mut a = UndefinedValue());
    rooted!(&in(cx) let mut b = UndefinedValue());
    kind.to_jsval(cx, a.handle_mut());
    payload.to_jsval(cx, b.handle_mut());
    crate::jsapi_glue::call_two(cx, global, fun, a.get(), b.get())
}

/// 内嵌 ESM 源（`node:net`；Socket/Server 建立在 node:events 之上）。
pub const SOURCE: &str = r#"
import { EventEmitter } from "node:events";
const Buffer = globalThis.Buffer;

function __b64dec(s) {
  const bin = atob(s);
  const u8 = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) u8[i] = bin.charCodeAt(i);
  return u8;
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
    this.__id = 0;
    this.__enc = null;
    this.remoteAddress = null;
    this.remotePort = null;
    this.localAddress = null;
    this.localPort = null;
    this.readable = false;
    this.writable = false;
    this.destroyed = false;
    // Node 默认 allowHalfOpen=false：收到远端 FIN（'end'）后自动 end 本端
    this.allowHalfOpen = !!(options && options.allowHalfOpen);
    if (options && typeof options === "object") {
      if (options.readable !== undefined) this.readable = !!options.readable;
      if (options.writable !== undefined) this.writable = !!options.writable;
    }
    // 事件循环派发钩子：dispatch 以 global 为 this 调用，须预绑定（self 语义）
    this.__ev = this.__ev.bind(this);
  }
  connect(...args) {
    let port, host, cb;
    if (typeof args[0] === "object" && args[0] !== null) {
      ({ port, host = "127.0.0.1" } = args[0]);
      cb = typeof args[1] === "function" ? args[1] : undefined;
    } else {
      port = args[0];
      if (typeof args[1] === "string") { host = args[1]; cb = typeof args[2] === "function" ? args[2] : undefined; }
      else { host = "127.0.0.1"; cb = typeof args[1] === "function" ? args[1] : undefined; }
    }
    if (cb) this.once("connect", cb);
    this.remoteAddress = String(host);
    this.remotePort = Number(port);
    this.__id = Number(__wjs_net_connect(this.remoteAddress, this.remotePort, this));
    return this;
  }
  // 事件循环派发钩子（Rust dispatch 调用；kind/data 均为字符串）
  __ev(kind, payload) {
    switch (kind) {
      case "connect": {
        try {
          const o = JSON.parse(payload || "{}");
          // serde SocketAddr → "ip:port"（IPv6 为 "[ip]:port"）
          if (typeof o.local === "string") {
            const m = o.local.match(/^\[?([^\]]+?)\]?:(\d+)$/);
            if (m) { this.localAddress = m[1]; this.localPort = Number(m[2]); }
          }
        } catch {}
        this.readable = true; this.writable = true;
        this.emit("connect");
        break;
      }
      case "data": {
        const u8 = __b64dec(payload);
        this.emit("data", this.__enc ? new TextDecoder(this.__enc).decode(u8) : Buffer.from(u8));
        break;
      }
      case "end": {
        this.readable = false;
        this.emit("end");
        // Node 口径：非 allowHalfOpen 时收 FIN 即自动回 FIN（'close' 随后）
        if (!this.allowHalfOpen && this.__id) __wjs_net_end(this.__id);
        break;
      }
      case "error": {
        const o = JSON.parse(payload);
        this.emit("error", __netErr(o.code, o.msg));
        break;
      }
      case "close": this.destroyed = true; this.emit("close"); break;
    }
  }
  write(data, enc, cb) {
    if (this.destroyed || !this.writable) throw __netErr("ERR_STREAM_DESTROYED", "Cannot call write after a stream was destroyed");
    const cb2 = typeof enc === "function" ? enc : cb;
    __wjs_net_write(this.__id, __toU8(data, "write"));
    // 记档：底层同步写队列，无 flush 语义，回调即刻
    if (cb2) queueMicrotask(cb2);
    return true;
  }
  end(data, enc, cb) {
    if (data !== undefined && data !== null) this.write(data, typeof enc === "string" ? enc : undefined);
    const cb2 = typeof enc === "function" ? enc : cb;
    if (this.__id) __wjs_net_end(this.__id);
    this.writable = false;
    if (cb2) this.once("close", cb2);
    return this;
  }
  destroy(err) {
    if (!this.destroyed) {
      this.destroyed = true;
      this.writable = false; this.readable = false;
      if (this.__id) __wjs_net_destroy(this.__id);
      if (err) this.emit("error", err);
    }
    return this;
  }
  address() {
    if (this.localAddress === null) return null;
    return { address: this.localAddress, port: this.localPort, family: String(this.localAddress).includes(":") ? "IPv6" : "IPv4" };
  }
  setEncoding(enc) { this.__enc = enc === null || enc === undefined ? null : String(enc); return this; }
  ref() { return this; }
  unref() { return this; }
}

class Server extends EventEmitter {
  constructor(options, cb) {
    super();
    this.__id = 0;
    this.__listening = null;
    if (typeof options === "function") { cb = options; options = undefined; }
    if (typeof cb === "function") this.on("connection", cb);
    // 派发钩子预绑定（同 Socket 注）
    this.__ev = this.__ev.bind(this);
  }
  listen(...args) {
    let port, host = null, cb = null;
    if (typeof args[0] === "object" && args[0] !== null) {
      port = args[0].port;
      host = args[0].host ?? null;
      cb = typeof args[1] === "function" ? args[1] : null;
    } else {
      port = args[0];
      for (let i = 1; i < args.length; i++) {
        if (typeof args[i] === "string" && host === null) host = args[i];
        else if (typeof args[i] === "function") cb = args[i];
      }
    }
    if (cb) this.once("listening", cb);
    this.__port = Number(port);
    this.__id = Number(__wjs_net_listen(Number(port), host === null ? "0.0.0.0" : host, this));
    return this;
  }
  __ev(kind, payload) {
    switch (kind) {
      case "listening": {
        const o = JSON.parse(payload);
        this.__listening = { address: o.addr, port: o.port, family: String(o.addr).includes(":") ? "IPv6" : "IPv4" };
        this.emit("listening");
        break;
      }
      case "connection": {
        const o = JSON.parse(payload);
        const s = new Socket();
        s.__attachConn(o);
        this.emit("connection", s);
        break;
      }
      case "error": {
        const o = JSON.parse(payload);
        const e = __netErr(o.code, o.msg);
        e.port = this.__listening ? this.__listening.port : this.__port;
        this.emit("error", e);
        break;
      }
      case "close": this.emit("close"); break;
    }
  }
  address() { return this.__listening; }
  close(cb) {
    if (typeof cb === "function") this.once("close", cb);
    if (this.__id) __wjs_net_destroy(this.__id);
    return this;
  }
  ref() { return this; }
  unref() { return this; }
}

Socket.prototype.__attachConn = function (info) {
  this.__id = Number(info.connId);
  this.remoteAddress = info.remoteAddress;
  this.remotePort = info.remotePort;
  this.localAddress = info.localAddress;
  this.localPort = info.localPort;
  this.readable = true; this.writable = true;
  __wjs_net_attach(this.__id, this);
};

export function createServer(options, cb) {
  return new Server(options, cb);
}
export function createConnection(...args) { return new Socket().connect(...args); }
export const connect = createConnection;
export { Socket, Server };
export const Stream = Socket;
const __api = { Socket, Server, createServer, createConnection, connect, Stream };
export default __api;
"#;
