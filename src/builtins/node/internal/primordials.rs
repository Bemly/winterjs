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
};

export default primordials;
export { primordials };

"#;
