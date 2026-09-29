//! tests/node/buffer.rs — 对齐 src/builtins/node/buffer.rs（node:buffer + 全局 Blob）。

use crate::common::*;
use crate::helpers::*;

#[test]
fn node_buffer_global() {
    // 正常：from/toString(hex/base64/utf8)/concat/alloc/byteLength；报错：坏hex/未知编码；
    // 边界：allocUnsafe零填/subarray保持Buffer/compare/equals/copy/write/toJSON。
    let code = r#"const b = Buffer.from("hello");
if (b.toString("hex") !== "68656c6c6f" || b.toString("base64") !== "aGVsbG8=") throw new Error("basic failed");
if (!Buffer.isBuffer(b) || Buffer.isBuffer(new Uint8Array(1))) throw new Error("isBuffer failed");
if (Buffer.byteLength("€") !== 3) throw new Error("byteLength failed");
if (Buffer.concat([Buffer.from("a"), Buffer.from("b")]).toString() !== "ab") throw new Error("concat failed");
if (Buffer.alloc(4, "ab").toString() !== "abab") throw new Error("alloc fill failed");
if (Buffer.from([104, 105]).toString() !== "hi") throw new Error("array failed");
if (Buffer.from("ff", "hex")[0] !== 255) throw new Error("hex failed");
if (Buffer.from("aGVsbG8=", "base64").toString() !== "hello") throw new Error("b64 failed");
const z = Buffer.allocUnsafe(8);
if (z.length !== 8 || ![...z].every((x) => x === 0)) throw new Error("allocUnsafe must be zeroed");
const s = b.subarray(1, 3);
if (!(s instanceof Buffer) || s.toString() !== "el") throw new Error("subarray failed");
if (Buffer.compare(Buffer.from("a"), Buffer.from("b")) >= 0) throw new Error("compare failed");
if (!b.equals(Buffer.from("hello"))) throw new Error("equals failed");
const t = Buffer.alloc(5);
if (b.copy(t, 1) !== 4 || t.slice(1).toString() !== "hell") throw new Error("copy failed");
const w = Buffer.alloc(8);
if (w.write("hi", 2) !== 2 || w.slice(2, 4).toString() !== "hi") throw new Error("write failed");
if (JSON.parse(JSON.stringify(b)).type !== "Buffer") throw new Error("toJSON failed");
// fs 互操作：Buffer 进出 writeFile/readFile
// 10f：坏 hex 不抛（真机 26 口径：非 hex 字符截断/空回，与旧实现抛错不同）；
if (Buffer.from("zz", "hex").length !== 0) throw new Error("bad-hex should be empty");
try { Buffer.from("x", "nope-enc"); throw new Error("must throw"); }
catch (e) { if (!String(e.message).includes("encoding")) throw e; }
console.log("buffer-ok");
"#;
    assert_eq!(
        stdout_of(&mut winterjs2().args(["--eval", code])),
        "buffer-ok\n"
    );
}

#[test]
fn phase9b_buffer_module_surface() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import buffer, { Buffer, constants, kMaxLength, INSPECT_MAX_BYTES } from "node:buffer";
// from + 编码面（口径：hex/base64/base64url/utf8/latin1/ascii/utf16le）
const b = Buffer.from("hi", "utf8");
console.log("enc", b.toString("hex"), b.toString("base64"), b.toString("base64url"),
  b.toString("utf8"), b.toString("latin1"), b.toString("ascii"));
// 10f：ascii 解码掩 0x7F（真机口径；旧实现与 latin1 同形）。
console.log("ascii-mask", JSON.stringify(Buffer.from([0x81, 0x99, 0xE5, 0xC0]).toString("ascii")));
// statics
console.log("blen", Buffer.byteLength("héllo"), Buffer.byteLength(new ArrayBuffer(4)),
  Buffer.byteLength(new Uint8Array(3)), Buffer.byteLength("中", "utf16le"));
console.log("isbuf", Buffer.isBuffer(b), Buffer.isBuffer(new Uint8Array(1)), b instanceof Uint8Array);
console.log("concat", Buffer.concat([Buffer.from("ab"), Buffer.from("cd"), Buffer.from("e")]).toString(),
  Buffer.concat([]).length, Buffer.concat([Buffer.from("abcde")], 2).toString());
