//! `node:internal/primordials`——9b 逐字源所用 primordials 名的静态包装表
/// 源：nodejs/node（MIT）对应件最小实现；偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"// 偏差：无防篡改硬化（bun-compat §4.1 口径）；引擎全局直接引用。
const AsyncIteratorPrototype = Object.getPrototypeOf(
  Object.getPrototypeOf(async function* () {}).prototype);

const primordials = {
  ArrayIsArray: Array.isArray,
  ArrayPrototypeIndexOf: (a, v, f) => a.indexOf(v, f),
  ArrayPrototypePop: (a) => a.pop(),
  ArrayPrototypePush: (a, ...v) => a.push(...v),
  ArrayPrototypeSlice: (a, s, e) => a.slice(s, e),
  AsyncIteratorPrototype,
  Boolean,
  Error,
  FunctionPrototypeCall: (fn, thisArg, ...args) => fn.call(thisArg, ...args),
  FunctionPrototypeSymbolHasInstance: (C, v) =>
    Function.prototype[Symbol.hasInstance].call(C, v),
  JSONParse: JSON.parse,
  MathFloor: Math.floor,
  Number,
  NumberIsInteger: Number.isInteger,
  NumberIsNaN: Number.isNaN,
  NumberParseInt: Number.parseInt,
  ObjectDefineProperties: Object.defineProperties,
  ObjectDefineProperty: Object.defineProperty,
  ObjectGetOwnPropertyDescriptor: Object.getOwnPropertyDescriptor,
  ObjectKeys: Object.keys,
  ObjectSetPrototypeOf: Object.setPrototypeOf,
  Promise,
  PromisePrototypeThen: (p, f, r) => p.then(f, r),
  PromiseReject: (e) => Promise.reject(e),
  PromiseResolve: (v) => Promise.resolve(v),
  PromiseWithResolvers: () => {
    let resolve, reject;
    const promise = new Promise((res, rej) => { resolve = res; reject = rej; });
    return { promise, resolve, reject };
  },
  ReflectApply: Reflect.apply,
  ReflectOwnKeys: Reflect.ownKeys,
  SafeSet: Set,
  StringPrototypeToLowerCase: (s) => s.toLowerCase(),
  Symbol,
  SymbolAsyncDispose: Symbol.asyncDispose ?? Symbol.for('Symbol.asyncDispose'),
  SymbolAsyncIterator: Symbol.asyncIterator,
  SymbolDispose: Symbol.dispose ?? Symbol.for('Symbol.dispose'),
  SymbolFor: Symbol.for,
  SymbolHasInstance: Symbol.hasInstance,
  SymbolIterator: Symbol.iterator,
  SymbolSpecies: Symbol.species,
  TypedArrayPrototypeSet: (ta, v, o) => ta.set(v, o),
  Uint8Array,
  ArrayPrototypeSort: (a, f) => a.sort(f),
  BigInt,
  Date,
  DateNow: Date.now,
  JSONStringify: JSON.stringify,
  Map,
  MapPrototypeClear: (m) => m.clear(),
  MapPrototypeDelete: (m, k) => m.delete(k),
  MapPrototypeEntries: (m) => m.entries(),
  MapPrototypeGet: (m, k) => m.get(k),
  MapPrototypeGetSize: (m) => m.size,
  MapPrototypeHas: (m, k) => m.has(k),
  MapPrototypeKeys: (m) => m.keys(),
  MapPrototypeSet: (m, k, v) => m.set(k, v),
  MathMax: Math.max,
  MathMin: Math.min,
  NumberMAX_SAFE_INTEGER: Number.MAX_SAFE_INTEGER,
  StringFromCharCode: String.fromCharCode,
  StringPrototypeEndsWith: (s, x, p) => s.endsWith(x, p),
  SymbolToStringTag: Symbol.toStringTag,
  // R2-iter：stream/iter 面 26 项（node 原文名逐项对位；Safe* 系无硬化直别名，
  // 与既有 SafeSet: Set 同口径；Array.fromAsync 经引擎真值，见 4.65 实测）。
  Array,
  String,
  ObjectEntries: Object.entries,
  ObjectFreeze: Object.freeze,
  ArrayBufferIsView: ArrayBuffer.isView,
  ArrayBufferPrototypeGetByteLength: (b) => b.byteLength,
  ArrayBufferPrototypeSlice: (b, s, e) => b.slice(s, e),
  ArrayFromAsync: Array.fromAsync,
  ArrayPrototypeEvery: (a, f) => a.every(f),
  ArrayPrototypeMap: (a, f) => a.map(f),
  ArrayPrototypeShift: (a) => a.shift(),
  DataViewPrototypeGetBuffer: (d) => d.buffer,
  DataViewPrototypeGetByteLength: (d) => d.byteLength,
  DataViewPrototypeGetByteOffset: (d) => d.byteOffset,
  SafeMap: Map,
  SafeWeakMap: WeakMap,
  SafePromiseAllReturnVoid: (arr) => Promise.all(arr).then(() => undefined),
  SafePromisePrototypeFinally: (p, f) => p.finally(f),
  SafePromiseRace: (arr) => Promise.race(arr),
  StringPrototypeStartsWith: (s, x, p) => s.startsWith(x, p),
  TypedArrayPrototypeFill: (ta, v, s, e) => ta.fill(v, s, e),
  TypedArrayPrototypeGetBuffer: (ta) => ta.buffer,
  TypedArrayPrototypeGetByteLength: (ta) => ta.byteLength,
  TypedArrayPrototypeGetByteOffset: (ta) => ta.byteOffset,
  TypedArrayPrototypeSlice: (ta, s, e) => ta.slice(s, e),
  Uint32Array,
  // R2-iter：unhandled 误报抑制（utils/consumers 管线中转 promise 先挂空
  // reject 分支，真处理器随后即到；与 V8 AddPromiseRejectHandler 同向）。
  markPromiseAsHandled: (p) => { Promise.prototype.then.call(p, undefined, () => {}); },
};

export default primordials;
export { primordials };

"#;
