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

// 11) OutgoingMessage 独立构造：未覆写 _implicitHeader 即 NOT_IMPLEMENTED
//（真机 proto 套件口径；旧静默缓冲系伪语义，§4.65 翻转）；覆写后缓冲 +
// writableLength 按 outputSize 累计。
{
  const om = new http.OutgoingMessage();
  assert.throws(() => { om.write("asd"); }, { code: "ERR_METHOD_NOT_IMPLEMENTED" });
  om._implicitHeader = function() {};
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

#[test]
fn phase11_http_outgoing_writable_length_faces() {
    // 基建轮 Slice B1：ServerResponse.writableLength 精确字节（渲染头同步计 +
    // 帧化块同步计 + 落盘递减 + _final 兜底清零）。正常（131/139/finish 0/
    // standalone 累计）+ writeHead 先行形三件套。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { createServer, OutgoingMessage } from "node:http";
import { get } from "node:http";

// 1) 裸写形：'' 即 131（渲染头），'asd' 再 +8（chunked 帧），finish 回 0。
await new Promise((resolve) => {
  const server = createServer((req, res) => {
    console.log("wl-init", res.writableLength === 0);
    res.write("");
    const len = res.writableLength;
    console.log("wl-empty", len === 131);
    res.write("asd");
    console.log("wl-asd", res.writableLength === len + 8);
    res.end();
    res.on("finish", () => {
      console.log("wl-finish", res.writableLength === 0);
      server.close(resolve);
    });
  });
  server.listen(0, "127.0.0.1", () => {
    get({ port: server.address().port }, (res) => {
      res.resume().on("end", () => {});
    });
  });
});
// 2) writeHead 先行形（157/165）。
await new Promise((resolve) => {
  const server = createServer((req, res) => {
    res.writeHead(200, { "Content-Type": "text/plain" });
    res.write("");
    const len = res.writableLength;
    console.log("wl-wh-empty", len === 157);
    res.write("asd");
    console.log("wl-wh-asd", res.writableLength === len + 8);
    res.end();
    res.on("finish", () => server.close(resolve));
  });
  server.listen(0, "127.0.0.1", () => {
    get({ port: server.address().port }, (res) => {
      res.resume().on("end", () => {});
    });
  });
});
// 3) 独立构造：无头即裸块累计。
{
  const msg = new OutgoingMessage();
  msg._implicitHeader = function() {};
  console.log("wl-standalone", msg.writableLength === 0);
  msg.write("a");
  msg.write("bc");
  console.log("wl-standalone-acc", msg.writableLength === 3);
}
console.log("wllen-done");
"#,
    );
    for tag in [
        "wl-init true",
        "wl-empty true",
        "wl-asd true",
        "wl-finish true",
        "wl-wh-empty true",
        "wl-wh-asd true",
        "wl-standalone true",
        "wl-standalone-acc true",
        "wllen-done",
    ] {
        assert!(out.contains(tag), "missing `{tag}`; out:\n{out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase11_http_pipelined_outgoing_queue_faces() {
    // 基建轮 Slice B2：eager-parse 管线队列（后继 res.socket null + 写停靠 +
    // 前响 finish 即 assignSocket 轮转 + drain 递延至落盘清零）。正常（null/
    // 回压/drain 零值/双体有序）三件套。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { createServer } from "node:http";
import net from "node:net";

await new Promise((resolve) => {
  let step = 0;
  let done = false;
  const finish = () => { if (!done) { done = true; resolve(); } };
  const server = createServer((req, res) => {
    step++;
    if (step === 1) {
      res.writeHead(200, { "Content-Type": "text/plain" });
      setTimeout(() => res.end("one"), 50);
      return;
    }
    console.log("q-socknull", res.socket === null);
    res.writeHead(200, { "Content-Type": "text/plain" });
    const chunk = Buffer.alloc(16 * 1024, "x");
    while (res.write(chunk));
    console.log("q-needDrain", res.writableNeedDrain === true);
    res.on("drain", () => {
      console.log("q-drain-len", res.writableLength === 0);
      res.end();
      server.close(finish);
    });
  });
  server.listen(0, "127.0.0.1", () => {
    const port = server.address().port;
    const client = net.connect(port);
    let buf = "";
    client.on("data", (c) => (buf += c.toString()));
    client.on("close", () => {
      console.log("q-bodies", buf.includes("one") && buf.includes("xxxxxxxxxxxxxxxx"));
      finish();
    });
    client.on("error", () => {});
    client.write(
      `GET /1 HTTP/1.1\r\nHost: localhost:${port}\r\n\r\n` +
      `GET /2 HTTP/1.1\r\nHost: localhost:${port}\r\n\r\n`,
    );
    client.resume();
    setTimeout(() => { try { client.destroy(); } catch {} finish(); }, 8000);
  });
});
console.log("queuedone");
"#,
    );
    for tag in [
        "q-socknull true",
        "q-needDrain true",
        "q-drain-len true",
        "q-bodies true",
        "queuedone",
    ] {
        assert!(out.contains(tag), "missing `{tag}`; out:\n{out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase11_http_header_join_faces() {
    // 头合并面：joinDuplicateHeaders 缺省首个赢/true 即合并（单例表亦压过）、
    // cookie 恒 '; '（解析/双端 wire）、缺 Host 1.1 即静默 400。
    // 正常（缺省/合并/cookie）+ 报错（400）+ 边界（set-cookie 恒数组）三件套。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { createServer, get } from "node:http";
import net from "node:net";

// 1) 缺省：重复 authorization 首个赢；cookie '; '；set-cookie 数组。
await new Promise((resolve) => {
  const server = createServer((req, res) => {
    console.log("j-auth", JSON.stringify(req.headers.authorization));
    console.log("j-cookie", JSON.stringify(req.headers.cookie));
    console.log("j-setck", JSON.stringify(req.headers["set-cookie"]));
    res.end();
    server.close(resolve);
  });
  server.listen(0, "127.0.0.1", () => {
    const c = net.connect(server.address().port, () => {
      c.end("GET / HTTP/1.1\r\nHost: x\r\nAuthorization: 1\r\nAuthorization: 2\r\nCookie: a=1\r\nCookie: b=2\r\nSet-Cookie: s1\r\nSet-Cookie: s2\r\n\r\n");
    });
    c.resume();
    c.on("close", () => {});
  });
});
// 2) join:true 即 ', ' 合并（压过单例表）。
await new Promise((resolve) => {
  const server = createServer({ joinDuplicateHeaders: true }, (req, res) => {
    console.log("j2-auth", JSON.stringify(req.headers.authorization));
    res.end();
    server.close(resolve);
  });
  server.listen(0, "127.0.0.1", () => {
    const c = net.connect(server.address().port, () => {
      c.end("GET / HTTP/1.1\r\nHost: x\r\nAuthorization: 1\r\nAuthorization: 2\r\n\r\n");
    });
    c.resume();
    c.on("close", () => {});
  });
});
// 3) 缺 Host 1.1 即静默 400（无 request、无 clientError）。
await new Promise((resolve) => {
  const server = createServer((req, res) => {
    console.log("j3 REQUEST?!");
    res.end();
  });
  server.on("clientError", () => console.log("j3 CLIENTERROR?!"));
  server.listen(0, "127.0.0.1", () => {
    const c = net.connect(server.address().port, () => {
      c.write("GET / HTTP/1.1\r\nConnection: close\r\n\r\n");
    });
    let buf = "";
    c.on("data", (d) => (buf += d.toString()));
    c.on("end", () => {
      console.log("j3-400", buf.startsWith("HTTP/1.1 400 Bad Request"));
      c.end();
    });
    c.on("close", () => server.close(resolve));
  });
});
// 4) 客户端数组 cookie 走单行 '; ' wire。
await new Promise((resolve) => {
  const server = net.createServer((sock) => {
    let buf = "";
    sock.on("data", (d) => {
      buf += d.toString();
      if (buf.includes("\r\n\r\n")) {
        const line = buf.split("\r\n").find((l) => l.toLowerCase().startsWith("cookie:"));
        console.log("j4-wire", JSON.stringify(line));
        sock.end("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
      }
    });
  });
  server.listen(0, "127.0.0.1", () => {
    const req = get({ port: server.address().port, headers: { cookie: ["a=1", "b=2"] } }, (res) => {
      res.resume().on("end", () => server.close(resolve));
    });
    req.on("error", () => {});
    req.end();
  });
});
console.log("joindone");
"#,
    );
    for tag in [
        "j-auth \"1\"",
        "j-cookie \"a=1; b=2\"",
        "j-setck [\"s1\",\"s2\"]",
        "j2-auth \"1, 2\"",
        "j3-400 true",
        "j4-wire \"cookie: a=1; b=2\"",
        "joindone",
    ] {
        assert!(out.contains(tag), "missing `{tag}`; out:\n{out}");
    }
    dir.close().unwrap();
}


// ===== 对拍 mapper（§4.202-①）：失败时一次输出定位三行 =====


/// 请求级 createConnection 错误路由（真机 _http_client.js 591-607 行口径）：
/// async cb 错 / sync throw 统一 nextTick emitErrorEvent——错误永不同步抛出
/// 构造器，无监听经 EE 落 uncaught（修前 err 被吞 → 套件
/// test-http-createConnection TIMEOUT）。
#[test]
fn phase11_http_create_connection_error_routing() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import http from "node:http";
await new Promise((resolve) => {
  const r = http.get({ createConnection: (o, cb) => process.nextTick(cb, new Error("boom-async")) });
  r.on("error", (e) => { console.log("async-err", e.message); resolve(); });
});
let syncThrew = false;
const r2 = http.get({ createConnection: () => { throw new Error("boom-sync"); } });
try { r2.on("error", (e) => { console.log("sync-err", e.message, "syncThrew", syncThrew); }); } catch { syncThrew = true; }
process.on("uncaughtException", (e) => {
  console.log("uncaught", e.message);
  process.exit(0);
});
http.get({ createConnection: () => { throw new Error("boom-uncaught"); } });
setTimeout(() => { console.log("uncaught MISSING"); process.exit(1); }, 500);
"#,
    );
    for tag in [
        "async-err boom-async",
        "sync-err boom-sync syncThrew false",
        "uncaught boom-uncaught",
    ] {
        assert!(out.contains(tag), "missing `{tag}`; out:\n{out}");
    }
    dir.close().unwrap();
}

/// OutgoingMessage captureRejections 递送（真机
/// test-http-outgoing-message-capture-rejection 三件）：
/// events.captureRejections 下 res/req 监听器 rejection 经
/// OutgoingMessage[nodejs.rejection] → destroy(err)——res 侧 socket 'error'
/// 收**同一 err 对象**（修前 ServerResponse.destroy 把 err 丢在
/// super.destroy() 外、_destroy 永裸杀）；client 侧体未齐断连带
/// aborted → error ECONNRESET → close 序（修前 error 永不发）；req 侧
/// 自身 'error' 收同一对象。边界：socket 无用户 error 监听 → 裸杀静默
/// （吞错口径，无 uncaught）。
#[test]
fn phase11_http_outgoing_capture_rejection_routing() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import http from "node:http";
import events from "node:events";
events.captureRejections = true;

// 1) res drain-throw → res.socket 'error' 收同一 err（套件 block1）。
await new Promise((resolve) => {
  const server = http.createServer((req, res) => {
    const _err = new Error("kaboom");
    res.on("drain", async () => { throw _err; });
    res.socket.on("error", (err) => {
      console.log("res-socket-err", err.message, err === _err);
      server.close();
      resolve();
    });
    res.writeHead(200, { Connection: "close" });
    while (res.write("hello"));
  });
  server.listen(0, () => {
    const req = http.request({ method: "GET", host: server.address().host, port: server.address().port });
    req.end();
    req.on("response", (res) => {
      res.on("aborted", () => console.log("client-aborted"));
      res.on("error", (e) => console.log("client-err", e.code));
      res.resume();
    });
  });
});

// 2) req drain-throw → req 'error' 收同一 err（套件 block2）。
await new Promise((resolve) => {
  let _res = null;
  const server = http.createServer((req, res) => { _res = res; });
  server.listen(0, () => {
    const _err = new Error("kaboom2");
    const req = http.request({ method: "POST", host: server.address().host, port: server.address().port });
    req.on("error", (err) => {
      console.log("req-err", err.message, err === _err);
      server.close();
      if (_res) _res.end();
      resolve();
    });
    req.on("drain", async () => { throw _err; });
    while (req.write("hello"));
  });
});

// 3) 边界：socket 无用户 error 监听 → 裸杀静默（无 uncaught、进程自退）。
await new Promise((resolve) => {
  const server = http.createServer((req, res) => {
    const _err = new Error("kaboom3");
    res.on("drain", async () => { throw _err; });
    res.writeHead(200, { Connection: "close" });
    while (res.write("hello"));
    setTimeout(() => { console.log("silent-kill ok"); server.close(); resolve(); }, 100);
  });
  server.listen(0, () => {
    const rq = http.get({ host: server.address().host, port: server.address().port });
    rq.on("response", (r) => r.resume());
  });
});
"#,
    );
    for tag in [
        "res-socket-err kaboom true",
        "client-aborted",
        "client-err ECONNRESET",
        "req-err kaboom2 true",
        "silent-kill ok",
    ] {
        assert!(out.contains(tag), "missing `{tag}`; out:\n{out}");
    }
    dir.close().unwrap();
}

