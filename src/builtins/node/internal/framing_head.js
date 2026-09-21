import { EventEmitter } from "node:events";
import { Readable, Writable } from "node:stream";
import { codes } from "node:internal/errors";

// node 内部符号（_http_server re-export；close-destroy-timeout/async-dispose 套件）
export const kConnectionsCheckingInterval = Symbol("kConnectionsCheckingInterval");
// node kHighWaterMark（_http_outgoing 同符号；server-options-highwatermark
// 套件断言 res[kHighWaterMark]）。
export const kHighWaterMark = Symbol("kHighWaterMark");
// node internal/streams/state getDefaultHighWaterMark（真机 65536/objectMode 16，
// 本仓 state 模块同值——server highWaterMark 缺省取它）。
import __streamsState from "node:internal/streams/state";
const { getDefaultHighWaterMark } = __streamsState;
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
// httpValidation 口径（node storeHTTPOptions + calculateLenientFlags 逐项对拍）：
// 'strict'（缺省）= RFC 7230；'relaxed' = Fetch 规约（值只拒 NUL/CR/LF）；'
// insecure' = 全宽松（同 insecureHTTPParser）。与 insecureHTTPParser 互斥
//（真机逐字：'cannot be used together with options.insecureHTTPParser'）。
const __HTTP_VALIDATIONS = ["strict", "relaxed", "insecure"];
function __resolveHttpValidation(httpValidation, insecureHTTPParser) {
  if (httpValidation !== undefined) {
    if (typeof httpValidation !== "string" || !__HTTP_VALIDATIONS.includes(httpValidation)) {
      throw new codes.ERR_INVALID_ARG_VALUE("options.httpValidation", httpValidation,
        "must be one of: 'strict', 'relaxed', or 'insecure'");
    }
    if (insecureHTTPParser !== undefined) {
      throw new codes.ERR_INVALID_ARG_VALUE("options.httpValidation", httpValidation,
        "cannot be used together with options.insecureHTTPParser");
    }
    return httpValidation;
  }
  return insecureHTTPParser === true ? "insecure" : "strict";
}
// 入站解析档位：strict / relaxed / lenient（lenient = insecureHTTPParser 全宽）。
function __parseModeOf(validation) {
  return validation === "insecure" ? "lenient" : validation;
}
// 出站头值门：strict 拒控制字符（除 HTAB）与 DEL；relaxed/insecure 只拒
// NUL/CR/LF（>0xff 不可能出现——latin1 文本）。违者 ERR_INVALID_CHAR。
function __checkOutboundHeaderValue(validation, value) {
  const v = String(value);
  if (validation === "relaxed" || validation === "insecure") {
    if (/[\x00\r\n]/.test(v)) {
      throw new codes.ERR_INVALID_CHAR("Invalid character in header content");
    }
    return;
  }
  if (!__validHeaderValue(v)) {
    throw new codes.ERR_INVALID_CHAR("Invalid character in header content");
  }
}
function __parseHead(headText, mode) {
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
    if (mode === "strict") {
      if (!__validHeaderValue(vRaw)) throw __mkParseError("invalid header value");
    } else if (mode === "relaxed") {
      // relaxed（Fetch 规约）：值只拒 NUL/CR/LF——CR/LF 行内不可能（按 CRLF
      // 分行），NUL 仍查；DEL/其余控制字符放行（header-value-relaxed 套件）。
      if (/[\x00\r\n]/.test(vRaw)) throw __mkParseError("invalid header value");
    }
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
function __lowerHeaders(obj, validation, namesSink) {
  const out = Object.create(null);
  for (const [k, v] of Object.entries(obj ?? {})) {
    // 头名字门（node checkIsHttpToken 口径；invalidheaderfield 套件）。
    if (!__TOKEN_RE.test(k)) throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", k);
    if (validation !== undefined) __checkOutboundHeaderValue(validation, v);
    out[k.toLowerCase()] = String(v);
    if (namesSink !== undefined) namesSink[k.toLowerCase()] = String(k);
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
  constructor(options) {
    super(options);
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
    // node 口径：aborted 缺省 false，中止置 true（aborted 套件双侧断言）。
    this.__aborted = false;
  }
  _read() {}
  get aborted() { return this.__aborted === true; }
  // 中止级联（aborted 同步恒发；error 有监听才发且递延——无监听发即抛错
  // （块二）；同步发则抢在 res 侧 PREMATURE_CLOSE 之前（pipeline 中断
  // 上传套件上报 ECONNRESET 而非 PREMATURE_CLOSE），真机 destroy 时序为异步。
  __abortWithError() {
    if (!this.__aborted) {
      this.__aborted = true;
      this.emit("aborted");
    }
    if (typeof this.listenerCount === "function" && this.listenerCount("error") > 0) {
      const e = new Error("aborted");
      e.code = "ECONNRESET";
      queueMicrotask(() => {
        try { this.emit("error", e); } catch { /* 关闭竞态 */ }
      });
    }
  }
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
    // node 口径：res.socket / res.connection 指向响应 socket（agent-keepalive
    // 套件服务端经 res.connection 取 socket 再 end）。
    this.socket = this.__sock;
    this.connection = this.__sock;
    this.statusCode = 200;
    this.statusMessage = undefined;
    this.__headers = Object.create(null);
    // 用户拼写记录（node kOutHeaders [name, value] 口径：wire 保留原大小写）。
    this.__headerNames = Object.create(null);
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
    __checkOutboundHeaderValue(this.__validation, value);
    const lk = String(name).toLowerCase();
    this.__headers[lk] = String(value);
    // node 口径：wire 保留用户原拼写（kOutHeaders 存 [name, value] 原文名）。
    this.__headerNames[lk] = String(name);
    return this;
  }
  // node OutgoingMessage.appendHeader（header-value-relaxed 套件点名）：同门
  // 校验后逗号拼接（node 口径：existing + ", " + value）。
  appendHeader(name, value) {
    if (!__TOKEN_RE.test(String(name))) throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", String(name));
    __checkOutboundHeaderValue(this.__validation, value);
    const lk = String(name).toLowerCase();
    const cur = this.__headers[lk];
    this.__headers[lk] = cur !== undefined ? `${cur}, ${value}` : String(value);
    this.__headerNames[lk] = String(name);
    return this;
  }
  getHeader(name) { return this.__headers[String(name).toLowerCase()]; }
  removeHeader(name) {
    const lk = String(name).toLowerCase();
    delete this.__headers[lk];
    delete this.__headerNames[lk];
    return this;
  }
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
    Object.assign(this.__headers, __lowerHeaders(obj, this.__validation, this.__headerNames));
    // node _storeHeader 口径（de-chunked-trailer 套件）：非 chunked 传输带
    // Trailer 头即同步抛 ERR_HTTP_TRAILER_INVALID（Trailer 只能随 chunked 走；
    // 无 CL/TE 时自动 chunked 故合法，不抛）。
    if (this.__headers["trailer"] !== undefined) {
      const __te = this.__headers["transfer-encoding"];
      const __teChunked = __te !== undefined && /(?:^|\W)chunked/i.test(String(__te));
      const __hasCL = this.__headers["content-length"] !== undefined;
      const __autoChunked = !__hasCL && __te === undefined &&
        this.__uced !== false && !this.__noBody && !this.__headOnly;
      if (!__teChunked && !__autoChunked) {
        const e = new Error("Trailers are invalid with this transfer encoding");
        e.code = "ERR_HTTP_TRAILER_INVALID";
        throw e;
      }
    }
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
  // node writeProcessing()：writeInformation(102) 速记。
  writeProcessing() {
    return this.writeInformation(102);
  }
  // node writeEarlyHints(hints)：103 Early Hints（early-hints 套件）。
  // link 缺席/空结果即静默跳过（mustNotCall information 形）；数组以 ", "
  // 连接；link 格式逐字真机正则；其余键原样透传（非法字符/名走既有头校验门）。
  writeEarlyHints(hints) {
    if (hints === null || typeof hints !== "object" || Array.isArray(hints)) {
      throw new codes.ERR_INVALID_ARG_TYPE("hints", "object", hints);
    }
    const link = hints.link;
    if (link === null || link === undefined) return;
    const __linkRe = /^(?:<[^>\r\n]*>)(?:\s*;\s*[^;"\s]+(?:=(")?[^;"\s]*\1)?)*$/;
    const __checkLink = (v) => {
      if (typeof v !== "string" || !__linkRe.test(v)) {
        throw new codes.ERR_INVALID_ARG_VALUE("hints", v);
      }
    };
    let joined;
    if (typeof link === "string") {
      __checkLink(link);
      joined = link;
    } else if (Array.isArray(link)) {
      if (link.length === 0) return;
      for (const e of link) __checkLink(e);
      joined = link.join(", ");
    } else {
      throw new codes.ERR_INVALID_ARG_VALUE("hints", link);
    }
    if (joined.length === 0) return;
    const headers = Object.create(null);
    headers.Link = joined;
    for (const k of Object.keys(hints)) {
      if (k !== "link") headers[k] = hints[k];
    }
    return this.writeInformation(103, headers);
  }
  // node writeInformation(statusCode[, headers])：1xx 中间响应直发（不占终态
  // 头、不置 headersSent；information/early-hints 套件；客户端侧 'information'
  // 事件既有）。非法码抛 ERR_HTTP_INVALID_STATUS_CODE（与 writeHead 同门）。
  writeInformation(info, headers) {
    let status;
    let hdrs;
    if (info !== null && typeof info === "object") {
      status = info.statusCode;
      hdrs = info.headers;
    } else {
      status = info;
      hdrs = headers;
    }
    if (typeof status !== "number" || !(status >= 100 && status <= 199)) {
      throw new codes.ERR_HTTP_INVALID_STATUS_CODE(`Invalid status code: ${String(status)}`);
    }
    if (this.__sock === null || this.__sock.destroyed) return false;
    const reason = STATUS_CODES[status] ?? "";
    const lines = [`HTTP/1.1 ${status} ${reason}`.trimEnd()];
    // 原拼写上网（rawHeaders 回显；information 套件断言 'Foo' 非 'foo'）。
    const __names = Object.create(null);
    for (const [k, v] of Object.entries(__lowerHeaders(hdrs ?? {}, this.__validation, __names))) {
      lines.push(`${__names[k] ?? k}: ${v}`);
    }
    try {
      this.__sock.write(new TextEncoder().encode(lines.join("\r\n") + "\r\n\r\n"));
    } catch { return false; /* gone */ }
    return true;
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
    // Node OutgoingMessage.end 口径（end-multiple 套件）：finished 后 end(chunk)
    // 走 onError（cb + 'error'，不碰基类错误通道——基类会置 errored 毒化在途
    // 首个 end 的 finish）；finished 后裸 end 回 ALREADY_FINISHED（同步）；
    // ending 中带块同 onError。皆不经 super.end。
    if (typeof chunk === "function") { cb = chunk; chunk = null; encoding = null; }
    else if (typeof encoding === "function") { cb = encoding; encoding = null; }
    const __hasChunk = chunk !== undefined && chunk !== null;
    const __cb = typeof cb === "function" ? cb : null;
    if (this.writableFinished) {
      if (__hasChunk) {
        if (this.destroyed) return this;
        const er = new codes.ERR_STREAM_WRITE_AFTER_END();
        queueMicrotask(() => { if (__cb) __cb(er); if (!this.destroyed) this.emit("error", er); });
      } else if (__cb) {
        __cb(new codes.ERR_STREAM_ALREADY_FINISHED("end"));
      }
      return this;
    }
    if (this.__userEnded) {
      if (!__hasChunk) return super.end(null, null, cb);
      if (this.destroyed) return this;
      const er = new codes.ERR_STREAM_WRITE_AFTER_END();
      queueMicrotask(() => { if (__cb) __cb(er); if (!this.destroyed) this.emit("error", er); });
      return this;
    }
    // CL 快路径判据（真机）：end 的数据块存在性——write 后裸 end() 不走快路径
    //（真机 chunked + 终结块口径）。
    this.__endHadData = chunk !== undefined && chunk !== null && typeof chunk !== "function";
    this.__userEnded = true;
    try {
      return super.end(chunk, encoding, cb);
    } catch (e) {
      // 基类校验抛（如数组 chunk）不得毒化旗位，否则后续合法 end 永不到
      // （end-types 套件 hang 根因）。
      this.__endHadData = false;
      this.__userEnded = false;
      throw e;
    }
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
    this.connection = sock;
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
      const user = this.__headerNames[k];
      const name = user !== undefined ? user
        : (__autoCase(k) ? (k === "keep-alive" ? "Keep-Alive" : k.charAt(0).toUpperCase() + k.slice(1)) : (canon[k] ?? k));
      head.push(`${name}: ${v}`);
    }
    return new TextEncoder().encode(head.join("\r\n") + "\r\n\r\n");
  }
  __sendHead() {
    // node 口径：header 独立成 write；首块合并只发生在 _final/flushFinal 快捷路。
    // socket 已销毁即静默丢弃（incoming-pipelined 套件：管线中连接被毁后
    // 续行响应的写不抛，node 写毁 socket 回 false 口径）。
    if (this.__sock === null || this.__sock.destroyed) return;
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
    if (this.__sock === null || this.__sock.destroyed) return;
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
      // node 口径：_write 完成异步回（socket 层 flush 节奏），同步回即
      // writableLength 即时清零、write 恒 true、背压永不触发
      // （outgoing-finish 系无限 while hang 根因）。
      queueMicrotask(cb);
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
    queueMicrotask(cb);
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
    // cont（re-feed 解析已读管线字节→503/408 等错误响应）先排，__last 的 FIN
    // 随后排：错误响应写落定时 socket 仍活，否则撞上已 end 即 "write after end"
    // 丢失（GET 管线超 maxRequests 形；POST 形靠体 pacing 碰巧，递延后确定性）。
    // node _last 口径：响应后关连接（close-delimited/显式 close/1.0 裸体）。
    cb();
    if (cont !== null) queueMicrotask(cont);
    if (this.__last) {
      const __s = this.__sock;
      queueMicrotask(() => { try { __s.end(); } catch { /* closed meanwhile */ } });
    }
  }
  // node 口径：destroy(err) 不外发 msg 'error'（outgoing-destroyed 要求吞错、
  // capture-rejection 经 socket 递错误；基类 trampoline 发 error 时序不可靠，
  // 吞错常驻——用户自有 error 监听仍可达，仅永不无监听抛错）。
  destroy(err) {
    if (this.destroyed) return this;
    this.on("error", () => {});
    return super.destroy(err);
  }
  _destroy(err, cb) {
    if (this.__holdTimer !== null) {
      clearTimeout(this.__holdTimer);
      this.__holdTimer = null;
    }
    // capture-rejection 套件：destroy(err) 透传 socket（有 error 监听才带
    // err——裸杀配 err 会无监听抛错；node 侧由常驻 socketOnError 承接，
    // 本仓无此常驻监听故按可观测等价门控）。
    try {
      const __s = this.__sock;
      if (err !== undefined && err !== null && typeof __s.listenerCount === "function" && __s.listenerCount("error") > 0) __s.destroy(err);
      else __s.destroy();
    } catch { /* closed meanwhile */ }
    cb(err);
  }
}
