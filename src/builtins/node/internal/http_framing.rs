//! `node:internal/http_framing`：HTTP/1.1 共享帧层（`node:http` 与 `node:https`
//! 共用；Phase 9d-6 新建，10b 流式化重构）。内容逐行取自 `node:http` 原 SOURCE
//! （行为保真，http 既有黑盒为回归网）；9d-6 起：解析器子集 + IncomingMessage +
//! ServerResponse + `withHttpServer(Base)` 服务端混入 +
//! `withClientRequest(open, flavor)` 客户端工厂。
//! 注入点仅两处：服务端基类（net.Server / tls.Server）、客户端开 socket 函数。
//!
//! 10b（Bun 高度）：整收改流式——
//! - `IncomingMessage`/`ServerResponse`/`ClientRequest` 进 `node:stream` 全家
//!  （Readable/Writable；pipe/for-await 免费；暂停/流动走标准语义）。
//! - 服务端 keep-alive：逐请求协商（HTTP/1.1 默认保活，`connection: close`
//!   则关），响应完回解析循环（microtask 续行，防管线洪水爆栈）；
//!   `server.close()` 销毁跟踪中的全部连接（含空闲保活）。
//! - 客户端 Agent 池：`keepAlive` 按 host:port 复用，`reusedSocket` 可观测；
//!   缺省全局 Agent 不池化（与 9d 行为一致）。
//! - 分块编码：10f G3 起按真机口径——CL 快路径仅当 end() 是首个头触发点
//!  （无 writeHead/write/flushHeaders 前置）且允许体（服务端随请求版本、
//!   客户端随方法族）；否则 chunked（HTTP/1.0 服务端裸体 close-delimited）。
//! - 204/304/HEAD 无体（CL 照算但不发字节，保活不断帧）。
//! 偏差记档（10b）：
//! - 请求头在首字节实际发出前不出网（write 先缓冲；包级时序差，语义同）。
//! - 管线请求顺序处理（后请求的解析等前响应结束；并发语义同，时序差）。
//! - 服务端无空闲保活超时（`server.close()` 即全毁；Node 有 keepAliveTimeout）。
//! - 1xx 中间响应按终态处理（无 `continue` 事件；100-continue 流程另切片）。
//! - trailer 不收不发（chunked 尾部直接终结；10f G3 起限深照算：trailer 名+值
//!   累计 ≥ maxHeaderSize 即 431、无冒号行 400；http2 triage 见 plan3 10b-4）。
//! - 10f G3：chunk 扩展限深（尺寸行扩展总量 > 16KiB 即 413、扩展字符集校验
//!   400）——llhttp 计数语义，真机 26.8.2 逐项实测定标。

