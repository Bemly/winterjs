//! `node:util`（Node `lib/util.js` 公开面移植，MIT；bun-compat §1 "先行件"）。
//!
//! 忠实移植：`format`/`formatWithOptions`/`inspect`/`stripVTControlCharacters`（经
//! internal/util/inspect）、`promisify`（含 custom/customPromisifyArgs/DEP0174
//! 警告）、`callbackify`（含 falsy rejection 包裹 + 描述符复制）、`inherits`、
//! `_extend`（DEP0060）、`isDeepStrictEqual`（严格面）、`toUSVString`、
//! `parseEnv`（9j 差分移植，真机 26.8.2 全例对过）、
//! `convertProcessSignalToExitCode`、legacy is* 判定、`debuglog`、`deprecate`。
//! 10a 新增（真机 26.8.2 逐项对过码与文案）：`parseArgs`（strict/nonstrict/
//! 短项组/tokens 全形态）、`MIMEType`/`MIMEParams`（essence no-op setter、
//! 无 size/sort/forEach、delete 回 undefined）、`getSystemErrorName/Message/Map`
//!（85 条 UV errno 定表由真机导出嵌入；Map 每次返回新拷贝）。
//!
//! 偏差（9a 口径，逐条记档）：
//! - `isDeepStrictEqual` 为务实重写（comparisons.js 引擎的严格面算法，含
//!   循环引用 memo/无序 Map/Set/TypedArray 内容/原型同一性/Object.is 数值）；
//!   宽松 `isDeepEqual` 非公开面未移植。
//! - `styleText` 最小实现（内联 ANSI 表；NO_COLOR/isTTY 判色；inspect.colors
//!   未暴露——本仓 inspect 无色）。
//! - 未移植（后续切片按需）：`getCallSites`/`markPromiseAsHandled`/`aborted`/
//!   transferable 系列（引擎绑定）。
//! - `TextEncoder`/`TextDecoder` 直通全局（本仓 prelude 实现）。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node lib/util.js public face (see module docs for deviations).
import inspectModule from 'node:internal/util/inspect';
import errors from 'node:internal/errors';
import types from 'node:internal/util/types';
import {
  validateBoolean,
  validateFunction,
  validateNumber,
  validateOneOf,
  validateString,
} from 'node:internal/validators';

const {
  codes: {
    ERR_FALSY_VALUE_REJECTION: { HideStackFramesError: ERR_FALSY_VALUE_REJECTION },
    ERR_INVALID_ARG_TYPE: { HideStackFramesError: ERR_INVALID_ARG_TYPE },
    ERR_INVALID_ARG_VALUE: { HideStackFramesError: ERR_INVALID_ARG_VALUE },
    ERR_INVALID_MIME_SYNTAX: { HideStackFramesError: ERR_INVALID_MIME_SYNTAX },
    ERR_OUT_OF_RANGE: { HideStackFramesError: ERR_OUT_OF_RANGE },
    ERR_PARSE_ARGS_UNKNOWN_OPTION: { HideStackFramesError: ERR_PARSE_ARGS_UNKNOWN_OPTION },
    ERR_PARSE_ARGS_INVALID_OPTION_VALUE: { HideStackFramesError: ERR_PARSE_ARGS_INVALID_OPTION_VALUE },
    ERR_PARSE_ARGS_UNEXPECTED_POSITIONAL: { HideStackFramesError: ERR_PARSE_ARGS_UNEXPECTED_POSITIONAL },
  },
} = errors;
const {
  format,
  formatWithOptions,
  inspect,
  stripVTControlCharacters,
} = inspectModule;

// ── inherits / _extend ────────────────────────────────────────────────────
function inherits(ctor, superCtor) {
  if (ctor === undefined || ctor === null)
    throw new ERR_INVALID_ARG_TYPE('ctor', 'Function', ctor);
  if (superCtor === undefined || superCtor === null)
    throw new ERR_INVALID_ARG_TYPE('superCtor', 'Function', superCtor);
  if (superCtor.prototype === undefined) {
    throw new ERR_INVALID_ARG_TYPE('superCtor.prototype', 'Object', superCtor.prototype);
  }
  Object.defineProperty(ctor, 'super_', {
    __proto__: null, value: superCtor, writable: true, configurable: true,
  });
  Object.setPrototypeOf(ctor.prototype, superCtor.prototype);
}

function _extend(target, source) {
  if (source === null || typeof source !== 'object') return target;
  const keys = Object.keys(source);
  let i = keys.length;
  while (i--) {
    target[keys[i]] = source[keys[i]];
  }
  return target;
}

// ── deprecate（warn-once；process.noDeprecation 尊重）──────────────────────
function deprecate(fn, msg, code, { modifyPrototype } = {}) {
  validateFunction(fn, 'fn');
  if (code !== undefined) validateString(code, 'code');
  let warned = false;
  function deprecated(...args) {
    if (!process.noDeprecation && !warned) {
      warned = true;
      process.emitWarning(
        msg ?? 'This API is deprecated.',
        code !== undefined ? 'DeprecationWarning' : 'DeprecationWarning',
        code,
        deprecate,
      );
    }
    if (new.target) {
      return Reflect.construct(fn, args, new.target);
    }
    return Reflect.apply(fn, this, args);
  }
  if (modifyPrototype !== false) {
    Object.setPrototypeOf(deprecated, fn);
    if (fn.prototype) {
      deprecated.prototype = fn.prototype;
    }
    try {
      Object.defineProperty(deprecated, 'length', {
        ...Object.getOwnPropertyDescriptor(fn, 'length'),
      });
    } catch {}
  }
  return deprecated;
}

