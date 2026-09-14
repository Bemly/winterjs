//! tests/node/util.rs — 对齐 src/builtins/node/util.rs（node:util（含 parseEnv））。

use crate::common::*;
use assert_fs::prelude::*;

#[test]
fn phase9a_util_format_inspect_inherits() {
    // test-util-format.js / test-util-inspect.js 命名子集
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("u.mjs");
    file.write_str(
        r#"import util, { format, inspect, inherits, stripVTControlCharacters } from "node:util";
console.log(format("%s=%d", "a", 1.5), format("%%"), format("%j", { x: 1 }));
console.log(format("%i:%f", 3.9, "1.5"));
console.log(util.inspect({ a: 1, b: [1, 2], c: new Map([["k", 1]]) }));
console.log(util.inspect("it's"), inspect(new Date(0).toISOString() === inspect(new Date(0)) ? { s: "q'a" } : 0));
class Base { base() { return 1; } }
class Sub {}
inherits(Sub, Base);
console.log("inh", new Sub().base(), Sub.super_ === Base);
console.log("strip", stripVTControlCharacters("\u001b[31mred\u001b[39m"), stripVTControlCharacters("plain"));
console.log("promise-inp", util.inspect(Promise.resolve()));
"#,
    )
    .unwrap();
    let out = winterjs()
        .args(["--run", file.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("a=1.5 %% {\"x\":1}"), "out: {out}");
    assert!(out.contains("3:1"), "out: {out}");
    assert!(
        out.contains("{ a: 1, b: [ 1, 2 ], c: Map(1) { 'k' => 1 } }"),
        "out: {out}"
    );
    assert!(out.contains("inh 1 true"), "out: {out}");
    assert!(out.contains("strip red plain"), "out: {out}");
    assert!(out.contains("Promise { <pending> }"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_util_promisify_callbackify_deep_equal() {
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("p.mjs");
    file.write_str(
        r#"import { promisify, callbackify, isDeepStrictEqual } from "node:util";
import assert from "node:assert";
const pb = promisify((a, cb) => cb(null, a * 2));
console.log("prom", await pb(21));
// multi-value + customPromisifyArgs
const pr = promisify((cb) => cb(null, "bytesRead", "buffer"));
pr[Symbol.for("nodejs.util.promisify.custom")] // 不设 custom
console.log("prom-obj", JSON.stringify(await (async () => {
  const fn = promisify((cb) => cb(null, 1, 2));
  fn[Symbol("customPromisifyArgs")] = ["a", "b"];
  const fn2 = promisify((cb) => { fn2args(cb); }) ; return 0;
})()));
// promisify.custom 通道
const raw = (a, cb) => cb(null, a);
raw[Symbol.for("nodejs.util.promisify.custom")] = (a) => Promise.resolve(a + 100);
console.log("custom", await promisify(raw)(1));
// callbackify 正常 + falsy rejection
const cf = callbackify(async (n) => n + 1);
const [err, val] = await new Promise((res) => cf(1, (e, v) => res([e, v])));
console.log("cb", err, val);
const cf2 = callbackify(async () => { throw null; });
await new Promise((res) => cf2((e) => res(console.log("falsy", e?.code, e instanceof Error))));
// isDeepStrictEqual 语义（test-assert 依赖同款）
console.log("eq", isDeepStrictEqual({ a: [1, { b: 2 }] }, { a: [1, { b: 2 }] }));
console.log("neq0", isDeepStrictEqual(1, "1"), isDeepStrictEqual({ a: 1 }, { a: 1, b: undefined }));
console.log("nan0", isDeepStrictEqual(NaN, NaN), isDeepStrictEqual(0, -0));
console.log("map", isDeepStrictEqual(new Map([[1, "a"]]), new Map([[1, "a"]])));
console.log("proto", isDeepStrictEqual(Object.create(null, { x: { value: 1, enumerable: true } }), { x: 1 }));
// 边界：非函数入参
try { promisify(42); } catch (e) { console.log("err1", e.code, e.message.includes("must be of type function")); }
try { callbackify("x"); } catch (e) { console.log("err2", e.code); }
assert.strictEqual(typeof promisify.custom, "symbol");
"#,
    )
    .unwrap();
    let out = winterjs()
        .args(["--run", file.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("prom 42"), "out: {out}");
    assert!(out.contains("custom 101"), "out: {out}");
    assert!(out.contains("cb null 2"), "out: {out}");
    assert!(
        out.contains("falsy ERR_FALSY_VALUE_REJECTION true"),
        "out: {out}"
    );
    assert!(out.contains("eq true"), "out: {out}");
    assert!(out.contains("neq0 false false"), "out: {out}");
    assert!(out.contains("nan0 true false"), "out: {out}");
    assert!(
        out.contains("map true") && out.contains("proto false"),
        "out: {out}"
    );
    assert!(out.contains("err1 ERR_INVALID_ARG_TYPE true"), "out: {out}");
    assert!(out.contains("err2 ERR_INVALID_ARG_TYPE"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9j_util_parse_env() {
    // 真机差分钉住（node 26.8.2 四组探针全同）：引号/注释/export/重复/排序/多行。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("pe.mjs");
    file.write_str(
        r##"
import { parseEnv } from "node:util";
console.log(JSON.stringify(parseEnv("B=2\nA=1")));
console.log(JSON.stringify(parseEnv("# c\nexport C=3\nD='a#b'\nE=\"x\\nY\"\nF=v # t\nG=\"m\nn\"\nH=\"q\"q")));
console.log(JSON.stringify(parseEnv("X=one\nX=two\n=v\nnoeq")));
try { parseEnv(42); } catch (e) { console.log("pe-t", e.code); }
"##,
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
    for line in [
        "{\"A\":\"1\",\"B\":\"2\"}",
        "{\"C\":\"3\",\"D\":\"a#b\",\"E\":\"x\\nY\",\"F\":\"v\",\"G\":\"m\\nn\",\"H\":\"q\"}",
        "{\"X\":\"two\"}",
        "pe-t ERR_INVALID_ARG_TYPE",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}
