import { EventEmitter } from "node:events";
import { Readable, Writable, Duplex } from "node:stream";
import { codes } from "node:internal/errors";
import { kSocket } from "node:internal/http2/util";
import { addAbortListener } from "node:internal/events/abort_listener";
import * as net from "node:net";
import fs from "node:fs";
const Buffer = globalThis.Buffer;

function __b64dec(s) {
  const bin = atob(s);
  const u8 = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) u8[i] = bin.charCodeAt(i);
  return u8;
}
function __b64enc(u8) {
  let s = "";
  for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000));
  return btoa(s);
}
function __toU8(data, what) {
  if (typeof data === "string") return new TextEncoder().encode(data);
  if (data instanceof Uint8Array) return data;
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  throw new TypeError(`${what}: data must be string or BufferSource`);
}
function __h2Err(code, msg, name = "Error") {
  // 类按 name 取（node 的 RangeError/TypeError 系 instanceof 与 name 同真）。
  const C = name === "RangeError" ? RangeError : name === "TypeError" ? TypeError : Error;
  const e = new C(msg);
  e.code = code;
  e.name = name;
  return e;
}
// http2 专属错误码（errors.rs 内核表外补；消息逐字对齐 node/lib/internal/errors.js）
const __codes = {
  ERR_HTTP2_STATUS_INVALID: (s) => __h2Err("ERR_HTTP2_STATUS_INVALID", `Invalid status code: ${s}`, "RangeError"),
  ERR_HTTP2_INFO_STATUS_NOT_ALLOWED: () => __h2Err("ERR_HTTP2_INFO_STATUS_NOT_ALLOWED", "Informational status codes cannot be used", "RangeError"),
  ERR_HTTP2_INVALID_INFO_STATUS: (s) => __h2Err("ERR_HTTP2_INVALID_INFO_STATUS", `Invalid informational status code: ${s}`),
  ERR_HTTP2_INVALID_PSEUDOHEADER: (s) => __h2Err("ERR_HTTP2_INVALID_PSEUDOHEADER", `"${s}" is an invalid pseudoheader or is used incorrectly`, "TypeError"),
  ERR_HTTP2_PSEUDOHEADER_NOT_ALLOWED: () => __h2Err("ERR_HTTP2_PSEUDOHEADER_NOT_ALLOWED", "Cannot set HTTP/2 pseudo headers after regular headers", "TypeError"),
  ERR_HTTP2_HEADER_SINGLE_VALUE: (s) => __h2Err("ERR_HTTP2_HEADER_SINGLE_VALUE", `Header field "${s}" must only have a single value`, "TypeError"),
  ERR_HTTP2_TRAILERS_CANNOT_BE_SENT: () => __h2Err("ERR_HTTP2_TRAILERS_CANNOT_BE_SENT", "Trailers cannot be sent at this stage."),
  ERR_HTTP2_TRAILERS_ALREADY_SENT: () => __h2Err("ERR_HTTP2_TRAILERS_ALREADY_SENT", "Trailers has already been sent."),
  ERR_HTTP2_TRAILERS_NOT_READY: () => __h2Err("ERR_HTTP2_TRAILERS_NOT_READY", "Trailing headers cannot be sent until after the wantTrailers event is emitted"),
  ERR_HTTP2_PUSH_DISABLED: () => __h2Err("ERR_HTTP2_PUSH_DISABLED", "Push streams are not enabled on this session."),
  ERR_HTTP2_NESTED_PUSH: () => __h2Err("ERR_HTTP2_NESTED_PUSH", "A push stream cannot be initiated from within a push stream."),
  ERR_HTTP2_GOAWAY_SESSION: () => __h2Err("ERR_HTTP2_GOAWAY_SESSION", "New streams cannot be created after receiving a GOAWAY"),
  ERR_HTTP2_SESSION_ERROR: (n) => __h2Err("ERR_HTTP2_SESSION_ERROR", `Session closed with error code ${n}`),
  ERR_HTTP2_MAX_PENDING_SETTINGS_ACK: () => __h2Err("ERR_HTTP2_MAX_PENDING_SETTINGS_ACK", "Maximum concurrent SETTINGS frames not acknowledged"),
  ERR_HTTP2_INVALID_SETTING_VALUE: (name, v) => __h2Err("ERR_HTTP2_INVALID_SETTING_VALUE", `Invalid value for setting "${name}": ${v}`, "RangeError"),
  ERR_HTTP2_PAYLOAD_FORBIDDEN: (s) => __h2Err("ERR_HTTP2_PAYLOAD_FORBIDDEN", `Responses with ${s} status must not have a payload`),
  ERR_HTTP2_NO_PAYLOAD: () => __h2Err("ERR_HTTP2_NO_PAYLOAD", "No payload supplied"),
  ERR_HTTP2_INVALID_PACKED_SETTINGS_LENGTH: () => __h2Err("ERR_HTTP2_INVALID_PACKED_SETTINGS_LENGTH", "Packed settings length must be a multiple of six", "RangeError"),
  ERR_HTTP2_INVALID_PROTOCOL: (v, a) => __h2Err("ERR_HTTP2_INVALID_PROTOCOL", `Protocol "${v}" does not contain "${a}"`),
  ERR_HTTP2_SEND_FILE: () => __h2Err("ERR_HTTP2_SEND_FILE", "Filename passed to sendFile must be absolute"),
  ERR_HTTP2_SEND_FILE_NOSEEK: () => __h2Err("ERR_HTTP2_SEND_FILE_NOSEEK", "Offset or length can only be specified for regular files"),
  ERR_HTTP2_PING_CANCEL: () => __h2Err("ERR_HTTP2_PING_CANCEL", "HTTP2 ping cancelled"),
  ABORT_ERR: () => {
    const e = __h2Err("ABORT_ERR", "This operation was aborted");
    e.name = "AbortError";
    return e;
  },
  ERR_HTTP2_PING_LENGTH: () => __h2Err("ERR_HTTP2_PING_LENGTH", "HTTP2 ping payload must be 8 bytes", "RangeError"),
  ERR_HTTP2_TOO_MANY_INVALID_FRAMES: (s) => __h2Err("ERR_HTTP2_TOO_MANY_INVALID_FRAMES", `Too many invalid HTTP/2 frames: ${s}`),
  ERR_HTTP2_FRAME_ERROR: (s) => __h2Err("ERR_HTTP2_FRAME_ERROR", `HTTP/2 frame error: ${s}`),
  ERR_HTTP2_STREAM_CLOSED: () => __h2Err("ERR_HTTP2_STREAM_CLOSED", "The stream has been destroyed"),
  // node：reason 实参只进 cause，message 恒此串（真机 26.8.2 实测）
  ERR_HTTP2_STREAM_CANCEL: () => __h2Err("ERR_HTTP2_STREAM_CANCEL", "The pending stream has been canceled"),
  ERR_HTTP2_INVALID_SESSION: () => __h2Err("ERR_HTTP2_INVALID_SESSION", "The session has been destroyed"),
  ERR_HTTP2_SOCKET_UNBOUND: () => __h2Err("ERR_HTTP2_SOCKET_UNBOUND", "The socket has been unbound from the session."),
  ERR_HTTP2_OUT_OF_BUFFERS: () => __h2Err("ERR_HTTP2_OUT_OF_BUFFERS", "Out of buffers"),
  ERR_HTTP2_HEADERS_OBJECT: () => __h2Err("ERR_HTTP2_HEADERS_OBJECT", "Headers must be an object"),
  ERR_HTTP2_CONNECT_AUTHORITY: () => __h2Err("ERR_HTTP2_CONNECT_AUTHORITY", ":authority header is required for CONNECT requests"),
  ERR_HTTP2_CONNECT_PATH: () => __h2Err("ERR_HTTP2_CONNECT_PATH", "The :path header is forbidden for CONNECT requests"),
  ERR_HTTP2_CONNECT_SCHEME: () => __h2Err("ERR_HTTP2_CONNECT_SCHEME", "The :scheme header is forbidden for CONNECT requests"),
  ERR_HTTP2_HEADERS_AFTER_RESPOND: () => __h2Err("ERR_HTTP2_HEADERS_AFTER_RESPOND", "Cannot specify additional headers after response has initiated"),
  ERR_HTTP2_INVALID_HEADER_VALUE: (v, n) => __h2Err("ERR_HTTP2_INVALID_HEADER_VALUE", `Invalid header value: "${v}" for header "${n}"`),
  ERR_HTTP2_NO_SOCKET_MANIPULATION: () => __h2Err("ERR_HTTP2_NO_SOCKET_MANIPULATION", "HTTP/2 sockets should not be directly manipulated (e.g. read and written)"),
  ERR_HTTP2_STREAM_SELF_DEPENDENCY: () => __h2Err("ERR_HTTP2_STREAM_SELF_DEPENDENCY", "A stream cannot depend on itself"),
  ERR_HTTP2_INVALID_STREAM: () => __h2Err("ERR_HTTP2_INVALID_STREAM", "The stream has been destroyed"),
  ERR_HTTP2_HEADERS_SENT: () => __h2Err("ERR_HTTP2_HEADERS_SENT", "Response has already been initiated."),
};
// node core.js：Http2Stream.priority 在 RFC 9113 废止优先级信令后弃用（DEP0194，进程级一次）。
let __h2PriorityWarned = false;
function __h2PriorityDeprecate() {
  if (__h2PriorityWarned) return;
  __h2PriorityWarned = true;
  process.emitWarning(
    "http2Stream.priority is longer supported after priority signalling was deprecated in RFC 9113",
    "DeprecationWarning", "DEP0194");
}
let __h2ReqPriorityWarned = false;
function __h2RequestPriorityDeprecate() {
  if (__h2ReqPriorityWarned) return;
  __h2ReqPriorityWarned = true;
  process.emitWarning("Priority signaling has been deprecated as of RFC 9113.", "DeprecationWarning", "DEP0194");
}
function __code(name, ...args) {
  const f = codes[name];
  // E() 工厂是 class（必须 new；返回对象的工厂 new 后同样取返回对象）
  if (typeof f === "function") { try { return new f(...args); } catch { /* fallthrough */ } }
  const g = __codes[name];
  if (typeof g === "function") return g(...args);
  return __h2Err(name, args.length ? String(args[0]) : name);
}
// node toHeaderObject 口径：重复值 cookie 以 "; " 并串、set-cookie 保数组、
// 其余以 ", " 并串（cookies 套件实测：abc → "1, 2, 3"、cookie → "a=b; c=d; e=f"）
function __pairsToObj(pairs) {
  const out = Object.create(null);
  for (const [k, v] of pairs) {
    const lk = String(k).toLowerCase();
    const val = String(v);
    if (out[lk] === undefined) out[lk] = val;
    else if (lk === "cookie") out[lk] = `${out[lk]}; ${val}`;
    else if (lk === "set-cookie") {
      out[lk] = Array.isArray(out[lk]) ? [...out[lk], val] : [out[lk], val];
    } else out[lk] = `${out[lk]}, ${val}`;
  }
  return out;
}
// node checkIsHttpToken 同款（header 名校验）
const __TOKEN_RE = /^[\^_`a-zA-Z\-0-9!#$%&'*+.|~]+$/;
function __validateHeaderName(name) {
  if (typeof name !== "string") {
    throw __code("ERR_INVALID_ARG_TYPE", "name", "string", name);
  }
  if (!__TOKEN_RE.test(name)) {
    throw __code("ERR_INVALID_HTTP_TOKEN", name);
  }
}
function __validateHeaderValue(name, value) {
  if (value === undefined || value === null) {
    throw __code("ERR_HTTP2_INVALID_HEADER_VALUE", value, name);
  }
  if (Array.isArray(value)) {
    for (const v of value) __validateHeaderValue(name, v);
    return;
  }
  if (typeof value !== "string" && typeof value !== "number" && typeof value !== "boolean") {
    throw __code("ERR_INVALID_ARG_TYPE", `header "${name}"`, "string|number|boolean", value);
  }
}
function __headerToWire(value) {
  if (Array.isArray(value)) return value.map((v) => String(v));
  return String(value);
}

// ── internal/http2/util 同款校验（套件直接可对齐；10g 欠账 G10）──────────────
const __PSEUDO_RE = /^:[a-zA-Z0-9_]+$/;
function assertValidPseudoHeader(key) {
  if (!__PSEUDO_RE.test(key)) throw __code("ERR_HTTP2_INVALID_PSEUDOHEADER", key);
}
function assertIsObject(val, name, type) {
  if (val === undefined || val === null || (typeof val !== "object" && typeof val !== "function") || Array.isArray(val)) {
    throw __code("ERR_INVALID_ARG_TYPE", name, type ?? "Object", val);
  }
}
function assertWithinRange(name, value, min, max) {
  if (typeof value !== "number" || !Number.isInteger(value) || value < min || value > max) {
    throw __code("ERR_HTTP2_INVALID_SETTING_VALUE", name, value);
  }
}
// connection-specific 头（RFC 7540 §8.1.2.2，node isConnectionSpecificHeader 同表）
const __CONN_HEADERS = new Set(["connection", "upgrade", "http2-settings", "te", "transfer-encoding", "keep-alive", "proxy-connection"]);
// node validateH2Headers：伪头白名单（respond 只允许 :status；trailers 全禁）+ 单值
function __validateH2Headers(headers, allowedPseudo = []) {
  if (headers === null || typeof headers !== "object") throw __code("ERR_HTTP2_HEADERS_OBJECT");
  for (const key of Object.keys(headers)) {
    const lk = String(key).toLowerCase();
    if (lk.startsWith(":")) {
      if (!allowedPseudo.includes(lk)) throw __code("ERR_HTTP2_INVALID_PSEUDOHEADER", lk);
      const v = headers[key];
      if (Array.isArray(v)) {
        if (v.length !== 1) throw __code("ERR_HTTP2_HEADER_SINGLE_VALUE", key);
      } else if (v === undefined) {
        throw __code("ERR_HTTP2_INVALID_HEADER_VALUE", v, key);
      }
    } else {
      __validateHeaderName(String(key));
      __validateHeaderValue(String(key), headers[key]);
    }
  }
}
// node util.js kSingleValueHeaders：大小写变体重复或数组多值即 HEADER_SINGLE_VALUE。
const __SINGLE_VALUE_HEADERS = new Set([
  ":status", ":method", ":authority", ":scheme", ":path", ":protocol",
  "access-control-allow-credentials", "access-control-max-age", "access-control-request-method",
  "age", "authorization", "content-encoding", "content-language", "content-length",
  "content-location", "content-md5", "content-range", "content-type", "date", "dnt", "etag",
  "expires", "from", "host", "if-match", "if-modified-since", "if-none-match", "if-range",
  "if-unmodified-since", "last-modified", "location", "max-forwards", "proxy-authorization",
  "range", "referer", "retry-after", "tk", "upgrade-insecure-requests", "user-agent",
  "x-content-type-options",
]);
// node mapToHeaders 的校验面：非伪头名须是 HTTP token（ERR_INVALID_HTTP_TOKEN），单值头不得重复。
const __REQ_PSEUDO = new Set([":method", ":authority", ":scheme", ":path", ":protocol"]);
function __mapToHeadersCheck(headers) {
  const seen = new Set();
  for (const key of Object.keys(headers)) {
    const lk = String(key).toLowerCase();
    if (lk.startsWith(":") && !__REQ_PSEUDO.has(lk)) throw __code("ERR_HTTP2_INVALID_PSEUDOHEADER", lk);
  }
  for (const key of Object.keys(headers)) {
    const lk = String(key).toLowerCase();
    const v = headers[key];
    if (__SINGLE_VALUE_HEADERS.has(lk)) {
      if (seen.has(lk) || (Array.isArray(v) && v.length > 1)) throw __code("ERR_HTTP2_HEADER_SINGLE_VALUE", lk);
      seen.add(lk);
    }
    if (!lk.startsWith(":") && lk !== "x-wjs-pho") __validateHeaderName(String(key));
  }
}
// node request() 选项类型门（validateBoolean/validateNumber 同码 ERR_INVALID_ARG_TYPE）。
function __validateRequestOptions(options) {
  if (options === undefined || options === null) return;
  for (const [k, t] of [["endStream", "boolean"], ["exclusive", "boolean"], ["silent", "boolean"],
    ["waitForTrailers", "boolean"], ["parent", "number"], ["weight", "number"]]) {
    const v = options[k];
    if (v !== undefined && typeof v !== t) throw __code("ERR_INVALID_ARG_TYPE", `options.${k}`, t, v);
  }
}
// 设置项取值/校验（node updateSettingsInternal 同口径；customSettings 放行）
const __SETTING_RANGES = {
  headerTableSize: [0, 0xffffffff],
  enablePush: "boolean",
  initialWindowSize: [0, 0x7fffffff],
  maxFrameSize: [16384, 16777215],
  maxConcurrentStreams: [0, 0xffffffff],
  maxHeaderListSize: [0, 0xffffffff],
  maxHeaderSize: [0, 0xffffffff],
  enableConnectProtocol: "boolean",
};
function __settingErr(name, v, isBool) {
  // node：数值档 RangeError、布尔档 TypeError（getpackedsettings 套件逐字）
  const e = __h2Err("ERR_HTTP2_INVALID_SETTING_VALUE", `Invalid value for setting "${name}": ${v}`, isBool ? "TypeError" : "RangeError");
  return e;
}
function __validateSettings(settings, argName = "settings") {
  if (settings === null || typeof settings !== "object") {
    throw __code("ERR_INVALID_ARG_TYPE", argName, "object", settings);
  }
  const out = {};
  for (const key of Object.keys(settings)) {
    const v = settings[key];
    const spec = __SETTING_RANGES[key];
    if (spec === undefined) {
      if (key === "customSettings") {
        out[key] = { ...v };
        continue;
      }
      continue; // 未知设置项忽略（node 静默忽略未知名——non-critical）
    }
    if (spec === "boolean") {
      if (typeof v !== "boolean") throw __settingErr(key, v, true);
      out[key] = v;
    } else if (typeof v !== "number" || !Number.isInteger(v) || v < spec[0] || v > spec[1]) {
      throw __settingErr(key, v, false);
    } else {
      out[key] = v;
    }
  }
  return out;
}
function __applySettings(base, extra) {
  return { ...base, ...extra };
}
const __DEFAULT_SETTINGS = {
  headerTableSize: 4096,
  enablePush: true,
  initialWindowSize: 65535,
  maxFrameSize: 16384,
  maxConcurrentStreams: 4294967295,
  maxHeaderListSize: 65535,
  enableConnectProtocol: false,
};

// ── socket 代理（node socketProxyPair 口径：net.Socket 假面 + 流委派）────────
const __SOCK_MANIP_KEYS = ["read", "write", "pause", "resume"];
function __socketManipErr() {
  return __code("ERR_HTTP2_NO_SOCKET_MANIPULATION");
}
function __mkSocketProxy(stream, server, peerObj) {
  const base = Object.create(net.Socket.prototype);
  Object.defineProperty(base, "connecting", { value: false, writable: true, configurable: true });
  const handler = {
    get(t, prop) {
      if (prop === "readable" || prop === "writable") return stream[prop];
      if (prop === "destroyed") return stream.__destroyed;
      if (__SOCK_MANIP_KEYS.includes(prop)) throw __socketManipErr();
      if (prop === "on" || prop === "once" || prop === "emit" ||
          prop === "end") {
        return stream[prop].bind(stream);
      }
      if (prop === "destroy") {
        // node：socket.destroy() 杀整个连接（stream.session.destroy 同收尾）
        return (...args) => {
          stream.destroy(...args);
          stream.session.destroy();
        };
      }
      if (prop === "setTimeout") return stream.session.setTimeout.bind(stream.session);
      if (prop === "address") return () => server.address();
      if (prop === "remoteAddress") return peerObj.addr;
      if (prop === "remotePort") return peerObj.port;
      if (prop === "localAddress") return server.__listening ? server.__listening.address : undefined;
      if (prop === "localPort") return server.__listening ? server.__listening.port : undefined;
      if (prop === "_server") return server;
      const v = Reflect.get(t, prop);
      if (v !== undefined || prop in t) return v;
      return server.__sockBag[prop];
    },
    set(t, prop, value) {
      if (prop === "readable") { stream.readable = !!value; return true; }
      if (prop === "writable") { stream.writable = !!value; return true; }
      if (prop === "destroyed") { stream.__destroyed = !!value; return true; }
      if (__SOCK_MANIP_KEYS.includes(prop)) throw __socketManipErr();
      if (prop === "on" || prop === "once" || prop === "emit" ||
          prop === "end" || prop === "destroy") {
        stream[prop] = value;
        return true;
      }

      if (prop === "setTimeout") { stream.session.setTimeout = value; return true; }
      t[prop] = value;
      server.__sockBag[prop] = value;
      return true;
    },
  };
  return new Proxy(base, handler);
}

// ── 会话级 socket（session.socket：EventEmitter 假面 + peer 反射）────────────
function __mkSessionSocket(session, peerObj, localObj) {
  const sock = new EventEmitter();
  sock.remoteAddress = peerObj.addr;
  sock.remotePort = peerObj.port;
  sock.localAddress = localObj?.address;
  sock.localPort = localObj?.port;
  sock.remoteFamily = peerObj.addr?.includes(":") ? "IPv6" : "IPv4";
  sock.connecting = false;
  sock.destroyed = false;
  sock.readable = true;
  sock.writable = true;
  sock.destroy = (err) => {
    if (sock.destroyed) return;
    sock.destroyed = true;
    if (err) sock.emit("error", err);
    session.destroy();
  };
  sock.end = () => { session.close(); return sock; };
  sock.write = () => true;
  setTimeout(() => { if (!session.destroyed) sock.emit("connect"); }, 0);
  return sock;
}

// ── Http2Session（服务端每连接一个；'session' 事件载体）──────────────────────
let __h2SessionSeq = 1;
class Http2Session extends EventEmitter {
  constructor(server, connId, peerObj) {
    super();
    this.__id = __h2SessionSeq++;
    this.__server = server;
    this.__conn = connId;
    this.type = 0; // NGHTTP2_SESSION_SERVER
    this.encrypted = !!server.__secure;
    this.connecting = false;
    this.destroyed = false;
    this.closed = false;
    this.__peer = peerObj ?? { addr: undefined, port: 0 };
    this.__settings = { ...__DEFAULT_SETTINGS, ...(server.__opts?.settings ?? {}) };
    this.__remoteSettings = { ...__DEFAULT_SETTINGS };
    this.__pendingSettingsAck = false;
    this.__outstandingSettings = 0;
    this.__maxOutstandingSettings = server.__opts?.maxOutstandingSettings ?? Infinity;
    // node 缺省 maxOutstandingPings = 2（ping.js 超额即 CANCEL）
    this.__maxOutstandingPings = server.__opts?.maxOutstandingPings ?? 2;
    this.__streams = new Map();
    this.state = {
      effectiveLocalWindowSize: 65535,
      effectiveRemoteWindowSize: 65535,
      localWindowSize: 65535,
      remoteWindowSize: 65535,
      outboundQueueSize: 0,
      deflateDynamicTableSize: 4096,
      inflateDynamicTableSize: 4096,
    };
    this.__ev = this.__ev.bind(this);
  }
  get socket() {
    if (this[kSocket] === undefined) {
      this[kSocket] = __mkSessionSocket(this, this.__peer, this.__server.__listening);
    }
    return this[kSocket];
  }
  get localSettings() { return this.__settings; }
  get remoteSettings() { return this.__remoteSettings; }
  get pendingSettingsAck() { return this.__pendingSettingsAck; }
  get originSet() { return this.encrypted ? undefined : undefined; }
  get alpnProtocol() { return this.encrypted ? "h2" : false; }
  get unrefed() { return false; }
  setTimeout(msecs, callback) {
    if (typeof msecs !== "number") throw __code("ERR_INVALID_ARG_TYPE", "msecs", "number", msecs);
    if (callback !== undefined && typeof callback !== "function") {
      throw __code("ERR_INVALID_ARG_TYPE", "callback", "function", callback);
    }
    if (typeof callback === "function") this.once("timeout", callback);
    const ms = Number(msecs) || 0;
    if (this.__timeoutTimer !== undefined) clearTimeout(this.__timeoutTimer);
    if (ms > 0 && !this.closed && !this.destroyed) {
      this.__timeoutTimer = setTimeout(() => {
        this.__timeoutTimer = undefined;
        this.emit("timeout");
      }, ms);
    }
    return this;
  }
  ref() { if (this.__conn) __wjs_net_ref(this.__conn); return this; }
  unref() { if (this.__conn) __wjs_net_unref(this.__conn); return this; }
  settings(settings = {}, cb) {
    const validated = __validateSettings(settings);
    if (this.destroyed) throw __code("ERR_HTTP2_INVALID_SESSION");
    this.__pendingSettingsAck = true;
    this.__outstandingSettings++;
    if (Number.isFinite(this.__maxOutstandingSettings) &&
        this.__outstandingSettings >= this.__maxOutstandingSettings) {
      this.__outstandingSettings = 0;
      this.__pendingSettingsAck = false;
      process.nextTick(() => {
        this.emit("error", __code("ERR_HTTP2_MAX_PENDING_SETTINGS_ACK"));
        this.destroy();
      });
      return this;
    }
    setTimeout(() => {
      this.__outstandingSettings = Math.max(0, this.__outstandingSettings - 1);
      if (this.__outstandingSettings === 0) this.__pendingSettingsAck = false;
      this.__settings = __applySettings(this.__settings, validated);
      if (!this.destroyed) {
        this.emit("localSettings", this.__settings);
        if (typeof cb === "function") cb();
      }
    }, 1);
    return this;
  }
  updateSettings(settings) {
    const validated = __validateSettings(settings);
    this.__settings = __applySettings(this.__settings, validated);
    return this;
  }
  // node：windowSize 校验（number、0..2^31-1）后仅本地状态面（flow control
  // 由底座管理；setLocalWindowSize-errors/套件只断言校验与 state 反映）
  setLocalWindowSize(windowSize) {
    // node：destroyed 校验先于实参校验（client-destroy 套件）
    if (this.destroyed) throw __code("ERR_HTTP2_INVALID_SESSION");
    if (typeof windowSize !== "number") {
      throw __code("ERR_INVALID_ARG_TYPE", "windowSize", "number", windowSize);
    }
    if (windowSize < 0 || windowSize > 2147483647) {
      throw __code("ERR_OUT_OF_RANGE", "windowSize", ">= 0 && <= 2147483647", windowSize);
    }
    this.state.effectiveLocalWindowSize = windowSize;
    this.state.localWindowSize = windowSize;
    return this;
  }
  // node 口径：ping([payload, ]callback)；payload 非 ArrayBufferView →
  // TypeError、长度≠8 → RangeError ERR_HTTP2_PING_LENGTH、超 maxOutstandingPings
  // → 返 false 且回调 ERR_HTTP2_PING_CANCEL（真机 ping.js/onping 实测）
  ping(payload, callback) {
    if (typeof payload === "function") { callback = payload; payload = undefined; }
    if (this.destroyed) throw __code("ERR_HTTP2_INVALID_SESSION");
    let buf = null;
    if (payload !== undefined && payload !== null) {
      if (typeof payload !== "object" || !ArrayBuffer.isView(payload)) {
        throw __code("ERR_INVALID_ARG_TYPE", "payload", ["Buffer", "TypedArray", "DataView"], payload);
      }
      const u8 = payload instanceof Uint8Array ? payload : new Uint8Array(payload.buffer, payload.byteOffset, payload.byteLength);
      if (u8.length !== 8) throw __code("ERR_HTTP2_PING_LENGTH");
      buf = u8;
    }
    if (typeof callback !== "function") {
      throw __code("ERR_INVALID_ARG_TYPE", "callback", "function", callback);
    }
    const cap = Number.isInteger(this.__maxOutstandingPings) ? this.__maxOutstandingPings : 2;
    if ((this.__outstandingPings ?? 0) >= cap) {
      const cancel = __code("ERR_HTTP2_PING_CANCEL");
      queueMicrotask(() => callback(cancel));
      return false;
    }
    this.__outstandingPings = (this.__outstandingPings ?? 0) + 1;
    const ret = Buffer.alloc(8);
    if (buf) Buffer.from(buf.buffer, buf.byteOffset, buf.byteLength).copy(ret);
    setTimeout(() => {
      this.__outstandingPings = Math.max(0, (this.__outstandingPings ?? 1) - 1);
      if (this.destroyed) { callback(__code("ERR_HTTP2_PING_CANCEL")); return; }
      callback(null, 1, ret);
    }, 1);
    return true;
  }
  goaway(code = 0, lastStreamID = 0, opaqueData) {
    if (this.destroyed) throw __code("ERR_HTTP2_INVALID_SESSION");
    if (typeof code === "object" && code !== null) {
      // goaway(options) 形（node：{errorCode, lastStreamID, opaqueData}）
      const o = code;
      code = o.errorCode ?? 0;
      lastStreamID = o.lastStreamID ?? 0;
      opaqueData = o.opaqueData;
    }
    this.__goaway = { code, lastStreamID, opaqueData };
    // 底座无 GOAWAY 帧下发（偏差记档）：本会话立即收尾（node goaway 后不再收流）。
    setImmediate(() => this.close());
    return this;
  }
  setNextStreamID(id) {
    if (this.destroyed) throw __code("ERR_HTTP2_INVALID_SESSION");
    if (typeof id !== "number" || !Number.isInteger(id) || id < 0 || id > 2147483647) {
      throw __code("ERR_OUT_OF_RANGE", "id", id);
    }
    this.__nextStreamID = id;
    return this;
  }
  altsvc(alt, origin) {
    // ALTSVC 帧无底座（偏差记档）：参数校验后 no-op
    if (typeof alt === "string" || alt === undefined) return this;
    throw __code("ERR_INVALID_ARG_TYPE", "alt", "string", alt);
  }
  origin(...origins) { return this; }
  __registerStream(stream) {
    this.__streams.set(stream.id, stream);
  }
  __unregisterStream(stream) {
    this.__streams.delete(stream.id);
  }
  close(cb) {
    if (typeof cb === "function") this.once("close", cb);
    if (this.closed || this.destroyed) {
      if (typeof cb === "function") process.nextTick(cb);
      return this;
    }
    this.closed = true;
    if (this.__conn) {
      __wjs_net_destroy(this.__conn);
    } else {
      setImmediate(() => this.__finish());
    }
    return this;
  }
  destroy(code = 0, cb) {
    if (typeof code === "function") { cb = code; code = 0; }
    if (typeof cb === "function") this.once("close", cb);
    if (this.destroyed) return this;
    this.destroyed = true;
    this.close();
    return this;
  }
  __finish() {
    if (this.__finished) return;
    this.__finished = true;
    this.closed = true;
    this.destroyed = true;
    for (const [, st] of this.__streams) {
      if (!st.destroyed) st.destroy();
    }
    this.__streams.clear();
    queueMicrotask(() => this.emit("close"));
  }
  __ev(kind, payload) {
    switch (kind) {
      case "connClose": {
        this.__server.__sessions.delete(this.__conn);
        this.__finish();
        break;
      }
    }
  }
}