// ── promisify（逐字移植）──────────────────────────────────────────────────
const kCustomPromisifiedSymbol = Symbol.for('nodejs.util.promisify.custom');
const kCustomPromisifyArgsSymbol = Symbol('customPromisifyArgs');

function promisify(original) {
  validateFunction(original, 'original');

  if (original[kCustomPromisifiedSymbol]) {
    const fn = original[kCustomPromisifiedSymbol];
    validateFunction(fn, 'util.promisify.custom');
    return Object.defineProperty(fn, kCustomPromisifiedSymbol, {
      __proto__: null, value: fn, enumerable: false, writable: false, configurable: true,
    });
  }

  // Names to create an object from in case the callback receives multiple
  // arguments, e.g. ['bytesRead', 'buffer'] for fs.read.
  const argumentNames = original[kCustomPromisifyArgsSymbol];

  function fn(...args) {
    return new Promise((resolve, reject) => {
      args.push((err, ...values) => {
        if (err) {
          return reject(err);
        }
        if (argumentNames !== undefined && values.length > 1) {
          const obj = {};
          for (let i = 0; i < argumentNames.length; i++)
            obj[argumentNames[i]] = values[i];
          resolve(obj);
        } else {
          resolve(values[0]);
        }
      });
      if (types.isPromise(Reflect.apply(original, this, args))) {
        process.emitWarning('Calling promisify on a function that returns a Promise is likely a mistake.',
                            'DeprecationWarning', 'DEP0174');
      }
    });
  }

  Object.setPrototypeOf(fn, Object.getPrototypeOf(original));

  Object.defineProperty(fn, kCustomPromisifiedSymbol, {
    __proto__: null, value: fn, enumerable: false, writable: false, configurable: true,
  });

  const descriptors = Object.getOwnPropertyDescriptors(original);
  const propertiesValues = Object.values(descriptors);
  for (let i = 0; i < propertiesValues.length; i++) {
    Object.setPrototypeOf(propertiesValues[i], null);
  }
  return Object.defineProperties(fn, descriptors);
}

promisify.custom = kCustomPromisifiedSymbol;

// ── callbackify（逐字移植）────────────────────────────────────────────────
const callbackifyOnRejected = (reason, cb) => {
  // `!reason` guard inspired by bluebird.
  if (!reason) {
    reason = new ERR_FALSY_VALUE_REJECTION(reason);
    Error.captureStackTrace(reason, callbackifyOnRejected);
  }
  return cb(reason);
};

function callbackify(original) {
  validateFunction(original, 'original');

  function callbackified(...args) {
    const maybeCb = args.pop();
    validateFunction(maybeCb, 'last argument');
    const cb = maybeCb.bind(this);
    Reflect.apply(original, this, args)
      .then((ret) => process.nextTick(cb, null, ret),
            (rej) => process.nextTick(callbackifyOnRejected, rej, cb));
  }

  const descriptors = Object.getOwnPropertyDescriptors(original);
  if (typeof descriptors.length?.value === 'number') {
    descriptors.length.value++;
  }
  if (typeof descriptors.name?.value === 'string') {
    descriptors.name.value += 'Callbackified';
  }
  const propertiesValues = Object.values(descriptors);
  for (let i = 0; i < propertiesValues.length; i++) {
    Object.setPrototypeOf(propertiesValues[i], null);
  }
  Object.defineProperties(callbackified, descriptors);
  return callbackified;
}

// ── isDeepStrictEqual（严格面务实重写，偏差见模块头注）────────────────────
const wellKnownPrimitiveWrappers = new Set(['[object Boolean]', '[object Number]', '[object String]', '[object Symbol]', '[object BigInt]']);

function isObjectLike(v) {
  return typeof v === 'object' && v !== null || typeof v === 'function';
}

function keySet(obj) {
  const keys = Object.keys(obj).concat(Object.getOwnPropertySymbols(obj).filter((s) =>
    Object.prototype.propertyIsEnumerable.call(obj, s)));
  return keys;
}

function isArrayOfTypedArrays(a, b) {
  return ArrayBuffer.isView(a) && ArrayBuffer.isView(b) &&
    !(a instanceof DataView) && !(b instanceof DataView) &&
    a.constructor === b.constructor;
}

function typedArraysEqual(a, b) {
  if (a.byteLength !== b.byteLength || a.length !== b.length) return false;
  const av = new a.constructor(a.buffer, a.byteOffset, a.length);
  const bv = new b.constructor(b.buffer, b.byteOffset, b.length);
  for (let i = 0; i < av.length; i++) {
    if (!Object.is(av[i], bv[i])) return false;
  }
  return true;
}

