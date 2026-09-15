//! tests/node/net.rs — 对齐 src/builtins/node/net.rs（node:net（含 http 流桩回环））。

use crate::common::*;
use crate::helpers::*;

#[test]
fn phase9d_net_echo_loopback() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import net, { Socket, createServer, createConnection } from "node:net";
import assert from "node:assert";
const server = createServer((sock) => {
  assert.ok(sock instanceof Socket);
  sock.on("data", (chunk) => {
    console.log("srv-recv", typeof chunk, String(chunk), sock.remoteAddress, sock.remotePort > 0);
    sock.write("echo:" + String(chunk));
  });
  sock.on("end", () => { console.log("srv-end"); sock.end(); });
  sock.on("close", () => console.log("srv-close"));
});
server.listen(0, "127.0.0.1", () => {
  const addr = server.address();
  console.log("listening", typeof addr.port === "number" && addr.port > 0, addr.address, addr.family);
  const s = net.connect(addr.port, "127.0.0.1", () => {
    console.log("cli-connect-cb");
  });
  s.on("connect", () => {
    console.log("cli-connect", s.remoteAddress, s.localAddress !== null);
    s.write("ping");
  });
  s.on("data", (chunk) => {
    console.log("cli-recv", String(chunk));
    s.end();
  });
  s.on("end", () => console.log("cli-end"));
  s.on("close", () => { console.log("cli-close"); server.close(); });
});
server.on("close", () => console.log("server-closed"));
// 第二连接：destroy 硬关 + write after destroy 报错
const srv2 = createServer((sock) => {
  sock.on("data", () => { sock.destroy(); });
});
srv2.listen(0, "127.0.0.1", () => {
  const c = createConnection(srv2.address().port, "127.0.0.1");
  c.on("connect", () => {
    c.write("boom");
  });
  c.on("close", () => {
    console.log("destroyed-close");
    try { c.write("late"); } catch (e) { console.log("wae", e.code); }
    srv2.close();
  });
});
setTimeout(() => console.log("end-ok"), 200);
"#,
    );
    assert!(out.contains("srv-recv object ping 127.0.0.1 true"), "out: {out}");
    assert!(out.contains("listening true 127.0.0.1 IPv4"), "out: {out}");
    assert!(out.contains("cli-connect-cb"), "out: {out}");
    assert!(out.contains("cli-connect 127.0.0.1 true"), "out: {out}");
    assert!(out.contains("cli-recv echo:ping"), "out: {out}");
    assert!(out.contains("cli-end"), "out: {out}");
    assert!(out.contains("cli-close"), "out: {out}");
    assert!(out.contains("srv-end"), "out: {out}");
    assert!(out.contains("srv-close"), "out: {out}");
    assert!(out.contains("server-closed"), "out: {out}");
    assert!(out.contains("destroyed-close"), "out: {out}");
    assert!(out.contains("wae ERR_STREAM_DESTROYED"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9d_net_server_errors() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { createServer } from "node:net";
// 占位 server 抢住端口，第二个 server 绑定同端口 → 'error' 事件 EADDRINUSE
const holder = createServer(() => {});
holder.listen(0, "127.0.0.1", () => {
  const port = holder.address().port;
  const s2 = createServer(() => {});
  s2.on("error", (e) => {
    console.log("bind-err", e.code, e.port === port);
    holder.close();
  });
  s2.on("close", () => console.log("s2-close"));
  s2.listen(port, "127.0.0.1");
});
holder.on("close", () => console.log("holder-close"));
setTimeout(() => console.log("end-ok"), 200);
"#,
    );
    assert!(out.contains("bind-err EADDRINUSE true"), "out: {out}");
    assert!(out.contains("s2-close"), "out: {out}");
    assert!(out.contains("holder-close"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}

// ── Phase 9d-2：node:dns（hermetic，仅 localhost/回环）──────────────────────

#[test]
fn phase9d_net_http_stream_stubs() {
    // ws/vite 等库直调的流最小面：pause/resume/setTimeout/cork/uncork
    // no-op 链式返回自身，read 恒 null；net.isIP 三态。缺桩曾报
    // `stream.resume is not a function`（M5 dev 实测）。
    let out = winterjs()
        .args(["--eval",
        r#"const net = await import("node:net"); const http = await import("node:http");
const s = new net.Socket();
console.log("sock", s.pause() === s, s.resume() === s, s.setTimeout() === s, s.read() === null, s.cork() === s, s.uncork() === s, s.setNoDelay() === s, s.setKeepAlive() === s);
console.log("isip", net.isIP("127.0.0.1"), net.isIP("::1"), net.isIP("nope"), net.isIPv4("1.2.3.4"), net.isIPv6("::1"));
const req = new http.IncomingMessage();
console.log("req", req.pause() === req, req.resume() === req, req.read() === null);
const res = new http.ServerResponse({ write() {}, end() {} });
console.log("res", res.cork() === res, res.uncork() === res);"#])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "sock true true true true true true true true\nisip 4 6 0 true true\nreq true true true\nres true true\n"
    );
}

#[test]
fn phase10f_net_autoselect_timeout() {
    // 10f：get/setDefaultAutoSelectFamilyAttemptTimeout（存值面；test/common 前置）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import net from "node:net";
console.log("def", net.getDefaultAutoSelectFamilyAttemptTimeout());
net.setDefaultAutoSelectFamilyAttemptTimeout(1000);
console.log("set", net.getDefaultAutoSelectFamilyAttemptTimeout());
try { net.setDefaultAutoSelectFamilyAttemptTimeout(-1); } catch (e) { console.log("neg", e.code); }
try { net.setDefaultAutoSelectFamilyAttemptTimeout("x"); } catch (e) { console.log("str", e.code); }
console.log("kept", net.getDefaultAutoSelectFamilyAttemptTimeout());
"#,
    );
    assert!(out.contains("def 500"), "out: {out}");
    assert!(out.contains("set 1000"), "out: {out}");
    assert!(out.contains("neg ERR_OUT_OF_RANGE"), "out: {out}");
    assert!(out.contains("str ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("kept 1000"), "out: {out}");
    dir.close().unwrap();
}