pub const SOURCE: &str = r#"
import { EventEmitter } from "node:events";
import { Readable, Writable } from "node:stream";
import { codes } from "node:internal/errors";

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
  // 真机口径：req.headers/res.headers 是普通对象（Object.prototype，node 26.8.2
  // 实测）——Object.create(null) 会挂 deepStrictEqual 直比；__proto__ 头名走
  // defineProperty 防原型污染。
  const headers = {};
  const rawHeaders = [];
  for (const line of lines) {
    if (line === "") continue;
    const c = line.indexOf(":");
    if (c <= 0) throw __mkParseError("malformed header line");
    const k = line.slice(0, c).trim();
    const v = line.slice(c + 1).trim();
    rawHeaders.push(k, v);
    const lk = k.toLowerCase();
    if (headers[lk] === undefined) {
      Object.defineProperty(headers, lk, { value: v, writable: true, enumerable: true, configurable: true });
    } else {
      headers[lk] = `${headers[lk]}, ${v}`;
    }
  }
  return { first, headers, rawHeaders };
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
// node lib/_http_client.js 同款（INVALID_PATH_REGEX）：控制字符与空格等禁入 path。
const INVALID_PATH_REGEX = /[^\u0021-\u00ff]/;
function __validateInteger(v, name, min = 0) {
  if (typeof v !== "number" || !Number.isInteger(v) || v < min) {
    throw new codes.ERR_OUT_OF_RANGE(name, `an integer >= ${min}`, v);
  }
  return v;
}
// 解析期校验失败哨兵：连接层捕到后回 400 + 销毁（Node clientError 默认行为）。
function __mkParseError(msg) {
  const e = new Error(msg ?? "parse error");
  e.__httpParse = true;
  return e;
}
const __TOKEN_RE = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;
// chunk 扩展字符集：RFC 7230 token + ';' + '='（真机 26.8.2 ASCII 全扫实测；
// 其余——空格/引号/括号/冒号/控制字符/高位字节——一律 400）。
const __EXT_RE = /^[!#$%&'*+\-.^_`|~0-9A-Za-z;=]$/;
// chunk 扩展字节上限（16KiB，node src/node_http_parser.cc 同值）：单条尺寸行
// 扩展总量 > 16384 即 413（真机：16384 过 / 16385 拒）。
const __MAX_CHUNK_EXT = 16384;
// trailer 名+值累计上限：≥ 16384（maxHeaderSize）即 431（真机：16383 过 /
// 16384 拒；': ' 分隔与 CRLF 不计，跨 trailer 行累计）。
const __MAX_TRAILER_NV = 16384;
// 请求行 + 头行校验（RFC token/版本形；Node llhttp 拒收面，失配即 400）。
function __validateRequestHead(first, headers) {
  if (first.length !== 3 || !__TOKEN_RE.test(first[0]) || /\s/.test(first[1]) ||
      !/^HTTP\/\d(\.\d)?$/.test(first[2])) {
    throw __mkParseError("bad request line");
  }
  for (const k of Object.keys(headers)) {
    if (!__TOKEN_RE.test(k)) throw __mkParseError("bad header name");
  }
}

// ---- 增量体帧 ----
// { type: 'none' }（无体）| { type: 'cl', remaining } |
// { type: 'chunked', sizeLine, need, buf } | { type: 'close' }（读到连接关）。
function __framingFor(headers, isResponse, statusCode, method) {
  if (isResponse) {
    if (method === "HEAD") return { type: "none" };
    if (statusCode !== null && (statusCode === 204 || statusCode === 304 ||
        (statusCode >= 100 && statusCode < 200))) return { type: "none" };
  }
  const te = (headers["transfer-encoding"] || "").toLowerCase();
  if (te.includes("chunked")) return { type: "chunked", need: -1, buf: new Uint8Array(0) };
  const cl = Number(headers["content-length"] ?? NaN);
  if (Number.isInteger(cl) && cl >= 0) return { type: "cl", remaining: cl };
  return isResponse ? { type: "close" } : { type: "none" };
}
// CL 泵：取 min(remaining, available) 推流（msg 为 null 则纯跳过，不推流）。
// 未完结时 rest 恒为空（余字节已进 fr 内部态；调用方直接覆盖缓冲）。
function __pumpCL(fr, msg, bytes) {
  const take = Math.min(fr.remaining, bytes.length);
  if (take > 0 && msg !== null) msg.push(globalThis.Buffer.from(bytes.slice(0, take)));
  fr.remaining -= take;
  return { done: fr.remaining === 0, rest: bytes.slice(take) };
}
// chunked 泵：增量解 size 行/数据/CRLF；trailer 逐行计数（不收内容）。
// 扩展/trailer 限深均按真机 26.8.2 逐项实测（见各常量注）。
function __pumpChunked(fr, msg, bytes) {
  let buf = __concat(fr.buf, bytes);
  fr.buf = new Uint8Array(0);
  while (true) {
    if (fr.need === -1) {
      // 尺寸行 `1*HEXDIG[';' 扩展]`：hex 段只收 hex digit（空格/空行即 400）；
      // 扩展段按字符集校验（token + ';' + '='，真机 ASCII 全扫），扩展字节
      // 总量（';' 不计）> 16KiB 即 413——是尺寸行内总量而非单 token 上限
      // （真机：单 16384 过 / 单 16385 拒 / 双 token 合计 16385 同拒 / 带 '='
      // 合计 16384 过）；换 chunk 清零（3×10KB 三分块全过）。行内裸 LF/CR
      // 落在禁字符集即 400（smuggling 套件 `2;\n` 形）。跨包累计。
      let eol = -1;
      for (let i = 0; i + 1 < buf.length; i++) {
        if (buf[i] === 13 && buf[i + 1] === 10) { eol = i; break; }
      }
      const scanTo = eol === -1 ? buf.length : eol;
      if (fr.__ext === undefined) fr.__ext = 0;
      let sawSemi = false;
      for (let i = 0; i < scanTo; i++) {
        const ch = String.fromCharCode(buf[i]);
        if (ch === ";") { sawSemi = true; continue; }
        if (sawSemi) {
          if (!__EXT_RE.test(ch)) return { error: 400 };
          fr.__ext++;
          if (fr.__ext > __MAX_CHUNK_EXT) return { error: 413 };
        } else if (!((ch >= "0" && ch <= "9") || (ch >= "A" && ch <= "F") || (ch >= "a" && ch <= "f"))) {
          return { error: 400 };
        }
      }
      if (eol === -1) { fr.buf = buf; return { done: false, rest: new Uint8Array(0) }; }
      const lineText = __latin1(buf.slice(0, eol));
      // 行尾 ';'：末段扩展为空（真机 400；行中空段如 `;;a` 合法）。
      if (lineText.endsWith(";")) return { error: 400 };
      const semi = lineText.indexOf(";");
      const sizePart = semi === -1 ? lineText : lineText.slice(0, semi);
      const size = sizePart === "" ? NaN : parseInt(sizePart, 16);
      if (!Number.isInteger(size) || size < 0) return { error: 400 };
      fr.__ext = 0;
      buf = buf.slice(eol + 2);
      if (size === 0) {
        // 终结：`0\r\n` 后跟空行（或 trailer 块 + 空行）；内容丢弃。
        fr.need = -2;
        continue;
      }
      fr.need = size;
    } else if (fr.need === -2) {
      // 终结段：trailer 块逐行计数到空行（内容不收）。名+值累计（': ' 与
      // CRLF 不计）≥ 16KiB 即 431（真机阈值：16383 过 / 16384 拒，跨行累计）；
      // 无冒号行 400（真机 `justname` 行即拒）。
      let eol = -1;
      for (let i = 0; i + 1 < buf.length; i++) {
        if (buf[i] === 13 && buf[i + 1] === 10) { eol = i; break; }
      }
      if (eol === -1) { fr.buf = buf; return { done: false, rest: new Uint8Array(0) }; }
      if (eol === 0) return { done: true, rest: buf.slice(2) };
      const lineText = __latin1(buf.slice(0, eol));
      const c = lineText.indexOf(":");
      if (c <= 0) return { error: 400 };
      fr.__tnv = (fr.__tnv === undefined ? 0 : fr.__tnv) + c + lineText.slice(c + 1).trim().length;
      if (fr.__tnv >= __MAX_TRAILER_NV) return { error: 431 };
      buf = buf.slice(eol + 2);
      continue; // 回 -2 分支续行（严禁落穿到数据泵：need 仍是 -2）
    }
    if (buf.length < fr.need + 2) { fr.buf = buf; return { done: false, rest: new Uint8Array(0) }; }
    if (fr.need > 0 && msg !== null) msg.push(globalThis.Buffer.from(buf.slice(0, fr.need)));
    buf = buf.slice(fr.need + 2);
    fr.need = -1;
  }
}

export class IncomingMessage extends Readable {
  constructor() {
    super();
    this.httpVersion = "1.1";
    this.method = null;
    this.url = null;
    this.statusCode = null;
    this.statusMessage = null;
    this.headers = {};
    this.rawHeaders = [];
    this.complete = false;
  }
  _read() {}
  // node lib/_http_incoming.js 口径：转发 socket 空闲计时（'timeout' 由 socket
  // 发出；cb 注册为 once 监听）。
  setTimeout(msecs, callback) {
    if (typeof callback === "function") this.once("timeout", callback);
    if (this.socket !== undefined && this.socket !== null) return this.socket.setTimeout(msecs);
    return this;
  }
  // 一次性喂体（兼容口）：推流 + 结束。
  __feed(body) {
    if (body.length > 0) this.push(globalThis.Buffer.from(body));
    this.__complete();
  }
  __complete() {
    if (this.complete) return;
    this.complete = true;
    this.push(null);
  }
}

export class ServerResponse extends Writable {
  constructor(sock) {
    // autoDestroy 关：finish 后连接必须活着（keep-alive 复用/优雅关由显式
    // destroy 负责；自动销毁会把保活连接一起杀掉）。
    super({ autoDestroy: false });
    // 构造首参：server 流程传 socket；独立构造传 req 形信息对象（node 口径
    // `new ServerResponse(req)`，standalone 套件）——非 socket 一律不入 __sock。
    this.__sock = sock && typeof sock.write === "function" ? sock : null;
    this.__sockAssigned = false;
    this.statusCode = 200;
    this.statusMessage = undefined;
    this.__headers = Object.create(null);
    this.headersSent = false;
    this.__headSent = false;
    // 真机口径（10f G3，node 26.8.2 实测）：CL 快路径仅当 end() 是首个头触发点
    // （此前无 writeHead/write/flushHeaders）；writeHead 在前 → chunked；
    // HTTP/1.0 请求 → 无 CL/TE，体裸写 + close（close-delimited）。
    // __req1_1 = useChunkedEncodingByDefault（随请求版本），standalone 缺省 1.1。
    this.__headStored = false;
    this.__req1_1 = true;
    this.__chunked = false;
    this.__rawCL = false;
    this.__buf1 = null;
    this.__holdTimer = null;
    this.__keepAlive = false;
    this.__headOnly = false;
    this.__noBody = false;
    this.__userEnded = false;
    this.__onDone = null;
  }
  setHeader(name, value) {
    const lk = String(name).toLowerCase();
    this.__headers[lk] = String(value);
    if (lk === "connection") this.__autoConn = false;
    return this;
  }
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
    // 头已存：随后的 end(data) 不再走 CL 快路径（真机 chunked 口径）。
    this.__headStored = true;
    return this;
  }
  write(chunk, encoding) {
    if (this.__userEnded) throw new Error("ERR_STREAM_WRITE_AFTER_END: write after end");
    return super.write(chunk, encoding);
  }
  end(chunk, encoding, cb) {
    if (this.__userEnded) {
      const f = typeof chunk === "function" ? chunk : (typeof encoding === "function" ? encoding : cb);
      if (typeof f === "function") f();
      return this;
    }
    // CL 快路径判据（真机）：end 的数据块存在性——write 后裸 end() 不走快路径
    //（真机 chunked + 终结块口径）。
    this.__endHadData = chunk !== undefined && chunk !== null && typeof chunk !== "function";
    this.__userEnded = true;
    return super.end(chunk, encoding, cb);
  }
  // 可写流最小面（ws Sender 的 cork/uncork；本仓写直通无聚合，no-op）。
  cork() { return this; }
  uncork() { return this; }
  // 立即发头（Node flushHeaders：body 可经 chunked 帧，end 后补终结块）。
  flushHeaders() {
    if (this.__headSent || this.__noBody || this.__headOnly) return;
    if (this.__headers["content-length"] === undefined && this.__req1_1) this.__chunked = true;
    this.__sendHead();
    if (this.__buf1 !== null) {
      const b = this.__buf1;
      this.__buf1 = null;
      this.__frame(b);
    }
  }
  // 独立构造的 res 后挂 socket（standalone 套件）；双挂即 ERR_HTTP_SOCKET_ASSIGNED。
  assignSocket(sock) {
    if (this.__sockAssigned) {
      const e = new Error("Socket is already assigned");
      e.code = "ERR_HTTP_SOCKET_ASSIGNED";
      throw e;
    }
    this.__sockAssigned = true;
    this.__sock = sock;
    this.socket = sock;
  }
  __headBytes() {
    if (this.__headSent) return new Uint8Array(0);
    this.__headSent = true;
    this.headersSent = true;
    if (this.statusCode === 204 || this.statusCode === 304) this.__noBody = true;
    const reason = this.statusMessage ?? STATUS_CODES[this.statusCode] ?? "";
    const head = [`HTTP/1.1 ${this.statusCode} ${reason}`.trimEnd()];
    if (this.__headers["content-length"] !== undefined) {
      this.__rawCL = true;
    } else if (!this.__noBody && !this.__headOnly) {
      if (this.__chunked) this.__headers["transfer-encoding"] = "chunked";
    }
    if (this.__headers["connection"] === undefined) {
      this.__headers["connection"] = this.__keepAlive ? "keep-alive" : "close";
      this.__autoConn = true;
    }
    // 自设头按用户拼写输出（node verbatim；本仓内部统一小写存取）；自动头的
    // 真机输出是规范大写（Transfer-Encoding/Content-Length/自动 Connection）。
    const canon = { "transfer-encoding": "Transfer-Encoding", "content-length": "Content-Length" };
    for (const [k, v] of Object.entries(this.__headers)) {
      const name = k === "connection" && this.__autoConn ? "Connection" : (canon[k] ?? k);
      head.push(`${name}: ${v}`);
    }
    return new TextEncoder().encode(head.join("\r\n") + "\r\n\r\n");
  }
  __sendHead() {
    // node 口径：header 独立成 write；首块合并只发生在 _final/flushFinal 快捷路。
    const head = this.__headBytes();
    if (head.length > 0) this.__sock.write(head);
  }
  __frame(u8) {
    if (this.__noBody || this.__headOnly) return;
    if (u8.length === 0) return;
    if (this.__chunked && !this.__rawCL) {
      const hex = new TextEncoder().encode(u8.length.toString(16) + "\r\n");
      this.__sock.write(__concat(hex, __concat(u8, new TextEncoder().encode("\r\n"))));
    } else {
      this.__sock.write(u8);
    }
  }
  _write(chunk, encoding, cb) {
    const u8 = chunk instanceof Uint8Array ? chunk : __toU8(String(chunk));
    this.__sawWrite = true;
    if (this.__buf1 === null && !this.__headSent) {
      // 首字节 holdback：一拍内 end 到达且此前无 writeHead 则走 CL 快捷，
      // 否则转 chunked 流式（1.0 裸写）。
      this.__buf1 = u8;
      this.__holdTimer = setTimeout(() => {
        this.__holdTimer = null;
        if (this.__buf1 !== null && !this.__headSent && !this.destroyed) {
          if (this.__req1_1) this.__chunked = true;
          this.__sendHead();
          const b = this.__buf1;
          this.__buf1 = null;
          this.__frame(b);
        }
      }, 0);
      cb();
      return;
    }
    if (!this.__headSent) {
      if (this.__req1_1) this.__chunked = true;
      this.__sendHead();
      if (this.__buf1 !== null) {
        const b = this.__buf1;
        this.__buf1 = null;
        this.__frame(b);
      }
    }
    this.__frame(u8);
    cb();
  }
  _final(cb) {
    if (this.__holdTimer !== null) {
      clearTimeout(this.__holdTimer);
      this.__holdTimer = null;
    }
    if (!this.__headSent) {
      // 快捷：end() 为首个头触发点（无 writeHead/write 前置）且请求 1.1 时，
      // Content-Length 一次发出（真机口径）；write/writeHead 在前 → chunked；
      // 1.0 → 无 CL/TE 裸体（close-delimited，真机实测）。
      const total = this.__buf1 !== null ? this.__buf1.length : 0;
      if (!this.__noBody && !this.__headOnly && this.__headers["content-length"] === undefined) {
        if (this.__req1_1 && !this.__headStored && (!this.__sawWrite || this.__endHadData)) {
          this.__headers["content-length"] = String(total);
        } else if (this.__req1_1) {
          this.__chunked = true;
          this.__headers["transfer-encoding"] = "chunked";
        }
      }
      const head = this.__headBytes();
      if (this.__sock !== null) {
        if (this.__buf1 !== null) {
          const b = this.__buf1;
          this.__buf1 = null;
          if (this.__chunked && !this.__rawCL && !this.__noBody && !this.__headOnly) {
            // chunked：头/体/终结分开帧（真机 per-chunk 帧口径；头体合并不带
            // 帧头会被对端判坏 chunked 体）。
            if (head.length > 0) this.__sock.write(head);
            this.__frame(b);
            this.__sock.write(new TextEncoder().encode("0\r\n\r\n"));
          } else if (head.length === 0) {
            this.__frame(b);
          } else if (!this.__noBody && !this.__headOnly && b.length > 0) {
            // node 口径：CL/raw 快捷时头 + 首块合并为一次 write（standalone 套件）。
            this.__sock.write(__concat(head, b));
          } else {
            this.__sock.write(head);
            this.__frame(b);
          }
        } else if (head.length > 0) {
          this.__sock.write(head);
          // end() 无数据 + chunked：终结块紧随（真机 writeHead+end() 口径）。
          if (this.__chunked && !this.__rawCL && !this.__noBody && !this.__headOnly) {
            this.__sock.write(new TextEncoder().encode("0\r\n\r\n"));
          }
        }
      }
    } else if (this.__chunked && !this.__rawCL && !this.__noBody && !this.__headOnly) {
      this.__sock.write(new TextEncoder().encode("0\r\n\r\n"));
    }
    const cont = (this.__keepAlive && this.__onDone !== null) ? this.__onDone : null;
    this.__onDone = null;
    if (!this.__keepAlive) {
      try { this.__sock.end(); } catch { /* closed meanwhile */ }
    }
    cb();
    if (cont !== null) queueMicrotask(cont);
  }
  _destroy(err, cb) {
    if (this.__holdTimer !== null) {
      clearTimeout(this.__holdTimer);
      this.__holdTimer = null;
    }
    try { this.__sock.destroy(); } catch { /* closed meanwhile */ }
    cb(err);
  }
}

