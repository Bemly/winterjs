//! `node:http`：HTTP/1.1 Server/Client——**纯 JS，架在 node:net 之上**（§9d-3
//! 切片决策，2026-09-12）。Node 本尊也是 net + 独立解析器的结构；此处解析器为
//! JS 子集（请求/响应头、Content-Length、chunked 双向解码），零新 native、
//! 零新依赖（hyper 直引/httparse 行级增补继续挂起待拍板，不需要）。
/// 偏差记档：无 keep-alive 连接复用（server 响应带 `connection: close` 并关连接，
/// 客户端同发 `connection: close`）——`test-simple-http-*` 单请求子集不受影响；
/// 头重复值后者覆盖（Node 为 set-cookie 例外数组）；请求/响应体一次性整收后
/// 按 'data'→'end' 派发；IncomingMessage/OutgoingMessage 非 node:stream 全家
/// （EventEmitter data/end/finish 形状）；chunked trailer 忽略。

/// 内嵌 ESM 源（`node:http`；net 底座 + JS 解析器）。
pub const SOURCE: &str = r#"
import * as net from "node:net";
import { EventEmitter } from "node:events";

// ── 常量面 ──────────────────────────────────────────────────────────────────
export const STATUS_CODES = {
  100: "Continue", 101: "Switching Protocols", 102: "Processing", 103: "Early Hints",
  200: "OK", 201: "Created", 202: "Accepted", 203: "Non-Authoritative Information",
  204: "No Content", 205: "Reset Content", 206: "Partial Content", 207: "Multi-Status",
  208: "Already Reported", 226: "IM Used",
  300: "Multiple Choices", 301: "Moved Permanently", 302: "Found", 303: "See Other",
  304: "Not Modified", 305: "Use Proxy", 307: "Temporary Redirect", 308: "Permanent Redirect",
  400: "Bad Request", 401: "Unauthorized", 402: "Payment Required", 403: "Forbidden",
  404: "Not Found", 405: "Method Not Allowed", 406: "Not Acceptable",
  407: "Proxy Authentication Required", 408: "Request Timeout", 409: "Conflict",
  410: "Gone", 411: "Length Required", 412: "Precondition Failed",
  413: "Payload Too Large", 414: "URI Too Long", 415: "Unsupported Media Type",
  416: "Range Not Satisfiable", 417: "Expectation Failed", 418: "I'm a Teapot",
  421: "Misdirected Request", 422: "Unprocessable Entity", 423: "Locked",
  424: "Failed Dependency", 425: "Too Early", 426: "Upgrade Required",
  428: "Precondition Required", 429: "Too Many Requests", 431: "Request Header Fields Too Large",
  451: "Unavailable For Legal Reasons",
  500: "Internal Server Error", 501: "Not Implemented", 502: "Bad Gateway",
  503: "Service Unavailable", 504: "Gateway Timeout", 505: "HTTP Version Not Supported",
  506: "Variant Also Negotiates", 507: "Insufficient Storage", 508: "Loop Detected",
  510: "Not Extended", 511: "Network Authentication Required",
};
export const METHODS = ["ACL", "BIND", "CHECKOUT", "CONNECT", "COPY", "DELETE", "GET",
  "HEAD", "LINK", "LOCK", "M-SEARCH", "MERGE", "MKACTIVITY", "MKCALENDAR", "MKCOL",
  "MOVE", "NOTIFY", "OPTIONS", "PATCH", "POST", "PROPFIND", "PROPPATCH", "PURGE",
  "PUT", "QUERY", "REBIND", "REPORT", "SEARCH", "SOURCE", "SUBSCRIBE", "TRACE",
  "UNBIND", "UNLINK", "UNLOCK", "UNSUBSCRIBE"];
export const maxHeaderSize = 16384;