function innerDeepStrictEqual(val1, val2, memos) {
  // 数值/原始值（Object.is：-0/NaN 语义）
  if (Object.is(val1, val2)) return true;
  if (!isObjectLike(val1) || !isObjectLike(val2)) return false;

  // 原型必须同一
  if (Object.getPrototypeOf(val1) !== Object.getPrototypeOf(val2)) return false;

  // 循环引用 memo
  let pair;
  for (pair of memos) {
    if (pair[0] === val1 && pair[1] === val2) return true;
    if (pair[1] === val1 && pair[0] === val2) return true;
  }
  memos.push([val1, val2]);
  try {
    // 值对象
    if (val1 instanceof Date && val2 instanceof Date) {
      return Object.is(val1.getTime(), val2.getTime());
    }
    if (val1 instanceof RegExp && val2 instanceof RegExp) {
      return val1.source === val2.source && val1.flags === val2.flags &&
        val1.lastIndex === val2.lastIndex;
    }
    if (types.isBoxedPrimitive(val1) && types.isBoxedPrimitive(val2)) {
      return Object.is(val1.valueOf(), val2.valueOf());
    }
    if (val1 instanceof Error && val2 instanceof Error) {
      return val1.message === val2.message && val1.name === val2.name &&
        innerDeepStrictEqual(val1.cause, val2.cause, memos);
    }
    if (ArrayBuffer.isView(val1) && ArrayBuffer.isView(val2)) {
      if (val1 instanceof DataView && val2 instanceof DataView) {
        if (val1.byteLength !== val2.byteLength || val1.byteOffset !== val2.byteOffset) return false;
        return innerDeepStrictEqual(val1.buffer, val2.buffer, memos);
      }
      if (val1 instanceof DataView || val2 instanceof DataView) return false;
      if (isArrayOfTypedArrays(val1, val2)) return typedArraysEqual(val1, val2);
      return false;
    }
    if ((val1 instanceof ArrayBuffer) && (val2 instanceof ArrayBuffer)) {
      if (val1.byteLength !== val2.byteLength) return false;
      return typedArraysEqual(new Uint8Array(val1), new Uint8Array(val2));
    }
    if (types.isMap(val1) && types.isMap(val2)) {
      if (val1.size !== val2.size) return false;
      // 无序比较：每个 entry 在 val2 中找严格相等 pair（先内容后键）
      const entries2 = [...val2.entries()];
      for (const [k1, v1] of val1.entries()) {
        let matched = -1;
        for (let i = 0; i < entries2.length; i++) {
          const [k2, v2] = entries2[i];
          if (innerDeepStrictEqual(k1, k2, memos) && innerDeepStrictEqual(v1, v2, memos)) {
            matched = i;
            break;
          }
        }
        if (matched === -1) return false;
        entries2.splice(matched, 1);
      }
      return true;
    }
    if (types.isSet(val1) && types.isSet(val2)) {
      if (val1.size !== val2.size) return false;
      const values2 = [...val2];
      for (const v1 of val1) {
        let matched = -1;
        for (let i = 0; i < values2.length; i++) {
          if (innerDeepStrictEqual(v1, values2[i], memos)) {
            matched = i;
            break;
          }
        }
        if (matched === -1) return false;
        values2.splice(matched, 1);
      }
      return true;
    }
    if (Array.isArray(val1) && Array.isArray(val2)) {
      if (val1.length !== val2.length) return false;
      for (let i = 0; i < val1.length; i++) {
        if (!innerDeepStrictEqual(val1[i], val2[i], memos)) return false;
      }
      // 稀疏洞一致性
      return keySet(val1).length === keySet(val2).length;
    }
    if (Array.isArray(val1) || Array.isArray(val2)) return false;

    // 普通对象：自有可枚举键集合（含 symbol）逐个严格比较
    const keys1 = keySet(val1);
    const keys2 = keySet(val2);
    if (keys1.length !== keys2.length) return false;
    for (const key of keys1) {
      if (!Object.prototype.hasOwnProperty.call(val2, key) ||
          !innerDeepStrictEqual(val1[key], val2[key], memos)) {
        return false;
      }
    }
    return true;
  } finally {
    memos.pop();
  }
}

function isDeepStrictEqual(a, b) {
  return innerDeepStrictEqual(a, b, []);
}

// ── 信号 → 退出码（convertProcessSignalToExitCode 面）────────────────────
const signalNumbers = {
  SIGHUP: 1, SIGINT: 2, SIGQUIT: 3, SIGILL: 4, SIGTRAP: 5, SIGABRT: 6, SIGIOT: 6,
  SIGBUS: 7, SIGFPE: 8, SIGKILL: 9, SIGUSR1: 10, SIGSEGV: 11, SIGUSR2: 12,
  SIGPIPE: 13, SIGALRM: 14, SIGTERM: 15, SIGSTKFLT: 16, SIGCHLD: 17, SIGCONT: 18,
  SIGSTOP: 19, SIGTSTP: 20, SIGTTIN: 21, SIGTTOU: 22, SIGURG: 23, SIGXCPU: 24,
  SIGXFSZ: 25, SIGVTALRM: 26, SIGPROF: 27, SIGWINCH: 28, SIGIO: 29, SIGPOLL: 29,
  SIGINFO: 29, SIGPWR: 30, SIGSYS: 31, SIGBREAK: 21,
};

function convertProcessSignalToExitCode(signal) {
  validateString(signal, 'signal');
  const num = signalNumbers[signal];
  return num === undefined ? undefined : 128 + num;
}

// ── debuglog（NODE_DEBUG 最小面）─────────────────────────────────────────
const debugCache = new Map();
function debuglog(set, cb) {
  validateString(set, 'set');
  if (typeof cb === 'function') cb(debugCache);
  if (debugCache.has(set)) return debugCache.get(set);
  const wildcard = process.env.NODE_DEBUG === '*' ||
    (process.env.NODE_DEBUG ?? '').split(',').map((s) => s.trim()).includes('*');
  const enabled = wildcard ||
    (process.env.NODE_DEBUG ?? '').split(',').map((s) => s.trim().toLowerCase()).includes(set.toLowerCase());
  const fn = enabled
    ? function debug(...args) { process.stderr.write(`${set} ${format(...args)}\n`); }
    : function debug() {};
  debugCache.set(set, fn);
  return fn;
}

