import { EventEmitter } from "node:events";

export const CC_ALGO_RENO = "reno";
export const CC_ALGO_CUBIC = "cubic";
export const CC_ALGO_BBR = "bbr";

export class QuicError extends Error {
  constructor(message, code = "ERR_QUIC_ERROR") {
    super(message);
    this.name = "QuicError";
    this.code = code;
  }
}

function __parseCoded(m, dflt) {
  const mm = String(m).match(/^(ERR_[A-Z0-9_]+): ([\s\S]*)$/);
  if (mm) return { code: mm[1], message: mm[2] };
  return { code: dflt, message: String(m) };
}
function __quicErr(e, dflt = "ERR_QUIC_ERROR") {
  const m = String((e && e.message) || e);
  const { code, message } = __parseCoded(m, dflt);
  const err = new QuicError(message, code);
  throw err;
}
function __callNative(fn, dflt) {
  try {
    return fn();
  } catch (e) {
    __quicErr(e, dflt);
  }
}
function __needStr(v, what) {
  if (typeof v !== "string") {
    const err = new TypeError(`The "${what}" argument must be of type string. Received type ${typeof v}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  return v;
}
function __needPort(v, what) {
  if (typeof v !== "number" || !Number.isInteger(v) || v < 0 || v > 65535) {
    const err = new RangeError(`The "${what}" argument must be an integer in range 0..65535. Received ${String(v)}`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  return v;
}
function __normAlpn(v, what, single) {
  const list = Array.isArray(v) ? v.slice() : [v];
  if (list.length === 0 || list.some((s) => typeof s !== "string" || s.length === 0)) {
    const err = new TypeError(`The "${what}" argument must be a non-empty string or array of non-empty strings.`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (single && list.length !== 1) {
    const err = new TypeError(`The "${what}" argument must be a single protocol string.`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  return single ? list[0] : list;
}
function __normCc(v) {
  if (v === undefined) return undefined;
  if (v !== "reno" && v !== "cubic" && v !== "bbr") {
    const err = new TypeError(`The "options.cc" property must be one of reno/cubic/bbr.`);
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
  return v;
}
function __normIdle(v) {
  if (v === undefined) return undefined;
  if (typeof v !== "number" || !Number.isInteger(v) || v < 0) {
    const err = new RangeError(`The "options.idleTimeout" property must be a non-negative integer.`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  return v;
}
function __parseAddr(addr) {
  if (typeof addr === "string") {
    let host, port;
    if (addr.startsWith("[")) {
      const rb = addr.indexOf("]");
      if (rb < 0) throw new TypeError(`Invalid address string.`);
      host = addr.slice(1, rb);
      const rest = addr.slice(rb + 1);
      if (!rest.startsWith(":")) throw new TypeError(`Invalid address string.`);
      port = Number(rest.slice(1));
    } else {
      const i = addr.lastIndexOf(":");
      if (i < 0) throw new TypeError(`Invalid address string.`);
      host = addr.slice(0, i);
      port = Number(addr.slice(i + 1));
    }
    if (host === "") host = "127.0.0.1";
    return { host, port: __needPort(port, "address port") };
  }
  if (addr !== null && typeof addr === "object") {
    const host = addr.address ?? addr.host ?? "127.0.0.1";
    const port = addr.port;
    __needStr(String(host), "address");
    return { host: String(host), port: __needPort(port, "address.port") };
  }
  const err = new TypeError(`The "address" argument must be of type string or object.`);
  err.code = "ERR_INVALID_ARG_TYPE";
  throw err;
}

export class QuicEndpoint extends EventEmitter {
  constructor(__id) {
    super();
    if (typeof __id !== "string") {
      const err = new TypeError(`QuicEndpoint needs an internal endpoint id.`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    this.__id = __id;
    this.__closed = false;
    this.__ev = this.__ev.bind(this);
    __wjs2_quic_ep_attach(__id, this);
  }
  __ev(kind, payload) {
    if (kind === "session") {
      const info = JSON.parse(String(payload));
      const sess = new QuicSession(info.sessionId, {
        local: "", remote: info.remote, alpn: info.alpn, servername: info.servername,
      });
      this.emit("session", sess);
    } else if (kind === "close") {
      if (this.__closed) return;
      this.__closed = true;
      this.emit("close");
    }
  }
  address() {
    const s = __callNative(() => __wjs2_quic_ep_addr(this.__id));
    const { host, port } = __parseAddr(String(s));
    return { address: host, port, family: host.includes(":") ? "IPv6" : "IPv4" };
  }
  close() {
    if (this.__closed) return;
    __callNative(() => __wjs2_quic_ep_close(this.__id));
  }
}

export class QuicSession extends EventEmitter {
  constructor(__id, __peer = {}) {
    super();
    if (typeof __id !== "string") {
      const err = new TypeError(`QuicSession needs an internal session id.`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    this.__id = __id;
    this.__peer = __peer;
    this.__secure = false;
    this.__closed = false;
    this.__h3Pending = new Set();
    this.__ev = this.__ev.bind(this);
    __wjs2_quic_sess_attach(__id, this);
  }
  __ev(kind, payload) {
    if (kind === "secure") {
      const info = JSON.parse(String(payload));
      this.__secure = true;
      this.__peer.alpn = info.alpn;
      this.__peer.servername = info.servername;
      this.emit("secure", info.servername, info.alpn);
    } else if (kind === "error") {
      const { code, message } = __parseCoded(String(payload), "ERR_QUIC_HANDSHAKE");
      this.emit("error", new QuicError(message, code));
    } else if (kind === "close") {
      if (this.__closed) return;
      this.__closed = true;
      const info = JSON.parse(String(payload));
      // H3 未决请求随会话关闭全部失败（避免 promise 悬挂）。
      for (const fail of this.__h3Pending) fail();
      this.__h3Pending.clear();
      this.emit("close", info.code, info.reason);
    } else if (kind === "request") {
      const info = JSON.parse(String(payload));
      const req = {
        id: info.streamId,
        method: info.method,
        path: info.path,
        headers: info.headers,
        body: Buffer.from(String(info.body || ""), "base64"),
        respond: ({ status = 200, headers = {}, body } = {}) => {
          const b64 = body === undefined || body === null ? ""
            : Buffer.isBuffer(body) ? body.toString("base64")
            : Buffer.from(body).toString("base64");
          __callNative(() => __wjs2_quic_h3_respond(this.__id, info.streamId,
            JSON.stringify({ status, headers, body: b64 })));
        },
      };
      this.emit("request", req);
    } else if (kind === "stream") {
      const info = JSON.parse(String(payload));
      const stream = new QuicStream(info.streamId, { dir: info.dir, qid: info.qid });
      this.emit("stream", stream);
    } else if (kind === "datagram") {
      this.emit("datagram", Buffer.from(String(payload), "base64"));
    }
  }
  get encrypted() { return this.__secure; }
  get alpnProtocol() {
    if (!this.__secure) return null;
    if (this.__peer.alpn) return this.__peer.alpn;
    const info = JSON.parse(__callNative(() => __wjs2_quic_sess_info(this.__id)));
    return info.alpn || null;
  }
  get servername() {
    if (!this.__secure) return null;
    // 发起侧 handshake 无 SNI 回显（quinn 口径恒 None），用 secure 事件缓存值。
    if (this.__peer.servername) return this.__peer.servername;
    const info = JSON.parse(__callNative(() => __wjs2_quic_sess_info(this.__id)));
    return info.servername || null;
  }
  get localAddress() { return this.__addrOf("local"); }
  get remoteAddress() { return this.__addrOf("remote"); }
  __addrOf(which) {
    if (which === "remote" && this.__peer.remote) {
      const { host, port } = __parseAddr(this.__peer.remote);
      return { address: host, port, family: host.includes(":") ? "IPv6" : "IPv4" };
    }
    const info = JSON.parse(__callNative(() => __wjs2_quic_sess_info(this.__id)));
    const raw = which === "local" ? info.local : info.remote;
    if (!raw) return null;
    const { host, port } = __parseAddr(raw);
    return { address: host, port, family: host.includes(":") ? "IPv6" : "IPv4" };
  }
  stats() {
    return JSON.parse(__callNative(() => __wjs2_quic_sess_stats(this.__id)));
  }
  close(code = 0) {
    if (typeof code !== "number" || !Number.isInteger(code) || code < 0) {
      const err = new RangeError(`The "code" argument must be a non-negative integer.`);
      err.code = "ERR_OUT_OF_RANGE";
      throw err;
    }
    __callNative(() => __wjs2_quic_sess_close(this.__id, String(code)));
  }
  destroy(err) {
    void err;
    this.close();
  }
  request({ method = "GET", path = "/", headers = {}, body } = {}) {
    // 9i-9 H3 面（自定）：仅 ALPN "h3" 会话；响应走一次性 promise。
    if (this.__peer.alpn !== "h3") {
      const err = new Error(`Session ALPN must be "h3" for request(). Received ${JSON.stringify(this.__peer.alpn)}`);
      err.code = "ERR_INVALID_PROTOCOL";
      throw err;
    }
    const b64 = body === undefined || body === null ? ""
      : Buffer.isBuffer(body) ? body.toString("base64")
      : Buffer.from(body).toString("base64");
    const sid = __callNative(() => __wjs2_quic_h3_request(this.__id,
      JSON.stringify({ method, path, headers, body: b64 })));
    const target = new EventEmitter();
    target.__ev = (kind, payload) => {
      if (kind === "response") {
        const info = JSON.parse(String(payload));
        this.__h3Pending.delete(fail);
        target.emit("response", { status: info.status, headers: info.headers, body: Buffer.from(String(info.body || ""), "base64") });
      } else if (kind === "error") {
        this.__h3Pending.delete(fail);
        const { code, message } = __parseCoded(String(payload), "ERR_QUIC_H3");
        target.emit("error", new QuicError(message, code));
      } else if (kind === "closed") {
        this.__h3Pending.delete(fail);
        target.emit("error", new QuicError("session closed before response", "ERR_QUIC_SESSION_CLOSED"));
      }
    };
    const fail = () => target.emit("closed");
    this.__h3Pending.add(fail);
    __wjs2_quic_stream_attach(String(sid), target);
    return new Promise((resolve, reject) => {
      target.once("response", resolve);
      target.once("error", reject);
    });
  }
  createBidirectionalStream() {
    return this.__openStream("bidi");
  }
  createUnidirectionalStream() {
    return this.__openStream("uni");
  }
  __openStream(dir) {
    const sid = __callNative(() => __wjs2_quic_sess_open(this.__id, dir));
    const stream = new QuicStream(String(sid), { dir: dir === "bidi" ? "bidi" : "send", qid: null });
    return new Promise((resolve, reject) => {
      stream.__pendingOpen = { resolve, reject };
    });
  }
  sendDatagram(data) {
    const buf = Buffer.isBuffer(data) ? data : Buffer.from(String(data ?? ""));
    return Boolean(__callNative(() => __wjs2_quic_sess_send_dgram(this.__id, buf)));
  }
  get maxDatagramSize() {
    return Number(__callNative(() => __wjs2_quic_sess_max_dgram(this.__id)));
  }
}

function __normStreamCode(v, what) {
  if (typeof v === "bigint") v = Number(v);
  if (typeof v !== "number" || !Number.isInteger(v) || v < 0 || v > 4294967295) {
    const err = new RangeError(`The "${what}" argument must be an integer in range 0..2^32-1.`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  return v;
}

export class QuicStream extends EventEmitter {
  constructor(__id, { dir, qid }) {
    super();
    if (typeof __id !== "string") {
      const err = new TypeError(`QuicStream needs an internal stream id.`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    this.__id = __id;
    this.__dir = dir;
    this.__qid = qid ?? null;
    // 半流：不存在的方向视为已 done（单出口 close 配对才成立）。
    this.__readDone = (dir === "send");
    this.__writeDone = (dir === "receive");
    // end()/close() 调后同步落旗（writedone 事件异步到，期间再写必须同步抛）。
    this.__ended = (dir === "receive");
    this.__closed = false;
    this.__closeCode = 0;
    this.__pendingOpen = null;
    this.__ev = this.__ev.bind(this);
    __wjs2_quic_stream_attach(__id, this);
  }
  __ev(kind, payload) {
    if (kind === "opened") {
      this.__qid = Number(payload);
      if (this.__pendingOpen) {
        const { resolve } = this.__pendingOpen;
        this.__pendingOpen = null;
        resolve(this);
      }
    } else if (kind === "data") {
      this.emit("data", Buffer.from(String(payload), "base64"));
    } else if (kind === "end") {
      this.__readDone = true;
      this.emit("end");
      this.__maybeClose();
    } else if (kind === "writedone") {
      this.__writeDone = true;
      this.emit("finish");
      this.__maybeClose();
    } else if (kind === "closed") {
      this.__forceClose(Number(payload));
    } else if (kind === "error") {
      const { code, message } = __parseCoded(String(payload), "ERR_QUIC_STREAM");
      if (this.__pendingOpen) {
        const { reject } = this.__pendingOpen;
        this.__pendingOpen = null;
        reject(new QuicError(message, code));
      } else {
        this.emit("error", new QuicError(message, code));
      }
    }
  }
  __maybeClose() {
    if (this.__closed || !this.__readDone || !this.__writeDone) return;
    this.__closed = true;
    this.emit("close", this.__closeCode);
  }
  __forceClose(code) {
    if (this.__closed) return;
    this.__closed = true;
    this.__readDone = true;
    this.__writeDone = true;
    this.__closeCode = code;
    this.emit("close", code);
  }
  get id() { return this.__qid; }
  get direction() { return this.__dir; }
  write(chunk, encoding) {
    if (this.__dir === "receive") {
      const err = new Error("Cannot write to receive-only stream.");
      err.code = "ERR_INVALID_STATE";
      throw err;
    }
    if (this.__writeDone || this.__closed || this.__ended) {
      const err = new Error("Write after end.");
      err.code = "ERR_STREAM_WRITE_AFTER_END";
      throw err;
    }
    const buf = Buffer.isBuffer(chunk) ? chunk : Buffer.from(String(chunk ?? ""), encoding ?? "utf8");
    const ok = __callNative(() => __wjs2_quic_stream_write(this.__id, buf));
    if (!ok) {
      const err = new Error("Write after end.");
      err.code = "ERR_STREAM_WRITE_AFTER_END";
      throw err;
    }
    return true;
  }
  end(data, encoding) {
    if (data !== undefined) this.write(data, encoding);
    this.__ended = true;
    __callNative(() => __wjs2_quic_stream_finish(this.__id));
    return this;
  }
  close() {
    this.__ended = true;
    __callNative(() => __wjs2_quic_stream_finish(this.__id));
  }
  destroy(err) {
    __callNative(() => __wjs2_quic_stream_reset(this.__id, 0));
    __callNative(() => __wjs2_quic_stream_stop(this.__id, 0));
    if (err !== undefined && err !== null) {
      this.emit("error", err instanceof Error ? err : new Error(String(err)));
    }
    this.__forceClose(0);
  }
  stopSending(code = 0) {
    const c = __normStreamCode(code, "code");
    __callNative(() => __wjs2_quic_stream_stop(this.__id, c));
  }
  resetStream(code = 0) {
    const c = __normStreamCode(code, "code");
    __callNative(() => __wjs2_quic_stream_reset(this.__id, c));
  }
}

export async function listen(callback, options = {}) {
  if (typeof callback !== "function") {
    const err = new TypeError(`The "callback" argument must be of type function.`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (options === null || typeof options !== "object") {
    const err = new TypeError(`The "options" argument must be of type object.`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const { host, port } = __parseAddr({ address: options.address ?? options.host, port: options.port });
  const alpn = __normAlpn(options.alpn, "options.alpn", false);
  if (options.key === undefined || options.cert === undefined) {
    const err = new TypeError(`listen needs options.key and options.cert (PEM).`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  // 校验提在 native 调用之外（包进 __callNative 会被重包成 ERR_QUIC_ERROR）。
  const idleMs = __normIdle(options.idleTimeout);
  const ccName = __normCc(options.cc);
  const id = __callNative(() => __wjs2_quic_listen(JSON.stringify({
    host, port, alpn,
    key_pem: String(options.key), cert_pem: String(options.cert),
    idle_timeout_ms: idleMs, cc: ccName,
  })));
  const ep = new QuicEndpoint(String(id));
  ep.on("session", callback);
  return ep;
}

export async function connect(address, options = {}) {
  const { host, port } = __parseAddr(address);
  if (options === null || typeof options !== "object") {
    const err = new TypeError(`The "options" argument must be of type object.`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const alpn = __normAlpn(options.alpn, "options.alpn", true);
  // 校验提在 native 调用之外（包进 __callNative 会被重包成 ERR_QUIC_ERROR）。
  const servername = options.servername === undefined ? undefined : __needStr(options.servername, "options.servername");
  const caPem = options.ca === undefined ? undefined : String(options.ca);
  const idleMs = __normIdle(options.idleTimeout);
  const ccName = __normCc(options.cc);
  const id = __callNative(() => __wjs2_quic_connect(JSON.stringify({
    host, port, alpn,
    servername,
    ca_pem: caPem,
    reject_unauthorized: options.rejectUnauthorized,
    idle_timeout_ms: idleMs, cc: ccName,
  })));
  return new QuicSession(String(id), {});
}

const __api = { listen, connect, QuicEndpoint, QuicSession, QuicStream, QuicError, CC_ALGO_RENO, CC_ALGO_CUBIC, CC_ALGO_BBR };
export default __api;
