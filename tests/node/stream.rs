//! tests/node/stream.rs — 对齐 src/builtins/node/stream.rs（node:stream 系（含 consumers/web））。

use crate::common::*;
use crate::helpers::*;
use assert_fs::prelude::*;

#[test]
fn phase9b_stream_readable_writable_core() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import stream, { Readable, Writable } from "node:stream";
// Readable：push → flow → end
const chunks = [];
const r = new Readable({ read() {} });
r.push("a"); r.push("b"); r.push(null);
r.on("data", (c) => chunks.push(c));
r.on("end", () => console.log("end", chunks.join(","), chunks.map((c) => c.constructor.name).join("/")));
// pause/resume（Node 口径：push(null) 排空后 end 即发，先于 resume）
const r2 = new Readable({ read() {} });
r2.push("x"); r2.push(null);
const seen = [];
r2.on("end", () => console.log("resume-end", seen.join(","), r2.readableEnded));
r2.on("data", (c) => { seen.push(c); r2.pause(); });
await new Promise((res) => setTimeout(res, 20));
console.log("paused", seen.length, r2.isPaused());
r2.resume();
// readable 面方法
const r3 = new Readable({ read() {} });
r3.push("q");
console.log("rface", r3.readableLength, typeof r3.read, typeof r3.unpipe);
console.log("rread", String(r3.read()));
// Writable：write/end/finish
const writes = [];
const w = new Writable({ write(chunk, enc, cb) { writes.push(String(chunk)); cb(); } });
w.write("1"); w.write("2"); w.end("3");
w.on("finish", () => console.log("finish", writes.join(""), w.writableEnded));
// cork/uncork 批量
let n = 0;
const w2 = new Writable({ write(c, e, cb) { n++; cb(); } });
w2.cork(); w2.write("a"); w2.write("b");
console.log("corked", n);
w2.uncork(); w2.end();
w2.on("finish", () => console.log("uncork", n));
// write after end → error 事件
const w3 = new Writable({ write(c, e, cb) { cb(); } });
const errs = [];
w3.on("error", (e) => errs.push(e.code));
w3.end();
w3.write("late");
await new Promise((res) => setTimeout(res, 20));
console.log("wae", errs.length, errs[0]);
// destroy/close
const r5 = new Readable({ read() {} });
r5.push("d");
r5.on("close", () => console.log("closed", r5.destroyed, stream.isDestroyed(r5)));
r5.destroy();
// destroy(err) → error + close
const r6 = new Readable({ read() {} });
const e6 = [];
r6.on("error", (e) => e6.push(e.message));
r6.on("close", () => console.log("destroy-err", e6.join(","), r6.destroyed, stream.isErrored(r6)));
r6.destroy(new Error("boom"));
await new Promise((res) => setTimeout(res, 30));
console.log("same", stream.Readable === Readable, stream.Writable === Writable, typeof stream.isDisturbed);
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("end a,b Buffer/Buffer"), "out: {out}");
    assert!(out.contains("paused 1 true"), "out: {out}");
    assert!(out.contains("resume-end x"), "out: {out}");
    assert!(out.contains("rface 1 function function"), "out: {out}");
    assert!(out.contains("rread q"), "out: {out}");
    assert!(out.contains("finish 123 true"), "out: {out}");
    assert!(out.contains("corked 0"), "out: {out}");
    assert!(out.contains("uncork 2"), "out: {out}");
    assert!(out.contains("wae 1 ERR_STREAM_WRITE_AFTER_END"), "out: {out}");
    assert!(out.contains("closed true true"), "out: {out}");
    assert!(out.contains("destroy-err boom true true"), "out: {out}");
    assert!(out.contains("same true true function"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9b_stream_duplex_transform_pipeline() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import stream, { Readable, Writable, Duplex, Transform, PassThrough, pipeline, finished, compose, addAbortSignal } from "node:stream";
import { pipeline as ppipeline, finished as pfinished } from "node:stream/promises";
// Duplex 双向
const dOut = [];
const d = new Duplex({ read() {}, write(c, e, cb) { dOut.push("w:" + String(c)); cb(); } });
d.on("data", (c) => dOut.push("r:" + String(c)));
d.push("r1"); d.write("w1");
await new Promise((res) => setTimeout(res, 30));
console.log("dup", dOut.sort().join(","));
// duplexPair 两侧互写
const [sa, sb] = stream.duplexPair();
const gotB = [];
sb.on("data", (c) => gotB.push(String(c)));
sa.write("ping");
await new Promise((res) => setTimeout(res, 20));
console.log("pair", gotB.join(","), sb.writable && sa.readable);
// Transform + pipeline（callback 形态，node:stream 命名导出——无回调即 validateFunction 报错）
const up = new Transform({ transform(c, e, cb) { cb(null, String(c).toUpperCase()); } });
const o1 = [];
const w1 = new Writable({ write(c, e, cb) { o1.push(String(c)); cb(); } });
await new Promise((res, rej) => pipeline(Readable.from(["a", "b"]), up, w1, (err) => (err ? rej(err) : res())));
console.log("pipeline", o1.join(""), up.writableEnded, up.readableEnded);
try { pipeline(Readable.from(["a"]), new Writable({ write(c, e, cb) { cb(); } })); }
catch (e) { console.log("pcall-err", e.message.includes("must be of type function")); }
// flush 尾包 + node:stream/promises 模块面
const fl = [];
const tf = new Transform({ transform(c, e, cb) { cb(null, c); }, flush(cb) { fl.push("f"); cb(null, "!"); } });
const o2 = [];
await ppipeline(Readable.from(["x"]), tf, new Writable({ write(c, e, cb) { o2.push(String(c)); cb(); } }));
console.log("flush", o2.join(""), fl.join(","));
// transform 报错沿 pipeline 传播
const bad = new Transform({ transform(c, e, cb) { cb(new Error("t-boom")); } });
try { await ppipeline(Readable.from(["a"]), bad, new Writable({ write(c, e, cb) { cb(); } })); }
catch (e) { console.log("terr", e.message); }
// 源错误传播（命名 pipeline callback 形态）
const rs = new Readable({ read() { this.destroy(new Error("src-boom")); } });
try { await new Promise((res, rej) => pipeline(rs, new Writable({ write(c, e, cb) { cb(); } }), (err) => (err ? rej(err) : res()))); }
catch (e) { console.log("perr", e.message); }
// compose（Readable + Transform → 单一流再接管道；promise 形态走 node:stream/promises）
const c1 = compose(Readable.from(["m"]), new Transform({ transform(c, e, cb) { cb(null, String(c) + "!"); } }));
const o3 = [];
await ppipeline(c1, new Writable({ write(c, e, cb) { o3.push(String(c)); cb(); } }));
console.log("compose", o3.join(""));
// PassThrough
const pt = new PassThrough();
pt.end("pt");
console.log("pt", await new Promise((res) => { let s = ""; pt.on("data", (c) => (s += String(c))); pt.on("end", () => res(s)); }));
// finished：promise 形态（node:stream/promises）+ callback 形态（命名导出）
const fw = new Writable({ write(c, e, cb) { cb(); } });
fw.end();
await pfinished(fw); console.log("fin-ok");
const fw2 = new Writable({ write(c, e, cb) { cb(); } });
fw2.end();
console.log("fin-cb", await new Promise((res) => finished(fw2, (err) => res(err ? err.code : "ok"))));
// eos 不消费流：须先让流流动，read 内的 destroy 才会触发（Node 同款）
const re = new Readable({ read() { this.destroy(new Error("fin-boom")); } });
const fp = new Promise((res) => finished(re, (err) => res(err.message)));
re.resume();
console.log("fin-err", await fp);
// addAbortSignal
const ac = new AbortController();
const r5 = new Readable({ read() {} });
const a5 = [];
r5.on("error", (e) => a5.push(e.name));
addAbortSignal(ac.signal, r5);
ac.abort();
await new Promise((res) => setTimeout(res, 20));
console.log("abort", a5.join(","), r5.destroyed);
// hwm 存取
stream.setDefaultHighWaterMark(true, 9999);
console.log("hwm", stream.getDefaultHighWaterMark(true), stream.getDefaultHighWaterMark(false));
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("dup r:r1,w:w1"), "out: {out}");
    assert!(out.contains("pair ping true"), "out: {out}");
    assert!(out.contains("pipeline AB true true"), "out: {out}");
    assert!(out.contains("pcall-err true"), "out: {out}");
    assert!(out.contains("flush x! f"), "out: {out}");
    assert!(out.contains("terr t-boom"), "out: {out}");
    assert!(out.contains("perr src-boom"), "out: {out}");
    assert!(out.contains("compose m!"), "out: {out}");
    assert!(out.contains("pt pt"), "out: {out}");
    assert!(out.contains("fin-ok"), "out: {out}");
    assert!(out.contains("fin-err fin-boom"), "out: {out}");
    assert!(out.contains("abort AbortError true"), "out: {out}");
    assert!(out.contains("hwm 9999 65536"), "out: {out}");
    dir.close().unwrap();
}

