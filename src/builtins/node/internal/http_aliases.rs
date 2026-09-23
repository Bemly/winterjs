//! node 内部模块遗留别名（`_http_agent` / `_http_common` / `_http_server` /
//! `_http_outgoing`；node 套件直引 `require('_http_agent')` 形）。符号面以
//! node 26.8.2 实测逐键对拍（agent-keys/common-keys/server-keys/outgoing-keys）。
//! HTTPParser 深件（llhttp 事件面兼容类 + lenient 位旗）另案，暂不导出。

/// `node:internal/http`（真机 keys：kOutHeaders；套件直引 `internal/http`）。
/// kOutHeaders 与帧层同源（http_framing 导出同一符号）。
pub const HTTP_SOURCE: &str = r#"
import { kOutHeaders } from "node:internal/http_framing";
export { kOutHeaders };
"#;

/// `node:_http_agent`（真机 keys：Agent,globalAgent）。
pub const AGENT_SOURCE: &str = r#"
// node 口径（真机实测）：_http_agent.Agent 即 http.Agent 同一类、
// _http_agent.globalAgent 即 http.globalAgent 同一实例——帧层基类无
// __openSocket（flavor 子类提供），裸 Agent 建连即炸（agent-keepalive
// 套件），故整体重导出 http 侧具体类。
import { Agent, globalAgent } from "node:http";
export { Agent, globalAgent };
"#;

