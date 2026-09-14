//! tests/node/string_decoder.rs — 对齐 src/builtins/node/string_decoder.rs（node:string_decoder）。

use crate::common::*;
use assert_fs::prelude::*;

#[test]
fn phase9a_string_decoder_encodings() {
    // test-string-decoder.js 命名子集：截断续读/end flush/全编码
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("s.mjs");
    file.write_str(
        r#"import { StringDecoder } from "node:string_decoder";
const utf8 = new StringDecoder("utf8");
let out = "";
out += utf8.write(Buffer.from([0xE4, 0xB8]));
out += utf8.write(Buffer.from([0xAD, "e".charCodeAt(0)]));
out += utf8.end();
console.log("utf8", out);
// invalid 序列 → FFFD 继续
const bad = new StringDecoder("utf8");
console.log("bad", bad.write(Buffer.from([0xFF, 0x41])).includes("\uFFFD"), bad.write(Buffer.from([0x42])));
const u16 = new StringDecoder("utf16le");
let o2 = u16.write(Buffer.from([0x61, 0]));
o2 += u16.write(Buffer.from([0x62]));
o2 += u16.end();
console.log("utf16", o2.length, o2.charCodeAt(1) === 0xFFFD);
const hex = new StringDecoder("hex");
console.log("hex", hex.write(Buffer.from([0xDE, 0xAD])), hex.end());
const b64 = new StringDecoder("base64");
let o3 = b64.write(Buffer.from("foobarb"));
o3 += b64.write(Buffer.from("az"));
o3 += b64.end();
console.log("b64", o3 === Buffer.from("foobarbaz").toString("base64"));
const latin = new StringDecoder("latin1");
console.log("latin1", latin.write(Buffer.from([0xE9, 0x41])), latin.end().length);
const ascii = new StringDecoder("ascii");
console.log("ascii", ascii.write(Buffer.from([0x80, 0x41])).length, ascii.write(Buffer.from([0x41])));
console.log("default", new StringDecoder().encoding, typeof utf8.lastChar, utf8.lastNeed === 0);
console.log("string-in", new StringDecoder().write("direct"));
try { new StringDecoder("nope"); } catch (e) { console.log("e1", e.code); }
try { new StringDecoder("utf8").write(42); } catch (e) { console.log("e2", e.code); }
try { new StringDecoder("utf8").write.call({ __wjsId: undefined }, Buffer.alloc(1)); } catch (e) { console.log("e3", e.code); }
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
    assert!(out.contains("utf8 中e"), "out: {out}");
    assert!(out.contains("bad true B"), "out: {out}");
    assert!(out.contains("utf16 2 true"), "out: {out}");
    assert!(out.contains("hex dead"), "out: {out}");
    assert!(out.contains("b64 true"), "out: {out}");
    assert!(out.contains("latin1 éA 0"), "out: {out}");
    assert!(out.contains("ascii 2 A"), "out: {out}");
    assert!(out.contains("default utf8 object true"), "out: {out}");
    assert!(out.contains("string-in direct"), "out: {out}");
    assert!(
        out.contains("e1 ERR_UNKNOWN_ENCODING") && out.contains("e2 ERR_INVALID_ARG_TYPE"),
        "out: {out}"
    );
    dir.close().unwrap();
}

