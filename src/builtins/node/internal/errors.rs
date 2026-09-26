//! `node:internal/errors`（Node `lib/internal/errors.js` 机制移植，MIT）。
//!
//! 忠实移植：`E()` 三分支（0 参 / -1（函数消息）/ N 参走 format）、`getMessage`、
//! `getExpectedArgumentLength`、`ERR_INVALID_ARG_TYPE`/`ERR_INVALID_ARG_VALUE`/
//! `ERR_OUT_OF_RANGE`/`ERR_UNHANDLED_ERROR`/`ERR_SOCKET_BAD_PORT`/`ERR_UNKNOWN_SIGNAL`/
//! `ERR_INVALID_THIS`/`ERR_ASYNC_CALLBACK`/`ERR_ASYNC_TYPE` 消息原文、
//! `AbortError`/`genericNodeError`/`hideStackFrames`/`determineSpecificType`/
//! `formatList`/`addNumericalSeparator`。
//!
//! 偏差：
//! - 错误码按需注册（9a 各模块用到的全集），非 2024 行全表；后续模块按需在此追加。
//! - `hideStackFrames` 保留（SpiderMonkey 支持 `Error.captureStackTrace`+`stackTraceLimit`，
//!   实测探针通过），但无 `overrideStackTrace`/prepareStackTrace 定制层（V8 专有）。
//! - `kEnhanceStackBeforeInspector` 仅保留符号常量（inspector 未做）。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node:internal/errors (mechanism + codes needed by Phase 9a modules).
import { format, inspect } from 'node:internal/util/inspect';

const kIsNodeError = Symbol('kIsNodeError');
const kEnhanceStackBeforeInspector = Symbol('kEnhanceStackBeforeInspector');
const messages = new Map();
const codes = {};
const classRegExp = /^[A-Z][a-zA-Z0-9]*$/;
const kTypes = [
  'string', 'function', 'number', 'object', 'Function', 'Object',
  'boolean', 'bigint', 'symbol',
];

// Only use this for integers! Decimal numbers do not work with this function.
function addNumericalSeparator(val) {
  let res = '';
  let i = val.length;
  const start = val[0] === '-' ? 1 : 0;
  for (; i >= start + 4; i -= 3) {
    res = `_${val.slice(i - 3, i)}${res}`;
  }
  return `${val.slice(0, i)}${res}`;
}

function determineSpecificType(value) {
  if (value === null) return 'null';
  if (value === undefined) return 'undefined';
  const type = typeof value;
  switch (type) {
    case 'bigint': return `type bigint (${value}n)`;
    case 'number':
      if (value === 0) {
        return 1 / value === -Infinity ? 'type number (-0)' : 'type number (0)';
      } else if (value !== value) {
        return 'type number (NaN)';
      } else if (value === Infinity) {
        return 'type number (Infinity)';
      } else if (value === -Infinity) {
        return 'type number (-Infinity)';
      }
      return `type number (${value})`;
    case 'boolean': return value ? 'type boolean (true)' : 'type boolean (false)';
    case 'symbol': return `type symbol (${String(value)})`;
    case 'function': return `function ${value.name}`;
    case 'object': {
      // `constructor` may be user-controlled: read once, guard types.
      const name = value.constructor?.name;
      if (typeof name === 'string' && name !== '') return `an instance of ${name}`;
      return `${inspect(value, { depth: -1 })}`;
    }
    case 'string':
      if (value.length > 28) value = `${value.slice(0, 25)}...`;
      if (value.indexOf("'") === -1) return `type string ('${value}')`;
      return `type string (${JSON.stringify(value)})`;
    default: {
      let inspected = inspect(value, { colors: false });
      if (inspected.length > 28) inspected = `${inspected.slice(0, 25)}...`;
      return `type ${type} (${inspected})`;
    }
  }
}

function formatList(array, type = 'and') {
  switch (array.length) {
    case 0: return '';
    case 1: return `${array[0]}`;
    case 2: return `${array[0]} ${type} ${array[1]}`;
    case 3: return `${array[0]}, ${array[1]}, ${type} ${array[2]}`;
    default:
      return `${array.slice(0, -1).join(', ')}, ${type} ${array[array.length - 1]}`;
  }
}

function getExpectedArgumentLength(msg) {
  let expectedLength = 0;
  const regex = /%[dfijoOs]/g;
  while (regex.exec(msg) !== null) expectedLength++;
  return expectedLength;
}