export class OutgoingMessage extends Writable {
  constructor(options) {
    // autoDestroy 关（同 ServerResponse：销毁一律显式）。
    // emitClose 关：req 'close' 在响应收齐/连接收尾时手动发出（9d 口径：
    // 上传 finish 不等于请求结束），见 __finishResponse/__onSockCloseEv。
    super({ autoDestroy: false, emitClose: false });
    this.headersSent = false;
    this.socket = null;
    // 独立构造（`new OutgoingMessage()`，outgoing-properties 系套件）：无 socket
    // 时 _write 缓冲不落盘——cb 不调（writableLength 保持，Node outputData 口径），
    // 有子类 socket 面时由子类 _write 覆写。
    this.__outputData = [];
  }
  _write(chunk, encoding, cb) {
    this.__outputData.push([chunk, encoding, cb]);
  }
  _implicitHeader() {
    throw new Error("_implicitHeader() method is not implemented");
  }
}

// 服务端混入：Base = net.Server / tls.Server（构造实参原样透传基类）。
// http 面选项（10f 对拍，node lib/_http_server.js 口径）：requestTimeout 默认
// 300000、headersTimeout 默认 min(60000, requestTimeout)、keepAliveTimeout 5000、
// keepAliveTimeoutBuffer 1000；headersTimeout > requestTimeout 即 ERR_OUT_OF_RANGE。
export function withHttpServer(Base) {
  class __HttpServer extends Base {
    constructor(...args) {
      super(...args);
      const o = (args[0] && typeof args[0] === "object" && !Array.isArray(args[0])) ? args[0] : {};
      this.timeout = 0;
      this.requestTimeout = 300_000;
      this.headersTimeout = 60_000;
      this.keepAliveTimeout = 5_000;
      this.keepAliveTimeoutBuffer = 1_000;
      this.maxRequestsPerSocket = 0;
      const rt = o.requestTimeout !== undefined ? __validateInteger(o.requestTimeout, "requestTimeout") : undefined;
      if (rt !== undefined) this.requestTimeout = rt;
      const ht = o.headersTimeout !== undefined ? __validateInteger(o.headersTimeout, "headersTimeout") : undefined;
      this.headersTimeout = ht !== undefined ? ht : Math.min(60_000, this.requestTimeout);
      if (this.requestTimeout > 0 && this.headersTimeout > 0 && this.headersTimeout > this.requestTimeout) {
        throw new codes.ERR_OUT_OF_RANGE("headersTimeout", "<= requestTimeout", o.headersTimeout);
      }
      const kt = o.keepAliveTimeout !== undefined ? __validateInteger(o.keepAliveTimeout, "keepAliveTimeout") : undefined;
      if (kt !== undefined) this.keepAliveTimeout = kt;
      const kb = o.keepAliveTimeoutBuffer !== undefined ? __validateInteger(o.keepAliveTimeoutBuffer, "keepAliveTimeoutBuffer") : undefined;
      if (kb !== undefined) this.keepAliveTimeoutBuffer = kb;
      if (o.maxRequestsPerSocket !== undefined) this.maxRequestsPerSocket = o.maxRequestsPerSocket;
      this.__closing = false;
      this.__sockets = new Set();
      this.on("connection", (sock) => {
        this.__sockets.add(sock);
        const st = { buf: new Uint8Array(0), req: null, framing: null, res: null, __hdT: null, __rqT: null, __kaT: null };
        sock.__httpState = st;
        sock.on("close", () => {
          this.__clearReqTimers(st);
          this.__sockets.delete(sock);
          // 连接断时未完的req/res一起收尾：req destroy触发pipeline的
          // PREMATURE_CLOSE（客户端中断上传用例），res destroy防写半开。
          if (st.req !== null && !st.req.complete && !st.req.destroyed) {
            st.req.destroy();
          }
          if (st.res !== null && !st.res.writableEnded && !st.res.destroyed) {
            st.res.destroy();
          }
        });
        // server.timeout：per-socket 空闲计时（10f；单发 timer，data 到达即重臂，
        // 见 data 处理器）。到期 server 发 'timeout'(socket)，不杀连接（net 口径）。
        if (this.timeout > 0) sock.setTimeout(this.timeout);
        sock.on("timeout", () => {
          if (!sock.destroyed) this.emit("timeout", sock);
        });
        sock.on("data", (chunk) => {
          if (this.__closing || sock.__upgraded) return;
          if (this.timeout > 0) sock.setTimeout(this.timeout);
          try {
            this.__feed(sock, st, chunk);
          } catch (e) {
            this.__feedError(sock, e);
          }
        });
        // 连接即开 headers 计时（headersTimeout 内须收到完整头，否则 408）。
        this.__armIdleTimers(st, sock);
      });
    }
    // 400 Bad Request（Node clientError 默认响应）+ 销毁。
    __badRequest(sock) {
      try { sock.write(new TextEncoder().encode("HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n")); } catch { /* gone */ }
      try { sock.destroy(); } catch { /* gone */ }
    }
    // 408 Request Timeout（requestTimeout/headersTimeout 到期；精确字节见
    // test-http-server-request-timeout-delayed-headers 套件）+ 销毁。
    __reqTimeout(sock) {
      try { sock.write(new TextEncoder().encode("HTTP/1.1 408 Request Timeout\r\nConnection: close\r\n\r\n")); } catch { /* gone */ }
      try { sock.destroy(); } catch { /* gone */ }
    }
    // 413 Payload Too Large：chunk 扩展总量超限（chunk-extensions-limit 套件，
    // 精确字节真机实测）+ 销毁。
    __payloadTooLarge(sock) {
      try { sock.write(new TextEncoder().encode("HTTP/1.1 413 Payload Too Large\r\nConnection: close\r\n\r\n")); } catch { /* gone */ }
      try { sock.destroy(); } catch { /* gone */ }
    }
    // 431 Request Header Fields Too Large：trailer 名+值累计超 maxHeaderSize
    // （真机实测精确字节）+ 销毁。
    __headerFieldsTooLarge(sock) {
      try { sock.write(new TextEncoder().encode("HTTP/1.1 431 Request Header Fields Too Large\r\nConnection: close\r\n\r\n")); } catch { /* gone */ }
      try { sock.destroy(); } catch { /* gone */ }
    }
    // __feed 解析错的统一出口（400 通道 / 兜底销毁）。数据事件与 res 收尾的
    // microtask re-feed（__onDone）共用；后者不在 data 处理器的 try/catch 内，
    // 裸抛会变成 unhandled rejection（chunked-smuggling 套件）。
    __feedError(sock, e) {
      if (e && e.__httpParse) {
        this.__badRequest(sock);
        return;
      }
      try { sock.destroy(); } catch { /* gone */ }
    }
    __clearReqTimers(st) {
      if (st.__hdT !== null) { clearTimeout(st.__hdT); st.__hdT = null; }
      if (st.__rqT !== null) { clearTimeout(st.__rqT); st.__rqT = null; }
      if (st.__kaT !== null) { clearTimeout(st.__kaT); st.__kaT = null; }
    }
    // 空闲期（等下一请求头）：headersTimeout → 408；keepAliveTimeout → 静默销毁
    // （ka 只在响应完成后臂，withKa——体齐响应未完时挂 ka 会误杀在途响应）。
    // 消息期（头已到、体未齐）：requestTimeout → 408。Node 口径：消息期计时器
    // 不因部分数据重置（interrupted/delayed 系套件依赖）。
    __armIdleTimers(st, sock, withKa = false) {
      this.__clearReqTimers(st);
      if (this.headersTimeout > 0) {
        st.__hdT = setTimeout(() => { st.__hdT = null; this.__reqTimeout(sock); }, this.headersTimeout);
        st.__hdT.unref();
      }
      if (withKa && st.sawRequest && this.keepAliveTimeout > 0) {
        st.__kaT = setTimeout(() => { st.__kaT = null; try { sock.destroy(); } catch { /* gone */ } }, this.keepAliveTimeout + this.keepAliveTimeoutBuffer);
        st.__kaT.unref();
      }
    }
    __armMsgTimer(st, sock) {
      this.__clearReqTimers(st);
      if (this.requestTimeout > 0) {
        st.__rqT = setTimeout(() => { st.__rqT = null; this.__reqTimeout(sock); }, this.requestTimeout);
        st.__rqT.unref();
      }
    }
    setTimeout(msecs, callback) {
      this.timeout = msecs;
      if (typeof callback === "function") this.on("timeout", callback);
      return this;
    }
    closeIdleConnections() {
      for (const sock of this.__sockets) {
        const st = sock.__httpState;
        if (!st || st.req === null) {
          try { sock.destroy(); } catch { /* gone */ }
        }
      }
    }
    closeAllConnections() {
      for (const sock of this.__sockets) {
        try { sock.destroy(); } catch { /* gone */ }
      }
    }
    __feed(sock, st, chunk) {
      st.buf = __concat(st.buf, chunk);
      while (true) {
        if (st.req === null) {
          const headEnd = __findHeadEnd(st.buf);
          if (headEnd === -1) {
            // 头未齐也可先校验请求行（llhttp 增量语义；管线残渣 "hello world\r\n"
            // 之类在行终结时就该 400，等不到 \r\n\r\n——blank-header 套件）。
            for (let i = 0; i + 1 < st.buf.length; i++) {
              if (st.buf[i] === 13 && st.buf[i + 1] === 10) {
                const lineText = __latin1(st.buf.slice(0, i)).split(" ");
                if (lineText.length !== 3 || !__TOKEN_RE.test(lineText[0]) ||
                    !/^HTTP\/\d(\.\d)?$/.test(lineText[2] ?? "")) {
                  throw __mkParseError("bad request line");
                }
                break;
              }
            }
            return;
          }
          const headText = __latin1(st.buf.slice(0, headEnd));
          const { first, headers, rawHeaders } = __parseHead(headText);
          __validateRequestHead(first, headers);
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
              const leftover = st.buf.slice(headEnd + 4);
              st.buf = new Uint8Array(0);
              this.emit("upgrade", req, sock, leftover);
            } else {
              sock.destroy();
            }
            return;
          }
          const framing = __framingFor(headers, false, null, req.method);
          const conn = (headers.connection || "").toLowerCase();
          const keepAlive = req.httpVersion === "1.1" ? conn !== "close" : conn === "keep-alive";
          const res = new ServerResponse(sock);
          // useChunkedEncodingByDefault 随请求版本（1.0 → 无 CL/TE 裸体口径）。
          res.__req1_1 = req.httpVersion === "1.1";
          res.req = req;
          req.res = res;
          res.__keepAlive = keepAlive && !this.__closing;
          res.__headOnly = req.method === "HEAD";
          st.req = req;
          st.framing = framing;
          st.res = res;
          // 头已齐、体在途：消息期 requestTimeout 计时。
          this.__armMsgTimer(st, sock);
          res.__onDone = () => {
            st.req = null;
            st.framing = null;
            st.res = null;
            st.sawRequest = true;
            // 请求+响应完整落地：回空闲期（headersTimeout/keepAliveTimeout 双计时）。
            this.__armIdleTimers(st, sock, true);
            // 连接已销毁（如 mid-body 400/413）则不再 re-feed 剩余缓冲；
            // 活着时解析错也要走统一 400 通道（microtask 内裸抛 = unhandled rejection）。
            if (sock.destroyed) return;
            try {
              this.__feed(sock, st, new Uint8Array(0));
            } catch (e) {
              this.__feedError(sock, e);
            }
          };
          if (this.__closing) {
            st.buf = st.buf.slice(headEnd + 4);
            res.writeHead(503);
            res.end();
            return;
          }
          st.buf = st.buf.slice(headEnd + 4);
          // §4.35：先 emit("request")（监听器登记 data/end），再喂体。
          this.emit("request", req, res);
          continue;
        }
        // 体泵：CL / chunked 增量；none 直接完结。
        const fr = st.framing;
        if (fr.type === "none") {
          st.req.__complete();
          st.req = null;
          st.framing = null;
          this.__armIdleTimers(st, sock);
          continue;
        }
        let r;
        if (fr.type === "cl") {
          r = __pumpCL(fr, st.req, st.buf);
        } else {
          r = __pumpChunked(fr, st.req, st.buf);
          // 413/431 是精确字节响应 + 销毁（不是 400 通道）；其余解析错走 400。
          if (r.error === 413) { this.__payloadTooLarge(sock); return; }
          if (r.error === 431) { this.__headerFieldsTooLarge(sock); return; }
          if (r.error) throw __mkParseError("bad chunked body");
        }
        st.buf = r.rest;
        if (!r.done) return;
        st.req.__complete();
        st.req = null;
        st.framing = null;
        this.__armIdleTimers(st, sock);
      }
    }
    close(cb) {
      this.__closing = true;
      if (typeof cb === "function") this.once("close", cb);
      // 空闲保活连接一并销毁，否则 'close' 永不到（10b；Node 关空闲同款）。
      for (const sock of this.__sockets) {
        try { sock.destroy(); } catch { /* closed meanwhile */ }
      }
      super.close();
      return this;
    }
  }
  // Node 口径：Server 裸调用返回新实例（lib/net.js 原文）。
  function HttpServer(...args) {
    if (!(this instanceof __HttpServer)) return new __HttpServer(...args);
    return Reflect.construct(__HttpServer, args, new.target ?? __HttpServer);
  }
  Object.setPrototypeOf(HttpServer, __HttpServer);
  HttpServer.prototype = __HttpServer.prototype;
  return HttpServer;
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
        host = options.host ?? options.hostname ?? "localhost";
        port = Number(options.port ?? flavor.defaultPort);
        path = options.path ?? "/";
        if (!path.startsWith("/")) path = "/" + path;
        userHeaders = options.headers ?? {};
        extra = options;
      }
      // node lib/_http_client.js：path 控制字符/空格即 ERR_UNESCAPED_CHARACTERS。
      if (INVALID_PATH_REGEX.test(path)) {
        throw new codes.ERR_UNESCAPED_CHARACTERS("Request path");
      }
      this.method = method;
      this.host = host;
      // node 口径：.port 不是自有属性（req.port === undefined；取值走 getPort()）。
      this.__port = port;
      this.path = path;
      this.timeout = options.timeout !== undefined ? Number(options.timeout) : undefined;
      this.socket = null;
      this.agent = options.agent === undefined ? (flavor.defaultAgent ?? null) : (options.agent || null);
      this.__headers = __lowerHeaders(userHeaders);
      if (this.__headers.host === undefined) {
        this.__headers.host = port === flavor.defaultPort ? host : `${host}:${port}`;
      }
      if (this.__headers.connection === undefined) {
        this.__headers.connection = (this.agent !== null && this.agent.keepAlive) ? "keep-alive" : "close";
        this.__autoConn = true;
      }
      this.__headSent = false;
      // 真机口径（10f G3，node 26.8.2 实测）：CL 快路径仅当 end(data) 是首个
      // 头触发点；write/flushHeaders 在前 → chunked；GET/HEAD/DELETE/OPTIONS/
      // TRACE/CONNECT（useChunkedEncodingByDefault=false 族）→ 无 CL/TE 裸体。
      this.__chunkDefault = !["GET", "HEAD", "DELETE", "OPTIONS", "TRACE", "CONNECT"].includes(method);
      this.__sawWrite = false;
      this.__endFast = false;
      this.__chunked = false;
      this.__rawCL = false;
      this.__buf1 = null;
      this.__holdTimer = null;
      this.__userEnded = false;
      this.__connected = false;
      this.__sock = null;
      this.__onSockClose = null;
      this.__res = null;
      this.__framing = null;
      this.__resBuf = new Uint8Array(0);
      this.__respDone = false;
      this.__closeEmitted = false;
      // 池键与 agent 键位统一（getName 形；keep-alive 测试以 agent.getName 命中；
      // 全量 extra 进键——https 的 TLS 选项字段参与去重）。
      this.__key = this.agent !== null
        ? this.agent.getName({ host, port, ...(extra ?? {}) })
        : `${host}:${port}`;
      this.reusedSocket = false;
      if (typeof cb === "function") this.on("response", cb);
      if (typeof options.createConnection === "function") {
        // request 级 createConnection（node _http_client 口径）：绕 agent 直建。
        this.__createConn = options.createConnection;
        let out;
        let settled = false;
        const oncreate = (err, s) => {
          settled = true;
          if (s) this.__attach(s, false);
        };
        const maybe = this.__createConn({ host, port, ...(extra ?? {}) }, oncreate);
        if (!settled && maybe) this.__attach(maybe, false);
      } else if (this.agent !== null) {
        this.agent.__acquire(this, host, port, extra, (sock, reused) => this.__attach(sock, reused));
      } else {
        this.__attach(openSocket(host, port, extra), false);
      }
    }
    __attach(sock, reused) {
      if (this.destroyed) {
        try { sock.destroy(); } catch { /* closed meanwhile */ }
        return;
      }
      this.__sock = sock;
      this.socket = sock;
      this.reusedSocket = reused === true;
      this.emit("socket", sock);
      if (this.__pendingNoDelay !== undefined) {
        try { sock.setNoDelay(this.__pendingNoDelay); } catch { /* gone */ }
      }
      if (this.__pendingKeepAlive !== undefined) {
        try { sock.setKeepAlive(this.__pendingKeepAlive[0], this.__pendingKeepAlive[1]); } catch { /* gone */ }
      }
      if (this.__reqTimeoutMs !== undefined) {
        sock.on("timeout", () => this.emit("timeout"));
        // 假 socket（createConnection 注入的 Duplex）无 setTimeout 面则跳过。
        if (typeof sock.setTimeout === "function") sock.setTimeout(this.__reqTimeoutMs);
      }
      this.__onSockClose = () => this.__onSockCloseEv();
      sock.on("connect", () => {
        this.__connected = true;
        this.__tryFlush();
        if (this.__pendingFinal) {
          this.__pendingFinal = false;
          this.__flushFinal();
        }
      });
      sock.on("secureConnect", () => {
        this.__connected = true;
        this.__tryFlush();
        if (this.__pendingFinal) {
          this.__pendingFinal = false;
          this.__flushFinal();
        }
      });
      sock.on("data", (chunk) => this.__onSockData(chunk));
      sock.on("error", (e) => {
        if (this.listenerCount("error") === 0) throw e;
        this.emit("error", e);
      });
      sock.on("close", this.__onSockClose);
      // 复用连接已连通：直接刷。
      if (reused === true) {
        this.__connected = true;
        this.__tryFlush();
        if (this.__pendingFinal) {
          this.__pendingFinal = false;
          this.__flushFinal();
        }
      }
    }
    setHeader(name, value) {
      const lk = String(name).toLowerCase();
      this.__headers[lk] = String(value);
      if (lk === "connection") this.__autoConn = false;
      return this;
    }
    getHeader(name) { return this.__headers[String(name).toLowerCase()]; }
    removeHeader(name) { delete this.__headers[String(name).toLowerCase()]; return this; }
    getHeaderNames() { return Object.keys(this.__headers); }
    getPort() { return this.__port; }
    getHost() { return this.host; }
    // node 口径：连接后落到 socket；未连接先存 pending（deferToConnect 语义）。
    setNoDelay(noDelay) {
      this.__pendingNoDelay = noDelay ?? true;
      if (this.__sock !== null && this.__sock !== undefined && typeof this.__sock.setNoDelay === "function") {
        try { this.__sock.setNoDelay(this.__pendingNoDelay); } catch { /* gone */ }
      }
      return this;
    }
    setSocketKeepAlive(enable, initialDelay) {
      this.__pendingKeepAlive = [enable ?? true, initialDelay ?? 0];
      if (this.__sock !== null && this.__sock !== undefined && typeof this.__sock.setKeepAlive === "function") {
        try { this.__sock.setKeepAlive(enable ?? true, initialDelay ?? 0); } catch { /* gone */ }
      }
      return this;
    }
    // node lib/_http_client.js：once('timeout') + socket 空闲计时（已连即臂，
    // 未连记位，__attach 落地）。
    setTimeout(msecs, callback) {
      if (typeof callback === "function") this.once("timeout", callback);
      const ms = Number(msecs) || 0;
      this.__reqTimeoutMs = ms > 0 ? ms : undefined;
      // 假 socket（createConnection 注入的 Duplex）无 setTimeout 面则跳过。
      if (this.__sock !== null && this.__sock !== undefined && typeof this.__sock.setTimeout === "function") {
        this.__sock.setTimeout(ms);
      }
      return this;
    }
    clearTimeout(cb) { return this.setTimeout(0, cb); }
    // 立即发头（node flushHeaders：_implicitHeader + 强制刷）。
    flushHeaders() {
      if (this.__headSent) return;
      if (this.__connected && this.__sock !== null && this.__sock !== undefined && !this.destroyed) {
        if (this.__buf1 !== null) this.__chunked = this.__chunkDefault;
        this.__sendHead();
        if (this.__buf1 !== null) {
          const q = this.__buf1;
          this.__buf1 = null;
          for (const b of q) this.__frame(b);
        }
      } else {
        this.__forceHead = true;
      }
    }
    write(chunk, encoding) {
      if (this.__userEnded) throw new Error("ERR_STREAM_WRITE_AFTER_END: write after end");
      return super.write(chunk, encoding);
    }
    end(chunk, encoding, cb) {
      if (this.__userEnded) {
        const f = typeof chunk === "function" ? chunk : (typeof encoding === "function" ? encoding : cb);
        if (typeof f === "function") f();
        return this;
      }
      // CL 快路径判据：end 是首个头触发点（此前无 write）。
      this.__endFast = !this.__sawWrite;
      this.__userEnded = true;
      return super.end(chunk, encoding, cb);
    }
    abort() { this.destroy(); }
    __sendHead() {
      if (this.__headSent) return;
      this.__headSent = true;
      this.headersSent = true;
      const head = [`${this.method} ${this.path} HTTP/1.1`];
      if (this.__headers["content-length"] !== undefined) {
        this.__rawCL = true;
      } else if (this.__chunked) {
        this.__headers["transfer-encoding"] = "chunked";
      }
      // 自动头规范大写（真机口径）；自设头按用户拼写（本仓小写存取）。
      const canon = { "transfer-encoding": "Transfer-Encoding", "content-length": "Content-Length" };
      for (const [k, v] of Object.entries(this.__headers)) {
        const name = k === "connection" && this.__autoConn ? "Connection" : (canon[k] ?? k);
        head.push(`${name}: ${v}`);
      }
      this.__sock.write(new TextEncoder().encode(head.join("\r\n") + "\r\n\r\n"));
    }
    __frame(u8) {
      if (u8.length === 0) return;
      if (this.__chunked && !this.__rawCL) {
        const hex = new TextEncoder().encode(u8.length.toString(16) + "\r\n");
        this.__sock.write(__concat(hex, __concat(u8, new TextEncoder().encode("\r\n"))));
      } else {
        this.__sock.write(u8);
      }
    }
    // 连接就绪或刷盘时机到：holdback 未决或 flushHeaders 逼头则刷出。
    // 队列逐帧发出（不合并）：保 TCP 分包，与连通后直发一致（blk09 计数型套件依赖）。
    __tryFlush() {
      if (!this.__connected || this.__sock === null || this.__headSent) return;
      if (this.destroyed) return;
      if (this.__buf1 === null && !this.__forceHead) return;
      if (this.__buf1 !== null) {
        this.__chunked = this.__chunkDefault;
      } else if (this.__chunkDefault && this.__headers["content-length"] === undefined) {
        // flushHeaders 先于 end：头已存（真机口径 chunked 起拍，end 后补终结块）。
        this.__chunked = true;
      }
      this.__forceHead = false;
      this.__sendHead();
      const q = this.__buf1;
      this.__buf1 = null;
      if (q !== null) for (const b of q) this.__frame(b);
    }
    _write(chunk, encoding, cb) {
      const u8 = chunk instanceof Uint8Array ? chunk : __toU8(String(chunk));
      this.__sawWrite = true;
      if (this.__buf1 === null && !this.__headSent) {
        this.__buf1 = [u8];
        this.__holdTimer = setTimeout(() => {
          this.__holdTimer = null;
          if (this.__buf1 !== null && !this.__headSent && !this.destroyed) this.__tryFlush();
        }, 0);
        cb();
        return;
      }
      if (!this.__headSent) {
        // 同拍内第二次写：必为流式，同步刷头。
        if (this.__connected && this.__sock !== null && !this.destroyed) {
          if (this.__chunkDefault) this.__chunked = true;
          this.__sendHead();
          if (this.__buf1 !== null) {
            const q = this.__buf1;
            this.__buf1 = null;
            for (const b of q) this.__frame(b);
          }
        } else {
          // 未连通：逐块排队（连通后逐帧刷出保分包，见 __tryFlush）。
          this.__buf1.push(u8);
          cb();
          return;
        }
      }
      if (this.__connected && this.__sock !== null && !this.destroyed) {
        if (!this.__chunked && !this.__rawCL && this.__chunkDefault) this.__chunked = true;
        this.__frame(u8);
      } else {
        (this.__buf1 ??= []).push(u8);
      }
      cb();
    }
    _final(cb) {
      if (this.__holdTimer !== null) {
        clearTimeout(this.__holdTimer);
        this.__holdTimer = null;
      }
      if (this.destroyed) {
        cb();
        return;
      }
      if (!this.__headSent) {
        if (!this.__connected) {
          // 连接未就绪：连通后一次性发出（CL 快捷；见 connect 回调）。
          this.__pendingFinal = true;
          cb();
          return;
        }
        this.__flushFinal();
      } else if (this.__chunked && !this.__rawCL) {
        if (this.__connected && this.__sock !== null) {
          this.__sock.write(new TextEncoder().encode("0\r\n\r\n"));
        }
      }
      cb();
    }
    // 收尾刷新（调用方保证已连通）：end(data) 为首个头触发点且方法允许体时
    // 走 CL 快捷（合并发出，真机单 write 口径）；write/flushHeaders 在前 →
    // chunked 逐帧（真机 per-write 帧口径）；GET/HEAD 族 → 无 CL/TE 裸体。
    __flushFinal() {
      if (this.__sock === null || this.destroyed) return;
      if (!this.__headSent) {
        const total = this.__buf1 !== null ? this.__buf1.reduce((a, b) => a + b.length, 0) : 0;
        if (this.__headers["content-length"] === undefined) {
          if (this.__chunkDefault && this.__endFast && !this.__forceHead) {
            this.__headers["content-length"] = String(total);
          } else if (this.__chunkDefault) {
            this.__chunked = true;
            this.__headers["transfer-encoding"] = "chunked";
          }
        }
        this.__sendHead();
        if (this.__buf1 !== null) {
          const q = this.__buf1;
          this.__buf1 = null;
          if (this.__chunked && !this.__rawCL) {
            for (const b of q) this.__frame(b);
          } else {
            this.__frame(__join(q));
          }
        }
      } else if (this.__chunked && !this.__rawCL) {
        this.__sock.write(new TextEncoder().encode("0\r\n\r\n"));
      }
    }
    _destroy(err, cb) {
      if (this.__holdTimer !== null) {
        clearTimeout(this.__holdTimer);
        this.__holdTimer = null;
      }
      if (this.agent !== null) this.agent.__cancel(this);
      // 响应同销（Node：destroy 中止整个事务；否则 res 永不完结，
      // 挂在它上面的收尾——如 server.close()——永不到）。
      if (this.__res !== null && !this.__res.complete) {
        try { this.__res.destroy(); } catch { /* already gone */ }
      }
      if (this.__sock !== null) {
        if (this.__onSockClose !== null) {
          try { this.__sock.removeListener("close", this.__onSockClose); } catch { /* closed meanwhile */ }
        }
        try { this.__sock.destroy(); } catch { /* closed meanwhile */ }
        this.__sock = null;
      }
      cb(err);
      if (!this.__closeEmitted) {
        this.__closeEmitted = true;
        queueMicrotask(() => this.emit("close"));
      }
    }
    __onSockData(chunk) {
      this.__resBuf = __concat(this.__resBuf, chunk);
      while (true) {
        if (this.__res === null) {
          const headEnd = __findHeadEnd(this.__resBuf);
          if (headEnd === -1) return;
          const headText = __latin1(this.__resBuf.slice(0, headEnd));
          const { first, headers, rawHeaders } = __parseHead(headText);
          if (!first[0].startsWith("HTTP/") || !/^\d{3}$/.test(first[1] ?? "")) {
            this.destroy(new Error("HPE_INVALID_CONSTANT: invalid HTTP response line"));
            return;
          }
          const res = new IncomingMessage();
          res.statusCode = Number(first[1]);
          // 状态行无短语合法（"HTTP/1.1 200\r\n"）：短语空串（status-message 套件）。
          res.statusMessage = first.length >= 3 ? first.slice(2).join(" ") : "";
          res.headers = headers;
          res.rawHeaders = rawHeaders;
          res.socket = this.__sock;
          res.connection = this.__sock;
          res.req = this;
          this.__framing = __framingFor(headers, true, res.statusCode, this.method);
          this.__res = res;
          this.__resBuf = this.__resBuf.slice(headEnd + 4);
          // §4.35：先 emit("response")（监听器登记 data/end），再喂体。
          this.emit("response", res);
          continue;
        }
        const fr = this.__framing;
        if (fr.type === "none") {
          this.__res.__complete();
          this.__finishResponse(false);
          return;
        }
        if (fr.type === "close") {
          if (this.__resBuf.length > 0) {
            this.__res.push(globalThis.Buffer.from(this.__resBuf));
            this.__resBuf = new Uint8Array(0);
          }
          return;
        }
        let r;
        if (fr.type === "cl") {
          r = __pumpCL(fr, this.__res, this.__resBuf);
        } else {
          r = __pumpChunked(fr, this.__res, this.__resBuf);
          if (r.error) {
            this.destroy(new Error("HPE_INVALID_CONSTANT: invalid chunked body"));
            return;
          }
        }
        this.__resBuf = r.rest;
        if (!r.done) return;
        this.__res.__complete();
        this.__finishResponse(false);
        return;
      }
    }
    // 响应收齐：归还池或半关；req 'close' 在 res 'end' 之后发出（Node 时序）。
    __finishResponse(fromClose) {
      if (this.__respDone) return;
      this.__respDone = true;
      const sock = this.__sock;
      this.__sock = null;
      const conn = this.__res !== null ? (this.__res.headers.connection || "").toLowerCase() : "close";
      const poolable = !fromClose && sock !== null && this.agent !== null && this.agent.keepAlive && conn !== "close";
      if (poolable) {
        this.agent.__release(sock, this.__key, this);
      } else if (!fromClose && sock !== null) {
        try { sock.end(); } catch { /* closed meanwhile */ }
      }
      const emitClose = () => {
        if (!this.__closeEmitted) {
          this.__closeEmitted = true;
          this.emit("close");
        }
      };
      if (this.__res !== null && !this.__res.readableEnded) {
        this.__res.once("end", emitClose);
      } else {
        emitClose();
      }
    }
    __onSockCloseEv() {
      if (this.__closeEmitted) return;
      if (this.__res !== null && !this.__res.complete) {
        if (this.__framing !== null && this.__framing.type === "close") {
          this.__res.__complete();
          this.__finishResponse(true);
          return;
        }
        // 意外截断：沿 9d 宽容口径直接结束（不抛）。
        this.__res.complete = true;
        this.__res.push(null);
        this.__finishResponse(true);
        return;
      }
      if (this.__res === null && !this.__respDone) {
        this.__closeEmitted = true;
        this.emit("close");
      }
    }
    destroy(err) {
      if (this.destroyed) return this;
      // super.destroy 会走 _destroy（清 socket + 补 'close'）。
      return super.destroy(err);
    }
  };
}

