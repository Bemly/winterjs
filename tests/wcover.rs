//! 原生覆盖 B1（WinterJS2.assert/util/punycode；Web 风）。
//! 正常 + 报错 + 边界三件；后续批次追加同文件。

mod common;

use assert_fs::prelude::*;
use common::*;

fn eval_ok(code: &str) -> String {
    let out = winterjs2().args(["--eval", code]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn wcover_b1_assert_faces() {
    let stdout = eval_ok(
        r#"WinterJS2.assert.ok(true); WinterJS2.assert.equal(1, "1"); WinterJS2.assert.strictEqual(1, 1); WinterJS2.assert.deepEqual({ a: [1, { b: 2 }] }, { a: [1, { b: 2 }] }); WinterJS2.assert.throws(() => { throw new Error("x"); }); await WinterJS2.assert.rejects(async () => { throw new Error("x"); }); WinterJS2.assert.match("foobar", /oob/); console.log("assert-faces-ok");"#,
    );
    assert!(stdout.contains("assert-faces-ok"), "out: {stdout}");
}

#[test]
fn wcover_b1_util_puny_faces() {
    let stdout = eval_ok(
        r#"console.log("fmt", WinterJS2.util.format("%s=%d %j", "a", 1, { x: 1 })); console.log("insp", WinterJS2.util.inspect({ a: 1 }).includes("a: 1")); console.log("puny", WinterJS2.punycode.toASCII("münchen.de"), WinterJS2.punycode.toUnicode("xn--mnchen-3ya.de"), WinterJS2.punycode.encode("bücher"), WinterJS2.punycode.decode("bcher-kva")); console.log("ucs2", JSON.stringify(WinterJS2.punycode.ucs2.decode("hi")));"#,
    );
    assert!(stdout.contains("fmt a=1 {\"x\":1}") || stdout.contains("fmt a=1"), "out: {stdout}");
    assert!(stdout.contains("insp true"), "out: {stdout}");
    assert!(stdout.contains("puny xn--mnchen-3ya.de münchen.de"), "out: {stdout}");
    assert!(stdout.contains("ucs2 [104,105]"), "out: {stdout}");
}

#[test]
fn wcover_b1_errors_boundary() {
    let stdout = eval_ok(
        r#"const t = (n, f) => { try { const r = f(); if (r && r.then) { r.then(() => console.log(n, "NO-THROW"), () => console.log(n, "THROW", "AssertionError")); } else console.log(n, "NO-THROW"); } catch (e) { console.log(n, "THROW", e.name); } }; t("ok", () => WinterJS2.assert.ok(false)); t("eq", () => WinterJS2.assert.strictEqual(1, "1")); t("deep", () => WinterJS2.assert.deepEqual({ a: 1 }, { a: 2 })); t("throws", () => WinterJS2.assert.throws(() => {})); t("notthrows", () => WinterJS2.assert.doesNotThrow(() => { throw new Error("x"); })); t("match", () => WinterJS2.assert.match("foo", /z/)); t("nan", () => WinterJS2.assert.deepEqual(NaN, NaN));"#,
    );
    for name in ["ok", "eq", "deep", "throws", "notthrows", "match"] {
        assert!(stdout.contains(&format!("{name} THROW AssertionError")), "out: {stdout}");
    }
    // NaN 自等（Object.is 口径）不抛。
    assert!(stdout.contains("nan NO-THROW"), "out: {stdout}");
}

#[test]
fn wcover_b3_command_faces() {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("p.mjs")
        .write_str(
            r#"
const exe = process.execPath;
const r = await WinterJS2.command.run(exe, ["--eval", "40 + 2"]);
console.log("run", r.code, new TextDecoder().decode(r.stdout).trim());
const h = WinterJS2.command.spawn(exe, ["--eval", "40 + 2"]);
console.log("pid", h.pid > 0);
let out = "";
for await (const c of h.stdout) out += new TextDecoder().decode(c);
console.log("spawn", out.trim());
console.log("wait", JSON.stringify(await h.wait()));
try {
  await WinterJS2.command.run("/nonexistent-binary-xyz", []);
  console.log("missing NO-THROW");
} catch (e) { console.log("missing THROW", e.constructor.name); }
console.log("term", typeof WinterJS2.terminal.createInterface === "function");
console.log("repl", typeof WinterJS2.repl.start === "function");
"#,
        )
        .unwrap();
    // 跑子进程需 --allow-run（ Brooke 门控与 node 侧同）。
    let out = winterjs2()
        .args(["--run", "p.mjs", "--allow-run"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    for line in ["run 0 42", "pid true", "spawn 42", "missing THROW", "term true", "repl true"] {
        assert!(stdout.contains(line), "missing: {line}\nout: {stdout}");
    }
    assert!(stdout.contains("\"code\":0"), "out: {stdout}");
    dir.close().unwrap();
}

#[test]
fn wcover_b4_conc_faces() {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("echo.mjs")
        .write_str(
            r#"
import { parentPort } from "node:worker_threads";
parentPort.on("message", (v) => { parentPort.postMessage(JSON.stringify({ echo: v })); });
"#,
        )
        .unwrap();
    dir.child("p.mjs")
        .write_str(
            r#"
const w = new Worker("echo.mjs");
w.postMessage({ n: 7 });
const got = await new Promise((res, rej) => {
  w.onmessage = (e) => res(e.data);
  w.onerror = (e) => rej(e.error);
  setTimeout(() => rej(new Error("timeout")), 5000);
});
console.log("worker", JSON.stringify(got) === '{"echo":{"n":7}}');
w.terminate();
console.log("vm", WinterJS2.vm.run("40 + 2") === 42);
console.log("cluster", typeof WinterJS2.cluster.isPrimary === "boolean");
console.log("test", typeof WinterJS2.test.test === "function");
"#,
        )
        .unwrap();
    let out = winterjs2()
        .args(["--run", "p.mjs"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    for line in ["worker true", "vm true", "cluster true", "test true"] {
        assert!(stdout.contains(line), "missing: {line}\nout: {stdout}");
    }
    dir.close().unwrap();
}

#[test]
fn wcover_b5_sys_faces() {
    let stdout = eval_ok(
        r#"console.log("os", typeof WinterJS2.os.platform() === "string" && typeof WinterJS2.os.arch() === "string"); console.log("path", WinterJS2.path.join("a", "b") === ("a" + WinterJS2.path.sep + "b")); const db = WinterJS2.db.open(":memory:"); db.exec("CREATE TABLE t(a)"); console.log("db", JSON.stringify(db.run("INSERT INTO t VALUES (7)", []))); console.log("q", JSON.stringify(db.query("SELECT * FROM t", []))); db.close(); console.log("inspect", WinterJS2.inspect.evaluate("1+1") === 2); console.log("tty", typeof WinterJS2.tty.isTTY() === "boolean");"#,
    );
    assert!(stdout.contains("os true"), "out: {stdout}");
    assert!(stdout.contains("path true"), "out: {stdout}");
    assert!(stdout.contains(r#""changes":1"#), "out: {stdout}");
    assert!(stdout.contains("[[7]]"), "out: {stdout}");
    assert!(stdout.contains("inspect true"), "out: {stdout}");
    assert!(stdout.contains("tty true"), "out: {stdout}");
}

#[test]
fn wcover_b6_misc_faces() {
    let stdout = eval_ok(
        r#"const rs = new ReadableStream({ start(c) { c.enqueue(new TextEncoder().encode("hi")); c.close(); } }); let out = ""; const ws = new WritableStream({ write(c) { out += new TextDecoder().decode(c); } }); await WinterJS2.stream.pipeline(rs, ws); console.log("pipe", out); const ch = WinterJS2.diagnostics.channel("wcover"); let seen = null; ch.subscribe((m) => { seen = m; }); ch.publish({ a: 1 }); console.log("diag", JSON.stringify(seen)); const d = WinterJS2.domain.create(); let ran = false; d.run(() => { ran = true; }); console.log("domain", ran); console.log("trace", typeof WinterJS2.trace.getEnabledCategories() === "string"); const als = new WinterJS2.AsyncLocalStorage(); console.log("als", als.run("v", () => als.getStore())); console.log("crypto", typeof WinterJS2.crypto.subtle === "object");"#,
    );
    assert!(stdout.contains("pipe hi"), "out: {stdout}");
    assert!(stdout.contains(r#"diag {"a":1}"#), "out: {stdout}");
    assert!(stdout.contains("domain true"), "out: {stdout}");
    assert!(stdout.contains("trace true"), "out: {stdout}");
    assert!(stdout.contains("als v"), "out: {stdout}");
    assert!(stdout.contains("crypto true"), "out: {stdout}");
}

#[test]
fn wcover_b6_serve_face() {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("p.mjs")
        .write_str(
            r#"
const s = await WinterJS2.serve({ port: 0, hostname: "127.0.0.1" }, (req) => new Response("wcover-serve"));
console.log("srv", typeof s.shutdown === "function");
const res = await fetch(`http://127.0.0.1:${s.addr.port}/`);
console.log("fetch", (await res.text()) === "wcover-serve");
await s.shutdown();
"#,
        )
        .unwrap();
    let out = winterjs2()
        .args(["--run", "p.mjs"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("srv true"), "out: {stdout}");
    assert!(stdout.contains("fetch true"), "out: {stdout}");
    dir.close().unwrap();
}

#[test]
fn wcover_b2_tcp_udp_dns_faces() {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("p.mjs")
        .write_str(
            r#"
const s = await WinterJS2.tcp.listen(0, "127.0.0.1");
console.log("listen", typeof s.address().port === "number");
const c = await WinterJS2.tcp.connect("127.0.0.1", s.address().port);
c.write("ping");
for await (const sock of s) {
  let got = "";
  for await (const chunk of sock) { got += new TextDecoder().decode(chunk); break; }
  console.log("echo-got", got);
  sock.write(got);
  sock.end();
  break;
}
let back = "";
for await (const chunk of c) back += new TextDecoder().decode(chunk);
console.log("rt", back);
c.destroy();
s.close();
const addrs = await WinterJS2.dns.lookup("localhost");
console.log("lookup", Array.isArray(addrs) && addrs.length > 0);
const u = await WinterJS2.udp.bind(0, "127.0.0.1");
const v = await WinterJS2.udp.bind(0, "127.0.0.1");
u.send("hello", v.address().port, "127.0.0.1");
for await (const m of v) {
  console.log("udp", new TextDecoder().decode(m.data) === "hello" && m.remote.port === u.address().port);
  break;
}
u.close();
v.close();
"#,
        )
        .unwrap();
    let out = winterjs2()
        .args(["--run", "p.mjs"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    for line in ["listen true", "echo-got ping", "rt ping", "lookup true", "udp true"] {
        assert!(stdout.contains(line), "missing: {line}\nout: {stdout}");
    }
    dir.close().unwrap();
}

#[test]
fn wcover_b2_errors_boundary() {
    let stdout = eval_ok(
        r#"const t = async (n, f) => { try { await f(); console.log(n, "NO-THROW"); } catch (e) { console.log(n, "THROW", e.constructor.name); } }; await t("host", () => WinterJS2.tcp.connect("", 80)); await t("port", () => WinterJS2.tcp.connect("127.0.0.1", "x")); await t("refused", () => WinterJS2.tcp.connect("127.0.0.1", 1)); await t("dns", () => WinterJS2.dns.lookup("")); await t("udp", () => WinterJS2.udp.bind(-1));"#,
    );
    for name in ["host", "port", "refused", "dns", "udp"] {
        assert!(stdout.contains(&format!("{name} THROW")), "out: {stdout}");
    }
}