/// shouldKeepAlive 判定矩阵 × 回池门（真机 test-http-should-keep-alive）：
/// 客户端 req.shouldKeepAlive 六形态逐项（1.0 缺省 false / 1.0 'keep-alive'
/// true / 1.0 'close' false / 1.1 缺省 true / 1.1 'keep-alive' true / 1.1
/// 'close' false）；回池门同口径——1.0 缺省响应的 socket **不入池**
/// （修前只看 conn !== 'close'，入池后被复用撞服务端单发语义 → 无响应
/// 挂死）；池态 socket 收 EOF 即销毁摘池。六请求跑完即完成（修前 TIMEOUT）。
#[test]
fn phase11_http_should_keep_alive_matrix() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import http from "node:http";
import net from "node:net";

const RESP = [
  "HTTP/1.0 200 ok\r\nContent-Length: 0\r\n\r\n",
  "HTTP/1.0 200 ok\r\nContent-Length: 0\r\nConnection: keep-alive\r\n\r\n",
  "HTTP/1.0 200 ok\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
  "HTTP/1.1 200 ok\r\nContent-Length: 0\r\n\r\n",
  "HTTP/1.1 200 ok\r\nContent-Length: 0\r\nConnection: keep-alive\r\n\r\n",
  "HTTP/1.1 200 ok\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
];
const EXPECT = [false, true, false, true, true, false];
http.globalAgent.maxSockets = 5;
let i = 0;
const server = net.createServer((sock) => {
  sock.write(RESP[i]);
  if (EXPECT[i]) sock.end();
});
server.listen(0, () => {
  function makeRequest() {
    const req = http.get({ port: server.address().port }, (res) => {
      console.log("ska", i, req.shouldKeepAlive === EXPECT[i]);
      res.resume();
      res.on("end", () => {
        i++;
        if (i < RESP.length) makeRequest();
        else server.close(() => { clearTimeout(watchdog); console.log("matrix done"); });
      });
    });
    req.on("error", (e) => console.log("req-err", i, e.code));
  }
  makeRequest();
});
// 看门只在矩阵未完成时触发（完成即 clearTimeout——node 同形脚本 5s 看门
// 也会先炸，引擎退出卫生由真套件 rc=0 覆盖）。
const watchdog = setTimeout(() => { console.log("stuck at", i); process.exit(9); }, 5000);
watchdog.unref();
"#,
    );
    for k in 0..6 {
        assert!(out.contains(&format!("ska {k} true")), "missing `ska {k} true`; out:\n{out}");
    }
    assert!(out.contains("matrix done"), "suite did not complete; out:\n{out}");
    assert!(!out.contains("req-err"), "unexpected req error; out:\n{out}");
    dir.close().unwrap();
}