// ── 解析器（JS 子集；字节级安全，头按 latin1 解码）──────────────────────────
function __concat(a, b) {
  if (a.length === 0) return b;
  if (b.length === 0) return a;
  const out = new Uint8Array(a.length + b.length);
  out.set(a, 0); out.set(b, a.length);
  return out;
}
function __join(parts) {
  return parts.reduce(__concat, new Uint8Array(0));
}
function __latin1(u8) { return new TextDecoder("latin1").decode(u8); }
function __findHeadEnd(u8) {
  for (let i = 0; i + 3 < u8.length; i++) {
    if (u8[i] === 13 && u8[i + 1] === 10 && u8[i + 2] === 13 && u8[i + 3] === 10) return i;
  }
  return -1;
}
// 头块（latin1 串）→ { first, headers（小写键）, rawHeaders }
function __parseHead(headText) {
  const lines = headText.split("\r\n");
  const first = lines.shift().split(" ");
  const headers = Object.create(null);
  const rawHeaders = [];
  for (const line of lines) {
    if (line === "") continue;
    const c = line.indexOf(":");
    if (c <= 0) continue;
    const k = line.slice(0, c).trim();
    const v = line.slice(c + 1).trim();
    rawHeaders.push(k, v);
    const lk = k.toLowerCase();
    headers[lk] = headers[lk] === undefined ? v : `${headers[lk]}, ${v}`;
  }
  return { first, headers, rawHeaders };
}
// CL 定长体；返回 { body, rest } 或 null（未齐）
function __takeBody(u8, headEnd, contentLength) {
  const bodyStart = headEnd + 4;
  const total = bodyStart + contentLength;
  if (u8.length < total) return null;
  return { body: u8.slice(bodyStart, total), rest: u8.slice(total) };
}
// chunked 解码；返回 { body, rest } 或 null（未齐）/ undefined（坏帧）；trailer 忽略
function __decodeChunked(u8, bodyStart) {
  let pos = bodyStart;
  const parts = [];
  while (true) {
    let eol = pos;
    while (eol + 1 < u8.length && !(u8[eol] === 13 && u8[eol + 1] === 10)) eol++;
    if (eol + 1 >= u8.length) return null;
    const size = parseInt(__latin1(u8.slice(pos, eol)).split(";")[0].trim(), 16);
    if (!Number.isInteger(size) || size < 0) return undefined;
    pos = eol + 2;
    if (size === 0) return { body: __join(parts), rest: u8.slice(pos + 2) };
    if (pos + size + 2 > u8.length) return null;
    parts.push(u8.slice(pos, pos + size));
    pos += size + 2; // 块后 CRLF
  }
}
function __toU8(data) {
  if (typeof data === "string") return new TextEncoder().encode(data);
  if (data instanceof Uint8Array) return data;
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  throw new TypeError("data must be string or BufferSource");
}
function __lowerHeaders(obj) {
  const out = Object.create(null);
  for (const [k, v] of Object.entries(obj ?? {})) out[k.toLowerCase()] = String(v);
  return out;
}

// ── IncomingMessage（请求/响应共用的事件式消息体）──────────────────────────
class IncomingMessage extends EventEmitter {
  constructor() {
    super();
    this.httpVersion = "1.1";
    this.method = null;       // 请求侧
    this.url = null;
    this.statusCode = null;   // 响应侧
    this.statusMessage = null;
    this.headers = Object.create(null);
    this.rawHeaders = [];
    this.complete = false;
    this.destroyed = false;
  }
  __feed(body) {
    if (body.length > 0) this.emit("data", globalThis.Buffer.from(body));
    this.complete = true;
    this.emit("end");
  }
  destroy(err) {
    if (this.destroyed) return this;
    this.destroyed = true;
    if (err) this.emit("error", err);
    this.emit("close");
    return this;
  }
}

// ── 服务端 ──────────────────────────────────────────────────────────────────
class ServerResponse extends EventEmitter {
  constructor(sock) {
    super();
    this.__sock = sock;
    this.statusCode = 200;
    this.statusMessage = undefined;
    this.__headers = Object.create(null);
    this.headersSent = false;
    this.__body = [];
    this.__total = 0;
    this.__done = false;
  }
  setHeader(name, value) { this.__headers[String(name).toLowerCase()] = String(value); return this; }
  getHeader(name) { return this.__headers[String(name).toLowerCase()]; }
  removeHeader(name) { delete this.__headers[String(name).toLowerCase()]; return this; }
  getHeaderNames() { return Object.keys(this.__headers); }
  hasHeader(name) { return this.__headers[String(name).toLowerCase()] !== undefined; }
  writeHead(status, ...rest) {
    const obj = rest.find((r) => r && typeof r === "object");
    const msg = rest.find((r) => typeof r === "string");
    this.statusCode = status;
    if (msg !== undefined) this.statusMessage = msg;
    Object.assign(this.__headers, __lowerHeaders(obj));
    return this;
  }
  write(chunk) {
    if (this.__done) throw new Error("ERR_STREAM_WRITE_AFTER_END: write after end");
    const u8 = __toU8(chunk);
    this.__body.push(u8);
    this.__total += u8.length;
    return true;
  }
  end(chunk) {
    if (this.__done) return this;
    if (chunk !== undefined && chunk !== null) this.write(chunk);
    this.__done = true;
    this.headersSent = true;
    const reason = this.statusMessage ?? STATUS_CODES[this.statusCode] ?? "";
    const head = [`HTTP/1.1 ${this.statusCode} ${reason}`.trimEnd()];
    if (this.__headers["content-length"] === undefined) {
      this.__headers["content-length"] = String(this.__total);
    }
    if (this.__headers["connection"] === undefined) {
      this.__headers["connection"] = "close"; // v1 无 keep-alive（记档）
    }
    for (const [k, v] of Object.entries(this.__headers)) head.push(`${k}: ${v}`);
    const headBytes = new TextEncoder().encode(head.join("\r\n") + "\r\n\r\n");
    const body = __join(this.__body);
    this.__sock.write(__concat(headBytes, body));
    this.__sock.end(); // connection: close 口径：响应毕即关（客户端 'close' 收尾）
    this.emit("finish");
    this.emit("close");
    return this;
  }
}

