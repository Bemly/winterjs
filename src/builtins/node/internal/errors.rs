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

// Generic Node.js error with extra properties.
const genericNodeError = hideStackFrames(function genericNodeError(message, errorProperties) {
  const err = new Error(message);
  if (errorProperties) Object.assign(err, errorProperties);
  return err;
});

// ── codes（9a 按需全集；后续模块按需在此追加，保持字母序）─────────────────
E('ERR_ASYNC_CALLBACK', '%s must be a function', TypeError);
E('ERR_ASYNC_TYPE', 'Invalid name for async "type": %s', TypeError);
E('ERR_FALSY_VALUE_REJECTION', 'A promise was rejected with a falsy value', Error, HideStackFramesError);
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
E('ERR_INVALID_ASYNC_ID', 'Invalid %s value: %s', RangeError);
E('ERR_INVALID_THIS', 'Value of "this" must be of type %s', TypeError, HideStackFramesError);
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
E('ERR_UNKNOWN_SIGNAL', 'Unknown signal: %s', TypeError, HideStackFramesError);

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
  kEnhanceStackBeforeInspector,
};
export default { AbortError, genericNodeError, codes, determineSpecificType, E, getMessage, formatList, hideStackFrames, addNumericalSeparator, kEnhanceStackBeforeInspector };
"#;
