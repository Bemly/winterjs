//! tests/node/assert.rs — 对齐 src/builtins/node/assert.rs（node:assert）。

use crate::common::*;

#[test]
fn phase4_node_assert_subset() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const assert = (await import("node:assert")).default; assert.ok(1); assert.strictEqual(1, 1); assert.notStrictEqual(1, "1"); assert.deepStrictEqual({ a: [1, 2] }, { a: [1, 2] }); assert.equal(1, "1"); assert.throws(() => { throw new TypeError("x"); }, TypeError); assert.throws(() => { throw new Error("boom"); }, /boom/); await assert.rejects(async () => { throw new Error("r"); }); assert.match("foobar", /^foo/); assert.ifError(null); console.log("assert-ok"); try { assert.strictEqual(1, 2); } catch (e) { console.log(e.code, e.operator, e.actual, e.expected); }"#]));
    assert_eq!(
        out, "assert-ok\nERR_ASSERTION strictEqual 1 2\n",
        "assert: {out}"
    );
}

#[test]
fn phase10f_assert_rejects_promise_or_fn() {
    // 10f：rejects/doesNotReject 收 promise 或函数（旧实现只收函数，套件点名抓到）。
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const assert = (await import("node:assert")).default;
await assert.rejects(Promise.reject(new TypeError("p")), TypeError);
await assert.rejects(async () => { throw new RangeError("f"); }, { code: undefined });
let threw = false;
try { await assert.rejects(Promise.reject(new TypeError("p")), RangeError); } catch (e) { threw = e.code === "ERR_ASSERTION"; }
console.log("mismatch", threw);
await assert.doesNotReject(Promise.resolve(1));
console.log("done");"#]));
    assert_eq!(out, "mismatch true\ndone\n", "assert: {out}");
}
