//! tests/node/punycode.rs — 对齐 src/builtins/node/punycode.rs（node:punycode）。

use crate::common::*;
use crate::helpers::*;
use assert_fs::prelude::*;

#[test]
fn phase9a_punycode_rfc3492() {
    // test-punycode.js 命名子集（RFC 3492 向量 + 域名 + ucs2）
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("p.mjs");
    file.write_str(
        r#"import punycode from "node:punycode";
console.log(punycode.encode("bücher"), punycode.decode("bcher-kva"));
console.log(punycode.toASCII("münchen.de"), punycode.toUnicode("xn--mnchen-3ya.de"));
console.log(punycode.toASCII("日本"), punycode.toUnicode("xn--wgv71a"));
console.log(punycode.ucs2.encode([0x1D306]) === "\u{1D306}", punycode.ucs2.decode("a\u{1D306}b").length);
console.log(punycode.toASCII("foo@bücher.de").split("@")[1]);
try { punycode.decode("!!!!!"); } catch (e) { console.log("err", e instanceof RangeError); }
"#,
    )
    .unwrap();
    let out = winterjs2()
        .args(["--run", file.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("bcher-kva bücher"), "out: {out}");
    assert!(out.contains("xn--mnchen-3ya.de münchen.de"), "out: {out}");
    assert!(out.contains("xn--wgv71a 日本"), "out: {out}");
    assert!(out.contains("true 3"), "out: {out}");
    assert!(out.contains("xn--bcher-kva.de"), "out: {out}");
    assert!(out.contains("err true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase10f_punycode_dep0040_warning() {
    // 10f：require('punycode') 发 DEP0040（test-punycode.js 点名；url 改懒加载不断链）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import process from "node:process";
let seen = null;
process.on("warning", (w) => { seen = `${w.name} ${w.code} ${w.message.slice(0, 20)}`; });
const punycode = (await import("node:punycode")).default;
// 真机 26.8.2：warning 经 nextTick 派发，await 续体先跑（此刻仍 null），下一轮才到。
console.log("warn-sync", seen);
await new Promise((r) => setTimeout(r, 1));
console.log("warn", seen);
console.log("works", punycode.encode("ü") === "tda");
await import("node:url");
console.log("url-ok", (await import("node:url")).domainToASCII("münchen.de") === "xn--mnchen-3ya.de");
"#,
    );
    assert!(out.contains("warn-sync null"), "out: {out}");
    assert!(out.contains("warn DeprecationWarning DEP0040 The `punycode` modul"), "out: {out}");
    assert!(out.contains("works true"), "out: {out}");
    assert!(out.contains("url-ok true"), "out: {out}");
    dir.close().unwrap();
}
