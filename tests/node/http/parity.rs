//! tests/node/http/parity.rs — 对拍 round1（对齐 src/builtins/node/http.rs）。

use crate::helpers::*;

#[test]
fn phase10f_http_parity_round1() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import http from "node:http";
import net from "node:net";
import { Duplex } from "node:stream";
import assert from "node:assert";

// 1) Agent 无 new + getName 键形（'host:port:localAddress(:family)'，缺省位留冒号）。
const a1 = http.Agent({ keepAlive: true });
assert.strictEqual(a1 instanceof http.Agent, true);
assert.strictEqual(a1.keepAlive, true);
assert.strictEqual(http.Agent().getName({ host: "h", port: 8 }), "h:8:");
assert.strictEqual(http.Agent().getName({}), "localhost::");
assert.strictEqual(http.Agent().getName({ host: "h", port: 8, family: 4 }), "h:8::4");
assert.strictEqual(http.Agent().getName({ host: "h", port: 8, localAddress: "1.2.3.4" }), "h:8:1.2.3.4");
console.log("p1 no-new-agent ok");

// 2) server 选项持久化 + 默认链（headersTimeout = min(60000, requestTimeout)）。
const s2 = http.createServer({ requestTimeout: 2000 }, () => {});
assert.strictEqual(s2.requestTimeout, 2000);
assert.strictEqual(s2.headersTimeout, 2000);
assert.strictEqual(s2.keepAliveTimeout, 5000);
assert.strictEqual(s2.timeout, 0);
const s2b = http.createServer();
assert.strictEqual(s2b.requestTimeout, 300000);
assert.strictEqual(s2b.headersTimeout, 60000);
console.log("p2 server-options ok");

// 3) server.setTimeout 链式 + 'timeout' 事件（idle 触发，带 socket）。
{
  const s = http.createServer(() => {});
  assert.strictEqual(s.setTimeout(80), s);
  s.listen(0, "127.0.0.1", () => {
    const port = s.address().port;
    const c = net.createConnection(port, "127.0.0.1");
    c.on("connect", () => {});
    c.on("close", () => {});
    setTimeout(() => c.destroy(), 300);
  });
  let fired = 0;
  s.on("timeout", (sock) => { fired++; assert.strictEqual(sock instanceof net.Socket, true); });
  setTimeout(() => {
    assert.strictEqual(fired >= 1, true, "server timeout not fired");
    s.close();
    console.log("p3 server-setTimeout ok", fired);
  }, 350);
}

// 4) 400 Bad Request：首个合法请求进 handler（node blank-header 口径：handler
//    只断言不响应），随后的管线残渣 "hello world" 解析失败即回 400 + 关连接。
{
  let handled = 0;
  const srv = http.createServer((rq, rs) => {
    handled++;
    assert.strictEqual(rq.headers.cookie, undefined);
  });
  srv.listen(0, "127.0.0.1", () => {
    const c = net.createConnection(srv.address().port, "127.0.0.1");
    let got = "";
    c.on("connect", () => c.write("GET /x HTTP/1.1\r\nHost: x\r\n\r\n\r\nhello world"));
    c.on("data", (d) => (got += d.toString()));
    c.on("close", () => {
      assert.strictEqual(handled, 1);
      assert.strictEqual(got, "HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n");
      srv.close();
      console.log("p4 bad-request ok");
    });
  });
}

// 5) 408 Request Timeout：headersTimeout 内头未齐（字面精确）。
{
  const srv = http.createServer({ headersTimeout: 250 }, () => {});
  srv.listen(0, "127.0.0.1", () => {
    const c = net.createConnection(srv.address().port, "127.0.0.1");
    let got = "";
    c.on("data", (d) => (got += d.toString()));
    c.on("connect", () => setTimeout(() => { if (!c.destroyed) c.write("GET / HTTP/1.1\r\n\r\n"); }, 600));
    c.on("close", () => {
      assert.strictEqual(got, "HTTP/1.1 408 Request Timeout\r\nConnection: close\r\n\r\n");
      srv.close();
      console.log("p5 request-timeout ok");
    });
  });
}