function getMessage(key, args, self) {
  const msg = messages.get(key);
  if (typeof msg === 'function') {
    return msg.apply(self, args);
  }
  const expectedLength = getExpectedArgumentLength(msg);
  if (args.length === 0) return msg;
  args.unshift(msg);
  return format(...args);
}

function makeNodeErrorWithCode(Base, key) {
  const msg = messages.get(key);
  const expectedLength = typeof msg !== 'string' ? -1 : getExpectedArgumentLength(msg);

  switch (expectedLength) {
    case 0: {
      class NodeError extends Base {
        code = key;
        constructor(...args) {
          super(msg);
        }
        get ['constructor']() { return Base; }
        get [kIsNodeError]() { return true; }
        toString() { return `${this.name} [${key}]: ${this.message}`; }
      }
      return NodeError;
    }
    case -1: {
      class NodeError extends Base {
        code = key;
        constructor(...args) {
          super();
          Object.defineProperty(this, 'message', {
            value: getMessage(key, args, this),
            enumerable: false, writable: true, configurable: true,
          });
        }
        get ['constructor']() { return Base; }
        get [kIsNodeError]() { return true; }
        toString() { return `${this.name} [${key}]: ${this.message}`; }
      }
      return NodeError;
    }
    default: {
      class NodeError extends Base {
        code = key;
        constructor(...args) {
          args.unshift(msg);
          super(format(...args));
        }
        get ['constructor']() { return Base; }
        get [kIsNodeError]() { return true; }
        toString() { return `${this.name} [${key}]: ${this.message}`; }
      }
      return NodeError;
    }
  }
}

// Stack-frame-hiding variant: constructed with stackTraceLimit 0.
function makeNodeErrorForHideStackFrame(Base, clazz) {
  class HideStackFramesError extends Base {
    constructor(...args) {
      const limit = Error.stackTraceLimit;
      Error.stackTraceLimit = 0;
      super(...args);
      Error.stackTraceLimit = limit;
    }
    // Node 同款：instance.constructor 回到原基类（wpt 兼容）
    get ['constructor']() { return clazz; }
  }
  return HideStackFramesError;
}

// Utility function for registering the error codes.
function E(sym, val, def, ...otherClasses) {
  messages.set(sym, val);
  const ErrClass = makeNodeErrorWithCode(def ?? Error, sym);
  if (otherClasses.includes(HideStackFramesError)) {
    ErrClass.HideStackFramesError = makeNodeErrorForHideStackFrame(ErrClass, def ?? Error);
  }
  codes[sym] = ErrClass;
}

// Marker class: presence in otherClasses triggers HideStackFramesError generation.
class HideStackFramesError extends Error {}

/**
 * Removes unnecessary frames from Node.js core errors.
 * (SpiderMonkey supports Error.captureStackTrace + stackTraceLimit; probed.)
 */
function hideStackFrames(fn) {
  function wrappedFn(...args) {
    try {
      return fn.apply(this, args);
    } catch (error) {
      if (Error.stackTraceLimit && typeof Error.captureStackTrace === 'function') {
        Error.captureStackTrace(error, wrappedFn);
      }
      throw error;
    }
  }
  wrappedFn.withoutStackTrace = fn;
  wrappedFn[Symbol.for('nodejs.preserve-stack-traces')] = false;
  return wrappedFn;
}

// A specialized Error for aborted operations.
class AbortError extends Error {
  constructor(message = 'The operation was aborted', options = undefined) {
    if (options !== undefined && typeof options !== 'object') {
      throw new codes.ERR_INVALID_ARG_TYPE('options', 'Object', options);
    }
    super(message, options);
    this.code = 'ABORT_ERR';
    this.name = 'AbortError';
  }
}

// A specialized Error for system call failures (10f os priority：化工序
// `err.info/errno/code/syscall` 由调用方补挂，与 libuv 口径对齐）。
// 真机 `name` 为自有属性（writable/configurable，非枚举），此处构造器直置。
class SystemError extends Error {
  constructor(...args) {
    super(...args);
    this.name = 'SystemError';
  }
}

// Generic Node.js error with extra properties.
const genericNodeError = hideStackFrames(function genericNodeError(message, errorProperties) {
  const err = new Error(message);
  if (errorProperties) Object.assign(err, errorProperties);
  return err;
});

