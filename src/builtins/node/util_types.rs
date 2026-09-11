//! `node:util/types`（Node `lib/util.js` 的 types 面 re-export，MIT）。
//! 实现在 `node:internal/util/types`（types.rs），此处为公开模块面。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
import types from 'node:internal/util/types';
export default types;
export const {
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
} = types;
"#;
