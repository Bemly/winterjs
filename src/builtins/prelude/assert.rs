//! 本体断言 JS 面：`WinterJS2.assert`（prelude 自含，Web 形结构化比较）。
//!
//! 与 `node:assert` 语义差（记档）：`deepEqual` 只比结构不比原型
//!（node 比 `[[Prototype]]`）；`strictEqual` 用 `Object.is`；
//! 文案自拼 `Expected … to …`，无 node  diff 体。
pub const ASSERT_JS: &str = r#"
{
  class AssertionError extends Error {
    constructor(msg, actual, expected, operator) {
      super(msg === undefined ? `${operator} failed` : String(msg));
      this.name = "AssertionError";
      this.code = "ERR_ASSERTION";
      this.actual = actual;
      this.expected = expected;
      this.operator = operator;
    }
  }
  const __wjs2_assert_same = (a, b) => {
    if (Object.is(a, b)) return true;
    if (typeof a !== typeof b) return false;
    if (a === null || b === null) return false;
    if (typeof a !== "object") return false;
    if (a instanceof Date || b instanceof Date) {
      return a instanceof Date && b instanceof Date && a.getTime() === b.getTime();
    }
    if (a instanceof RegExp || b instanceof RegExp) {
      return a instanceof RegExp && b instanceof RegExp && a.source === b.source && a.flags === b.flags;
    }
    if (ArrayBuffer.isView(a) || ArrayBuffer.isView(b)) {
      if (!ArrayBuffer.isView(a) || !ArrayBuffer.isView(b)) return false;
      const ua = new Uint8Array(a.buffer, a.byteOffset, a.byteLength);
      const ub = new Uint8Array(b.buffer, b.byteOffset, b.byteLength);
      if (ua.length !== ub.length) return false;
      for (let i = 0; i < ua.length; i++) if (ua[i] !== ub[i]) return false;
      return true;
    }
    if (Array.isArray(a) || Array.isArray(b)) {
      if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) return false;
      for (let i = 0; i < a.length; i++) if (!__wjs2_assert_same(a[i], b[i])) return false;
      return true;
    }
    const ka = Object.keys(a), kb = Object.keys(b);
    if (ka.length !== kb.length) return false;
    for (const k of ka) {
      if (!Object.prototype.hasOwnProperty.call(b, k)) return false;
      if (!__wjs2_assert_same(a[k], b[k])) return false;
    }
    return true;
  };
  const __wjs2_assert_fail = (actual, expected, msg, op) => {
    throw new AssertionError(msg, actual, expected, op);
  };
  const assert = {
    AssertionError,
    ok(v, msg) { if (!v) __wjs2_assert_fail(v, true, msg, "ok"); },
    equal(a, b, msg) { if (a != b) __wjs2_assert_fail(a, b, msg, "=="); },
    notEqual(a, b, msg) { if (a == b) __wjs2_assert_fail(a, b, msg, "!="); },
    strictEqual(a, b, msg) { if (!Object.is(a, b)) __wjs2_assert_fail(a, b, msg, "strictEqual"); },
    notStrictEqual(a, b, msg) { if (Object.is(a, b)) __wjs2_assert_fail(a, b, msg, "notStrictEqual"); },
    deepEqual(a, b, msg) { if (!__wjs2_assert_same(a, b)) __wjs2_assert_fail(a, b, msg, "deepEqual"); },
    notDeepEqual(a, b, msg) { if (__wjs2_assert_same(a, b)) __wjs2_assert_fail(a, b, msg, "notDeepEqual"); },
    throws(fn, msg) {
      if (typeof fn !== "function") throw new TypeError("assert.throws requires a function");
      try { fn(); } catch { return; }
      __wjs2_assert_fail(undefined, "throw", msg, "throws");
    },
    doesNotThrow(fn, msg) {
      if (typeof fn !== "function") throw new TypeError("assert.doesNotThrow requires a function");
      try { fn(); } catch (e) { __wjs2_assert_fail(e, "no throw", msg, "doesNotThrow"); }
    },
    async rejects(fn, msg) {
      if (typeof fn !== "function") throw new TypeError("assert.rejects requires a function");
      try { await fn(); } catch { return; }
      __wjs2_assert_fail(undefined, "reject", msg, "rejects");
    },
    async doesNotReject(fn, msg) {
      if (typeof fn !== "function") throw new TypeError("assert.doesNotReject requires a function");
      try { await fn(); } catch (e) { __wjs2_assert_fail(e, "no reject", msg, "doesNotReject"); }
    },
    fail(msg) { __wjs2_assert_fail(undefined, undefined, msg, "fail"); },
    match(s, re, msg) {
      if (typeof s !== "string" || !(re instanceof RegExp)) throw new TypeError("assert.match requires (string, RegExp)");
      if (!re.test(s)) __wjs2_assert_fail(s, re, msg, "match");
    },
    doesNotMatch(s, re, msg) {
      if (typeof s !== "string" || !(re instanceof RegExp)) throw new TypeError("assert.doesNotMatch requires (string, RegExp)");
      if (re.test(s)) __wjs2_assert_fail(s, re, msg, "doesNotMatch");
    },
  };
  try {
    const W = globalThis.WinterJS2;
    if (W && W.assert === undefined) W.assert = assert;
  } catch {}
}
"#;
