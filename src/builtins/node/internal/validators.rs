//! `node:internal/validators`（Node `lib/internal/validators.js` 按需移植，MIT）。
//!
//! 忠实移植 9a 所需验证器（消息/抛错语义逐字）：validateString/Number/Integer/
//! Int32/Uint32/Boolean/Function/PlainFunction/Object(+k 常量)/Array/StringArray/
//! BooleanArray/AbortSignal(Array)/OneOf/Dictionary/Undefined/Union/Buffer/Port/
//! SignalName/parseFileMode/isInt32/isUint32。
//!
//! 偏差：
//! - `signals` 常量表内联（Node 来自 `internalBinding('constants').os.signals`）：
//!   unix 全集 + Windows 子集；validateSignalName 用。
//! - `validateEncoding`/Link 头/IgnoreOption 系列（9a 未用）未移植。
//! - `normalizeEncoding` 从 `node:internal/util` 引入。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node:internal/validators (validators needed by Phase 9a modules).
import errors from 'node:internal/errors';
import { normalizeEncoding } from 'node:internal/util';
import types from 'node:internal/util/types';
const {
  codes: {
    ERR_INVALID_ARG_TYPE: { HideStackFramesError: ERR_INVALID_ARG_TYPE },
    ERR_INVALID_ARG_VALUE: { HideStackFramesError: ERR_INVALID_ARG_VALUE },
    ERR_INVALID_THIS: { HideStackFramesError: ERR_INVALID_THIS },
    ERR_OUT_OF_RANGE: { HideStackFramesError: ERR_OUT_OF_RANGE },
    ERR_SOCKET_BAD_PORT: { HideStackFramesError: ERR_SOCKET_BAD_PORT },
    ERR_UNKNOWN_SIGNAL: { HideStackFramesError: ERR_UNKNOWN_SIGNAL },
  },
  hideStackFrames,
} = errors;
const { isAsyncFunction, isArrayBufferView, isRegExp } = types;

// Node: internalBinding('constants').os.signals（unix 全集 + win 子集）。
const signals = {
  SIGHUP: 1, SIGINT: 2, SIGQUIT: 3, SIGILL: 4, SIGTRAP: 5, SIGABRT: 6, SIGIOT: 6,
  SIGBUS: 7, SIGFPE: 8, SIGKILL: 9, SIGUSR1: 10, SIGSEGV: 11, SIGUSR2: 12,
  SIGPIPE: 13, SIGALRM: 14, SIGTERM: 15, SIGSTKFLT: 16, SIGCHLD: 17, SIGCONT: 18,
  SIGSTOP: 19, SIGTSTP: 20, SIGTTIN: 21, SIGTTOU: 22, SIGURG: 23, SIGXCPU: 24,
  SIGXFSZ: 25, SIGVTALRM: 26, SIGPROF: 27, SIGWINCH: 28, SIGIO: 29, SIGPOLL: 29,
  SIGINFO: 29, SIGPWR: 30, SIGSYS: 31, SIGBREAK: 21,
};

function isInt32(value) {
  return value === (value | 0);
}

function isUint32(value) {
  return value === (value >>> 0);
}

const octalReg = /^[0-7]+$/;
const modeDesc = 'must be a 32-bit unsigned integer or an octal string';

function parseFileMode(value, name, def) {
  value ??= def;
  if (typeof value === 'string') {
    if (octalReg.exec(value) === null) {
      throw new ERR_INVALID_ARG_VALUE(name, value, modeDesc);
    }
    value = Number.parseInt(value, 8);
  }
  validateUint32(value, name);
  return value + 0;
}

const validateInteger = hideStackFrames(
  (value, name, min = -9007199254740991, max = 9007199254740991) => {
    if (typeof value !== 'number') throw new ERR_INVALID_ARG_TYPE(name, 'number', value);
    if (!Number.isInteger(value)) throw new ERR_OUT_OF_RANGE(name, 'an integer', value);
    if (value < min || value > max) {
      throw new ERR_OUT_OF_RANGE(name, `>= ${min} && <= ${max}`, value);
    }
  },
);

const validateInt32 = hideStackFrames(
  (value, name, min = -2147483648, max = 2147483647) => {
    if (typeof value !== 'number') {
      throw new ERR_INVALID_ARG_TYPE(name, 'number', value);
    }
    if (!Number.isInteger(value)) {
      throw new ERR_OUT_OF_RANGE(name, 'an integer', value);
    }
    if (value < min || value > max) {
      throw new ERR_OUT_OF_RANGE(name, `>= ${min} && <= ${max}`, value);
    }
  },
);

const validateUint32 = hideStackFrames((value, name, positive = false) => {
  if (typeof value !== 'number') {
    throw new ERR_INVALID_ARG_TYPE(name, 'number', value);
  }
  if (!Number.isInteger(value)) {
    throw new ERR_OUT_OF_RANGE(name, 'an integer', value);
  }
  const min = positive ? 1 : 0;
  const max = 4294967295; // 2 ** 32 - 1
  if (value < min || value > max) {
    throw new ERR_OUT_OF_RANGE(name, `>= ${min} && <= ${max}`, value);
  }
});