// ── Phase 9b-4：Readable.from / 异步迭代器 / stream/web / consumers ─────────

#[test]
fn phase9b_stream_from_iterators() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { Readable } from "node:stream";
// from 变体：字符串按码元逐个、数组保真、生成器、异步生成器
const r1 = Readable.from("ab");
const got = [];
for await (const c of r1) got.push(String(c));
console.log("from-str", got.join(","));
console.log("from-arr", (await Readable.from([1, 2, 3]).toArray()).join(","));
function* g() { yield "x"; yield "y"; }
console.log("from-gen", (await Readable.from(g()).toArray()).join(","));
async function* ag() { await new Promise((res) => setTimeout(res, 10)); yield "s"; }
console.log("from-async", (await Readable.from(ag()).toArray()).join(","));
// objectMode 保真（非字节块原样传递）
const om = Readable.from([{ a: 1 }, [2, 3], 42]);
console.log("objmode", JSON.stringify(await om.toArray()));
// 早退 break → 流销毁
const rb = Readable.from([1, 2, 3, 4]);
const picked = [];
for await (const c of rb) { picked.push(c); if (c === 2) break; }
console.log("break", picked.join(","), rb.destroyed);
// 非可迭代源报错
try { Readable.from(42); } catch (e) { console.log("e1", e.code); }
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("from-str ab"), "out: {out}");
    assert!(out.contains("from-arr 1,2,3"), "out: {out}");
    assert!(out.contains("from-gen x,y"), "out: {out}");
    assert!(out.contains("from-async s"), "out: {out}");
    assert!(out.contains(r#"objmode [{"a":1},[2,3],42]"#), "out: {out}");
    assert!(out.contains("break 1,2 true"), "out: {out}");
    assert!(out.contains("e1 ERR_INVALID_ARG_TYPE"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9b_stream_web_and_consumers() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { Readable, Writable } from "node:stream";
import * as streamWeb from "node:stream/web";
import * as consumers from "node:stream/consumers";
// toWeb：node Readable → web ReadableStream，chunk 原样透传
const webR = Readable.toWeb(Readable.from(["tw"]));
const rd = webR.getReader();
const parts = [];
while (true) { const { done, value } = await rd.read(); if (done) break; parts.push(new TextDecoder().decode(value)); }
console.log("t2w", parts.join(","), webR instanceof ReadableStream);
// fromWeb：web → node，chunk 为 Uint8Array
const nfw = Readable.fromWeb(new ReadableStream({ start(c) { c.enqueue(new Uint8Array([9])); c.close(); } }));
const a9 = await nfw.toArray();
console.log("f2w", a9.length, a9[0].constructor.name);
// Writable.toWeb / fromWeb
const got = [];
const nw = new Writable({ write(c, e, cb) { got.push(new TextDecoder().decode(c)); cb(); } });
const ww = Writable.toWeb(nw).getWriter();
await ww.write(new TextEncoder().encode("hx")); await ww.close();
console.log("w2w", got.join(","));
const nw2 = Writable.fromWeb(new WritableStream({ write(c) { got.push("f:" + new TextDecoder().decode(c)); } }));
nw2.write("q"); await new Promise((res) => nw2.end(res));
console.log("w2w", got.join(","));
// node:stream/web 面 = Web 全局类
console.log("webmod", streamWeb.ReadableStream === ReadableStream,
  new streamWeb.TransformStream() instanceof TransformStream);
// consumers 六件套
console.log("c-text", await consumers.text(Readable.from(["he", "llo"])));
const ab2 = await consumers.arrayBuffer(Readable.from([new Uint8Array([1, 2]), new Uint8Array([3])]));
console.log("c-ab", ab2.byteLength, new Uint8Array(ab2).join(","));
console.log("c-json", JSON.stringify(await consumers.json(Readable.from(['{"n":', '5}']))));
console.log("c-buf", (await consumers.buffer(Readable.from(["z"]))).constructor.name);
console.log("c-bytes", (await consumers.bytes(Readable.from(["z"]))).constructor.name);
const bl = await consumers.blob(Readable.from(["q"]));
console.log("c-blob", bl.size, bl instanceof Blob);
// 报错：非法 JSON
try { await consumers.json(Readable.from(["nope"])); } catch (e) { console.log("c-e1", e.constructor.name); }
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("t2w tw true"), "out: {out}");
    assert!(out.contains("f2w 1 Buffer"), "out: {out}");
    assert!(out.contains("w2w hx"), "out: {out}");
    assert!(out.contains("w2w hx,f:q"), "out: {out}");
    assert!(out.contains("webmod true true"), "out: {out}");
    assert!(out.contains("c-text hello"), "out: {out}");
    assert!(out.contains("c-ab 3 1,2,3"), "out: {out}");
    assert!(out.contains(r#"c-json {"n":5}"#), "out: {out}");
    assert!(out.contains("c-buf Buffer"), "out: {out}");
    assert!(out.contains("c-bytes Uint8Array"), "out: {out}");
    assert!(out.contains("c-blob 1 true"), "out: {out}");
    assert!(out.contains("c-e1 SyntaxError"), "out: {out}");
    dir.close().unwrap();
}

// ── Phase 9b-5：node:timers/promises ────────────────────────────────────────

#[test]
fn phase10f_stream_parity_tick_scheduler_and_fs_readstream() {
    // 10f stream 对拍收口面：nextTick 原生队列（实参展开/uncaughtException
    // 路由/微任务期入队 tick 恒后于整轮微任务——V8 checkpoint 原子性）、
    // compose post-loop throw 经管线 reject、fs.ReadStream 事件序、stdout
    // EE 表面（pipe dest）、QueuingStrategy 双全局。标签互不为子串（§4.42）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p10f.mjs",
        r#"
import { Readable, Writable } from "node:stream";
import fs from "node:fs";
import assert from "node:assert";

// nextTick：实参展开（fire_due 同款展开器路径）
process.nextTick((a, b) => console.log("tick-args", a === "x", b === 7), "x", 7);

// nextTick 抛错 → uncaughtException 监听（不是 rejection）
process.on("uncaughtException", (e) => {
  if (e.message === "tickboom") console.log("tick-caught", true);
});

// 微任务期入队的 tick：恒后于既有微任务（promise 链先排空）——compose/pipeline
// 竞速的根；真机 V8 checkpoint 原子性
Promise.resolve().then(() => {
  process.nextTick(() => order.push("tick"));
  order.push("micro");
});
const order = ["start"];
setTimeout(() => console.log("tick-order", order.join(",")), 20);

// compose：源耗尽后抛错 → toArray reject（错误不被管线提前收工吞掉）
Readable.from([1, 2, 3, 4, 5]).compose(async function* (src) {
  for await (const c of src) {}
  throw new Error("postloop");
}).toArray().then(
  () => console.log("compose-postloop", false),
  (e) => console.log("compose-postloop", e.message === "postloop"),
);

// fs.ReadStream：事件序 open→ready→data(Buffer)→end→close + path/autoClose
const rs = fs.createReadStream("/etc/hosts", { highWaterMark: 8 });
const ev = [];
rs.on("open", () => ev.push("open"));
rs.on("ready", () => ev.push("ready"));
rs.on("data", (c) => { if (!ev.some((x) => x.startsWith("data"))) ev.push("data:" + (c.constructor.name === "Buffer")); });
rs.on("end", () => ev.push("end"));
rs.on("close", () => {
  ev.push("close");
  console.log("rs-order", ev.join(",") === "open,ready,data:true,end,close", rs.path === "/etc/hosts", rs.autoClose === true);
});

// stdout：EE 表面接得住 pipe 的 dest（on/emit/write）；Writable pipe 到 stdout
// 走 ERR_STREAM_CANNOT_PIPE（真机口径）
console.log("stdout-ee", typeof process.stdout.on === "function", typeof process.stdout.write === "function", typeof process.stdout.emit === "function");
const w = new Writable({ autoDestroy: false });
w._write = () => {};
let pipeErr = null;
w.on("error", (e) => { pipeErr = e; });
w.pipe(process.stdout);
console.log("cannot-pipe", pipeErr !== null && pipeErr.code === "ERR_STREAM_CANNOT_PIPE");

// QueuingStrategy 双全局（真机口径：hwm 原型 getter、size 稳定共享函数）
const bl = new ByteLengthQueuingStrategy({ highWaterMark: 3 });
const cq = new CountQueuingStrategy({ highWaterMark: 4 });
console.log("qs-bl", bl.highWaterMark === 3, bl.size({ byteLength: 5 }) === 5, bl.size({}) === undefined);
console.log("qs-cq", cq.highWaterMark === 4, cq.size({}) === 1, bl.size === new ByteLengthQueuingStrategy({ highWaterMark: 1 }).size);
process.nextTick(() => { throw new Error("tickboom"); });
"#,
    );
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    for line in [
        "tick-args true true",
        "tick-caught true",
        "tick-order start,micro,tick",
        "compose-postloop true",
        "rs-order true true true",
        "stdout-ee true true true",
        "cannot-pipe true",
        "qs-bl true true true",
        "qs-cq true true true",
    ] {
        assert!(
            text.lines().any(|l| l.starts_with(line) || l == line),
            "missing: {line}\nout: {text}"
        );
    }
    dir.close().unwrap();
}

#[test]
fn phase11_stream_r1_eos_hooks_faces() {
    // P2-stream R1：eos 三套件 + 连字符回落 + tty_wrap（node 原文口径）。
    // 正常：finished 回调触发；AsyncResource 构造触发 init（STREAM_END_OF_STREAM
    //   上下文传播）；enable 后 enabledHooksExist 真。
    // 报错：internal 连字符形可解（add-abort-signal/end-of-stream 不抛未映射）。
    // 边界：无钩无 ALS 即 enabledHooksExist 假；tty_wrap.TTY 三键不可枚举。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "r1.mjs",
        r#"
import { Readable, finished } from "node:stream";
import { createHook, executionAsyncId } from "node:async_hooks";
const { enabledHooksExist } = await import("node:internal/async_hooks").then((m) => m.default ?? m);

// 正常：init 触发 + 上下文传播（bindAsyncResource-path 套件同构）
const cmap = new Map();
cmap.set(executionAsyncId(), "abc-123");
createHook({
  init(asyncId, type, triggerAsyncId) {
    if (type === "STREAM_END_OF_STREAM") cmap.set(asyncId, cmap.get(triggerAsyncId));
  },
}).enable();
console.log("hooks-exist", enabledHooksExist() === true);
const r = new Readable({ read() {} });
finished(r, () => {
  console.log("fin-ctx", cmap.get(executionAsyncId()) === "abc-123");
});
r.destroy();
// 报错面：连字符形可解
let ok = true;
try { await import("node:internal/streams/end-of-stream"); } catch { ok = false; }
console.log("hyphen-eos", ok);
try { await import("node:internal/streams/add-abort-signal"); } catch { ok = false; }
console.log("hyphen-aas", ok);
"#,
    );
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    for line in ["hooks-exist true", "fin-ctx true", "hyphen-eos true", "hyphen-aas true"] {
        assert!(text.lines().any(|l| l == line), "missing: {line}\nout: {text}");
    }
    // 边界（独立进程面）：无钩无 ALS 即假；tty_wrap 三键不可枚举。
    let (ok, out, _) = wjs(
        &[
            "--eval",
            "import('node:internal/async_hooks').then((m) => { const f = (m.default ?? m).enabledHooksExist; console.log('no-hooks', f() === false); });",
        ],
        &dir,
    );
    assert!(ok && out.contains("no-hooks true"), "out: {out}");
    let (ok, out, _) = wjs(
        &["--expose-internals", "--eval",
            "const { internalBinding } = require('internal/test/binding');\
             const TTY = internalBinding('tty_wrap').TTY;\
             const f = Object.prototype.propertyIsEnumerable.bind(TTY);\
             console.log('tty-enum', f('bytesRead') === false && f('fd') === false && f('_externalStream') === false);"],
        &dir,
    );
    assert!(ok && out.contains("tty-enum true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase11_stream_r2_iter_faces() {
    // P2-stream R2：`stream/iter` 门控面（node 原文口径）。
    // 正常（旗开）：push/write/end + text() 回环；Stream 命名空间冻结 + 工厂齐备；
    //   fromSync 跨 realm 按结构收（internal/types 口径）。
    // 报错：无旗下 `require("node:stream/iter")` 即 `No such built-in module`，
    //   裸名即 `Cannot find module`（disabled 套件同构）。
    // 边界：ERR 变体类构造器（`ERR_INVALID_STATE.TypeError` 可 new，R2-errors 面）。
    let dir = assert_fs::TempDir::new().unwrap();
    // 旗开面（显式子进程带旗；文件直写，不经无旗 run_node_file）。
    dir.child("r2.mjs")
        .write_str(
            r#"
import { push, text, fromSync } from "node:stream/iter";
import streamIter from "node:stream/iter";
const { writer, readable } = push();
writer.write("hello");
writer.end();
console.log("roundtrip", await text(readable) === "hello");
console.log("ns-frozen", Object.isFrozen(streamIter.Stream));
console.log("factories", ["push", "duplex", "from", "fromSync", "pull", "bytes", "text"].every((k) => typeof streamIter[k] === "function"));
import vm from "node:vm";
const cross = vm.runInNewContext("new Uint8Array([1,2,3])");
console.log("xrealm", (await text(fromSync([cross]))).length === 3);
const E = (await import("node:internal/errors")).codes;
console.log("err-variant", new E.ERR_INVALID_STATE.TypeError("x").code === "ERR_INVALID_STATE");
"#,
        )
        .unwrap();
    let out = winterjs2()
        .args(["--experimental-stream-iter", "--run", "r2.mjs"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    for line in ["roundtrip true", "ns-frozen true", "factories true", "xrealm true", "err-variant true"] {
        assert!(text.lines().any(|l| l == line), "missing: {line}\nout: {text}");
    }
    // 无旗面（报错双形）。
    let (ok, out, _) = wjs(
        &["--eval", "try { require('node:stream/iter'); } catch (e) { console.log('gated-node', e.message); }"],
        &dir,
    );
    assert!(ok && out.contains("gated-node No such built-in module: node:stream/iter"), "out: {out}");
    let (ok, out, _) = wjs(
        &["--eval", "try { require('stream/iter'); } catch (e) { console.log('gated-bare', e.message.slice(0, 27)); }"],
        &dir,
    );
    assert!(ok && out.contains("gated-bare Cannot find module 'stream"), "out: {out}");
    dir.close().unwrap();
}