/// 服务端体背压流控（真机 test-http-no-read-no-dump）：
/// ① 不消费的 POST 体灌满 req 缓冲（>HWM）→ 服务端 socket 发 'pause'
/// （node parserOnBody→readStop 口径）→ handler 借 pause 收尾 res + 客户端
/// 才续发体（onPause 形）；响应完未消费体进丢弃泵（st.req null 纯跳过、
/// framing 存活到体完），连接回空闲续**排队**的 GET（agent maxSockets=1
/// 在途排队——修前无流控即整链挂死）。GET 发法必须套件排队形（POST 在途
/// 时即 .end()）；finish 后再发不回（post 'finish' 事件不走，见 plan3 记档）。
/// ② pause 转换沿：同一次暂停期不重复发。③ 慢消费：req._read 拉取即解
/// 暂停续读，迟到 30ms 的消费仍收齐全部体。
#[test]
fn phase11_http_server_body_backpressure_flow() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import http from "node:http";

setTimeout(() => { console.log("WATCHDOG"); process.exit(9); }, 8000).unref();

// ①② 套件镜像（onPause 形 + 排队 GET）。
await new Promise((resolve) => {
  let pauses = 0;
  let onPause = () => {};
  const server = http.createServer((req, res) => {
    if (req.method === "GET") {
      console.log("srv get");
      return res.end();
    }
    req.connection.on("pause", () => {
      pauses++;
      console.log("srv pause", pauses);
      res.end();
      onPause();
    });
    res.writeHead(200);
    res.flushHeaders();
  });
  server.listen(0, () => {
    const agent = new http.Agent({ maxSockets: 1, keepAlive: true });
    const port = server.address().port;
    const post = http.request({ agent, method: "POST", port }, (res) => {
      res.resume();
      post.write(Buffer.alloc(64 * 1024).fill("X"));
      onPause = () => post.end("tail");
    });
    post.on("error", (e) => console.log("post-err", e.code));
    post.write("initial");
    http.request({ agent, method: "GET", port }, (res) => {
      console.log("get ok");
      server.close(() => resolve());
    }).end();
  });
});

