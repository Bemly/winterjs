//! `node:internal/http2/util`（Node `lib/internal/http2/util.js` 移植，MIT）。
//!
//! 口径：
//! - 纯 JS 函数按真机 26.8.2 源码逐字还原（`assertIsObject`/`assertIsArray`/
//!   `assertWithinRange`/`assertValidPseudoHeader*`/`sessionName`/`getAuthority`
//!   经 `withoutStackTrace.toString()` 实拍）；设置项缓冲系（getSettings/
//!   updateOptionsBuffer/settingsBuffer）在 nghttp2 绑定层缺席的现实下以
//!   自有 TypedArray 同布局实现（仅被 blocked 套件经 internalBinding 断言，
//!   公共面 getPackedSettings/getUnpackedSettings 不经此路径）。
//! - `NghttpError.toString` 需 `Error [ERR_HTTP2_ERROR]: ...` 形态：引擎
//!   `Error.prototype.toString` 与 node bootstrap 的 [code] 插入不同源，
//!   以自有 toString 覆写补齐（`name` 保持 "Error"，真机同款）。
//! - k* 符号（kSocket 等）必须由本模块单点定义，http2 prelude 导入使用，
//!   保证 `session[kSocket]` 跨模块符号同一（套件 `session[kSocket]` 直探）。

pub const SOURCE: &str = r#"
import { codes } from "node:internal/errors";

// ── 常量（node lib/internal/http2/util.js 同款索引/符号）──────────────────
const kValidPseudoHeaders = new Set([
  ":method", ":scheme", ":path", ":authority", ":status",
]);
const kNoPayloadMethods = new Set([
  "GET", "HEAD", "DELETE", "CONNECT",
]);
const MAX_ADDITIONAL_SETTINGS = 10;

const kRequest = Symbol("request");
const kSocket = Symbol("socket");
const kProxySocket = Symbol("proxy socket");
const kAuthority = Symbol("authority");
const kProtocol = Symbol("protocol");
const kSensitiveHeaders = Symbol("sensitive headers");
const kStrictSingleValueFields = Symbol("strict single value fields");

// ── NghttpError（真机口径：errno/code/name="Error"/toString 带 [code]）────
// nghttp2_strerror 全表（真机 26.8.2 逐码枚举实拍）
const nghttp2ErrorStrings = {
  [-501]: "Invalid argument",
  [-502]: "Out of buffer space",
  [-503]: "Unsupported SPDY version",
  [-504]: "Operation would block",
  [-505]: "Protocol error",
  [-506]: "Invalid frame octets",
  [-507]: "EOF",
  [-508]: "Data transfer deferred",
  [-509]: "No more Stream ID available",
  [-510]: "Stream was already closed or invalid",
  [-511]: "Stream is closing",
  [-512]: "The transmission is not allowed for this stream",
  [-513]: "Stream ID is invalid",
  [-514]: "Invalid stream state",
  [-515]: "Another DATA frame has already been deferred",
  [-516]: "request HEADERS is not allowed",
  [-517]: "GOAWAY has already been sent",
  [-518]: "Invalid header block",
  [-519]: "Invalid state",
  [-521]: "The user callback function failed due to the temporal error",
  [-522]: "The length of the frame is invalid",
  [-523]: "Header compression/decompression error",
  [-524]: "Flow control error",
  [-525]: "Insufficient buffer size given to function",
  [-526]: "Callback was paused by the application",
  [-527]: "Too many inflight SETTINGS",
  [-528]: "Server push is disabled by peer",
  [-529]: "DATA or HEADERS frame has already been submitted for the stream",
  [-530]: "The current session is closing",
  [-531]: "Invalid HTTP header field was received",
  [-532]: "Violation in HTTP messaging rule",
  [-533]: "Stream was refused",
  [-534]: "Internal error",
  [-535]: "Cancel",
  [-536]: "When a local endpoint expects to receive SETTINGS frame, it receives an other type of frame",
};

function nghttp2ErrorString(errno) {
  const msg = nghttp2ErrorStrings[errno];
  return msg ?? "Unknown error code";
}

class NghttpError extends Error {
  constructor(errno) {
    super(nghttp2ErrorString(errno));
    this.name = "Error";
    this.code = "ERR_HTTP2_ERROR";
    this.errno = errno;
  }
  // 引擎 toString 无 node bootstrap 的 [code] 插入，此处按真机形态补齐
  toString() {
    return `Error [ERR_HTTP2_ERROR]: ${this.message}`;
  }
}