// 6) 选项校验：headersTimeout > requestTimeout 即 ERR_OUT_OF_RANGE；
//    非整数/负值同码。边界：等值合法。
{
  let ok = false;
  try { http.createServer({ requestTimeout: 1000, headersTimeout: 2000 }); } catch (e) { ok = e.code === "ERR_OUT_OF_RANGE"; }
  assert.ok(ok, "expected ERR_OUT_OF_RANGE");
  let ok2 = false;
  try { http.createServer({ keepAliveTimeout: -1 }); } catch (e) { ok2 = e.code === "ERR_OUT_OF_RANGE"; }
  assert.ok(ok2, "expected ERR_OUT_OF_RANGE for negative");
  http.createServer({ requestTimeout: 5000, headersTimeout: 5000 }).close();
  console.log("p6 validation ok");
}

// 7) 客户端 res 面：socket 在场 + 状态行无短语解析为空串（raw 服务端）。
{
  const rawSrv = net.createServer((c) => {
    c.on("data", () => {
      c.write("HTTP/1.1 200 No-Reason\r\nContent-Length: 2\r\n\r\nhi");
      c.end();
    });
  });
  rawSrv.listen(0, "127.0.0.1", async () => {
    const res = await new Promise((resolve, reject) => {
      const rq = http.request({ port: rawSrv.address().port, createConnection: () => net.createConnection(rawSrv.address().port, "127.0.0.1") }, resolve);
      rq.on("error", reject);
      rq.end();
    });
    let body = "";
    res.on("data", (c2) => (body += c2));
    await new Promise((r) => res.on("end", r));
    assert.strictEqual(res.socket instanceof net.Socket, true);
    assert.strictEqual(res.statusCode, 200);
    assert.strictEqual(res.statusMessage, "No-Reason");
    assert.strictEqual(body, "hi");
    const rawSrv2 = net.createServer((c) => {
      c.on("data", () => {
        c.write("HTTP/1.1 201\r\nContent-Length: 0\r\n\r\n");
        c.end();
      });
    });
    rawSrv2.listen(0, "127.0.0.1", async () => {
      const res2 = await new Promise((resolve, reject) => {
        const rq = http.request({ port: rawSrv2.address().port }, resolve);
        rq.on("error", reject);
        rq.end();
      });
      res2.resume(); // paused 模式不消费则 'end' 不发（真机同款语义）
      await new Promise((r) => res2.on("end", r));
      assert.strictEqual(res2.statusMessage, "");
      rawSrv2.close();
      console.log("p7 client-res-surface ok");
    });
    rawSrv.close();
  });
}

// 8) agent.createConnection 覆盖（假 Duplex 黑洞 socket 全链）。
{
  class FakeAgent extends http.Agent {
    createConnection() {
      const d = new Duplex();
      let once = false;
      d._read = function () {
        if (once) return this.push(null);
        once = true;
        this.push("HTTP/1.1 200 Ok\r\nTransfer-Encoding: chunked\r\n\r\n");
        this.push("b\r\nhello world\r\n");
        this.push("0\r\n\r\n");
      };
      d._write = function (data, enc, cb) { cb(); };
      d.destroy = d.destroySoon = function () { this.writable = false; };
      return d;
    }
  }
  const req = http.request({ agent: new FakeAgent() }, (res) => {
    let got = "";
    res.on("data", (c) => (got += c));
    res.on("end", () => {
      assert.strictEqual(got, "hello world");
      console.log("p8 fake-agent ok");
    });
  });
  req.on("error", () => {});
  req.end();
}

// 9) closeIdleConnections / closeAllConnections：空闲 raw 连接被清。
{
  const srv = http.createServer(() => {});
  srv.listen(0, "127.0.0.1", () => {
    const c = net.createConnection(srv.address().port, "127.0.0.1");
    c.on("close", () => {
      srv.close();
      console.log("p9 close-idle ok");
    });
    setTimeout(() => srv.closeIdleConnections(), 60);
  });
}