// ③ 慢消费：30ms 后才消费（体 > HWM 中途 pause → resume 收齐）。
await new Promise((resolve) => {
  const server = http.createServer((req, res) => {
    let got = 0;
    setTimeout(() => {
      req.on("data", (d) => { got += d.length; });
      req.on("end", () => {
        console.log("slow end", got === 200 * 1024);
        res.end();
        server.close(() => resolve());
      });
      req.resume();
    }, 30);
  });
  server.listen(0, () => {
    const post = http.request({ method: "POST", port: server.address().port }, (res) => res.resume());
    post.on("error", (e) => console.log("post2-err", e.code));
    post.end(Buffer.alloc(200 * 1024).fill("Z"));
  });
});
"#,
    );
    for tag in [
        "srv pause 1",
        "srv get",
        "get ok",
        "slow end true",
    ] {
        assert!(out.contains(tag), "missing `{tag}`; out:\n{out}");
    }
    assert!(!out.contains("srv pause 2"), "unexpected second pause; out:\n{out}");
    assert!(!out.contains("WATCHDOG"), "watchdog fired (hang); out:\n{out}");
    assert!(!out.contains("post-err"), "unexpected post error; out:\n{out}");
    dir.close().unwrap();
}

/// drain 不死锁（真机 test-http-outgoing-flush-drain，node#64680）：socket
/// HWM(2MB) > OM HWM(64KB) 时写 500KB——write 返 false 后 drain 必达（服务
/// 端慢消费 500ms）。服务端体背压为**状态驱动事件**（缓冲 ≥HWM 发 'pause'、
/// 落回发 'resume'，泵不停读不中断）——停读+续喂耦合在"体一次性到齐"形
/// 死锁（残段扣 fr.buf 等再喂而包不会再有，§4.206 坑二同源实录）。
#[test]
fn phase11_http_outgoing_flush_drain_no_deadlock() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import http from "node:http";
setTimeout(() => { console.log("WATCHDOG"); process.exit(9); }, 8000).unref();
const server = http.createServer((req, res) => {
  setTimeout(() => {
    req.resume();
    req.on("end", () => res.end("ok"));
  }, 300);
});
server.listen(0, () => {
  const agent = new http.Agent({ keepAlive: true });
  const reqB = http.request({ host: "localhost", port: server.address().port, method: "POST", agent }, (res) => {
    res.resume();
    res.on("end", () => { console.log("b-end ok"); server.close(() => process.exit(0)); });
  });
  const result = reqB.write(Buffer.alloc(500 * 1024));
  console.log("write-false", result === false);
  reqB.on("drain", () => { console.log("drain fired"); reqB.end(); });
});
"#,
    );
    for tag in ["write-false true", "drain fired", "b-end ok"] {
        assert!(out.contains(tag), "missing `{tag}`; out:\n{out}");
    }
    assert!(!out.contains("WATCHDOG"), "deadlock; out:\n{out}");
    dir.close().unwrap();
}
