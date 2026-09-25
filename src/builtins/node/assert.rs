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
  // 10f test-runner-custom-assertions：真机文案逐字（`Expected values to be
  // strictly equal` 前缀 + 实际值行；套件用正则钉住）。
  if (!Object.is(actual, expected)) {
    __fail(actual, expected,
      message || `Expected values to be strictly equal:\n\n${__fmt(actual)} !== ${__fmt(expected)}\n`,
      "strictEqual");
  }
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
    // 10f：真机口径（lib/assert.js expectedException）：先 `actual instanceof
    // expected`（Error 本体亦走此门——`Error.prototype instanceof Error` 为
    // false，旧门把裸 Error 踢进校验函数分支，`Error(e)` 回对象≠true 永假，
    // setlocaladdress 全线现形）；再判是否为 Error 构造器族（是则直接不过，
    // 不当校验器调）；余下才当校验函数调且须严格回 true。
    // 箭头函数无 prototype，`instanceof` 右值即抛——先验门再运算（§4.99）。
    const __isErrorCtor = (f) => {
      let p = f;
      while (p !== null && p !== undefined) {
        if (p === Error) return true;
        p = Object.getPrototypeOf(p);
      }
      return false;
    };
    try {
      if (expected.prototype !== undefined && e instanceof expected) {
        ok = true;
      } else if (!__isErrorCtor(expected)) {
        ok = expected(e) === true;
      } else {
        ok = false;
      }
    } catch {
      ok = false;
    }
  } else if (expected instanceof RegExp) {
    // 10f：真机测 String(err)；V8 的 String(带码错) 含 `[CODE]`
    // （如 `RangeError [ERR_OUT_OF_RANGE]: …`），SM 无——此处补齐后测，
    // 全局 toString 不动（爆破半径最小，dns max-timeout 套件门）。
    // 补钉（opendir 套件）：仅 ERR_* 码进 name 括号——errno 系（ENOENT/ENOTDIR）
    // 真机 name 恒裸 'Error'，误补即 `/Error: ENOTDIR: …/` 正则全灭。
    let s;
    try {
      s = String(e);
      if (e && typeof e.code === "string" && e.code.startsWith("ERR_") && !s.includes(`[${e.code}]`)) {
        s = `${e.constructor?.name ?? "Error"} [${e.code}]: ${e.message ?? ""}`;
      }
    } catch {
      s = String((e && e.message) || e);
    }
    ok = expected.test(s);
  } else if (typeof expected === "object" && expected !== null) {
    // 10f：真机口径——实际值为 string 且期望为正则时做正则匹配
    //（`{ message: /re/ }` 形；旧实现 `==` 永假，os.getPriority 用例现形）。
    // 逐键：原始值 ObjectIs 严格等；对象值 deepStrictEqual（node
    // expectedException 口径——DOMException cause 等实例按深度比较，
    // exec abortcontroller 套件 `{ name, cause: new DOMException(...) }` 点名）。
    ok = Object.entries(expected).every(([k, v]) => {
      const a = e ? e[k] : undefined;
      if (typeof a === "string" && v instanceof RegExp) return v.test(a);
      if (v !== null && typeof v === "object") {
        try { return __deep(a, v, true, []); } catch { return false; }
      }
      // ObjectIs（NaN 与 NaN 等值——node 逐字口径）。
      if (typeof a === "number" && typeof v === "number") {
        if (Number.isNaN(a) && Number.isNaN(v)) return true;
      }
      return a === v;
    });
  }
  if (!ok) {
    // 排障提速（2026-09-25）：文案带上实际抛出的错与首个不符键——修前只有
    // "unexpected throw"，每次都要插桩才知道抛了什么。
    let got;
    try {
      got = e instanceof Error
        ? `${e.name}${e.code !== undefined ? ` [${e.code}]` : ""}: ${e.message}`
        : String(e);
    } catch { got = "<unprintable>"; }
    let why = "";
    if (typeof expected === "object" && expected !== null && !(expected instanceof RegExp)) {
      for (const [k, v] of Object.entries(expected)) {
        let a;
        try { a = e ? e[k] : undefined; } catch { a = "<throws>"; }
        const same = (typeof a === "string" && v instanceof RegExp) ? v.test(a) : a === v;
        if (!same && !(v !== null && typeof v === "object")) {
          why = `; key "${k}": got ${JSON.stringify(a)} expected ${v instanceof RegExp ? String(v) : JSON.stringify(v)}`;
          break;
        }
      }
    }
    throw new AssertionError({
      message: `${prefix}: unexpected throw (got ${got}${why})`,
      actual: e, expected, operator: prefix,
    });
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
