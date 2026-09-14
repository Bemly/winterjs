//! `node:internal/http_framing`：HTTP/1.1 共享帧层（`node:http` 与 `node:https`
//! 共用；Phase 9d-6 新建）。内容逐行取自 `node:http` 原 SOURCE（行为保真，
//! http 既有黑盒为回归网）：解析器子集 + IncomingMessage + ServerResponse +
//! `withHttpServer(Base)` 服务端混入 + `withClientRequest(open, flavor)` 客户端工厂。
//! 注入点仅两处：服务端基类（net.Server / tls.Server）、客户端开 socket 函数。

pub const SOURCE: &str = r#"
import { EventEmitter } from "node:events";

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
function __takeBody(u8, headEnd, contentLength) {
  const bodyStart = headEnd + 4;
  const total = bodyStart + contentLength;
  if (u8.length < total) return null;
  return { body: u8.slice(bodyStart, total), rest: u8.slice(total) };
}
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
    pos += size + 2;
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

export class IncomingMessage extends EventEmitter {
  constructor() {
    super();
    this.httpVersion = "1.1";
    this.method = null;
    this.url = null;
    this.statusCode = null;
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
  // 可读流最小面（ws/vite 等库直接调用；整收口径无缓冲，pause/resume 为
  // no-op，read 恒 null——M5 dev 实测 `stream.resume is not a function`）。
  pause() { return this; }
  resume() { return this; }
  read() { return null; }
  unshift() { return this; }
  destroy(err) {
    if (this.destroyed) return this;
    this.destroyed = true;
    if (err) this.emit("error", err);
    this.emit("close");
    return this;
  }
}

export class ServerResponse extends EventEmitter {
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
  // 可写流最小面（ws Sender 的 cork/uncork；本仓写直通无聚合，no-op）。
  cork() { return this; }
  uncork() { return this; }
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
      this.__headers["connection"] = "close";
    }
    for (const [k, v] of Object.entries(this.__headers)) head.push(`${k}: ${v}`);
    const headBytes = new TextEncoder().encode(head.join("\r\n") + "\r\n\r\n");
    const body = __join(this.__body);
    this.__sock.write(__concat(headBytes, body));
    this.__sock.end();
    this.emit("finish");
    this.emit("close");
    return this;
  }
}

export class OutgoingMessage extends EventEmitter {
  constructor() {
    super();
    this.headersSent = false;
    this.writableEnded = false;
    this.destroyed = false;
  }
}

// 服务端混入：Base = net.Server / tls.Server（构造实参原样透传基类）。
export function withHttpServer(Base) {
  return class HttpServer extends Base {
    constructor(...args) {
      super(...args);
      this.__closing = false;
      this.on("connection", (sock) => {
        let buf = new Uint8Array(0);
        sock.on("data", (chunk) => {
          if (this.__closing || sock.__upgraded) return;
          try {
            buf = this.__feed(buf, chunk, sock);
          } catch {
            sock.destroy();
          }
        });
      });
    }
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
      req.socket = sock;
      req.connection = sock;
      // Node 口径：带 Upgrade 头的请求不进 request 管线——派发 'upgrade'
      //（req, 原始 socket；vite 的 ws 库经它完成 101 握手与帧收发），无监听
      // 则销毁连接。升级后本连接停止 HTTP 解析（__upgraded 旗）；头后残留
      // 字节（罕见）经 microtask 以裸 data 事件回灌（监听方已同步登记）。
      if (headers.upgrade !== undefined) {
        sock.__upgraded = true;
        if (this.listenerCount("upgrade") > 0) {
          // Node 口径三参 (req, socket, head)：head 恒 Buffer（零长=无残留，
          // ws 库 setSocket 读 head.length——undefined 即 TypeError）。
          const leftover = buf.slice(headEnd + 4);
          this.emit("upgrade", req, sock, leftover);
        } else {
          sock.destroy();
        }
        return new Uint8Array(0);
      }
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
      this.emit("request", req, res);
      req.__feed(body);
      return new Uint8Array(0);
    }
    close(cb) {
      this.__closing = true;
      if (typeof cb === "function") this.once("close", cb);
      super.close();
      return this;
    }
  };
}

// 客户端工厂：openSocket(host, port, extra) 开传输 socket；
// flavor = { protocol: "http:", defaultPort: 80, other: "node:https" }。
export function withClientRequest(openSocket, flavor) {
  return class ClientRequest extends OutgoingMessage {
    constructor(options, cb) {
      super();
      let host, port, path, method, userHeaders, extra;
      if (typeof options === "string" || options instanceof URL) {
        const u = new URL(String(options));
        if (u.protocol !== flavor.protocol) {
          throw new Error(`ERR_INVALID_PROTOCOL: protocol '${u.protocol}' not supported (use ${flavor.other})`);
        }
        method = "GET";
        host = u.hostname;
        port = u.port ? Number(u.port) : flavor.defaultPort;
        path = u.pathname + u.search;
        userHeaders = {};
        extra = {};
      } else {
        method = (options.method ?? "GET").toUpperCase();
        host = options.host ?? options.hostname ?? "127.0.0.1";
        port = Number(options.port ?? flavor.defaultPort);
        path = options.path ?? "/";
        if (!path.startsWith("/")) path = "/" + path;
        userHeaders = options.headers ?? {};
        extra = options;
      }
      this.method = method;
      this.host = host;
      this.port = port;
      this.path = path;
      this.__headers = __lowerHeaders(userHeaders);
      if (this.__headers.host === undefined) {
        this.__headers.host = port === flavor.defaultPort ? host : `${host}:${port}`;
      }
      if (this.__headers.connection === undefined) this.__headers.connection = "close";
      this.__body = [];
      this.__connected = false;
      this.__buf = new Uint8Array(0);
      this.__res = null;
      if (typeof cb === "function") this.on("response", cb);
      this.__sock = openSocket(host, port, extra);
      this.__sock.on("connect", () => {
        this.__connected = true;
        this.__maybeFlush();
      });
      this.__sock.on("secureConnect", () => {
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
      this.__sock.end();
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
        if (got === null) return;
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
      this.__noLength = true;
      if (this.__buf.length > bodyStart) res.emit("data", globalThis.Buffer.from(this.__buf.slice(bodyStart)));
    }
    __finish() {
      this.__res.complete = true;
      this.__sock.end();
      this.emit("close");
    }
  };
}

export class Agent {
  constructor() { this.keepAlive = false; this.maxSockets = Infinity; }
  destroy() {}
}

// request/get 三形态归一：(url[, options][, cb]) / (options[, cb]) → [options, cb]。
// url 与 options 并存时 options 优先；跨协议即 ERR_INVALID_PROTOCOL（Node 口径）。
export function normalizeRequestArgs(a, b, c, flavor) {
  if (typeof a === "string" || a instanceof URL) {
    const u = new URL(String(a));
    if (u.protocol !== flavor.protocol) {
      throw new Error(`ERR_INVALID_PROTOCOL: protocol '${u.protocol}' not supported (use ${flavor.other})`);
    }
    const fromUrl = { hostname: u.hostname };
    if (u.port) fromUrl.port = Number(u.port);
    fromUrl.path = u.pathname + u.search;
    if (typeof b === "function") return [fromUrl, b];
    return [{ ...fromUrl, ...(b ?? {}) }, c];
  }
  return [a, typeof b === "function" ? b : undefined];
}
export function requestFrom(ClientRequest, options, cb) {
  const req = new ClientRequest(options, cb);
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
export function getFrom(ClientRequest, options, cb) {
  const req = requestFrom(ClientRequest, options, cb);
  req.end();
  return req;
}
"#;
