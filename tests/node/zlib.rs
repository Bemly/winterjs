//! tests/node/zlib.rs — 对齐 src/builtins/node/zlib.rs（node:zlib）。

use crate::helpers::*;

#[test]
fn phase9d_zlib_sync_roundtrip() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import z, {
  deflateSync, inflateSync, deflateRawSync, inflateRawSync,
  gzipSync, gunzipSync, unzipSync, brotliCompressSync, brotliDecompressSync,
  zstdCompressSync, zstdDecompressSync, constants, codes,
} from "node:zlib";
const s = "the quick brown fox jumps over the lazy dog. ".repeat(40);
const pairs = [
  ["deflate", deflateSync, inflateSync],
  ["deflateRaw", deflateRawSync, inflateRawSync],
  ["gzip", gzipSync, gunzipSync],
  ["brotli", brotliCompressSync, brotliDecompressSync],
  ["zstd", zstdCompressSync, zstdDecompressSync],
];
for (const [name, enc, dec] of pairs) {
  const c = enc(s);
  const back = dec(c);
  console.log(name, c.length < s.length, Buffer.isBuffer(c), back.toString() === s);
}
// 输入形态：string / Uint8Array / ArrayBuffer / DataView
console.log("u8", gunzipSync(gzipSync(new TextEncoder().encode(s))).toString() === s);
console.log("ab", gunzipSync(gzipSync(new TextEncoder().encode(s).buffer)).toString() === s);
console.log("dv", gunzipSync(gzipSync(new DataView(new TextEncoder().encode(s).buffer))).toString() === s);
// unzip 自动识别 gzip 与 zlib 包裹
console.log("unzip", unzipSync(gzipSync(s)).toString() === s, unzipSync(deflateSync(s)).toString() === s);
// level 生效：0（stored）大于默认压缩体积
console.log("level", gzipSync(s, { level: 0 }).length > gzipSync(s).length);
// brotli params[1]（BROTLI_PARAM_QUALITY）与 quality 等效
const a = brotliCompressSync(s, { quality: 1 });
const b = brotliCompressSync(s, { params: { 1: 1 } });
console.log("brotli-q", a.length === b.length, brotliDecompressSync(b).toString() === s);
// constants / codes / 顶层别名（Node 口径）
console.log("const", constants.Z_OK === 0, constants.Z_DATA_ERROR === -3,
  constants.Z_BEST_COMPRESSION === 9, constants.Z_DEFAULT_COMPRESSION === -1,
  constants.BROTLI_OPERATION_PROCESS === 0, constants.BROTLI_PARAM_QUALITY === 1,
  constants.BROTLI_MAX_QUALITY === 11);
