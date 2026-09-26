import { EventEmitter } from "node:events";
import { Socket } from "node:net";
import {
  validateFunction as __tlsVFunction,
  validateNumber as __tlsVNumber,
} from "node:internal/validators";
const Buffer = globalThis.Buffer;

// ── node:tls（P2，2026-09-25）：TLSSocket 建在 net.Socket 之上（读写/流面/计时/ref 全继承，
// Rust 侧 TLS 流与 TCP 共用 net 泵），本文件只加 TLS 语义：握手结果（tlsInfo 事件）、
// 授权位、secureConnect、证书/套件/协议查询与 node 顶层 API（CA/SecureContext/身份校验）。

function __tlsErr(code, msg) {
  const e = new Error(msg);
  e.code = code;
  return e;
}
function __pemOf(v) {
  if (v === undefined || v === null) return undefined;
  if (Array.isArray(v)) return v.map(__pemOf).filter((x) => x !== undefined).join("\n");
  if (typeof v === "object" && v !== null && !ArrayBuffer.isView(v) && v.pem !== undefined) return __pemOf(v.pem);
  return String(Buffer.isBuffer(v) || ArrayBuffer.isView(v) ? Buffer.from(v).toString("utf8") : v);
}

// rustls 套件名（Debug 形）→ node getCipher()：TLS1.3 name=standardName=IANA 名；
// TLS1.2 name 为 OpenSSL 名（常见套件表），standardName 为 IANA 名。
const __OSSL12 = {
  TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256: "ECDHE-RSA-AES128-GCM-SHA256",
  TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384: "ECDHE-RSA-AES256-GCM-SHA384",
  TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256: "ECDHE-RSA-CHACHA20-POLY1305",
  TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256: "ECDHE-ECDSA-AES128-GCM-SHA256",
  TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384: "ECDHE-ECDSA-AES256-GCM-SHA384",
  TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256: "ECDHE-ECDSA-CHACHA20-POLY1305",
};
function __cipherInfo(debugName, version) {
  if (!debugName) return undefined;
  const standardName = String(debugName).replace(/^TLS13_/, "TLS_");
  const name = version === "TLSv1.2" ? (__OSSL12[standardName] ?? standardName) : standardName;
  return { name, standardName, version: version ?? "unknown" };
}
function __legacyCert(b64) {
  if (!b64) return null;
  const raw = Buffer.from(atob(b64), "latin1");
  try {
    const { X509Certificate } = globalThis.require("node:crypto");
    const o = new X509Certificate(raw).toLegacyObject();
    o.raw = raw;
    return o;
  } catch {
    return { raw };
  }
}