// ── styleText（最小 ANSI 表，偏差见模块头注）─────────────────────────────
const ansiCodes = {
  red: [31, 39], green: [32, 39], yellow: [33, 39], blue: [34, 39],
  magenta: [35, 39], cyan: [36, 39], white: [37, 39], black: [30, 39],
  gray: [90, 39], grey: [90, 39],
  redBright: [91, 39], greenBright: [92, 39], yellowBright: [93, 39],
  blueBright: [94, 39], magentaBright: [95, 39], cyanBright: [96, 39], whiteBright: [97, 39],
  bgRed: [41, 49], bgGreen: [42, 49], bgYellow: [43, 49], bgBlue: [44, 49],
  bgMagenta: [45, 49], bgCyan: [46, 49], bgWhite: [47, 49], bgGray: [100, 49],
  bold: [1, 22], dim: [2, 22], italic: [3, 23], underline: [4, 24],
  strikethrough: [9, 29], hidden: [8, 28], inverse: [7, 27],
  reset: [0, 0], overline: [53, 55],
};

function styleText(formatArg, text, options) {
  const validateStream = options?.validateStream ?? true;
  validateString(text, 'text');
  if (options !== undefined) validateObject(options, 'options');
  validateBoolean(validateStream, 'options.validateStream');

  let skipColorize = false;
  if (validateStream) {
    const stream = options?.stream ?? process.stdout;
    const isTTY = stream === process.stdout ? process.stdout.isTTY :
      stream === process.stderr ? process.stderr.isTTY : false;
    skipColorize = !isTTY || process.env.NO_COLOR !== undefined;
  }
  if (skipColorize) return text;

  const formatArray = Array.isArray(formatArg) ? formatArg : [formatArg];
  let out = text;
  let open = '';
  let close = '';
  for (const key of formatArray) {
    if (key === 'none') continue;
    const codes = ansiCodes[key];
    if (!codes) {
      validateOneOf(key, 'format', Object.getOwnPropertyNames(ansiCodes));
    }
    open += `\u001b[${codes[0]}m`;
    close = `\u001b[${codes[1]}m${close}`;
  }
  return `${open}${out}${close}`;
}

// ── legacy is* 判定（DEP0044-DEP0056 口径）────────────────────────────────
function isBuffer(value) { return types.isUint8Array(value) && value.constructor?.name === 'Buffer'; }
function isArray(value) { return Array.isArray(value); }
function isBoolean(value) { return typeof value === 'boolean'; }
function isNull(value) { return value === null; }
function isNullOrUndefined(value) { return value == null; }
function isNumber(value) { return typeof value === 'number'; }
function isString(value) { return typeof value === 'string'; }
function isSymbol(value) { return typeof value === 'symbol'; }
function isUndefined(value) { return value === undefined; }
function isRegExp(value) { return types.isRegExp(value); }
function isObject(value) { return value !== null && typeof value === 'object'; }
function isDate(value) { return types.isDate(value); }
function isError(value) { return types.isNativeError(value) || value instanceof Error; }
function isFunction(value) { return typeof value === 'function'; }
function isPrimitive(value) { return value === null || (typeof value !== 'object' && typeof value !== 'function'); }

// ── toUSVString ───────────────────────────────────────────────────────────
function toUSVString(input) {
  return `${input}`.toWellFormed();
}

// ── parseEnv（dotenv 语义；真机 26.8.2 差分钉住，plan 9j）───────────────────
// 行按 \n 切；trim 后空行/`#` 跳过；`export ` 前缀剥离；首个 `=` 分键值
// （空键/无 `=` 丢弃）；值 trim 后首字符为引号则扫到同種闭引号（可跨行吞
// 后续行；永不闭合则首行原文、后续行照常解析），双引号内容只展开 `\n`，
// 单引号原文；其余（非引号）在首个 `#` 处截断再去尾空格。
function parseEnv(content) {
  validateString(content, 'content');
  const entries = new Map();
  const lines = content.split('\n');
  for (let i = 0; i < lines.length; i++) {
    let line = lines[i].trim();
    if (line === '' || line.startsWith('#')) continue;
    if (line === 'export') continue;
    if (/^export\s/.test(line)) line = line.slice(6).trim();
    const idx = line.indexOf('=');
    if (idx <= 0) continue;
    const key = line.slice(0, idx).trim();
    if (key === '') continue;
    let val = line.slice(idx + 1).trim();
    const q = val[0];
    if (q === '"' || q === "'") {
      let end = val.indexOf(q, 1);
      let j = i;
      let acc = val;
      while (end === -1 && j + 1 < lines.length) {
        j++;
        acc += '\n' + lines[j];
        end = acc.indexOf(q, 1);
      }
      if (end !== -1) {
        i = j;
        val = acc.slice(1, end);
        if (q === '"') val = val.replace(/\\n/g, '\n');
      }
    } else {
      const hash = val.indexOf('#');
      if (hash !== -1) val = val.slice(0, hash);
      val = val.trimEnd();
    }
    entries.set(key, val);
  }
  // 真机行为：结果键按 ASCII 排序（`B=1\nA=2` → A,B；大小写敏感，大写在前）。
  const obj = {};
  for (const k of [...entries.keys()].sort()) obj[k] = entries.get(k);
  return obj;
}