console.log("codes", codes.Z_DATA_ERROR === -3, codes[-3] === "Z_DATA_ERROR", codes[0] === "Z_OK");
console.log("alias", z.Z_OK === 0, z.Z_STREAM_END === 1, z.Z_SYNC_FLUSH === 2);
console.log("ns", typeof z.deflate === "function", typeof z.gunzipSync === "function");
"#,
    );
    assert!(out.contains("deflate true true true"), "out: {out}");
    assert!(out.contains("deflateRaw true true true"), "out: {out}");
    assert!(out.contains("gzip true true true"), "out: {out}");
    assert!(out.contains("brotli true true true"), "out: {out}");
    assert!(out.contains("zstd true true true"), "out: {out}");
    assert!(out.contains("u8 true"), "out: {out}");
    assert!(out.contains("ab true"), "out: {out}");
    assert!(out.contains("dv true"), "out: {out}");
    assert!(out.contains("unzip true true"), "out: {out}");
    assert!(out.contains("level true"), "out: {out}");
    assert!(out.contains("brotli-q true true"), "out: {out}");
    assert!(out.contains("const true true true true true true true"), "out: {out}");
    assert!(out.contains("codes true true true"), "out: {out}");
    assert!(out.contains("alias true true true"), "out: {out}");
    assert!(out.contains("ns true true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9d_zlib_async_callback() {
    // 回调链严格嵌套（§4.33：独立异步链交错即 flaky）
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import z from "node:zlib";
const s = "async zlib chain ".repeat(60);
z.gzip(s, (e1, c1) => {
  console.log("gzip", !e1, Buffer.isBuffer(c1));
  z.gunzip(c1, (e2, b1) => {
    console.log("gunzip", !e2, String(b1) === s);
    z.deflate(s, { level: 9 }, (e3, c2) => {
      console.log("deflate", !e3);
      z.inflate(c2, (e4, b2) => {
        console.log("inflate", !e4, String(b2) === s);
        z.brotliCompress(s, (e5, c3) => {
          console.log("brotliC", !e5);
          z.brotliDecompress(c3, (e6, b3) => {
            console.log("brotliD", !e6, String(b3) === s);
            z.zstdCompress(s, (e7, c4) => {
              console.log("zstdC", !e7);
              z.zstdDecompress(c4, (e8, b4) => {
                console.log("zstdD", !e8, String(b4) === s);
                // 回调内错误路径：坏输入进 err，不抛
                z.gunzip(Buffer.from("garbage-in-garbage-out!!!!!!!!!!!!"), (e9, b5) => {
                  console.log("bad", !!e9, e9.code, e9.errno, b5 === undefined);
                  console.log("done");
                });
              });
            });
          });
        });
      });
    });
  });
});
"#,
    );
    assert!(out.contains("gzip true true"), "out: {out}");
    assert!(out.contains("gunzip true true"), "out: {out}");
    assert!(out.contains("deflate true"), "out: {out}");
    assert!(out.contains("inflate true true"), "out: {out}");
    assert!(out.contains("brotliC true"), "out: {out}");
    assert!(out.contains("brotliD true true"), "out: {out}");
    assert!(out.contains("zstdC true"), "out: {out}");
    assert!(out.contains("zstdD true true"), "out: {out}");
    assert!(out.contains("bad true Z_DATA_ERROR -3 true"), "out: {out}");
    assert!(out.contains("done"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9d_zlib_errors_boundary() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { gunzipSync, inflateSync, gzipSync, brotliCompressSync, brotliDecompressSync, gzip, unzipSync } from "node:zlib";
// 报错：坏输入 code/errno 形状
for (const [name, fn] of [["gunzip", gunzipSync], ["inflate", inflateSync], ["unzip", unzipSync], ["brotliD", brotliDecompressSync]]) {
  try { fn(Buffer.from("definitely not compressed data at all!!!")); console.log(name, "no-throw"); }
  catch (e) { console.log(name, e.code, e.errno, e instanceof Error); }
}
// 报错：越界 level/quality → ERR_OUT_OF_RANGE（直通不套 zlib 形）
for (const [name, fn] of [["lv-hi", () => gzipSync("x", { level: 10 })], ["lv-lo", () => gzipSync("x", { level: -2 })], ["q-hi", () => brotliCompressSync("x", { quality: 12 })]]) {
  try { fn(); console.log(name, "no-throw"); }
  catch (e) { console.log(name, e.code, e instanceof RangeError); }
}
// 报错：缺回调同步抛 TypeError；错输入类型同步抛 TypeError
try { gzip("x"); } catch (e) { console.log("nocb", e.constructor.name === "TypeError"); }
try { gzipSync(123); } catch (e) { console.log("badin", e.constructor.name === "TypeError"); }
// 边界：空输入往返；单字节；大块 1MB
console.log("empty", gunzipSync(gzipSync("")).length === 0);
console.log("one", gunzipSync(gzipSync("Q")).toString() === "Q");
const big = "0123456789abcdef".repeat(65536);
console.log("big", gunzipSync(gzipSync(big)).toString() === big);
console.log("stored", gunzipSync(gzipSync(big, { level: 0 })).toString() === big);
"#,
    );
    assert!(out.contains("gunzip Z_DATA_ERROR -3 true"), "out: {out}");
    assert!(out.contains("inflate Z_DATA_ERROR -3 true"), "out: {out}");
    assert!(out.contains("unzip Z_DATA_ERROR -3 true"), "out: {out}");
    assert!(out.contains("brotliD Z_DATA_ERROR -3 true"), "out: {out}");
    assert!(out.contains("lv-hi ERR_OUT_OF_RANGE true"), "out: {out}");
    assert!(out.contains("lv-lo ERR_OUT_OF_RANGE true"), "out: {out}");
    assert!(out.contains("q-hi ERR_OUT_OF_RANGE true"), "out: {out}");
    assert!(out.contains("nocb true"), "out: {out}");
    assert!(out.contains("badin true"), "out: {out}");
    assert!(out.contains("empty true"), "out: {out}");
    assert!(out.contains("one true"), "out: {out}");
    assert!(out.contains("big true"), "out: {out}");
    assert!(out.contains("stored true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase10a_zlib_crc32() {
    // 10a：crc32（ISO-HDLC；真机值对拍：空串/链式/双报错）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "c.mjs",
        r#"import zlib, { crc32 } from "node:zlib";
console.log("base", crc32("hello"), crc32("hello", 0).toString(16));
console.log("buf", crc32(Buffer.from("hello")), crc32(new Uint8Array([104, 105])));
console.log("view", crc32(new DataView(new Uint8Array([104, 101, 108, 108, 111]).buffer)));
console.log("empty", crc32(""));
console.log("chain", crc32("world", crc32("hello")));
console.log("named", zlib.crc32("hello") === crc32("hello"), zlib.crc32Table);
try { crc32(42); } catch (e) { console.log("t-data", e.code, e.message); }
try { crc32("a", "x"); } catch (e) { console.log("t-value", e.code, e.message); }
"#,
    );
    for line in [
        "base 907060870 3610a686",
        "buf 907060870 3633523372",
        "view 907060870",
        "empty 0",
        "chain 4192936109",
        "named true undefined",
        "t-data ERR_INVALID_ARG_TYPE The \"data\" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received type number (42)",
        "t-value ERR_INVALID_ARG_TYPE The \"value\" argument must be of type number. Received type string ('x')",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_zlib_stream_teardown() {
    // 10f zlib 流收尾与内部小面（真机 26.8.2 对拍）：_handle/_closed 生命周期、
    // _processChunk（含 _outOffset 越界门）、空输入 flush 尺寸（20/1/9）、
    // flush kind 逐族校验、reset 分发中/已关闭双形、ZSTD_e_* 常量。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import z from "node:zlib";
const g = new z.Gzip();
console.log("open", g._handle !== null, g._closed === false, g._chunkSize === 16384, typeof g._processChunk);
g.destroy();
console.log("dst", g._handle === null, g._closed === true);
const g2 = new z.Gzip();
g2.close(() => console.log("close-cb", g2._handle === null, g2._closed === true));
const pc = new z.Gzip()._processChunk(Buffer.from("hi"), z.constants.Z_FINISH);
console.log("pc", z.gunzipSync(pc).toString() === "hi");
const bad = new z.Deflate();
bad._outOffset = bad._chunkSize + 1;
try { bad._processChunk(Buffer.alloc(1), z.constants.Z_FINISH); console.log("BAD no-throw"); }
catch (e) { console.log("pc-range", e.code); }
bad.close();
console.log("empty", z.gzipSync(Buffer.alloc(0)).length, z.brotliCompressSync(Buffer.alloc(0)).length, z.zstdCompressSync(Buffer.alloc(0)).length);
console.log("brotli-bytes", Buffer.from(z.brotliCompressSync(Buffer.from("Hello, world!".repeat(20)))).toString("hex") === "1b0301f88d946ed6540dc2825426d942de6a96c5aa010d6c966301");
for (const [n, f, ok, badk] of [["gz", z.createGzip, [0, 4, 5], [-1, 6, 100]], ["br", z.createBrotliCompress, [0, 1, 2, 3], [-1, 4, 6, 100]], ["zs", z.createZstdCompress, [0, 1, 2], [-1, 3, 4, 100]]]) {
  for (const k of ok) { const s = f(); s.on("error", () => {}); s.flush(k); }
  for (const k of badk) { try { f().flush(k); console.log("BAD flush-nothrow", n, k); } catch (e) { console.log("flush-oor", n, k, e.code); } }
  for (const k of ["x", null, {}]) { try { f().flush(k); console.log("BAD flush-nothrow2", n); } catch (e) { console.log("flush-arg", n, e.code); } }
  const sn = f(); sn.on("error", () => {}); sn.flush(NaN); sn.flush(() => {});
  console.log("flush-nan-ok", n);
}
const r = z.createDeflate();
r.write(Buffer.alloc(16, 65), () => {});
try { r._handle.reset(); console.log("BAD reset-nothrow"); }
catch (e) { console.log("reset-busy", e.message === "Cannot reset zlib stream while a write is in progress"); }
const rc = z.createDeflate();
rc.close(() => {
  try { rc.reset(); console.log("BAD closed-reset-nothrow"); }
  catch (e) { console.log("reset-closed", e.code); }
});
console.log("zstd-const", z.constants.ZSTD_e_continue === 0, z.constants.ZSTD_e_flush === 1, z.constants.ZSTD_e_end === 2);
"#,
    );
    for line in [
        "open true true true function",
        "dst true true",
        "close-cb true true",
        "pc true",
        "pc-range ERR_OUT_OF_RANGE",
        "empty 20 1 9",
        "brotli-bytes true",
        "flush-nan-ok gz",
        "flush-nan-ok br",
        "flush-nan-ok zs",
        "reset-busy true",
        "reset-closed ERR_INTERNAL_ASSERTION",
        "zstd-const true true true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    assert!(!out.contains("BAD "), "out: {out}");
    assert_eq!(out.matches("flush-oor").count(), 3 + 4 + 4, "out: {out}");
    assert_eq!(out.matches("flush-arg").count(), 3 + 3 + 3, "out: {out}");
    dir.close().unwrap();
}
