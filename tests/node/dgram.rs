//! tests/node/dgram.rs — 对齐 src/builtins/node/dgram.rs（node:dgram）。

use crate::common::*;
use crate::helpers::*;
use assert_fs::prelude::*;

#[test]
fn phase9d_dgram_loopback() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import dgram, { createSocket } from "node:dgram";
import assert from "node:assert";
// createSocket 类型校验
try { createSocket("udp7"); } catch (e) { console.log("bad-type", e.code); }
const server = createSocket("udp4", (msg, rinfo) => {
  console.log("srv-msg", String(msg), rinfo.address, rinfo.port > 0, rinfo.family,
    rinfo.size === msg.length);
  server.send(Buffer.from("pong"), rinfo.port, rinfo.address);
});
server.on("listening", () => {
  const addr = server.address();
  console.log("srv-addr", addr.port > 0, addr.address, addr.family);
  const client = createSocket({ type: "udp4" });
  client.on("message", (msg) => {
    console.log("cli-msg", String(msg));
    client.close();
  });
  client.on("close", () => server.close());
  client.bind(0, "127.0.0.1", () => {
    console.log("cli-bound", client.address().port > 0);
    // send：string + Buffer 两种形态
    client.send("ping", addr.port, "127.0.0.1");
  });
});
server.bind(0, "127.0.0.1");
server.on("close", () => console.log("srv-close"));
setTimeout(() => console.log("end-ok"), 200);
"#,
    );
    let out = out;
    assert!(out.contains("bad-type ERR_SOCKET_BAD_TYPE"), "out: {out}");
    assert!(out.contains("srv-addr true 127.0.0.1 IPv4"), "out: {out}");
    assert!(out.contains("cli-bound true"), "out: {out}");
    assert!(out.contains("srv-msg ping 127.0.0.1 true 4 true"), "out: {out}");
    assert!(out.contains("cli-msg pong"), "out: {out}");
    assert!(out.contains("srv-close"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}
// ── Phase 9d-5：node:zlib ────

#[test]
fn phase10a_dgram_multicast_connect() {
    // 10a：组播/connect/ref 全家——同步校验 + 回环/组播投递 + unref 释放。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "m.mjs",
        r#"
import dgram, { createSocket } from "node:dgram";
// ── 同步校验（真机逐项对过码与文案子集）──
try { createSocket("udp4").addMembership(); } catch (e) { console.log("mem-miss", e.code); }
try { createSocket("udp4").addMembership("999.1.1.1"); } catch (e) { console.log("mem-bad", e.code, e.message); }
try { createSocket("udp4").addMembership("192.168.1.1"); } catch (e) { console.log("mem-uni", e.code); }
try { createSocket("udp4").dropMembership(); } catch (e) { console.log("drop-miss", e.code); }
const t0 = createSocket("udp4");
for (const [tag, fn] of [
  ["ttl-0", () => t0.setTTL(0)],
  ["ttl-256", () => t0.setTTL(256)],
  ["mttl-256", () => t0.setMulticastTTL(256)],
  ["conn-none", () => t0.connect()],
  ["remote-before", () => t0.remoteAddress()],
  ["send-noaddr", () => t0.send("hi")],
]) {
  try { fn(); console.log(tag, "NO-THROW"); }
  catch (e) { console.log(tag, e.code); }
}
try { t0.setTTL("x"); } catch (e) { console.log("ttl-str", e.code); }
// 未绑 sockopt 即同步 EBADF（真机口径；旧静默挂起系偏差）——回值断言改走已绑 socket。
t0.bind(0, "127.0.0.1", () => {
  console.log("ttl-ret", t0.setTTL(64), t0.setMulticastTTL(5), t0.setMulticastLoopback(false), t0.setBroadcast(true));
  t0.close();
});
// ── connect 回环（默认远端发送）──
const server = createSocket("udp4");
server.on("error", (e) => console.log("srv-error", e.code));
server.on("listening", () => {
  const port = server.address().port;
  const client = createSocket("udp4");
  client.on("error", (e) => console.log("cli-error", e.code));
  client.on("connect", () => {
    console.log("conn-remote", JSON.stringify(client.remoteAddress()));
    client.send("hi-connected");
  });
  client.on("message", (msg) => {
    console.log("conn-back", String(msg));
    try { client.disconnect(); console.log("disc-ok"); } catch (e) { console.log("disc-err", e.code); }
    try { client.remoteAddress(); } catch (e) { console.log("remote-after", e.code); }
    try { client.disconnect(); } catch (e) { console.log("disc-twice", e.code); }
    client.close(() => server.close());
  });
  client.connect(port, "127.0.0.1");
});
server.on("message", (msg, rinfo) => {
  console.log("srv-got", String(msg), rinfo.port > 0);
  server.send("hello-back", rinfo.port, rinfo.address);
});
server.bind(0, "127.0.0.1");
// ── 组播投递（hermetic 配方：双端绑 0.0.0.0 + 默认接口加组；
// 127.0.0.1 端在此沙箱收不到组播——真机同配方同样收不到，已对拍）──
const mcast = createSocket("udp4");
mcast.on("error", (e) => console.log("mcast-error", e.code));
mcast.on("listening", () => {
  const port = mcast.address().port;
  mcast.addMembership("239.0.0.1");
  const sender = createSocket("udp4");
  sender.on("error", (e) => console.log("sender-error", e.code));
  sender.bind(0, "0.0.0.0", () => {
    sender.send("mcast-hi", port, "239.0.0.1");
    setTimeout(() => sender.close(), 500);
  });
});
mcast.on("message", (msg, rinfo) => {
  console.log("mcast-got", String(msg));
  mcast.dropMembership("239.0.0.1");
  mcast.close();
});
mcast.bind(0, "0.0.0.0");
setTimeout(() => console.log("end-ok"), 1500);
"#,
    );
    for line in [
        "mem-miss ERR_MISSING_ARGS",
        "mem-bad EINVAL addMembership EINVAL",
        "mem-uni EINVAL",
        "drop-miss ERR_MISSING_ARGS",
        "ttl-0 EINVAL",
        "ttl-256 EINVAL",
        "mttl-256 EINVAL",
        "conn-none ERR_SOCKET_BAD_PORT",
        "remote-before ERR_SOCKET_DGRAM_NOT_CONNECTED",
        "send-noaddr ERR_SOCKET_BAD_PORT",
        "ttl-str ERR_INVALID_ARG_TYPE",
        "ttl-ret 64 5 false undefined",
        "conn-remote {\"address\":\"127.0.0.1\",\"port\":",
        "srv-got hi-connected true",
        "conn-back hello-back",
        "disc-ok",
        "remote-after ERR_SOCKET_DGRAM_NOT_CONNECTED",
        "disc-twice ERR_SOCKET_DGRAM_NOT_CONNECTED",
        "mcast-got mcast-hi",
        "end-ok",
    ] {
        assert!(out.lines().any(|l| l == line || l.starts_with(line)), "missing line: {line}\nout: {out}");
    }
    assert!(!out.contains("srv-error"), "out: {out}");
    assert!(!out.contains("cli-error"), "out: {out}");
    assert!(!out.contains("mcast-error"), "out: {out}");
    assert!(!out.contains("sender-error"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase10a_dgram_unref_releases_loop() {
    // 10a：unref 真计数——唯一句柄 unref 后循环即退。
    // 脚本内零 timer（pending timer 同样续命，会掩盖结论）；挂了由外部
    // 8s 超时判失败，不 hang 住全量。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("u.mjs");
    file.write_str(
        r#"import { createSocket } from "node:dgram";
const s = createSocket("udp4");
s.bind(0, "127.0.0.1", () => {
  console.log("unref-ret", s.unref() === s, s.ref() === s);
  s.unref();
});
"#,
    )
    .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(file.path())
        .current_dir(dir.path())
        .timeout(std::time::Duration::from_secs(8))
        .output()
        .expect("unref test hung: socket still holds loop (8s timeout)");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("unref-ret true true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase10f_dgram_send_buffer_surface() {
    // 10f dgram 欠账轮：recvbuf 四方法（未绑 ERR_SOCKET_BUFFER_SIZE 逐字、绑后
    // set/get 回读、构造选项）+ send 未绑隐式绑定（cb (null, bytes)、address 可查）
    // + 数组 send + EMSGSIZE 路由回回调 + ALREADY_BOUND + address() 未绑 EBADF +
    // udp4 bind('localhost') 族匹配 127.0.0.1。标签互不为子串（§4.42）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import dgram from "node:dgram";
import assert from "node:assert";
// 1. 未绑 buffer size 抛（文案逐字）
{
  const s = dgram.createSocket("udp4");
  try { s.getRecvBufferSize(); } catch (e) {
    console.log("bufsz-unbound-get", e.code, e.message === "Could not get or set buffer size: uv_recv_buffer_size returned EBADF (bad file descriptor)");
  }
  try { s.setSendBufferSize(4096); } catch (e) {
    console.log("bufsz-unbound-set", e.code, e.message.includes("uv_send_buffer_size"));
  }
}
// 2. 未绑 send 隐式绑定 + cb (null, bytes) + address 可查
{
  const rx = dgram.createSocket("udp4");
  rx.bind(0, "127.0.0.1", () => {
    const s = dgram.createSocket("udp4");
    const msg = Buffer.from("hello dgram");
    s.send(msg, rx.address().port, "127.0.0.1", (err, bytes) => {
      console.log("implicit-send", err === null, bytes === msg.length, s.address().port > 0);
      // 3. EMSGSIZE 路由回回调（256KB > 上限）
      const big = Buffer.alloc(256 * 1024);
      s.send(big, 0, big.length, 41234, "127.0.0.1", (e2) => {
        console.log("emsgsize-cb", e2 !== null && e2.code === "EMSGSIZE", e2 && e2.address === "127.0.0.1" && e2.port === 41234);
        s.close(); rx.close();
      });
    });
  });
}
// 4. 绑后 set/get 回读 + 构造选项
{
  const s = dgram.createSocket({ type: "udp4", recvBufferSize: 8192 });
  s.bind(0, "127.0.0.1", () => {
    console.log("bufsz-opt", s.getRecvBufferSize() === 8192);
    s.setSendBufferSize(16384);
    console.log("bufsz-set", s.getSendBufferSize() === 16384);
    // 5. 双 bind
    try { s.bind(0); } catch (e) { console.log("already-bound", e.code, e.message === "Socket is already bound"); }
    s.close();
  });
}
// 6. 数组 send
{
  const rx = dgram.createSocket("udp4");
  rx.on("message", (m) => { console.log("array-recv", m.toString() === "ab"); rx.close(); });
  rx.bind(0, "127.0.0.1", () => {
    const s = dgram.createSocket("udp4");
    s.send([Buffer.from("a"), Buffer.from("b")], rx.address().port, "127.0.0.1", (err, bytes) => {
      console.log("array-sent", err === null, bytes === 2);
      s.close();
    });
  });
}
// 7. address() 未绑 EBADF 逐字 + udp4 localhost 族匹配
{
  const s = dgram.createSocket("udp4");
  try { s.address(); } catch (e) { console.log("addr-unbound", e.code === "EBADF", e.message === "getsockname EBADF"); }
  const s2 = dgram.createSocket("udp4");
  s2.bind(0, "localhost", () => {
    console.log("fam-resolve", s2.address().address === "127.0.0.1");
    s2.close();
  });
}
// 8. connect 状态机 + 关闭门 + send 切片形 + TTL 校验 + 未绑 sockopt EBADF
{
  const c = dgram.createSocket("udp4");
  c.connect(12345, "127.0.0.1", () => {
    c.disconnect();
    try { c.disconnect(); } catch (e) { console.log("disconn-twice", e.code === "ERR_SOCKET_DGRAM_NOT_CONNECTED"); }
    try { c.remoteAddress(); } catch (e) { console.log("raddr-disc", e.code === "ERR_SOCKET_DGRAM_NOT_CONNECTED"); }
    c.close();
  });
  try { c.connect(12345); } catch (e) { console.log("conn-twice", e.code === "ERR_SOCKET_DGRAM_IS_CONNECTED"); }
  const m = dgram.createSocket("udp4");
  m.close(() => {
    try { m.addMembership("224.0.0.114"); } catch (e) { console.log("memb-closed", e.code === "ERR_SOCKET_DGRAM_NOT_RUNNING"); }
    try { m.setMulticastInterface("0.0.0.0"); } catch (e) { console.log("mif-closed", e.code === "ERR_SOCKET_DGRAM_NOT_RUNNING"); }
  });
  const t = dgram.createSocket("udp4");
  try { t.setMulticastLoopback(16); } catch (e) { console.log("loop-unbound", e.code === "EBADF"); }
  try { t.setTTL("foo"); } catch (e) { console.log("ttl-type", e.code === "ERR_INVALID_ARG_TYPE"); }
  try { t.setTTL(1000); } catch (e) { console.log("ttl-range", e.code === "EINVAL"); }
  t.close();
  const rx = dgram.createSocket("udp4");
  rx.bind(0, "127.0.0.1", () => {
    const s = dgram.createSocket("udp4");
    const msg = Buffer.from("xyzh");
    s.send(msg, 1, 2, rx.address().port, "127.0.0.1", (err, bytes) => {
      console.log("send-slice", err === null && bytes === 2);
      s.close();
    });
  });
  rx.on("message", (buf) => { console.log("slice-recv", buf.toString() === "yz"); rx.close(); });
}
// 9. bindSync/connectSync 同步面（地址即时有效、事件递延、关即抑制）。
{
  const s = dgram.createSocket("udp4");
  const addr = s.bindSync({ address: "127.0.0.1", port: 0 });
  console.log("bsync", addr.address === "127.0.0.1" && addr.family === "IPv4" && addr.port > 0);
  console.log("bsync-self", s.address().port === addr.port);
  try { s.bindSync({ port: 0 }); } catch (e) { console.log("bsync-twice", e.code === "ERR_SOCKET_ALREADY_BOUND"); }
  try { s.bindSync(0); } catch (e) { console.log("bsync-arg", e.code === "ERR_INVALID_ARG_TYPE"); }
  const c = dgram.createSocket("udp4");
  c.connectSync(addr.port, "127.0.0.1");
  console.log("csync", c.remoteAddress().address === "127.0.0.1" && c.remoteAddress().port === addr.port);
  try { c.connectSync(1); } catch (e) { console.log("csync-twice", e.code === "ERR_SOCKET_DGRAM_IS_CONNECTED"); }
  c.disconnect();
  const c2 = dgram.createSocket("udp4");
  try { c2.connectSync(1, "localhost"); } catch (e) { console.log("csync-dns", e.code === "ERR_INVALID_ARG_VALUE"); }
  c2.close();
  c.close();
  s.close();
}
setTimeout(() => process.exit(0), 3000);
"#,
    );
    for line in [
        "bufsz-unbound-get ERR_SOCKET_BUFFER_SIZE true",
        "bufsz-unbound-set ERR_SOCKET_BUFFER_SIZE true",
        "implicit-send true true true",
        "emsgsize-cb true true",
        "bufsz-opt true",
        "bufsz-set true",
        "already-bound ERR_SOCKET_ALREADY_BOUND true",
        "array-sent true true",
        "array-recv true",
        "addr-unbound true true",
        "fam-resolve true",
        "conn-twice true",
        "disconn-twice true",
        "raddr-disc true",
        "memb-closed true",
        "mif-closed true",
        "loop-unbound true",
        "ttl-type true",
        "ttl-range true",
        "send-slice true",
        "slice-recv true",
        "bsync true",
        "bsync-self true",
        "bsync-twice true",
        "bsync-arg true",
        "csync true",
        "csync-twice true",
        "csync-dns true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
    dir.close().unwrap();
}
