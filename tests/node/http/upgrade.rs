//! tests/node/http/upgrade.rs — 升级流回归（G11 upgrade 轮）。
//!
//! 覆盖：connection token 门（单头不升级）+ 无监听回落 request +
//! shouldUpgradeCallback 真/假/抛三态 + 升级后体路由（chunked 体进 req、
//! 体完转 socket）+ 迟挂 data 不丢 + destroy(err) 异步 uncaught。
//! 正常 + 报错 + 边界三件；防挂守卫回归只红不挂。

use crate::helpers::*;

#[test]
fn phase11_http_upgrade_faces() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import http from "node:http";
import net from "node:net";
import assert from "node:assert";
setTimeout(() => { console.log("HANG"); process.exit(1); }, 15000).unref();

// 正常 1：单头不升级（upgrade 裸头 / connection 裸头均走 request）。
{
  const srv = http.createServer((req, res) => { res.end("regular"); });
  srv.on("upgrade", () => console.log("BAD-upgrade"));
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  const port = srv.address().port;
  for (const h of [[{ upgrade: "h2c" }, "u1"], [{ connection: "upgrade" }, "u2"]]) {
    const body = await new Promise((resolve, reject) => {
      http.get({ port, headers: h[0] }, (res) => {
        let b = "";
        res.on("data", (c) => (b += c));
        res.on("end", () => resolve(b));
      }).on("error", reject);
    });
    assert.strictEqual(body, "regular", h[1]);
  }
  srv.close();
  console.log("u1 single-header-regular ok");
}

// 正常 2：无监听回落 request（advertise 末段口径，非销毁）。
{
  const srv = http.createServer((req, res) => { res.end("fallback"); });
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  const body = await new Promise((resolve, reject) => {
    const req = net.connect({ port: srv.address().port, host: "127.0.0.1" });
    req.write("GET / HTTP/1.1\r\nHost: x\r\nConnection: upgrade\r\nUpgrade: ws\r\n\r\n");
    let b = "";
    req.on("data", (c) => (b += c.toString()));
    req.on("end", () => resolve(b));
    req.on("error", () => resolve("BAD-err"));
  });
  assert.ok(body.includes("fallback"), "fallback body");
  srv.close();
  console.log("u2 no-listener-fallback ok");
}

// 正常 3：shouldUpgradeCallback 真/假分流。
{
  const srv = http.createServer({
    shouldUpgradeCallback: (req) => req.url !== "/plain",
  });
  srv.on("upgrade", (req, sock, head) => {
    assert.ok(Buffer.isBuffer(head));
    sock.end("HTTP/1.1 101 U\r\nConnection: upgrade\r\nUpgrade: ws\r\n\r\n");
  });
  srv.on("request", (req, res) => res.end("plain"));
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  const port = srv.address().port;
  const doUpgrade = (path) => new Promise((resolve, reject) => {
    const req = http.request({ port, path, headers: { connection: "upgrade", upgrade: "ws" } });
    req.on("upgrade", (res, sock) => { sock.destroy(); resolve("up"); });
    req.on("response", (res) => { res.resume(); res.on("end", () => resolve("req")); });
    req.on("error", reject);
    req.end();
  });
  assert.strictEqual(await doUpgrade("/ws"), "up");
  assert.strictEqual(await doUpgrade("/plain"), "req");
  srv.close();
  console.log("u3 callback-true-false ok");
}

// 报错：shouldUpgradeCallback 抛错走 uncaught（连接先死）。
{
  const srv = http.createServer({ shouldUpgradeCallback: () => { throw new Error("cb boom"); } });
  srv.on("upgrade", () => console.log("BAD-upgrade2"));
  srv.on("request", () => console.log("BAD-request2"));
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  await new Promise((resolve) => {
    process.once("uncaughtException", (e) => {
      assert.strictEqual(e.message, "cb boom");
      resolve();
    });
    const req = http.request({
      port: srv.address().port, path: "/",
      headers: { connection: "upgrade", upgrade: "ws" },
    });
    req.on("error", () => {});
    req.end();
  });
  srv.close();
  console.log("u4 callback-throw-uncaught ok");
}

// 正常 4：升级后体路由（chunked 体进 req，体完转 socket；迟挂不丢）。
{
  const srv = http.createServer();
  let seenBody = "";
  let seenSock = "";
  srv.on("upgrade", (req, sock, head) => {
    assert.ok(Buffer.isBuffer(head));
    assert.strictEqual(head.length, 0);
    socketWrite101(sock);
    req.on("data", (c) => (seenBody += c.toString()));
    req.on("end", () => {
      assert.strictEqual(seenBody, "hello");
      setTimeout(() => {
        sock.on("data", (d) => {
          seenSock += d.toString();
          if (seenSock.length >= 10) {
            assert.strictEqual(seenSock, "afterbody!");
            sock.end();
            srv.close();
          }
        });
      }, 50);
    });
  });
  function socketWrite101(sock) {
    sock.write("HTTP/1.1 101 U\r\nConnection: upgrade\r\nUpgrade: ws\r\n\r\n");
  }
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  const port = srv.address().port;
  await new Promise((resolve, reject) => {
    const c = net.connect({ port, host: "127.0.0.1" });
    c.on("connect", () => {
      c.write("POST / HTTP/1.1\r\nHost: x\r\nConnection: upgrade\r\nUpgrade: ws\r\nTransfer-Encoding: chunked\r\n\r\n");
      setTimeout(() => {
        c.write("5\r\nhello\r\n0\r\n\r\n");
        c.write("afterbody!");
        c.on("data", () => {});
        c.on("end", resolve);
      }, 200);
    });
    c.on("error", reject);
  });
  console.log("u5 body-routing ok");
}

// 报错 2：destroy(err) 异步 uncaught（非同步抛）。
{
  const srv = http.createServer();
  srv.on("upgrade", (req, sock) => {
    req.on("data", () => {
      const r = req.socket.destroy(new Error("sim err"));
      assert.strictEqual(r, req.socket, "destroy returns socket");
      console.log("u6 destroy-returned ok");
    });
  });
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  await new Promise((resolve) => {
    process.once("uncaughtException", (e) => {
      assert.strictEqual(e.message, "sim err");
      resolve();
    });
    const c = net.connect({ port: srv.address().port, host: "127.0.0.1" });
    c.on("connect", () => {
      c.write("POST / HTTP/1.1\r\nHost: x\r\nConnection: upgrade\r\nUpgrade: ws\r\nTransfer-Encoding: chunked\r\n\r\n");
      setTimeout(() => c.write("5\r\nhello\r\n"), 200);
    });
    c.on("error", () => {});
  });
  srv.close();
  console.log("u6 destroy-async-uncaught ok");
}

console.log("END");
"#,
    );
    for tag in [
        "u1 single-header-regular ok",
        "u2 no-listener-fallback ok",
        "u3 callback-true-false ok",
        "u4 callback-throw-uncaught ok",
        "u5 body-routing ok",
        "u6 destroy-returned ok",
        "u6 destroy-async-uncaught ok",
        "END",
    ] {
        assert!(out.lines().any(|l| l == tag), "missing `{tag}`; out:\n{out}");
    }
    assert!(!out.contains("HANG"), "out:\n{out}");
    assert!(!out.contains("BAD"), "out:\n{out}");
    dir.close().unwrap();
}
