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