// ── getSystemError*（UV errno 定表；85 条数据由真机 26.8.2
// `getSystemErrorMap()` 导出嵌入，非手抄，见 plan3 10a）────────────────────
const systemErrorEntries = [[-4095,['EOF','end of file']],[-4094,['UNKNOWN','unknown error']],[-4080,['ECHARSET','invalid Unicode character']],[-4056,['ENONET','machine is not on the network']],[-4030,['EREMOTEIO','remote I/O error']],[-4023,['EUNATCH','protocol driver not attached']],[-3014,['EAI_PROTOCOL','resolved protocol is unknown']],[-3013,['EAI_BADHINTS','invalid value for hints']],[-3011,['EAI_SOCKTYPE','socket type not supported']],[-3010,['EAI_SERVICE','service not available for socket type']],[-3009,['EAI_OVERFLOW','argument buffer overflow']],[-3008,['EAI_NONAME','unknown node or service']],[-3007,['EAI_NODATA','no address']],[-3006,['EAI_MEMORY','out of memory']],[-3005,['EAI_FAMILY','ai_family not supported']],[-3004,['EAI_FAIL','permanent failure']],[-3003,['EAI_CANCELED','request canceled']],[-3002,['EAI_BADFLAGS','bad ai_flags value']],[-3001,['EAI_AGAIN','temporary failure']],[-3000,['EAI_ADDRFAMILY','address family not supported']],[-100,['EPROTO','protocol error']],[-96,['ENODATA','no data available']],[-92,['EILSEQ','illegal byte sequence']],[-89,['ECANCELED','operation canceled']],[-84,['EOVERFLOW','value too large for defined data type']],[-79,['EFTYPE','inappropriate file type or format']],[-78,['ENOSYS','function not implemented']],[-66,['ENOTEMPTY','directory not empty']],[-65,['EHOSTUNREACH','host is unreachable']],[-64,['EHOSTDOWN','host is down']],[-63,['ENAMETOOLONG','name too long']],[-62,['ELOOP','too many symbolic links encountered']],[-61,['ECONNREFUSED','connection refused']],[-60,['ETIMEDOUT','connection timed out']],[-58,['ESHUTDOWN','cannot send after transport endpoint shutdown']],[-57,['ENOTCONN','socket is not connected']],[-56,['EISCONN','socket is already connected']],[-55,['ENOBUFS','no buffer space available']],[-54,['ECONNRESET','connection reset by peer']],[-53,['ECONNABORTED','software caused connection abort']],[-51,['ENETUNREACH','network is unreachable']],[-50,['ENETDOWN','network is down']],[-49,['EADDRNOTAVAIL','address not available']],[-48,['EADDRINUSE','address already in use']],[-47,['EAFNOSUPPORT','address family not supported']],[-45,['ENOTSUP','operation not supported on socket']],[-44,['ESOCKTNOSUPPORT','socket type not supported']],[-43,['EPROTONOSUPPORT','protocol not supported']],[-42,['ENOPROTOOPT','protocol not available']],[-41,['EPROTOTYPE','protocol wrong type for socket']],[-40,['EMSGSIZE','message too long']],[-39,['EDESTADDRREQ','destination address required']],[-38,['ENOTSOCK','socket operation on non-socket']],[-37,['EALREADY','connection already in progress']],[-35,['EAGAIN','resource temporarily unavailable']],[-34,['ERANGE','result too large']],[-32,['EPIPE','broken pipe']],[-31,['EMLINK','too many links']],[-30,['EROFS','read-only file system']],[-29,['ESPIPE','invalid seek']],[-28,['ENOSPC','no space left on device']],[-27,['EFBIG','file too large']],[-26,['ETXTBSY','text file is busy']],[-25,['ENOTTY','inappropriate ioctl for device']],[-24,['EMFILE','too many open files']],[-23,['ENFILE','file table overflow']],[-22,['EINVAL','invalid argument']],[-21,['EISDIR','illegal operation on a directory']],[-20,['ENOTDIR','not a directory']],[-19,['ENODEV','no such device']],[-18,['EXDEV','cross-device link not permitted']],[-17,['EEXIST','file already exists']],[-16,['EBUSY','resource busy or locked']],[-14,['EFAULT','bad address in system call argument']],[-13,['EACCES','permission denied']],[-12,['ENOMEM','not enough memory']],[-9,['EBADF','bad file descriptor']],[-8,['ENOEXEC','exec format error']],[-7,['E2BIG','argument list too long']],[-6,['ENXIO','no such device or address']],[-5,['EIO','i/o error']],[-4,['EINTR','interrupted system call']],[-3,['ESRCH','no such process']],[-2,['ENOENT','no such file or directory']],[-1,['EPERM','operation not permitted']]];
const systemErrorMap = new Map(systemErrorEntries);
function checkSystemErrno(err) {
  if (typeof err !== 'number') throw new ERR_INVALID_ARG_TYPE('err', 'number', err);
  if (!Number.isInteger(err) || err > -1) throw new ERR_OUT_OF_RANGE('err', 'a negative integer', err);
}
function getSystemErrorName(err) {
  checkSystemErrno(err);
  const hit = systemErrorMap.get(err);
  return hit ? hit[0] : `Unknown system error ${err}`;
}
function getSystemErrorMessage(err) {
  checkSystemErrno(err);
  const hit = systemErrorMap.get(err);
  return hit ? hit[1] : `Unknown system error ${err}`;
}
function getSystemErrorMap() {
  // 真机每次返回新 Map（同一性 false），条目数组同样拷贝。
  return new Map(systemErrorEntries.map(([code, pair]) => [code, [pair[0], pair[1]]]));
}