// Agent：node lib/_http_agent.js 口径的函数式构造器——`http.Agent({...})` 无 new
// 亦合法（keepalive-client/free/override 系套件点名）。键位统一走 getName 形
// （'host:port:localAddress(:family)'，缺省位仍带分隔冒号——agent-getname 套件）。
function Agent(options = {}) {
  if (!(this instanceof Agent)) return new Agent(options);
  Agent.prototype.__init.call(this, options);
}
Object.setPrototypeOf(Agent.prototype, EventEmitter.prototype);
Object.setPrototypeOf(Agent, EventEmitter);
Agent.prototype.__init = function (options = {}) {
  EventEmitter.call(this);
  this.options = options ?? {};
  this.keepAlive = options.keepAlive ?? false;
  this.keepAliveMsecs = options.keepAliveMsecs ?? 1000;
  this.maxSockets = options.maxSockets ?? Infinity;
  this.maxFreeSockets = options.maxFreeSockets ?? 256;
  this.maxTotalSockets = options.maxTotalSockets ?? Infinity;
  this.totalSocketCount = 0;
  this.scheduling = options.scheduling ?? "lifo";
  this.sockets = {};
  this.freeSockets = {};
  this.requests = {};
  // __openSocket/__defaultPort 由 flavor 子类经 prototype 提供（本类不设 own
  // 属性，否则遮蔽子类覆盖；裸 BaseAgent 直接用即 TypeError）。
};
// node 口径：createConnection 是 agent 的建连钩（测试以假 Duplex 覆盖做黑洞/
// 依此注入 socket）；默认回落 flavor 的 __openSocket。同步回值与 cb 双形态，
// settled 旗防双取。
Agent.prototype.createConnection = function (options, cb) {
  const s = this.__openSocket(options.host, options.port, options);
  if (typeof cb === "function") cb(null, s);
  return s;
};
Agent.prototype.getName = function (options = {}) {
  let name = options.host ?? options.hostname ?? "localhost";
  name += ":";
  if (options.port) name += options.port;
  name += ":";
  if (options.localAddress) name += options.localAddress;
  if (options.family === 4 || options.family === 6) name += ":" + options.family;
  return name;
};
Agent.prototype.__list = function (map, key) {
  if (map[key] === undefined) map[key] = [];
  return map[key];
};
Agent.prototype.__liveCount = function (key) {
  const all = this.__list(this.sockets, key).filter((s) => !s.destroyed);
  return all.length;
};
// 建连：走 createConnection 钩（同步回值/cb 双形态），返回 socket 或 undefined。
Agent.prototype.__createSock = function (options) {
  let out;
  let settled = false;
  const oncreate = (err, s) => {
    settled = true;
    if (!err && s) out = s;
  };
  const maybe = this.createConnection(options, oncreate);
  if (!settled && maybe) out = maybe;
  return out;
};
Agent.prototype.__trackSocket = function (sock, key) {
  this.__list(this.sockets, key).push(sock);
  this.totalSocketCount++;
  const cleaner = () => this.__noteClosed(sock);
  sock.__poolCleaner = cleaner;
  sock.on("close", cleaner);
};
Agent.prototype.__unpool = function (sock) {
  sock.__inPool = false;
  if (sock.__poolCleaner !== undefined) {
    try { sock.removeListener("close", sock.__poolCleaner); } catch { /* gone */ }
    sock.__poolCleaner = undefined;
  }
};
// 取空闲或新建；满额则排队（release 时续行）。onSocket(sock, reused)。
Agent.prototype.__acquire = function (req, host, port, extra, onSocket) {
  const key = this.getName({ host, port, ...(extra ?? {}) });
  const free = this.__list(this.freeSockets, key);
  while (free.length > 0) {
    const sock = this.scheduling === "fifo" ? free.shift() : free.pop();
    if (!sock.destroyed) {
      this.__unpool(sock);
      req.__poolKey = key;
      onSocket(sock, true);
      return;
    }
  }
  if (this.__liveCount(key) >= this.maxSockets) {
    this.__list(this.requests, key).push({ req, host, port, extra, onSocket });
    req.__poolKey = key;
    req.__queued = true;
    return;
  }
  req.__poolKey = key;
  req.__queued = false;
  const sock = this.__createSock({ host, port, ...(extra ?? {}) });
  if (!sock) {
    const err = new Error("socket hang up");
    err.code = "ECONNREFUSED";
    req.destroy(err);
    return;
  }
  this.__trackSocket(sock, key);
  onSocket(sock, false);
};
Agent.prototype.__noteClosed = function (sock) {
  const key = sock.__poolKey;
  if (key === undefined) return;
  const drop = (map) => {
    const arr = map[key];
    if (arr !== undefined) {
      const i = arr.indexOf(sock);
      if (i !== -1) arr.splice(i, 1);
    }
  };
  drop(this.sockets);
  drop(this.freeSockets);
};
Agent.prototype.__release = function (sock, key, req) {
  if (req !== null && req !== undefined) req.__queued = false;
  if (sock.destroyed || !this.keepAlive) {
    if (!sock.destroyed) {
      try { sock.destroy(); } catch { /* gone */ }
    }
    this.__noteClosed(sock);
  } else {
    const free = this.__list(this.freeSockets, key);
    if (free.length >= this.maxFreeSockets) {
      try { sock.destroy(); } catch { /* gone */ }
      this.__noteClosed(sock);
    } else {
      sock.__inPool = true;
      free.push(sock);
      if (sock.__poolCleaner === undefined) {
        const cleaner = () => this.__noteClosed(sock);
        sock.__poolCleaner = cleaner;
        sock.on("close", cleaner);
      }
    }
  }
  // 续行排队请求。
  const q = this.__list(this.requests, key);
  while (q.length > 0) {
    const next = q.shift();
    if (next.req.destroyed) continue;
    next.req.__queued = false;
    this.__acquire(next.req, next.host, next.port, next.extra, next.onSocket);
    break;
  }
};
Agent.prototype.__cancel = function (req) {
  const key = req.__poolKey;
  if (key === undefined || !req.__queued) return;
  req.__queued = false;
  const q = this.__list(this.requests, key);
  const i = q.findIndex((e) => e.req === req);
  if (i !== -1) q.splice(i, 1);
};
// node lib/_http_agent.js addRequest 口径（freeSockets 直投/建连/排队三路）：
// 外部直塞 freeSockets 再 addRequest 即复用（agent-uninitialized 套件）。
Agent.prototype.addRequest = function (req, options, port, localAddress) {
  if (typeof options === "string") options = { host: options, port, localAddress };
  options = { ...(options ?? {}), ...(this.options ?? {}) };
  if (options.socketPath) options.path = options.socketPath;
  const name = this.getName(options);
  this.__list(this.sockets, name);
  const free = this.freeSockets[name];
  let sock;
  if (free) {
    while (free.length > 0 && free[0].destroyed) free.shift();
    sock = this.scheduling === "fifo" ? free.shift() : free.pop();
    if (free.length === 0) delete this.freeSockets[name];
  }
  if (sock) {
    this.__unpool(sock);
    this.__list(this.sockets, name).push(sock);
    req.__poolKey = name;
    req.__queued = false;
    if (typeof req.__attach === "function") req.__attach(sock, true);
    return;
  }
  if (this.__liveCount(name) >= this.maxSockets) {
    this.__list(this.requests, name).push({ req, host: options.host, port: options.port, extra: options, onSocket: (s, reused) => req.__attach(s, reused) });
    req.__poolKey = name;
    req.__queued = true;
    return;
  }
  req.__poolKey = name;
  req.__queued = false;
  const s = this.__createSock({ host: options.host ?? options.hostname ?? "localhost", port: options.port ?? this.__defaultPort ?? 80, ...options });
  if (!s) {
    const err = new Error("socket hang up");
    err.code = "ECONNREFUSED";
    req.destroy(err);
    return;
  }
  this.__trackSocket(s, name);
  if (typeof req.__attach === "function") req.__attach(s, false);
};
Agent.prototype.destroy = function () {
  for (const key of Object.keys(this.requests)) {
    const q = this.requests[key];
    this.requests[key] = [];
    for (const { req } of q) {
      if (!req.destroyed) {
        const err = new Error("socket hang up");
        err.code = "ECONNRESET";
        req.destroy(err);
      }
    }
  }
  for (const key of Object.keys(this.sockets)) {
    const arr = this.sockets[key];
    this.sockets[key] = [];
    for (const sock of arr) {
      try { sock.destroy(); } catch { /* gone */ }
    }
  }
  this.freeSockets = {};
};
export { Agent };

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
  return req;
}
export function getFrom(ClientRequest, options, cb) {
  const req = requestFrom(ClientRequest, options, cb);
  req.end();
  return req;
}
"#;
