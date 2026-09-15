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

#[test]
fn phase10a_util_parse_args_mime_errno() {
    // 10a：parseArgs/MIMEType/getSystemError*（真机 26.8.2 探针结论转断言）。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("u10.mjs");
    file.write_str(
        r##"
import util, { parseArgs, MIMEType, getSystemErrorName, getSystemErrorMessage, getSystemErrorMap } from "node:util";
// parseArgs 正常
console.log("pa-base", JSON.stringify(parseArgs({ args: ["--port", "8080", "--verbose", "pos1"], options: { port: { type: "string" }, verbose: { type: "boolean" } }, allowPositionals: true })));
console.log("pa-def", JSON.stringify(parseArgs({ args: [], options: { p: { type: "string", default: "d" }, m: { type: "string", multiple: true, default: ["a"] } } })));
console.log("pa-multi", JSON.stringify(parseArgs({ args: ["--m", "1", "--m", "2"], options: { m: { type: "string", multiple: true } } }).values));
console.log("pa-short", JSON.stringify(parseArgs({ args: ["-ab", "-p8080"], options: { a: { type: "boolean", short: "a" }, b: { type: "boolean", short: "b" }, port: { type: "string", short: "p" } } }).values));
console.log("pa-dd", JSON.stringify(parseArgs({ args: ["--", "--a"], options: { a: { type: "boolean" } }, allowPositionals: true }).positionals));
console.log("pa-ns", JSON.stringify(parseArgs({ args: ["--x=1", "-z"], options: {}, strict: false })));
console.log("pa-tok", JSON.stringify(parseArgs({ args: ["--port=8080", "pos"], options: { port: { type: "string" } }, allowPositionals: true, tokens: true }).tokens));
// parseArgs 报错
for (const [tag, fn] of [
  ["pa-unknown", () => parseArgs({ args: ["--nope"], options: { a: { type: "boolean" } } })],
  ["pa-noval", () => parseArgs({ args: ["--a"], options: { a: { type: "string" } } })],
  ["pa-pos", () => parseArgs({ args: ["pos"], options: {} })],
  ["pa-boolval", () => parseArgs({ args: ["--v=false"], options: { v: { type: "boolean" } } })],
  ["pa-shortmissing", () => parseArgs({ args: ["-p"], options: { port: { type: "string", short: "p" } } })],
  ["pa-badtype", () => parseArgs({ args: [], options: { a: { type: "number" } } })],
  ["pa-badshort", () => parseArgs({ args: [], options: { a: { type: "boolean", short: "ab" } } })],
  ["pa-baddef", () => parseArgs({ args: [], options: { m: { type: "boolean", multiple: true, default: true } } })],
]) {
  try { fn(); console.log(tag, "NO-THROW"); }
  catch (e) { console.log(tag, e.code, e instanceof TypeError, e.message); }
}
// MIME 正常
const m = new MIMEType("Text/HTML; Charset=UTF-8; x=1");
console.log("mi-base", m.type, m.subtype, m.essence, m.params.get("charset"), String(m));
console.log("mi-set", (() => { m.type = "application"; m.subtype = "json"; return String(m); })());
console.log("mi-ess", (() => { m.essence = "text/plain"; return String(m); })());
console.log("mi-params", (() => { const p = new MIMEType("text/html; a=1; b=2").params; return [String(p.size), p.get("a"), p.has("c")].join(",") + " " + [...p.keys()].join(",") + " " + String(p); })());
console.log("mi-del", (() => { const p = new MIMEType("text/html; a=1").params; return p.delete("a") + "/" + p.delete("z"); })());
console.log("mi-quote", String(new MIMEType("text/html; a=\"b c\"")));
// MIME 报错
for (const [tag, fn] of [
  ["mi-t1", () => new MIMEType("nope")],
  ["mi-t2", () => new MIMEType("A B/c")],
  ["mi-t3", () => new MIMEType("a/")],
  ["mi-t4", () => { new MIMEType("text/html").subtype = "BAD TYPE"; }],
  ["mi-t5", () => new MIMEType("text/html").params.set("", "x")],
  ["mi-t6", () => new MIMEType(42)],
]) {
  try { fn(); console.log(tag, "NO-THROW"); }
  catch (e) { console.log(tag, e.code, e instanceof TypeError, e.message); }
}
// errno 正常 + 报错 + 边界
console.log("en-size", getSystemErrorMap().size);
console.log("en-name", getSystemErrorName(-2), getSystemErrorName(-9999));
console.log("en-msg", getSystemErrorMessage(-2), getSystemErrorMessage(-9999));
console.log("en-ident", getSystemErrorMap() === getSystemErrorMap());
console.log("en-def", util.getSystemErrorName(-13));
for (const [tag, fn] of [
  ["en-t0", () => getSystemErrorName(0)],
  ["en-tm", () => getSystemErrorMessage(0)],
  ["en-ts", () => getSystemErrorName("x")],
]) {
  try { fn(); console.log(tag, "NO-THROW"); }
  catch (e) { console.log(tag, e.code, e instanceof RangeError, e.message); }
}
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
        "pa-base {\"values\":{\"port\":\"8080\",\"verbose\":true},\"positionals\":[\"pos1\"]}",
        "pa-def {\"values\":{\"p\":\"d\",\"m\":[\"a\"]},\"positionals\":[]}",
        "pa-multi {\"m\":[\"1\",\"2\"]}",
        "pa-short {\"a\":true,\"b\":true,\"port\":\"8080\"}",
        "pa-dd [\"--a\"]",
        "pa-ns {\"values\":{\"x\":\"1\",\"z\":true},\"positionals\":[]}",
        "pa-tok [{\"kind\":\"option\",\"name\":\"port\",\"rawName\":\"--port\",\"index\":0,\"value\":\"8080\",\"inlineValue\":true},{\"kind\":\"positional\",\"index\":1,\"value\":\"pos\"}]",
        "pa-unknown ERR_PARSE_ARGS_UNKNOWN_OPTION true Unknown option '--nope'",
        "pa-noval ERR_PARSE_ARGS_INVALID_OPTION_VALUE true Option '--a <value>' argument missing",
        "pa-pos ERR_PARSE_ARGS_UNEXPECTED_POSITIONAL true Unexpected argument 'pos'. This command does not take positional arguments",
        "pa-boolval ERR_PARSE_ARGS_INVALID_OPTION_VALUE true Option '--v' does not take an argument",
        "pa-shortmissing ERR_PARSE_ARGS_INVALID_OPTION_VALUE true Option '-p, --port <value>' argument missing",
        "pa-badtype ERR_INVALID_ARG_TYPE true The \"options.a.type\" property must be ('string|boolean'). Received type string ('number')",
        "pa-badshort ERR_INVALID_ARG_VALUE true The property 'options.a.short' must be a single character. Received 'ab'",
        "pa-baddef ERR_INVALID_ARG_TYPE true The \"options.m.default\" property must be an instance of Array. Received type boolean (true)",
        "mi-base text html text/html UTF-8 text/html;charset=UTF-8;x=1",
        "mi-set application/json;charset=UTF-8;x=1",
        "mi-ess application/json;charset=UTF-8;x=1",
        "mi-params undefined,1,false a,b a=1;b=2",
        "mi-del undefined/undefined",
        "mi-quote text/html;a=\"b c\"",
        "mi-t1 ERR_INVALID_MIME_SYNTAX true The MIME syntax for a type in \"nope\" is invalid",
        "mi-t2 ERR_INVALID_MIME_SYNTAX true The MIME syntax for a type in \"A B/c\" is invalid at 1",
        "mi-t3 ERR_INVALID_MIME_SYNTAX true The MIME syntax for a subtype in \"a/\" is invalid",
        "mi-t4 ERR_INVALID_MIME_SYNTAX true The MIME syntax for a subtype in \"BAD TYPE\" is invalid at 3",
        "mi-t5 ERR_INVALID_MIME_SYNTAX true The MIME syntax for a parameter name in \"\" is invalid",
        "mi-t6 ERR_INVALID_ARG_TYPE true The \"input\" argument must be of type string. Received type number (42)",
        "en-size 85",
        "en-name ENOENT Unknown system error -9999",
        "en-msg no such file or directory Unknown system error -9999",
        "en-ident false",
        "en-def EACCES",
        "en-t0 ERR_OUT_OF_RANGE true The value of \"err\" is out of range. It must be a negative integer. Received 0",
        "en-tm ERR_OUT_OF_RANGE true The value of \"err\" is out of range. It must be a negative integer. Received 0",
        "en-ts ERR_INVALID_ARG_TYPE false The \"err\" argument must be of type number. Received type string ('x')",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10a_sys_alias() {
    // 10a：`node:sys` 是 util 的废弃别名——import 与 require 同实例，无运行时警告。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("s.mjs");
    file.write_str(
        r#"import sysDefault, { format } from "node:sys";
import utilDefault from "node:util";
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const sysReq = require("sys");
const utilReq = require("util");
console.log("fmt", sysDefault.format("%s", "ok"), format("%d", 7));
console.log("same-import", sysDefault === utilDefault);
console.log("same-require", sysReq === utilReq);
console.log("same-cross", sysDefault === utilReq);
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
    for line in [
        "fmt ok 7",
        "same-import true",
        "same-require true",
        "same-cross true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}
