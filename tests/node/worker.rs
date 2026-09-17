//! tests/node/worker.rs — 对齐 src/builtins/node/worker.rs（node:worker_threads）。

use crate::helpers::*;
use assert_fs::prelude::*;

#[test]
fn phase9f_worker_channel_roundtrip() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { MessageChannel, MessagePort, receiveMessageOnPort } from "node:worker_threads";
const { port1, port2 } = new MessageChannel();
console.log("ch-ports", port1 instanceof MessagePort, port2 instanceof MessagePort);
port1.on("message", (m) => {
  console.log("ch-p1", JSON.stringify(m) === JSON.stringify({ n: 41 }));
  port1.postMessage([1, "x", true]);
});
port2.on("message", (m) => {
  console.log("ch-p2", Array.isArray(m) && m[1] === "x");
  port1.close(); port2.close();
});
port2.postMessage({ n: 41 });
// 迟挂监听：先投递再 on，newListener 开闸照样收到
const late = new MessageChannel();
late.port2.postMessage("late-hi");
await new Promise((r) => setTimeout(r, 20));
late.port1.on("message", (m) => {
  console.log("ch-late", m === "late-hi");
  late.port1.close(); late.port2.close();
});
// 无监听排队：receiveMessageOnPort 同步取出
const q = new MessageChannel();
q.port2.postMessage("q1");
q.port2.postMessage("q2");
await new Promise((r) => setTimeout(r, 20));
console.log("ch-recv", receiveMessageOnPort(q.port1).message === "q1", receiveMessageOnPort(q.port1).message === "q2", receiveMessageOnPort(q.port1) === undefined);
try { q.port2.postMessage(() => {}); } catch (e) { console.log("ch-fn", e.name === "DataCloneError"); }
q.port1.close(); q.port2.close();
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("ch-ports true true"), "out: {out}");
    assert!(out.contains("ch-p1 true"), "out: {out}");
    assert!(out.contains("ch-p2 true"), "out: {out}");
    assert!(out.contains("ch-late true"), "out: {out}");
    assert!(out.contains("ch-recv true true true"), "out: {out}");
    assert!(out.contains("ch-fn true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9f_worker_thread_info_boundary() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { isMainThread, threadId, parentPort, workerData, resourceLimits, SHARE_ENV, setEnvironmentData, getEnvironmentData, markAsUncloneable, moveMessagePortToContext, MessageChannel } from "node:worker_threads";
console.log("th-self", isMainThread === true, threadId === 0, parentPort === null, workerData === null);
console.log("th-res", typeof resourceLimits === "object", typeof SHARE_ENV === "symbol");
setEnvironmentData("wk", { v: 7 });
console.log("th-env", JSON.stringify(getEnvironmentData("wk")) === JSON.stringify({ v: 7 }), getEnvironmentData("missing") === undefined);
try { setEnvironmentData(42, 1); } catch (e) { console.log("th-badkey", e.code === "ERR_INVALID_ARG_TYPE"); }
try { getEnvironmentData(42); } catch (e) { console.log("th-badkey2", e.code === "ERR_INVALID_ARG_TYPE"); }
const o = { a: 1 };
markAsUncloneable(o);
const { port1, port2 } = new MessageChannel();
try { port2.postMessage(o); } catch (e) { console.log("th-unc", e.name === "DataCloneError"); }
console.log("th-move", moveMessagePortToContext(port1, {}) === port1);
port1.close(); port2.close();
// unref 端口不续命：不 close 照样退出
const u = new MessageChannel();
u.port1.unref(); u.port2.unref();
u.port2.postMessage("dropped");
console.log("th-unref", true);
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("th-self true true true true"), "out: {out}");
    assert!(out.contains("th-res true true"), "out: {out}");
    assert!(out.contains("th-env true true"), "out: {out}");
    assert!(out.contains("th-badkey true"), "out: {out}");
    assert!(out.contains("th-badkey2 true"), "out: {out}");
    assert!(out.contains("th-unc true"), "out: {out}");
    assert!(out.contains("th-move true"), "out: {out}");
    assert!(out.contains("th-unref true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9f_worker_eval_and_data() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { Worker, isMainThread, threadId } from "node:worker_threads";
console.log("wk-self", isMainThread === true, threadId === 0);
const w = new Worker("import { parentPort } from 'node:worker_threads'; parentPort.postMessage(40 + 2);", { eval: true });
console.log("wk-tid", w.threadId > 0);
w.on("online", () => console.log("wk-online", true));
w.on("message", (m) => console.log("wk-msg", m === 42));
w.on("error", (e) => console.log("wk-err", e.message));
w.on("exit", (c) => console.log("wk-exit", c === 0));
const d = new Worker("import { parentPort, workerData } from 'node:worker_threads'; parentPort.postMessage({ e: workerData.n + 1 });", { eval: true, workerData: { n: 41 } });
d.on("message", (m) => console.log("wk-data", m.e === 42));
d.on("exit", () => {});
d.on("error", (e) => console.log("wk-derr", e.message));
console.log("wk-ref", w.unref() === w, w.ref() === w);
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("wk-self true true"), "out: {out}");
    assert!(out.contains("wk-tid true"), "out: {out}");
    assert!(out.contains("wk-online true"), "out: {out}");
    assert!(out.contains("wk-msg true"), "out: {out}");
    assert!(out.contains("wk-exit true"), "out: {out}");
    assert!(out.contains("wk-data true"), "out: {out}");
    assert!(out.contains("wk-ref true true"), "out: {out}");
    assert!(!out.contains("wk-err"), "out: {out}");
    assert!(!out.contains("wk-derr"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9f_worker_twoway_terminate() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { Worker } from "node:worker_threads";
const w = new Worker("import { parentPort } from 'node:worker_threads'; parentPort.on('message', (m) => parentPort.postMessage(m * 2));", { eval: true });
w.on("online", () => w.postMessage(21));
w.on("message", (m) => {
  console.log("wx-msg", m === 42);
  w.terminate().then((c) => console.log("wx-term", c === 1));
});
w.on("exit", (c) => console.log("wx-exit", c === 1));
w.on("error", (e) => console.log("wx-err", e.message));
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("wx-msg true"), "out: {out}");
    assert!(out.contains("wx-term true"), "out: {out}");
    assert!(out.contains("wx-exit true"), "out: {out}");
    assert!(!out.contains("wx-err"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9f_worker_errors_boundary() {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("wfile.js").write_str("import { parentPort } from \"node:worker_threads\";\nparentPort.postMessage(\"file-ok\");\n").unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { Worker } from "node:worker_threads";
try { new Worker(42); } catch (e) { console.log("we-badfile", e.code === "ERR_INVALID_ARG_TYPE"); }
const f = new Worker("./wfile.js");
f.on("message", (m) => console.log("we-file", m === "file-ok"));
f.on("exit", () => {});
f.on("error", (e) => console.log("we-ferr", e.message));
const t = new Worker("throw new Error('boom-x')", { eval: true });
t.on("error", (e) => console.log("we-throw", e.message.includes("boom-x")));
t.on("exit", (c) => console.log("we-texit", c === 1));
const m = new Worker("./nope-missing.js");
m.on("error", (e) => console.log("we-miss", e.message.includes("nope-missing")));
m.on("exit", (c) => console.log("we-mexit", c === 1));
const e2 = new Worker("void 0", { eval: true });
e2.on("exit", (c) => {
  console.log("we-e2", c === 0);
  e2.postMessage("late-drop");
  e2.terminate().then((cc) => console.log("we-term2", cc === 0));
});
e2.on("error", (e) => console.log("we-e2err", e.message));
const n = new Worker("import { workerData } from 'node:worker_threads'; import { parentPort } from 'node:worker_threads'; parentPort.postMessage(workerData === null);", { eval: true });
n.on("message", (mm) => console.log("we-novalue", mm === true));
n.on("exit", () => {});
n.on("error", (e) => console.log("we-nerr", e.message));
const x = new Worker("process.exit(7);", { eval: true });
x.on("exit", (c) => console.log("we-code", c === 7));
x.on("error", (e) => console.log("we-xerr", e.message));
"#,
    );
    assert!(out.contains("we-badfile true"), "out: {out}");
    assert!(out.contains("we-file true"), "out: {out}");
    assert!(out.contains("we-throw true"), "out: {out}");
    assert!(out.contains("we-texit true"), "out: {out}");
    assert!(out.contains("we-miss true"), "out: {out}");
    assert!(out.contains("we-mexit true"), "out: {out}");
    assert!(out.contains("we-e2 true"), "out: {out}");
    assert!(out.contains("we-term2 true"), "out: {out}");
    assert!(out.contains("we-novalue true"), "out: {out}");
    assert!(out.contains("we-code true"), "out: {out}");
    assert!(!out.contains("we-ferr"), "out: {out}");
    assert!(!out.contains("we-e2err"), "out: {out}");
    assert!(!out.contains("we-nerr"), "out: {out}");
    assert!(!out.contains("we-xerr"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9i_worker_transfer_buffer_types() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { MessageChannel } from "node:worker_threads";
const { port1, port2 } = new MessageChannel();
const seen = [];
port2.on("message", (m) => { seen.push(m); });
const ab = new Uint8Array([1, 2, 3]).buffer;
port1.postMessage(ab, [ab]);
console.log("w9i-detach", ab.byteLength === 0);
const ab2 = new Uint8Array([4, 5]).buffer;
port1.postMessage(ab2);
console.log("w9i-copy", ab2.byteLength === 2);
const sub = new Uint8Array([1, 2, 3, 4]).subarray(1, 3);
port1.postMessage({ sub });
port1.postMessage({ bi: 5n, u: undefined, m: new Map([[1, 2]]), s: new Set([3]), d: new Date(0), ta: new Uint8Array([9]), dv: new DataView(new Uint8Array([7, 8]).buffer) });
const t = (n, f) => { try { f(); console.log(n, "NO-THROW"); } catch (e) { console.log(n, e.name); } };
t("w9i-baditem", () => port1.postMessage({ x: 1 }, [{ x: 1 }]));
t("w9i-dup", () => port1.postMessage("x", [ab2, ab2]));
t("w9i-detached", () => port1.postMessage(ab));
t("w9i-fn", () => port1.postMessage(() => {}));
// 循环/共享保留（M5 升级：先序 ref，真机口径；此前抛错记档作废）。
const circ = { n: 1 }; circ.me = circ; circ.arr = [circ];
port1.postMessage({ c: circ });
const shared = { v: 7 };
port1.postMessage({ a: shared, b: shared, m: new Map([["k", shared]]) });
setTimeout(() => {
  const [det, c2, subMsg, ty, circMsg, sharedMsg] = seen;
  console.log("w9i-gotxfer", det instanceof ArrayBuffer, det.byteLength === 3);
  console.log("w9i-gotbuf", c2 instanceof ArrayBuffer, c2.byteLength === 2, new Uint8Array(c2)[0] === 4);
  console.log("w9i-sub", ty !== undefined && subMsg.sub instanceof Uint8Array, subMsg.sub.length === 2, subMsg.sub[0] === 2, subMsg.sub.byteOffset === 1);
  console.log("w9i-types", typeof ty.bi === "bigint", ("u" in ty) && ty.u === undefined, ty.m instanceof Map && ty.m.get(1) === 2, ty.s instanceof Set && ty.s.has(3), ty.d instanceof Date && ty.d.getTime() === 0, ty.ta instanceof Uint8Array && ty.ta[0] === 9, ty.dv instanceof DataView && ty.dv.getUint8(1) === 8);
  console.log("w9i-circular", circMsg.c.n === 1, circMsg.c.me === circMsg.c, circMsg.c.arr[0] === circMsg.c, circMsg.c.arr !== circ.arr);
  console.log("w9i-shared", sharedMsg.a.v === 7, sharedMsg.a === sharedMsg.b, sharedMsg.m.get("k") === sharedMsg.a);
  port1.close();
  port2.close();
}, 200);
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    for line in [
        "w9i-detach true",
        "w9i-copy true",
        "w9i-baditem DataCloneError",
        "w9i-dup DataCloneError",
        "w9i-detached DataCloneError",
        "w9i-fn DataCloneError",
        "w9i-gotxfer true true",
        "w9i-gotbuf true true true",
        "w9i-sub true true true true",
        "w9i-types true true true true true true true",
        "w9i-circular true true true true",
        "w9i-shared true true true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase9i_worker_transfer_port_migration() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { MessageChannel, MessagePort } from "node:worker_threads";
const { port1, port2 } = new MessageChannel();
const { port1: a1, port2: a2 } = new MessageChannel();
let a1got = [];
a1.on("message", (m) => { a1got.push(m); });
port1.postMessage({ p: a2 }, [a2]);
// 源端 neutered：后用静默，a1 收不到。
a2.postMessage("lost");
port2.on("message", (m) => {
  const ok = m.p instanceof MessagePort;
  console.log("w9i-mig", ok);
  if (!ok) return;
  globalThis.__mig = m.p;
  m.p.on("message", (x) => console.log("w9i-migmsg", x === "to-migrated"));
  m.p.postMessage("to-a1");
  a1.postMessage("to-migrated");
});
const t = (n, f) => { try { f(); console.log(n, "NO-THROW"); } catch (e) { console.log(n, e.name); } };
t("w9i-portnotransfer", () => port1.postMessage({ p: a1 }));
setTimeout(() => {
  console.log("w9i-neuter", !a1got.includes("lost"), a1got.includes("to-a1"));
  if (globalThis.__mig) globalThis.__mig.close();
  port1.close();
  port2.close();
  a1.close();
}, 300);
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    for line in [
        "w9i-mig true",
        "w9i-migmsg true",
        "w9i-portnotransfer DataCloneError",
        "w9i-neuter true true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase9i_worker_transfer_cross_thread_and_broadcast() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { MessageChannel, BroadcastChannel, Worker } from "node:worker_threads";
// 同会话 BC：自收排除、关者止收。
const b1 = new BroadcastChannel("w9i-bc");
const b2 = new BroadcastChannel("w9i-bc");
const b3 = new BroadcastChannel("w9i-other");
const seen = [];
b2.onmessage = (e) => { seen.push(["b2", e.data]); };
b1.onmessage = () => { seen.push(["b1", "SELF"]); };
b3.onmessage = (e) => { seen.push(["b3", e.data]); };
b1.postMessage("hello");
await new Promise((r) => setTimeout(r, 100));
b2.close();
b1.postMessage("after");
await new Promise((r) => setTimeout(r, 100));
// 跨线程：端口经 workerData 迁移 + BC 跨线程扇出。
const { port1, port2 } = new MessageChannel();
const back = [];
port1.on("message", (m) => { back.push(m); });
const w = new Worker(
  "import { parentPort, workerData, BroadcastChannel } from 'node:worker_threads';" +
  "const bc = new BroadcastChannel('w9i-x');" +
  "workerData.p.on('message', (m) => { parentPort.postMessage('w saw ' + m); bc.postMessage('from-worker'); });" +
  "workerData.p.postMessage('hi-main');",
  { eval: true, workerData: { n: 41n, p: port2 }, transferList: [port2] }
);
const xseen = [];
const xb = new BroadcastChannel("w9i-x");
xb.onmessage = (e) => { xseen.push(e.data); };
w.on("message", (m) => { back.push("W:" + m); });
w.on("error", (e) => { back.push("ERR" + e.message); });
port1.postMessage("hi-worker");
setTimeout(() => {
  console.log("w9i-bc", JSON.stringify(seen) === JSON.stringify([["b2", "hello"]]));
  console.log("w9i-xfer", back.includes("hi-main"), back.includes("W:w saw hi-worker"));
  console.log("w9i-xbc", xseen.includes("from-worker"));
  w.terminate().then(() => {
    b1.close();
    b3.close();
    xb.close();
    port1.close();
  });
}, 600);
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    for line in ["w9i-bc true", "w9i-xfer true true", "w9i-xbc true"] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase9m_worker_stdio_surface() {
    // Worker 标出流：无选项 null；stdout:true 即有 pipe/unpipe + end/close
    // 语义（无数据流，退出即 end）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { Worker } from "node:worker_threads";
const a = new Worker("import { parentPort } from 'node:worker_threads'; parentPort.postMessage(1);", { eval: true });
console.log("null-stdio", a.stdout === null, a.stderr === null);
a.on("exit", () => {});
const b = new Worker("import { parentPort } from 'node:worker_threads'; parentPort.postMessage(2);", { eval: true, stdout: true, stderr: true });
console.log("has-stdio", b.stdout !== null, b.stderr !== null, typeof b.stdout.pipe, typeof b.stdout.unpipe);
let ended = 0;
b.stdout.once("end", () => { ended++; });
b.stdout.once("close", () => { ended++; });
b.on("message", () => {});
b.on("exit", (c) => console.log("stdio-exit", c === 0, ended === 2, b.stdout.readableEnded));
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    for line in ["null-stdio true true", "has-stdio true true function function", "stdio-exit true true true"] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_worker_error_shape_and_event_faces() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p10f.mjs",
        r#"
import { Worker, MessageChannel, MessagePort, BroadcastChannel, workerData, threadName, parentPort } from "node:worker_threads";
import assert from "node:assert";

// ── worker 分支：threadName 导出 + 抛原始值（number/string 各一 worker）──
if (workerData?.throwKind === "num") {
  parentPort.postMessage("armed");
  throw 42;
}
if (workerData?.throwKind === "str") {
  parentPort.postMessage("armed");
  throw "boom";
}

// 1) MessageEvent 全局：默认面 + init 转换（WebIDL USVString/DOMString）。
assert.strictEqual(typeof MessageEvent, "function");
const ev = new MessageEvent("message", { data: 2, origin: 1, lastEventId: 0 });
console.log("p1", `${ev.type}|${ev.data}|${ev.origin}|${ev.lastEventId}|${ev.source}|${JSON.stringify(ev.ports)}` === "message|2|1|0|null|[]");
console.log("p1-inst", new MessageEvent("m") instanceof Event);
let t1 = false;
try { new MessageEvent("m", { source: 1 }); } catch (e) { t1 = /Expected eventInitDict\.source \("1"\) to be an instance of MessagePort\./.test(e.message); }
let t2 = false;
try { new MessageEvent("m", { ports: 0 }); } catch (e) { t2 = /eventInitDict\.ports \(0\) is not iterable\./.test(e.message); }
let t3 = false;
try { new MessageEvent("m", { ports: [null] }); } catch (e) { t3 = /Expected eventInitDict\.ports\[0\] \("null"\) to be an instance of MessagePort\./.test(e.message); }
console.log("p1-err", t1, t2, t3);

// 2) MessagePort EventTarget 双面：自定义类型 CustomEvent(detail)；EE 裸值不变；
//    onmessage 收真 MessageEvent（data/target/ports）。
{
  const { port1, port2 } = new MessageChannel();
  let etType = "", etDetail = "", eeVal = "";
  port2.addEventListener("foo", (e) => { etType = e.type; etDetail = e.detail; });
  port2.on("foo", (v) => { eeVal = v; });
  port2.emit("foo", "bar");
  console.log("p2", `${etType}|${etDetail}|${eeVal}` === "foo|bar|bar");
  // removeEventListener 摘净
  const fn = () => { etType = "BAD"; };
  port2.addEventListener("foo", fn);
  port2.removeEventListener("foo", fn);
  port2.emit("foo", "x");
  console.log("p2-rm", etType === "foo");
  const got = await new Promise((res) => {
    port1.onmessage = (m) => res(m);
    port2.postMessage(4);
  });
  console.log("p2-om", got instanceof MessageEvent, got.data === 4, got.target === port1, Array.isArray(got.ports) && got.ports.length === 0);
  port1.close(); port2.close();
}

// 3) BroadcastChannel：message 事件收 MessageEvent(data)；缺参/已关报错。
{
  const bc1 = new BroadcastChannel("ch-10f");
  const bc2 = new BroadcastChannel("ch-10f");
  const got = await new Promise((res) => {
    bc1.addEventListener("message", (e) => res(e));
    bc2.postMessage("hello");
  });
  console.log("p3", got instanceof MessageEvent, got.data === "hello");
  bc1.close(); bc2.close();
  const bcX = new BroadcastChannel("ch-10f");
  bcX.close(); bcX.close();
  let threw1 = "";
  try { bcX.postMessage(null); } catch (e) { threw1 = e.message; }
  let threw2 = "";
  const bcY = new BroadcastChannel("ch-10f");
  try { bcY.postMessage(); } catch (e) { threw2 = e.message; }
  bcY.close();
  console.log("p3-err", threw1 === "BroadcastChannel is closed", threw2 === 'The "message" argument must be specified');
}

// 4) threadName（属性 + 退出置 null）+ resourceLimits 缺省 {}。
{
  const w = new Worker(new URL(import.meta.url).pathname, { name: "tn-10f", workerData: { throwKind: "num" } });
  console.log("p4-name", w.threadName === "tn-10f", JSON.stringify(w.resourceLimits) === "{}");
  const errs = [];
  w.on("error", (e) => errs.push(e));
  w.on("exit", (c) => {
    console.log("p4-prim", c === 1, errs.length === 1, typeof errs[0] === "number", errs[0] === 42);
    console.log("p4-null", w.threadName === null);
    // string 原始值
    const w2 = new Worker(new URL(import.meta.url).pathname, { workerData: { throwKind: "str" } });
    const errs2 = [];
    w2.on("error", (e) => errs2.push(e));
    w2.on("exit", (c2) => {
      console.log("p4-str", c2 === 1, errs2.length === 1, errs2[0] === "boom");
      run5();
    });
  });
}

// 5) 缺主模块：error 事件文案 node 形（Cannot find module '<abs>'）。
function run5() {
  const missing = new URL("file:///no/such/worker-10f-does-not-exist.js");
  const w3 = new Worker(missing);
  w3.on("error", (e) => console.log("p5", /Cannot find module .+worker-10f-does-not-exist\.js/.test(e.message)));
  w3.on("exit", () => console.log("END"));
}
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    for line in [
        "p1 true",
        "p1-inst true",
        "p1-err true true true",
        "p2 true",
        "p2-rm true",
        "p2-om true true true true",
        "p3 true true",
        "p3-err true true",
        "p4-name true true",
        "p4-prim true true true true",
        "p4-null true",
        "p4-str true true true",
        "p5 true",
        "END",
    ] {
        assert!(stdout.lines().any(|l| l == line), "missing line: {line}\nout: {stdout}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_worker_typed_view_and_sab_envelope() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p10f-view.mjs",
        r#"
import { MessageChannel, MessagePort } from "node:worker_threads";
import assert from "node:assert";

// 边界：BPE>1 的 typed array 跨端（byteLength → 元素数折算；修前 Int32/Float64
// 全 OOB 静默丢消息）+ 子视图偏移 + DataView + SAB 品牌副本。
const { port1, port2 } = new MessageChannel();
port2.on("message", (m) => {
  port2.postMessage(m);
});
const results = await new Promise((res) => {
  const out = [];
  let step = 0;
  port1.on("message", (m) => {
    out.push(m);
    step++;
    if (step === 4) res(out);
  });
  port1.postMessage(new Int32Array([1, 2, 3, 4]));
  const f = new Float64Array([1.5, -2.5]);
  port1.postMessage(f);
  const ab = new ArrayBuffer(16);
  port1.postMessage(new Uint8Array(ab, 4, 8));
  port1.postMessage(new DataView(new ArrayBuffer(8)));
});
console.log(
  "view-i32",
  results[0] instanceof Int32Array, JSON.stringify([...results[0]]) === "[1,2,3,4]",
);
console.log("view-f64", results[1] instanceof Float64Array, JSON.stringify([...results[1]]) === "[1.5,-2.5]");
console.log("view-sub", results[2] instanceof Uint8Array, results[2].byteOffset === 4, results[2].byteLength === 8, results[2].buffer.byteLength === 16);
console.log("view-dv", results[3] instanceof DataView, results[3].byteLength === 8);

// SAB：品牌 roundtrip（副本语义；真共享内存跨线程底座记档）。
if (typeof SharedArrayBuffer === "function") {
  const sab = new SharedArrayBuffer(8);
  new Uint8Array(sab).set([7, 7]);
  const got = await new Promise((res) => {
    port1.on("message", (m) => { res(m); });
    port1.postMessage(sab);
  });
  console.log("sab", got instanceof SharedArrayBuffer, got.byteLength === 8, new Uint8Array(got)[0] === 7);
} else {
  console.log("sab skip");
}
port1.close(); port2.close();
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    for line in [
        "view-i32 true true",
        "view-f64 true true",
        "view-sub true true true true",
        "view-dv true true",
        "sab true true true",
    ] {
        assert!(stdout.lines().any(|l| l == line), "missing line: {line}\nout: {stdout}");
    }
    dir.close().unwrap();
}