console.log("cmp", Buffer.compare(Buffer.from("b"), Buffer.from("a")), Buffer.compare(Buffer.from("a"), Buffer.from("a")));
console.log("alloc", JSON.stringify([...Buffer.alloc(3, 1)]), JSON.stringify([...Buffer.alloc(3, "ab", "utf8")]),
  JSON.stringify([...Buffer.alloc(2, 7)]));
console.log("unsafe", JSON.stringify([...Buffer.allocUnsafe(2)]), Buffer.allocUnsafeSlow(3).length);
// prototype
const t = Buffer.alloc(8); t.write("abcd", 2);
console.log("write", t.toString("latin1").replace(/\0/g, "."), t.toJSON().type);
const s = t.subarray(2, 6); s[0] = 120;
console.log("subarray-share", t[2] === 120, t.slice(2, 6).length);
const d = Buffer.alloc(4); t.copy(d, 0, 2, 6);
console.log("copy", d.toString("latin1"));
console.log("eq", Buffer.from("x").equals(Buffer.from("x")), Buffer.from("x").equals(Buffer.from("y")));
// 模块面（10f：SlowBuffer 真机 26 已移除，断言 undefined）
console.log("mod", typeof buffer.Buffer, constants.MAX_LENGTH === kMaxLength, INSPECT_MAX_BYTES,
  typeof buffer.SlowBuffer, buffer.kStringMaxLength > 0);