// ── codes（9a 按需全集；后续模块按需在此追加，保持字母序）─────────────────
E('ERR_ASYNC_CALLBACK', '%s must be a function', TypeError);
E('ERR_ASYNC_TYPE', 'Invalid name for async "type": %s', TypeError);
E('ERR_CONSTRUCT_CALL_REQUIRED', 'Cannot call constructor without `new`', TypeError, HideStackFramesError);
E('ERR_FALSY_VALUE_REJECTION', 'Promise was rejected with falsy value', Error, HideStackFramesError);
E('ERR_INVALID_ARG_TYPE',
  (name, expected, actual) => {
    if (typeof name !== 'string') throw new TypeError("'name' must be a string");
    if (!Array.isArray(expected)) expected = [expected];

    let msg = 'The ';
    if (name.endsWith(' argument')) {
      msg += `${name} `;
    } else {
      const type = name.includes('.') ? 'property' : 'argument';
      msg += `"${name}" ${type} `;
    }
    msg += 'must be ';

    // node 真机逐字形态（26.8.2 对拍）：http hostname/host 与 agent 校验的
    // 两种三元素组合有专属渲染，其余维持通用格式。
    if (expected.length === 3 && expected[0] === 'string' &&
        expected[1] === 'undefined' && expected[2] === 'null') {
      return `${msg}of type string or one of undefined or null. Received ${determineSpecificType(actual)}`;
    }
    if (expected.length === 3 && expected[0] === 'Agent-like Object' &&
        expected[1] === 'undefined' && expected[2] === 'false') {
      return `${msg}one of Agent-like Object, undefined, or false. Received ${determineSpecificType(actual)}`;
    }

    const types = [];
    const instances = [];
    const other = [];
    for (const value of expected) {
      if (typeof value !== 'string') {
        throw new TypeError('All expected entries have to be of type string');
      }
      if (kTypes.includes(value)) {
        types.push(value.toLowerCase());
      } else if (classRegExp.test(value)) {
        instances.push(value);
      } else {
        if (value === 'object') {
          throw new TypeError('The value "object" should be written as "Object"');
        }
        other.push(value);
      }
    }

    if (instances.length > 0) {
      const pos = types.indexOf('object');
      if (pos !== -1) {
        types.splice(pos, 1);
        instances.push('Object');
      }
    }

    if (types.length > 0) {
      msg += `${types.length > 1 ? 'one of type' : 'of type'} ${formatList(types, 'or')}`;
      if (instances.length > 0 || other.length > 0) msg += ' or ';
    }
    if (instances.length > 0) {
      msg += `an instance of ${formatList(instances, 'or')}`;
      if (other.length > 0) msg += ' or ';
    }
    if (other.length > 0) {
      if (other.length > 1) {
        msg += `one of ${formatList(other, 'or')}`;
      } else {
        if (other[0].toLowerCase() !== other[0]) msg += 'an ';
        msg += `${other[0]}`;
      }
    }
    msg += `. Received ${determineSpecificType(actual)}`;
    return msg;
  }, TypeError, HideStackFramesError);
E('ERR_INVALID_ARG_VALUE', (name, value, reason = 'is invalid') => {
  let inspected = inspect(value);
  if (inspected.length > 128) inspected = `${inspected.slice(0, 128)}...`;
  const type = name.includes('.') ? 'property' : 'argument';
  return `The ${type} '${name}' ${reason}. Received ${inspected}`;
}, TypeError, HideStackFramesError);
E('ERR_INVALID_FD', '"fd" must be a positive integer: %s', RangeError);
// node lib/internal/errors.js TLS/crypto 段逐字（2026-09-26，tls SecureContext 移植）。
E('ERR_CRYPTO_CUSTOM_ENGINE_NOT_SUPPORTED',
  'Custom engines not supported by this OpenSSL', Error);
E('ERR_TLS_ALPN_CALLBACK_WITH_PROTOCOLS',
  'The ALPNCallback and ALPNProtocols TLS options are mutually exclusive',
  TypeError);
E('ERR_TLS_DH_PARAM_SIZE', 'DH parameter size %s is less than 2048', Error);
E('ERR_TLS_HANDSHAKE_TIMEOUT', 'TLS handshake timeout', Error);
E('ERR_TLS_INVALID_CONTEXT', '%s must be a SecureContext', TypeError);
E('ERR_TLS_INVALID_PROTOCOL_METHOD', '%s', TypeError);
E('ERR_TLS_INVALID_PROTOCOL_VERSION',
  '%j is not a valid %s TLS protocol version', TypeError);