/// `node:_http_common`（真机 keys：_checkInvalidHeaderChar,_checkIsHttpToken,
/// chunkExpression,continueExpression,CRLF,freeParser,methods,parsers,
/// kIncomingMessage,HTTPParser,isLenient,calculateLenientFlags,prepareError,
/// kSkipPendingData）。HTTPParser 另案。
pub const COMMON_SOURCE: &str = r#"
import { METHODS, maxHeaderSize } from "node:internal/http_framing";
export { maxHeaderSize };
export const methods = METHODS;
// node lib/_http_common.js 头字符门（真机实测：\x01 invalid、空格/tab 合法、
// 高位 \x80-\xff 合法）。
const HEADER_CHAR_RE = /[^\t\x20-\x7e\x80-\xff]/;
// lenient 位（insecureHTTPParser；Fetch 规约口径，套件逐字符对拍）：只拒
// NUL/CR/LF 与 >0xff，其余控制字符放行。
const LENIENT_HEADER_CHAR_RE = /[\x00\r\n]|[^\x00-\xff]/;
export function _checkInvalidHeaderChar(val, lenient = false) {
  if (lenient === true) return LENIENT_HEADER_CHAR_RE.test(val);
  return HEADER_CHAR_RE.test(val);
}
const TOKEN_RE = /^[\^_`a-zA-Z\-0-9!#$%&'*+.|~]+$/;
export function _checkIsHttpToken(val) {
  return typeof val === "string" && TOKEN_RE.test(val);
}
export const chunkExpression = /^[^]*$/;
export const continueExpression = /^[^]*100[ \t]*(?:$|\n)/;
export const CRLF = "\r\n";
export const kIncomingMessage = Symbol("IncomingMessage");
export const kSkipPendingData = Symbol("kSkipPendingData");
// node 口径：parser 池（maxHTTPParserPool 面）。alloc 经 binding 表取当前
// HTTPParser（lazy-loaded 套件 monkey-patch binding.HTTPParser 后 alloc 即新类）。
export const parsers = {
  max: 1000,
  size: 0,
  alloc() {
    const __cls = (globalThis.__wjs_bindHttpParser !== undefined &&
      globalThis.__wjs_bindHttpParser !== null &&
      typeof globalThis.__wjs_bindHttpParser.HTTPParser === "function")
      ? globalThis.__wjs_bindHttpParser.HTTPParser
      : HTTPParser;
    return new __cls();
  },
  free() { return undefined; },
};
export function freeParser() { return undefined; }
export function prepareError() { return undefined; }

// HTTPParser 兼容类：llhttp 事件面纯 JS 实现（parser 系套件：test-http-parser.js
// 全 20 段 + max-header-pairs-cache + bad-ref + timeout-reset 的 execute 面）。
// 真机 26.8.2 逐键对拍的静态常量面见下（header-value-relaxed 套件以
// kLenientHeaderValueRelaxed 常量值门控 inbound 测试段）。
export class HTTPParser {
  constructor(kind) {
    this.kind = kind;
    // 品牌位（unbound execute 即 TypeError，parser.js 末段）。
    this.__brand = true;
    this.__buf = new Uint8Array(0);
    this.__headDone = false;
    this.__framing = null;
    this.__first = null;
    this.__headRaw = null;
    this.maxHeaderPairs = 2000;
  }
  // node 口径 initialize(type[, opts...])：重置解析态（kOn* 监听保留；
  // 多余实参忽略——reinit 形传 Buffer 照过）。
  initialize(type) {
    this.__ptype = type;
    this.__buf = new Uint8Array(0);
    this.__headDone = false;
    this.__framing = null;
    this.__first = null;
    this.__headRaw = null;
    return undefined;
  }
  // 解析执行：喂块 → 攒 carry → 逐消息泵（头/kOnHeaders/kOnHeadersComplete/
  // 体分块 kOnBody/终结 kOnMessageComplete）。恒返回消费字节数；回调抛错原样
  // 上抛（parser.js 'hello world' 段）；maxHeaderPairs 逐段现读（getter 抛错
  // 即透传，max-header-pairs-cache 套件）。
  execute(chunk, start, len) {
    if (this === undefined || this === null || this.__brand !== true) {
      throw new TypeError("Illegal invocation");
    }
    const __kOnExecute = this[HTTPParser.kOnExecute];
    if (typeof __kOnExecute === "function") {
      try { __kOnExecute.call(this); } catch (e) { throw e; }
    }
    let u8;
    if (chunk === undefined || chunk === null) u8 = new Uint8Array(0);
    else if (typeof chunk === "string") u8 = new TextEncoder().encode(chunk);
    else if (chunk instanceof Uint8Array) u8 = chunk.slice(start ?? 0, (start ?? 0) + (len ?? chunk.length));
    else if (chunk instanceof ArrayBuffer) u8 = new Uint8Array(chunk).slice(start ?? 0, (start ?? 0) + (len ?? chunk.byteLength));
    else u8 = new Uint8Array(0);
    const __consumed = len ?? u8.length;
    if (u8.length > 0) {
      const __nb = new Uint8Array(this.__buf.length + u8.length);
      __nb.set(this.__buf, 0); __nb.set(u8, this.__buf.length);
      this.__buf = __nb;
    }
    const __isResp = this.__ptype === HTTPParser.RESPONSE;
    for (;;) {
      if (!this.__headDone) {
        const __he = __hpFindHeadEnd(this.__buf);
        if (__he === -1) return __consumed;
        const __headText = new TextDecoder("latin1").decode(this.__buf.slice(0, __he));
        const __lines = __headText.split("\r\n");
        const __first = __lines.shift().split(" ");
        const __headers = [];
        // maxHeaderPairs 逐段现读（非正/非数即不限；getter 抛错透传）。
        const __mp = this.maxHeaderPairs;
        const __capped = typeof __mp === "number" && __mp > 0;
        let __pairs = 0;
        for (const __line of __lines) {
          if (__line === "") continue;
          const __c = __line.indexOf(":");
          if (__c <= 0) continue;
          if (__capped && __pairs >= __mp) break;
          __pairs++;
          __headers.push(__line.slice(0, __c).trim(), __line.slice(__c + 1).trim());
        }
        // 版本/方法/地址形态（parser.js 逐项断言）。
        let __vMaj = 1, __vMin = 1, __method, __url = "", __status = undefined, __msg = undefined;
        if (__isResp) {
          const __vv = (__first[0] ?? "").slice(5).split(".");
          __vMaj = Number(__vv[0] ?? 1) || 0; __vMin = Number(__vv[1] ?? 1) || 0;
          __status = Number(__first[1]); __msg = __first.length >= 3 ? __first.slice(2).join(" ") : "";
        } else {
          const __vv = (__first[2] ?? "HTTP/1.1").replace("HTTP/", "").split(".");
          __vMaj = Number(__vv[0] ?? 1) || 0; __vMin = Number(__vv[1] ?? 1) || 0;
          const __mi = METHODS.indexOf(__first[0]);
          __method = __mi >= 0 ? __mi : undefined;
          __url = __first[1] ?? "";
        }
        const __kOnH = this[HTTPParser.kOnHeaders];
        if (typeof __kOnH === "function") __kOnH.call(this, __headers.slice(), __url);
        const __kOnHC = this[HTTPParser.kOnHeadersComplete];
        if (typeof __kOnHC === "function") {
          if (__isResp) __kOnHC.call(this, __vMaj, __vMin, __headers.slice(), undefined, __url, __status, __msg);
          else __kOnHC.call(this, __vMaj, __vMin, __headers.slice(), __method, __url);
        }
        this.__first = __first;
        this.__headRaw = __headers;
        // 体帧（CL/Transfer-Encoding: chunked/无体）。
        const __hm = {};
        for (let __i = 0; __i < __headers.length; __i += 2) __hm[__headers[__i].toLowerCase()] = __headers[__i + 1];
        const __te = (__hm["transfer-encoding"] ?? "").toLowerCase();
        if (__te.split(",").map((s) => s.trim()).includes("chunked")) {
          this.__framing = { type: "chunked", sizeLine: "", need: 0, phase: "size" };
        } else if (__hm["content-length"] !== undefined) {
          const __n = Number(__hm["content-length"]);
          this.__framing = { type: "cl", remaining: Number.isFinite(__n) && __n >= 0 ? __n : 0 };
        } else {
          this.__framing = { type: "none" };
        }
        this.__buf = this.__buf.slice(__he + 4);
        this.__headDone = true;
        if (this.__framing.type === "none") {
          const __kOnMC = this[HTTPParser.kOnMessageComplete];
          if (typeof __kOnMC === "function") __kOnMC.call(this);
          this.__headDone = false;
          this.__framing = null;
          continue;
        }
      }
      // 体泵（CL 整块 / chunked 增量；trailer 行走 kOnHeaders）。
      const __fr = this.__framing;
      const __kOnB = this[HTTPParser.kOnBody];
      if (__fr.type === "cl") {
        if (this.__buf.length < __fr.remaining) return __consumed;
        const __body = this.__buf.slice(0, __fr.remaining);
        this.__buf = this.__buf.slice(__fr.remaining);
        __fr.remaining = 0;
        if (__body.length > 0 && typeof __kOnB === "function") {
          __kOnB.call(this, globalThis.Buffer.from(__body));
        }
        const __kOnMC = this[HTTPParser.kOnMessageComplete];
        if (typeof __kOnMC === "function") __kOnMC.call(this);
        this.__headDone = false;
        this.__framing = null;
        continue;
      }
      // chunked 增量机（尺寸行/块/终结/trailer；trailer 累积挂 framing，
      // 跨 execute 存活）。
      let __progress = false;
      for (;;) {
        if (__fr.phase === "size") {
          const __nl = __hpFindLine(this.__buf);
          if (__nl === -1) break;
          const __line = new TextDecoder("latin1").decode(this.__buf.slice(0, __nl));
          this.__buf = this.__buf.slice(__nl + 2);
          __progress = true;
          const __semi = __line.indexOf(";");
          const __hex = (__semi === -1 ? __line : __line.slice(0, __semi)).trim();
          __fr.need = parseInt(__hex, 16);
          if (!Number.isFinite(__fr.need) || __fr.need < 0) __fr.need = 0;
          if (__fr.need === 0) { __fr.phase = "trailer"; continue; }
          __fr.phase = "data";
          continue;
        }
        if (__fr.phase === "data") {
          if (this.__buf.length < __fr.need + 2) break;
          const __body = this.__buf.slice(0, __fr.need);
          this.__buf = this.__buf.slice(__fr.need + 2);
          __fr.phase = "size";
          __progress = true;
          if (__body.length > 0 && typeof __kOnB === "function") {
            __kOnB.call(this, globalThis.Buffer.from(__body));
          }
          continue;
        }
        // trailer 行：空行即终结（累积行一次 kOnHeaders 后落完成）。
        const __nl = __hpFindLine(this.__buf);
        if (__nl === -1) break;
        const __line = new TextDecoder("latin1").decode(this.__buf.slice(0, __nl));
        this.__buf = this.__buf.slice(__nl + 2);
        __progress = true;
        if (__line === "") {
          if (__fr.trailerAcc !== null && __fr.trailerAcc !== undefined) {
            const __kOnH = this[HTTPParser.kOnHeaders];
            if (typeof __kOnH === "function") {
              __kOnH.call(this, __fr.trailerAcc, this.__first[1] ?? "");
            }
            __fr.trailerAcc = null;
          }
          const __kOnMC = this[HTTPParser.kOnMessageComplete];
          if (typeof __kOnMC === "function") __kOnMC.call(this);
          this.__headDone = false;
          this.__framing = null;
          break;
        }
        const __c = __line.indexOf(":");
        if (__c > 0) {
          // trailer 段首行即读一次上限（maxHeaderPairs 逐段现读）。
          if (__fr.trailerAcc === null || __fr.trailerAcc === undefined) {
            __fr.trailerAcc = [];
            void this.maxHeaderPairs;
          }
          __fr.trailerAcc.push(__line.slice(0, __c).trim(), __line.slice(__c + 1).trim());
        }
        continue;
      }
      if (!__progress) return __consumed;
    }
  }
  // 收尾（bad-ref 套件：残段落定；幂等，无事即空转）。
  finish() {
    if (this === undefined || this === null || this.__brand !== true) {
      throw new TypeError("Illegal invocation");
    }
    return undefined;
  }
  // 绑定传输句柄读数（timeout-reset 套件：consume(socket._handle) 后到包即泵）。
  // 句柄→socket 经全局弱表回查（net_socket 侧登记），无表即空转。
  consume(handle) {
    if (this === undefined || this === null || this.__brand !== true) {
      throw new TypeError("Illegal invocation");
    }
    try {
      const __reg = globalThis.__wjs_sockByHandle;
      const __sock = __reg !== undefined && __reg !== null ? __reg.get(handle) : undefined;
      if (__sock !== undefined && __sock !== null && typeof __sock.on === "function") {
        const __self = this;
        __sock.on("data", function __parserConsumeFeed(chunk) {
          try {
            const __u = chunk instanceof Uint8Array ? chunk
              : (chunk instanceof ArrayBuffer ? new Uint8Array(chunk) : new TextEncoder().encode(String(chunk)));
            __self.execute(__u, 0, __u.length);
          } catch { /* 泵错不阻传输 */ }
        });
      }
    } catch { /* 无表即空转 */ }
    return undefined;
  }
  close() { return undefined; }
  remove() { return undefined; }
}
// 头终结扫描（\r\n\r\n；与帧层 __findHeadEnd 同口径）。
function __hpFindHeadEnd(u8) {
  for (let i = 0; i + 3 < u8.length; i++) {
    if (u8[i] === 13 && u8[i + 1] === 10 && u8[i + 2] === 13 && u8[i + 3] === 10) return i;
  }
  return -1;
}
// 单行终结扫描（\r\n）。
function __hpFindLine(u8) {
  for (let i = 0; i + 1 < u8.length; i++) {
    if (u8[i] === 13 && u8[i + 1] === 10) return i;
  }
  return -1;
}
HTTPParser.REQUEST = 1;
HTTPParser.RESPONSE = 2;
HTTPParser.kOnMessageBegin = 0;
HTTPParser.kOnHeaders = 1;
HTTPParser.kOnHeadersComplete = 2;
HTTPParser.kOnBody = 3;
HTTPParser.kOnMessageComplete = 4;
HTTPParser.kOnExecute = 5;
HTTPParser.kOnTimeout = 6;
HTTPParser.kLenientNone = 0;
HTTPParser.kLenientHeaders = 1;
HTTPParser.kLenientChunkedLength = 2;
HTTPParser.kLenientKeepAlive = 4;
HTTPParser.kLenientTransferEncoding = 8;
HTTPParser.kLenientVersion = 16;
HTTPParser.kLenientDataAfterClose = 32;
HTTPParser.kLenientOptionalLFAfterCR = 64;
HTTPParser.kLenientOptionalCRLFAfterChunk = 128;
HTTPParser.kLenientOptionalCRBeforeLF = 256;
HTTPParser.kLenientSpacesAfterChunkSize = 512;
HTTPParser.kLenientHeaderValueRelaxed = 1024;
HTTPParser.kLenientAll = 1023;
// node isLenient(parser)（返回该 parser 的 lenient 位；本仓无 llhttp parser
// 实例面，恒 strict 0）。
export function isLenient() { return 0; }
// node calculateLenientFlags(httpValidation, insecureHTTPParser)（真机实测：
// ('relaxed')→1024、(true,true)→1023、(false,false)→0）。
export function calculateLenientFlags(httpValidation, insecureHTTPParser) {
  if (httpValidation === "insecure" || insecureHTTPParser === true) return HTTPParser.kLenientAll;
  if (httpValidation === "relaxed") return HTTPParser.kLenientHeaderValueRelaxed;
  return HTTPParser.kLenientNone;
}
"#;

/// `node:_http_server`（真机 keys：STATUS_CODES,Server,ServerResponse,
/// setupConnectionsTracking,storeHTTPOptions,_connectionListener,
/// kServerResponse,httpServerPreClose,kConnectionsCheckingInterval）。
/// Server/_connectionListener 为组合体（withHttpServer(net.Server)），不导出。
pub const SERVER_SOURCE: &str = r#"
import { STATUS_CODES } from "node:internal/http_framing";
import { kConnectionsCheckingInterval, kServerResponse } from "node:internal/http_framing";
export { STATUS_CODES, kConnectionsCheckingInterval, kServerResponse };
export function setupConnectionsTracking() { return undefined; }
export function storeHTTPOptions() { return undefined; }
export function httpServerPreClose() { return undefined; }
"#;

/// `node:_http_outgoing`（真机 keys：kHighWaterMark,kUniqueHeaders,
/// parseUniqueHeadersOption,validateHeaderName,validateHeaderValue,
/// OutgoingMessage）。
pub const OUTGOING_SOURCE: &str = r#"
import { OutgoingMessage } from "node:internal/http_framing";
import { kHighWaterMark } from "node:internal/http_framing";
export { OutgoingMessage, kHighWaterMark };
export const kUniqueHeaders = Symbol("uniqueHeaders");
// node lib/_http_outgoing.js validateHeaderName/Value（message 逐字）。
const TOKEN_RE = /^[\^_`a-zA-Z\-0-9!#$%&'*+.|~]+$/;
export function validateHeaderName(name) {
  if (typeof name !== "string" || !TOKEN_RE.test(name)) {
    const e = new TypeError(`Invalid character in header name ["${String(name)}"]`);
    e.code = "ERR_INVALID_HTTP_TOKEN"; throw e;
  }
}
export function validateHeaderValue(name, value) {
  if (value === undefined) {
    const e = new TypeError(`Invalid value in header set for "${String(name)}"`);
    e.code = "ERR_HTTP_INVALID_CHAR"; throw e;
  }
  if (/[\r\n\u0000]/.test(String(value))) {
    const e = new TypeError(`Invalid character in header content ["${String(name)}"]`);
    e.code = "ERR_INVALID_CHAR"; throw e;
  }
}
export function parseUniqueHeadersOption(headers) {
  if (headers === null) return null;
  if (typeof headers !== "object") {
    const e = new TypeError("Option \"uniqueHeaders\" must be one of type array, object, or null. Received " + typeof headers);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  if (!Array.isArray(headers)) return new Set([String(headers)]);
  return new Set(headers.map(String));
}
"#;
