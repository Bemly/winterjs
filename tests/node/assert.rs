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

#[test]
fn phase10f_assert_throws_regex_string() {
    // 10f：throws 正则测 String(err)（含名；旧实现只测 message，套件点名）。
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const assert = (await import("node:assert")).default;
assert.throws(() => { const e = new RangeError("Invalid input"); throw e; }, /^RangeError: Invalid input$/);
console.log("regex-name true");
assert.throws(() => { throw new Error("boom"); }, /boom/);
console.log("regex-sub true");"#]));
    assert_eq!(out, "regex-name true\nregex-sub true\n", "assert: {out}");
}

#[test]
fn phase10f_assert_validation_xrealm() {
    // 10f：throws 族参数校验 + AssertionError 构造器校验 + Error 消息跨域重抛。
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const assert = (await import("node:assert")).default;
const vm = (await import("node:vm")).default;
try { assert.throws(42); } catch (e) { console.log("t42", e.code); }
try { assert.doesNotThrow(42); } catch (e) { console.log("dnt42", e.code); }
try { await assert.rejects(42); } catch (e) { console.log("rej42", e.code); }
try { new assert.AssertionError(42); } catch (e) { console.log("ae42", e.code); }
const ctx = vm.createContext({});
const xerr = vm.runInContext("new SyntaxError('custom error')", ctx);
try { assert(false, xerr); } catch (e) { console.log("xrealm", e.name, e.message); }
try { assert.fail(xerr); } catch (e) { console.log("fail-err", e.name); }"#]));
    assert_eq!(
        out,
        "t42 ERR_INVALID_ARG_TYPE\ndnt42 ERR_INVALID_ARG_TYPE\nrej42 ERR_INVALID_ARG_TYPE\nae42 ERR_INVALID_ARG_TYPE\nxrealm SyntaxError custom error\nfail-err SyntaxError\n",
        "assert: {out}"
    );
}
