//! `node:tls` + `node:https` 的传输底座（tokio-rustls，dependencies2 §9d-剩余表口径）。
//! 复用 `node:net` 的事件通道与状态机（`NetEvent`/`NetCmd`/`state::net_*` + 泵），
//! TLS 只负责握手：client（Tcp connect → TlsConnector）与 server（Tcp accept →
//! TlsAcceptor），握完把流 split 后交 `net::spawn_pumps`（9d-6 泛化）。
//! 偏差记档：
//! - 证书校验：`ca`（PEM 串）→ 自建 roots；`rejectUnauthorized:false` → 跳过校验；
//!   缺省 → 系统 roots（rustls-native-certs）。hostname（SAN）校验由 rustls 全量做。
//! - 握手失败事件 code 为 `ERR_TLS_HANDSHAKE`（rustls 细粒度 cert 码未逐项映射，顺延）。
//! - 服务端 PEM 错误同步抛 TypeError（fail fast，Node 口径）；握手中单连接失败静默丢弃。
//! - 不做：`createSecureContext`/`getPeerCertificate`/客户端证书/`checkServerIdentity`
//!   自定义（顺延，另切片）。

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::context::JSContext;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::builtins::node::net::{spawn_pumps, NetEvent, NetKind};
use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};
use crate::state;

/// rustls ring provider（fetch/serve 同款；重复安装忽略）。
fn ensure_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// `rejectUnauthorized:false` 用的空校验器（测试/自签场景；生产缺省仍校验）。
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
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// 服务端配置（PEM 串 → ServerConfig；纯函数，单元测试覆盖）。
fn server_config(cert_pem: &str, key_pem: &str) -> Result<rustls::ServerConfig, String> {
    let certs: Vec<rustls::pki_types::CertificateDer<'static>> =
        rustls_pemfile::certs(&mut cert_pem.as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("TypeError: tls cert: bad PEM ({e})"))?;
    if certs.is_empty() {
        return Err("TypeError: tls cert: no certificate in PEM".into());
    }
    let key = rustls_pemfile::private_key(&mut key_pem.as_bytes())
        .map_err(|e| format!("TypeError: tls key: bad PEM ({e})"))?
        .ok_or_else(|| "TypeError: tls key: no private key in PEM".to_string())?;
    rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| format!("TypeError: tls: bad key/cert pair ({e})"))
}

/// 客户端配置（纯函数，单元测试覆盖）。
/// `reject_unauthorized=false` → 跳过校验；`ca_pem` → 自建 roots；缺省系统 roots。
fn client_config(ca_pem: Option<&str>, reject_unauthorized: bool) -> Result<rustls::ClientConfig, String> {
    if !reject_unauthorized {
        return Ok(rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(std::sync::Arc::new(NoVerifier))
            .with_no_client_auth());
    }
    let mut roots = rustls::RootCertStore::empty();
    if let Some(pem) = ca_pem {
        let mut n = 0usize;
        for cert in rustls_pemfile::certs(&mut pem.as_bytes()) {
            let cert = cert.map_err(|e| format!("TypeError: tls ca: bad PEM ({e})"))?;
            roots.add(cert).map_err(|e| format!("TypeError: tls ca: rejected ({e})"))?;
            n += 1;
        }
        if n == 0 {
            return Err("TypeError: tls ca: no certificate in PEM".into());
        }
    } else {
        let loaded = rustls_native_certs::load_native_certs();
        let mut added = 0usize;
        for cert in loaded.certs {
            if roots.add(cert).is_ok() {
                added += 1;
            }
        }
        if added == 0 {
            return Err(format!("TLS: no system roots ({} load errors)", loaded.errors.len()));
        }
    }
    Ok(rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth())
}

/// 服务端配置（HTTP/2，ALPN `h2`；`node:http2` 用）。
pub(crate) fn server_config_h2(cert_pem: &str, key_pem: &str) -> Result<rustls::ServerConfig, String> {
    let mut cfg = server_config(cert_pem, key_pem)?;
    cfg.alpn_protocols = vec![b"h2".to_vec()];
    Ok(cfg)
}

