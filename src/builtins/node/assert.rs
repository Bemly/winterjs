//! `node:assert` 起步子集（纯 JS，无 natives）：
//! ok/equal/strict/deep/throws/rejects/fail/ifError/match + AssertionError。
//! deep 相等支持循环（seen 对），Date/RegExp/TypedArray 按值；函数按引用。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
export class AssertionError extends Error {
  constructor(options) {
    // 10f：真机只收 options 对象（非对象即 ERR_INVALID_ARG_TYPE 文案逐字，套件点名）。
    if (options !== undefined && (typeof options !== "object" || options === null)) {
      __errInvalidArg("options", "object", options);
    }
    const message = options?.message;
    super(message);
    this.name = "AssertionError";
    this.code = "ERR_ASSERTION";
    this.actual = options?.actual;
    this.expected = options?.expected;
    this.operator = options?.operator || "==";
  }
}
function __fmt(v) {
  try {
    if (typeof v === "string") return `'${v}'`;
    if (typeof v === "function") return `[Function ${(v.name || "anonymous")}]`;
    return JSON.stringify(v) ?? String(v);
  } catch {
    return String(v);
  }
}
function __fail(actual, expected, message, operator) {
  // 10f：Error 消息原样重抛（含跨域，`toString` tag 透出 `[object Error]`；
  // 真机 `isError` 同款语义，test-assert.js 点名）。
  if (__isErr(message)) throw message;
  throw new AssertionError({
    message: message || `Expected ${__fmt(actual)} ${operator} ${__fmt(expected)}`,
    actual, expected, operator,
  });
}
function __isErr(v) {
  if (v === null || (typeof v !== 'object' && typeof v !== 'function')) return false;
  try {
    if (v instanceof Error) return true;
  } catch {}
  try {
    return Object.prototype.toString.call(v) === '[object Error]';
  } catch {
    return false;
  }
}
function __isObj(v) { return typeof v === "object" && v !== null; }
function __deep(a, b, strict, seen) {
  if (Object.is(a, b)) return true;
  if (!strict && a == b) return true;
  if (!__isObj(a) || !__isObj(b)) return false;
  for (const [x, y] of seen) if (x === a && y === b) return true;
  seen.push([a, b]);
  if (a instanceof Date || b instanceof Date) {
    return a instanceof Date && b instanceof Date && a.getTime() === b.getTime();
  }
  if (a instanceof RegExp || b instanceof RegExp) {
    return a instanceof RegExp && b instanceof RegExp && a.source === b.source && a.flags === b.flags;
  }
  if (ArrayBuffer.isView(a) || ArrayBuffer.isView(b)) {
    if (!ArrayBuffer.isView(a) || !ArrayBuffer.isView(b)) return false;
    const x = new Uint8Array(a.buffer, a.byteOffset, a.byteLength);
    const y = new Uint8Array(b.buffer, b.byteOffset, b.byteLength);
    if (x.length !== y.length) return false;
    return x.every((v, i) => v === y[i]);
  }
  if (Array.isArray(a) || Array.isArray(b)) {
    if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) return false;
    return a.every((v, i) => __deep(v, b[i], strict, seen));
  }
  if (strict && Object.getPrototypeOf(a) !== Object.getPrototypeOf(b)) return false;
  const ka = Object.keys(a), kb = Object.keys(b);
  if (ka.length !== kb.length) return false;
  return ka.every((k) => Object.hasOwn(b, k) && __deep(a[k], b[k], strict, seen));
}
export function ok(value, message) {
  if (!value) __fail(value, true, message, "==");
}
export function equal(actual, expected, message) {
  // eslint-disable-next-line eqeqeq
  if (actual != expected) __fail(actual, expected, message, "==");
}
export function notEqual(actual, expected, message) {
  // eslint-disable-next-line eqeqeq
  if (actual == expected) __fail(actual, expected, message, "!=");
}
export function strictEqual(actual, expected, message) {
  if (!Object.is(actual, expected)) __fail(actual, expected, message, "strictEqual");
}
export function notStrictEqual(actual, expected, message) {
  if (Object.is(actual, expected)) __fail(actual, expected, message, "notStrictEqual");
}
export function deepEqual(actual, expected, message) {
  if (!__deep(actual, expected, false, [])) __fail(actual, expected, message, "deepEqual");
}
export function notDeepEqual(actual, expected, message) {
  if (__deep(actual, expected, false, [])) __fail(actual, expected, message, "notDeepEqual");
}
export function deepStrictEqual(actual, expected, message) {
  if (!__deep(actual, expected, true, [])) __fail(actual, expected, message, "deepStrictEqual");
}
export function notDeepStrictEqual(actual, expected, message) {
  if (__deep(actual, expected, true, [])) __fail(actual, expected, message, "notDeepStrictEqual");
}
export function fail(message) {
  // 10f：Error 消息原样重抛（含跨域；真机口径）。
  if (__isErr(message)) throw message;
  throw new AssertionError({ message: String(message ?? "Failed"), operator: "fail" });
}
export function ifError(value) {
  if (value !== null && value !== undefined) {
    // 10f：跨域 Error 同样重抛（`instanceof` 跨 compartment 恒 false，见 §4.57）。
    if (__isErr(value)) throw value;
    throw new AssertionError({ message: String(value), actual: value, expected: null, operator: "ifError" });
  }
}
function __checkThrow(e, expected, prefix) {
  if (expected === undefined) return;
  let ok = false;
  if (typeof expected === "function") {
    // 10f：真机口径——先 instanceof，不过再当校验函数调且须严格回 true
    //（旧实现 `!!expected(e)` 把构造器调用的真值对象当通过）。
    try {
      ok = e instanceof expected;
      if (!ok) ok = expected(e) === true;
    } catch {
      ok = false;
    }
  } else if (expected instanceof RegExp) {
    // 10f：真机测 String(err)（"RangeError: Invalid input" 含名；旧实现只测 message）。
    let s;
    try {
      s = String(e);
    } catch {
      s = String((e && e.message) || e);
    }
    ok = expected.test(s);
  } else if (typeof expected === "object" && expected !== null) {
    // 10f：真机口径——实际值为 string 且期望为正则时做正则匹配
    //（`{ message: /re/ }` 形；旧实现 `==` 永假，os.getPriority 用例现形）。
    ok = Object.entries(expected).every(([k, v]) => {
      const a = e ? e[k] : undefined;
      if (typeof a === "string" && v instanceof RegExp) return v.test(a);
      // eslint-disable-next-line eqeqeq
      return a == v;
    });
  }
  if (!ok) {
    throw new AssertionError({ message: `${prefix}: unexpected throw`, actual: e, expected, operator: prefix });
  }
}
function __needFn(fn, what) {
  // 10f：throws 族只收函数（真机 ERR_INVALID_ARG_TYPE 文案逐字，套件点名）。
  if (typeof fn !== "function") {
    __errInvalidArg(what, "function", fn);
  }
}
function __received(v) {
  // 真机 invalidArgTypeHelper 口径（test-assert.js 逐字断言）。
  if (v === null) return "null";
  if (v === undefined) return "undefined";
  const t = typeof v;
  if (t === "string") return `type string ('${v}')`;
  if (t === "object") {
    const n = v.constructor && v.constructor.name ? v.constructor.name : "Object";
    return `an instance of ${n}`;
  }
  return `type ${t} (${String(v)})`;
}
function __errInvalidArg(name, expected, actual) {
  const err = new TypeError(`The "${name}" argument must be of type ${expected}. Received ${__received(actual)}`);
  err.code = "ERR_INVALID_ARG_TYPE";
  throw err;
}
export function throws(fn, expected, message) {
  if (typeof expected === "string") { message = expected; expected = undefined; }
  __needFn(fn, "fn");
  try {
    fn();
  } catch (e) {
    __checkThrow(e, expected, "throws");
    return;
  }
  __fail(undefined, expected, message || "Missing expected exception", "throws");
}
export function doesNotThrow(fn, message) {
  __needFn(fn, "fn");
  try {
    fn();
  } catch (e) {
    __fail(e, undefined, message || `Got unwanted exception: ${String((e && e.message) || e)}`, "doesNotThrow");
  }
}
export async function rejects(fn, expected, message) {
  // 10f：真机收 promise 或函数（旧实现只收函数，套件点名抓到）。
  if (typeof expected === "string") { message = expected; expected = undefined; }
  if (typeof fn !== "function" && (!fn || (typeof fn.then !== "function"))) {
    __errInvalidArg("promiseFn", "function or an instance of Promise", fn);
  }
  const p = typeof fn === "function" ? fn() : fn;
  try {
    await p;
  } catch (e) {
    __checkThrow(e, expected, "rejects");
    return;
  }
  __fail(undefined, expected, message || "Missing expected rejection", "rejects");
}
export async function doesNotReject(fn, message) {
  if (typeof fn !== "function" && (!fn || (typeof fn.then !== "function"))) {
    __errInvalidArg("promiseFn", "function or an instance of Promise", fn);
  }
  const p = typeof fn === "function" ? fn() : fn;
  try {
    await p;
  } catch (e) {
    __fail(e, undefined, message || `Got unwanted rejection: ${String((e && e.message) || e)}`, "doesNotReject");
  }
}
export function match(value, reg, message) {
  if (!reg.test(String(value))) __fail(value, reg, message, "match");
}
export function doesNotMatch(value, reg, message) {
  if (reg.test(String(value))) __fail(value, reg, message, "doesNotMatch");
}
const strict = { equal: strictEqual, deepEqual: deepStrictEqual, ok, fail, ifError, throws, rejects, doesNotThrow, doesNotReject, match, doesNotMatch };
export { strict };
// 默认导出即可调用函数（真机口径：`typeof require('assert') === 'function'`；
// 但 `default.ok !== default`——`ok` 是独立函数（与 `strict.ok` 同一，名 `ok`），
// 默认本体名 `assert`；M5 vitest 牵引：reporter 直调 `assert(cond, msg)`）。
function assert(value, message) { return ok(value, message); }
const __default = Object.assign(assert, { ok, equal, notEqual, strictEqual, notStrictEqual, deepEqual, notDeepEqual, deepStrictEqual, notDeepStrictEqual, throws, doesNotThrow, rejects, doesNotReject, fail, ifError, match, doesNotMatch, strict, AssertionError });
export default __default;
"#;