class TLSSocket extends Socket {
  constructor(socket, options) {
    // node：new TLSSocket([socket][, options])——首参非 Socket 的对象即 options。
    if (options === undefined && socket !== null && typeof socket === "object" && !(socket instanceof Socket)) {
      options = socket;
      socket = undefined;
    }
    super(options && typeof options === "object" ? { allowHalfOpen: !!options.allowHalfOpen } : undefined);
    this.encrypted = true;
    this.authorized = false;
    this.authorizationError = null;
    this.alpnProtocol = false;
    this.servername = null;
    this._secureEstablished = false;
    this.__tls = null;
    this.__tlsOpts = options ?? {};
    this.__wrapped = socket ?? null;
  }
  connect(...args) {
    let port, host = "localhost", options = {}, cb;
    if (typeof args[0] === "object" && args[0] !== null) {
      const o = args[0];
      port = o.port; host = o.host ?? o.servername ?? host; options = o;
      cb = typeof args[1] === "function" ? args[1] : undefined;
    } else {
      port = args[0];
      if (typeof args[1] === "string") { host = args[1]; options = args[2] ?? {}; cb = typeof args[3] === "function" ? args[3] : (typeof args[2] === "function" ? args[2] : undefined); }
      else if (typeof args[1] === "object" && args[1] !== null) { options = args[1]; cb = typeof args[2] === "function" ? args[2] : undefined; }
      else { cb = typeof args[1] === "function" ? args[1] : undefined; }
    }
    if (typeof options === "function") { cb = options; options = {}; }
    if (cb) this.once("secureConnect", cb);
    this.__tlsOpts = { ...this.__tlsOpts, ...options };
    const sc = options.secureContext?.context ?? {};
    this.__verify = options.rejectUnauthorized !== false;
    const wire = {};
    const servername = options.servername ?? (host && !__isIP(host) ? host : undefined);
    if (servername !== undefined) wire.servername = String(servername);
    const ca = __pemOf(options.ca ?? sc.ca) ?? (__defaultCA !== null ? __defaultCA.join("\n") : undefined);
    if (ca !== undefined) wire.ca = ca;
    if (options.rejectUnauthorized !== undefined) wire.rejectUnauthorized = !!options.rejectUnauthorized;
    // 连接期面（net.__realConnect 同口径：先建柄、读写可达，写缓冲至握手完成）。
    this.__targetHost = String(host);
    this.__targetPort = Number(port);
    this.remoteAddress = undefined; this.remotePort = undefined; this.remoteFamily = undefined;
    this.readable = true; this.writable = true;
    this.__handleClosed = false;
    this._handle = this.__makeHandle();
    this.__id = Number(__wjs_tls_connect(this.__targetHost, this.__targetPort, JSON.stringify(wire), this));
    return this;
  }
  __applyTlsInfo(info) {
    this.__tls = info;
    this.alpnProtocol = info.alpn ?? false;
    if (info.servername) this.servername = info.servername;
  }
  __ev(kind, payload) {
    if (kind === "tlsInfo") {
      try { this.__applyTlsInfo(JSON.parse(payload)); } catch { /* 坏载荷即无信息 */ }
      return;
    }
    if (kind === "error" && !this._secureEstablished) {
      // 握手失败：rustls 细码 → node/OpenSSL 码与文案（net 的 "connect CODE addr" 整形会吞掉细节）。
      let o = {};
      try { o = JSON.parse(payload); } catch { /* 非 JSON 即原样 */ }
      if (o.code === "ERR_TLS_HANDSHAKE") {
        this.destroy(__handshakeErr(String(o.msg ?? ""), this.servername ?? this.__targetHost));
        return;
      }
    }
    if (kind === "connect") {
      // 握手已过：校验开则授权成立，否则记未授权（node 口径：rejectUnauthorized:false 仍连上）。
      this.authorized = this.__verify !== false;
      this._secureEstablished = true;
      if (!this.authorized) {
        this.authorizationError = __tlsErr("UNABLE_TO_VERIFY_LEAF_SIGNATURE", "unable to verify the first certificate");
      }
      super.__ev(kind, payload);
      this.emit("secureConnect");
      return;
    }
    return super.__ev(kind, payload);
  }
  getProtocol() { return this.__tls?.protocol ?? null; }
  getCipher() { return __cipherInfo(this.__tls?.cipher, this.__tls?.protocol); }
  getPeerCertificate(detailed) {
    const chain = this.__tls?.peerChain ?? [];
    if (chain.length === 0) return {};
    const certs = chain.map(__legacyCert);
    if (detailed) {
      for (let i = 0; i < certs.length; i++) certs[i].issuerCertificate = certs[i + 1] ?? certs[i];
    }
    return certs[0];
  }
  getPeerX509Certificate() {
    const b = this.__tls?.peerChain?.[0];
    if (!b) return undefined;
    const { X509Certificate } = globalThis.require("node:crypto");
    return new X509Certificate(Buffer.from(atob(b), "latin1"));
  }
  getCertificate() { return this.__ownCert ? __legacyCert(this.__ownCert) : {}; }
  getX509Certificate() { return undefined; }
  getSession() { return undefined; }
  isSessionReused() { return false; }
  getTLSTicket() { return undefined; }
  getFinished() { return undefined; }
  getPeerFinished() { return undefined; }
  getEphemeralKeyInfo() { return this.__tls?.protocol ? {} : null; }
  getSharedSigalgs() { return []; }
  setServername(name) {
    if (typeof name !== "string") throw __tlsErr("ERR_INVALID_ARG_TYPE", `The "name" argument must be of type string.`);
    this.servername = name;
  }
  setMaxSendFragment(size) { return size >= 512 && size <= 16384; }
  renegotiate(options, cb) {
    const f = typeof options === "function" ? options : cb;
    // TLS1.3/rustls 无重协商（node 对 1.3 同样报错）。
    if (typeof f === "function") process.nextTick(f, __tlsErr("ERR_TLS_RENEGOTIATION_DISABLED", "TLS session renegotiation disabled for this socket"));
    return false;
  }
  disableRenegotiation() {}
  enableTrace() {}
  exportKeyingMaterial() { throw __tlsErr("ERR_TLS_INVALID_STATE", "TLS socket connection must be securely established"); }
}