/// 客户端配置（HTTP/2，ALPN `h2`；`node:http2` 用）。
pub(crate) fn client_config_h2(
    ca_pem: Option<&str>,
    reject_unauthorized: bool,
) -> Result<rustls::ClientConfig, String> {
    let mut cfg = client_config(ca_pem, reject_unauthorized)?;
    cfg.alpn_protocols = vec![b"h2".to_vec()];
    Ok(cfg)
}

fn set_rval_str(cx: &mut JSContext, frame: &Frame, s: &str) {
    rooted!(&in(cx) let mut v = UndefinedValue());
    s.to_jsval(cx, v.handle_mut());
    frame.set_rval(v.get());
}

fn opt_num(frame: &Frame, i: u32) -> Option<f64> {
    let v = frame.arg(i);
    if v.is_number() { Some(v.to_number()) } else { None }
}

/// 连接选项 JSON（client）：`{servername?, ca?, rejectUnauthorized?}`。
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct ConnectOpts {
    servername: Option<String>,
    #[serde(rename = "ca")]
    ca_pem: Option<String>,
    #[serde(rename = "rejectUnauthorized")]
    reject_unauthorized: Option<bool>,
}

/// 监听选项 JSON（server）：`{cert, key}`（PEM 串；缺失即同步 TypeError）。
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct ListenOpts {
    cert: Option<String>,
    key: Option<String>,
}

/// `__wjs_tls_connect(host, port, optsJson, target)` → id。
pub unsafe extern "C" fn tls_connect(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 || !frame.arg(3).is_object() {
        report_error(&mut cx, "TypeError: tls connect internals missing target");
        return false;
    }
    let host = value_to_string(&mut cx, frame.arg(0));
    let Some(port) = opt_num(&frame, 1) else {
        report_error(&mut cx, "TypeError: tls connect: port must be a number");
        return false;
    };
    let opts: ConnectOpts = serde_json::from_str(&value_to_string(&mut cx, frame.arg(2)))
        .unwrap_or_default();
    let target = frame.arg(3);
    ensure_provider();
    let reject = opts.reject_unauthorized.unwrap_or(true);
    let cfg = match client_config(opts.ca_pem.as_deref(), reject) {
        Ok(c) => c,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let Some((id, ev_tx)) = state::net_alloc() else {
        report_error(&mut cx, "OperationError: net driver not installed");
        return false;
    };
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        report_error(&mut cx, "OperationError: no async runtime for tls connect");
        return false;
    };
    let cmd_rx = state::net_socket_add(id, target);
    set_rval_str(&mut cx, &frame, &id.to_string());
    let servername = opts.servername.unwrap_or_else(|| host.clone());
    handle.spawn(async move {
        let tcp = match crate::builtins::node::net_pumps::tcp_connect_resolved(host.as_str(), port as u16).await {
            Ok(s) => {
                // https 客户端默认 noDelay（Node https.js 口径）。
                let _ = s.set_nodelay(true);
                s
            }
            Err((code, msg)) => {
                let _ = ev_tx.send(NetEvent {
                    id,
                    kind: NetKind::Error { code: code.into(), msg },
                });
                let _ = ev_tx.send(NetEvent { id, kind: NetKind::Close });
                return;
            }
        };
        let name = match rustls::pki_types::ServerName::try_from(servername.clone()) {
            Ok(n) => n,
            Err(e) => {
                let _ = ev_tx.send(NetEvent {
                    id,
                    kind: NetKind::Error {
                        code: "ERR_TLS_HANDSHAKE".into(),
                        msg: format!("ERR_TLS_HANDSHAKE: bad servername '{servername}': {e}"),
                    },
                });
                let _ = ev_tx.send(NetEvent { id, kind: NetKind::Close });
                return;
            }
        };
        let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(cfg));
        match connector.connect(name, tcp).await {
            Err(e) => {
                let _ = ev_tx.send(NetEvent {
                    id,
                    kind: NetKind::Error {
                        code: "ERR_TLS_HANDSHAKE".into(),
                        msg: format!("ERR_TLS_HANDSHAKE: {e}"),
                    },
                });
                let _ = ev_tx.send(NetEvent { id, kind: NetKind::Close });
            }
            Ok(tls) => {
                let _ = ev_tx.send(NetEvent { id, kind: NetKind::Connect { local: None } });
                let (r, w) = tokio::io::split(tls);
                spawn_pumps(id, r, w, ev_tx, cmd_rx);
            }
        }
    });
    true
}