// 10) flushHeaders 双侧：req 头先于 end 到服务端；res 头先于体。
{
  const srv = http.createServer((rq, rs) => {
    assert.strictEqual(rq.headers["x-early"], "1");
    rs.flushHeaders();
    rs.write("part");
    setTimeout(() => rs.end(":end"), 60);
  });
  srv.listen(0, "127.0.0.1", async () => {
    const got = await new Promise((resolve, reject) => {
      const rq = http.request({ port: srv.address().port, headers: { "x-early": "1" } }, (rs) => {
        let b = "";
        rs.on("data", (c) => (b += c));
        rs.on("end", () => resolve(b));
      });
      rq.on("error", reject);
      rq.flushHeaders();
      rq.end();
    });
    assert.strictEqual(got, "part:end");
    srv.close();
    console.log("p10 flush-headers ok");
  });
}

// 11) OutgoingMessage 独立构造：_write 缓冲 + writableLength 保留。
{
  const om = new http.OutgoingMessage();
  om.write("asd");
  assert.strictEqual(om.writableLength, 3);
  const om2 = new http.OutgoingMessage();
  assert.strictEqual(om2.writableObjectMode, false);
  assert.ok(om2.writableHighWaterMark > 0);
  console.log("p11 outgoing ok");
}

// 12) 路径校验：控制字符即 ERR_UNESCAPED_CHARACTERS；普通路径不受影响。
{
  let ok = false;
  try { http.request({ host: "x", path: "/a b\u0001" }); } catch (e) { ok = e.code === "ERR_UNESCAPED_CHARACTERS"; }
  assert.ok(ok, "expected ERR_UNESCAPED_CHARACTERS");
  console.log("p12 path-validation ok");
}

// 13) IncomingMessage.setTimeout 转发 socket + ClientRequest .port 非自有属性。
{
  const im = new http.IncomingMessage();
  assert.strictEqual(typeof im.setTimeout, "function");
  const blackhole = new Duplex();
  blackhole._read = function () {};
  blackhole._write = function (c, e, cb) { cb(); };
  const rq = new http.ClientRequest({ host: "x", port: 1234, createConnection: () => blackhole });
  rq.on("error", () => {});
  rq.setTimeout(30);
  rq.destroy();
  assert.strictEqual(rq.port, undefined);
  assert.strictEqual(rq.getPort(), 1234);
  assert.strictEqual(rq.getHost(), "x");
  console.log("p13 im-req-surface ok");
}

