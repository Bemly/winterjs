//! tests/node/assert.rs — 对齐 src/builtins/node/assert.rs（node:assert）。

use crate::common::*;
use assert_fs::prelude::*;

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

#[test]
fn phase10f_assert_throws_object_regex() {
    // 10f：throws 对象形态中正则期望按匹配语义（旧 `==` 永假；os.getPriority 用例现形）。
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const assert = (await import("node:assert")).default;
assert.throws(() => { const e = new Error("boom-x"); e.code = "E_X"; throw e; }, { code: "E_X", message: /boom/, name: "Error" });
assert.throws(() => { throw new TypeError("bad input"); }, { name: "TypeError", message: /bad/ });
let bad = false;
try { assert.throws(() => { throw new Error("nope"); }, { message: /zzz/ }); } catch (e) { bad = e.code === "ERR_ASSERTION"; }
console.log("regex-obj", bad);
try { assert.throws(() => { throw new Error("nope"); }, { code: "E_MISSING" }); } catch (e) { console.log("code-mismatch", e.code === "ERR_ASSERTION"); }"#]));
    assert_eq!(out, "regex-obj true\ncode-mismatch true\n", "assert: {out}");
}

#[test]
fn phase10f_throws_arrow_validator() {
    // 10f：函数形期望的 instanceof 门——箭头函数无 prototype，instanceof
    // 须以 Error 子类为门，否则校验器永不到达（§4.99）。正常+报错+边界。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("a.mjs");
    file.write_str(
        r#"
import assert from "node:assert";
const L = [];
// 箭头校验器：instanceof 形 + 码形
assert.throws(() => { decodeURIComponent("%E0%A4%A"); }, (e) => e instanceof URIError);
try { assert.throws(() => {}, (e) => e instanceof URIError); } catch (e) { L.push("nofn " + (e.code === "ERR_ASSERTION" || e instanceof assert.AssertionError)); }
// 校验器返回 false → unexpected throw
try { assert.throws(() => { throw new Error("x"); }, () => false); } catch (e) { L.push("rej " + (e instanceof assert.AssertionError)); }
// 类校验器（Error 子类 instanceof 门）不受影响
try { assert.throws(() => { throw new TypeError("t"); }, (e) => e instanceof URIError); } catch (e) { L.push("cls-rej " + (e instanceof assert.AssertionError)); }
// 函数形真值非 true（如返回对象）不通过（真机 === true 口径）
try { assert.throws(() => { throw new Error("y"); }, () => ({})); } catch (e) { L.push("truthy " + (e instanceof assert.AssertionError)); }
console.log(L.join("\n"));
"#,
    )
    .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(file.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    for line in ["nofn true", "rej true", "cls-rej true", "truthy true"] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}