/// `__wjs_tls_listen(port, host, optsJson, target)` → id。
pub unsafe extern "C" fn tls_listen(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 || !frame.arg(3).is_object() {
        report_error(&mut cx, "TypeError: tls listen internals missing target");
        return false;
    }
    let Some(port) = opt_num(&frame, 0) else {
        report_error(&mut cx, "TypeError: tls listen: port must be a number");
        return false;
    };
    let host = value_to_string(&mut cx, frame.arg(1));
    let opts: ListenOpts = serde_json::from_str(&value_to_string(&mut cx, frame.arg(2)))
        .unwrap_or_default();
    let target = frame.arg(3);
    ensure_provider();
    let (Some(cert_pem), Some(key_pem)) = (opts.cert, opts.key) else {
        report_error(&mut cx, "TypeError: tls server needs { key, cert } PEM strings");
        return false;
    };
    let cfg = match server_config(&cert_pem, &key_pem) {
        Ok(c) => std::sync::Arc::new(c),
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let Some((id, ev_tx)) = state::net_alloc() else {
        report_error(&mut cx, "OperationError: net driver not installed");
        return false;
    };
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        report_error(&mut cx, "OperationError: no async runtime for tls listen");
        return false;
    };
    let mut cmd_rx = state::net_socket_add(id, target);
    set_rval_str(&mut cx, &frame, &id.to_string());
    handle.spawn(async move {
        let acceptor = tokio_rustls::TlsAcceptor::from(cfg);
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
                    let Ok((stream, peer)) = acc else { continue };
                    let Ok(tls) = acceptor.accept(stream).await else { continue };
                    let conn_local = tls
                        .get_ref()
                        .0
                        .local_addr()
                        .unwrap_or_else(|_| "0.0.0.0:0".parse::<std::net::SocketAddr>().expect("literal addr"));
                    let (conn_id, conn_cmd_rx) = state::net_conn_add();
                    let (r, w) = tokio::io::split(tls);
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
                _ = cmd_rx.recv() => break,
            }
        }
        let _ = ev_tx.send(NetEvent { id, kind: NetKind::ServerClose });
    });
    true
}

/// 内嵌 ESM 源（`node:tls`；TLSSocket/Server 建在 node:events 之上，net 语义复刻）。
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
function __tlsErr(code, msg) {
  const e = new Error(msg);
  e.code = code;
  return e;
}

