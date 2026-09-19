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
  // G9-2 真机对拍：brotliDecompressSync(垃圾) 真机 26.8.2 报 ERR__ERROR_FORMAT_PADDING_1
  //（我们 rust-brotli 状态机同错误类 PADDING_2，记档偏离）；其余 zlib 族 Z_DATA_ERROR。
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
    assert!(out.lines().any(|l| l.starts_with("brotliD ERR__ERROR_FORMAT") && l.ends_with("true")), "out: {out}");
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

#[test]
fn phase10f_zlib_flush_opts() {
    // 10f zlibFlush 选项校验（真机逐字）：flush/finishFlush/fullFlush 三键
    // 构造期校验；另覆盖 write 后 close 即关（close-after-write 套件同形）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import z from "node:zlib";
z.createGzip({ flush: 0, finishFlush: 2, fullFlush: 5 });
console.log("valid-ok");
for (const [k, v] of [["flush", "x"], ["finishFlush", null], ["fullFlush", {}]]) {
  try { z.createGzip({ [k]: v }); console.log("BAD no-throw", k); }
  catch (e) { console.log("arg", k, e.code, e.message); }
}
for (const [k, v] of [["flush", 6], ["finishFlush", -1], ["fullFlush", 2.5]]) {
  try { z.createGzip({ [k]: v }); console.log("BAD no-throw2", k); }
  catch (e) { console.log("oor", k, e.code, e.message); }
}
z.gzip("hello", (err, out) => {
  const unzip = z.createGunzip();
  unzip.write(out);
  unzip.close(() => console.log("waclose"));
});
"#,
    );
    for line in [
        "valid-ok",
        "arg flush ERR_INVALID_ARG_TYPE The \"options.flush\" property must be of type number. Received type string ('x')",
        "arg finishFlush ERR_INVALID_ARG_TYPE The \"options.finishFlush\" property must be of type number. Received null",
        "arg fullFlush ERR_INVALID_ARG_TYPE The \"options.fullFlush\" property must be of type number. Received an instance of Object",
        "oor flush ERR_OUT_OF_RANGE The value of \"options.flush\" is out of range. It must be >= 0 and <= 5. Received 6",
        "oor finishFlush ERR_OUT_OF_RANGE The value of \"options.finishFlush\" is out of range. It must be >= 0 and <= 5. Received -1",
        "oor fullFlush ERR_OUT_OF_RANGE The value of \"options.fullFlush\" is out of range. It must be >= 0 and <= 5. Received 2.5",
        "waclose",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    assert!(!out.contains("BAD "), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase10f_zlib_zip_archive() {
    // 10f zlib Zip归档面（node lib/internal/zip逐字移植）：round-trip/store回落/
    // ZipBuffer索引/maxSize与CRC门/坏档形状。正常+报错+边界三件。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import zlib from "node:zlib";
async function buildArchive(entries, comment) {
  const chunks = [];
  for await (const chunk of zlib.createZipArchive(entries, comment)) chunks.push(chunk);
  return Buffer.concat(chunks);
}
const entries = [
  await zlib.ZipEntry.create('hello.txt', Buffer.from('Hello, world!'.repeat(20))),
  await zlib.ZipEntry.create('raw.bin', Buffer.from([1, 2, 3, 4, 5]), { method: 'store' }),
  await zlib.ZipEntry.create('empty.txt', Buffer.alloc(0)),
  await zlib.ZipEntry.create('dir/', Buffer.alloc(0)),
];
console.log("methods", entries.map((e) => e.method).join(","));
const archive = await buildArchive(entries, 'test comment');
const read = [...zlib.ZipEntry.read(archive)];
console.log("read", read.length);
const byName = new Map(read.map((e) => [e.name, e]));
console.log("hello", (await byName.get('hello.txt').content()).toString() === 'Hello, world!'.repeat(20));
console.log("raw", byName.get('raw.bin').method, (await byName.get('raw.bin').content()).length);
console.log("dir", byName.get('dir/').isDirectory, byName.get('hello.txt').isFile);
// ZipBuffer索引面
{
  const a2 = await buildArchive([
    await zlib.ZipEntry.create('a.txt', Buffer.from('a')),
    await zlib.ZipEntry.create('b.txt', Buffer.from('b')),
  ]);
  using zip = new zlib.ZipBuffer(a2);
  console.log("zb", zip.size, zip.has('a.txt'), zip.has('missing.txt'),
    (await zip.get('a.txt').content()).toString(), [...zip.keys()].sort().join(","));
  try { zip.get('missing.txt'); console.log("BAD no-throw"); }
  catch (e) { console.log("notfound", e.code); }
}
// 报错：坏档 + 目录带内容拒收
try { [...zlib.ZipEntry.read(Buffer.from('nope'))]; console.log("BAD archive-nothrow"); }
catch (e) { console.log("badarch", e.code); }
try { await zlib.ZipEntry.create('dir/', Buffer.from('x')); console.log("BAD dir-nothrow"); }
catch (e) { console.log("dircontent", e.code); }
// 边界：maxSize门 + 篡改CRC + 同步往返
{
  const e = zlib.ZipEntry.createSync('a.txt', Buffer.from('hello world'), { method: 'store' });
  try { e.contentSync({ maxSize: 1 }); console.log("BAD maxsize-nothrow"); }
  catch (err) { console.log("maxsize", err.code); }
  const syncArch = Buffer.concat([...zlib.createZipArchiveSync([e])]);
  const tampered = Buffer.from(syncArch);
  tampered[30 + 'a.txt'.length] ^= 0xff;
  const [te] = zlib.ZipEntry.read(tampered);
  try { te.contentSync(); console.log("BAD crc-nothrow"); }
  catch (err) { console.log("corrupt", err.code); }
  console.log("noverify", te.contentSync({ verify: false }).length);
  console.log("maxsz", zlib.getMaxZipContentSize() > 0);
}
"#,
    );
    for line in [
        "methods 8,0,0,0",
        "read 4",
        "hello true",
        "raw 0 5",
        "dir true true",
        "zb 2 true false a a.txt,b.txt",
        "notfound ERR_ZIP_ENTRY_NOT_FOUND",
        "badarch ERR_ZIP_INVALID_ARCHIVE",
        "dircontent ERR_INVALID_ARG_VALUE",
        "maxsize ERR_ZIP_ENTRY_TOO_LARGE",
        "corrupt ERR_ZIP_ENTRY_CORRUPT",
        "noverify 11",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    assert!(!out.contains("BAD "), "out: {out}");
    assert!(out.contains("maxsz true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase_g92_zlib_incremental_streams() {
    // G9-2 增量流面（真机 26.8.2 对拍）：write 即时压出、flush 档位即时出边界
    // （test-zlib-flush 套件向量）、finishFlush 容忍截断（truncated）、
    // rejectGarbageAfterEnd 双面（reject-garbage 套件）、bytesWritten 只计引擎
    // 消费（premature-end）、一次性解压真机错误口径（unexpected end of file/
    // unknown compression method/Missing dictionary）、多成员 gunzip 拼接。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import z from "node:zlib";
const C = z.constants;

// 1. flush 档位即时出边界（test-zlib-flush 套件真机向量：level 0）
{
  const def = z.createDeflate({ level: 0 });
  const chunk = Buffer.from("/9j/4AAQSkZJRgABAQEASA==", "base64");
  let actualNone, actualFull;
  def.write(chunk, function () {
    def.flush(C.Z_NO_FLUSH, function () {
      actualNone = def.read();
      def.flush(function () {
        const bufs = []; let buf;
        while ((buf = def.read()) !== null) bufs.push(buf);
        actualFull = Buffer.concat(bufs);
        console.log("flush-vec", actualNone.equals(Buffer.from([0x78, 0x01])),
          actualFull.equals(Buffer.concat([Buffer.from([0x00,0x10,0x00,0xef,0xff]), chunk, Buffer.from([0x00,0x00,0x00,0xff,0xff])])));
      });
    });
  });
}

// 2. write 即时压出（解压流 data 事件在 write 后即可读，无需 end）
{
  const s = z.createInflateRaw();
  let out = "";
  s.setEncoding("utf8");
  s.on("data", (c) => out += c);
  s.on("error", () => {});
  s.write(z.deflateRawSync("hello"), () => {
    console.log("write-instant", out === "hello");
  });
}

// 3. bytesWritten 只计引擎消费（trailing 垃圾不计入）
{
  const s = z.createInflateRaw();
  let out = "";
  s.setEncoding("utf8");
  s.on("data", (c) => out += c);
  s.on("end", () => {
    const comp = z.deflateRawSync("0123456789".repeat(4));
    console.log("bytesWritten", out.length === 40, s.bytesWritten === comp.length);
  });
  s.write(z.deflateRawSync("0123456789".repeat(4)));
  s.write(Buffer.from("not valid compressed data"));
  s.end();
}

// 4. rejectGarbageAfterEnd：boolean 校验 + junk TypeError
{
  const a = z.deflateSync("a");
  const two = Buffer.concat([a, a]);
  console.log("rej-sync", z.inflateSync(two).toString() === "a");
  for (const v of [1, "true", null]) {
    try { z.inflateSync(a, { rejectGarbageAfterEnd: v }); console.log("BAD rej-nothrow"); }
    catch (e) { console.log("rej-arg", e.code); }
    try { z.createInflate({ rejectGarbageAfterEnd: v }); console.log("BAD ctor-nothrow"); }
    catch (e) { console.log("rej-ctor", e.code); }
  }
  const s = z.createInflate({ rejectGarbageAfterEnd: true });
  s.on("error", (e) => console.log("rej-stream", e.name, e.code));
  s.on("data", () => {});
  s.end(two);
}

// 5. 一次性解压真机错误口径
try { z.inflateSync(z.deflateSync("ΩΩLorem ipsum dolor sit amet consectetur adipiscing").subarray(0, 8)); console.log("BAD trunc-nothrow"); }
catch (e) { console.log("trunc", e.code, /unexpected end of file/.test(e.message)); }
try { z.gunzipSync(Buffer.concat([z.gzipSync("abc"), Buffer.from([0x1f, 0x8b, 0xff, 0xff]), Buffer.alloc(10)])); console.log("BAD hdr-nothrow"); }
catch (e) { console.log("gzhdr", e.code, e.message === "unknown compression method"); }
try { z.inflateSync(z.deflateSync("abc", { dictionary: Buffer.from("hello") })); console.log("BAD dict-nothrow"); }
catch (e) { console.log("dict-miss", e.code, e.message === "Missing dictionary"); }
try { z.inflateSync(z.deflateSync("abc", { dictionary: Buffer.from("hello") }), { dictionary: Buffer.from("world") }); console.log("BAD dict2-nothrow"); }
catch (e) { console.log("dict-bad", e.code, e.message === "Bad dictionary"); }
console.log("dict-ok", z.inflateSync(z.deflateSync("abc", { dictionary: Buffer.from("hello") }), { dictionary: Buffer.from("hello") }).toString() === "abc");

// 6. gunzip 多成员拼接 + 尾零（流式 + 一次性）
{
  const s = z.createGunzip();
  let out = "";
  s.setEncoding("utf8");
  s.on("data", (c) => out += c);
  s.on("end", () => console.log("gz-multi", out === "abcdef"));
  s.end(Buffer.concat([z.gzipSync("abc"), z.gzipSync("def"), Buffer.alloc(10)]));
}
console.log("gz-multi-sync", z.gunzipSync(Buffer.concat([z.gzipSync("abc"), z.gzipSync("def"), Buffer.alloc(10)])).toString() === "abcdef");

// 7. finishFlush 容忍截断（部分解出）
{
  const comp = z.deflateSync("x".repeat(100));
  const partial = z.inflateSync(comp.subarray(0, comp.length / 2), { finishFlush: C.Z_SYNC_FLUSH });
  console.log("tolerate", partial.length > 0);
}

// 8. reset 复用引擎（_handle.reset 后可重写）
{
  const s = z.createDeflate();
  s.on("error", () => {});
  s.write(Buffer.from("first"), () => {
    s._handle.reset();
    s.write(Buffer.from("second"), () => console.log("reset-reuse", true));
  });
}

// 9. node:test require 可调用形 + t.mock 最小面（write-after-end 套件口径）
{
  const t = require("node:test");
  console.log("test-req", typeof t === "function", typeof t.test === "function", typeof t.describe === "function");
}

// 10. 大数据 round-trip（随机数据 finish 泵完——zip-property 丢尾回归）
{
  const data = Buffer.alloc(256 * 1024);
  for (let i = 0; i < data.length; i++) data[i] = (i * 2654435761 >>> 16) & 0xff;
  const back = z.inflateRawSync(z.deflateRawSync(data));
  console.log("big-rt", back.length === data.length, back.equals(data));
}
"#,
    );
    for line in [
        "flush-vec true true",
        "write-instant true",
        "bytesWritten true true",
        "rej-sync true",
        "rej-arg ERR_INVALID_ARG_TYPE",
        "rej-ctor ERR_INVALID_ARG_TYPE",
        "rej-stream TypeError ERR_TRAILING_JUNK_AFTER_STREAM_END",
        "trunc Z_BUF_ERROR true",
        "gzhdr Z_DATA_ERROR true",
        "dict-miss Z_NEED_DICT true",
        "dict-bad Z_NEED_DICT true",
        "dict-ok true",
        "gz-multi true",
        "gz-multi-sync true",
        "tolerate true",
        "reset-reuse true",
        "test-req true true true",
        "big-rt true true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    assert!(!out.contains("BAD "), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase10g_zlib_dict_pledged_webstream() {
    // G9-3 收官三面：raw 字典流式（G9-3a）/ 字典严格校验 + pledgedSrcSize（G9-3b/c）/
    // Web CompressionStream·DecompressionStream（G9-3d，type-error 套件同款）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import z, {
  createDeflateRaw, createInflateRaw, createBrotliCompress, createBrotliDecompress,
  createZstdCompress, zstdCompressSync, brotliCompressSync, constants,
} from "node:zlib";

// 1. raw 字典流式 + 视图族 + reset 组合（dictionary 套件 raw/rawreset 形）
{
  const dict = Buffer.from("lorem ipsum dolor sit amet 0123456789 adipiscing elit");
  const input = "HTTP/1.1 200 Ok\r\nServer: x\r\n\r\n".repeat(40);
  const ab = dict.buffer.slice(dict.byteOffset, dict.byteOffset + dict.byteLength);
  const sources = [["buf", dict], ["ab", ab], ["u8", new Uint8Array(ab)], ["dv", new DataView(ab)]];
  for (const [label, d] of sources) {
    const def = createDeflateRaw({ dictionary: d });
    const inf = createInflateRaw({ dictionary: d });
    let out = "";
    inf.setEncoding("utf-8");
    inf.on("data", (c) => { out += c; });
    def.on("data", (c) => inf.write(c));
    const ok = await new Promise((res) => {
      inf.on("end", () => res(out === input));
      inf.on("error", (e) => { console.log("raw-" + label, "ERR", e.code, e.message); res(false); });
      def.on("error", (e) => { console.log("raw-" + label, "DERR", e.code, e.message); res(false); });
      def.on("end", () => inf.end());
      def.end(input);
    });
    console.log("raw-dict-" + label, ok);
  }
  // reset 组合：write → flush → reset → write → end（修前 raw 形必 "bad state"）
  {
    const def = createDeflateRaw({ dictionary: dict });
    const inf = createInflateRaw({ dictionary: dict });
    let out = ""; let resetDone = false;
    inf.setEncoding("utf-8");
    inf.on("data", (c) => { out += c; });
    def.on("data", (c) => { if (resetDone) inf.write(c); });
    inf.on("error", (e) => { console.log("raw-reset ERR", e.code); });
    const ok = await new Promise((res) => {
      inf.on("end", () => res(out === input));
      def.on("end", () => inf.end());
      def.write(input);
      def.flush(() => { def.reset(); resetDone = true; def.write(input); def.end(); });
    });
    // 第一轮输出按设计被 resetDone 门丢弃，out 恰为 reset 后新流解出的一份 input。
    console.log("raw-reset", ok, out.length === input.length);
  }
}

// 2. 字典严格校验（G9-3b：string 是合法数据输入但非法字典）
{
  for (const [label, bad] of [["str", "s"], ["num", 123], ["bool", true], ["obj", { a: 1 }], ["arr", [1, 2, 3]]]) {
    let got = "";
    try { createBrotliCompress({ dictionary: bad }); got = "NO"; } catch (e) { got = e.code; }
    console.log("bdict-ctor-" + label, got === "ERR_INVALID_ARG_TYPE");
    got = "";
    try { createBrotliDecompress({ dictionary: bad }); got = "NO"; } catch (e) { got = e.code; }
    console.log("bdec-ctor-" + label, got === "ERR_INVALID_ARG_TYPE");
  }
  let got = "";
  try { brotliCompressSync("x", { dictionary: "s" }); got = "NO"; } catch (e) { got = e.code; }
  console.log("bdict-sync", got === "ERR_INVALID_ARG_TYPE");
}

// 3. pledgedSrcSize（G9-3c）
console.log("pledged-const", constants.ZSTD_error_srcSize_wrong === 72);
{
  let code = "", errno = 0;
  try { zstdCompressSync("x".repeat(10), { pledgedSrcSize: 9 }); code = "NO-THROW"; }
  catch (e) { code = e.code; errno = e.errno; }
  console.log("pledged-sync-mismatch", code === "ZSTD_error_srcSize_wrong", errno === 72);
}
{
  let ok = false, round = false;
  try {
    const c = zstdCompressSync("x".repeat(10), { pledgedSrcSize: 10 });
    ok = true;
    round = z.zstdDecompressSync(c).toString() === "x".repeat(10);
  } catch {}
  console.log("pledged-sync-match", ok, round);
}
{
  const r = await new Promise((res) => {
    const c = createZstdCompress({ pledgedSrcSize: 5 });
    c.on("error", (e) => res(e.code + " " + (e.errno === 72)));
    c.on("end", () => res("NO-ERR"));
    c.write("x".repeat(7), () => { c.end(); c.resume(); });
  });
  console.log("pledged-stream-mismatch", r === "ZSTD_error_srcSize_wrong true");
}
for (const [label, v, expect] of [["str", "1", "ERR_INVALID_ARG_TYPE"], ["nul", null, "ERR_INVALID_ARG_TYPE"],
  ["nan", NaN, "ERR_OUT_OF_RANGE"], ["frac", 1.9, "ERR_OUT_OF_RANGE"], ["neg", -1, "ERR_OUT_OF_RANGE"],
  ["big", 9007199254740992, "ERR_OUT_OF_RANGE"]]) {
  let got = "";
  try { zstdCompressSync("x", { pledgedSrcSize: v }); got = "NO"; } catch (e) { got = e.code; }
  console.log("pledged-bad-" + label, got === expect);
}

// 4. Web CompressionStream / DecompressionStream（G9-3d）
{
  console.log("cs-global", typeof CompressionStream === "function", typeof DecompressionStream === "function");
  const { CompressionStream: CS, DecompressionStream: DS } = await import("node:stream/web");
  console.log("cs-web", typeof CS === "function", typeof DS === "function", CS === globalThis.CompressionStream);
  const ds = new DS("gzip");
  console.log("cs-shape", ds instanceof Object, ds instanceof TransformStream === false,
    Object.prototype.toString.call(ds) === "[object DecompressionStream]");
  let threw = "";
  try { new DS("nope"); } catch (e) { threw = e.name; }
  console.log("cs-badfmt", threw === "TypeError");
  // 四族 roundtrip（CS → DS，pipeThrough + async 迭代）
  const text = "hello web streams compression " + "x".repeat(200);
  for (const fmt of ["deflate", "gzip", "deflate-raw", "brotli"]) {
    const chunks = [];
    for await (const c of new Blob([text]).stream().pipeThrough(new CS(fmt)).pipeThrough(new DS(fmt))) chunks.push(c);
    console.log("rt-" + fmt, Buffer.concat(chunks).toString() === text);
  }
  // 尾垃圾四形（type-error 套件同款：截断 1B + 双流拼接）
  const validGz = z.gzipSync("a");
  const validDf = z.deflateSync("a");
  const validBr = z.brotliCompressSync("a");
  async function trail(fmt, chunks) {
    try {
      await Array.fromAsync(new Blob(chunks).stream().pipeThrough(new DS(fmt)));
      return "NO-REJECT";
    } catch (e) { return e.name + " " + e.code; }
  }
  console.log("trail-deflate", await trail("deflate", [new Uint8Array([...validDf, 1])]) === "TypeError ERR_TRAILING_JUNK_AFTER_STREAM_END",
    await trail("deflate", [new Uint8Array([...validDf, ...validDf])]) === "TypeError ERR_TRAILING_JUNK_AFTER_STREAM_END");
  console.log("trail-gzip", await trail("gzip", [new Uint8Array([...validGz, 1])]) === "TypeError ERR_TRAILING_JUNK_AFTER_STREAM_END",
    await trail("gzip", [new Uint8Array([...validGz, ...validGz])]) === "TypeError ERR_TRAILING_JUNK_AFTER_STREAM_END");
  console.log("trail-brotli", await trail("brotli", [new Uint8Array([...validBr, 1])]) === "TypeError ERR_TRAILING_JUNK_AFTER_STREAM_END",
    await trail("brotli", [new Uint8Array([...validBr, ...validBr])]) === "TypeError ERR_TRAILING_JUNK_AFTER_STREAM_END");
}
"#,
    );
    for line in [
        "raw-dict-buf true",
        "raw-dict-ab true",
        "raw-dict-u8 true",
        "raw-dict-dv true",
        "raw-reset true true",
        "bdict-ctor-str true",        "bdec-ctor-str true",
        "bdict-sync true",
        "pledged-const true",
        "pledged-sync-mismatch true true",
        "pledged-sync-match true true",
        "pledged-stream-mismatch true",
        "pledged-bad-str true",
        "pledged-bad-nul true",
        "pledged-bad-nan true",
        "pledged-bad-frac true",
        "pledged-bad-neg true",
        "pledged-bad-big true",
        "cs-global true true",
        "cs-web true true true",
        "cs-shape true true true",
        "cs-badfmt true",
        "rt-deflate true",
        "rt-gzip true",
        "rt-deflate-raw true",
        "rt-brotli true",
        "trail-deflate true true",
        "trail-gzip true true",
        "trail-brotli true true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    for label in ["num", "bool", "obj", "arr"] {
        assert!(out.lines().any(|l| l == format!("bdict-ctor-{label} true")), "bdict-ctor-{label}\nout: {out}");
        assert!(out.lines().any(|l| l == format!("bdec-ctor-{label} true")), "bdec-ctor-{label}\nout: {out}");
    }
    assert!(!out.contains("ERR "), "out: {out}");
    dir.close().unwrap();
}
