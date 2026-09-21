//! tests/node/http/timeout.rs — http TIMEOUT 簇回归（G11 首批）。
//!
//! 覆盖：req.setTimeout 转发武装（client-timeout hang 根因）+ 构造期超时
//! socket 事件可见、覆写值 defer 到 connect（client-set-timeout 时序）+
//! 请求级覆盖 agent 级（timeout-option-with-agent）+ finish 后 setTimeout
//! noop（set-timeout-after-end）+ keepSocketAlive 可覆写与池超时自毁
//! （agent-timeout 块 2/4）。正常 + 报错 + 边界三件。

use crate::helpers::*;

#[test]
fn phase11_http_client_request_timeout_faces() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import http from "node:http";
import net from "node:net";
import assert from "node:assert";

// 正常 1：无响应服务端 + req.setTimeout → timeout → destroy → close。
{
  const srv = http.createServer(() => {});
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  const port = srv.address().port;
  await new Promise((resolve, reject) => {
    const req = http.request({ port, host: "127.0.0.1", path: "/" }, () => {});
    req.on("close", () => {
      assert.strictEqual(req.destroyed, true);
      resolve();
    });
    req.on("error", () => {});
    req.setTimeout(30, () => req.destroy());
    req.end();
  });
  srv.close();
  console.log("t1 req-timeout-close ok");
}

// 正常 2：构造期 2000 + 同步 setTimeout(1000) → socket 事件见 2000，
// connect 后见 1000（defer 口径）。
{
  const srv = http.createServer(() => {});
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  const port = srv.address().port;
  await new Promise((resolve, reject) => {
    const req = http.get({ port, timeout: 2000 });
    req.setTimeout(1000);
    req.on("socket", (sock) => {
      assert.strictEqual(sock.timeout, 2000);
      sock.on("connect", () => {
        assert.strictEqual(sock.timeout, 1000);
        req.destroy();
        resolve();
      });
    });
    req.on("error", () => {});
  });
  srv.close();
  console.log("t2 defer-to-connect ok");
}

// 正常 3：请求级 100 覆盖 agent 级 50（socket 事件即 100；noop lookup
// 永不连通亦可断言，不依赖建连时序）。
{
  const req = http.get({
    agent: new http.Agent({ timeout: 50 }),
    lookup: () => {},
    timeout: 100,
  });
  await new Promise((resolve) => {
    req.on("socket", (sock) => {
      assert.strictEqual(sock.timeout, 100);
      assert.strictEqual(sock.listeners("timeout").length, 2);
      assert.strictEqual(sock.listeners("timeout")[1], req.timeoutCb);
      req.destroy();
      resolve();
    });
    req.on("error", () => {});
  });
  console.log("t3 req-over-agent ok");
}

// 边界：res 'end' 后 setTimeout(0) 即 noop（监听数恒 1，返回自身）。
{
  const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });
  const srv = http.createServer((req, res) => res.end());
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  const port = srv.address().port;
  let sock;
  await new Promise((resolve, reject) => {
    const req = http.get({ agent, port }, (res) => {
      res.on("end", () => {
        assert.strictEqual(req.setTimeout(0), req);
        assert.strictEqual(sock.listenerCount("timeout"), 1);
        resolve();
      });
      res.resume();
    });
    req.on("socket", (s) => (sock = s));
    req.on("error", reject);
  });
  agent.destroy();
  srv.close();
  console.log("t4 setTimeout-after-end noop ok");
}

// 正常 4：CustomAgent keepSocketAlive 覆写（super 后置 60 生效）。
{
  const CUSTOM_TIMEOUT = 60;
  class CustomAgent extends http.Agent {
    keepSocketAlive(sock) {
      if (!super.keepSocketAlive(sock)) return false;
      sock.setTimeout(CUSTOM_TIMEOUT);
      return true;
    }
  }
  const agent = new CustomAgent({ keepAlive: true, timeout: 50 });
  const srv = http.createServer((req, res) => res.end());
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  const port = srv.address().port;
  await new Promise((resolve, reject) => {
    http.get({ port, agent }).on("response", (res) => {
      const sock = res.socket;
      res.resume();
      sock.on("free", () => {
        sock.on("timeout", () => {
          assert.strictEqual(sock.timeout, CUSTOM_TIMEOUT);
          resolve();
        });
      });
    }).on("error", reject);
  });
  agent.destroy();
  srv.close();
  console.log("t5 custom-keepSocketAlive ok");
}