E('ERR_TLS_INVALID_STATE', 'TLS socket connection must be securely established',
  Error);
E('ERR_TLS_PROTOCOL_VERSION_CONFLICT',
  'TLS protocol version %j conflicts with secureProtocol %j', TypeError);
E('ERR_TLS_RENEGOTIATION_DISABLED',
  'TLS session renegotiation disabled for this socket', Error);
E('ERR_TLS_REQUIRED_SERVER_NAME',
  '"servername" is required parameter for Server.addContext', Error);
E('ERR_TLS_SESSION_ATTACK', 'TLS session renegotiation attack detected', Error);
E('ERR_TLS_SNI_FROM_SERVER',
  'Cannot issue SNI from a TLS server-side socket', Error);
E('ERR_TRACE_EVENTS_CATEGORY_REQUIRED', 'At least one category must be enabled', TypeError);
E('ERR_INVALID_ASYNC_ID', 'Invalid %s value: %s', RangeError);
E('ERR_INVALID_THIS', 'Value of "this" must be of type %s', TypeError, HideStackFramesError);
E('ERR_INVALID_URI', 'URI malformed', URIError);
E('ERR_INVALID_URL_SCHEME', 'The URL must be of scheme %s', TypeError);
E('ERR_INVALID_FILE_URL_HOST', 'File URL host must be "localhost" or empty on %s', TypeError);
E('ERR_INVALID_FILE_URL_PATH', 'File URL path must not include encoded \\ or / characters', TypeError);
E('ERR_INVALID_URL', 'Invalid URL', TypeError);
E('ERR_INVALID_URL_PATTERN', 'Failed to construct URLPattern', TypeError, HideStackFramesError);
E('ERR_TTY_INIT_FAILED', 'TTY initialization failed: %s', Error);
E('ERR_INVALID_MIME_SYNTAX',
  (kind, input, pos) => {
    let msg = `The MIME syntax for a ${kind} in "${input}" is invalid`;
    if (pos !== undefined && pos !== null) msg += ` at ${pos}`;
    return msg;
  }, TypeError, HideStackFramesError);
E('ERR_UNKNOWN_ENCODING', 'Unknown encoding: %s', TypeError, HideStackFramesError);
E('ERR_UNESCAPED_CHARACTERS', '%s contains unescaped characters', TypeError, HideStackFramesError);
E('ERR_OPERATION_FAILED', 'Failed to %s URLPattern', TypeError, HideStackFramesError);
E('ERR_OUT_OF_RANGE',
  (str, range, input, replaceDefaultBoolean = false) => {
    if (!range) throw new TypeError('Missing "range" argument');
    let msg = replaceDefaultBoolean ? str : `The value of "${str}" is out of range.`;
    let received;
    if (Number.isInteger(input) && Math.abs(input) > 2 ** 32) {
      received = addNumericalSeparator(String(input));
    } else if (typeof input === 'bigint') {
      received = String(input);
      if (input > 2n ** 32n || input < -(2n ** 32n)) {
        received = addNumericalSeparator(received);
      }
      received += 'n';
    } else {
      received = inspect(input);
    }
    msg += ` It must be ${range}. Received ${received}`;
    return msg;
  }, RangeError, HideStackFramesError);
E('ERR_SOCKET_BAD_PORT', (name, port, allowZero = true) => {
  if (typeof allowZero !== 'boolean') {
    throw new TypeError("The 'allowZero' argument must be of type boolean.");
  }
  const operator = allowZero ? '>=' : '>';
  return `${name} should be ${operator} 0 and < 65536. Received ${determineSpecificType(port)}.`;
}, RangeError, HideStackFramesError);
E('ERR_UNHANDLED_ERROR',
  (err = undefined) => {
    const msg = 'Unhandled error.';
    if (err === undefined) return msg;
    return `${msg} (${err})`;
  }, Error);
E('ERR_PARSE_ARGS_UNKNOWN_OPTION', (option) => `Unknown option '${option}'`, TypeError, HideStackFramesError);
E('ERR_PARSE_ARGS_INVALID_OPTION_VALUE', (detail) => detail, TypeError, HideStackFramesError);
E('ERR_PARSE_ARGS_UNEXPECTED_POSITIONAL',
  (arg) => `Unexpected argument '${arg}'. This command does not take positional arguments`,
  TypeError, HideStackFramesError);