const validateString = hideStackFrames((value, name) => {
  if (typeof value !== 'string') throw new ERR_INVALID_ARG_TYPE(name, 'string', value);
});

const validateStringWithoutNullBytes = hideStackFrames((value, name) => {
  validateString(value, name);
  if (value.includes('\u0000')) {
    throw new ERR_INVALID_ARG_VALUE(name, value, 'must be a string without null bytes');
  }
});

const validateNumber = hideStackFrames((value, name, min = undefined, max) => {
  if (typeof value !== 'number') throw new ERR_INVALID_ARG_TYPE(name, 'number', value);
  if ((min != null && value < min) || (max != null && value > max) ||
    ((min != null || max != null) && Number.isNaN(value))) {
    throw new ERR_OUT_OF_RANGE(
      name,
      `${min != null ? `>= ${min}` : ''}${min != null && max != null ? ' && ' : ''}${max != null ? `<= ${max}` : ''}`,
      value);
  }
});

const validateOneOf = hideStackFrames((value, name, oneOf) => {
  if (!oneOf.includes(value)) {
    const allowed = oneOf.map((v) => (typeof v === 'string' ? `'${v}'` : String(v))).join(', ');
    const reason = 'must be one of: ' + allowed;
    throw new ERR_INVALID_ARG_VALUE(name, value, reason);
  }
});

const validateBoolean = hideStackFrames((value, name) => {
  if (typeof value !== 'boolean') throw new ERR_INVALID_ARG_TYPE(name, 'boolean', value);
});

const kValidateObjectNone = 0;
const kValidateObjectAllowNullable = 1 << 0;
const kValidateObjectAllowArray = 1 << 1;
const kValidateObjectAllowFunction = 1 << 2;
const kValidateObjectAllowObjects = kValidateObjectAllowArray | kValidateObjectAllowFunction;
const kValidateObjectAllowObjectsAndNull = kValidateObjectAllowNullable |
  kValidateObjectAllowArray | kValidateObjectAllowFunction;

const validateObject = hideStackFrames(
  (value, name, options = kValidateObjectNone) => {
    if (options === kValidateObjectNone) {
      if (value === null || Array.isArray(value)) {
        throw new ERR_INVALID_ARG_TYPE(name, 'Object', value);
      }
      if (typeof value !== 'object') {
        throw new ERR_INVALID_ARG_TYPE(name, 'Object', value);
      }
    } else {
      const throwOnNullable = (kValidateObjectAllowNullable & options) === 0;
      if (throwOnNullable && value === null) {
        throw new ERR_INVALID_ARG_TYPE(name, 'Object', value);
      }
      const throwOnArray = (kValidateObjectAllowArray & options) === 0;
      if (throwOnArray && Array.isArray(value)) {
        throw new ERR_INVALID_ARG_TYPE(name, 'Object', value);
      }
      const throwOnFunction = (kValidateObjectAllowFunction & options) === 0;
      const typeofValue = typeof value;
      if (typeofValue !== 'object' && (throwOnFunction || typeofValue !== 'function')) {
        throw new ERR_INVALID_ARG_TYPE(name, 'Object', value);
      }
    }
  });

const validateDictionary = hideStackFrames((value, name) => {
  if (value != null && typeof value !== 'object' && typeof value !== 'function') {
    throw new ERR_INVALID_ARG_TYPE(name, 'a dictionary', value);
  }
});

const validateArray = hideStackFrames((value, name, minLength = 0) => {
  if (!Array.isArray(value)) {
    throw new ERR_INVALID_ARG_TYPE(name, 'Array', value);
  }
  if (value.length < minLength) {
    const reason = `must have a length of at least ${minLength}`;
    throw new ERR_INVALID_ARG_VALUE(name, value, reason);
  }
});

const validateStringArray = hideStackFrames((value, name) => {
  validateArray(value, name);
  for (let i = 0; i < value.length; ++i) {
    if (typeof value[i] !== 'string') {
      throw new ERR_INVALID_ARG_TYPE(`${name}[${i}]`, 'string', value[i]);
    }
  }
});

const validateBooleanArray = hideStackFrames((value, name) => {
  validateArray(value, name);
  for (let i = 0; i < value.length; ++i) {
    if (value[i] !== true && value[i] !== false) {
      throw new ERR_INVALID_ARG_TYPE(`${name}[${i}]`, 'boolean', value[i]);
    }
  }
});

function validateAbortSignalArray(value, name) {
  validateArray(value, name);
  for (let i = 0; i < value.length; i++) {
    const signal = value[i];
    const indexedName = `${name}[${i}]`;
    if (signal == null) {
      throw new ERR_INVALID_ARG_TYPE(indexedName, 'AbortSignal', signal);
    }
    validateAbortSignal(signal, indexedName);
  }
}