// 正常 5：池 socket 超时即销毁（第二请求换新连接）。
{
  const agent = new http.Agent({ keepAlive: true, timeout: 40 });
  const srv = http.createServer((req, res) => res.end());
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  const port = srv.address().port;
  await new Promise((resolve, reject) => {
    http.get({ port, agent }).on("response", (res) => {
      const sock = res.socket;
      res.resume();
      sock.on("free", () => {
        sock.on("timeout", () => {
          http.get({ port, agent }).on("response", (res2) => {
            assert.notStrictEqual(sock, res2.socket);
            assert.strictEqual(sock.destroyed, true);
            res2.resume();
            res2.on("end", resolve);
          }).on("error", reject);
        });
      });
    }).on("error", reject);
  });
  agent.destroy();
  srv.close();
  console.log("t6 pooled-timeout-destroyed ok");
}

// 报错：非法 timeout 值逐字（构造期与 setTimeout 双侧）。
{
  let ok = false;
  try { http.get({ port: 1, timeout: "x" }); } catch (e) { ok = e.code === "ERR_INVALID_ARG_TYPE"; }
  assert.ok(ok, "expected ARG_TYPE for string timeout");
  console.log("t7 invalid-timeout ok");
}

// 正常 8：客户端 101 升级——摘池（totalSocketCount 归零）+ req close 随后。
{
  const raw = net.createServer((c) => {
    c.on("data", () => {
      c.write("HTTP/1.1 101 Switching Protocols\r\nconnection: upgrade\r\nupgrade: websocket\r\n\r\nbody-bytes");
    });
  });
  await new Promise((r) => raw.listen(0, "127.0.0.1", r));
  const port = raw.address().port;
  await new Promise((resolve, reject) => {
    const req = http.request({ port, host: "127.0.0.1", headers: { connection: "upgrade", upgrade: "websocket" } });
    req.end();
    req.on("upgrade", (res, sock, head) => {
      assert.strictEqual(res.statusCode, 101);
      assert.strictEqual(head.toString(), "body-bytes");
      assert.strictEqual(req.agent.totalSocketCount, 0);
      req.on("close", () => {
        sock.destroy();
        resolve();
      });
    });
    req.on("error", reject);
  });
  raw.close();
  console.log("t8 client-upgrade-detach ok");
}

// 报错 2：非 chunked 带 Trailer 即同步抛 ERR_HTTP_TRAILER_INVALID；
// 边界：Trailer + 自动 chunked（无 CL）合法不抛。
{
  const srv = http.createServer((req, res) => {
    res.setHeader("Trailer", "x-sum");
    let ok = false;
    try { res.writeHead(200, { "Content-Length": "2" }); } catch (e) { ok = e.code === "ERR_HTTP_TRAILER_INVALID"; }
    assert.ok(ok, "expected TRAILER_INVALID");
    res.removeHeader("Trailer");
    res.end("ok");
  });
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  const body = await new Promise((resolve, reject) => {
    http.get({ port: srv.address().port }, (res) => {
      let b = "";
      res.on("data", (c) => (b += c));
      res.on("end", () => resolve(b));
    }).on("error", reject);
  });
  assert.strictEqual(body, "ok");
  srv.close();
  const srv2 = http.createServer((req, res) => {
    res.setHeader("Trailer", "x-sum");
    res.write("hi");
    res.addTrailers({ "x-sum": "42" });
    res.end();
  });
  await new Promise((r) => srv2.listen(0, "127.0.0.1", r));
  const t = await new Promise((resolve, reject) => {
    http.get({ port: srv2.address().port }, (res) => {
      res.resume();
      res.on("end", () => resolve(res.trailers["x-sum"]));
    }).on("error", reject);
  });
  assert.strictEqual(t, "42");
  srv2.close();
  console.log("t9 trailer-gate ok");
}

console.log("END");
"#,
    );
    for tag in [
        "t1 req-timeout-close ok",
        "t2 defer-to-connect ok",
        "t3 req-over-agent ok",
        "t4 setTimeout-after-end noop ok",
        "t5 custom-keepSocketAlive ok",
        "t6 pooled-timeout-destroyed ok",
        "t7 invalid-timeout ok",
        "t8 client-upgrade-detach ok",
        "t9 trailer-gate ok",
        "END",
    ] {
        assert!(out.contains(tag), "missing `{tag}`; out:\n{out}");
    }
    dir.close().unwrap();
}
