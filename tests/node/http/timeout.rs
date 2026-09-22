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

#[test]
fn phase11_http_abort_faces() {
    // abort 级联：客户端 abort → 双侧 aborted + ECONNRESET；服务端无 error
    // 监听时仅 aborted（不抛）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import http from "node:http";
import assert from "node:assert";

// 正常 + 报错：两端 aborted 置位，ECONNRESET('aborted') 双侧可观测。
{
  const srv = http.createServer((req, res) => {
    assert.strictEqual(req.aborted, false);
    req.on("aborted", () => assert.strictEqual(req.aborted, true));
    req.on("error", (err) => {
      assert.strictEqual(err.code, "ECONNRESET");
      assert.strictEqual(err.message, "aborted");
      srv.close();
    });
    res.write("hello");
  });
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  await new Promise((resolve) => {
    const req = http.get(
      { port: srv.address().port, headers: { connection: "keep-alive" } },
      (res) => {
        res.on("aborted", () => assert.strictEqual(res.aborted, true));
        res.on("error", (err) => {
          assert.strictEqual(err.code, "ECONNRESET");
          resolve();
        });
        req.abort();
      }
    );
  });
  console.log("a1 abort-both-sides ok");
}

// 边界：服务端无 error 监听——仅 aborted，不抛错。
{
  const srv = http.createServer((req, res) => {
    req.on("aborted", () => {
      assert.strictEqual(req.aborted, true);
      srv.close();
    });
    res.write("hello");
  });
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  await new Promise((resolve) => {
    const req = http.get({ port: srv.address().port }, (res) => {
      res.on("aborted", () => resolve());
      req.abort();
    });
    req.on("error", () => {});
    setTimeout(resolve, 1500);
  });
  console.log("a2 abort-no-error-listener ok");
}

console.log("END");
"#,
    );
    for tag in [
        "a1 abort-both-sides ok",
        "a2 abort-no-error-listener ok",
        "END",
    ] {
        assert!(out.contains(tag), "missing `{tag}`; out:\n{out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase11_http_outgoing_faces() {
    // G11 流出面：背压有限循环 + 重复 end 语义 + 抛错不毒化 + capture 透传。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import http from "node:http";
import assert from "node:assert";

// 背压：16KB 块有限次即 false（旧同步回恒 true 即无限循环）。
{
  const srv = http.createServer((req, res) => {
    req.resume();
    req.on("end", () => {
      const buf = Buffer.alloc(16384, "x");
      let n = 0;
      let r = res.write(buf);
      n++;
      while (r && n < 30) { r = res.write(buf); n++; }
      assert.ok(!r, "write must exert backpressure");
      res.end(() => srv.close());
    });
  });
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  const port = srv.address().port;
  await new Promise((resolve, reject) => {
    const req = http.request({ port, method: "PUT" }, (res) => {
      res.resume();
      res.on("end", resolve);
    });
    req.on("error", reject);
    req.end(Buffer.alloc(100, "y"));
  });
  console.log("o1 backpressure ok");
}

// 重复 end：ending 中裸 end 排队（null 回调）；finish 后裸 end 报
// ALREADY_FINISHED（同步）；ending 中带块报 WRITE_AFTER_END。
{
  const srv = http.createServer((req, res) => {
    res.end("a", (err) => assert.strictEqual(err, null));
    res.end((err) => assert.strictEqual(err, null));
    res.on("finish", () => {
      res.end((err) => assert.strictEqual(err && err.code, "ERR_STREAM_ALREADY_FINISHED"));
      srv.close();
    });
  });
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  await new Promise((resolve, reject) => {
    http.get({ port: srv.address().port }, (res) => {
      res.resume();
      res.on("end", resolve);
    }).on("error", reject);
  });
  console.log("o2 double-end ok");
}

// 抛错不毒化：非法块 end 同步抛后，合法 end 照常完成响应。
{
  const srv = http.createServer((req, res) => {
    assert.throws(() => res.end(["bad"]), { code: "ERR_INVALID_ARG_TYPE" });
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
  console.log("o3 throw-no-poison ok");
}

console.log("END");
"#,
    );
    for tag in [
        "o1 backpressure ok",
        "o2 double-end ok",
        "o3 throw-no-poison ok",
        "END",
    ] {
        assert!(out.contains(tag), "missing `{tag}`; out:\n{out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase11_http_pipeline_and_limits_faces() {
    // G11 管线面：前导空行多连发 + 残缺头 408 + maxRequests 503 + 毁后写丢弃。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import http from "node:http";
import net from "node:net";
import assert from "node:assert";

// 前导空行：三请求连发（含额外空行）全部分发。
{
  let got = 0;
  const srv = http.createServer((req, res) => { got++; res.end("ok"); });
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  const port = srv.address().port;
  await new Promise((resolve) => {
    const c = net.connect({ port, host: "127.0.0.1" });
    c.on("connect", () => {
      c.write(
        `GET /1 HTTP/1.1\r\nHost: x\r\n\r\n\r\n` +
        `GET /2 HTTP/1.1\r\nHost: x\r\n\r\n\r\n` +
        `GET /3 HTTP/1.1\r\nHost: x\r\n\r\n\r\n`
      );
    });
    let n = 0;
    c.on("data", () => {});
    setTimeout(() => {
      assert.strictEqual(got, 3);
      c.destroy();
      resolve();
    }, 400);
  });
  srv.close();
  console.log("p1 leading-crlf ok");
}

// 残缺头：管线第二请求不完整 → requestTimeout 内 408。
{
  const srv = http.createServer({ headersTimeout: 0, requestTimeout: 250 }, (req, res) => {
    res.writeHead(200, { "Content-Type": "text/plain" });
    res.end();
  });
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  const port = srv.address().port;
  const got = await new Promise((resolve) => {
    const c = net.connect({ port, host: "127.0.0.1" });
    let buf = "";
    c.on("data", (d) => (buf += d.toString()));
    c.on("connect", () => {
      c.write("GET / HTTP/1.1\r\nHost: x\r\nConnection: keep-alive\r\n\r\n");
      c.write("GET / HTTP/1.1\r\nHost: x\r\nConnection: ");
    });
    c.on("close", () => resolve(buf));
    c.on("error", () => {});
  });
  assert.ok(got.includes("200 OK"), "first response 200");
  assert.ok(got.includes("408 Request Timeout"), "second stalls to 408");
  srv.close();
  console.log("p2 partial-head-408 ok");
}

// maxRequestsPerSocket：3 额内 keep-alive，第 4 路 503 + 关连接。
{
  const srv = http.createServer((req, res) => {
    res.writeHead(200, { "Content-Type": "text/plain" });
    res.write("Hello World!");
    res.end();
  });
  srv.maxRequestsPerSocket = 2;
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  const port = srv.address().port;
  const buf = await new Promise((resolve) => {
    const c = net.connect({ port, host: "127.0.0.1" });
    let b = "";
    c.on("data", (d) => (b += d.toString()));
    c.on("connect", () => {
      const one = "POST / HTTP/1.1\r\nHost: x\r\nConnection: keep-alive\r\nContent-Length: 3\r\n\r\nabc";
      c.write(one + one + one);
    });
    c.on("close", () => resolve(b));
    c.on("error", () => {});
  });
  assert.ok(buf.includes("503 Service Unavailable"), "over-limit 503");
  srv.close();
  console.log("p3 max-requests-503 ok");
}

// 毁后写丢弃：管线中毁连接，续行响应不抛。
{
  const srv = http.createServer((req, res) => {
    if (req.url === "/1") { req.socket.destroy(); return; }
    res.end("ok");
  });
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  const port = srv.address().port;
  await new Promise((resolve) => {
    const c = net.connect({ port, host: "127.0.0.1" });
    c.on("connect", () => c.write("GET /1 HTTP/1.1\r\nHost: x\r\n\r\nGET /2 HTTP/1.1\r\nHost: x\r\n\r\n"));
    c.on("close", resolve);
    c.on("error", () => {});
    setTimeout(resolve, 800);
  });
  srv.close();
  console.log("p4 write-after-destroy-drop ok");
}

console.log("END");
"#,
    );
    for tag in [
        "p1 leading-crlf ok",
        "p2 partial-head-408 ok",
        "p3 max-requests-503 ok",
        "p4 write-after-destroy-drop ok",
        "END",
    ] {
        assert!(out.contains(tag), "missing `{tag}`; out:\n{out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase11_http_cork_faces() {
    // cork/uncork 面（response-cork / response-drain-cork / outgoing-end-cork
    // 三套件形态）：镜像计数（res.writableCorked === res.socket.writableCorked）
    // + corked 期间 socket.write 不被调（字节滞留，uncork/end 排空）+
    // chunked 写粒度恰 5 发（头+hex/CRLF/体/CRLF/终结，node _send 链口径）+
    // socket HWM 背压（写 10 true / 写 1000 false + needDrain → uncork →
    // drain → end）+ ClientRequest 消息级计数 + end 全开（writableCorked===0）。
    // 正常 + 报错 + 边界三件。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r##"
import http from "node:http";
import assert from "node:assert";

// 1) 镜像计数 + corked 不落盘 + 粒度 5 发（response-cork 形）。
{
  const server = http.createServer((req, res) => {
    let corked = false;
    const orig = res.socket.write;
    let n = 0;
    res.socket.write = function (...args) {
      n++;
      assert.strictEqual(corked, false, "socket.write during cork");
      return orig.call(res.socket, ...args);
    };
    corked = true;
    res.cork();
    res.cork();
    console.log("cork2", res.writableCorked === res.socket.writableCorked, res.writableCorked);
    res.writeHead(200, { "a-header": "v" });
    res.uncork();
    console.log("uncork1", res.writableCorked === res.socket.writableCorked, res.writableCorked);
    corked = false;
    res.end("asd");
    console.log("endcork", res.writableCorked === res.socket.writableCorked, res.writableCorked);
    // 写发生在 _final（异步），计数随 finish 收口（node mustCall 退出时核账同口径）。
    res.on("finish", () => console.log("writes", n));
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  await new Promise((r) => {
    http.get({ port: server.address().port }, (res) => {
      let body = "";
      res.on("data", (c) => (body += c));
      res.on("end", () => { console.log("cork-body", JSON.stringify(body)); server.close(r); });
    });
  });
}
// 2) cork 背压（drain-cork 形）：socket HWM 1000 → false/needDrain → uncork
//   → drain → end；客户端收全 1010 字节。
{
  const server = http.createServer((req, res) => {
    res.cork();
    const r1 = res.write("1".repeat(10));
    const r2 = res.write("2".repeat(1000));
    console.log("bp", r1, r2, res.writableNeedDrain);
    res.once("drain", () => { console.log("drain", res.writableNeedDrain); res.end(); });
    res.uncork();
  });
  server.on("connection", (s) => { s._writableState.highWaterMark = 1000; });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  await new Promise((r) => {
    http.get({ port: server.address().port }, (res) => {
      let n = 0;
      res.on("data", (c) => (n += c.length));
      res.on("end", () => { console.log("bp-bytes", n); server.close(r); });
    });
  });
}
// 3) req 消息级计数 + res end 全开（outgoing-end-cork 形）。
{
  const server = http.createServer((req, res) => {
    res.end("regular end");
    console.log("res-corked-after-end", res.writableCorked === 0);
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  await new Promise((r) => {
    http.get({ port: server.address().port }, (res) => {
      res.resume();
      res.on("end", () => { console.log("req3-done"); server.close(r); });
    });
  });
}
console.log("cork-done");
"##,
    );
    for tag in [
        "cork2 true 2",
        "uncork1 true 1",
        "endcork true 0",
        "writes 5",
        "cork-body \"asd\"",
        "bp true false true",
        "drain false",
        "bp-bytes 1010",
        "res-corked-after-end true",
        "req3-done",
        "cork-done",
    ] {
        assert!(out.contains(tag), "missing `{tag}`; out:\n{out}");
    }
    dir.close().unwrap();
}