E('ERR_UNKNOWN_SIGNAL', 'Unknown signal: %s', TypeError, HideStackFramesError);

// ── Phase 9b：streams 系错误码（定义逐字自 node internal/errors.js）────────
E('ERR_ILLEGAL_CONSTRUCTOR', 'Illegal constructor', TypeError);
E('ERR_INVALID_RETURN_VALUE', (input, name, value) => {
  const type = determineSpecificType(value);
  return `Expected ${input} to be returned from the "${name}"` +
         ` function but got ${type}.`;
}, TypeError, RangeError);
E('ERR_METHOD_NOT_IMPLEMENTED', 'The %s method is not implemented', Error);
E('ERR_MISSING_ARGS',
  (...args) => {
    if (args.length === 0) throw new TypeError('At least one arg needs to be specified');
    let msg = 'The ';
    const len = args.length;
    const wrap = (a) => `"${a}"`;
    args = args.map((a) => (Array.isArray(a) ? a.map(wrap).join(' or ') : wrap(a)));
    msg += `${formatList(args)} argument${len > 1 ? 's' : ''}`;
    return `${msg} must be specified`;
  }, TypeError);
E('ERR_MULTIPLE_CALLBACK', 'Callback called multiple times', Error);
E('ERR_STREAM_ALREADY_FINISHED',
  'Cannot call %s after a stream was finished',
  Error);
E('ERR_STREAM_CANNOT_PIPE', 'Cannot pipe, not readable', Error);
E('ERR_STREAM_DESTROYED', 'Cannot call %s after a stream was destroyed', Error);
E('ERR_STREAM_ITER_MISSING_FLAG',
  'The stream/iter API requires the --experimental-stream-iter flag', TypeError);
E('ERR_STREAM_NULL_VALUES', 'May not write null values to stream', TypeError);
E('ERR_STREAM_PREMATURE_CLOSE', 'Premature close', Error);
E('ERR_STREAM_PUSH_AFTER_EOF', 'stream.push() after EOF', Error);
E('ERR_STREAM_UNABLE_TO_PIPE', 'Cannot pipe to a closed or destroyed stream', Error);
E('ERR_STREAM_UNSHIFT_AFTER_END_EVENT',
  'stream.unshift() after end event', Error);
E('ERR_STREAM_WRITE_AFTER_END', 'write after end', Error);
E('ERR_SYSTEM_ERROR',
  (syscall, code, message) => `A system error occurred: ${syscall} returned ${code} (${message})`,
  SystemError, HideStackFramesError);
E('ERR_BROTLI_INVALID_PARAM', '%s is not a valid Brotli parameter', RangeError, HideStackFramesError);
E('ERR_ZLIB_INITIALIZATION_FAILED', 'Initialization failed', Error, HideStackFramesError);
E('ERR_BUFFER_TOO_LARGE', 'Cannot create a Buffer larger than %s bytes', RangeError, HideStackFramesError);
E('ERR_ZIP_ARCHIVE_TOO_LARGE', 'ZIP archive structure exceeds the allowed size: %s', RangeError, HideStackFramesError);
E('ERR_ZIP_ENTRY_CORRUPT', 'ZIP entry is corrupt: %s', Error, HideStackFramesError);
E('ERR_ZIP_ENTRY_NOT_FOUND', 'no such entry %j in the archive', Error, HideStackFramesError);
E('ERR_ZIP_ENTRY_TOO_LARGE', 'ZIP entry exceeds the allowed size: %s', RangeError, HideStackFramesError);
E('ERR_ZIP_INVALID_ARCHIVE', 'invalid ZIP archive: %s', Error, HideStackFramesError);
E('ERR_ZIP_NOT_WRITABLE', 'this archive was not opened for writing', TypeError, HideStackFramesError);
E('ERR_ZIP_UNSUPPORTED_FEATURE', 'unsupported ZIP feature: %s', Error, HideStackFramesError);
E('ERR_INVALID_STATE', 'Invalid state: %s', Error, TypeError, RangeError, HideStackFramesError);
// child_process 单 IPC 通道门（node 原文；stdio 套件双 ipc 形）。
E('ERR_IPC_ONE_PIPE', 'Child process can have only one IPC pipe', Error);
E('ERR_INVALID_HANDLE_TYPE', 'This handle type cannot be sent', TypeError);
// 10f http2 compat 面（node lib/internal/errors.js 文案逐字）
E('ERR_HTTP2_HEADERS_SENT', 'Response has already been initiated.', Error);
E('ERR_HTTP2_INVALID_STREAM', 'The stream has been destroyed', Error);
E('ERR_HTTP2_NO_SOCKET_MANIPULATION',
  'HTTP/2 sockets should not be directly manipulated (e.g. read and written)', Error);