// 报错三件
try { Buffer.from(42); } catch (e) { console.log("e1", e.constructor.name); }
try { Buffer.alloc(-1); } catch (e) { console.log("e2", e.constructor.name); }
try { Buffer.concat("no"); } catch (e) { console.log("e3", e.constructor.name); }
try { Buffer.byteLength(42); } catch (e) { console.log("e4", e.constructor.name); }
try { Buffer.alloc(1).write("x", -1); } catch (e) { console.log("e5", e.constructor.name); }
try { Buffer.alloc(1).copy("no"); } catch (e) { console.log("e6", e.constructor.name); }
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("enc 6869 aGk= aGk hi hi hi"), "out: {out}");
    assert!(out.contains("ascii-mask \"\\u0001\\u0019e@\""), "out: {out}");
    assert!(out.contains("blen 6 4 3 2"), "out: {out}");
    assert!(out.contains("isbuf true false true"), "out: {out}");
    assert!(out.contains("concat abcde 0 ab"), "out: {out}");
    assert!(out.contains("cmp 1 0"), "out: {out}");
    assert!(out.contains("alloc [1,1,1] [97,98,97] [7,7]"), "out: {out}");
    assert!(out.contains("unsafe [0,0] 3"), "out: {out}");
    assert!(out.contains("write ..abcd.. Buffer"), "out: {out}");
    assert!(out.contains("subarray-share true 4"), "out: {out}");
    assert!(out.contains("copy xbcd"), "out: {out}");
    assert!(out.contains("eq true false"), "out: {out}");
    assert!(out.contains("mod function true 50 undefined true"), "out: {out}");
    assert!(out.contains("e1 TypeError"), "out: {out}");
    assert!(out.contains("e2 RangeError"), "out: {out}");
    assert!(out.contains("e3 TypeError"), "out: {out}");
    assert!(out.contains("e4 TypeError"), "out: {out}");
    assert!(out.contains("e5 RangeError"), "out: {out}");
    assert!(out.contains("e6 TypeError"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9b_buffer_int_rw() {
    // 定长整数/浮点读写系（M5 dev 实测 `writeUInt16BE is not a function` 后补齐，
    // sourcemap 等链路直调；DataView 直通，越界/值域即 RangeError）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { Buffer } from "node:buffer";
const b = Buffer.alloc(28);
console.log("w",
  b.writeUInt8(0xAB, 0), b.writeUInt16LE(0xCDEF, 1), b.writeUInt16BE(0x1234, 3),
  b.writeUInt32LE(0x89ABCDEF, 5), b.writeUInt32BE(0x13579BDF, 9),
  b.writeInt8(-5, 13), b.writeInt16BE(-300, 14), b.writeInt32LE(-70000, 16),
  b.writeBigUInt64BE(12345678901234567890n, 20));
console.log("r",
  b.readUInt8(0).toString(16), b.readUInt16LE(1).toString(16), b.readUInt16BE(3).toString(16),
  b.readUInt32LE(5).toString(16), b.readUInt32BE(9).toString(16),
  b.readInt8(13), b.readInt16BE(14), b.readInt32LE(16), b.readBigUInt64BE(20).toString());
const f = Buffer.alloc(12);
f.writeFloatLE(0.5, 0); f.writeDoubleBE(Math.PI, 4);
console.log("f", f.readFloatLE(0) === 0.5, f.readDoubleBE(4) === Math.PI);
// 报错三件：越界读/越界写/值域
try { b.readUInt16BE(27); } catch (e) { console.log("e1", e.constructor.name); }
try { b.writeUInt32LE(1, 25); } catch (e) { console.log("e2", e.constructor.name); }
try { b.writeUInt8(999, 0); } catch (e) { console.log("e3", e.constructor.name); }
try { b.writeInt8(-200, 0); } catch (e) { console.log("e4", e.constructor.name); }
// 边界：0 偏移默认 + 返回值为下一偏移
const z = Buffer.alloc(4);
console.log("z", z.writeUInt16BE(1), z.readUInt16BE(0));
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("w 1 3 5 9 13 14 16 20 28"), "out: {out}");
    assert!(out.contains("r ab cdef 1234 89abcdef 13579bdf -5 -300 -70000 12345678901234567890"), "out: {out}");
    assert!(out.contains("f true true"), "out: {out}");
    assert!(out.contains("e1 RangeError"), "out: {out}");
    assert!(out.contains("e2 RangeError"), "out: {out}");
    assert!(out.contains("e3 RangeError"), "out: {out}");
    assert!(out.contains("e4 RangeError"), "out: {out}");
    assert!(out.contains("z 2 1"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9b_blob_global_surface() {
    // 9b-1 补的全局 Blob（Web spec 语义，text/arrayBuffer/bytes/slice/stream）
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
const blob = new Blob(["he", new Uint8Array([108, 108, 111]), new ArrayBuffer(0)], { type: "Text/PLAIN" });
console.log("blob", blob.size, blob.type);
console.log("text", await blob.text());
const ab = await blob.arrayBuffer();
console.log("ab", ab.byteLength, new Uint8Array(ab)[0]);
const u8 = await blob.bytes();
console.log("bytes", u8.length, u8 instanceof Uint8Array);
const s1 = blob.slice(2, 4, "a/b");
console.log("slice", s1.size, s1.type, await s1.text());
console.log("neg", blob.slice(-1).size, blob.slice(2, 100).size);
const reader = blob.stream().getReader();
let n = 0;
while (true) { const { done, value } = await reader.read(); if (done) break; n += value.length; }
console.log("stream", n, blob instanceof Blob);
console.log("nested", new Blob([blob, "zz"]).size, new Blob().size);
try { new Blob(42); } catch (e) { console.log("e1", e.constructor.name); }
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("blob 5 text/plain"), "out: {out}");
    assert!(out.contains("text hello"), "out: {out}");
    assert!(out.contains("ab 5 104"), "out: {out}");
    assert!(out.contains("bytes 5 true"), "out: {out}");
    assert!(out.contains("slice 2 a/b ll"), "out: {out}");
    assert!(out.contains("neg 1 3"), "out: {out}");
    assert!(out.contains("stream 5 true"), "out: {out}");
    assert!(out.contains("nested 7 0"), "out: {out}");
    assert!(out.contains("e1 TypeError"), "out: {out}");
    dir.close().unwrap();
}

// ── Phase 9b-2/3：Readable / Writable 核心 + Duplex / Transform / pipeline ──

#[test]
fn node_buffer_encoding_validators() {
    // 正常/边界/报错三件。真机 26.8.2 对拍：node:buffer 校验器全集就
    // isAscii/isUtf8（isUtf16* 非 Node 面，超集误加已删，§4.65 纪律）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import buffer, { isAscii, isUtf8 } from "node:buffer";
console.log("fns", typeof isAscii, typeof isUtf8, typeof buffer.isUtf16Le);
console.log("ascii", isAscii(Buffer.from("hi")), isAscii(Buffer.from([0x80])), isAscii(new Uint8Array([65])));
console.log("utf8", isUtf8(Buffer.from("héllo")), isUtf8(Buffer.from([0xc3])), isUtf8(Buffer.from([0x28])));
const ab = new ArrayBuffer(2);
console.log("view", isUtf8(new Uint8Array(ab)), isAscii(ab));
try { isUtf8("str"); console.log("NO-ERR"); }
catch (e) { console.log("err", e.constructor.name); }
console.log("filemod", typeof buffer.File);
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    for line in [
        "fns function function undefined",
        "ascii true false true",
        "utf8 true false true",
        "view true true",
        "err TypeError",
        "filemod function",
    ] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_buffer_parity_fixes() {
    // 10f buffer 对拍牵引的回归（test-buffer-* 套件门）：
    // 正常：伪 AB 品牌拒收/真 AB 直通/transfer detach 后 isAscii 真/池共享/
    //   INSPECT_MAX_BYTES 具名 50/Uint8Array 子类化透传；
    // 报错：池 postMessage DataCloneError(25)/池 transfer TypeError；
    // 边界：INSPECT_MAX_BYTES 负值 RangeError/空串池化不崩。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import buffer, { Buffer, INSPECT_MAX_BYTES } from "node:buffer";
import { MessageChannel } from "node:worker_threads";
// 品牌：伪 AB（原型链伪造）拒收，真 AB 放行
function AB() {}
Object.setPrototypeOf(AB, ArrayBuffer);
Object.setPrototypeOf(AB.prototype, ArrayBuffer.prototype);
try { Buffer.from(new AB()); console.log("brand BAD"); }
catch (e) { console.log("brand", e.code === "ERR_INVALID_ARG_TYPE", /an instance of AB/.test(e.message)); }
console.log("real", Buffer.from(new ArrayBuffer(5)).length === 5);
// detach：transfer 即归零，isAscii/isUtf8 视空为真
{
  const ab = new ArrayBuffer(1);
  const ta = new Uint8Array(ab); ta[0] = 0xff;
  const { isAscii } = buffer;
  console.log("pre", isAscii(ab) === false);
  structuredClone(ab, { transfer: [ab] });
  console.log("detached", ab.byteLength === 0, ta.length === 0, isAscii(ab) === true);
}
// 池：小串共享池 AB；postMessage 拒收 25；transfer 拒收 TypeError；事后仍共享
{
  const a = Buffer.from("hello world");
  const b = Buffer.from("hello world");
  console.log("pool-share", a.buffer === b.buffer, a.length === 11);
  const { port1 } = new MessageChannel();
  try { port1.postMessage(a, [a.buffer]); console.log("post BAD"); }
  catch (e) { console.log("post", e.name === "DataCloneError", e.code === 25); }
  console.log("still", a.buffer === b.buffer, a.length === 11);
  try { a.buffer.transfer(); console.log("xfer BAD"); }
  catch (e) { console.log("xfer", e.name === "TypeError"); }
  console.log("still2", a.buffer === b.buffer);
}
// INSPECT_MAX_BYTES：具名 50；负值 RangeError（边界）
console.log("imb", INSPECT_MAX_BYTES === 50, buffer.INSPECT_MAX_BYTES === 50);
try { buffer.INSPECT_MAX_BYTES = -1; console.log("imb-set BAD"); }
catch (e) { console.log("imb-err", e.constructor.name === "RangeError"); }
// Proxy newTarget：用户子类化不断链（stream fromWeb 回归）
{
  class E extends Uint8Array {}
  const e = new E(new ArrayBuffer(4), 0, 2);
  console.log("subclass", Object.getPrototypeOf(e) === E.prototype, e.constructor.name === "E");
}
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    for line in [
        "brand true true",
        "real true",
        "pre true",
        "detached true true true",
        "pool-share true true",
        "post true true",
        "still true true",
        "xfer true",
        "still2 true",
        "imb true true",
        "imb-err true",
        "subclass true true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
    dir.close().unwrap();
}
