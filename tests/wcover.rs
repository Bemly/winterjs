//! 原生覆盖 B1（WinterJS.assert/util/punycode；Web 风）。
//! 正常 + 报错 + 边界三件；后续批次追加同文件。

mod common;

use common::*;

fn eval_ok(code: &str) -> String {
    let out = winterjs().args(["--eval", code]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn wcover_b1_assert_faces() {
    let stdout = eval_ok(
        r#"WinterJS.assert.ok(true); WinterJS.assert.equal(1, "1"); WinterJS.assert.strictEqual(1, 1); WinterJS.assert.deepEqual({ a: [1, { b: 2 }] }, { a: [1, { b: 2 }] }); WinterJS.assert.throws(() => { throw new Error("x"); }); await WinterJS.assert.rejects(async () => { throw new Error("x"); }); WinterJS.assert.match("foobar", /oob/); console.log("assert-faces-ok");"#,
    );
    assert!(stdout.contains("assert-faces-ok"), "out: {stdout}");
}

#[test]
fn wcover_b1_util_puny_faces() {
    let stdout = eval_ok(
        r#"console.log("fmt", WinterJS.util.format("%s=%d %j", "a", 1, { x: 1 })); console.log("insp", WinterJS.util.inspect({ a: 1 }).includes("a: 1")); console.log("puny", WinterJS.punycode.toASCII("münchen.de"), WinterJS.punycode.toUnicode("xn--mnchen-3ya.de"), WinterJS.punycode.encode("bücher"), WinterJS.punycode.decode("bcher-kva")); console.log("ucs2", JSON.stringify(WinterJS.punycode.ucs2.decode("hi")));"#,
    );
    assert!(stdout.contains("fmt a=1 {\"x\":1}") || stdout.contains("fmt a=1"), "out: {stdout}");
    assert!(stdout.contains("insp true"), "out: {stdout}");
    assert!(stdout.contains("puny xn--mnchen-3ya.de münchen.de"), "out: {stdout}");
    assert!(stdout.contains("ucs2 [104,105]"), "out: {stdout}");
}

#[test]
fn wcover_b1_errors_boundary() {
    let stdout = eval_ok(
        r#"const t = (n, f) => { try { const r = f(); if (r && r.then) { r.then(() => console.log(n, "NO-THROW"), () => console.log(n, "THROW", "AssertionError")); } else console.log(n, "NO-THROW"); } catch (e) { console.log(n, "THROW", e.name); } }; t("ok", () => WinterJS.assert.ok(false)); t("eq", () => WinterJS.assert.strictEqual(1, "1")); t("deep", () => WinterJS.assert.deepEqual({ a: 1 }, { a: 2 })); t("throws", () => WinterJS.assert.throws(() => {})); t("notthrows", () => WinterJS.assert.doesNotThrow(() => { throw new Error("x"); })); t("match", () => WinterJS.assert.match("foo", /z/)); t("nan", () => WinterJS.assert.deepEqual(NaN, NaN));"#,
    );
    for name in ["ok", "eq", "deep", "throws", "notthrows", "match"] {
        assert!(stdout.contains(&format!("{name} THROW AssertionError")), "out: {stdout}");
    }
    // NaN 自等（Object.is 口径）不抛。
    assert!(stdout.contains("nan NO-THROW"), "out: {stdout}");
}