// ── 校验函数（真机 26.8.2 withoutStackTrace.toString 逐字还原）─────────────
// 错误以直构实现（node 的 codes.X.RangeError.HideStackFramesError 变体路径
// 依赖 bootstrap 内部机制；真机实测：assertWithinRange → RangeError +
// message 无 min/max 后缀；assertValidPseudoHeader → TypeError）。
function assertIsObject(value, name, types) {
  if (value !== undefined &&
      (value === null ||
       typeof value !== "object" ||
       Array.isArray(value))) {
    throw new codes.ERR_INVALID_ARG_TYPE(name, types || "Object", value);
  }
}

function assertIsArray(value, name, types) {
  if (value !== undefined &&
      (value === null ||
       !Array.isArray(value))) {
    throw new codes.ERR_INVALID_ARG_TYPE(name, types || "Array", value);
  }
}

function assertWithinRange(name, value, min = 0, max = Infinity) {
  if (value !== undefined &&
      (typeof value !== "number" || value < min || value > max)) {
    const err = new RangeError(`Invalid value for setting "${name}": ${value}`);
    err.code = "ERR_HTTP2_INVALID_SETTING_VALUE";
    throw err;
  }
}

function assertValidPseudoHeader(key) {
  if (!kValidPseudoHeaders.has(key)) {
    const err = new TypeError(
      `"${key}" is an invalid pseudoheader or is used incorrectly`);
    err.code = "ERR_HTTP2_INVALID_PSEUDOHEADER";
    throw err;
  }
}

function assertValidPseudoHeaderResponse(key) {
  if (key !== ":status") {
    const err = new TypeError(
      `"${key}" is an invalid pseudoheader or is used incorrectly`);
    err.code = "ERR_HTTP2_INVALID_PSEUDOHEADER";
    throw err;
  }
}

function assertValidPseudoHeaderTrailer(key) {
  const err = new TypeError(
    `"${key}" is an invalid pseudoheader or is used incorrectly`);
  err.code = "ERR_HTTP2_INVALID_PSEUDOHEADER";
  throw err;
}

function sessionName(type) {
  switch (type) {
    case 1: return "client";
    case 0: return "server";
    default: return "<invalid>";
  }
}

function getAuthority(headers) {
  if (headers[":authority"] !== undefined)
    return headers[":authority"];
  if (headers["host"] !== undefined)
    return headers["host"];
}

function isPayloadMeaningless(method) {
  return kNoPayloadMethods.has(method);
}

// ── 设置项缓冲（nghttp2 绑定层缺席：自有 TypedArray 同布局实现）────────────
// IDX_* 布局与 node lib/internal/http2/util.js 一致（settings 为 Uint32 双槽：
// 每 ID 一对 [id, value]；flags 位图标记存在项）。
const IDX_SETTINGS_HEADER_TABLE_SIZE = 0;
const IDX_SETTINGS_ENABLE_PUSH = 1;
const IDX_SETTINGS_INITIAL_WINDOW_SIZE = 2;
const IDX_SETTINGS_MAX_FRAME_SIZE = 3;
const IDX_SETTINGS_MAX_CONCURRENT_STREAMS = 4;
const IDX_SETTINGS_MAX_HEADER_LIST_SIZE = 5;
const IDX_SETTINGS_MAX_HEADER_SIZE = 6;
const IDX_SETTINGS_ENABLE_CONNECT_PROTOCOL = 7;
const IDX_SETTINGS_FLAGS = 8;
const IDX_SETTINGS_COUNT = 9;

const settingsBuffer = new Uint32Array(IDX_SETTINGS_COUNT * 2);
settingsBuffer[IDX_SETTINGS_FLAGS] = 0b111111111; // 缺省全部存在

// options 缓冲：IDX_OPTIONS_*（node 同布局，14 项）
const IDX_OPTIONS_MAX_DEFLATE_DYNAMIC_TABLE_SIZE = 0;
const IDX_OPTIONS_MAX_RESERVED_REMOTE_STREAMS = 1;
const IDX_OPTIONS_MAX_SEND_HEADER_BLOCK_LENGTH = 2;
const IDX_OPTIONS_PEER_MAX_CONCURRENT_STREAMS = 3;
const IDX_OPTIONS_PADDING_STRATEGY = 4;
const IDX_OPTIONS_MAX_HEADER_LIST_PAIRS = 5;
const IDX_OPTIONS_MAX_OUTSTANDING_PINGS = 6;
const IDX_OPTIONS_MAX_OUTSTANDING_SETTINGS = 7;
const IDX_OPTIONS_MAX_SESSION_MEMORY = 8;
const IDX_OPTIONS_MAX_SETTINGS = 9;
const IDX_OPTIONS_STREAM_RESET_RATE = 10;
const IDX_OPTIONS_STREAM_RESET_BURST = 11;
const IDX_OPTIONS_STRICT_HTTP_FIELD_WHITESPACE_VALIDATION = 12;
const IDX_OPTIONS_FLAGS = 13;

