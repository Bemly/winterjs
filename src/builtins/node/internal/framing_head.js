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
// node 口径默认头限动态值（max-header-size 套件）：CLI 兼容旗
// `--max-http-header-size=N`（=值/空格两形，见 cli::strip_node_compat_args）
// 覆写缺省；非法值回落 16384。http.maxHeaderSize 经 __api getter 同源。
export function __defaultMaxHeaderSize() {
  try {
    const __compat = globalThis.__wjs_nodeCompat;
    if (Array.isArray(__compat)) {
      for (let __i = 0; __i < __compat.length; __i++) {
        const __t = String(__compat[__i]);
        let __v = null;
        // "--max-http-header-size=" 长 23（2 横杠 + 20 名 + 1 等号）。
        if (__t.startsWith("--max-http-header-size=")) __v = __t.slice(23);
        else if (__t === "--max-http-header-size" && __i + 1 < __compat.length) __v = String(__compat[__i + 1]);
        if (__v !== null) {
          const __n = Number(__v);
          if (Number.isFinite(__n) && __n >= 0) return __n;
          return maxHeaderSize;
        }
      }
    }
  } catch { /* 环境不可读即缺省 */ }
  return maxHeaderSize;
}

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
// latin1 编码（真机 write latin1 口径）：头串逐 charCode 取低 8 位——
// 非 ASCII 头值（'binary' 形）按 latin1 字节上网，非 UTF-8（non-utf8-header 套件）。
function __latin1Bytes(str) {
  const out = new Uint8Array(str.length);
  for (let i = 0; i < str.length; i++) out[i] = str.charCodeAt(i) & 0xff;
  return out;
}
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
// socket 写确认等待（writable-finished 套件）：mock Duplex 的写确认经微任务
// 到（_write 异步一跳），同步读 box 恒空——实锤坑。故落盘收尾一律经此等全部
// ack（pend 计数）：同步全回即下一拍，异步 mock 等其确认；确认永不到即
// socket 违约（node 同款挂起）。三文件（head/outgoing/agent）concat 同域共用。
// box 形：{ err, pend, waiters }，由各类的 __sockCap 造。
function __afterSockFlush(stream, box, fn) {
  queueMicrotask(() => {
    if (stream.destroyed) return;
    if (box === null || box === undefined || box.pend === 0) {
      try { fn(box !== null && box !== undefined && box.err !== null && box.err !== undefined ? box.err : undefined); } catch {}
      return;
    }
    box.waiters.push((err) => {
      if (stream.destroyed) return;
      try { fn(err ?? undefined); } catch {}
    });
  });
}
// node _http_incoming.js matchKnownFields 无前缀单值表（重复头首个赢；
// content-length 亦单值——重 CL 正常面由 HPE 门拒，lenient 下首个赢）。
const __SINGLETON_HEADERS = new Set([
  "age", "authorization", "content-encoding", "content-length", "content-type",
  "etag", "expires", "from", "host", "if-modified-since", "if-unmodified-since",
  "last-modified", "location", "max-forwards", "proxy-authorization",
  "referer", "retry-after", "server", "user-agent", "x-forwarded-host",
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
  // 重复 Transfer-Encoding 行计数（Test 16：relaxed/strict 下 llhttp 拒收，
  // 仅 lenient 放行；insecure 模式跳过此门）。
  let __teLines = 0;
  for (const line of lines) {
    if (line === "") continue;
    const c = line.indexOf(":");
    if (c <= 0) throw __mkParseError("malformed header line");
    // RFC 7230 §3.2.4 / node llhttp 口径：冒号前空格即 HPE 拒收（走私向量；
    // request-smuggling-content-length 套件 'Content-Length : 5'——lenient
    // （insecure）维持旧行为放行）。
    if (mode !== "lenient" && /[ \t]/.test(line.slice(0, c))) {
      throw __mkParseError("invalid header field (space before colon)");
    }
    const k = line.slice(0, c).trim();
    const vRaw = line.slice(c + 1);
    const v = vRaw.trim();
    if (k.toLowerCase() === "transfer-encoding") {
      __teLines++;
      // llhttp 口径（Test 16）：重复 TE 行在 strict/relaxed 下即拒，
      // 仅 lenient（insecure）放行合并。
      if (__teLines > 1 && mode !== "lenient") {
        throw __mkParseError("duplicate Transfer-Encoding");
      }
    }
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
    } else if (joinDup === true || !__SINGLETON_HEADERS.has(lk)) {
      // node _addHeaderLine 口径：joinable 表 + 未知头缺省恒 ', ' 合并
      //（matchKnownFields flag \u0000；multiheaders2 套件 multipleAllowed
      // 全 join）；joinDuplicateHeaders:true 压过单值表（join-authorization
      // 套件 '1, 2'）。
      headers[lk] = `${headers[lk]}, ${v}`;
    }
    // else：单值表首个赢，后续丢弃（rawHeaders 照收）。
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
// __parseErr 旗供客户端 __sockOnData 区分解析错（→ req destroy+error）与用户
// 回调 throw（→ 重抛 uncaught；10b 套件严格响应头错即走前者）。
function __mkParseError(msg) {
  const e = new Error(msg ?? "parse error");
  e.__httpParse = true;
  e.__parseErr = true;
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
  // 再判 chunked；字符串直判。chunked 必须是**整词 token**（逗号切分逐段
  // 全等比对）——'chunkedchunked' 走私形不得命中（te-repeated-chunked 套件）。
  const __tev = headers["transfer-encoding"];
  const te = (Array.isArray(__tev) ? __tev.join(", ") : (__tev || "")).toLowerCase();
  const __teChunked = te.split(",").some((t) => t.trim() === "chunked");
  if (__teChunked) return { type: "chunked", need: -1, buf: new Uint8Array(0) };
  const cl = Number(headers["content-length"] ?? NaN);
  if (Number.isInteger(cl) && cl >= 0) return { type: "cl", remaining: cl };
  if (isResponse) return { type: "close" };
  // TE 在场但非 chunked（无 CL）：请求已派发但体不可帧化——data/end 永不发，
  // 余字节按下一请求解析、垃圾即 HPE → 400 + close（真机 te-repeated-chunked
  // 口径：handler mustCall×1 + 客户端见 400）。
  if (te !== "") return { type: "teInvalid" };
  return { type: "none" };
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
  // node 口径 req.signal（request-signal 套件）：AbortSignal，连接早夭
  // （响应未完）即 abort，正常收齐永不 abort（真机探针：end+10ms 与 close
  // 后皆 false）。惰性创建，__abortReq/socket-close 处触发（见 outgoing）。
  get signal() {
    if (this.__abortController === null || this.__abortController === undefined) {
      try { this.__abortController = new AbortController(); } catch { return undefined; }
      // 早夭先于首次访问：补 abort（真机：先死后读仍 aborted）。
      if (this.__signalAborted === true) {
        try { this.__abortController.abort(); } catch {}
      }
    }
    return this.__abortController.signal;
  }
  // node 口径：消息销毁的 socket/signal 联动——
  // ① socket：带错必杀；无错仅未收齐（incomplete）且无属主请求接管时才杀。
  //    属主 ClientRequest._destroy 级联 res.destroy() 时不杀（socket 归属
  //    req 的池化/清理逻辑；杀了即 listeners-leak 回退）。其余无错销毁
  //    （如 pipe 形客户端 res.destroy()）由 res-close 闸门经 __finishSock
  //    收尾（destroyed 即销），此处不动。
  // ② signal：带错或未收齐即 abort（request-signal Test3/5；正常收齐的自动
  //    destroy 不动——Test2）。errored 照记（destroy 侧 checkError 先行）；
  //    本体 error 吞掉（cb() 无错，基类不排 error 发射）。
  _destroy(err, cb) {
    const __hasErr = err !== undefined && err !== null;
    const __incomplete = this.readableEnded !== true;
    if (__hasErr || __incomplete) {
      try {
        if (this.__abortController !== null && this.__abortController !== undefined) {
          try { this.__abortController.abort(); } catch {}
        } else {
          this.__signalAborted = true;
        }
      } catch { /* signal 永不阻收尾 */ }
    }
    if (__hasErr) {
      const __s = this.socket;
      if (__s !== null && __s !== undefined && !__s.destroyed && typeof __s.destroy === "function") {
        try { __s.destroy(err); } catch { /* closed meanwhile */ }
      }
    }
    cb();
  }
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
  // node lib/_http_incoming.js 逐字（optimize-empty-requests 套件）：快路径
  // 收口——跳过流生命周期，五个状态位全置位；readableEnded/destroyed 即 true，
  // 后挂 data/end 监听永不触发。
  _dumpAndCloseReadable() {
    this._dumped = true;
    const st = this._readableState;
    if (st !== undefined && st !== null) {
      st.ended = true;
      st.endEmitted = true;
      st.destroyed = true;
      st.closed = true;
      st.closeEmitted = true;
    }
  }
  // node lib/_http_incoming.js 口径：转发 socket 空闲计时（'timeout' 由 socket
  // 发出；cb 注册为 once 监听）。
  setTimeout(msecs, callback) {
    if (typeof callback === "function") this.once("timeout", callback);
    if (this.socket !== undefined && this.socket !== null) {
      // node responseOnTimeout 口径的 res 侧桥：res.setTimeout 后置武装
      //（attach 期 socket.timeout 尚 0 的形，client-response-timeout 套件）；
      // complete 即哑（≈真机 end 摘监听），res close 即摘。
      if (!this.__resTimeoutFwd) {
        this.__resTimeoutFwd = true;
        const __fwd = () => {
          if (this.complete) return;
          // node socketOnTimeout（_http_server 903 行）口径：timeout 事件带
          // socket 实参（set-timeout-server 套件 cb(socket) → socket.destroy()）。
          this.emit("timeout", this.socket);
        };
        try { this.socket.on("timeout", __fwd); } catch { /* gone */ }
        this.once("close", () => {
          try { this.socket.removeListener("timeout", __fwd); } catch { /* gone */ }
        });
      }
      this.socket.setTimeout(msecs);
    }
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

