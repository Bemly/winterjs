//! tests/node/querystring.rs — 对齐 src/builtins/node/querystring.rs（node:querystring）。

use crate::common::*;
use assert_fs::prelude::*;

#[test]
fn phase9a_querystring_roundtrip() {
    // test-querystring.js 命名子集
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("q.mjs");
    file.write_str(
        r#"import qs from "node:querystring";
console.log(JSON.stringify(qs.parse("a=1&b=x%20y&b=2&c")));
console.log(qs.stringify({ a: "x y", b: [1, 2] }));
console.log(qs.escape("ä b"), qs.unescape("%C3%A4+b"));
console.log(JSON.stringify(qs.parse("a=1;a=2", ";", "=")));
console.log(Object.keys(qs.parse("a=1&b=2&c=3", null, null, { maxKeys: 2 })).length);
// 自定义 enc/dec
const p = qs.parse("a=%20", null, null, { decodeURIComponent: (s) => s });
console.log(JSON.stringify(p));
// 边界：非字符串入参 → 空对象；maxKeys=1 截断
console.log(JSON.stringify(qs.parse(null)), JSON.stringify(qs.parse("")));
console.log(typeof qs.parse("a=1&b=2", null, null, { maxKeys: 1 }).a);
console.log("fallback", qs.unescape("%E0%A4%A").includes("%"), qs.unescapeBuffer("a+b", true).toString());
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
    assert!(
        out.contains(r#"{"a":"1","b":["x y","2"],"c":""}"#),
        "out: {out}"
    );
    assert!(out.contains("a=x%20y&b=1&b=2"), "out: {out}");
    assert!(out.contains("%C3%A4%20b ä+b"), "out: {out}");
    assert!(
        out.contains(r#"{"a":["1","2"]}"#),
        "out: {out}"
    );
    assert!(out.contains("2"), "out: {out}");
    assert!(out.contains(r#"{"a":"%20"}"#), "out: {out}");
    assert!(out.contains("{} {}"), "out: {out}");
    assert!(out.contains("fallback true a b"), "out: {out}");
    dir.close().unwrap();
}