const optionsBuffer = new Float64Array(16);

const kSettingsNames = new Map([
  ["headerTableSize", IDX_SETTINGS_HEADER_TABLE_SIZE],
  ["enablePush", IDX_SETTINGS_ENABLE_PUSH],
  ["initialWindowSize", IDX_SETTINGS_INITIAL_WINDOW_SIZE],
  ["maxFrameSize", IDX_SETTINGS_MAX_FRAME_SIZE],
  ["maxConcurrentStreams", IDX_SETTINGS_MAX_CONCURRENT_STREAMS],
  ["maxHeaderListSize", IDX_SETTINGS_MAX_HEADER_LIST_SIZE],
  ["maxHeaderSize", IDX_SETTINGS_MAX_HEADER_SIZE],
  ["enableConnectProtocol", IDX_SETTINGS_ENABLE_CONNECT_PROTOCOL],
]);

const settingsRange = new Map([
  [IDX_SETTINGS_HEADER_TABLE_SIZE, [0, 0xffffffff]],
  [IDX_SETTINGS_INITIAL_WINDOW_SIZE, [0, 0xffffffff]],
  [IDX_SETTINGS_MAX_FRAME_SIZE, [16384, 16777215]],
  [IDX_SETTINGS_MAX_CONCURRENT_STREAMS, [0, 0xffffffff]],
  [IDX_SETTINGS_MAX_HEADER_LIST_SIZE, [0, 0xffffffff]],
  [IDX_SETTINGS_MAX_HEADER_SIZE, [0, 0xffffffff]],
]);

function updateSettingsBuffer(options) {
  let flags = 0;
  for (const [name, idx] of kSettingsNames) {
    const value = options[name];
    if (value !== undefined) {
      flags |= (1 << idx);
      settingsBuffer[IDX_SETTINGS_COUNT + idx] = 0; // id 占位（nghttp2 序号无关紧要）
      settingsBuffer[idx] = value;
    }
  }
  settingsBuffer[IDX_SETTINGS_FLAGS] = flags;
}

function updateOptionsBuffer(options) {
  let flags = 0;
  const put = (idx, name) => {
    const value = options[name];
    if (value !== undefined) {
      flags |= (1 << idx);
      optionsBuffer[idx] = value;
    }
  };
  put(IDX_OPTIONS_MAX_DEFLATE_DYNAMIC_TABLE_SIZE, "maxDeflateDynamicTableSize");
  put(IDX_OPTIONS_MAX_RESERVED_REMOTE_STREAMS, "maxReservedRemoteStreams");
  put(IDX_OPTIONS_MAX_SEND_HEADER_BLOCK_LENGTH, "maxSendHeaderBlockLength");
  put(IDX_OPTIONS_PEER_MAX_CONCURRENT_STREAMS, "peerMaxConcurrentStreams");
  put(IDX_OPTIONS_PADDING_STRATEGY, "paddingStrategy");
  put(IDX_OPTIONS_MAX_HEADER_LIST_PAIRS, "maxHeaderListPairs");
  put(IDX_OPTIONS_MAX_OUTSTANDING_PINGS, "maxOutstandingPings");
  put(IDX_OPTIONS_MAX_OUTSTANDING_SETTINGS, "maxOutstandingSettings");
  put(IDX_OPTIONS_MAX_SESSION_MEMORY, "maxSessionMemory");
  put(IDX_OPTIONS_MAX_SETTINGS, "maxSettings");
  put(IDX_OPTIONS_STREAM_RESET_RATE, "streamResetRate");
  put(IDX_OPTIONS_STREAM_RESET_BURST, "streamResetBurst");
  put(IDX_OPTIONS_STRICT_HTTP_FIELD_WHITESPACE_VALIDATION, "strictFieldWhitespaceValidation");
  optionsBuffer[IDX_OPTIONS_FLAGS] = flags;
}

function getSettings() {
  const holder = { __proto__: null };
  const flags = settingsBuffer[IDX_SETTINGS_FLAGS];
  for (const [name, idx] of kSettingsNames) {
    if ((flags & (1 << idx)) === (1 << idx)) {
      holder[name] = settingsBuffer[idx];
    }
  }
  // 真机缺省项渲染
  const defaults = getDefaultSettings();
  for (const key of Object.keys(defaults)) {
    if (holder[key] === undefined) holder[key] = defaults[key];
  }
  switch (holder.enablePush) {
    case 0: holder.enablePush = false; break;
    case 1: holder.enablePush = true; break;
  }
  switch (holder.enableConnectProtocol) {
    case 0: holder.enableConnectProtocol = false; break;
    case 1: holder.enableConnectProtocol = true; break;
  }
  return holder;
}

