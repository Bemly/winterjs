import { EventEmitter } from "node:events";
import { Readable, Writable } from "node:stream";
import { codes } from "node:internal/errors";
import { __etAdd, __etRemove } from "node:internal/events/abort_listener";

// node 内部符号（_http_server re-export；close-destroy-timeout/async-dispose 套件）
export const kConnectionsCheckingInterval = Symbol("kConnectionsCheckingInterval");
// node kHighWaterMark（_http_outgoing 同符号；server-options-highwatermark
// 套件断言 res[kHighWaterMark]）。
export const kHighWaterMark = Symbol("kHighWaterMark");
// node kOutHeaders（internal/http 同符号；correct-hostname/renderHeaders 套件）：
// OutgoingMessage 上的 [原名, 值] 对表（小写键 → [name, value]）。
export const kOutHeaders = Symbol("kOutHeaders");
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
// node _send 的 crlf_buf 常量（lib/_http_outgoing.js 915 行同款）。
const __CRLF = new TextEncoder().encode("\r\n");
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
  // node 口径（lib/_http_common.js checkInvalidHeaderChar）：合法 = HTAB /
  // 可打印 ASCII / latin1（0x80–0xFF）；C0（除 HTAB）、DEL、>0xFF 全拒
  //（header-validators 套件希伯来文形）。
  for (let i = 0; i < v.length; i++) {
    const cc = v.charCodeAt(i);
    if (cc === 9 || (cc >= 32 && cc <= 126) || (cc >= 128 && cc <= 255)) continue;
    return false;
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
// NUL/CR/LF（>0xff 不可能出现——latin1 文本）。违者 ERR_INVALID_CHAR（具名
// 调用带 ["key"] 后缀，无名走裸文案）。
function __checkOutboundHeaderValue(validation, value, name = undefined) {
  const v = String(value);
  if (validation === "relaxed" || validation === "insecure") {
    if (/[\x00\r\n]/.test(v)) {
      throw new codes.ERR_INVALID_CHAR(name);
    }
    return;
  }
  if (!__validHeaderValue(v)) {
    throw new codes.ERR_INVALID_CHAR(name);
  }
}
// node 单例头（重名首个赢；multiheaders2 套件 11 件 + 真机三轮实测
// Age/ETag/Server/Expires/Last-Modified/Retry-After 六件）。
const __SINGLETON_HEADERS = new Set([
  "age", "authorization", "content-type", "etag", "expires", "from", "host",
  "if-modified-since", "if-unmodified-since", "last-modified", "location",
  "max-forwards", "proxy-authorization", "referer", "retry-after",
  "server", "user-agent",
]);
// __trunc：超限静默截断（node 客户端响应口径——lib/_http_common.js
// parserOnHeaders "stop collecting"：maxHeaderPairs 上限后不再收集、
// 响应照常完成；服务端请求超限仍抛 HPE_HEADER_OVERFLOW 走 clientError）。
// joinDup：重复头合并门（node joinDuplicateHeaders 口径，真机 26.8.2 实测）：
// 缺省 false = 首个赢（cookie 恒 '; '、set-cookie 恒数组，不受门控）；
// true = 其余 ', ' 合并。
function __parseHead(headText, mode, maxPairs, __trunc, joinDup) {
  const lines = headText.split("\r\n");
  const first = lines.shift().split(" ");
  // 真机口径：req.headers/res.headers 是普通对象（Object.prototype，node 26.8.2
  // 实测）——Object.create(null) 会挂 deepStrictEqual 直比；__proto__ 头名走
  // defineProperty 防原型污染。
  // maxHeadersCount（max-headers-count 套件）：超限对不再收录（静默截断，
  // 响应照常完成；null/0/undefined 即不限）。
  const headers = {};
  const rawHeaders = [];
  // node 口径 headersDistinct：null 原型、每 wire 行一个元素（unique 合并行
  // 为单元素；单例首个赢只影响 headers，distinct 照收——真机实测）。
  const headersDistinct = Object.create(null);
  let __pairs = 0;
  const __capped = typeof maxPairs === "number" && maxPairs > 0;
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
    // maxHeadersCount 超限：node 口径抛 HPE_HEADER_OVERFLOW 走 clientError
    //（count-overflow 套件；有监听自理 431，无监听默认 431 + 销毁——400 通道
    // 不适用）。旧静默截断系伪语义（count 套件从未超限，边界 50/50 无恙）。
    if (__capped && __pairs >= maxPairs) {
      if (__trunc) break;
      const __ov = new Error("HPE_HEADER_OVERFLOW: too many headers");
      __ov.code = "HPE_HEADER_OVERFLOW";
      __ov.__httpParse = true;
      throw __ov;
    }
    __pairs++;
    rawHeaders.push(k, v);
    const lk = k.toLowerCase();
    if (headersDistinct[lk] === undefined) headersDistinct[lk] = [];
    headersDistinct[lk].push(v);
    // node 口径：判重走自有属性（'constructor' 等继承键不得参与合并初值；
    // multiheaders 套件 'constructor: foo, bar, baz'）。重名合并：
    // set-cookie 恒数组、cookie 用 '; '、单例表首个赢（真机三轮实测），其余 ', '。
    if (!Object.prototype.hasOwnProperty.call(headers, lk)) {
      const __first = lk === "set-cookie" ? [v] : v;
      Object.defineProperty(headers, lk, { value: __first, writable: true, enumerable: true, configurable: true });
    } else if (lk === "set-cookie") {
      headers[lk].push(v);
    } else if (lk === "cookie") {
      headers[lk] = `${headers[lk]}; ${v}`;
    } else if (joinDup === true) {
      // joinDuplicateHeaders:true 压过单例表（authorization 套件 '1, 2'）。
      headers[lk] = `${headers[lk]}, ${v}`;
    } else if (__SINGLETON_HEADERS.has(lk)) {
      // 首个赢，后续丢弃（rawHeaders 照收）。
    } else {
      // 缺省：重复头首个赢（authorization 套件真机实测；cookie/set-cookie
      // 上已分流，不受门控）。
    }
  }
  return { first, headers, rawHeaders, headersDistinct };
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
    throw new codes.ERR_INVALID_CHAR();
  }
}
function __lowerHeaders(obj, validation, namesSink) {
  const out = Object.create(null);
  for (const [k, v] of Object.entries(obj ?? {})) {
    // 头名字门（node checkIsHttpToken 口径；invalidheaderfield 套件）。
    if (!__TOKEN_RE.test(k)) throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", k);
    if (validation !== undefined) __checkOutboundHeaderValue(validation, v, k);
    // node 口径：数组值原样保留（wire 逐行发出——'Content-Length': [1,2] 即
    // 两行，double-content-length 套件；旧 String(v) 洗成 '1,2' 系伪语义）。
    // 校验仍按合并串（与旧口径同结果），仅存值分流。
    out[k.toLowerCase()] = Array.isArray(v) ? v.map((__e) => String(__e)) : String(v);
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
// llhttp 口径服务端解析错（真机逐形实测 10 探针）：'Parse Error: <msg>' +
// HPE_* 码 + bytesParsed（当前消息内错误偏移）+ rawPacket（触发本次解析的
// 数据片，非累计）+ __httpParse 旗。rawPacket 缺席时由 __feed 按当前片补齐。
function __hpeServer(code, msg, bytesParsed, rawPacket) {
  const e = new Error(`Parse Error: ${msg}`);
  e.code = code;
  e.bytesParsed = bytesParsed;
  e.rawPacket = rawPacket;
  e.reason = msg;
  e.__httpParse = true;
  return e;
}
// llhttp 方法增量匹配（真机 7 探针钉住）：首字节须 A-Z（否则偏移 0）；后续与
// METHODS 表逐字节取最长公共前缀（分叉处即偏移）；空格结尾但整词未知
// （如 'GE'）偏移即词长。isComplete=false（头未齐）时前缀仍活即 -1（等更多
// 字节，'GE' 未终结不等死不定错）；-1 恒表"合法或待定"。
function __methodErrOffset(token, isComplete) {
  if (token.length === 0) return -1;
  const c0 = token.charCodeAt(0);
  if (c0 < 65 || c0 > 90) return 0;
  let cands = METHODS.filter((m) => m.charCodeAt(0) === c0);
  for (let i = 1; i < token.length; i++) {
    const cc = token.charCodeAt(i);
    cands = cands.filter((m) => m.length > i && m.charCodeAt(i) === cc);
    if (cands.length === 0) return i;
  }
  if (cands.some((m) => m.length === token.length)) return -1;
  return isComplete ? token.length : -1;
}
// Expect: 100-continue 判据（真机 26.8.2 对拍：'100-continue'/'100-Continue'/
// 'foo, 100-continue'/'100-continue, foo' 命中；'100continue'（无连字符）与
// '200-ok' 不命中 → 417 通道）。
const __EXPECT_CONTINUE_RE = /(?:^|[^\w])100-continue(?![\w])/i;
// 泵收尾把 trailer 落到消息上（rawTrailers 保存原拼写；trailers 小写键）。
function __applyTrailers(msg, trailersRaw) {
  // node 口径：wire 回填与 addTrailers 即时落账合并（multiple-headers 套件
  // req 'end' 断言两者叠加：trailers 以 ", " 续接、distinct 续元素）。
  if (msg.rawTrailers === undefined) msg.rawTrailers = [];
  if (msg.trailers === undefined) msg.trailers = {};
  // node 口径：trailersDistinct 为 null 原型对象、值全数组（multiple-headers 套件）。
  if (msg.trailersDistinct === undefined) msg.trailersDistinct = Object.create(null);
  for (let i = 0; i < trailersRaw.length; i += 2) {
    msg.rawTrailers.push(trailersRaw[i], trailersRaw[i + 1]);
    const __lk = trailersRaw[i].toLowerCase();
    msg.trailers[__lk] = `${msg.trailers[__lk] !== undefined ? msg.trailers[__lk] + ", " : ""}${trailersRaw[i + 1]}`;
    if (msg.trailersDistinct[__lk] === undefined) msg.trailersDistinct[__lk] = [];
    msg.trailersDistinct[__lk].push(trailersRaw[i + 1]);
  }
}
// node lib/_http_common.js validateHeaderName/validateHeaderValue
//（header-validators 套件；node:http 具名导出）。
export function validateHeaderName(name) {
  if (typeof name !== "string" || name === "" || !__TOKEN_RE.test(name)) {
    throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", String(name));
  }
}
export function validateHeaderValue(name, value) {
  if (value === undefined) {
    throw new codes.ERR_HTTP_INVALID_HEADER_VALUE("undefined", String(name));
  }
  __checkOutboundHeaderValue("strict", value, String(name));
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
  // 方法段走 llhttp 增量匹配终判（空格结尾整词未知即词长偏移；空方法仍走
  // 通用门——真机未点名，400 口径不变）。
  const __mOff = __methodErrOffset(first[0] ?? "", true);
  if (__mOff !== -1) throw __hpeServer("HPE_INVALID_METHOD", "Invalid method encountered", __mOff);
  if (first.length !== 3 || first[0] === "" || /\s/.test(first[1]) ||
      !/^HTTP\/\d(\.\d)?$/.test(first[2])) {
    throw __mkParseError("bad request line");
  }
  for (const k of Object.keys(headers)) {
    if (!__TOKEN_RE.test(k)) throw __mkParseError("bad header name");
  }
}

// llhttp 头字节计数（真机 10 点二分校准，2026-09-23）：
// 请求 = url 长 + Σ(名长 + 值长)；响应 = 状态短语长 + Σ(名长 + 值长)；
// 值计已到达部分（前导 OWS 剥离；完成行全 trim，与 __parseHead 同口径）；
// 未完成行（无冒号）不计；首行未完成（buf 内无 CRLF）时按字节 backstop
// （有效头首行恒短，无 CRLF 即超长行，旧字节门行为保留）。
// 结论：`count >= limit` 即 HPE_HEADER_OVERFLOW（16383 过 / 16384 拒）。
function __headSemCount(buf, isResponse) {
  const text = __latin1(buf);
  const lines = text.split("\r\n");
  if (lines.length === 1) return buf.length;
  let n = 0;
  const first = lines[0].split(" ");
  if (isResponse) {
    n += first.length >= 3 ? first.slice(2).join(" ").length : 0;
  } else {
    n += first.length >= 2 ? first[1].length : 0;
  }
  for (let i = 1; i < lines.length - 1; i++) {
    const line = lines[i];
    const c = line.indexOf(":");
    if (c <= 0) continue;
    const v = line.slice(c + 1).replace(/^[ \t]+/, "").replace(/[ \t]+$/, "");
    n += line.slice(0, c).trim().length + v.length;
  }
  const tail = lines[lines.length - 1];
  if (tail !== "") {
    const c = tail.indexOf(":");
    if (c > 0) {
      n += tail.slice(0, c).trim().length + tail.slice(c + 1).replace(/^[ \t]+/, "").length;
    }
  }
  return n;
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
      // 方法段内 CR 即方法终结（'Oopsie-doopsie\r\n' 形）：整词终判——分叉处
      // 即偏移（真机 1，非词长），与空格终结同 matcher。
      if (seg === 0 && method !== "") {
        const __off = __methodErrOffset(method, true);
        if (__off !== -1) throw __hpeServer("HPE_INVALID_METHOD", "Invalid method encountered", __off);
      }
      throw __mkParseError("bad request line");
    }
    if (ch === 10 || ch === 0) throw __mkParseError("bad request line");
    if (ch === 32) {
      if (seg === 0) {
        if (method === "") throw __mkParseError("bad request line");
        // 方法段终结：整词终判（增量匹配分叉处即偏移，未知词即词长）。
        const __off = __methodErrOffset(method, true);
        if (__off !== -1) throw __hpeServer("HPE_INVALID_METHOD", "Invalid method encountered", __off);
      } else if (seg === 2 || (seg === 1 && !urlStarted)) {
        throw __mkParseError("bad request line");
      }
      seg++;
      continue;
    }
    if (seg === 0) {
      // 方法段收字节不设 token 门——合法性由尾部增量匹配统一判定
      // （小写首字节/分叉字节在匹配器内定偏移）。
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
  // 尾部：方法段未终结（头未齐）→ 增量终判（分叉即错，前缀仍活即等更多字节，
  // 'GE' 悬置不等死不定错——真机 request-timeout 形）。
  if (seg === 0 && method !== "") {
    const __off = __methodErrOffset(method, false);
    if (__off !== -1) throw __hpeServer("HPE_INVALID_METHOD", "Invalid method encountered", __off);
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
  // node 口径：TE 值数组（重复 TE 行合并数组，multiheaders 套件）先 join
  // 再判 chunked；字符串直判。
  const __tev = headers["transfer-encoding"];
  const te = (Array.isArray(__tev) ? __tev.join(", ") : (__tev || "")).toLowerCase();
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
    this.trailersDistinct = Object.create(null);
    this.headersDistinct = Object.create(null);
    this.complete = false;
    // node 口径：aborted 缺省 false，中止置 true（aborted 套件双侧断言）。
    this.__aborted = false;
    // node 口径 connection/socket 联动（connection-setter 套件）：
    // connection 赋值同步 socket（反之亦然）；双双缺席即 undefined。
    this.__connSock = undefined;
  }
  get connection() { return this.__connSock; }
  set connection(v) { this.__connSock = v; }
  get socket() { return this.__connSock; }
  set socket(v) { this.__connSock = v; }
  // node 口径 client（socket 废弃别名，req-close-robust 套件直读 _events）。
  get client() { return this.socket; }
  set client(v) { this.socket = v; }
  // node lib/_http_incoming.js _addHeaderLine（matchKnownFields 套件）：
  // 单例首个赢（含 undefined 占位）、set-cookie 数组、cookie '; '、其余 ', '。
  // dest 缺席即落自身 headers（内部复用）。
  _addHeaderLine(field, value, dest) {
    const target = dest ?? this.headers;
    const lk = String(field).toLowerCase();
    if (lk === "set-cookie") {
      if (!Array.isArray(target[lk])) target[lk] = [];
      target[lk].push(value);
      return;
    }
    if (__SINGLETON_HEADERS.has(lk)) {
      if (!Object.prototype.hasOwnProperty.call(target, lk)) target[lk] = value;
      return;
    }
    if (lk === "cookie") {
      target[lk] = target[lk] === undefined ? value : `${target[lk]}; ${value}`;
      return;
    }
    target[lk] = target[lk] === undefined ? value : `${target[lk]}, ${value}`;
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
    // 写机构 HWM 跟 socket 可写 HWM（node 口径：res 背压由 conn.write 治理，
    // socket 机构 HWM 是判据——response-drain-cork 套件改
    // `socket._writableState.highWaterMark = 1000` 后 res.write(1010) 即 false；
    // 本仓背压机构在 res 层，HWM 构造期取 socket 侧，缺省 socket 双 65536）。
    const __shwm = sock && typeof sock.write === "function" &&
      sock._writableState !== undefined && sock._writableState !== null
      ? sock._writableState.highWaterMark : undefined;
    super({ autoDestroy: false, highWaterMark: __shwm });
    // 构造首参：server 流程传 socket；独立构造传 req 形信息对象（node 口径
    // `new ServerResponse(req)`，standalone 套件）——非 socket 一律不入 __sock。
    this.__sock = sock && typeof sock.write === "function" ? sock : null;
    this.__sockAssigned = false;
    // node 口径 writableLength（outgoing-properties 套件）：已排队待上 socket
    // 的字节（渲染头 + 帧化块；standalone 无头即裸块累计）。流机构 length 不
    // 可用（_write 回调 microtask 即清零，与落盘节奏脱节），故独立记账：
    // 写时预测递增、落盘按实际递减、_final 兜底清零。
    this.__wlen = 0;
    this.__headCounted = false;
    // node 口径：res.socket / res.connection 指向响应 socket（agent-keepalive
    // 套件服务端经 res.connection 取 socket 再 end）。
    this.socket = this.__sock;
    this.connection = this.__sock;
    this.statusCode = 200;
    this.statusMessage = undefined;
    this.__headers = Object.create(null);
    // node 口径 _removedHeader：删掉的头不再自动补（remove-header 套件）。
    this._removedHeader = {};
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
    // node 口径（head-throw 套件真机实测）：rejectNonStandardBodyWrites 为 true
    // 时，1xx/204/304/HEAD 响应的 write/end(chunk) 同步抛 BODY_NOT_ALLOWED。
    this.__rejectBody = false;
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
  // node 口径 writableLength 覆写（基类流机构 getter 只计未调 _write 的字节，
  // 与 socket 落盘脱节——outgoing-properties 套件要渲染头+帧化块精确字节）。
  get writableLength() { return this.__wlen ?? 0; }
  // socket 落盘统一出口（记账递减 + 递送；钳零——100-continue/终结块等非 body
  // 写不参与计数，CL 快捷真值由 _final 兜底清零对齐）。
  __sockWrite(b) {
    try {
      if (typeof this.__wlen === "number") {
        this.__wlen = Math.max(0, this.__wlen - (b !== undefined && b !== null ? b.length : 0));
      }
      // bytesWritten pending 核销（实际落盘长度；CL 快捷等真值偏差由 _final 兜底）。
      if (this.__sock !== null && typeof this.__sock.__bwSub === "function") {
        this.__sock.__bwSub(b !== undefined && b !== null ? b.length : 0);
      }
    } catch { /* 计数永不阻递送 */ }
    // 停靠 drain 释放（计数清零即递送，异步一轮——真机 drain 恒异步）。
    if (this.__wlen === 0 && this.__parkedDrain === true) {
      this.__parkedDrain = false;
      queueMicrotask(() => {
        if (!this.destroyed) { try { super.emit("drain"); } catch { /* 监听抛错不阻收尾 */ } }
      });
    }
    return this.__sock.write(b);
  }
  // 头渲染 dry-run（计数专用）：快照→渲染→取值→还原。调用点保证头已终局
  // （首 _write 后 setHeader/writeHead 即抛，头冻结），故重渲染逐字节恒等
  // （Date 同长），还原后真实渲染不受影响。
  __predictHeadLen() {
    const __snap = {
      headers: { ...this.__headers },
      headSent: this.__headSent,
      headersSent: this.headersSent,
      chunked: this.__chunked,
      rawCL: this.__rawCL,
      last: this.__last,
      keepAlive: this.__keepAlive,
      autoConn: this.__autoConn,
      autoDate: this.__autoDate,
      autoKA: this.__autoKA,
      defaultKA: this.__defaultKA,
      contentLength: this.__contentLength,
    };
    let __n = 0;
    try {
      __n = this.__headBytes().length;
    } catch { __n = 0; }
    this.__headers = __snap.headers;
    this.__headSent = __snap.headSent;
    this.headersSent = __snap.headersSent;
    this.__chunked = __snap.chunked;
    this.__rawCL = __snap.rawCL;
    this.__last = __snap.last;
    this.__keepAlive = __snap.keepAlive;
    this.__autoConn = __snap.autoConn;
    this.__autoDate = __snap.autoDate;
    this.__autoKA = __snap.autoKA;
    this.__defaultKA = __snap.defaultKA;
    this.__contentLength = __snap.contentLength;
    return __n;
  }
  // 块帧化长度预测（与 __frame / _write 落盘判定同谓词，见 _write 1338 行族）。
  __predictFrameLen(u8) {
    if (this.__noBody || this.__headOnly || u8.length === 0) return 0;
    const __ch = this.__chunked || (this.__uced && this.__headers["content-length"] === undefined &&
      this.__headers["transfer-encoding"] === undefined && !this.__frameSuppressed());
    if (__ch && !this.__rawCL) return u8.length.toString(16).length + 2 + u8.length + 2;
    return u8.length;
  }
  // 写时记账（_write/_send 入口）：首渲染头 + 帧化块。socket-null 停靠（Slice B
  // 管线队列）同计数，assignSocket 排空时按实际递减。
  __countOut(u8) {
    if (!this.__headCounted && !this.__headSent) {
      this.__headCounted = true;
      this.__wlen += this.__predictHeadLen();
    }
    this.__wlen += this.__predictFrameLen(u8);
  }
  setHeader(name, value) {
    if (this.headersSent) throw new codes.ERR_HTTP_HEADERS_SENT("set");
    // node 口径（write-head 套件真机实测）：数字名亦 HTTP_TOKEN（"3840" 本身是
    // 合法 token 字符，故不能只测 String(name)，须先判 typeof）。
    if (typeof name !== "string") throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", String(name));
    if (!__TOKEN_RE.test(name)) throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", name);
    if (value === undefined) throw new codes.ERR_HTTP_INVALID_HEADER_VALUE("undefined", String(name));
    const lk = String(name).toLowerCase();
    if (this._removedHeader !== undefined) delete this._removedHeader[lk];
    // node 口径：数组值原样存（set-cookie/array 套件；wire 逐行发出）。
    if (Array.isArray(value)) {
      const __arr = [];
      for (const __e of value) {
        __checkOutboundHeaderValue(this.__validation, __e, String(name));
        __arr.push(__e);
      }
      this.__headers[lk] = __arr;
    } else {
      __checkOutboundHeaderValue(this.__validation, value, String(name));
      this.__headers[lk] = value;
    }
    // node 口径：wire 保留用户原拼写（kOutHeaders 存 [name, value] 原文名，
    // 首写优先——multiple-headers 套件全 'X-Res-a'）。
    if (this.__headerNames[lk] === undefined) this.__headerNames[lk] = String(name);
    return this;
  }
  // node OutgoingMessage.appendHeader（header-value-relaxed 套件点名）。
  // node 口径（multiple-headers 套件 + 真机探针）：缺省追加为单元素；
  // 任意一侧数组即数组拼接；发头后即 ERR_HTTP_HEADERS_SENT。
  appendHeader(name, value) {
    if (this.headersSent) throw new codes.ERR_HTTP_HEADERS_SENT("append");
    if (typeof name !== "string") throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", String(name));
    if (!__TOKEN_RE.test(name)) throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", name);
    const lk = String(name).toLowerCase();
    const __vals = Array.isArray(value) ? value : [value];
    for (const __e of __vals) __checkOutboundHeaderValue(this.__validation, __e, String(name));
    const cur = this.__headers[lk];
    if (cur === undefined) {
      this.__headers[lk] = Array.isArray(value) ? [...value] : value;
    } else if (Array.isArray(cur)) {
      for (const __e of __vals) cur.push(__e);
    } else {
      this.__headers[lk] = Array.isArray(value) ? [cur, ...value] : [cur, value];
    }
    if (this.__headerNames[lk] === undefined) this.__headerNames[lk] = String(name);
    return this;
  }
  getHeader(name) {
    if (typeof name !== "string") throw new codes.ERR_INVALID_ARG_TYPE("name", "string", name);
    return this.__headers[name.toLowerCase()];
  }
  removeHeader(name) {
    // node 口径：发头后即 ERR_HTTP_HEADERS_SENT（remove-header-after-sent 套件）。
    if (this.headersSent) throw new codes.ERR_HTTP_HEADERS_SENT("remove");
    if (typeof name !== "string") throw new codes.ERR_INVALID_ARG_TYPE("name", "string", name);
    const lk = name.toLowerCase();
    delete this.__headers[lk];
    delete this.__headerNames[lk];
    // node _removedHeader 口径：删掉的自动头不再补（remove-header 套件）。
    if (this._removedHeader !== undefined) this._removedHeader[lk] = true;
    return this;
  }
  getHeaderNames() { return Object.keys(this.__headers); }
  hasHeader(name) {
    if (typeof name !== "string") throw new codes.ERR_INVALID_ARG_TYPE("name", "string", name);
    return this.__headers[name.toLowerCase()] !== undefined;
  }
  getHeaders() {
    const __out = Object.create(null);
    for (const k of Object.keys(this.__headers)) __out[k] = this.__headers[k];
    return __out;
  }
  getRawHeaderNames() {
    const __names = this.__headerNames ?? {};
    return Object.keys(this.__headers).map((k) => __names[k] ?? k);
  }
  writeHead(status, ...rest) {
    // node 口径（write-head 套件真机实测）：已发头再 write 即 HEADERS_SENT。
    if (this.headersSent) throw new codes.ERR_HTTP_HEADERS_SENT("write");
    // node 口径：writeHead 即算发头（setheaders 套件 writeHead 后 setHeaders
    // 即 HEADERS_SENT）——旗在合并完成后立，合并走 setHeader 不得自炸。
    // 状态码门（node lib/_http_server.js writeHead 原文 + response-statuscode
    // 套件 13 形态：`statusCode |= 0` 后判 100..999，错抛原值（`%s` 遇对象走
    // inspect——{}→'{}'；字符串 '1000' 越界仍原样；RangeError）。
    const __origStatus = status;
    status |= 0;
    if (status < 100 || status > 999) {
      throw new codes.ERR_HTTP_INVALID_STATUS_CODE(__origStatus);
    }
    const obj = rest.find((r) => r && typeof r === "object");
    // node 口径：writeHead 的头参数收扁平数组（setheaders 套件块 4
    // ['foo','3'] 即覆盖；与 ClientRequest 构造器数组形同源）。
    const __objArr = Array.isArray(obj) ? obj : null;
    const msg = rest.find((r) => typeof r === "string");
    // node 口径：writeHead 原子提交——合并/校验抛错（TRAILER_INVALID 等）即
    // 全量回滚（timeout 黑盒 t9：失败的 writeHead 不得污染 CL/状态码/旗位，
    // 否则后续 removeHeader + end 全灭）。
    const __snap = {
      headers: this.__headers,
      names: this.__headerNames,
      removed: this._removedHeader,
      code: this.statusCode,
      message: this.statusMessage,
      stored: this.__storedStatus,
      sent: this.headersSent,
    };
    // 浅拷贝表层（值数组另拷，防合并中途污染原数组）。
    const __copyHeaders = () => {
      const __o = Object.create(null);
      for (const __k of Object.keys(this.__headers)) {
        const __v = this.__headers[__k];
        __o[__k] = Array.isArray(__v) ? [...__v] : __v;
      }
      return __o;
    };
    this.__headers = __copyHeaders();
    this.__headerNames = { ...(this.__headerNames ?? {}) };
    this._removedHeader = { ...(this._removedHeader ?? {}) };
    const __rollback = (e) => {
      this.__headers = __snap.headers;
      this.__headerNames = __snap.names;
      this._removedHeader = __snap.removed;
      this.statusCode = __snap.code;
      this.statusMessage = __snap.message;
      this.__storedStatus = __snap.stored;
      this.headersSent = __snap.sent;
      throw e;
    };
    this.statusCode = status;
    // node 口径：wire 状态码以 writeHead 时为准，事后改 statusCode 属性只改
    // 属性值、不改 wire（mutable-headers writeHead 案：属性 201/wire 200）。
    this.__storedStatus = status;
    // node 口径（write-head 套件真机实测）：无显式短语即标准短语，未知码为
    // 'unknown'（220 案；wire 同）。
    if (msg !== undefined) this.statusMessage = msg;
    else this.statusMessage = STATUS_CODES[status] ?? "unknown";
    // node 口径：writeHead 合并头值数组原样存（逐行发出；multiple-headers 套件
    // 'x-res-c': ['HHH','III'] 即两行，旧 Object.assign 经 __lowerHeaders 洗成
    // 逗号串系伪语义）。扁平数组即逐对 setHeader（setheaders 套件块 4）。
    // 内联合并（不得走 setHeader：headersSent 门会自炸；校验与存值同对象分支）。
    // 合并 + 校验整体 try 包裹，抛错即回滚（上见 __rollback）。
    try {
    if (__objArr !== null) {
      // node 口径（set-trailers 套件真机实测）：writeHead 收对形 `[[k,v],...]`
      //（与 ClientRequest 构造器双形同源）——逐对取 [0]/[1]（ ["b"] 即 value
      // undefined 走 INVALID_HEADER_VALUE；超长对多余元忽略），归一扁平后走
      // 下方同套逻辑。
      let __pairs = __objArr;
      if (__objArr.length > 0 && Array.isArray(__objArr[0])) {
        __pairs = [];
        for (const __p of __objArr) __pairs.push(__p[0], __p[1]);
      }
      // node 口径（write-head 套件真机实测）：奇长数组即 ARG_VALUE 'headers'，
      // 非 ARG_TYPE（"The argument 'headers' is invalid"）。
      if (__pairs.length % 2 !== 0) throw new codes.ERR_INVALID_ARG_VALUE("headers", obj);
      // node 口径（write-head-after-set-header 套件真机实测）：扁平数组内同键
      // 对逐行保留（['a','1','a','2'] 即两行）；首对覆写先前 setHeader，同键后对
      // 累积（首触覆写、再触累积）。
      const __touched = new Set();
      for (let __i = 0; __i < __pairs.length; __i += 2) {
        const __k = String(__pairs[__i]);
        const __v = __pairs[__i + 1];
        if (!__TOKEN_RE.test(__k)) throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", __k);
        if (__v === undefined) throw new codes.ERR_HTTP_INVALID_HEADER_VALUE("undefined", __k);
        const __lk = __k.toLowerCase();
        if (this._removedHeader !== undefined) delete this._removedHeader[__lk];
        const __vals = Array.isArray(__v) ? __v : [__v];
        for (const __e of __vals) __checkOutboundHeaderValue(this.__validation, __e, __k);
        if (!__touched.has(__lk)) {
          __touched.add(__lk);
          this.__headers[__lk] = Array.isArray(__v) ? [...__v] : __v;
        } else {
          const __cur = this.__headers[__lk];
          if (Array.isArray(__cur)) for (const __e of __vals) __cur.push(__e);
          else this.__headers[__lk] = [__cur, ...__vals];
        }
        // node 口径（write-head 套件真机实测）：writeHead 覆写拼写（setHeader
        // 'test' 后 writeHead {Test:'2'}，wire 为 'Test'，非首写优先——首写优先
        // 仅 setHeader 之间）。
        this.__headerNames[__lk] = __k;
      }
    }
    this.headersSent = true;
    if (obj !== undefined && __objArr === null) {
      for (const k of Object.keys(obj)) {
        if (!__TOKEN_RE.test(String(k))) throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", String(k));
        const __v = obj[k];
        const __lk = String(k).toLowerCase();
        if (this._removedHeader !== undefined) delete this._removedHeader[__lk];
        if (Array.isArray(__v)) {
          const __arr = [];
          for (const __e of __v) {
            __checkOutboundHeaderValue(this.__validation, __e, String(k));
            __arr.push(__e);
          }
          this.__headers[__lk] = __arr;
        } else {
          __checkOutboundHeaderValue(this.__validation, __v, String(k));
          this.__headers[__lk] = __v;
        }
        this.__headerNames[__lk] = String(k);
      }
    }
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
    } catch (__whErr) {
      __rollback(__whErr);
    }
    this.headersSent = true;
    // 头已存：随后的 end(data) 不再走 CL 快路径（真机 chunked 口径）。
    this.__headStored = true;
    return this;
  }
  // node writeContinue：headersSent 前一次性发 100 Continue 中间响应。
  writeContinue() {
    if (this.__continueSent || this.headersSent || this.__headSent) return;
    this.__continueSent = true;
    if (this.__sock !== null) {
      try { this.__sockWrite(new TextEncoder().encode("HTTP/1.1 100 Continue\r\n\r\n")); } catch { /* gone */ }
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
      throw new codes.ERR_HTTP_INVALID_STATUS_CODE(status);
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
      this.__sockWrite(new TextEncoder().encode(lines.join("\r\n") + "\r\n\r\n"));
    } catch { return false; /* gone */ }
    return true;
  }
  // node setHeaders：收 Headers 实例或 Map（setheaders 套件真机实测双形；
  // 其余（数组/对象/null/undefined/字符串/数字）一律 ERR_INVALID_ARG_TYPE；
  // 品牌按名判定，跨域稳定）。
  // 发头后即 ERR_HTTP_HEADERS_SENT（首块）。
  setHeaders(headers) {
    if (this.headersSent) throw new codes.ERR_HTTP_HEADERS_SENT("set");
    let __isHeaders = false;
    if (headers !== null && typeof headers === "object" && typeof headers.entries === "function") {
      const __tag = headers[Symbol.toStringTag];
      const __ctor = headers.constructor !== undefined && headers.constructor !== null
        ? headers.constructor.name : "";
      __isHeaders = __tag === "Headers" || __tag === "Map" || __ctor === "Headers" || __ctor === "Map";
    }
    if (!__isHeaders) {
      throw new codes.ERR_INVALID_ARG_TYPE("headers", ["Headers instance"], headers);
    }
    for (const [k, v] of headers.entries()) this.appendHeader(k, v);
    return this;
  }
  // node addTrailers：分块响应终结块尾随头（原拼写输出；真机 rawTrailers 口径）。
  // 值数组按元素展开多行（multiple-headers 套件；拼写取用户原文）。
  // trailers/trailersDistinct/rawTrailers 即时落账（multiple-headers 套件
  // req 'end' 内断言 wire 回环值）。
  addTrailers(trailers) {
    if (trailers === null || trailers === undefined) return this;
    for (const k of Object.keys(trailers)) {
      if (!__TOKEN_RE.test(String(k))) throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", String(k));
      const __v = trailers[k];
      const __vals = Array.isArray(__v) ? __v : [__v];
      // node 口径 uniqueHeaders：名单内 trailer 行 wire 合并单行 '; '
      //（multiple-headers 套件；与 ClientRequest 侧同口径）。
      const __uniq = this.__uniqueHeaders;
      if (Array.isArray(__uniq) && __uniq.includes(String(k).toLowerCase())) {
        for (const __e of __vals) __validateHeaderValue(__e);
        this.__trailer = (this.__trailer ?? "") + `${k}: ${__vals.join("; ")}\r\n`;
        if (this.trailers !== undefined) {
          if (this.rawTrailers === undefined) this.rawTrailers = [];
          this.rawTrailers.push(k, __vals.join("; "));
          const __lk = String(k).toLowerCase();
          this.trailers[__lk] = __vals.join("; ");
          if (this.trailersDistinct === undefined) this.trailersDistinct = Object.create(null);
          if (this.trailersDistinct[__lk] === undefined) this.trailersDistinct[__lk] = [];
          this.trailersDistinct[__lk].push(__vals.join("; "));
        }
        continue;
      }
      for (const __e of __vals) {
        __validateHeaderValue(__e);
        this.__trailer = (this.__trailer ?? "") + `${k}: ${__e}\r\n`;
        if (this.trailers !== undefined) {
          if (this.rawTrailers === undefined) this.rawTrailers = [];
          this.rawTrailers.push(k, String(__e));
          const __lk = String(k).toLowerCase();
          this.trailers[__lk] = `${this.trailers[__lk] !== undefined ? this.trailers[__lk] + ", " : ""}${__e}`;
          if (this.trailersDistinct === undefined) this.trailersDistinct = Object.create(null);
          if (this.trailersDistinct[__lk] === undefined) this.trailersDistinct[__lk] = [];
          this.trailersDistinct[__lk].push(String(__e));
        }
      }
    }
    return this;
  }
  // node 口径（write-after-end 套件 + head-throw 套件真机实测）：
  // end 后再写不进基类（基类置 errored 会压住在途 _final 致 chunk 终结块丢失，
  // 同步/异步双探针实证）——自发 error + 回 false；end 优先于拒写旗（204 先
  // end 后写仍走 WRITE_AFTER_END，非同步抛）。
  write(chunk, encoding, cb) {
    if (!this.destroyed && (this.__userEnded || this.writableEnded)) {
      if (typeof encoding === "function") { cb = encoding; encoding = null; }
      const __cb = typeof cb === "function" ? cb : null;
      const er = new codes.ERR_STREAM_WRITE_AFTER_END();
      queueMicrotask(() => { if (__cb) __cb(er); if (!this.destroyed) this.emit("error", er); });
      return false;
    }
    // node 口径（head-throw 套件真机实测）：拒写旗下无体响应的 write 同步抛
    //（含空串；校验在 write 包装层，不进 _write——流内抛会毒化 writing 态，
    // 后续 end 永挂）。
    if (this.__rejectBody && this.__isNoBodyStatus()) {
      throw new codes.ERR_HTTP_BODY_NOT_ALLOWED();
    }
    if (this.__sockGone || this.__sock === null || (this.__sock !== null && this.__sock.destroyed)) {
      // socket-null 有两种：入列停靠（__queued，写继续走流机构→_write park）与
      // 独立构造（未入列，旧口径回 false——incoming-pipelined 套件管线续行写）。
      if (this.__sock === null && this.__queued === true) {
        if (chunk !== undefined && chunk !== null) {
          try { this.__countOut(chunk instanceof Uint8Array ? chunk : __toU8(String(chunk))); } catch { /* 计数永不阻写 */ }
        }
        return super.write(chunk, encoding, cb);
      }
      return false;
    }
    // 写时记账（writableLength 精确字节，见 __countOut）：write 包装层同步计
    // （流机构异步派发 _write，_write 时机计数会漏同步读——outgoing-properties
    // 套件连写两行后同步读）；end 块由 end 包装层计，_write 内不计（防双计）。
    // socket bytesWritten 同步预测（byteswritten 套件）：__wlen 增量同步到
    // socket pending（落盘 __sockWrite 核销、_final 兜底清零）。
    if (chunk !== undefined && chunk !== null) {
      try {
        const __before = this.__wlen ?? 0;
        this.__countOut(chunk instanceof Uint8Array ? chunk : __toU8(String(chunk)));
        const __d = (this.__wlen ?? 0) - __before;
        if (__d > 0 && this.__sock !== null && typeof this.__sock.__bwAdd === "function") {
          this.__sock.__bwAdd(__d);
        }
      } catch { /* 计数永不阻写 */ }
    }
    return super.write(chunk, encoding, cb);
  }
  // node 口径（head-throw 套件）：1xx/204/304/HEAD 为无体响应（writeHead 置
  // __noBody/方法置 __headOnly；writeHead 前按 statusCode 活读）。
  __isNoBodyStatus() {
    const __sc = this.__storedStatus !== undefined ? this.__storedStatus : this.statusCode;
    return this.__headOnly || (__sc >= 100 && __sc <= 199) || __sc === 204 || __sc === 304;
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
    // node 口径（head-throw 套件真机实测）：拒写旗下 end(chunk) 同步抛（write 同）。
    if (__hasChunk && this.__rejectBody && this.__isNoBodyStatus()) {
      throw new codes.ERR_HTTP_BODY_NOT_ALLOWED();
    }
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
    // node 口径：end() 返回后头即算发出（multiple-headers 套件 end 后
    // appendHeader 即 HEADERS_SENT；_final 异步，旗必须同步立）。
    // node end() 原文：end 即全开——socket corked 置 1 再 uncork（强制归零）
    // + 消息级 corked 置 1 再 uncork（滞留尾随排空；outgoing-end-cork 套件
    // end 后 writableCorked===0；response-cork 套件 end 后双侧 corked 恒等）。
    if (this._writableState && this._writableState.corked > 0) {
      this._writableState.corked = 1;
      super.uncork();
    }
    if (this.__sock !== null && typeof this.__sock.uncork === "function" &&
        (this.__sock.__corkCnt ?? 0) > 0) {
      this.__sock.__corkCnt = 1;
      this.__sock.uncork();
    }
    this.headersSent = true;
    this.__endHadData = chunk !== undefined && chunk !== null && typeof chunk !== "function";
    this.__userEnded = true;
    try {
      const __r = super.end(chunk, encoding, cb);
      // end 块同步记账（流 end 经内部 _write 直调，不走 write 包装层，此处补计；
      // socket pending 同上）。
      if (this.__endHadData) {
        try {
          const __before = this.__wlen ?? 0;
          this.__countOut(chunk instanceof Uint8Array ? chunk : __toU8(String(chunk)));
          const __d = (this.__wlen ?? 0) - __before;
          if (__d > 0 && this.__sock !== null && typeof this.__sock.__bwAdd === "function") {
            this.__sock.__bwAdd(__d);
          }
        } catch { /* 计数永不阻收尾 */ }
      }
      return __r;
    } catch (e) {
      // 基类校验抛（如数组 chunk）不得毒化旗位，否则后续合法 end 永不到
      // （end-types 套件 hang 根因）。
      this.__endHadData = false;
      this.__userEnded = false;
      throw e;
    }
  }
  // node lib/_http_outgoing.js cork 口径（response-cork/response-drain-cork/
  // outgoing-end-cork 三套件逐项对拍）：res.cork() = 消息级计数 + socket.cork()
  // 镜像（writableCorked 双侧恒等；node 消息级 kCorked 以流机构 cork 承载——
  // 本仓 res 即 Writable，机构 cork 滞留字节于 res 缓冲，corked 期间 _write
  // 不被调、socket.write 不被调；node 由 socket 机构/kChunkedBuffer 持，
  // 可观测等价：滞留不落盘、write() 返回值走 HWM、drain 随排空发射）。
  // 偏差记档：node uncork 尾flush 把滞留块**合并为一个 chunk**（kChunkedBuffer
  // 总长一帧），本仓机构排空逐块成帧——字节流恒等，chunk 边界不同，套件未点名。
  cork() {
    super.cork();
    if (this.__sock !== null && typeof this.__sock.cork === "function") this.__sock.cork();
    return this;
  }
  uncork() {
    super.uncork();
    if (this.__sock !== null && typeof this.__sock.uncork === "function") this.__sock.uncork();
    return this;
  }
  // drain 门控（drain-writable-length 套件）：socket 落盘未完（__wlen>0）时
  // 流机构的 'drain' 递延至清零（早发即 writableLength 非零）；销毁即弃。
  // 无积压即直通（常规路径零行为差）。
  emit(ev, ...args) {
    if (ev === "drain" && (this.__wlen ?? 0) > 0) {
      if (!this.destroyed) this.__parkedDrain = true;
      return false;
    }
    return super.emit(ev, ...args);
  }
  // CL/TE 被删掉时自动帧全停（remove-header 套件；__headBytes 帧决策同口径）。
  __frameSuppressed() {
    return this._removedHeader !== undefined &&
      !!(this._removedHeader["content-length"] || this._removedHeader["transfer-encoding"]);
  }
  // 立即发头（Node flushHeaders：body 可经 chunked 帧，end 后补终结块）。
  flushHeaders() {
    if (this.__headSent || this.__noBody || this.__headOnly) return;
    if (this.__headers["content-length"] === undefined && this.__headers["transfer-encoding"] === undefined && this.__uced && !this.__frameSuppressed()) {
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
  // 管线轮转同样经此（排空停靠写 + 递补终结，见 __feed/__onDone）。
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
    this.__queued = false;
    // node 口径：assign 即发 'socket'（setTimeout 无 sock 等待形与监听侧靠它）。
    try { this.emit("socket", sock); } catch { /* 监听抛错不阻排空 */ }
    const __q = this.__parked ?? [];
    this.__parked = [];
    for (const [__b, __c] of __q) {
      try { this._write(__b, null, __c); } catch { try { __c(); } catch { /* gone */ } }
    }
    if (this.__finalParked !== null && this.__finalParked !== undefined) {
      const __f = this.__finalParked;
      this.__finalParked = null;
      try { this._final(__f); } catch { try { __f(); } catch { /* gone */ } }
    }
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
    // wire 状态码以 writeHead 快照为准（__storedStatus；无 writeHead 即活读）。
    const __sc = this.__headStored && this.__storedStatus !== undefined ? this.__storedStatus : this.statusCode;
    if (__sc === 204 || __sc === 304) {
      if (this.__chunked) { this.__chunked = false; this.__keepAlive = false; }
      this.__noBody = true;
    }
    const reason = this.statusMessage ?? STATUS_CODES[__sc] ?? "unknown";
    const head = [`HTTP/1.1 ${__sc} ${reason}`.trimEnd()];
    // node 口径：用户头（含 writeHead 合并头）恒在自动头（Date/Connection/
    // Keep-Alive/CL/TE）之前（真机三探针：plain/set/set+wh 全同序）。
    const __auto = [];
    const __user = [];
    const __autoKey = (k) => {
      if (k === "connection" && this.__autoConn) return true;
      if (k === "date" && this.__autoDate) return true;
      if (k === "keep-alive" && this.__autoKA) return true;
      // 本轮新补的自动 CL/TE（用户未显式设置时）。
      if ((k === "content-length" || k === "transfer-encoding") && this.__headerNames[k] === undefined) return true;
      return false;
    };
    // 自动 Date 头（node 口径：响应缺 date 即补 UTC 串；删掉的不补；
    // sendDate === false 不补——test-http-1.0 套件 curl 形断言无 Date 行）。
    // 顺序：Date 恒在 Connection/Keep-Alive 之前（真机 wire 顺序，chunked-304
    // 套件 `/^Connection: close\r\n$/m` 钉住 Connection 紧贴头终结）。
    if (this.sendDate !== false && this.__headers["date"] === undefined &&
        !(this._removedHeader !== undefined && this._removedHeader.date)) {
      this.__headers["date"] = new Date().toUTCString();
      this.__autoDate = true;
    }
    // Connection 自动决策（node keep-alive logic 口径）：
    // shouldSendKeepAlive = shouldKeepAlive && (用户CL || UCED)；
    // maxRequestsPerSocket 达标 → close；否则 keep-alive（+Keep-Alive: timeout）；
    // 否则 close + _last。
    // _removedHeader 口径：删掉的 connection 不再自动补（remove-header 套件）。
    if (!__st.conn && !(this._removedHeader !== undefined && this._removedHeader.connection)) {
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
    // 帧决策（node：!contLen && !te 分支）：无用户 CL/TE 时按
    // noBody/UCED/__contentLength（end() 快路径预置）决定 auto CL 或 chunked；
    // 1.0（UCED false）→ _last（close-delimited）。CL/TE 被删掉时不自动补
    // CL/chunked，直接 _last（remove-header 套件：close-delimited 体）。
    if (!__st.cl && !__st.te) {
      const __frSup = this._removedHeader !== undefined &&
        !!(this._removedHeader["content-length"] || this._removedHeader["transfer-encoding"]);
      if (__frSup) {
        this.__chunked = false;
        this.__last = true;
      } else if (this.__noBody || this.__headOnly) {
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
      // node 口径：数组值逐行发出（set-cookie/多值头）；uniqueHeaders 名单内
      // 用 '; ' 合并单行（multiple-headers 套件；发送侧合并，解析侧天然单元素）。
      // cookie 数组恒单行 '; ' 合并（真机 26.8.2 实测，双端同）。
      if (Array.isArray(v)) {
        const __uniq = this.__uniqueHeaders;
        if (k === "cookie") {
          (__autoKey(k) ? __auto : __user).push(`${name}: ${v.join("; ")}`);
          continue;
        }
        if (Array.isArray(__uniq) && __uniq.includes(k)) {
          (__autoKey(k) ? __auto : __user).push(`${name}: ${v.join("; ")}`);
          continue;
        }
        for (const __e of v) (__autoKey(k) ? __auto : __user).push(`${name}: ${__e}`);
        continue;
      }
      (__autoKey(k) ? __auto : __user).push(`${name}: ${v}`);
    }
    for (const __line of __user) head.push(__line);
    for (const __line of __auto) head.push(__line);
    return new TextEncoder().encode(head.join("\r\n") + "\r\n\r\n");
  }
  __sendHead() {
    // node 口径：header 独立成 write；首块合并只发生在 _final/flushFinal 快捷路。
    // socket 已销毁即静默丢弃（incoming-pipelined 套件：管线中连接被毁后
    // 续行响应的写不抛，node 写毁 socket 回 false 口径）。
    if (this.__sock === null || this.__sock.destroyed) return;
    const head = this.__headBytes();
    if (head.length > 0) this.__sockWrite(head);
  }
  // chunked 终结块：`0\r\n` + trailer 行 + 空行（addTrailers 的 trailer 跟尾）。
  __chunkTerminator() {
    return "0\r\n" + (this.__trailer ?? "") + "\r\n";
  }
  // node _send 粒度（lib/_http_outgoing.js write_ 原文 + response-cork 套件
  // socket.write spy 恰 5 次）：chunked 帧四发——hex / CRLF / 体 / CRLF，
  // 每次 _send 独立 conn.write；CL/裸体一发（头 prepend 首块，见
  // __sendHeadWithFirst）。字节流恒等，write 调用次数与真机对齐。
  __frame(u8) {
    if (this.__noBody || this.__headOnly) return;
    if (u8.length === 0) return;
    if (this.__sock === null || this.__sock.destroyed) return;
    if (this.__chunked && !this.__rawCL) {
      // node _send 粒度：尺寸行 hex **不含 CRLF**（crlf_buf 独立一发）——
      // hex 带 CRLF 再发 __CRLF 即双 CRLF，整条 chunked 流错位（实锤坑）。
      this.__sockWrite(new TextEncoder().encode(u8.length.toString(16)));
      this.__sockWrite(__CRLF);
      this.__sockWrite(u8);
      this.__sockWrite(__CRLF);
    } else {
      this.__sockWrite(u8);
    }
  }
  // node _http_outgoing _send 内部面（test-http-1.0 套件直调 res._send('')）：
  // 头未发即发头（'' 调用 = 强制冲头，_headerSent 口径）；已发且 data 非空即
  // 直写（套件只用 ''；本仓帧化归 __frame，_send 不做 chunk 帧化）。
  _send(data) {
    const __d = data === undefined || data === null ? "" : String(data);
    // 先记账后落盘（_send 先于首 _write 时头尚未计数；已计数即只递减）。
    if (!this.__headCounted && !this.__headSent) {
      this.__headCounted = true;
      this.__wlen += this.__predictHeadLen();
    }
    if (!this.__headSent) {
      if (this.__sock !== null && !this.__sock.destroyed) {
        const head = this.__headBytes();
        if (head.length > 0) this.__sockWrite(head);
        if (__d.length > 0) this.__sockWrite(new TextEncoder().encode(__d));
      }
      return this;
    }
    if (__d.length > 0 && this.__sock !== null && !this.__sock.destroyed) {
      this.__sockWrite(new TextEncoder().encode(__d));
    }
    return this;
  }
  // 首块带头发（node _send 的 _header prepend 口径：头未发即拼进首个 _send
  //——chunked 拼 hex、CL/裸体拼体；空块/无体头独立一发）。
  __sendHeadWithFirst(b) {
    if (this.__sock === null || this.__sock.destroyed) return;
    const head = this.__headBytes();
    const __doChunk = this.__chunked && !this.__rawCL && !this.__noBody && !this.__headOnly;
    if (__doChunk && b !== null && b !== undefined && b.length > 0) {
      // hex 不含 CRLF（见 __frame 注）：head+hex / CRLF / 体 / CRLF。
      const hex = new TextEncoder().encode(b.length.toString(16));
      this.__sockWrite(head.length > 0 ? __concat(head, hex) : hex);
      this.__sockWrite(__CRLF);
      this.__sockWrite(b);
      this.__sockWrite(__CRLF);
    } else if (head.length > 0 && b !== null && b !== undefined && b.length > 0) {
      this.__sockWrite(__concat(head, b));
    } else if (head.length > 0) {
      this.__sockWrite(head);
    } else if (b !== null && b !== undefined && b.length > 0) {
      this.__frame(b);
    }
  }
  _write(chunk, encoding, cb) {
    const u8 = chunk instanceof Uint8Array ? chunk : __toU8(String(chunk));
    this.__sawWrite = true;
    // 管线停靠（drain-writable-length 套件）：socket 未就位的入列响应停靠
    // chunk，cb 暂扣（流机构自然等待，背压天然成立）；assignSocket 回放。
    // 独立构造（未入列）仍直落旧路（丢弃 + 回调，零行为差）。
    if (this.__sock === null && this.__queued === true && !this.destroyed) {
      (this.__parked ??= []).push([u8, cb]);
      return;
    }
    // 记账在 write/end 包装层同步完成（见上），此处不再计。
    // node 口径：首个 write 即算发头（setheaders-after-sent 套件 write 后
    // setHeader 即 HEADERS_SENT；holdback 只延迟落盘，旗同步立）。
    this.headersSent = true;
    if (this.__buf1 === null && !this.__headSent) {
      // 首字节 holdback：一拍内 end 到达且此前无 writeHead 则走 CL 快捷，
      // 否则转 chunked 流式（1.0 裸写）。cb 经 microtask 回（base 依此排
      // _final，抢在 timer(0) 前保 CL 快捷；defer 到落盘后会反让 timer 先赢，
      // 实测回退）。
      this.__buf1 = u8;
      this.__holdTimer = setTimeout(() => {
        this.__holdTimer = null;
        if (this.__buf1 !== null && !this.destroyed) {
          if (!this.__headSent) {
            if (this.__uced && this.__headers["content-length"] === undefined && this.__headers["transfer-encoding"] === undefined && !this.__frameSuppressed()) this.__chunked = true;
            const b = this.__buf1;
            this.__buf1 = null;
            this.__sendHeadWithFirst(b);
          } else {
            // _send('') 已冲头：滞留首块补帧（1.0 套件 write→_send('') 序）。
            const b = this.__buf1;
            this.__buf1 = null;
            this.__frame(b);
          }
        }
      }, 0);
      // node 口径：_write 完成异步回（socket 层 flush 节奏），同步回即
      // writableLength 即时清零、write 恒 true、背压永不触发
      // （outgoing-finish 系无限 while hang 根因）。
      queueMicrotask(cb);
      return;
    }
    if (this.__headSent && this.__buf1 !== null) {
      // _send('') 已冲头：滞留首块先行（保序——1.0 套件 write→_send('')→write 序）。
      const b0 = this.__buf1;
      this.__buf1 = null;
      this.__frame(b0);
    }
    if (!this.__headSent) {
      if (this.__uced && this.__headers["content-length"] === undefined && this.__headers["transfer-encoding"] === undefined && !this.__frameSuppressed()) this.__chunked = true;
      const b = this.__buf1;
      this.__buf1 = null;
      // node _header prepend：头未发即拼进首个 _send（头+hex 或 头+体一体）。
      this.__sendHeadWithFirst(b);
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
      // 入列停靠中：终结 parked（流等待 assign 回放，Node 管线口径；回放后重
      // 走本函数正常收尾，__onDone 照常轮转）。
      if (this.__sock === null && this.__queued === true && !this.destroyed) {
        this.__finalParked = cb;
        return;
      }
      // 无处可送：挂起计数清零（finish 口径 writableLength 恒 0；
      // socket 已死时 pending 同清，未落盘不再落盘）。
      this.__wlen = 0;
      try {
        if (this.__sock !== null && typeof this.__sock.__bwSub === "function") {
          this.__sock.__bwPend = 0;
        }
      } catch { /* 计数永不阻收尾 */ }
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
          this.__headers["transfer-encoding"] === undefined &&
          !this.__frameSuppressed()) {
        if (this.__uced && !this.__headStored && (!this.__sawWrite || this.__endHadData)) {
          this.__contentLength = this.__endHadData ? total : 0;
        }
      }
      const head = this.__headBytes();
      if (this.__sock !== null) {
        if (this.__buf1 !== null) {
          const b = this.__buf1;
          this.__buf1 = null;
          const __chunkedFrame = this.__chunked && !this.__rawCL && !this.__noBody && !this.__headOnly;
          if (__chunkedFrame && b.length > 0) {
            // chunked：头拼首帧 hex（node _send 的 _header prepend 口径）+
            // CRLF/体/CRLF + 终结块独立发——response-cork 套件 socket.write
            // spy 恰 5 次（头+hex/CRLF/体/CRLF/终结）。hex 不含 CRLF
            //（crlf_buf 独立一发，真机 _send 链口径）。
            const hex = new TextEncoder().encode(b.length.toString(16));
            this.__sockWrite(head.length > 0 ? __concat(head, hex) : hex);
            this.__sockWrite(__CRLF);
            this.__sockWrite(b);
            this.__sockWrite(__CRLF);
          } else if (head.length === 0) {
            this.__frame(b);
          } else if (!this.__noBody && !this.__headOnly && b.length > 0) {
            // node 口径：CL/raw 快捷时头 + 首块合并为一次 write（standalone 套件）。
            this.__sockWrite(__concat(head, b));
          } else {
            this.__sockWrite(head);
            this.__frame(b);
          }
          if (__chunkedFrame) {
            this.__sockWrite(new TextEncoder().encode(this.__chunkTerminator()));
          }
        } else if (head.length > 0) {
          // end() 无数据：node _send prepend——chunked 头拼终结块一发，
          // 非 chunked 头独立一发（真机 writeHead+end() 口径）。
          if (this.__chunked && !this.__rawCL && !this.__noBody && !this.__headOnly) {
            this.__sockWrite(__concat(head, new TextEncoder().encode(this.__chunkTerminator())));
          } else {
            this.__sockWrite(head);
          }
        }
      }
    } else if (this.__chunked && !this.__rawCL && !this.__noBody && !this.__headOnly) {
      this.__sockWrite(new TextEncoder().encode(this.__chunkTerminator()));
    }
    const cont = this.__onDone;
    this.__onDone = null;
    // _final 落盘全量同步完成（CL 快捷真值可能覆盖写时预测）：收尾计数恒清零，
    // 与 socket 实际落盘对齐（finish 口径 writableLength 恒 0；pending 预测
    // 偏差一并清零，base 持有全部实发）。
    this.__wlen = 0;
    try {
      if (this.__sock !== null && typeof this.__sock.__bwSub === "function") {
        this.__sock.__bwPend = 0;
      }
    } catch { /* 计数永不阻收尾 */ }
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
    // 销毁即无后续落盘：挂起计数/停靠 drain/停靠写/停靠终结全清
    // （destroy 不走 _final；停靠回调永不递送；socket pending 同清）。
    this.__wlen = 0;
    this.__parkedDrain = false;
    this.__parked = [];
    this.__finalParked = null;
    try {
      if (this.__sock !== null && typeof this.__sock.__bwSub === "function") {
        this.__sock.__bwPend = 0;
      }
    } catch { /* 计数永不阻销毁 */ }
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