function __handshakeErr(msg, host) {
  const map = [
    [/NotValidForName/, "ERR_TLS_CERT_ALTNAME_INVALID", `Hostname/IP does not match certificate's altnames: Host: ${host}. is not in the cert's altnames`],
    [/UnknownIssuer/, "UNABLE_TO_VERIFY_LEAF_SIGNATURE", "unable to verify the first certificate"],
    [/Expired/, "CERT_HAS_EXPIRED", "certificate has expired"],
    [/NotValidYet/, "CERT_NOT_YET_VALID", "certificate is not yet valid"],
    [/BadSignature/, "CERT_SIGNATURE_FAILURE", "certificate signature failure"],
  ];
  for (const [re, code, text] of map) {
    if (re.test(msg)) {
      const e = __tlsErr(code, text);
      e.detail = msg;
      return e;
    }
  }
  const e = __tlsErr("ERR_SSL_HANDSHAKE_FAILURE", msg.replace(/^ERR_TLS_HANDSHAKE: /, ""));
  e.detail = msg;
  return e;
}
function __isIP(h) { return /^[\d.]+$/.test(h) || String(h).includes(":"); }

class Server extends EventEmitter {
  constructor(options, cb) {
    super();
    this.__id = 0;
    this.__listening = null;
    this.__tlsOpts = {};
    this.__contexts = [];
    // node internal/tls/wrap.js Server()：参数形 → ALPN 互斥 → setSecureContext（校验全在
    // createSecureContext）→ 握手超时/SNI/PSK 回调校验。
    if (typeof options === "function") {
      cb = options;
      options = {};
    } else if (options == null || typeof options === "object") {
      options ??= {};
    } else {
      throw new __tlsC.ERR_INVALID_ARG_TYPE("options", "Object", options);
    }
    this._contexts = [];
    this.requestCert = options.requestCert === true;
    this.rejectUnauthorized = options.rejectUnauthorized !== false;
    this.ALPNCallback = options.ALPNCallback;
    if (this.ALPNCallback && options.ALPNProtocols) {
      throw new __tlsC.ERR_TLS_ALPN_CALLBACK_WITH_PROTOCOLS();
    }
    if (options.sessionTimeout) this.sessionTimeout = options.sessionTimeout;
    if (options.ticketKeys) this.ticketKeys = options.ticketKeys;
    this.setSecureContext(options);
    this.__handshakeTimeout = options.handshakeTimeout || (120 * 1000);
    this.__SNICallback = options.SNICallback;
    this.__pskCallback = options.pskCallback;
    this.__pskIdentityHint = options.pskIdentityHint;
    __tlsVNumber(this.__handshakeTimeout, "options.handshakeTimeout");
    if (this.__SNICallback) __tlsVFunction(this.__SNICallback, "options.SNICallback");
    if (this.__pskCallback) __tlsVFunction(this.__pskCallback, "options.pskCallback");
    if (this.__pskIdentityHint) __tlsVString(this.__pskIdentityHint, "options.pskIdentityHint");
    if (typeof cb === "function") this.on("secureConnection", cb);
    // 派发钩子预绑定（dispatch 以 global 为 this，见 §4.34/§4.36）
    this.__ev = this.__ev.bind(this);
  }
  listen(...args) {
    let port = 0, host = null, cb = null;
    if (typeof args[0] === "object" && args[0] !== null) {
      port = args[0].port ?? 0;
      host = args[0].host ?? null;
      cb = typeof args[1] === "function" ? args[1] : null;
    } else {
      for (const a of args) {
        if (typeof a === "number" || (typeof a === "string" && /^\d+$/.test(a) && port === 0)) port = Number(a);
        else if (typeof a === "string" && host === null) host = a;
        else if (typeof a === "function") cb = a;
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
        Socket.prototype.__attachConn.call(s, o);
        s.server = this;
        s.authorized = false;
        s._secureEstablished = true;
        // tlsInfo 事件紧随其后到 conn 自身（Rust 序：Connection → TlsInfo → 泵），
        // secureConnection 推迟一拍，让握手信息先落位。
        queueMicrotask(() => {
          this.emit("connection", s);
          this.emit("secureConnection", s);
        });
        break;
      }
      case "tlsClientError": {
        const o = JSON.parse(payload);
        const e = __tlsErr(o.code, o.msg);
        e.library = "SSL routines";
        this.emit("tlsClientError", e, undefined);
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
  addContext(hostname, context) {
    if (typeof hostname !== "string") throw __tlsErr("ERR_INVALID_ARG_TYPE", `The "hostname" argument must be of type string.`);
    // SNI 多证书：底座单证书解析器暂不分流（记档），先登记。
    this.__contexts.push([hostname, context]);
  }
  // node Server.prototype.setSecureContext：选项逐项落实例 → createSecureContext 共享凭据；
  // rustls 底座取其 key/cert PEM（secureContext 选项形同样经此）。
  setSecureContext(options) {
    __tlsVObject(options, "options");
    for (const k of ["pfx", "key", "passphrase", "cert", "clientCertEngine", "ca", "minVersion",
      "maxVersion", "secureProtocol", "crl", "ciphers", "dhparam"]) {
      this[k] = options[k] ? options[k] : undefined;
    }
    this.sigalgs = options.sigalgs;
    this.ecdhCurve = options.ecdhCurve;
    this.honorCipherOrder = options.honorCipherOrder !== undefined ? !!options.honorCipherOrder : true;
    this.secureOptions = options.secureOptions || undefined;
    this.sessionIdContext = options.sessionIdContext ? options.sessionIdContext : undefined;
    if (options.sessionTimeout) this.sessionTimeout = options.sessionTimeout;
    if (options.ticketKeys) this.ticketKeys = options.ticketKeys;
    this.privateKeyIdentifier = options.privateKeyIdentifier;
    this.privateKeyEngine = options.privateKeyEngine;
    this.certificateCompression = options.certificateCompression;
    this._sharedCreds = createSecureContext({
      pfx: this.pfx, key: this.key, passphrase: this.passphrase, cert: this.cert,
      clientCertEngine: this.clientCertEngine, ca: this.ca, ciphers: this.ciphers,
      sigalgs: this.sigalgs, ecdhCurve: this.ecdhCurve, dhparam: this.dhparam,
      minVersion: this.minVersion, maxVersion: this.maxVersion, secureProtocol: this.secureProtocol,
      secureOptions: this.secureOptions, honorCipherOrder: this.honorCipherOrder, crl: this.crl,
      sessionIdContext: this.sessionIdContext, ticketKeys: this.ticketKeys,
      sessionTimeout: this.sessionTimeout, privateKeyIdentifier: this.privateKeyIdentifier,
      privateKeyEngine: this.privateKeyEngine, certificateCompression: this.certificateCompression,
    });
    const sc = options.secureContext?.context ?? this._sharedCreds.context;
    const key = __pemOf(sc.key);
    const cert = __pemOf(sc.cert);
    // node：key/cert 可缺（握手时无证书可出示 → tlsClientError）。
    this.__tlsOpts = key !== undefined && cert !== undefined ? { key, cert } : {};
  }
  getTicketKeys() { return this._sharedCreds.context.getTicketKeys(); }
  setTicketKeys(keys) {
    __tlsVBuffer(keys);
    if (keys.byteLength !== 48) {
      throw __tlsErr("ERR_ASSERTION", "Session ticket keys must be a 48-byte buffer");
    }
    this._sharedCreds.context.setTicketKeys(keys);
  }
  ref() { return this; }
  unref() { return this; }
}

// ── CA 面（SecureContext 见 tls_context.js）────────────────────────────────
let __defaultCA = null;
function __certsOf(kind) {
  return JSON.parse(__wjs_tls_ca_certs(kind));
}
export function getCACertificates(type = "default") {
  if (typeof type !== "string") {
    const e = new TypeError(`The "type" argument must be of type string.`);
    e.code = "ERR_INVALID_ARG_TYPE";
    throw e;
  }
  if (type === "default") return __defaultCA !== null ? [...__defaultCA] : [...rootCertificates];
  if (type === "bundled") return rootCertificates;
  if (type === "system" || type === "extra") return __certsOf(type);
  const e = new TypeError(`The argument 'type' must be one of: 'default', 'bundled', 'system', 'extra'. Received '${type}'`);
  e.code = "ERR_INVALID_ARG_VALUE";
  throw e;
}
// node tls.js setDefaultCACertificates：数组/元素类型校验 → 原生 resetRootCertStore（逐个解析，
// 一个都解析不出即 ERR_CRYPTO_OPERATION_FAILED，默认集不变）。
export function setDefaultCACertificates(certs) {
  if (!Array.isArray(certs)) {
    throw new __tlsC.ERR_INVALID_ARG_TYPE("certs", "Array", certs);
  }
  for (let i = 0; i < certs.length; i++) {
    if (typeof certs[i] !== "string" && !ArrayBuffer.isView(certs[i])) {
      throw new __tlsC.ERR_INVALID_ARG_TYPE(`certs[${i}]`, ["string", "ArrayBufferView"], certs[i]);
    }
  }
  const { X509Certificate } = globalThis.require("node:crypto");
  const valid = [];
  for (const c of certs) {
    const pem = typeof c === "string" ? c : Buffer.from(c.buffer, c.byteOffset, c.byteLength).toString("utf8");
    try {
      new X509Certificate(pem);
      if (!valid.includes(pem)) valid.push(pem);
    } catch {
      // 形似 PEM 证书块但 ASN.1 解不开 → OpenSSL PEM 层报错（整批作废，默认集不变）；
      // 根本不是 PEM 的串才静默跳过。
      if (pem.includes("-----BEGIN CERTIFICATE-----")) {
        const e = new Error("error:0488000D:PEM routines::ASN1 lib");
        e.code = "ERR_OSSL_PEM_ASN1_LIB";
        e.library = "PEM routines";
        e.reason = "ASN1 lib";
        throw e;
      }
    }
  }
  if (certs.length > 0 && valid.length === 0) {
    const e = new Error("No valid certificates found in the provided array");
    e.code = "ERR_CRYPTO_OPERATION_FAILED";
    throw e;
  }
  __defaultCA = valid;
}
// node 口径：bundled/root 证书 PEM 无尾换行，且 getCACertificates('bundled') 即同一冻结数组。
export const rootCertificates = Object.freeze(__certsOf("bundled").map((c) => c.replace(/\n+$/, "")));

// node tls.checkServerIdentity：SAN(DNS/IP) 优先、无 SAN 回落 CN；通配仅最左单层。
function __hostMatch(pattern, host) {
  const p = String(pattern).toLowerCase(), h = String(host).toLowerCase();
  if (p.startsWith("*.")) {
    const i = h.indexOf(".");
    return i > 0 && h.slice(i + 1) === p.slice(2);
  }
  return p === h;
}
export function checkServerIdentity(hostname, cert) {
  const subject = cert?.subject ?? {};
  const alt = String(cert?.subjectaltname ?? "");
  const dns = [], ips = [];
  for (const part of alt.split(/,\s*/)) {
    if (part.startsWith("DNS:")) dns.push(part.slice(4));
    else if (part.startsWith("IP Address:")) ips.push(part.slice(11));
  }
  let valid, reason;
  if (__isIP(hostname)) {
    valid = ips.includes(hostname);
    if (!valid) reason = `IP: ${hostname} is not in the cert's list: ${ips.join(", ")}`;
  } else if (dns.length > 0) {
    valid = dns.some((d) => __hostMatch(d, hostname));
    if (!valid) reason = `Host: ${hostname}. is not in the cert's altnames: ${alt}`;
  } else {
    const cn = subject.CN;
    valid = Array.isArray(cn) ? cn.some((c) => __hostMatch(c, hostname)) : (cn !== undefined && __hostMatch(cn, hostname));
    if (!valid) reason = cn === undefined ? "Cert does not contain a DNS name" : `Host: ${hostname}. is not cert's CN: ${cn}`;
  }
  if (!valid) {
    const e = new Error(`Hostname/IP does not match certificate's altnames: ${reason}`);
    e.code = "ERR_TLS_CERT_ALTNAME_INVALID";
    e.reason = reason;
    e.host = hostname;
    e.cert = cert;
    return e;
  }
  return undefined;
}

export const DEFAULT_ECDH_CURVE = "auto";
export const DEFAULT_MIN_VERSION = "TLSv1.2";
export const DEFAULT_MAX_VERSION = "TLSv1.3";
export const DEFAULT_CIPHERS =
  "TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256:TLS_AES_128_GCM_SHA256:" +
  "ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES256-GCM-SHA384:" +
  "ECDHE-ECDSA-AES256-GCM-SHA384";
export function getCiphers() {
  return ["tls_aes_128_gcm_sha256", "tls_aes_256_gcm_sha384", "tls_chacha20_poly1305_sha256",
    "ecdhe-ecdsa-aes128-gcm-sha256", "ecdhe-ecdsa-aes256-gcm-sha384", "ecdhe-ecdsa-chacha20-poly1305",
    "ecdhe-rsa-aes128-gcm-sha256", "ecdhe-rsa-aes256-gcm-sha384", "ecdhe-rsa-chacha20-poly1305"];
}
export const CLIENT_RENEG_LIMIT = 3;
export const CLIENT_RENEG_WINDOW = 600;

export function createServer(options, cb) {
  return new Server(options, cb);
}
export function connect(...args) {
  const o = typeof args[0] === "object" && args[0] !== null ? args[0] : (typeof args[1] === "object" && args[1] !== null ? args[1] : (typeof args[2] === "object" && args[2] !== null ? args[2] : {}));
  // node exports.connect：缺省合并 → 回调/DH 校验 → createSecureContext(options)（套件/证书/密钥
  // 校验同 server）→ servername 禁 IP。
  const options = {
    ciphers: __api.DEFAULT_CIPHERS,
    checkServerIdentity: __api.checkServerIdentity,
    minDHSize: 1024,
    ...o,
  };
  __tlsVFunction(options.checkServerIdentity, "options.checkServerIdentity");
  __tlsVNumber(options.minDHSize, "options.minDHSize", 1);
  const context = options.secureContext || createSecureContext(options);
  if (options.servername && __isIP(options.servername)) {
    throw new __tlsC.ERR_INVALID_ARG_VALUE(
      "options.servername",
      options.servername,
      "Setting the TLS ServerName to an IP address is not permitted.",
    );
  }
  if (!o.secureContext) o.secureContext = context;
  return new TLSSocket(undefined, { allowHalfOpen: !!o.allowHalfOpen }).connect(...args);
}
// node 口径：`tls.Server(...)`/`SecureContext(...)` 无 new 可调（函数构造器自 new）；
// Proxy apply→construct，extends/instanceof 照常（connect-simple 等 9 件）。
const __callable = (C) => new Proxy(C, { apply(t, _this, args) { return new t(...args); } });
const __ServerCallable = __callable(Server);
export { connect as createConnection, TLSSocket, __ServerCallable as Server, SecureContext };
const __api = {
  TLSSocket, Server: __ServerCallable, SecureContext, createServer, connect, createConnection: connect,
  createSecureContext, getCACertificates, setDefaultCACertificates,
  checkServerIdentity, getCiphers, DEFAULT_ECDH_CURVE, DEFAULT_MIN_VERSION, DEFAULT_MAX_VERSION,
  DEFAULT_CIPHERS, CLIENT_RENEG_LIMIT, CLIENT_RENEG_WINDOW,
};
// node 口径：rootCertificates 为只读访问器（赋值在严格模式抛 TypeError，root-certificates 套件）。
Object.defineProperty(__api, "rootCertificates", {
  __proto__: null,
  configurable: false,
  enumerable: true,
  get: () => rootCertificates,
});
export default __api;
