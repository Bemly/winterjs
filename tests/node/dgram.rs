//! tests/node/dgram.rs — 对齐 src/builtins/node/dgram.rs（node:dgram）。

use crate::helpers::*;

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