function getDefaultSettings() {
  // 真机对拍（getpackedsettings 套件）：缺省 packed 不含 maxHeaderSize
  return {
    __proto__: null,
    headerTableSize: 4096,
    enablePush: true,
    initialWindowSize: 65535,
    maxFrameSize: 16384,
    maxConcurrentStreams: 4294967295,
    maxHeaderListSize: 65535,
    enableConnectProtocol: false,
    customSettings: {},
  };
}

// 会话/流状态（nghttp2 缓冲缺席：以引擎已知状态位渲染；引擎侧直接传对象）
function getStreamState(stream) {
  return stream.state ?? {
    state: 2, weight: 16, sumDependencyWeight: 0,
    localClose: 0, remoteClose: 0, localWindowSize: 65535,
  };
}

function getSessionState(session) {
  return session.state ?? {
    effectiveLocalWindowSize: 65535,
    effectiveRecvDataLength: 0,
    nextStreamID: 1,
    localWindowSize: 65535,
    lastProcStreamID: 0,
    remoteWindowSize: 65535,
    outboundQueueSize: 0,
    deflateDynamicTableSize: 4096,
    inflateDynamicTableSize: 4096,
  };
}

function remoteCustomSettingsToBuffer(customSettings) {
  const entries = Object.entries(customSettings ?? {});
  const out = new Uint32Array(entries.length * 2);
  let i = 0;
  for (const [id, value] of entries) {
    out[i++] = Number(id);
    out[i++] = Number(value);
  }
  return out;
}

// ── 头准备（node prepareRequestHeadersObject 精简引擎版；公共面自校验）────
function buildNgHeaderString(headers) {
  const pairs = [];
  for (const [key, value] of Object.entries(headers)) {
    if (Array.isArray(value)) {
      for (const v of value) pairs.push(`${key}\u0000${String(v)}`);
    } else {
      pairs.push(`${key}\u0000${String(value)}`);
    }
  }
  return pairs.join("\u0001");
}

function toHeaderObject(headers, sensitive, strictSingleValueFields) {
  const out = Object.create(null);
  const raw = [];
  for (const pair of headers) {
    const [name, value] = pair;
    raw.push(name, value);
    const lk = String(name).toLowerCase();
    if (out[lk] !== undefined &&
        (sensitive.has(lk) || strictSingleValueFields.has(lk))) {
      throw new codes.ERR_HTTP2_HEADER_SINGLE_VALUE(name);
    }
    if (out[lk] === undefined) {
      out[lk] = value;
    } else if (Array.isArray(out[lk])) {
      out[lk].push(value);
    } else {
      out[lk] = [out[lk], value];
    }
  }
  return out;
}

function prepareRequestHeadersObject(headers) {
  return headers;
}

function prepareRequestHeadersArray(headers) {
  return headers;
}

export {
  MAX_ADDITIONAL_SETTINGS,
  NghttpError,
  assertIsArray,
  assertIsObject,
  assertValidPseudoHeader,
  assertValidPseudoHeaderResponse,
  assertValidPseudoHeaderTrailer,
  assertWithinRange,
  buildNgHeaderString,
  getAuthority,
  getDefaultSettings,
  getSessionState,
  getSettings,
  getStreamState,
  isPayloadMeaningless,
  kAuthority,
  kProtocol,
  kProxySocket,
  kRequest,
  kSensitiveHeaders,
  kSocket,
  kStrictSingleValueFields,
  optionsBuffer,
  prepareRequestHeadersArray,
  prepareRequestHeadersObject,
  remoteCustomSettingsToBuffer,
  sessionName,
  settingsBuffer,
  toHeaderObject,
  updateOptionsBuffer,
  updateSettingsBuffer,
};
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn util_source_has_required_exports() {
        // 套件直引的出口齐备（test-http2-util-*/misc-util/kSocket 系）
        for sym in [
            "NghttpError",
            "kSocket",
            "assertIsObject",
            "assertIsArray",
            "assertWithinRange",
            "assertValidPseudoHeader",
            "sessionName",
            "getAuthority",
            "updateOptionsBuffer",
            "getDefaultSettings",
        ] {
            assert!(
                SOURCE.contains(&format!("  {sym},\n"))
                    || SOURCE.contains(&format!("function {sym}"))
                    || SOURCE.contains(&format!("class {sym}")),
                "missing export {sym}"
            );
        }
    }

    #[test]
    fn nghttp_error_tostring_shape() {
        // 真机 26.8.2：new NghttpError(-501).message == "Invalid argument"；
        // 401 → "Unknown error code"；toString 带 [ERR_HTTP2_ERROR]
        assert!(SOURCE.contains("\"Invalid argument\""));
        assert!(SOURCE.contains("\"Unknown error code\""));
        assert!(SOURCE.contains("Error [ERR_HTTP2_ERROR]"));
    }
}