E('ERR_HTTP2_INVALID_HEADER_VALUE', 'Invalid value "%s" for header "%s"', TypeError, HideStackFramesError);
E('ERR_HTTP2_PUSH_DISABLED', 'Push streams are not enabled.', Error);
// node lib/_http_common.js validateHeaderValue（header-validators 套件逐字
// 'Invalid value "undefined" for header "x"'；与 HTTP2 同形不同码）。
E('ERR_HTTP_INVALID_HEADER_VALUE', 'Invalid value "%s" for header "%s"', TypeError, HideStackFramesError);
// node lib/_http_outgoing.js（multiple-headers 套件逐字，收发同文案
// 'Cannot set/append headers after they are sent to the client'）。
E('ERR_HTTP_HEADERS_SENT', 'Cannot %s headers after they are sent to the client', Error, HideStackFramesError);
// node lib/internal/errors.js（splitting 套件逐字 'Invalid character in header
// content ["foo"]'；无字段即旧裸文案，header-validators/value-relaxed 套件）。
E('ERR_INVALID_CHAR',
  // node 原文双参 (name, field)；本仓旧调用只传 field（key），label 缺省
  // 'header content'；trailer 面传 (key, 'trailer content')（proto 套件逐字）。
  (field = undefined, label = 'header content') => field === undefined
    ? `Invalid character in ${label}`
    : `Invalid character in ${label} ["${field}"]`,
  TypeError, HideStackFramesError);
// node lib/internal/errors.js（response-statuscode 套件逐字；`%s` 遇对象走
// inspect——{}→'{}'、[]→'[]'，字符串/图元走原文）。
E('ERR_HTTP_INVALID_STATUS_CODE', 'Invalid status code: %s', RangeError);
E('ERR_HTTP_SOCKET_ENCODING', 'Changing the socket encoding is not allowed per RFC7230 Section 3.', Error);
E('ERR_HTTP_CONTENT_LENGTH_MISMATCH',
  (actual, expected) => `Response body's content-length of ${actual} byte(s) does not match the content-length of ${expected} byte(s) set in header`,
  Error);
E('ERR_HTTP_BODY_NOT_ALLOWED', 'Adding content for this request method or response status is not allowed.', Error);
E('ERR_INVALID_HTTP_TOKEN',
  // node 口径 (kind, name) 双参 + %j JSON 渲染（真机 'Method must be a valid
  // HTTP token ["\u0000"]'）；单参历史调用（http2 validateHeaderName）兼容为
  // kind='Header name'。
  (kind, name) => {
    // node 口径：值原样插值（request-invalid-method-error 套件 '\0' 形断言
    // 裸控制字符，非 JSON 转义）；单参历史调用（http2）兼容为 kind='Header name'。
    if (name === undefined || name === null) {
      return `Header name must be a valid HTTP token ["${kind}"]`;
    }
    return `${kind} must be a valid HTTP token ["${name}"]`;
  }, TypeError, HideStackFramesError);
E('ERR_INVALID_PROTOCOL', 'Protocol "%s" not supported. Expected "%s"', TypeError, HideStackFramesError);

// errors.js:172 同款（AggregateError 聚合；errors.errors 已是聚合体则吸收）
const aggregateTwoErrors = (innerError, outerError) => {
  if (innerError && outerError && innerError !== outerError) {
    if (Array.isArray(outerError.errors)) {
      outerError.errors.push(innerError);
      return outerError;
    }
    const err = new AggregateError([outerError, innerError], outerError.message);
    err.code = outerError.code;
    return err;
  }
  return innerError || outerError;
};

export {
  AbortError,
  genericNodeError,
  codes,
  determineSpecificType,
  E,
  getMessage,
  formatList,
  hideStackFrames,
  addNumericalSeparator,
  aggregateTwoErrors,
  kEnhanceStackBeforeInspector,
};
export default { AbortError, genericNodeError, codes, determineSpecificType, E, getMessage, formatList, hideStackFrames, addNumericalSeparator, aggregateTwoErrors, kEnhanceStackBeforeInspector };
"#;