const validateSignalName = hideStackFrames((signal, name = 'signal') => {
  validateString(signal, name);
  if (signals[signal] === undefined) {
    if (signals[signal.toUpperCase()] !== undefined) {
      throw new ERR_UNKNOWN_SIGNAL(signal + ' (signals must use all capital letters)');
    }
    throw new ERR_UNKNOWN_SIGNAL(signal);
  }
});

const validateBuffer = hideStackFrames((buffer, name = 'buffer') => {
  if (!isArrayBufferView(buffer)) {
    throw new ERR_INVALID_ARG_TYPE(name, ['Buffer', 'TypedArray', 'DataView'], buffer);
  }
});

const validatePort = hideStackFrames((port, name = 'Port', allowZero = true) => {
  if ((typeof port !== 'number' && typeof port !== 'string') ||
      (typeof port === 'string' && port.trim().length === 0) ||
      +port !== (+port >>> 0) ||
      port > 0xffff ||
      (+port === 0 && !allowZero)) {
    throw new ERR_SOCKET_BAD_PORT(name, port, allowZero);
  }
  return port | 0;
});

const validateAbortSignal = hideStackFrames((signal, name) => {
  if (signal !== undefined &&
      (signal === null || typeof signal !== 'object' || !('aborted' in signal))) {
    throw new ERR_INVALID_ARG_TYPE(name, 'AbortSignal', signal);
  }
});

const validateFunction = hideStackFrames((value, name) => {
  if (typeof value !== 'function') throw new ERR_INVALID_ARG_TYPE(name, 'Function', value);
});

const validatePlainFunction = hideStackFrames((value, name) => {
  if (typeof value !== 'function' || isAsyncFunction(value)) {
    throw new ERR_INVALID_ARG_TYPE(name, 'Function', value);
  }
});

const validateUndefined = hideStackFrames((value, name) => {
  if (value !== undefined) throw new ERR_INVALID_ARG_TYPE(name, 'undefined', value);
});

function validateUnion(value, name, union) {
  if (!union.includes(value)) {
    throw new ERR_INVALID_ARG_TYPE(name, `('${union.join('|')}')`, value);
  }
}

const validateThisInternalField = hideStackFrames((object, fieldKey, className) => {
  if (typeof object !== 'object' || object === null ||
      !Object.prototype.hasOwnProperty.call(object, fieldKey)) {
    throw new ERR_INVALID_THIS(className);
  }
});

// R2-iter（node 原文逐字）：undefined/NaN 回 false，余下走 validateNumber +
// 无穷即 ERR_OUT_OF_RANGE（checkRanges 系 budget 门用）。
const validateFiniteNumber = hideStackFrames((number, name) => {
  if (number === undefined) {
    return false;
  }
  if (Number.isFinite(number)) {
    return true;
  }
  if (Number.isNaN(number)) {
    return false;
  }
  validateNumber(number, name);
  throw new ERR_OUT_OF_RANGE(name, 'a finite number', number);
});

const checkRangesOrGetDefault = hideStackFrames(
  (number, name, lower, upper, def) => {
    if (!validateFiniteNumber(number, name)) {
      return def;
    }
    if (number < lower || number > upper) {
      throw new ERR_OUT_OF_RANGE(name, `>= ${lower} and <= ${upper}`, number);
    }
    return number;
  },
);

export {
  isInt32,
  isUint32,
  parseFileMode,
  validateArray,
  validateStringArray,
  validateBooleanArray,
  validateAbortSignalArray,
  validateBoolean,
  validateBuffer,
  validateDictionary,
  validateFunction,
  validateInt32,
  validateInteger,
  validateNumber,
  validateObject,
  kValidateObjectNone,
  kValidateObjectAllowNullable,
  kValidateObjectAllowArray,
  kValidateObjectAllowFunction,
  kValidateObjectAllowObjects,
  kValidateObjectAllowObjectsAndNull,
  validateOneOf,
  validatePlainFunction,
  validatePort,
  validateSignalName,
  validateString,
  validateStringWithoutNullBytes,
  validateUint32,
  validateUndefined,
  validateUnion,
  validateAbortSignal,
  validateThisInternalField,
  validateFiniteNumber,
  checkRangesOrGetDefault,
};
export default { isInt32, isUint32, parseFileMode, validateArray, validateStringArray, validateBooleanArray, validateAbortSignalArray, validateBoolean, validateBuffer, validateDictionary, validateFunction, validateInt32, validateInteger, validateNumber, validateObject, kValidateObjectNone, kValidateObjectAllowNullable, kValidateObjectAllowArray, kValidateObjectAllowFunction, kValidateObjectAllowObjects, kValidateObjectAllowObjectsAndNull, validateOneOf, validatePlainFunction, validatePort, validateSignalName, validateString, validateStringWithoutNullBytes, validateUint32, validateUndefined, validateUnion, validateAbortSignal, validateThisInternalField, validateFiniteNumber, checkRangesOrGetDefault };
"#;