setTimeout(() => console.log("END"), 900);
"#,
    );
    for tag in [
        "p1 no-new-agent ok",
        "p2 server-options ok",
        "p3 server-setTimeout ok",
        "p4 bad-request ok",
        "p5 request-timeout ok",
        "p6 validation ok",
        "p7 client-res-surface ok",
        "p8 fake-agent ok",
        "p9 close-idle ok",
        "p10 flush-headers ok",
        "p11 outgoing ok",
        "p12 path-validation ok",
        "p13 im-req-surface ok",
        "END",
    ] {
        assert!(out.contains(tag), "missing `{tag}`; out:\n{out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase11_http_socket_push_and_server_parse_errors() {
    // 基建轮 Slice A：Socket.push 可读侧注入 + 服务端 llhttp 解析错三件
    // （code/message/bytesParsed/rawPacket）+ TE+CL/重 CL 门 + 客户端数组头
    // 分行。正常（push 读写/null 收尾）+ 报错（overflow/method/TE/CL 四形）+
    // 边界（非法块类型/销毁后 push）三件套。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { createServer, get } from "node:http";
import net from "node:net";

// 1) push 基础：返回值 + 数据 + null 收尾（未连接即 END + CLOSE）。
{
  const s = new net.Socket();
  const seen = [];
  s.on("data", (c) => seen.push(String(c)));
  console.log("push-ret", s.push("hi"), s.push(Buffer.from("!")));
  s.on("end", () => console.log("push-end", seen.join("") === "hi!"));
  s.on("close", () => console.log("push-close"));
  console.log("push-null", s.push(null));
  console.log("push-after", s.push("x"));
}
// 2) 边界：非法块类型 + 销毁后 push。
{
  const s = new net.Socket();
  try { s.push({}); console.log("push-bad NO"); }
  catch (e) { console.log("push-bad", e.code); }
  s.push(null);
}
// 3) 服务端 overflow 三件（validator 实跑，非 vacuous）。
await new Promise((resolve) => {
  const server = createServer(() => {});
  server.on("connection", (sock) => {
    sock.on("error", (e) => {
      console.log("ovf", e.code === "HPE_HEADER_OVERFLOW", e.bytesParsed,
        Buffer.isBuffer(e.rawPacket), e.rawPacket.length);
    });
    sock.push("GET /blah HTTP/1.1\r\nCookie: " + "a".repeat(16384));
  });
  server.listen(0, "127.0.0.1", () => {
    const c = net.connect(server.address().port);
    let got = "";
    c.on("data", (d) => (got += d.toString()));
    c.on("end", () => {
      console.log("ovf-cli", got === "HTTP/1.1 431 Request Header Fields Too Large\r\nConnection: close\r\n\r\n");
      c.end();
    });
    c.on("close", () => server.close(resolve));
  });
});
// 4) 方法错：clientError 三件 + 默认 400 可达。
await new Promise((resolve) => {
  const server = createServer(() => console.log("m9y REQUEST?!"));
  server.on("clientError", (e, sock) => {
    console.log("m9y", e.code, e.bytesParsed, e.rawPacket.length, e.message);
    sock.end("HTTP/1.1 400 Bad Request\r\n\r\n");
    server.close(resolve);
  });
  server.listen(0, "127.0.0.1", () => {
    const c = net.connect(server.address().port, () => c.end("FOO /\r\n"));
    c.on("error", () => {});
  });
});
// 5) TE+CL 并存门。
await new Promise((resolve) => {
  const server = createServer(() => console.log("te REQUEST?!"));
  server.on("clientError", (e, sock) => {
    console.log("te", e.code);
    sock.destroy();
    server.close(resolve);
  });
  server.listen(0, "127.0.0.1", () => {
    const c = net.connect(server.address().port, () => {
      c.end("POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 10\r\nTransfer-Encoding: chunked\r\n\r\n");
    });
    c.on("error", () => {});
  });
});
// 6) 重 CL：客户端数组头走两行 wire，服务端拒收。
await new Promise((resolve) => {
  const server = createServer(() => console.log("dc REQUEST?!"));
  server.on("clientError", (e, sock) => {
    console.log("dc", e.code, e.message);
    sock.destroy();
    server.close(resolve);
  });
  server.listen(0, "127.0.0.1", () => {
    const req = get({ port: server.address().port, headers: { "Content-Length": [1, 2] } }, () => {});
    req.on("error", () => {});
    req.end();
  });
});
console.log("pushparse-done");
"#,
    );
    for tag in [
        "push-ret true true",
        "push-end true",
        "push-close",
        "push-null false",
        "push-after false",
        "push-bad ERR_INVALID_ARG_TYPE",
        "ovf true 16412 true 16412",
        "ovf-cli true",
        "m9y HPE_INVALID_METHOD 1 7 Parse Error: Invalid method encountered",
        "te HPE_INVALID_TRANSFER_ENCODING",
        "dc HPE_UNEXPECTED_CONTENT_LENGTH Parse Error: Duplicate Content-Length",
        "pushparse-done",
    ] {
        assert!(out.contains(tag), "missing `{tag}`; out:\n{out}");
    }
    dir.close().unwrap();
}

