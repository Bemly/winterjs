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

// node 内部符号（_http_server re-export；close-destroy-timeout/async-dispose 套件）
export const kConnectionsCheckingInterval = Symbol("kConnectionsCheckingInterval");
export const kServerResponse = Symbol("kServerResponse");
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
// 头值严格门（node llhttp strict 口径，真机 26.8.2 实测）：值内允许 HTAB、
// 0x20-0x7E、0x80-0xFF；其余控制字符（如 \x08）仅 insecureHTTPParser 放行。
function __validHeaderValue(v) {
  for (let i = 0; i < v.length; i++) {
    const cc = v.charCodeAt(i);
    if (cc !== 9 && (cc < 32 || cc === 127)) return false;
  }
  return true;
}
function __parseHead(headText, strict) {
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
    const vRaw = line.slice(c + 1);
    const v = vRaw.trim();
    // 严格门查原始值（trim 前）——前导控制字符（如 'x:\nTE' 的裸 LF）不得
    // 被 trim 吞掉而漏检（missing-header-separator 套件现场记录）。
    if (strict && !__validHeaderValue(vRaw)) throw __mkParseError("invalid header value");
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
// 头值字符门（node checkInvalidHeaderChar 口径）——违者 ERR_INVALID_CHAR
//（header-validators 套件；真机 message 'Invalid character in header content'）。
function __validateHeaderValue(v) {
  if (!__validHeaderValue(String(v))) {
    throw new codes.ERR_INVALID_CHAR("Invalid character in header content");
  }
}
function __lowerHeaders(obj) {
  const out = Object.create(null);
  for (const [k, v] of Object.entries(obj ?? {})) {
    // 头名字门（node checkIsHttpToken 口径；invalidheaderfield 套件）。
    if (!__TOKEN_RE.test(k)) throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", k);
    out[k.toLowerCase()] = String(v);
  }
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
// Expect: 100-continue 判据（真机 26.8.2 对拍：'100-continue'/'100-Continue'/
// 'foo, 100-continue'/'100-continue, foo' 命中；'100continue'（无连字符）与
// '200-ok' 不命中 → 417 通道）。
const __EXPECT_CONTINUE_RE = /(?:^|[^\w])100-continue(?![\w])/i;
// 泵收尾把 trailer 落到消息上（rawTrailers 保存原拼写；trailers 小写键）。
function __applyTrailers(msg, trailersRaw) {
  msg.rawTrailers = [];
  msg.trailers = {};
  for (let i = 0; i < trailersRaw.length; i += 2) {
    msg.rawTrailers.push(trailersRaw[i], trailersRaw[i + 1]);
    msg.trailers[trailersRaw[i].toLowerCase()] = trailersRaw[i + 1];
  }
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

// 请求行前缀增量校验（llhttp strict 口径，真机 26.8.2 实测）：
// - 方法段：token 字符；空格转入 URL 段（空方法/行首空格即 400）；
// - URL 段：首字节须 '/'（origin-form）、'*'（asterisk-form）或 CONNECT 的
//   authority-form（token 字符）；控制字节即 400（'hello world' 现场记录）；
// - 版本段：整行形状由 __validateRequestHead 终验；此处只拒空格/控制字节；
// - CR 在版本段 = 行终结起点（等 CRLF 由 headEnd 扫描接手），其余位置即 400。
function __checkRequestLinePrefix(buf) {
  let seg = 0;
  let method = "";
  let urlStarted = false;
  for (let i = 0; i < buf.length; i++) {
    const ch = buf[i];
    if (ch === 13) {
      if (seg === 2) return;
      throw __mkParseError("bad request line");
    }
    if (ch === 10 || ch === 0) throw __mkParseError("bad request line");
    if (ch === 32) {
      if (seg === 2 || (seg === 0 && method === "") || (seg === 1 && !urlStarted)) {
        throw __mkParseError("bad request line");
      }
      seg++;
      continue;
    }
    if (seg === 0) {
      if (!__TOKEN_RE.test(String.fromCharCode(ch))) throw __mkParseError("bad request line");
      method += String.fromCharCode(ch);
    } else if (seg === 1) {
      if (!urlStarted) {
        urlStarted = true;
        if (ch !== 47 && ch !== 42 && method !== "CONNECT") throw __mkParseError("bad request line");
      } else if (ch < 33 || ch === 127) {
        throw __mkParseError("bad request line");
      }
    } else {
      if (ch < 32 || ch === 127) throw __mkParseError("bad request line");
    }
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
      // 终结段：trailer 块逐行计数到空行。名+值累计（': ' 与 CRLF 不计）
      // ≥ 16KiB 即 431（真机阈值：16383 过 / 16384 拒，跨行累计）；
      // 无冒号行 400（真机 `justname` 行即拒）。10f G11 起内容收集
      //（真机口径：req.trailers/res.trailers 在场；raw 保存原拼写）。
      let eol = -1;
      for (let i = 0; i + 1 < buf.length; i++) {
        if (buf[i] === 13 && buf[i + 1] === 10) { eol = i; break; }
      }
      if (eol === -1) { fr.buf = buf; return { done: false, rest: new Uint8Array(0) }; }
      if (eol === 0) {
        const raw = fr.__trRaw ?? [];
        return { done: true, rest: buf.slice(2), trailersRaw: raw.length > 0 ? raw : undefined };
      }
      const lineText = __latin1(buf.slice(0, eol));
      const c = lineText.indexOf(":");
      if (c <= 0) return { error: 400 };
      fr.__tnv = (fr.__tnv === undefined ? 0 : fr.__tnv) + c + lineText.slice(c + 1).trim().length;
      if (fr.__tnv >= __MAX_TRAILER_NV) return { error: 431 };
      if (fr.__trRaw === undefined) fr.__trRaw = [];
      fr.__trRaw.push(lineText.slice(0, c).trim(), lineText.slice(c + 1).trim());
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
    this.httpVersionMajor = 1;
    this.httpVersionMinor = 1;
    this.method = null;
    this.url = null;
    this.statusCode = null;
    this.statusMessage = null;
    this.headers = {};
    this.rawHeaders = [];
    this.trailers = {};
    this.rawTrailers = [];
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
    // node _storeHeader 决策表旗标（真机 26.8.2 + lib/_http_outgoing.js 对拍）：
    // __uced = useChunkedEncodingByDefault（1.1 恒 true；1.0 = /chunked/i.test
    // 请求 TE 头）；__last = 响应后关连接（node _last）；__keepAlive = llhttp
    // shouldKeepAlive；__contentLength 由 end() 快路径预置（node _contentLength）。
    this.__headStored = false;
    this.__uced = true;
    this.__last = false;
    this.__keepAlive = false;
    this.__chunked = false;
    this.__rawCL = false;
    this.__contentLength = undefined;
    this.__kaTimeout = undefined;
    this.__maxReq = 0;
    this.__maxReqReached = false;
    this.__defaultKA = true;
    this.__buf1 = null;
    this.__holdTimer = null;
    this.__headOnly = false;
    this.__noBody = false;
    this.__userEnded = false;
    this.__onDone = null;
    // 独立构造：从 req 形对象提取版本/方法面（node ServerResponse ctor 口径）。
    if (this.__sock === null && sock && typeof sock === "object") {
      if (sock.method === "HEAD") this.__headOnly = true;
      const hv = String(sock.httpVersion ?? "1.1");
      if (hv === "1.0") {
        this.__uced = /(?:^|\W)chunked/i.test(sock.headers?.te ?? "");
        this.__keepAlive = false;
      }
    }
  }
  setHeader(name, value) {
    if (!__TOKEN_RE.test(String(name))) throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", String(name));
    const lk = String(name).toLowerCase();
    this.__headers[lk] = String(value);
    return this;
  }
  getHeader(name) { return this.__headers[String(name).toLowerCase()]; }
  removeHeader(name) { delete this.__headers[String(name).toLowerCase()]; return this; }
  getHeaderNames() { return Object.keys(this.__headers); }
  hasHeader(name) { return this.__headers[String(name).toLowerCase()] !== undefined; }
  writeHead(status, ...rest) {
    // 状态码门（node validateStatusCode 口径，response-statuscode 套件 13 形态：
    // undefined/Infinity/NaN/{}/99/1000/'1000'/null/true/[]/'this is not valid'/
    // '404 ...' 全拒；message 'Invalid status code: <String(值)>'，RangeError）。
    if (typeof status !== "number" || !(status >= 100 && status <= 999)) {
      throw new codes.ERR_HTTP_INVALID_STATUS_CODE(`Invalid status code: ${String(status)}`);
    }
    const obj = rest.find((r) => r && typeof r === "object");
    const msg = rest.find((r) => typeof r === "string");
    this.statusCode = status;
    if (msg !== undefined) this.statusMessage = msg;
    Object.assign(this.__headers, __lowerHeaders(obj));
    // 头已存：随后的 end(data) 不再走 CL 快路径（真机 chunked 口径）。
    this.__headStored = true;
    return this;
  }
  // node writeContinue：headersSent 前一次性发 100 Continue 中间响应。
  writeContinue() {
    if (this.__continueSent || this.headersSent || this.__headSent) return;
    this.__continueSent = true;
    if (this.__sock !== null) {
      try { this.__sock.write(new TextEncoder().encode("HTTP/1.1 100 Continue\r\n\r\n")); } catch { /* gone */ }
    }
  }
  // node setHeaders：只收 Headers 实例（entries 方法），否则 ERR_INVALID_ARG_TYPE。
  setHeaders(headers) {
    if (headers === null || typeof headers !== "object" || typeof headers.entries !== "function") {
      throw new codes.ERR_INVALID_ARG_TYPE("headers", ["Headers instance"], headers);
    }
    for (const [k, v] of headers.entries()) this.setHeader(k, v);
    return this;
  }
  // node addTrailers：分块响应终结块尾随头（原拼写输出；真机 rawTrailers 口径）。
  addTrailers(trailers) {
    const lowered = __lowerHeaders(trailers ?? {});
    for (const k of Object.keys(lowered)) {
      __validateHeaderValue(lowered[k]);
      this.__trailer = (this.__trailer ?? "") + `${k}: ${lowered[k]}\r\n`;
    }
    return this;
  }
  write(chunk, encoding, cb) {
    if (this.__userEnded) return __writeAfterEnd(this, encoding, cb);
    if (this.__sockGone || this.__sock === null || this.__sock.destroyed) return false;
    return super.write(chunk, encoding, cb);
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
    if (this.__headers["content-length"] === undefined && this.__headers["transfer-encoding"] === undefined && this.__uced) {
      this.__chunked = true;
    }
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
    // 用户头预处理（node matchHeader 口径）：
    // connection close → _last；connection 其他 → shouldKeepAlive=true；
    // TE chunked → chunked 帧；CL → raw；keep-alive 头在场 → 抑制自动 Keep-Alive。
    const __st = { conn: false, cl: false, te: false, trailer: false };
    for (const k of Object.keys(this.__headers)) {
      const lk = k.toLowerCase();
      const v = String(this.__headers[k]);
      if (lk === "connection") {
        __st.conn = true;
        if (/(?:^|\W)close(?:\W|$)/i.test(v)) this.__last = true;
        else this.__keepAlive = true;
      } else if (lk === "transfer-encoding") {
        __st.te = true;
        if (/(?:^|\W)chunked/i.test(v)) this.__chunked = true;
      } else if (lk === "content-length") {
        __st.cl = true;
        this.__rawCL = true;
      } else if (lk === "trailer") {
        __st.trailer = true;
      } else if (lk === "keep-alive") {
        this.__defaultKA = false;
      }
    }
    // 204/304 + chunked → 抑制零块 + 强制关连接（node _storeHeader 开头口径，
    // chunked-304 套件：响应带 Connection: close 且无 0\r\n 零块）。
    if (this.statusCode === 204 || this.statusCode === 304) {
      if (this.__chunked) { this.__chunked = false; this.__keepAlive = false; }
      this.__noBody = true;
    }
    const reason = this.statusMessage ?? STATUS_CODES[this.statusCode] ?? "";
    const head = [`HTTP/1.1 ${this.statusCode} ${reason}`.trimEnd()];
    // Connection 自动决策（node keep-alive logic 口径）：
    // shouldSendKeepAlive = shouldKeepAlive && (用户CL || UCED)；
    // maxRequestsPerSocket 达标 → close；否则 keep-alive（+Keep-Alive: timeout）；
    // 否则 close + _last。
    if (!__st.conn) {
      const shouldSendKeepAlive = this.__keepAlive && (__st.cl || this.__uced);
      if (shouldSendKeepAlive && this.__maxReqReached) {
        this.__headers["connection"] = "close";
        this.__autoConn = true;
        this.__last = true;
      } else if (shouldSendKeepAlive) {
        this.__headers["connection"] = "keep-alive";
        this.__autoConn = true;
        if ((this.__kaTimeout ?? 0) > 0 && this.__defaultKA) {
          const t = Math.floor(this.__kaTimeout / 1000);
          const max = this.__maxReq > 0 ? `, max=${this.__maxReq}` : "";
          this.__headers["keep-alive"] = `timeout=${t}${max}`;
          this.__autoKA = true;
        }
      } else {
        this.__headers["connection"] = "close";
        this.__autoConn = true;
        this.__last = true;
      }
    }
    // 自动 Date 头（node 口径：响应缺 date 即补 UTC 串）。
    if (this.__headers["date"] === undefined) {
      this.__headers["date"] = new Date().toUTCString();
      this.__autoDate = true;
    }
    // 帧决策（node：!contLen && !te 分支）：无用户 CL/TE 时按
    // noBody/UCED/__contentLength（end() 快路径预置）决定 auto CL 或 chunked；
    // 1.0（UCED false）→ _last（close-delimited）。
    if (!__st.cl && !__st.te) {
      if (this.__noBody || this.__headOnly) {
        this.__chunked = false;
      } else if (!this.__uced) {
        this.__last = true;
      } else if (!__st.trailer && this.__contentLength !== undefined) {
        this.__headers["content-length"] = String(this.__contentLength);
        this.__rawCL = true;
      } else {
        this.__headers["transfer-encoding"] = "chunked";
        this.__chunked = true;
      }
    }
    // 自设头按用户拼写输出（node verbatim；本仓内部统一小写存取）；自动头的
    // 真机输出是规范大写（Transfer-Encoding/Content-Length/自动 Connection/
    // Date/Keep-Alive）。
    const canon = { "transfer-encoding": "Transfer-Encoding", "content-length": "Content-Length" };
    const __autoCase = (k) => (k === "connection" && this.__autoConn) || (k === "date" && this.__autoDate) ||
      (k === "keep-alive" && this.__autoKA);
    for (const [k, v] of Object.entries(this.__headers)) {
      const name = __autoCase(k) ? (k === "keep-alive" ? "Keep-Alive" : k.charAt(0).toUpperCase() + k.slice(1)) : (canon[k] ?? k);
      head.push(`${name}: ${v}`);
    }
    return new TextEncoder().encode(head.join("\r\n") + "\r\n\r\n");
  }
  __sendHead() {
    // node 口径：header 独立成 write；首块合并只发生在 _final/flushFinal 快捷路。
    const head = this.__headBytes();
    if (head.length > 0) this.__sock.write(head);
  }
  // chunked 终结块：`0\r\n` + trailer 行 + 空行（addTrailers 的 trailer 跟尾）。
  __chunkTerminator() {
    return "0\r\n" + (this.__trailer ?? "") + "\r\n";
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
          if (this.__uced && this.__headers["content-length"] === undefined && this.__headers["transfer-encoding"] === undefined) this.__chunked = true;
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
      if (this.__uced && this.__headers["content-length"] === undefined && this.__headers["transfer-encoding"] === undefined) this.__chunked = true;
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
    if (this.__sockGone || this.__sock === null || this.__sock.destroyed) {
      cb();
      return;
    }
    if (!this.__headSent) {
      // CL 快捷：end() 为首个头触发点（无 writeHead/write 前置）且 UCED 时，
      // __contentLength 预置（node _contentLength 口径；end() 裸调 = 0）；
      // write/writeHead 在前 → chunked；1.0 无 TE → 裸体（_last close-delimited）。
      const total = this.__buf1 !== null ? this.__buf1.length : 0;
      if (!this.__noBody && !this.__headOnly &&
          this.__headers["content-length"] === undefined &&
          this.__headers["transfer-encoding"] === undefined) {
        if (this.__uced && !this.__headStored && (!this.__sawWrite || this.__endHadData)) {
          this.__contentLength = this.__endHadData ? total : 0;
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
            this.__sock.write(new TextEncoder().encode(this.__chunkTerminator()));
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
            this.__sock.write(new TextEncoder().encode(this.__chunkTerminator()));
          }
        }
      }
    } else if (this.__chunked && !this.__rawCL && !this.__noBody && !this.__headOnly) {
      this.__sock.write(new TextEncoder().encode(this.__chunkTerminator()));
    }
    const cont = this.__onDone;
    this.__onDone = null;
    if (this.__last) {
      // node _last 口径：响应后关连接（close-delimited/显式 close/1.0 裸体）。
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
  // node 口径：destroy(err) 不外发 'error'（仅记 errored），异步发一次 'close'
  //（outgoing-destroyed 套件：destroyed/closed/errored 三面 + close 事件）。
  destroy(err) {
    if (this.destroyed) return this;
    this.__omErrored = err ?? null;
    const __swallow = () => {};
    this.on("error", __swallow);
    const __ret = super.destroy(err);
    queueMicrotask(() => {
      this.removeListener("error", __swallow);
      if (!this.__closeEmitted) {
        this.__closeEmitted = true;
        this.emit("close");
      }
    });
    return __ret;
  }
  get errored() {
    return this.__omErrored ?? (this._writableState ? this._writableState.errored : null);
  }
}

// 头段内裸 CR（后随非 LF）检测——client/server 两侧严格门共用
//（client-reject-cr-no-lf 套件；服务端同形走 400 通道）。
function __hasBareCR(headText) {
  for (let i = 0; i < headText.length; i++) {
    if (headText[i] === "\r" && headText[i + 1] !== "\n") return true;
  }
  return false;
}

// 客户端解析错（node llhttp 口径）：message 'Parse Error: ...' + HPE_* 码
//（client-reject-* 套件断言 err.code 与 /^Parse Error/）。
function __hpe(code, msg) {
  const e = new Error(`Parse Error: ${msg}`);
  e.code = code;
  return e;
}

// node Writable 口径：end 后写不抛——错误走 cb（有则）或下一拍 'error'
//（一次）；errored 后续写静默 false（server-write-after-end/outgoing-destroyed
// 套件真机对拍）。
// node Writable 口径：end 后写不抛——错误走 cb（有则）或下一拍 'error'
//（一次）；errored 后续写静默 false（server-write-after-end/outgoing-destroyed
// 套件真机对拍）。
function __writeAfterEnd(msg, encoding, cb) {
  if (msg.__waeErrored || msg.__waeQueued) return false;
  msg.__waeQueued = true;
  const f = typeof encoding === "function" ? encoding : cb;
  queueMicrotask(() => {
    msg.__waeQueued = false;
    msg.__waeErrored = true;
    const err = new Error("ERR_STREAM_WRITE_AFTER_END: write after end");
    err.code = "ERR_STREAM_WRITE_AFTER_END";
    if (typeof f === "function") f(err);
    else msg.emit("error", err);
  });
  return false;
}

// 服务端混入：Base = net.Server / tls.Server（构造实参原样透传基类）。
// http 面选项（10f 对拍，node lib/_http_server.js 口径）：requestTimeout 默认
// 300000、headersTimeout 默认 min(60000, requestTimeout)、keepAliveTimeout 5000、
// keepAliveTimeoutBuffer 1000；headersTimeout > requestTimeout 即 ERR_OUT_OF_RANGE。
export function withHttpServer(Base) {
  // 初始化逻辑独立成函数：`new Server()` 走构造器，`http.Server.call(this)`
  //（upgrade-server 套件 testServer 老式继承）直接在 this 上跑同一段。
  function __initServer(self, args) {
      const o = (args[0] && typeof args[0] === "object" && !Array.isArray(args[0])) ? args[0] : {};
      self.timeout = 0;
      self.requestTimeout = 300_000;
      self.headersTimeout = 60_000;
      self.keepAliveTimeout = 5_000;
      self.keepAliveTimeoutBuffer = 1_000;
      self.maxRequestsPerSocket = 0;
      // 每服务器宽松解析旗（insecure-parser-per-stream 套件）。
      self.insecureHTTPParser = o.insecureHTTPParser ?? false;
      const rt = o.requestTimeout !== undefined ? __validateInteger(o.requestTimeout, "requestTimeout") : undefined;
      if (rt !== undefined) self.requestTimeout = rt;
      const ht = o.headersTimeout !== undefined ? __validateInteger(o.headersTimeout, "headersTimeout") : undefined;
      self.headersTimeout = ht !== undefined ? ht : Math.min(60_000, self.requestTimeout);
      if (self.requestTimeout > 0 && self.headersTimeout > 0 && self.headersTimeout > self.requestTimeout) {
        throw new codes.ERR_OUT_OF_RANGE("headersTimeout", "<= requestTimeout", o.headersTimeout);
      }
      const kt = o.keepAliveTimeout !== undefined ? __validateInteger(o.keepAliveTimeout, "keepAliveTimeout") : undefined;
      if (kt !== undefined) self.keepAliveTimeout = kt;
      const kb = o.keepAliveTimeoutBuffer !== undefined ? __validateInteger(o.keepAliveTimeoutBuffer, "keepAliveTimeoutBuffer") : undefined;
      if (kb !== undefined) self.keepAliveTimeoutBuffer = kb;
      if (o.maxRequestsPerSocket !== undefined) self.maxRequestsPerSocket = o.maxRequestsPerSocket;
      self.__closing = false;
      self.__sockets = new Set();
      self.on("connection", (sock) => {
        self.__sockets.add(sock);
        const st = { buf: new Uint8Array(0), req: null, framing: null, res: null, __hdT: null, __rqT: null, __kaT: null };
        sock.__httpState = st;
        sock.on("close", () => {
          self.__clearReqTimers(st);
          self.__sockets.delete(sock);
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
        if (self.timeout > 0) sock.setTimeout(self.timeout);
        sock.on("timeout", () => {
          if (!sock.destroyed) self.emit("timeout", sock);
        });
        sock.on("data", (chunk) => {
          if (self.__closing || sock.__upgraded) return;
          if (self.timeout > 0) sock.setTimeout(self.timeout);
          try {
            self.__feed(sock, st, chunk);
          } catch (e) {
            self.__feedError(sock, e);
          }
        });
        // node socketOnEnd 口径：客户端 FIN——非 half-open 直接销毁（res 'close'
        // 经 close 处理器）；half-open 留给响应自身收口（server.js 套件的半关
        // 后续响应仍须可写），被截断的请求体提前夭折（'aborted' 语义）。
        sock.on("end", () => {
          sock.__finReceived = true;
          if (!self.httpAllowHalfOpen) {
            try { sock.destroy(); } catch { /* gone */ }
          } else {
            if (st.req !== null && !st.req.complete && !st.req.destroyed) {
              st.req.destroy();
            }
            if (st.res !== null) {
              st.res.once("close", () => {
                try { sock.destroy(); } catch { /* gone */ }
              });
            }
          }
        });
        // 连接即开 headers 计时（headersTimeout 内须收到完整头，否则 408）。
        self.__armIdleTimers(st, sock);
      });
  }
  class __HttpServer extends Base {
    constructor(...args) {
      super(...args);
      __initServer(this, args);
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
        // node 口径：clientError 事件恒发；无监听才落默认 400 + 销毁。
        this.emit("clientError", e, sock);
        if (this.listenerCount("clientError") === 0) this.__badRequest(sock);
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
            // 头段超出 maxHeaderSize：431 Request Header Fields Too Large
            //（llhttp HPE_HEADER_OVERFLOW；header-overflow 套件精确字节）。
            if (st.buf.length > maxHeaderSize) { this.__headerFieldsTooLarge(sock); return; }
            // 头未齐也可先校验请求行（llhttp 增量语义；管线残渣 "hello world"
            // 在 URL 段首字节即 400，等不到行终结——blank-header 套件）。
            // llhttp 口径：请求行前的 CRLF 空行容忍（管线残段；insecure-parser
            // 套件尾部 '\r\n\r\n' 现场记录），先吞再校验。
            while (st.buf.length >= 2 && st.buf[0] === 13 && st.buf[1] === 10) {
              st.buf = st.buf.slice(2);
            }
            __checkRequestLinePrefix(st.buf);
            return;
          }
          const headText = __latin1(st.buf.slice(0, headEnd));
          if (this.insecureHTTPParser !== true && __hasBareCR(headText)) {
            throw __mkParseError("LF expected after CR");
          }
          const { first, headers, rawHeaders } = __parseHead(headText, this.insecureHTTPParser !== true);
          __validateRequestHead(first, headers);
          const req = new IncomingMessage();
          req.method = first[0];
          req.url = first[1];
          req.httpVersion = first[2].replace("HTTP/", "");
          {
            const __vv = req.httpVersion.split(".");
            req.httpVersionMajor = Number(__vv[0] ?? 1) || 0;
            req.httpVersionMinor = Number(__vv[1] ?? 1) || 0;
          }
          req.headers = headers;
          req.rawHeaders = rawHeaders;
          req.socket = sock;
          req.connection = sock;
          // Node 口径：CONNECT 方法请求不进 request 管线——派发 'connect'
          //（req, socket, head；无监听则销毁连接），socket 停止 HTTP 解析。
          if (req.method === "CONNECT") {
            sock.__upgraded = true;
            const __leftover = st.buf.slice(headEnd + 4);
            st.buf = new Uint8Array(0);
            if (this.listenerCount("connect") > 0) {
              this.emit("connect", req, sock, globalThis.Buffer.from(__leftover));
            } else {
              sock.destroy();
            }
            return;
          }
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
          // node ServerResponse ctor 口径：UCED 1.1 恒 true；1.0 = 请求 TE 头
          // 含 chunked（真机 1.0-keep-alive 套件 TE: chunked 形）。
          res.__uced = req.httpVersion === "1.1" ? true : /(?:^|\W)chunked/i.test(headers.te ?? "");
          res.req = req;
          req.res = res;
          res.__keepAlive = keepAlive;
          res.__headOnly = req.method === "HEAD";
          // 响应头决策所需服务端上下文（Keep-Alive: timeout / maxRequestsPerSocket）。
          res.__kaTimeout = this.keepAliveTimeout;
          res.__maxReq = this.maxRequestsPerSocket;
          st.reqCount = (st.reqCount ?? 0) + 1;
          if (this.maxRequestsPerSocket > 0 && st.reqCount >= this.maxRequestsPerSocket) {
            // node 口径：达额请求的响应带 Connection: close（响应完关连接）。
            res.__maxReqReached = true;
            res.__keepAlive = false;
          }
          // Expect 头三路（node parserOnIncoming 口径，真机 26.8.2 对拍）：
          // 100-continue → checkContinue 监听在场发它、否则默认 writeContinue+request；
          // 其他期望值 → checkExpectation 监听在场发它、否则默认 417 应急停
          //（响应后原请求体进丢弃泵，st.req 不接线）。
          let __wired = true;
          let __ev = "request";
          const __expect = headers.expect;
          if (__expect !== undefined) {
            if (__EXPECT_CONTINUE_RE.test(__expect)) {
              if (this.listenerCount("checkContinue") > 0) __ev = "checkContinue";
              else res.writeContinue();
            } else if (this.listenerCount("checkExpectation") > 0) {
              __ev = "checkExpectation";
            } else {
              __wired = false;
              __ev = null;
              res.statusCode = 417;
            }
          }
          st.req = __wired ? req : null;
          st.framing = framing;
          st.res = res;
          // 头已齐、体在途：消息期 requestTimeout 计时。
          this.__armMsgTimer(st, sock);
          res.__onDone = () => {
            st.req = null;
            st.framing = null;
            st.res = null;
            st.sawRequest = true;
            // 半关连接（客户端已 FIN）且这是最后一个在途响应：收口不续
            // keep-alive；还有管线中响应（st.res 已是后继请求的 res）则继续。
            if (sock.__finReceived && st.res === res) {
              try { sock.destroy(); } catch { /* gone */ }
              return;
            }
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
          // node 口径：server.close() 只停监听，既有连接上的后续管线请求照常
          // 服务（pipeline-assertionerror-finish 套件 mustCall(10) 点名；
          // 原自创 503 路径会使后续响应写进已 end 的 socket）。
          st.buf = st.buf.slice(headEnd + 4);
          if (__wired) {
            // §4.35：先 emit("request")（监听器登记 data/end），再喂体。
            this.emit(__ev, req, res);
          } else {
            // 417 默认路径：响应立即收尾；后续体字节走丢弃泵（st.req 为 null）。
            res.end();
          }
          continue;
        }
        // 体泵：CL / chunked 增量；none 直接完结（st.req 为 null = 417 丢弃泵）。
        const fr = st.framing;
        if (fr.type === "none") {
          if (st.req !== null) st.req.__complete();
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
        if (r.trailersRaw !== undefined && st.req !== null) __applyTrailers(st.req, r.trailersRaw);
        if (st.req !== null) st.req.__complete();
        st.req = null;
        st.framing = null;
        this.__armIdleTimers(st, sock);
      }
    }
    close(cb) {
      this.__closing = true;
      if (typeof cb === "function") this.once("close", cb);
      // node close 口径：空闲连接销毁；仍有在途响应的连接 unref（进程不等待，
      // 响应落地后自然退出——pipeline-assertionerror-finish 套件）。
      for (const sock of this.__sockets) {
        const __st = sock.__httpState;
        if (!__st || __st.req === null) {
          try { sock.destroy(); } catch { /* closed meanwhile */ }
        } else if (typeof sock.unref === "function") {
          try { sock.unref(); } catch { /* gone */ }
        }
      }
      // node _http_server.js close 口径：仍有活连接时起 connectionsChecking
      // interval（句柄存符号键下；清零即 clearInterval——
      // close-destroy-timeout/async-dispose 套件断言 _destroyed）。
      const alive = [...this.__sockets].filter((s) => !s.destroyed);
      if (alive.length > 0 && this[kConnectionsCheckingInterval] === undefined) {
        const iv = setInterval(() => {
          for (const s of [...this.__sockets]) {
            const st = s.__httpState;
            if (!st || st.req === null) { try { s.destroy(); } catch { /* gone */ } }
          }
          if (![...this.__sockets].some((s) => !s.destroyed)) {
            clearInterval(this[kConnectionsCheckingInterval]);
            this[kConnectionsCheckingInterval] = undefined;
          }
        }, 1000);
        if (typeof iv.unref === "function") iv.unref();
        this[kConnectionsCheckingInterval] = iv;
      }
      super.close();
      return this;
    }
    // node Server asyncDispose（node 26：close 承诺化）。
    [Symbol.asyncDispose]() {
      return new Promise((resolve) => {
        this.close(() => resolve());
      });
    }
  }
  // Node 口径：Server 裸调用返回新实例（lib/_http_server.js 原文）；
  // `Server.call(this)` 形（upgrade-server 套件 testServer 老式继承）直接在
  // this 上跑初始化——Reflect.construct 会造新对象弃 this，老式子类全挂。
  function HttpServer(...args) {
    if (!(this instanceof __HttpServer)) return new __HttpServer(...args);
    if (new.target !== undefined) return Reflect.construct(__HttpServer, args, new.target);
    __initServer(this, args);
  }
  Object.setPrototypeOf(HttpServer, __HttpServer);
  HttpServer.prototype = __HttpServer.prototype;
  // http.rs Server 壳的 .call 形入口（this 已是派生实例时在其上初始化）。
  HttpServer.__initOn = (obj, args) => __initServer(obj, args);
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
        const u = __parseUrlArg(String(options));
        if (u.protocol !== flavor.protocol) {
          throw new codes.ERR_INVALID_PROTOCOL(u.protocol, flavor.protocol);
        }
        method = "GET";
        host = u.hostname;
        port = u.port ? Number(u.port) : flavor.defaultPort;
        path = u.pathname + u.search;
        userHeaders = {};
        extra = {};
      } else {
        // node _http_client.js：协议门（url.parse 形对象带 protocol 字段；
        // url.parse-only 套件——file:/mailto:/ftp: 等一律 ERR_INVALID_PROTOCOL）。
        if (options.protocol !== undefined && options.protocol !== flavor.protocol) {
          throw new codes.ERR_INVALID_PROTOCOL(options.protocol, flavor.protocol);
        }
        // host/hostname 类型门（真机逐字：'of type string or one of undefined
        // or null'；hostname-typechecking 套件）。
        if (options.hostname !== undefined && options.hostname !== null && typeof options.hostname !== "string") {
          throw new codes.ERR_INVALID_ARG_TYPE("options.hostname", ["string", "undefined", "null"], options.hostname);
        }
        if (options.host !== undefined && options.host !== null && typeof options.host !== "string") {
          throw new codes.ERR_INVALID_ARG_TYPE("options.host", ["string", "undefined", "null"], options.host);
        }
        // agent 门（Agent-like Object/undefined/null/false；
        // reject-unexpected-agent 套件真机逐字）。
        if (options.agent !== undefined && options.agent !== null && options.agent !== false) {
          if (typeof options.agent !== "object" || typeof options.agent.addRequest !== "function") {
            throw new codes.ERR_INVALID_ARG_TYPE("options.agent", ["Agent-like Object", "undefined", "false"], options.agent);
          }
        }
        // insecureHTTPParser 类型门（insecure-parser-per-stream 套件 test5）。
        if (options.insecureHTTPParser !== undefined && typeof options.insecureHTTPParser !== "boolean") {
          throw new codes.ERR_INVALID_ARG_TYPE("options.insecureHTTPParser", "boolean", options.insecureHTTPParser);
        }
        // method 门：非串 → ARG_TYPE（check-http-token 套件）；非法 token →
        // ERR_INVALID_HTTP_TOKEN（request-invalid-method-error 套件 '\0'）。
        if (options.method !== undefined && options.method !== null) {
          if (typeof options.method !== "string") {
            throw new codes.ERR_INVALID_ARG_TYPE("options.method", "string", options.method);
          }
          // 空串等 falsy method → 缺省 GET（client-defaults 套件）；非法
          // token → ERR_INVALID_HTTP_TOKEN（request-invalid-method-error 套件）。
          if (options.method !== "" && !__TOKEN_RE.test(options.method)) {
            throw new codes.ERR_INVALID_HTTP_TOKEN("Method", options.method);
          }
        }
        method = (options.method ?? "GET").toUpperCase() || "GET";
        host = options.host ?? options.hostname ?? "localhost";
        // node 口径：defaultPort 逐级——显式 port > agent.defaultPort > flavor 缺省
        //（default-port 套件：globalAgent.defaultPort 动态改写生效，host 头
        // 按“port === 生效缺省”省略端口；agent 缺省取隐式 globalAgent）。
        const __ag = options.agent !== undefined ? options.agent : flavor.defaultAgent;
        const __agentDp = __ag && __ag.defaultPort !== undefined ? __ag.defaultPort : flavor.defaultPort;
        port = Number(options.port ?? __agentDp);
        path = options.path ?? "/";
        if (!path.startsWith("/")) path = "/" + path;
        userHeaders = options.headers ?? {};
        extra = options;
        // node addRequest 口径：socketPath 在场即以之改写 connect 用的 path
        //（防 HTTP path 泄进 net.connect 误连错目标——ENOTSOCK 现场记录）。
        if (options.socketPath !== undefined) options.path = options.socketPath;
      }
      // node lib/_http_client.js：path 控制字符/空格即 ERR_UNESCAPED_CHARACTERS。
      if (INVALID_PATH_REGEX.test(path)) {
        throw new codes.ERR_UNESCAPED_CHARACTERS("Request path");
      }
      this.method = method;
      this.host = host;
      // node 口径：.port 不是自有属性（req.port === undefined；取值走 getPort()）。
      this.__port = port;
      // IPC 形（node：req.socketPath 自有属性；openSocket 钩按它走 UDS）。
      this.socketPath = options.socketPath;
      // path 访问器（node setPath 口径）：赋值即校验，控制字符/空格一律
      // ERR_UNESCAPED_CHARACTERS（path-toctou 套件：`req.path = '/evil\r\n...'`）。
      Object.defineProperty(this, "path", {
        get() { return this.__pathVal; },
        set(v) {
          const s = typeof v === "string" ? v : String(v);
          if (INVALID_PATH_REGEX.test(s)) {
            throw new codes.ERR_UNESCAPED_CHARACTERS("Request path");
          }
          this.__pathVal = v;
        },
        enumerable: true,
        configurable: true,
      });
      this.path = path;
      // 自设请求头名字门（invalidheaderfield 套件：'testing 123' → TypeError）。
      for (const __k of Object.keys(userHeaders ?? {})) {
        if (!__TOKEN_RE.test(__k)) {
          throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", __k);
        }
      }
      // 每请求宽松解析旗（insecure-parser-per-stream 套件：头值控制字符严格门）。
      this.insecureHTTPParser = options.insecureHTTPParser ?? false;
      this.socket = null;
      this.agent = options.agent === undefined ? (flavor.defaultAgent ?? null) : (options.agent || null);
      this.__agentFalse = options.agent === false;
      this.__defaultPort = this.agent !== null && this.agent.defaultPort !== undefined
        ? this.agent.defaultPort : flavor.defaultPort;
      // timeout 双检（node validateNumber 口径，真机 26.8.2 逐项：null/'x' →
      // ARG_TYPE，NaN/负 → OUT_OF_RANGE）。
      if (options.timeout !== undefined) {
        if (typeof options.timeout !== "number") {
          throw new codes.ERR_INVALID_ARG_TYPE("timeout", "number", options.timeout);
        }
        if (!Number.isFinite(options.timeout) || options.timeout < 0) {
          throw new codes.ERR_OUT_OF_RANGE("timeout", "a non-negative finite number", options.timeout);
        }
      }
      this.timeout = options.timeout !== undefined ? Number(options.timeout) : undefined;
      // 请求级 timeout（__attach 时覆盖 agent 级的 socket 计时）。
      this.__reqTimeoutMs = this.timeout !== undefined && this.timeout > 0 ? this.timeout : undefined;
      // node onSocket 口径：请求级 timeout 优先于 agent 级；任一在场即挂
      // timeoutCb（emitRequestTimeout——转发 socket 'timeout' → req 'timeout'）。
      const __agentTimeout = this.agent !== null && this.agent.options ? this.agent.options.timeout : undefined;
      this.__timeoutMs = this.timeout ?? __agentTimeout;
      if (this.timeout !== undefined || (typeof __agentTimeout === "number" && __agentTimeout > 0)) {
        this.timeoutCb = () => this.emit("timeout");
      }
      this.__headers = __lowerHeaders(userHeaders);
      if (this.__headers.host === undefined) {
        this.__headers.host = port === this.__defaultPort ? host : `${host}:${port}`;
      }
      // node ctor 口径：shouldKeepAlive = agent 在场且 keepAlive（真机
      // _http_client.js ctor：无 agent / 非 keepAlive agent → Connection: close）。
      this.shouldKeepAlive = this.agent !== null && this.agent.keepAlive === true;
      if (this.__headers.connection === undefined) {
        this.__headers.connection = this.shouldKeepAlive ? "keep-alive" : "close";
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
      this.__contentLength = undefined;
      this.__headerStored = false;
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
        // 实参是请求 options 的浅拷贝且 path 值被摘除（TCP；真机逐键实测
        // ["createConnection","headers","host","path(undefined)","port"]）、
        // IPC 时改写为 socketPath——防 HTTP path 泄进 net.connect 误走管道。
        this.__createConn = options.createConnection;
        const connOpts = { ...(extra ?? {}) };
        connOpts.path = options.socketPath !== undefined ? options.socketPath : undefined;
        let out;
        let settled = false;
        const oncreate = (err, s) => {
          settled = true;
          if (s) this.__attach(s, false);
        };
        const maybe = this.__createConn(connOpts, oncreate);
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
      // node setRequestProps 口径：socket._httpMessage 指回当前请求（connect
      // 套件在 'socket' 事件断言全等）。
      sock._httpMessage = this;
      this.reusedSocket = reused === true;
      // node onSocket 口径：'socket' 事件异步（nextTick）发出——get()/request()
      // 返回后同步注册的监听器必须能收到（agent-timeout-option 套件形态）。
      queueMicrotask(() => {
        if (!this.destroyed) this.emit("socket", sock);
      });
      if (this.__pendingNoDelay !== undefined) {
        try { sock.setNoDelay(this.__pendingNoDelay); } catch { /* gone */ }
      }
      if (this.__pendingKeepAlive !== undefined) {
        try { sock.setKeepAlive(this.__pendingKeepAlive[0], this.__pendingKeepAlive[1]); } catch { /* gone */ }
      }
      // node onSocket 口径：timeoutCb 在场即挂 once；请求级 timeout 覆盖 agent 级
      //（socket.timeout 反映最后一次 setTimeout）；agent 级已在建连时置位则不重臂。
      // 假 socket（createConnection 注入的 Duplex）无 setTimeout 面则跳过。
      if (this.timeoutCb !== undefined) {
        if (this.__reqTimeoutMs !== undefined) {
          this.__applySockTimeout(sock, this.__reqTimeoutMs);
        } else if (!sock.timeout) {
          const __ms = this.__timeoutMs;
          if (typeof __ms === "number" && __ms > 0) this.__applySockTimeout(sock, __ms);
        }
        // 复用连接换请求：摘上一请求的 emitRequestTimeout（listeners 计数契约：
        // [onTimeout, emitRequestTimeout, responseOnTimeout] 不随复用累加）。
        if (sock.__lastTimeoutCb !== undefined && sock.__lastTimeoutCb !== this.timeoutCb) {
          try { sock.removeListener("timeout", sock.__lastTimeoutCb); } catch { /* gone */ }
        }
        sock.__lastTimeoutCb = this.timeoutCb;
        sock.once("timeout", this.timeoutCb);
      }
      this.__onSockClose = () => this.__onSockCloseEv();
      sock.on("connect", () => {
        this.__connected = true;
        if (this.__pendingFinal) {
          // end() 已调：整事务一次刷出（CL 决策在 end 时已定）。
          this.__pendingFinal = false;
          this.__flushFinal();
          return;
        }
        // node _flush 口径：连通即发头（无体请求——如 Expect: 100-continue
        // 等 continue 的形态——头也必须立即出网）。
        this.__tryFlush();
      });
      sock.on("secureConnect", () => {
        this.__connected = true;
        if (this.__pendingFinal) {
          this.__pendingFinal = false;
          this.__flushFinal();
          return;
        }
        this.__tryFlush();
      });
      sock.on("data", (chunk) => {
        try {
          this.__onSockData(chunk);
        } catch (e) {
          // node 口径：响应头解析错（严格门）→ req 'error'（经 destroy(err)）。
          this.destroy(e);
        }
      });
      sock.on("error", (e) => {
        if (this.listenerCount("error") === 0) throw e;
        this.emit("error", e);
        // node 口径：连接错无响应即销毁请求（'close' 时 req.destroyed === true，
        // agent-close/timeout-option 系套件断言）。
        this.destroy();
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
      if (!__TOKEN_RE.test(String(name))) throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", String(name));
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
    // socket 空闲计时（http 层驱动时补记 .timeout + 'onTimeout' 占位监听——
    // 对齐 node net 内部监听形状：listeners('timeout') = [onTimeout,
    // emitRequestTimeout, ...]；onTimeout 是 socket 单例（node net 构造时挂一
    // 次，setTimeout 重臂不重复挂）；net 域 setTimeout 不发布 .timeout，偏差记档）。
    __applySockTimeout(sock, ms) {
      if (typeof sock.setTimeout !== "function") return;
      sock.setTimeout(ms);
      sock.timeout = ms;
      if (!sock.__onTimeoutSingleton) {
        sock.__onTimeoutSingleton = true;
        sock.on("timeout", function onTimeout() {});
      }
    }
    // node lib/_http_client.js：once('timeout') + socket 空闲计时（已连即臂，
    // 未连记位，__attach 落地）。
    setTimeout(msecs, callback) {
      if (typeof callback === "function") this.once("timeout", callback);
      const ms = Number(msecs) || 0;
      this.__reqTimeoutMs = ms > 0 ? ms : undefined;
      if (this.__sock !== null && this.__sock !== undefined) {
        if (ms > 0) {
          this.__applySockTimeout(this.__sock, ms);
        } else if (typeof this.__sock.setTimeout === "function") {
          this.__sock.setTimeout(0);
          this.__sock.timeout = 0;
        }
      }
      return this;
    }
    clearTimeout(cb) { return this.setTimeout(0, cb); }
    // 立即发头（node flushHeaders：_implicitHeader + 强制刷）。
    flushHeaders() {
      if (this.__headSent) return;
      this.__headerStored = true;
      if (this.__connected && this.__sock !== null && this.__sock !== undefined && !this.destroyed) {
        if (this.__buf1 !== null && this.__headers["content-length"] === undefined &&
            this.__headers["transfer-encoding"] === undefined) {
          this.__chunked = this.__chunkDefault;
        }
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
    write(chunk, encoding, cb) {
      if (this.__userEnded) return __writeAfterEnd(this, encoding, cb);
      return super.write(chunk, encoding, cb);
    }
    end(chunk, encoding, cb) {
      if (this.__userEnded) {
        const f = typeof chunk === "function" ? chunk : (typeof encoding === "function" ? encoding : cb);
        if (typeof f === "function") f();
        return this;
      }
      if (this.destroyed) {
        // 已销毁（abort 后 end）：node 口径不抛（abort-before-end 套件
        // req.on('error') mustNotCall）；回调按空转处理。
        const f = typeof chunk === "function" ? chunk : (typeof encoding === "function" ? encoding : cb);
        if (typeof f === "function") f();
        return this;
      }
      // CL 快路径判据：end 是首个头触发点（此前无 write）。
      this.__endFast = !this.__sawWrite;
      this.__userEnded = true;
      // node maybePrepareFinalChunk 口径：end(data) 为首个头触发点时
      // _contentLength 即刻落定（UCED 方法族；GET 族无 CL——framing 顺序）。
      if (this.__endFast && !this.__headSent && !this.__headerStored &&
          this.__chunkDefault &&
          this.__headers["content-length"] === undefined &&
          this.__headers["transfer-encoding"] === undefined) {
        let __len = 0;
        if (chunk !== undefined && chunk !== null && typeof chunk !== "function") {
          if (typeof chunk === "string") {
            __len = globalThis.Buffer !== undefined && typeof globalThis.Buffer.byteLength === "function"
              ? globalThis.Buffer.byteLength(chunk, typeof encoding === "string" ? encoding : "utf8")
              : new TextEncoder().encode(chunk).length;
          } else if (chunk.byteLength !== undefined) {
            __len = chunk.byteLength;
          }
        }
        this.__contentLength = __len;
      }
      return super.end(chunk, encoding, cb);
    }
    // node 口径（弃用面仍测）：abort = destroy + 'abort' 事件 + aborted 旗。
    abort() {
      if (this.destroyed) return;
      this.__aborted = true;
      this.destroy();
      this.emit("abort");
    }
    get aborted() { return this.__aborted === true; }
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
    // 连接就绪或刷盘时机到：头恒发（node _flush 口径）；有体位时按 UCED 定
    // chunked。队列逐帧发出（不合并）：保 TCP 分包（blk09 计数型套件依赖）。
    __tryFlush() {
      if (!this.__connected || this.__sock === null || this.__headSent) return;
      if (this.destroyed) return;
      if (this.__buf1 !== null) {
        if (this.__headers["content-length"] === undefined && this.__headers["transfer-encoding"] === undefined) {
          this.__chunked = this.__chunkDefault;
        }
      } else if (this.__chunkDefault && this.__headers["content-length"] === undefined &&
                 this.__headers["transfer-encoding"] === undefined) {
        // 无体 UCED 请求（含 Expect: 100-continue 等 continue 的形态）：chunked 起拍。
        this.__chunked = true;
      }
      this.__forceHead = false;
      this.__headerStored = true;
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
        // CL 决策已在 end() 落定（node _contentLength 口径）；无 CL 的 UCED
        // 请求 chunked；GET 族（UCED false）无 CL/TE 裸体。
        if (this.__contentLength !== undefined) {
          this.__headers["content-length"] = String(this.__contentLength);
          this.__rawCL = true;
        } else if (this.__chunkDefault && this.__headers["content-length"] === undefined &&
                   this.__headers["transfer-encoding"] === undefined) {
          this.__chunked = true;
          this.__headers["transfer-encoding"] = "chunked";
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
      // 请求已完成（回池/半关）后 socket 的 data 监听仍在——后续响应归下一个
      // 请求，已完成者不得再泵（keep-alive + chunked 响应复用件实测必需）。
      if (this.__respDone || this.__sock === null) return;
      this.__resBuf = __concat(this.__resBuf, chunk);
      while (true) {
        if (this.__res === null) {
          const headEnd = __findHeadEnd(this.__resBuf);
          if (headEnd === -1) return;
          const headText = __latin1(this.__resBuf.slice(0, headEnd));
          if (this.insecureHTTPParser !== true && __hasBareCR(headText)) {
            this.destroy(__hpe("HPE_LF_EXPECTED", "Expected LF after CR"));
            return;
          }
          const { first, headers, rawHeaders } = __parseHead(headText, this.insecureHTTPParser !== true);
          if (!first[0].startsWith("HTTP/") || !/^\d{3}$/.test(first[1] ?? "")) {
            this.destroy(__hpe("HPE_INVALID_CONSTANT", "invalid HTTP response line"));
            return;
          }
          // node llhttp strict：TE 与 CL 并存即拒（client-reject-chunked-with-
          // content-length 套件）。
          if (this.insecureHTTPParser !== true && headers["transfer-encoding"] !== undefined &&
              headers["content-length"] !== undefined) {
            this.destroy(__hpe("HPE_INVALID_TRANSFER_ENCODING", "Transfer-Encoding can't be present with Content-Length"));
            return;
          }
          const __statusCode = Number(first[1]);
          // CONNECT 响应：隧道建立——'connect' 事件（res, socket, head 原始态），
          // 不发 'response'，socket 停止 HTTP 解析、不回池（node _http_client 口径）。
          if (this.method === "CONNECT") {
            const res = new IncomingMessage();
            res.statusCode = __statusCode;
            res.statusMessage = first.length >= 3 ? first.slice(2).join(" ") : "";
            res.httpVersion = first[0].slice(5);
            {
              const __vv = res.httpVersion.split(".");
              res.httpVersionMajor = Number(__vv[0] ?? 1) || 0;
              res.httpVersionMinor = Number(__vv[1] ?? 1) || 0;
            }
            res.headers = headers;
            res.rawHeaders = rawHeaders;
            res.socket = this.__sock;
            res.connection = this.__sock;
            res.req = this;
            const __leftover = this.__resBuf.slice(headEnd + 4);
            this.__resBuf = new Uint8Array(0);
            this.__respDone = true;
            this.__upgraded = true;
            this.__res = res;
            this.emit("connect", res, this.__sock, globalThis.Buffer.from(__leftover));
            return;
          }
          // 1xx 信息响应（node parserOnIncomingClient 口径，真机 26.8.2 对拍）：
          // 100 → 'continue' 事件（无实参）；1xx（含 100）→ 'information'（res 形态）；
          // 都不发 'response'，继续等真响应。101 → 'upgrade'（有监听）或销毁。
          if (__statusCode >= 100 && __statusCode < 200) {
            const __leftover = this.__resBuf.slice(headEnd + 4);
            if (__statusCode === 101) {
              if (this.listenerCount("upgrade") > 0) {
                const res = new IncomingMessage();
                res.statusCode = __statusCode;
                res.statusMessage = first.length >= 3 ? first.slice(2).join(" ") : "";
                res.httpVersion = first[0].slice(5);
                {
                  const __vv = res.httpVersion.split(".");
                  res.httpVersionMajor = Number(__vv[0] ?? 1) || 0;
                  res.httpVersionMinor = Number(__vv[1] ?? 1) || 0;
                }
                res.headers = headers;
                res.rawHeaders = rawHeaders;
                res.socket = this.__sock;
                res.connection = this.__sock;
                res.req = this;
                res.__complete();
                this.__resBuf = new Uint8Array(0);
                this.__respDone = true;
                this.__upgraded = true;
                this.__res = res;
                this.emit("upgrade", res, this.__sock, globalThis.Buffer.from(__leftover));
              } else {
                this.destroy();
              }
              return;
            }
            if (__statusCode === 100) this.emit("continue");
            const info = new IncomingMessage();
            info.statusCode = __statusCode;
            info.statusMessage = first.length >= 3 ? first.slice(2).join(" ") : "";
            info.httpVersion = first[0].slice(5);
            {
              const __vv = info.httpVersion.split(".");
              info.httpVersionMajor = Number(__vv[0] ?? 1) || 0;
              info.httpVersionMinor = Number(__vv[1] ?? 1) || 0;
            }
            info.headers = headers;
            info.rawHeaders = rawHeaders;
            info.socket = this.__sock;
            info.connection = this.__sock;
            info.req = this;
            this.__resBuf = this.__resBuf.slice(headEnd + 4);
            this.emit("information", info);
            continue;
          }
          const res = new IncomingMessage();
          res.statusCode = __statusCode;
          // 状态行无短语合法（"HTTP/1.1 200\r\n"）：短语空串（status-message 套件）。
          res.statusMessage = first.length >= 3 ? first.slice(2).join(" ") : "";
          res.httpVersion = first[0].slice(5);
          {
            const __vv = res.httpVersion.split(".");
            res.httpVersionMajor = Number(__vv[0] ?? 1) || 0;
            res.httpVersionMinor = Number(__vv[1] ?? 1) || 0;
          }
          res.headers = headers;
          res.rawHeaders = rawHeaders;
          // node parserOnIncomingClient 口径：req.shouldKeepAlive 由响应决定
          //（1.1 缺省 keep、'close' 关；1.0 须显式 'keep-alive'；
          // should-keep-alive 套件逐项对拍）。
          {
            const __rc = (headers.connection || "").toLowerCase();
            this.shouldKeepAlive = res.httpVersion === "1.0" ? __rc === "keep-alive" : __rc !== "close";
          }
          res.socket = this.__sock;
          res.connection = this.__sock;
          res.req = this;
          this.__framing = __framingFor(headers, true, res.statusCode, this.method);
          this.__res = res;
          // 释放闸门先于 'response' 挂载（node responseOnEnd 内部先挂口径）：
          // res 'end'/'close' → 回池/关连 + req 'close'，用户 end 处理器晚于释放。
          this.__armReleaseGates(this.__sock);
          this.__resBuf = this.__resBuf.slice(headEnd + 4);
          // node _http_client.js：响应到达即挂 responseOnTimeout（一次性/socket；
          // 转发 socket 'timeout' → req 'timeout'，响应完结后不再转发）。
          // 计数契约：listeners('timeout') = [onTimeout, emitRequestTimeout,
          // responseOnTimeout]，跨 keep-alive 复用不累加（listeners 套件）。
          if (this.__sock !== null && this.__sock.timeout > 0 && !this.__sock.__respOnTimeout) {
            this.__sock.__respOnTimeout = true;
            const __req = this;
            this.__sock.on("timeout", function responseOnTimeout() {
              if (__req.__res !== null && __req.__res.complete) return;
              __req.emit("timeout");
            });
          }
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
            this.destroy(__hpe("HPE_INVALID_CONSTANT", "invalid chunked body"));
            return;
          }
        }
        this.__resBuf = r.rest;
        if (!r.done) return;
        if (r.trailersRaw !== undefined) __applyTrailers(this.__res, r.trailersRaw);
        this.__res.__complete();
        this.__finishResponse(false);
        return;
      }
    }
    // 响应收齐：释放闸门（__armReleaseGates）在 res 'end'/'close' 触发回池/关连
    //（真机 p-free/p-ka：未消费前 freeSockets 恒空、排队请求不续行）。
    // socket 先断（close-delimited/半途）时直接收尾。
    __armReleaseGates(sock) {
      if (this.__gatesArmed || sock === null || this.__res === null) return;
      this.__gatesArmed = true;
      let __fired = false;
      const __once = () => {
        if (__fired) return;
        __fired = true;
        this.__finishSock(sock);
        if (!this.__closeEmitted) {
          this.__closeEmitted = true;
          this.emit("close");
        }
      };
      this.__res.once("end", __once);
      this.__res.once("close", __once);
      this.__gateOnce = __once;
    }
    __finishSock(sock) {
      if (sock === null || sock.destroyed) return;
      const conn = this.__res !== null ? (this.__res.headers.connection || "").toLowerCase() : "close";
      const poolable = this.agent !== null && this.agent.keepAlive && conn !== "close";
      if (this.agent !== null) {
        // node 口径：socket 'free' 事件恒发（agent onFree 在此续行排队请求）；
        // __release 内按 keepAlive 决定回池或销毁，并 resume 队列。
        try { sock.emit("free"); } catch { /* gone */ }
        this.agent.__release(sock, this.__key, this);
      } else {
        try { sock.end(); } catch { /* closed meanwhile */ }
      }
    }
    __finishResponse(fromClose) {
      if (this.__respDone) return;
      this.__respDone = true;
      const sock = this.__sock;
      this.__sock = null;
      const emitClose = () => {
        if (!this.__closeEmitted) {
          this.__closeEmitted = true;
          this.emit("close");
        }
      };
      if (fromClose || this.__res === null || this.__res.readableEnded || this.__res.destroyed) {
        this.__finishSock(sock);
        emitClose();
      }
      // 否则：闸门已挂（response 派发前），res 终结时统一收尾。
    }
    __onSockCloseEv() {
      if (this.__closeEmitted) return;
      // CONNECT/upgrade 后的裸 socket 关闭：直接发 req 'close'（不回池不触 res）。
      if (this.__upgraded) {
        this.__closeEmitted = true;
        this.emit("close");
        return;
      }
      // 响应体已齐但 res 未被消费时连接先断：毁 res 引发 'close' → finish 链。
      if (this.__respDone && this.__res !== null && !this.__res.readableEnded && !this.__res.destroyed) {
        try { this.__res.destroy(); } catch { /* gone */ }
        return;
      }
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
  // maxTotalSockets 门（agent-maxtotalsockets 套件真机逐项：非串 → ARG_TYPE；
  // -1/0/NaN → OUT_OF_RANGE；Infinity 合法）。
  if (options.maxTotalSockets !== undefined) {
    if (typeof options.maxTotalSockets !== "number") {
      throw new codes.ERR_INVALID_ARG_TYPE("maxTotalSockets", "number", options.maxTotalSockets);
    }
    // node 口径：NaN/-1/0 拒、Infinity 过（agent-maxtotalsockets 套件点名）。
    if (!(options.maxTotalSockets > 0)) {
      throw new codes.ERR_OUT_OF_RANGE("maxTotalSockets", "> 0", options.maxTotalSockets);
    }
  }
  this.totalSocketCount = 0;
  this.scheduling = options.scheduling ?? "lifo";
  this.sockets = {};
  this.freeSockets = {};
  this.requests = {};
  // __openSocket/__defaultPort 由 flavor 子类经 prototype 提供（本类不设 own
  // 属性，否则遮蔽子类覆盖；裸 BaseAgent 直接用即 TypeError）。
};
// node 口径：createConnection 是 agent 的建连钩（默认 = net.createConnection，
// 同步回值、不调 cb——cb 由 createSocket 的 oncreate 统一收口）；测试以假
// Duplex 覆盖做黑洞/依此注入 socket。同步回值与 cb 双形态由 createSocket 兜。
Agent.prototype.createConnection = function (options, cb) {
  return this.__openSocket(options.host, options.port, options);
};
// node lib/_http_agent.js：createConnection 的记账壳（req, options, cb 三参；
// 同步回值与 cb 双形态，settled 旗防双取）；用户可整体覆写（agent-close 套件
// `createSocket = (req, options, cb) => cb(err)` → req 'error' + 销毁）。
Agent.prototype.createSocket = function (req, options, cb) {
  let settled = false;
  const oncreate = (err, s) => {
    settled = true;
    if (typeof cb === "function") cb(err, s);
  };
  const maybe = this.createConnection(options, oncreate);
  if (!settled && maybe) oncreate(null, maybe);
  return maybe;
};
Agent.prototype.getName = function (options = {}) {
  let name = options.host ?? options.hostname ?? "localhost";
  name += ":";
  if (options.port) name += options.port;
  name += ":";
  if (options.localAddress) name += options.localAddress;
  // node lib/_http_agent.js：socketPath 占独立槽（'localhost:::/path'，
  // agent-getname 套件点名；unix socket 与 TCP localhost 池键由此区分）。
  if (options.socketPath) name += ":" + options.socketPath;
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
// 全局活 socket 数（maxTotalSockets 帽的判定口径；agent-maxtotalsockets 套件
// getTotalSocketsCount 同款）。
Agent.prototype.__totalLive = function () {
  let n = 0;
  for (const key of Object.keys(this.sockets)) n += this.__liveCount(key);
  return n;
};
Agent.prototype.__trackSocket = function (sock, key) {
  this.__list(this.sockets, key).push(sock);
  this.totalSocketCount++;
  // node agent：options.timeout 在建连时即置 socket 空闲计时（agent-timeout-
  // option 套件：'socket' 事件时 socket.timeout 已 === 50；onTimeout 单例）。
  if (this.options && typeof this.options.timeout === "number" && this.options.timeout > 0) {
    if (typeof sock.setTimeout === "function") {
      sock.setTimeout(this.options.timeout);
      sock.timeout = this.options.timeout;
      if (!sock.__onTimeoutSingleton) {
        sock.__onTimeoutSingleton = true;
        sock.on("timeout", function onTimeout() {});
      }
    }
  }
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
// 建连统一走 createSocket 钩（req, options, cb 三参——用户覆写点）。
Agent.prototype.__acquire = function (req, host, port, extra, onSocket) {
  const key = this.getName({ host, port, ...(extra ?? {}) });
  const free = this.__list(this.freeSockets, key);
  while (free.length > 0) {
    const sock = this.scheduling === "fifo" ? free.shift() : free.pop();
    if (!sock.destroyed) {
      this.__unpool(sock);
      this.__list(this.sockets, key).push(sock);
      req.__poolKey = key;
      onSocket(sock, true);
      return;
    }
  }
  if (this.__liveCount(key) >= this.maxSockets ||
      (this.maxTotalSockets !== Infinity && this.__totalLive() >= this.maxTotalSockets)) {
    this.__list(this.requests, key).push({ req, host, port, extra, onSocket });
    req.__poolKey = key;
    req.__queued = true;
    return;
  }
  req.__poolKey = key;
  req.__queued = false;
  // host/port 后置归一（extra 的 null/undefined host 不得覆盖归一值——
  // hostname-typechecking 的 {host: null} 值形会漏进 net.connect 炸类型门）。
  const opts = { ...(extra ?? {}), host, port };
  let done = false;
  const oncreate = (err, sock) => {
    if (done) return;
    done = true;
    if (err || !sock) {
      if (sock) { try { sock.destroy(); } catch { /* gone */ } }
      const e = err ?? (() => { const x = new Error("socket hang up"); x.code = "ECONNREFUSED"; return x; })();
      if (!req.destroyed) req.destroy(e);
      return;
    }
    this.__trackSocket(sock, key);
    onSocket(sock, false);
  };
  this.createSocket(req, opts, oncreate);
};
Agent.prototype.__noteClosed = function (sock) {
  const key = sock.__poolKey;
  if (key === undefined) return;
  const drop = (map) => {
    const arr = map[key];
    if (arr !== undefined) {
      const i = arr.indexOf(sock);
      if (i !== -1) arr.splice(i, 1);
      // node 口径：清空即删键（keep-alive 套件 process.on('exit') 断言
      // `!(name in agent.sockets/requests)`——残留空数组会判真）。
      if (arr.length === 0) delete map[key];
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
      // node 口径：入池即移出在用表（agent.sockets 只计在用——
      // agent-maxtotalsockets 的 getTotalSocketsCount 口径）。
      const __inUse = this.__list(this.sockets, key);
      const __i = __inUse.indexOf(sock);
      if (__i !== -1) __inUse.splice(__i, 1);
      if (sock.__poolCleaner === undefined) {
        const cleaner = () => this.__noteClosed(sock);
        sock.__poolCleaner = cleaner;
        sock.on("close", cleaner);
      }
    }
  }
  // 续行排队请求（同键优先；全局 maxTotalSockets 帽下跨键唤醒，同键队列空
  // 时补扫其余键，防他键请求饿死）。
  const tryResume = (k) => {
    const q = this.__list(this.requests, k);
    while (q.length > 0) {
      const next = q.shift();
      if (next.req.destroyed) continue;
      next.req.__queued = false;
      this.__acquire(next.req, next.host, next.port, next.extra, next.onSocket);
      return true;
    }
    return false;
  };
  if (!tryResume(key)) {
    for (const k of Object.keys(this.requests)) {
      if (k === key) continue;
      if (tryResume(k)) break;
    }
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
    if (!sock.connecting && sock.pending) {
      // node onSocket 口径：手动塞入的裸 socket（无句柄，new net.Socket()）
      // 按请求选项补连，'connect' 事件后续行（agent-uninitialized 套件）。
      const sp = req.socketPath ?? options.socketPath;
      const copts = sp !== undefined && sp !== null
        ? { path: sp }
        : { host: req.host ?? "localhost", port: typeof req.getPort === "function" ? req.getPort() : undefined };
      sock.connect(copts);
      if (typeof req.__attach === "function") req.__attach(sock, false);
      return;
    }
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
  const opts = { ...options, host: options.host ?? options.hostname ?? "localhost", port: options.port ?? this.__defaultPort ?? 80 };
  let done = false;
  const oncreate = (err, s) => {
    if (done) return;
    done = true;
    if (err || !s) {
      if (s) { try { s.destroy(); } catch { /* gone */ } }
      const e = err ?? (() => { const x = new Error("socket hang up"); x.code = "ECONNREFUSED"; return x; })();
      if (!req.destroyed) req.destroy(e);
      return;
    }
    this.__trackSocket(s, name);
    if (typeof req.__attach === "function") req.__attach(s, false);
  };
  this.createSocket(req, opts, oncreate);
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
// node 口径：URL 形实参解析失败一律 ERR_INVALID_URL（invalid-urls 套件：
// 'www.nodejs.org' 等无协议串 → TypeError/ERR_INVALID_URL，真机对拍）。
function __parseUrlArg(str) {
  try {
    return new URL(str);
  } catch (e) {
    const err = new TypeError("Invalid URL");
    err.code = "ERR_INVALID_URL";
    err.input = str;
    throw err;
  }
}
export function normalizeRequestArgs(a, b, c, flavor) {
  if (typeof a === "string" || a instanceof URL) {
    const u = __parseUrlArg(String(a));
    if (u.protocol !== flavor.protocol) {
      throw new codes.ERR_INVALID_PROTOCOL(u.protocol, flavor.protocol);
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