class TLSSocket extends EventEmitter {
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
    this.encrypted = true;
    this.authorized = false;
    this.authorizationError = null;
    this.allowHalfOpen = !!(options && options.allowHalfOpen);
    // 派发钩子预绑定（dispatch 以 global 为 this，见 §4.34/§4.36）
    this.__ev = this.__ev.bind(this);
  }
  connect(...args) {
    let port, host = "127.0.0.1", options = {}, cb;
    if (typeof args[0] === "object" && args[0] !== null) {
      const o = args[0];
      port = o.port; host = o.host ?? o.servername ?? host; options = o;
      cb = typeof args[1] === "function" ? args[1] : undefined;
    } else {
      port = args[0];
      if (typeof args[1] === "string") { host = args[1]; options = args[2] ?? {}; cb = typeof args[3] === "function" ? args[3] : undefined; }
      else if (typeof args[1] === "object" && args[1] !== null) { options = args[1]; cb = typeof args[2] === "function" ? args[2] : undefined; }
      else { cb = typeof args[1] === "function" ? args[1] : undefined; }
    }
    if (cb) this.once("secureConnect", cb);
    this.remoteAddress = String(host);
    this.remotePort = Number(port);
    this.__verify = options.rejectUnauthorized !== false;
    const wire = {};
    if (options.servername !== undefined) wire.servername = String(options.servername);
    if (options.ca !== undefined) wire.ca = String(options.ca);
    if (options.rejectUnauthorized !== undefined) wire.rejectUnauthorized = !!options.rejectUnauthorized;
    this.__id = Number(__wjs_tls_connect(this.remoteAddress, this.remotePort, JSON.stringify(wire), this));
    return this;
  }
  __ev(kind, payload) {
    switch (kind) {
      case "connect": {
        this.readable = true; this.writable = true;
        // 握手已过：校验开则授权成立，否则记未授权（Node 口径）
        this.authorized = this.__verify !== false;
        if (!this.authorized) {
          this.authorizationError = __tlsErr("UNABLE_TO_VERIFY_LEAF_SIGNATURE", "self-signed certificate (rejectUnauthorized:false)");
        }
        this.emit("secureConnect");
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
        // 池化空闲 socket 见 FIN 即销毁（同 net.js，10b）。
        if (this.__inPool) {
          this.destroy();
          break;
        }
        this.emit("end");
        if (!this.allowHalfOpen && this.__id) __wjs_net_end(this.__id);
        break;
      }
      case "error": {
        const o = JSON.parse(payload);
        this.emit("error", __tlsErr(o.code, o.msg));
        break;
      }
      case "close": this.destroyed = true; this.emit("close"); break;
    }
  }
  write(data, enc, cb) {
    if (this.destroyed || !this.writable) throw __tlsErr("ERR_STREAM_DESTROYED", "Cannot call write after a stream was destroyed");
    const cb2 = typeof enc === "function" ? enc : cb;
    __wjs_net_write(this.__id, __toU8(data, "write"));
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
    this.__tlsOpts = {};
    if (typeof options === "function") { cb = options; options = undefined; }
    else if (options && typeof options === "object") {
      if (options.key === undefined || options.cert === undefined) {
        throw new TypeError("tls.createServer needs { key, cert } PEM strings");
      }
      this.__tlsOpts = { key: String(options.key), cert: String(options.cert) };
    }
    if (typeof cb === "function") this.on("secureConnection", cb);
    // 派发钩子预绑定（同上）
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
    this.__id = Number(__wjs_tls_listen(Number(port), host === null ? "0.0.0.0" : host, JSON.stringify(this.__tlsOpts), this));
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
        const s = new TLSSocket();
        s.__attachConn(o);
        this.emit("secureConnection", s);
        this.emit("connection", s);
        break;
      }
      case "error": {
        const o = JSON.parse(payload);
        const e = __tlsErr(o.code, o.msg);
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

TLSSocket.prototype.__attachConn = function (info) {
  this.__id = Number(info.connId);
  this.remoteAddress = info.remoteAddress;
  this.remotePort = info.remotePort;
  this.localAddress = info.localAddress;
  this.localPort = info.localPort;
  this.readable = true; this.writable = true;
  this.authorized = true;
  __wjs_net_attach(this.__id, this);
};

export function createServer(options, cb) {
  return new Server(options, cb);
}
export function createConnection(...args) { return new TLSSocket().connect(...args); }
export const connect = createConnection;
export { TLSSocket, Server };
const __api = { TLSSocket, Server, createServer, createConnection, connect };
export default __api;
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls_server_config_rejects_garbage() {
        assert!(server_config("nope", "nope").is_err());
        assert!(server_config("", "").is_err());
    }

    #[test]
    fn tls_client_config_no_verify_builds() {
        // rejectUnauthorized:false 不碰系统 roots，纯构造不断言网络
        assert!(client_config(None, false).is_ok());
        assert!(client_config(Some("definitely not pem"), true).is_err());
    }
}
