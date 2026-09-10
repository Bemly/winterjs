//! `node:assert` 起步子集（纯 JS，无 natives）：
//! ok/equal/strict/deep/throws/rejects/fail/ifError/match + AssertionError。
//! deep 相等支持循环（seen 对），Date/RegExp/TypedArray 按值；函数按引用。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
export class AssertionError extends Error {
  constructor(message, actual, expected, operator) {
    super(message);
    this.name = "AssertionError";
    this.code = "ERR_ASSERTION";
    this.actual = actual;
    this.expected = expected;
    this.operator = operator || "==";
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
  throw new AssertionError(
    message || `Expected ${__fmt(actual)} ${operator} ${__fmt(expected)}`,
    actual, expected, operator,
  );
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
  throw new AssertionError(message instanceof Error ? message.message : String(message ?? "Failed"), undefined, undefined, "fail");
}
export function ifError(value) {
  if (value !== null && value !== undefined) {
    throw value instanceof Error ? value : new AssertionError(String(value), value, null, "ifError");
  }
}
function __checkThrow(e, expected, prefix) {
  if (expected === undefined) return;
  let ok = false;
  if (typeof expected === "function") {
    try {
      ok = e instanceof expected;
      if (!ok) ok = !!expected(e);
    } catch {
      ok = false;
    }
  } else if (expected instanceof RegExp) {
    ok = expected.test(String((e && e.message) || e));
  } else if (typeof expected === "object" && expected !== null) {
    ok = Object.entries(expected).every(([k, v]) => (e && e[k]) == v);
  }
  if (!ok) {
    throw new AssertionError(`${prefix}: unexpected throw`, e, expected, prefix);
  }
}
export function throws(fn, expected, message) {
  if (typeof expected === "string") { message = expected; expected = undefined; }
  try {
    fn();
  } catch (e) {
    __checkThrow(e, expected, "throws");
    return;
  }
  __fail(undefined, expected, message || "Missing expected exception", "throws");
}
export function doesNotThrow(fn, message) {
  try {
    fn();
  } catch (e) {
    __fail(e, undefined, message || `Got unwanted exception: ${String((e && e.message) || e)}`, "doesNotThrow");
  }
}
export async function rejects(fn, expected, message) {
  if (typeof expected === "string") { message = expected; expected = undefined; }
  try {
    await fn();
  } catch (e) {
    __checkThrow(e, expected, "rejects");
    return;
  }
  __fail(undefined, expected, message || "Missing expected rejection", "rejects");
}
export async function doesNotReject(fn, message) {
  try {
    await fn();
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
export default { ok, equal, notEqual, strictEqual, notStrictEqual, deepEqual, notDeepEqual, deepStrictEqual, notDeepStrictEqual, throws, doesNotThrow, rejects, doesNotReject, fail, ifError, match, doesNotMatch, strict, AssertionError };
"#;
