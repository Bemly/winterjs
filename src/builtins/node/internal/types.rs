//! `node:internal/util/types`（Node `lib/internal/util/types.js` + V8 `internalBinding('types')`
//! 面的标签实现，MIT）。
//!
//! - TypedArray/DataView 系逐字移植（`Symbol.toStringTag` 判据）。
//! - V8 internal 面的其余检查以 toString 标签/instanceof 等价实现（9a+util.types 用）。
//! - 偏差：`isProxy` 纯 JS 不可探测（恒 false）；`isWebAssemblyCompiledModule`
//!   未做（恒 false）；跨 realm 判定以本 realm 构造器为准。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node:internal/util/types + the V8 internalBinding('types') face.
function tagOf(value) {
  return Object.prototype.toString.call(value);
}

function isDataView(value) {
  return ArrayBuffer.isView(value) && value[Symbol.toStringTag] === undefined;
}

function isTypedArray(value) {
  return value[Symbol.toStringTag] !== undefined && ArrayBuffer.isView(value) && !(value instanceof DataView);
}

const typedTags = [
  'Int8Array', 'Uint8Array', 'Uint8ClampedArray', 'Int16Array', 'Uint16Array',
  'Int32Array', 'Uint32Array', 'Float16Array', 'Float32Array', 'Float64Array',
  'BigInt64Array', 'BigUint64Array',
];

function isTypedArrayOf(tag) {
  return (value) => ArrayBuffer.isView(value) && value[Symbol.toStringTag] === tag;
}

const isUint8Array = isTypedArrayOf('Uint8Array');
const isUint8ClampedArray = isTypedArrayOf('Uint8ClampedArray');
const isUint16Array = isTypedArrayOf('Uint16Array');
const isUint32Array = isTypedArrayOf('Uint32Array');
const isInt8Array = isTypedArrayOf('Int8Array');
const isInt16Array = isTypedArrayOf('Int16Array');
const isInt32Array = isTypedArrayOf('Int32Array');
const isFloat16Array = isTypedArrayOf('Float16Array');
const isFloat32Array = isTypedArrayOf('Float32Array');
const isFloat64Array = isTypedArrayOf('Float64Array');
const isBigInt64Array = isTypedArrayOf('BigInt64Array');
const isBigUint64Array = isTypedArrayOf('BigUint64Array');

function isArrayBufferView(value) {
  return ArrayBuffer.isView(value);
}

function isAnyArrayBuffer(value) {
  return value instanceof ArrayBuffer ||
    (typeof SharedArrayBuffer === 'function' && value instanceof SharedArrayBuffer);
}

function isArrayBuffer(value) {
  return value instanceof ArrayBuffer;
}

function isSharedArrayBuffer(value) {
  return typeof SharedArrayBuffer === 'function' && value instanceof SharedArrayBuffer;
}

function isAsyncFunction(value) {
  return tagOf(value) === '[object AsyncFunction]';
}

function isGeneratorFunction(value) {
  const tag = tagOf(value);
  return tag === '[object GeneratorFunction]' || tag === '[object AsyncGeneratorFunction]';
}

function isGeneratorObject(value) {
  const tag = tagOf(value);
  return tag === '[object Generator]' || tag === '[object AsyncGenerator]';
}

function isPromise(value) {
  return value instanceof Promise;
}

function isMap(value) { return value instanceof Map; }
function isSet(value) { return value instanceof Set; }
function isWeakMap(value) { return value instanceof WeakMap; }
function isWeakSet(value) { return value instanceof WeakSet; }
function isDate(value) { return value instanceof Date; }
function isRegExp(value) { return value instanceof RegExp; }

function isMapIterator(value) { return tagOf(value) === '[object Map Iterator]'; }
function isSetIterator(value) { return tagOf(value) === '[object Set Iterator]'; }
function isArgumentsObject(value) { return tagOf(value) === '[object Arguments]'; }
function isBooleanObject(value) { return typeof value === 'object' && value !== null && tagOf(value) === '[object Boolean]'; }
function isNumberObject(value) { return typeof value === 'object' && value !== null && tagOf(value) === '[object Number]'; }
function isStringObject(value) { return typeof value === 'object' && value !== null && tagOf(value) === '[object String]'; }
function isSymbolObject(value) { return typeof value === 'object' && value !== null && tagOf(value) === '[object Symbol]'; }
function isBigIntObject(value) { return typeof value === 'object' && value !== null && tagOf(value) === '[object BigInt]'; }

function isBoxedPrimitive(value) {
  return isBooleanObject(value) || isNumberObject(value) || isStringObject(value) ||
    isSymbolObject(value) || isBigIntObject(value);
}

const errorConstructors = [
  Error, TypeError, RangeError, SyntaxError, URIError, EvalError, ReferenceError,
  ...(typeof AggregateError === 'function' ? [AggregateError] : []),
];

function isNativeError(value) {
  if (typeof value !== 'object' || value === null) return false;
  return errorConstructors.some((ctor) => value instanceof ctor);
}

function isModuleNamespaceObject(value) { return tagOf(value) === '[object Module]'; }

// 偏差：纯 JS 无法探测 Proxy（V8/JSC internal flag）；恒 false（bun-compat §1 记录）。
function isProxy(_value) { return false; }

// 偏差：WebAssembly 编译模块检查未做（无引擎接口）；恒 false。
function isWebAssemblyCompiledModule(_value) { return false; }

export {
  isArrayBufferView,
  isDataView,
  isTypedArray,
  isUint8Array,
  isUint8ClampedArray,
  isUint16Array,
  isUint32Array,
  isInt8Array,
  isInt16Array,
  isInt32Array,
  isFloat16Array,
  isFloat32Array,
  isFloat64Array,
  isBigInt64Array,
  isBigUint64Array,
  isAnyArrayBuffer,
  isArrayBuffer,
  isSharedArrayBuffer,
  isAsyncFunction,
  isGeneratorFunction,
  isGeneratorObject,
  isPromise,
  isMap,
  isSet,
  isWeakMap,
  isWeakSet,
  isDate,
  isRegExp,
  isMapIterator,
  isSetIterator,
  isArgumentsObject,
  isBooleanObject,
  isNumberObject,
  isStringObject,
  isSymbolObject,
  isBigIntObject,
  isBoxedPrimitive,
  isNativeError,
  isModuleNamespaceObject,
  isProxy,
  isWebAssemblyCompiledModule,
};
export default {
  isArrayBufferView, isDataView, isTypedArray, isUint8Array, isUint8ClampedArray,
  isUint16Array, isUint32Array, isInt8Array, isInt16Array, isInt32Array,
  isFloat16Array, isFloat32Array, isFloat64Array, isBigInt64Array, isBigUint64Array,
  isAnyArrayBuffer, isArrayBuffer, isSharedArrayBuffer, isAsyncFunction,
  isGeneratorFunction, isGeneratorObject, isPromise, isMap, isSet, isWeakMap,
  isWeakSet, isDate, isRegExp, isMapIterator, isSetIterator, isArgumentsObject,
  isBooleanObject, isNumberObject, isStringObject, isSymbolObject, isBigIntObject,
  isBoxedPrimitive, isNativeError, isModuleNamespaceObject, isProxy,
  isWebAssemblyCompiledModule,
};
"#;