// ── MIMEType/MIMEParams（WHATWG MIME 解析子集；真机 26.8.2 逐项对过）──────
// essence 错：`for a type/subtype in "<essence>" is invalid[ at N]`（空串无 at）；
// setter 同口径（shown = 所赋值）；type/subtype 小写化；essence setter 是 no-op；
// 无 `=` 的参数段跳过；名小写化、值大小写保留；空值/特殊字符序列化加引号；
// MIMEParams 无 size/sort/forEach，delete 回 undefined。
function mimeBadCharIndex(s) {
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    const ok = (c >= 48 && c <= 57) || (c >= 65 && c <= 90) || (c >= 97 && c <= 122) ||
      c === 33 || c === 35 || c === 36 || c === 37 || c === 38 || c === 39 || c === 42 ||
      c === 43 || c === 45 || c === 46 || c === 94 || c === 95 || c === 96 || c === 124 || c === 126;
    if (!ok) return i;
  }
  return -1;
}
function mimeCheckToken(kind, shown, value) {
  if (value === '') throw new ERR_INVALID_MIME_SYNTAX(kind, shown);
  const bad = mimeBadCharIndex(value);
  if (bad !== -1) throw new ERR_INVALID_MIME_SYNTAX(kind, shown, bad);
}
function mimeQuoteValue(value) {
  if (value === '' || /[^!#$%&'*+\-.^_`|~0-9A-Za-z]/.test(value)) {
    return '"' + value.replace(/(["\\])/g, '\\$1') + '"';
  }
  return value;
}
class MIMEParams {
  #pairs;
  constructor(pairs = []) { this.#pairs = pairs; }
  get(name) {
    name = String(name).toLowerCase();
    for (const [k, v] of this.#pairs) {
      if (k === name) return v;
    }
    return null;
  }
  set(name, value) {
    name = String(name).toLowerCase();
    value = String(value);
    mimeCheckToken('parameter name', name, name);
    this.#pairs = this.#pairs.filter(([k]) => k !== name);
    this.#pairs.push([name, value]);
  }
  has(name) {
    name = String(name).toLowerCase();
    return this.#pairs.some(([k]) => k === name);
  }
  delete(name) {
    name = String(name).toLowerCase();
    this.#pairs = this.#pairs.filter(([k]) => k !== name);
  }
  keys() { return this.#pairs.map(([k]) => k).values(); }
  values() { return this.#pairs.map(([, v]) => v).values(); }
  entries() { return this.#pairs.map(([k, v]) => [k, v]).values(); }
  toString() {
    return this.#pairs.map(([k, v]) => `${k}=${mimeQuoteValue(v)}`).join(';');
  }
}
class MIMEType {
  #type;
  #subtype;
  #params;
  constructor(input) {
    if (typeof input !== 'string') throw new ERR_INVALID_ARG_TYPE('input', 'string', input);
    const semi = input.indexOf(';');
    const ess = (semi === -1 ? input : input.slice(0, semi)).trim();
    const slash = ess.indexOf('/');
    const type = (slash === -1 ? ess : ess.slice(0, slash)).trim();
    const subtype = slash === -1 ? '' : ess.slice(slash + 1).trim();
    if (slash === -1 || type === '') {
      throw new ERR_INVALID_MIME_SYNTAX('type', ess);
    }
    mimeCheckToken('type', ess, type);
    if (subtype === '') {
      throw new ERR_INVALID_MIME_SYNTAX('subtype', ess);
    }
    mimeCheckToken('subtype', ess, subtype);
    this.#type = type.toLowerCase();
    this.#subtype = subtype.toLowerCase();
    const pairs = [];
    if (semi !== -1) {
      for (const part of input.slice(semi + 1).split(';')) {
        const eq = part.indexOf('=');
        if (eq === -1) continue;
        const name = part.slice(0, eq).trim().toLowerCase();
        if (name === '') continue;
        mimeCheckToken('parameter name', name, name);
        let value = part.slice(eq + 1).trim();
        if (value.length >= 2 && value[0] === '"' && value[value.length - 1] === '"') {
          value = value.slice(1, -1).replace(/\\(.)/g, '$1');
        }
        pairs.push([name, value]);
      }
    }
    this.#params = new MIMEParams(pairs);
  }
  get type() { return this.#type; }
  set type(v) {
    v = String(v);
    mimeCheckToken('type', v, v);
    this.#type = v.toLowerCase();
  }
  get subtype() { return this.#subtype; }
  set subtype(v) {
    v = String(v);
    mimeCheckToken('subtype', v, v);
    this.#subtype = v.toLowerCase();
  }
  get essence() { return `${this.#type}/${this.#subtype}`; }
  set essence(_) {}
  get params() { return this.#params; }
  toString() {
    const ps = this.#params.toString();
    return ps ? `${this.essence};${ps}` : this.essence;
  }
}

// ── parseArgs（Node lib/util.js 口径；真机 26.8.2 逐项对过码与文案）────────
// strict 下未知项抛 ERR_PARSE_ARGS_UNKNOWN_OPTION；non-strict 下未知长项按
// 有 `=` 进 string、无则进 true（短项未知进 true），未知不抛；
// allowPositionals 缺省 = !strict；`--` 后全进 positionals；
// token 形态：option{kind,name,rawName,index[,value,inlineValue]} /
// positional{kind,index,value} / option-terminator{kind,index}（index 恒为选项位）。
function parseArgs(config = {}) {
  const {
    args = process.argv.slice(2),
    strict = true,
    allowPositionals = !strict,
    tokens = false,
    options = {},
  } = config;
  if (!Array.isArray(args)) throw new ERR_INVALID_ARG_TYPE('args', 'Array', args);
  if (typeof strict !== 'boolean') throw new ERR_INVALID_ARG_TYPE('strict', 'boolean', strict);
  if (typeof allowPositionals !== 'boolean') {
    throw new ERR_INVALID_ARG_TYPE('allowPositionals', 'boolean', allowPositionals);
  }
  if (typeof tokens !== 'boolean') throw new ERR_INVALID_ARG_TYPE('tokens', 'boolean', tokens);
  const shortMap = { __proto__: null };
  for (const longName of Object.keys(options)) {
    const opt = options[longName];
    const pfx = `options.${longName}`;
    if (opt.type !== 'string' && opt.type !== 'boolean') {
      throw new ERR_INVALID_ARG_TYPE(`${pfx}.type`, "('string|boolean')", opt.type);
    }
    if (opt.short !== undefined) {
      if (typeof opt.short !== 'string') {
        throw new ERR_INVALID_ARG_TYPE(`${pfx}.short`, 'string', opt.short);
      }
      if (opt.short.length !== 1) {
        throw new ERR_INVALID_ARG_VALUE(`${pfx}.short`, opt.short, 'must be a single character');
      }
      shortMap[opt.short] = longName;
    }
    if (opt.multiple !== undefined && typeof opt.multiple !== 'boolean') {
      throw new ERR_INVALID_ARG_TYPE(`${pfx}.multiple`, 'boolean', opt.multiple);
    }
    if (opt.default !== undefined) {
      if (opt.multiple) {
        if (!Array.isArray(opt.default)) {
          throw new ERR_INVALID_ARG_TYPE(`${pfx}.default`, 'Array', opt.default);
        }
      } else if (opt.type === 'string' && typeof opt.default !== 'string') {
        throw new ERR_INVALID_ARG_TYPE(`${pfx}.default`, 'string', opt.default);
      } else if (opt.type === 'boolean' && typeof opt.default !== 'boolean') {
        throw new ERR_INVALID_ARG_TYPE(`${pfx}.default`, 'boolean', opt.default);
      }
    }
  }
  const values = { __proto__: null };
  for (const longName of Object.keys(options)) {
    const opt = options[longName];
    if (opt.default !== undefined) {
      values[longName] = opt.multiple ? [...opt.default] : opt.default;
    }
  }
  const setValue = (longName, value) => {
    const opt = options[longName];
    if (opt && opt.multiple) {
      if (!Array.isArray(values[longName])) values[longName] = [];
      values[longName].push(value);
    } else {
      values[longName] = value;
    }
  };
  const positionals = [];
  const toks = [];
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === '--') {
      if (tokens) toks.push({ kind: 'option-terminator', index: i });
      i++;
      for (; i < args.length; i++) {
        if (!allowPositionals) throw new ERR_PARSE_ARGS_UNEXPECTED_POSITIONAL(args[i]);
        positionals.push(args[i]);
        if (tokens) toks.push({ kind: 'positional', index: i, value: args[i] });
      }
      break;
    }
    if (arg.startsWith('--')) {
      const optIndex = i;
      const eq = arg.indexOf('=');
      const name = eq === -1 ? arg.slice(2) : arg.slice(2, eq);
      const inline = eq === -1 ? undefined : arg.slice(eq + 1);
      const opt = options[name];
      if (opt === undefined) {
        if (strict) throw new ERR_PARSE_ARGS_UNKNOWN_OPTION(`--${name}`);
        if (inline !== undefined) {
          values[name] = inline;
          if (tokens) {
            toks.push({ kind: 'option', name, rawName: `--${name}`, index: optIndex, value: inline, inlineValue: true });
          }
        } else {
          values[name] = true;
          if (tokens) toks.push({ kind: 'option', name, rawName: `--${name}`, index: optIndex });
        }
        continue;
      }
      if (opt.type === 'boolean') {
        if (inline !== undefined) {
          throw new ERR_PARSE_ARGS_INVALID_OPTION_VALUE(`Option '--${name}' does not take an argument`);
        }
        setValue(name, true);
        if (tokens) toks.push({ kind: 'option', name, rawName: `--${name}`, index: optIndex });
      } else {
        let value = inline;
        let inlineValue = inline !== undefined;
        if (!inlineValue) {
          if (i + 1 >= args.length) {
            throw new ERR_PARSE_ARGS_INVALID_OPTION_VALUE(`Option '--${name} <value>' argument missing`);
          }
          value = args[i + 1];
          i++;
        }
        setValue(name, value);
        if (tokens) toks.push({ kind: 'option', name, rawName: `--${name}`, index: optIndex, value, inlineValue });
      }
      continue;
    }
    if (arg.length > 1 && arg.charCodeAt(0) === 45) {
      let j = 1;
      while (j < arg.length) {
        const short = arg[j];
        const longName = shortMap[short];
        if (longName === undefined) {
          if (strict) throw new ERR_PARSE_ARGS_UNKNOWN_OPTION(`-${short}`);
          values[short] = true;
          if (tokens) toks.push({ kind: 'option', name: short, rawName: `-${short}`, index: i });
          j++;
          continue;
        }
        const opt = options[longName];
        if (opt.type === 'boolean') {
          setValue(longName, true);
          if (tokens) toks.push({ kind: 'option', name: longName, rawName: `-${short}`, index: i });
          j++;
        } else {
          const optIndex = i;
          let value;
          let inlineValue;
          if (j + 1 < arg.length) {
            value = arg.slice(j + 1);
            inlineValue = true;
          } else {
            if (i + 1 >= args.length) {
              throw new ERR_PARSE_ARGS_INVALID_OPTION_VALUE(`Option '-${short}, --${longName} <value>' argument missing`);
            }
            value = args[i + 1];
            i++;
            inlineValue = false;
          }
          setValue(longName, value);
          if (tokens) toks.push({ kind: 'option', name: longName, rawName: `-${short}`, index: optIndex, value, inlineValue });
          break;
        }
      }
      continue;
    }
    if (!allowPositionals) throw new ERR_PARSE_ARGS_UNEXPECTED_POSITIONAL(arg);
    positionals.push(arg);
    if (tokens) toks.push({ kind: 'positional', index: i, value: arg });
  }
  const result = { values, positionals };
  if (tokens) result.tokens = toks;
  return result;
}

// ── 组装（Node module.exports 形态）───────────────────────────────────────
const _extendDep = deprecate(_extend, 'The `util._extend` API is deprecated. Please use Object.assign() instead.', 'DEP0060');
const isArrayDep = deprecate(isArray, 'The `util.isArray` API is deprecated. Please use `Array.isArray()` instead.', 'DEP0044');
const isBooleanDep = deprecate(isBoolean, 'The `util.isBoolean` API is deprecated. Please use `typeof arg === "boolean"` instead.', 'DEP0045');
const isNullDep = deprecate(isNull, 'The `util.isNull` API is deprecated. Please use `arg === null` instead.', 'DEP0046');
const isNullOrUndefinedDep = deprecate(isNullOrUndefined, 'The `util.isNullOrUndefined` API is deprecated. Please use `arg == null` instead.', 'DEP0047');
const isNumberDep = deprecate(isNumber, 'The `util.isNumber` API is deprecated. Please use `typeof arg === "number"` instead.', 'DEP0048');
const isStringDep = deprecate(isString, 'The `util.isString` API is deprecated. Please use `typeof arg === "string"` instead.', 'DEP0049');
const isSymbolDep = deprecate(isSymbol, 'The `util.isSymbol` API is deprecated. Please use `typeof arg === "symbol"` instead.', 'DEP0050');
const isUndefinedDep = deprecate(isUndefined, 'The `util.isUndefined` API is deprecated. Please use `arg === undefined` instead.', 'DEP0051');
const isRegExpDep = deprecate(isRegExp, 'The `util.isRegExp` API is deprecated. Please use `arg instanceof RegExp` instead.', 'DEP0052');
const isObjectDep = deprecate(isObject, 'The `util.isObject` API is deprecated. Please use `arg !== null && typeof arg === "object"` instead.', 'DEP0053');
const isDateDep = deprecate(isDate, 'The `util.isDate` API is deprecated. Please use `arg instanceof Date` instead.', 'DEP0054');
const isErrorDep = deprecate(isError, 'The `util.isError` API is deprecated. Please use `arg instanceof Error` instead.', 'DEP0055');
const isFunctionDep = deprecate(isFunction, 'The `util.isFunction` API is deprecated. Please use `typeof arg === "function"` instead.', 'DEP0056');
const isPrimitiveDep = deprecate(isPrimitive, 'The `util.isPrimitive` API is deprecated. Please use `arg === null || (typeof arg !== "object" && typeof arg !== "function")` instead.', 'DEP0057');
const isBufferDep = deprecate(isBuffer, 'The `util.isBuffer` API is deprecated. Please use `Buffer.isBuffer()` instead.', 'DEP0038');

const util = {
  _errnoException: undefined, // 需 uv errno 面（9c），先占位 undefined
  _exceptionWithHostPort: undefined,
  _extend: _extendDep,
  callbackify,
  convertProcessSignalToExitCode,
  debug: debuglog,
  debuglog,
  deprecate,
  format,
  formatWithOptions,
  inherits,
  inspect,
  isArray: isArrayDep,
  isBoolean: isBooleanDep,
  isBuffer: isBufferDep,
  isDate: isDateDep,
  isError: isErrorDep,
  isFunction: isFunctionDep,
  isNull: isNullDep,
  isNullOrUndefined: isNullOrUndefinedDep,
  isNumber: isNumberDep,
  isObject: isObjectDep,
  isPrimitive: isPrimitiveDep,
  isRegExp: isRegExpDep,
  isString: isStringDep,
  isSymbol: isSymbolDep,
  isUndefined: isUndefinedDep,
  isDeepStrictEqual,
  parseEnv,
  parseArgs,
  getSystemErrorName,
  getSystemErrorMessage,
  getSystemErrorMap,
  MIMEType,
  MIMEParams,
  promisify,
  stripVTControlCharacters,
  toUSVString,
  types,
  styleText,
};

export default util;
export {
  callbackify,
  convertProcessSignalToExitCode,
  debuglog,
  deprecate,
  format,
  formatWithOptions,
  getSystemErrorMap,
  getSystemErrorMessage,
  getSystemErrorName,
  inherits,
  inspect,
  isDeepStrictEqual,
  MIMEParams,
  MIMEType,
  parseArgs,
  parseEnv,
  promisify,
  stripVTControlCharacters,
  styleText,
  toUSVString,
  types,
  kCustomPromisifiedSymbol as promisify_custom,
};
export { debuglog as debug };
"#;