class Server extends net.Server {
  constructor(listener) {
    super();
    this.__closing = false;
    if (typeof listener === "function") this.on("request", listener);
    this.on("connection", (sock) => {
      let buf = new Uint8Array(0);
      sock.on("data", (chunk) => {
        if (this.__closing) return;
        try {
          buf = this.__feed(buf, chunk, sock);
        } catch {
          sock.destroy();
        }
      });
    });
  }
  // 解析喂入；返回剩余未消费字节。v1：一连接一请求，响应 end 后连接即关（记档）。
  __feed(buf, chunk, sock) {
    buf = __concat(buf, chunk);
    const headEnd = __findHeadEnd(buf);
    if (headEnd === -1) return buf;
    const headText = __latin1(buf.slice(0, headEnd));
    const { first, headers, rawHeaders } = __parseHead(headText);
    if (first.length < 3) throw new Error("bad request line");
    const req = new IncomingMessage();
    req.method = first[0];
    req.url = first[1];
    req.httpVersion = first[2].replace("HTTP/", "");
    req.headers = headers;
    req.rawHeaders = rawHeaders;
    const te = (headers["transfer-encoding"] || "").toLowerCase();
    let body = null;
    if (te.includes("chunked")) {
      const got = __decodeChunked(buf, headEnd + 4);
      if (got === null) return buf;
      if (got === undefined) { sock.destroy(); return new Uint8Array(0); }
      body = got.body;
    } else {
      const cl = Number(headers["content-length"] ?? 0);
      const got = __takeBody(buf, headEnd, Number.isFinite(cl) ? cl : 0);
      if (got === null) return buf;
      body = got.body;
    }
    const res = new ServerResponse(sock);
    if (this.__closing) { res.writeHead(503); res.end(); return new Uint8Array(0); }
    // 先派发 request（监听器就位）再喂体（data/end 时序，Node 口径）
    this.emit("request", req, res);
    req.__feed(body);
    return new Uint8Array(0);
  }
  close(cb) {
    this.__closing = true;
    if (typeof cb === "function") this.once("close", cb);
    super.close(); // net Server：ServerClose 事件派发 'close'（勿双发）
    return this;
  }
}

// ── 客户端 ──────────────────────────────────────────────────────────────────
class OutgoingMessage extends EventEmitter {
  constructor() {
    super();
    this.headersSent = false;
    this.writableEnded = false;
    this.destroyed = false;
  }
}

