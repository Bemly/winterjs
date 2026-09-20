//! node 内部模块遗留别名（`_http_agent` / `_http_common` / `_http_server` /
//! `_http_outgoing`；node 套件直引 `require('_http_agent')` 形）。符号面以
//! node 26.8.2 实测逐键对拍（agent-keys/common-keys/server-keys/outgoing-keys）。
//! HTTPParser 深件（llhttp 事件面兼容类 + lenient 位旗）另案，暂不导出。

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
// node 口径：parser 池（maxHTTPParserPool 面；池化不适用本仓——对象只供
// 套件断言默认值）。
export const parsers = { max: 1000, size: 0 };
export function freeParser() { return undefined; }
export function prepareError() { return undefined; }

// HTTPParser 兼容骨架：llhttp 事件面（execute/kOnHeadersComplete 族）本仓为
// JS 解析器实现（http_framing __feed），不在此接线——另案记档。此处导出真机
// 26.8.2 逐键对拍的静态常量面（header-value-relaxed 套件以
// kLenientHeaderValueRelaxed 常量值门控 inbound 测试段）。
export class HTTPParser {
  constructor(kind) { this.kind = kind; }
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
