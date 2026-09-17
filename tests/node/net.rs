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

#[test]
fn phase10f_socket_settimeout_fires_without_closing() {
    // 10f timers 对拍：socket.setTimeout(ms[, cb]) 真实现——单发 'timeout'
    // 事件（Node 口径：不关连接、socket 仍可写；cb 注册为 once 监听），
    // 内部 timer 恒 unref（套件 test-timers-socket-timeout-removes-other-socket-
    // unref-timer 形状收窄为 hermetic 单 socket）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import net from "node:net";
const server = net.createServer((sock) => {
  sock.setTimeout(30, () => {
    console.log("timeout-fired", sock.writable, sock.destroyed === false);
    sock.end();
  });
});
server.listen(0, "127.0.0.1", () => {
  const addr = server.address();
  const c = net.connect(addr.port, "127.0.0.1", () => {
    console.log("cli-connect");
  });
  c.on("close", () => server.close(() => console.log("closed")));
});
"#,
    );
    assert!(out.contains("cli-connect"), "out: {out}");
    assert!(out.contains("timeout-fired true true"), "out: {out}");
    assert!(out.contains("closed"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase10f_net_write_after_destroy_cb() {
    // 10f net 对拍：destroy 后 write 有 cb 走 cb(err)+false、无 cb 才同步抛；
    // WRITE_AFTER_END（end 后）与 DESTROYED（destroy 后）双码；destroy 无参不发 error。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import net from "node:net";
const s = new net.Socket();
let errEv = 0;
s.on("error", () => { errEv++; });
s.destroy();
s.write("x", (e) => console.log("cb-code", e && e.code));
try { s.write("x"); console.log("no-throw BAD"); } catch (e) { console.log("threw", e.code); }
const srv = net.createServer((sock) => { sock.resume(); sock.on("end", () => sock.end()); });
srv.listen(0, "127.0.0.1", () => {
  const c = net.connect(srv.address().port, "127.0.0.1", () => {
    c.end("hello");
    c.write("x", (e) => console.log("wae-cb", e && e.code));
    try { c.write("y"); console.log("wae-ret BAD"); } catch (e) { console.log("wae-threw", e.code); }
    c.on("error", () => {});
    setTimeout(() => { console.log("errEv", errEv); srv.close(); }, 300);
  });
});
"#,
    );
    assert!(out.contains("cb-code ERR_STREAM_DESTROYED"), "out: {out}");
    assert!(out.contains("threw ERR_STREAM_DESTROYED"), "out: {out}");
    // write-after-end 无 cb 形：同步抛（真机 ret=false + error 事件；本仓抛 WRITE_AFTER_END，
    // 偏离记档——error 事件已发，抛码与真机 ret 形不同，见 bun-parity）。
    assert!(out.contains("wae-threw ERR_STREAM_WRITE_AFTER_END"), "out: {out}");
    assert!(out.contains("wae-cb ERR_STREAM_WRITE_AFTER_END"), "out: {out}");

    assert!(out.contains("errEv 0"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase10f_net_blocklist_and_lookup() {
    // 10f net 对拍：connect { blockList } 命中即 ERR_IP_BLOCKED；自定义 lookup 生效。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import net from "node:net";
const bl = new net.BlockList();
bl.addAddress("127.0.0.1");
console.log("check", bl.check("127.0.0.1"), bl.check("127.0.0.2"), bl.size);
const s = net.connect({ port: 9999, host: "127.0.0.1", blockList: bl });
s.on("error", (e) => console.log("blocked", e.code));
const srv = net.createServer((sock) => { sock.resume(); sock.on("data", (d) => sock.end(d)); });
srv.listen(0, "127.0.0.1", () => {
  const port = srv.address().port;
  const c = net.connect({ port, host: "localhost", lookup: (_, __, cb) => cb(null, "127.0.0.1", 4) });
  c.on("connect", () => { console.log("lookup-conn"); c.end("ping"); });
  c.on("data", (d) => console.log("lookup-got", String(d)));
  c.on("close", () => srv.close(() => console.log("done")));
  c.on("error", (e) => console.log("lookup-err", e.code));
});
"#,
    );
    assert!(out.contains("check true false 1"), "out: {out}");
    assert!(out.contains("blocked ERR_IP_BLOCKED"), "out: {out}");
    assert!(out.contains("lookup-conn"), "out: {out}");
    assert!(out.contains("lookup-got ping"), "out: {out}");
    assert!(out.contains("done"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase10f_net_isip_zone_and_pending() {
    // 10f net 对拍：isIP zone 尾（%eth0 收 / %@ 拒）+ pending/readyState/connecting 三态。
    let out = winterjs()
        .args(["--eval",
        r#"const net = await import("node:net");
console.log("zone", net.isIP("fe80::2008%eth0"), net.isIP("fe80::2008%eth0@1"), net.isIP("::1"), net.isIP("1.2.3.4"), net.isIP("nope"));
const s = new net.Socket();
console.log("pre", s.pending, s.readyState, s.connecting);
console.log("exit-ok");"#])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("zone 6 0 6 4 0"), "out: {text}");
    assert!(text.contains("pre true open false"), "out: {text}");
}

#[test]
fn phase10f_net_unix_socket_roundtrip() {
    // 10f net 对拍：listen(path)/connect(path) UDS 回环（地址全 undefined，address() 回 {} / path 串）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.cjs",
        r#"
const net = require("node:net");
const path = require("node:path");
const P = path.join(__dirname || ".", "t10f.sock");
const srv = net.createServer((s) => {
  console.log("srv-remote", String(s.remoteAddress), "family", String(s.remoteFamily), "addr", JSON.stringify(s.address()));
  s.resume();
  s.on("data", (d) => { console.log("srv-got", d.toString()); s.write("hi-uds"); });
  s.on("end", () => s.end());
});
srv.listen(P, () => {
  console.log("srv-addr", JSON.stringify(srv.address()));
  const c = net.connect(P, () => {
    console.log("cli-remote", String(c.remoteAddress), "addr", JSON.stringify(c.address()), "pending", c.pending, "state", c.readyState);
    c.write("hello");
    c.on("data", (d) => { console.log("cli-got", d.toString()); c.end(); });
    c.on("close", () => srv.close(() => console.log("done")));
  });
  c.on("error", (e) => console.log("cli-err", e.code));
});
srv.on("error", (e) => console.log("srv-err", e.code));
"#,
    );
    assert!(out.contains("srv-remote undefined family undefined addr {}"), "out: {out}");
    assert!(out.contains("t10f.sock\""), "out: {out}");
    assert!(out.contains("cli-remote undefined addr {} pending false state open"), "out: {out}");
    assert!(out.contains("srv-got hello"), "out: {out}");
    assert!(out.contains("cli-got hi-uds"), "out: {out}");
    assert!(out.contains("done"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase10f_net_boundsocket_surface() {
    // 10f net 对拍：BoundSocket 校验族 + fd 真值 + adopt 失效 + EADDRINUSE 逐字形。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import net from "node:net";
console.log("typeof", typeof net.BoundSocket, "isPipe-proto", "isPipe" in net.BoundSocket.prototype);
const b = new net.BoundSocket({ host: "127.0.0.1", port: 0 });
console.log("addr", b.address().address, b.address().family, b.address().port > 0, b.isPipe);
console.log("fd", typeof b.fd() === "number" && b.fd() >= 0);
b.close();
try { b.address(); console.log("adopt BAD"); } catch (e) { console.log("adopt", e.code); }
try { new net.BoundSocket(0); } catch (e) { console.log("num", e.code); }
try { new net.BoundSocket({ host: "localhost", port: 0 }); } catch (e) { console.log("localhost", e.code, e.name); }
try { new net.BoundSocket({ host: 1234 }); } catch (e) { console.log("hostnum", e.code); }
try { new net.BoundSocket({ path: 1234 }); } catch (e) { console.log("pathnum", e.code); }
try { new net.BoundSocket({ path: "x.sock", port: 0 }); } catch (e) { console.log("pathtcp", e.code); }
const srv = net.createServer();
srv.listen(0, "127.0.0.1", () => {
  const port = srv.address().port;
  const b2 = new net.BoundSocket({ host: "127.0.0.1", port: 0 });
  const lp = b2.address().port;
  const c = new net.Socket({ handle: b2 });
  c.connect({ host: "127.0.0.1", port }, () => {
    console.log("adopt-conn", c.localPort === lp, c.localAddress);
    c.destroy(); srv.close(() => console.log("done"));
  });
  c.on("error", (e) => console.log("adopt-err", e.code));
});
"#,
    );
    assert!(out.contains("typeof function isPipe-proto true"), "out: {out}");
    assert!(out.contains("addr 127.0.0.1 IPv4 true false"), "out: {out}");
    assert!(out.contains("fd true"), "out: {out}");
    assert!(out.contains("adopt ERR_SOCKET_HANDLE_ADOPTED"), "out: {out}");
    assert!(out.contains("num ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("localhost ERR_INVALID_ARG_VALUE TypeError"), "out: {out}");
    assert!(out.contains("hostnum ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("pathnum ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("pathtcp ERR_INVALID_ARG_VALUE"), "out: {out}");
    assert!(out.contains("adopt-conn true 127.0.0.1"), "out: {out}");
    assert!(out.contains("done"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase10f_net_server_attrs_finish() {
    // 10f net 对拍：sock.server 全等/getConnections/localFamily/bufferSize/finish/allowHalfOpen。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import net from "node:net";
const server = net.createServer((socket) => {
  console.log("server-eq", socket.server === server, "localFam", socket.localFamily);
  console.log("getConn-ret", server.getConnections());
  server.getConnections((e, n) => console.log("conns", n));
  socket.resume();
  socket.on("data", (d) => console.log("bytesRead", socket.bytesRead > 0, "bufSize", socket.bufferSize));
  socket.on("end", () => { console.log("srv-end"); server.close(() => console.log("done")); });
});
server.listen(0, "127.0.0.1", () => {
  const c = net.connect({ port: server.address().port, host: "127.0.0.1", allowHalfOpen: true }, () => {
    console.log("cli localFam", c.localFamily, "bufSize", c.bufferSize);
    c.on("finish", () => console.log("cli-finish"));
    c.write("hi");
    c.end();
    c.on("data", () => {});
    c.on("close", () => console.log("cli-close"));
  });
  c.on("error", (e) => console.log("cli-err", e.code));
});
server.on("error", (e) => console.log("srv-err", e.code));
"#,
    );
    assert!(out.contains("server-eq true localFam IPv4"), "out: {out}");
    assert!(out.contains("getConn-ret 1"), "out: {out}");
    assert!(out.contains("conns 1"), "out: {out}");
    assert!(out.contains("cli localFam IPv4 bufSize 0"), "out: {out}");
    assert!(out.contains("bytesRead true bufSize 0"), "out: {out}");
    assert!(out.contains("cli-finish"), "out: {out}");
    assert!(out.contains("srv-end"), "out: {out}");
    assert!(out.contains("done"), "out: {out}");
    dir.close().unwrap();
}