class ClientRequest extends OutgoingMessage {
  constructor(options, cb) {
    super();
    let host, port, path, method, userHeaders;
    if (typeof options === "string" || options instanceof URL) {
      const u = new URL(String(options));
      if (u.protocol !== "http:") {
        throw new Error(`ERR_INVALID_PROTOCOL: protocol '${u.protocol}' not supported (use node:https)`);
      }
      method = "GET";
      host = u.hostname;
      port = u.port ? Number(u.port) : 80;
      path = u.pathname + u.search;
      userHeaders = {};
    } else {
      method = (options.method ?? "GET").toUpperCase();
      host = options.host ?? options.hostname ?? "127.0.0.1";
      port = Number(options.port ?? 80);
      path = options.path ?? "/";
      if (!path.startsWith("/")) path = "/" + path;
      userHeaders = options.headers ?? {};
    }
    this.method = method;
    this.host = host;
    this.port = port;
    this.path = path;
    this.__headers = __lowerHeaders(userHeaders);
    if (this.__headers.host === undefined) {
      this.__headers.host = port === 80 ? host : `${host}:${port}`;
    }
    if (this.__headers.connection === undefined) this.__headers.connection = "close";
    this.__body = [];
    this.__connected = false;
    this.__buf = new Uint8Array(0);
    this.__res = null;
    if (typeof cb === "function") this.on("response", cb);
    this.__sock = net.connect(port, host);
    this.__sock.on("connect", () => {
      this.__connected = true;
      this.__maybeFlush();
    });
    this.__sock.on("data", (chunk) => this.__feed(chunk));
    this.__sock.on("error", (e) => {
      if (this.listenerCount("error") === 0) throw e;
      this.emit("error", e);
    });
  }
  setHeader(name, value) { this.__headers[String(name).toLowerCase()] = String(value); return this; }
  getHeader(name) { return this.__headers[String(name).toLowerCase()]; }
  removeHeader(name) { delete this.__headers[String(name).toLowerCase()]; return this; }
  getHeaderNames() { return Object.keys(this.__headers); }
  // v1：头+体一次成帧（connect 且 end 齐备后；体全部缓冲，无流式 chunked 上行）
  __maybeFlush() {
    if (!this.__connected || !this.writableEnded || this.headersSent) return;
    this.headersSent = true;
    const head = [`${this.method} ${this.path} HTTP/1.1`];
    let total = 0;
    for (const b of this.__body) total += b.length;
    if (this.__headers["content-length"] === undefined) {
      this.__headers["content-length"] = String(total);
    }
    for (const [k, v] of Object.entries(this.__headers)) head.push(`${k}: ${v}`);
    const headBytes = new TextEncoder().encode(head.join("\r\n") + "\r\n\r\n");
    const body = __join(this.__body);
    this.__sock.write(__concat(headBytes, body));
    this.__sock.end(); // 半关写端；响应由对端送达（connection: close 口径）
  }
  write(chunk) {
    if (this.writableEnded) throw new Error("ERR_STREAM_WRITE_AFTER_END: write after end");
    this.__body.push(__toU8(chunk));
    return true;
  }
  end(chunk) {
    if (this.writableEnded) return this;
    if (chunk !== undefined && chunk !== null) this.write(chunk);
    this.writableEnded = true;
    this.__maybeFlush();
    return this;
  }
  destroy(err) {
    if (this.destroyed) return this;
    this.destroyed = true;
    this.__sock.destroy();
    if (err) this.emit("error", err);
    return this;
  }
  __feed(chunk) {
    if (this.__res) {
      // 无 CL/TE 的响应：读到连接关闭为止
      if (this.__noLength && chunk.length > 0) this.__res.emit("data", chunk);
      return;
    }
    this.__buf = __concat(this.__buf, chunk);
    const headEnd = __findHeadEnd(this.__buf);
    if (headEnd === -1) return;
    const headText = __latin1(this.__buf.slice(0, headEnd));
    const { first, headers, rawHeaders } = __parseHead(headText);
    if (first.length < 3 || !first[0].startsWith("HTTP/")) {
      this.emit("error", new Error("HPE_INVALID_CONSTANT: invalid HTTP response line"));
      return;
    }
    const res = new IncomingMessage();
    res.statusCode = Number(first[1]);
    res.statusMessage = first.slice(2).join(" ");
    res.headers = headers;
    res.rawHeaders = rawHeaders;
    const te = (headers["transfer-encoding"] || "").toLowerCase();
    const cl = Number(headers["content-length"] ?? -1);
    const bodyStart = headEnd + 4;
    this.__res = res;
    if (te.includes("chunked")) {
      const got = __decodeChunked(this.__buf, bodyStart);
      if (got === null) return; // 未齐（后续块经 __res 分支丢弃，chunked 中间态见记档）
      this.emit("response", res);
      if (got) res.__feed(got.body);
      this.__finish();
      return;
    }
    this.emit("response", res);
    if (cl >= 0) {
      const got = __takeBody(this.__buf, headEnd, cl);
      if (got) res.__feed(got.body);
      this.__finish();
      return;
    }
    // 无 CL/TE：流式收至 EOF（'close' 兜底收尾）
    this.__noLength = true;
    if (this.__buf.length > bodyStart) res.emit("data", globalThis.Buffer.from(this.__buf.slice(bodyStart)));
  }
  __finish() {
    // end 已由 res.__feed 派发（体整收口径）；此处仅收连接与请求侧事件
    this.__res.complete = true;
    this.__sock.end();
    this.emit("close");
  }
}

export function request(options, cb) {
  const req = new ClientRequest(options, cb);
  // 无 CL 响应 / 服务端提前断开：EOF 时收尾
  req.__sock.on("close", () => {
    if (req.__res) {
      if (!req.__res.complete) {
        req.__res.complete = true;
        req.__res.emit("end");
        req.emit("close");
      }
    } else if (req.listenerCount("response") === 0) {
      req.emit("close");
    }
  });
  return req;
}
export function get(options, cb) {
  const req = request(options, cb);
  req.end();
  return req;
}

class Agent {
  constructor() { this.keepAlive = false; this.maxSockets = Infinity; }
  destroy() {}
}
const globalAgent = new Agent();
export { Agent, globalAgent };
export { Server, ServerResponse, IncomingMessage, ClientRequest, OutgoingMessage };
export function createServer(options, cb) { return new Server(options, cb); }

const __api = {
  STATUS_CODES, METHODS, maxHeaderSize, request, get, Agent, globalAgent,
  Server, ServerResponse, IncomingMessage, ClientRequest, OutgoingMessage, createServer,
};
export default __api;
"#;
