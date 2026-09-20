//! tests/node/http.rs — 对齐 src/builtins/node/http.rs（node:http）。

use crate::helpers::*;

#[test]
fn phase9d_http_loopback() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import http, { createServer, request, get, STATUS_CODES, IncomingMessage, ServerResponse } from "node:http";
import assert from "node:assert";
const server = createServer((req, res) => {
  assert.ok(req instanceof IncomingMessage);
  assert.ok(res instanceof ServerResponse);
  if (req.method === "POST") {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      res.writeHead(201, { "x-reply": "ok" });
      res.end("echo:" + body);
    });
    return;
  }
  if (req.url === "/404") { res.writeHead(404); res.end("nope"); return; }
  if (req.url === "/500") { res.writeHead(500, "Boom"); res.end("bad"); return; }
  if (req.url === "/head-hz") { res.setHeader("x-sync", "1"); res.end("hz"); return; }
  res.end("hello");
});
server.listen(0, "127.0.0.1", () => {
  const port = server.address().port;
  console.log("listening", typeof port === "number" && port > 0);
  // GET（options 形态 + 自定义头）
  http.get({ port, path: "/a?b=1", headers: { "X-Custom": "yes" } }, (res) => {
    console.log("get", res.statusCode, res.headers["x-sync"], res.httpVersion,
      typeof res.headers["content-length"]);
    let body = "";
    res.on("data", (c) => (body += c));
    res.on("end", () => {
      console.log("get-body", body);
      // POST（回声：体经 data/end 回传）
      const req = http.request({ port, path: "/echo", method: "POST" }, (res2) => {
        let b = "";
        res2.on("data", (c) => (b += c));
        res2.on("end", () => {
          console.log("post", res2.statusCode, res2.headers["x-reply"], b);
          // URL 字符串形态 + 404
          get(`http://127.0.0.1:${port}/404`, (r3) => {
            let b3 = "";
            r3.on("data", (c) => (b3 += c));
            r3.on("end", () => {
              console.log("404", r3.statusCode, r3.statusMessage, b3);
              // 500 + 自定义 statusMessage + writeHead 头
              const rq = request({ port, path: "/500", method: "PUT" }, (r4) => {
                let b4 = "";
                r4.on("data", (c) => (b4 += c));
                r4.on("end", () => {
                  console.log("500", r4.statusCode, r4.statusMessage, b4);
                  // setHeader 路径 + finish 事件
                  const rq2 = request({ port, path: "/head-hz" }, (r5) => {
                    console.log("finish-res", r5.statusCode);
                    r5.on("data", () => {});
                    r5.on("end", () => server.close());
                  });
                  rq2.setHeader("x-a", "b");
                  rq2.end();
                  rq2.on("close", () => console.log("rq2-close"));
                });
              }).end("payload");
            });
          });
        });
      });
      req.write("hi");
      req.end("!");
    });
  });
});
server.on("close", () => console.log("server-closed", STATUS_CODES[201], STATUS_CODES[418]));
setTimeout(() => console.log("end-ok"), 300);
"#,
    );
    let out = out;
    assert!(out.contains("listening true"), "out: {out}");
    assert!(out.contains("get 200 undefined 1.1 string"), "out: {out}");
    assert!(out.contains("get-body hello"), "out: {out}");
    assert!(out.contains("post 201 ok echo:hi!"), "out: {out}");
    assert!(out.contains("404 404 Not Found nope"), "out: {out}");
    assert!(out.contains("500 500 Boom bad"), "out: {out}");
    assert!(out.contains("finish-res 200"), "out: {out}");
    assert!(out.contains("rq2-close"), "out: {out}");
    assert!(out.contains("server-closed Created I'm a Teapot"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9d_http_client_errors() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import http from "node:http";
// 连接拒绝（回环空闲端口）→ 'error' 事件 ECONNREFUSED
const s = http.request({ port: 1, path: "/", host: "127.0.0.1" }, () => {});
s.on("error", (e) => {
  console.log("conn-err", e.code);
  // https 协议拒绝（ERR_INVALID_PROTOCOL）
  // 10f G3：错误码化后 message 为 node 原文（'Protocol "https:" not
  // supported. Expected "http:"'），断言改走 e.code（§4.36）。
  try { http.get("https://127.0.0.1/x"); } catch (e2) { console.log("proto-err", e2.code, e2.name); }
  // write after end
  const req = http.request({ port: 1, host: "127.0.0.1" }, () => {});
  req.on("error", () => {}); // 无监听的 error 事件即抛错（Node 口径），此处静默
  req.end();
  // 10f G3：write-after-end 不抛（node Writable 口径）——错误走 cb（无 cb 则
  // 异步 'error'，此处已有静默监听）。
  req.write("x", (e3) => console.log("wae", e3.code === "ERR_STREAM_WRITE_AFTER_END"));
  setTimeout(() => console.log("end-ok"), 30);
});
"#,
    );
    let out = out;
    assert!(out.contains("conn-err ECONNREFUSED"), "out: {out}");
    assert!(out.contains("proto-err ERR_INVALID_PROTOCOL TypeError"), "out: {out}");
    assert!(out.contains("wae true"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase10b_http_keepalive_reuse() {
    // 10b：keep-alive 复用——3 请求 1 连接，reusedSocket 可观测。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "k.mjs",
        r#"
import http, { createServer, Agent } from "node:http";
let conns = 0, reqs = 0;
const server = createServer((req, res) => {
  reqs++;
  req.on("data", () => {});
  req.on("end", () => res.end(`r${reqs}`));
});
server.on("connection", () => { conns++; });
server.listen(0, "127.0.0.1", async () => {
  const port = server.address().port;
  const agent = new Agent({ keepAlive: true });
  const getOne = () => new Promise((resolve, reject) => {
    const r = http.get({ port, path: "/", agent }, (res) => {
      let b = "";
      res.on("data", (c) => (b += c));
      res.on("end", () => resolve({ body: b, reused: r.reusedSocket, conn: res.headers.connection }));
    });
    r.on("error", reject);
  });
  const a = await getOne();
  const b = await getOne();
  const c = await getOne();
  console.log("bodies", a.body, b.body, c.body);
  console.log("reused", a.reused, b.reused, c.reused);
  console.log("counts", conns, reqs);
  console.log("conn-hdr", a.conn);
  agent.destroy();
  server.close();
});
server.on("close", () => console.log("srv-close"));
setTimeout(() => console.log("end-ok"), 1000);
"#,
    );
    for line in [
        "bodies r1 r2 r3",
        "reused false true true",
        "counts 1 3",
        "conn-hdr keep-alive",
        "srv-close",
        "end-ok",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10b_http_chunked_stream() {
    // 10b：分块编码对拍 + for-await/pipe 流消费 + 上传流式。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "c.mjs",
        r#"
import http, { createServer } from "node:http";
import { Writable } from "node:stream";
const server = createServer((req, res) => {
  if (req.url === "/up") {
    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => {
      res.writeHead(200, { "content-type": "text/plain" });
      res.write("got:");
      res.end(Buffer.concat(chunks).toString());
    });
    return;
  }
  if (req.url === "/stream") {
    res.writeHead(200, { "content-type": "text/plain" });
    res.write("a");
    setTimeout(() => { res.write("b"); }, 20);
    setTimeout(() => { res.end("c"); }, 40);
    return;
  }
  res.end("?");
});
server.listen(0, "127.0.0.1", async () => {
  const port = server.address().port;
  const t1 = await new Promise((resolve, reject) => {
    http.get({ port, path: "/stream" }, async (res) => {
      try {
        console.log("te", res.headers["transfer-encoding"], res.headers["content-length"]);
        let s = "";
        for await (const c of res) s += c;
        resolve("stream-body " + s);
      } catch (e) { reject(e); }
    }).on("error", reject);
  });
  console.log(t1);
  const t2 = await new Promise((resolve, reject) => {
    const r = http.request({ port, path: "/up", method: "POST" }, (res) => {
      let b = "";
      res.on("data", (c) => (b += c));
      res.on("end", () => resolve("up-body " + b));
    });
    r.on("error", reject);
    r.write("x");
    setTimeout(() => { r.write("y"); setTimeout(() => r.end("z"), 10); }, 10);
  });
  console.log(t2);
  const t3 = await new Promise((resolve, reject) => {
    const r = http.request({ port, path: "/up", method: "POST" }, (res) => {
      const acc = [];
      const sink = new Writable({ write(c, e, cb) { acc.push(c); cb(); } });
      res.pipe(sink);
      sink.on("finish", () => resolve("pipe-body " + Buffer.concat(acc).toString()));
    });
    r.on("error", reject);
    r.end("piped!");
  });
  console.log(t3);
  server.close();
});
server.on("close", () => console.log("srv-close"));
setTimeout(() => console.log("end-ok"), 1500);
"#,
    );
    for line in [
        "te chunked undefined",
        "stream-body abc",
        "up-body got:xyz",
        "pipe-body got:piped!",
        "srv-close",
        "end-ok",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10b_http_destroy_midflight() {
    // 10b：中途 destroy——客户端杀连接，服务端见 close，客户端无 end 有 close。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "d.mjs",
        r#"
import http, { createServer } from "node:http";
const server = createServer((req, res) => {
  req.on("data", () => {});
  req.on("end", () => {
    res.write("part1");
  });
});
server.on("connection", (sock) => {
  sock.on("close", () => console.log("srv-conn-close"));
});
server.listen(0, "127.0.0.1", () => {
  const port = server.address().port;
  const r = http.get({ port, path: "/" }, (res) => {
    res.on("data", () => {
      console.log("cli-first-data");
      r.destroy();
    });
    res.on("end", () => console.log("cli-end-NEVER"));
    res.on("close", () => {
      console.log("cli-res-close");
      server.close();
    });
  });
  r.on("error", (e) => console.log("cli-req-error", e.code));
  r.on("close", () => console.log("cli-req-close"));
});
server.on("close", () => console.log("srv-close"));
setTimeout(() => console.log("end-ok"), 1500);
"#,
    );
    for line in [
        "cli-first-data",
        "cli-res-close",
        "cli-req-close",
        "srv-conn-close",
        "srv-close",
        "end-ok",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    assert!(!out.contains("cli-end-NEVER"), "out: {out}");
    assert!(!out.contains("cli-req-error"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase10f_http_pipeline_upload_interrupt_and_dechunk() {
    // 10f：上传中断（pipeline(req,res) + 客户端 11 块 chunked 上传 + 读 10 块后 destroy）。
    // 真机 11 次 data（Agent noDelay 默认 + 未连通缓冲逐帧刷出保分包）；合包即 hang（blk09）。
    // 另断言连通后逐写 11 块 → 服务端 11 次 data（分包回归）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import http, { createServer } from "node:http";
import { Readable, pipeline } from "node:stream";
// 分包回归：连通后逐写 11 块 → 11 次 data。
const s2 = createServer((req, res) => {
  let n = 0;
  req.on("data", () => { n++; });
  req.on("end", () => { console.log("dechunk", n); res.end("x"); s2.close(); });
});
s2.listen(0, "127.0.0.1", () => {
  const port = s2.address().port;
  const req = http.request({ port, path: "/", method: "POST" }, (res) => {
    res.resume();
    res.on("end", () => {
      // 中断回归：blk09 原文（11 块上传 + 读 10 块后 destroy 源）。
      const server = createServer((q, r) => {
        pipeline(q, r, (err) => console.log("srv-pipe", err?.code));
      });
      server.listen(0, "127.0.0.1", () => {
        const p2 = server.address().port;
        // 真机口径（10f G3）：上传用 POST——GET 属 useChunkedEncodingByDefault=false
        // 族，真机 body 裸写（无 TE/CL），chunked 分包回归只在 POST 形成立。
        const req2 = http.request({ port: p2, method: "POST" });
        let sent = 0;
        const rs = new Readable({ read() { if (sent++ > 10) return; rs.push("hello"); } });
        pipeline(rs, req2, () => { console.log("cli-pipe-done"); server.close(); });
        req2.on("response", (resp) => {
          let cnt = 10;
          resp.on("data", () => { if (--cnt === 0) rs.destroy(); });
          resp.resume();
        });
      });
    });
  });
  setTimeout(() => {
    for (let i = 0; i < 11; i++) req.write("hello");
    req.end();
  }, 300);
});
setTimeout(() => console.log("end-ok"), 3000);
"#,
    );
    for line in ["dechunk 11", "srv-pipe ERR_STREAM_PREMATURE_CLOSE", "cli-pipe-done", "end-ok"] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10b_http_big_body() {
    // 10b：大体压测（≥1MB 上下行；GC 压力回归 §4.40）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "b.mjs",
        r#"
import http, { createServer } from "node:http";
const big = "0123456789abcdef".repeat(65536);
const sum = (s) => { let h = 0; for (let i = 0; i < s.length; i++) h = (h + s.charCodeAt(i)) | 0; return h; };
const server = createServer((req, res) => {
  if (req.url === "/up") {
    const chunks = [];
    let n = 0;
    req.on("data", (c) => { chunks.push(c); n += c.length; });
    req.on("end", () => {
      const s = Buffer.concat(chunks).toString();
      res.end(`up:${n}:${sum(s)}`);
    });
    return;
  }
  res.end(big);
});
server.listen(0, "127.0.0.1", async () => {
  const port = server.address().port;
  const down = await new Promise((resolve, reject) => {
    http.get({ port, path: "/down" }, (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => resolve(Buffer.concat(chunks).toString()));
    }).on("error", reject);
  });
  console.log("down", down.length, sum(down) === sum(big));
  const up = await new Promise((resolve, reject) => {
    const r = http.request({ port, path: "/up", method: "POST" }, (res) => {
      let b = "";
      res.on("data", (c) => (b += c));
      res.on("end", () => resolve(b));
    });
    r.on("error", reject);
    for (let i = 0; i < 16; i++) r.write(big.slice(i * 65536, (i + 1) * 65536));
    r.end();
  });
  console.log("up", up);
  server.close();
});
server.on("close", () => console.log("srv-close"));
setTimeout(() => console.log("end-ok"), 3000);
"#,
    );
    assert!(out.contains("down 1048576 true"), "out: {out}");
    assert!(out.lines().any(|l| l.starts_with("up up:1048576:")), "out: {out}");
    assert!(out.contains("srv-close"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}

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
fn phase10g_http_chunk_ext_and_trailer_limits() {
    // 欠账 G3：chunk 扩展限深 + trailer 计数（llhttp 计数语义，真机 26.8.2
    // 逐项实测定标）。正常（16384 恰好过/换 chunk 清零）+ 报错（413/431/400
    // 精确字节）+ 边界（分包累计 16385 拒、16384 过）三件套。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { createServer } from "node:http";
import net from "node:net";

const OK200 = "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\nconnection: close\r\ndate: now\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nbye\r\n0\r\n\r\n";
const R413 = "HTTP/1.1 413 Payload Too Large\r\nConnection: close\r\n\r\n";
const R431 = "HTTP/1.1 431 Request Header Fields Too Large\r\nConnection: close\r\n\r\n";
const R400 = "HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n";

function once(build, expect, label) {
  return new Promise((resolve) => {
    const server = createServer((req, res) => {
      req.on("end", () => {
        res.writeHead(200, { "content-type": "text/plain", connection: "close", date: "now" });
        res.end("bye");
      });
      req.resume();
    });
    server.listen(0, "127.0.0.1", () => {
      const port = server.address().port;
      const sock = net.connect(port);
      let data = "";
      sock.on("data", (c) => (data += c.toString()));
      sock.on("end", () => {
        console.log(label, data === expect ? "ok" : "mismatch");
        server.close();
        resolve();
      });
      build(sock, port);
    });
  });
}
const head = (p) => `GET / HTTP/1.1\r\nHost: localhost:${p}\r\nTransfer-Encoding: chunked\r\n\r\n`;

// 1) 扩展总量 17000 > 16KiB → 413 精确字节 + 连接关闭。
await once((s, p) => s.end(head(p) + `2;${"a".repeat(17000)}\r\nAA\r\n0\r\n\r\n`), R413, "b1-413");
// 2) 扩展恰好 16384 → 过（200，writeHead 后 end 走 chunked 响应口径）。
await once((s, p) => s.end(head(p) + `2;${"a".repeat(16384)}\r\nAA\r\n0\r\n\r\n`), OK200, "b2-16k-ok");
// 3) 分包累计（8500+8500=17000）→ 413（计数跨包有效）。
await once((s, p) => {
  s.write(head(p) + "2;");
  s.write("A".repeat(8500));
  setTimeout(() => s.write("A".repeat(8500) + "\r\nAA\r\n0\r\n\r\n"), 10);
}, R413, "b3-split-413");
// 4) 换 chunk 清零：3×10KB 扩展三分块全过 → 200 精确字节。
await once((s, p) => s.end(head(p) +
  `2;${"A".repeat(10000)}=bar\r\nAA\r\n` +
  `2;${"A".repeat(10000)}=bar\r\nAA\r\n` +
  `2;${"A".repeat(10000)}=bar\r\nAA\r\n` +
  "0\r\n\r\n"), OK200, "b4-reset-200");
// 5) 扩展字符集：裸 LF（smuggling 形 `2;\n`）→ 400。
await once((s, p) => s.end(head(p) + "2;\nxx\r\nAA\r\n0\r\n\r\n"), R400, "b5-ext-lf-400");
// 6) trailer 名+值累计 16384 → 431 精确字节（': '/CRLF 不计入）。
await once((s, p) => s.end(head(p) + `2;a\r\nAA\r\n0\r\nX: ${"a".repeat(16383)}\r\n\r\n`), R431, "b6-trailer-431");
// 7) trailer 名+值 16383 → 过 → 200。
await once((s, p) => s.end(head(p) + `2;a\r\nAA\r\n0\r\nX: ${"a".repeat(16382)}\r\n\r\n`), OK200, "b7-trailer-ok");
// 8) trailer 无冒号行 → 400。
await once((s, p) => s.end(head(p) + "2;a\r\nAA\r\n0\r\njustname\r\n\r\n"), R400, "b8-trailer-colon-400");
console.log("limits-done");
"#,
    );
    for tag in [
        "b1-413 ok",
        "b2-16k-ok ok",
        "b3-split-413 ok",
        "b4-reset-200 ok",
        "b5-ext-lf-400 ok",
        "b6-trailer-431 ok",
        "b7-trailer-ok ok",
        "b8-trailer-colon-400 ok",
        "limits-done",
    ] {
        assert!(out.contains(tag), "missing `{tag}`; out:\n{out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10g_http_ipc_socket_path() {
    // 欠账 G3：ClientRequest 的 IPC 形（options.socketPath → UDS；node
    // lib/_http_client.js 口径：req.socketPath 自有属性、池键
    // 'localhost:::<path>' 槽）。正常（回环 200）+ 报错（ENOENT）+ 边界
    // （keepAlive 复用 + agent 键位）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import http, { Agent, createServer } from "node:http";
import net from "node:net";
import assert from "node:assert";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";

const sockPath = path.join(os.tmpdir(), `wjs-g3-uds-${process.pid}.sock`);

// 1) 池键 socketPath 槽（node 26.8.2 实测 'localhost:::/path'）。
const a0 = new Agent();
assert.strictEqual(a0.getName({ socketPath: "/tmp/pipe1" }), "localhost:::/tmp/pipe1");
assert.strictEqual(a0.getName({ host: "h", port: 8, family: 4 }), "h:8::4");
assert.strictEqual(a0.getName({}), "localhost::");

// 2) 回环：UDS server + socketPath 客户端（正常件）。
const seen = [];
const server = createServer((req, res) => {
  seen.push(req.url);
  // 不发 connection: close——保活复用件需要连接回池（真机默认 keep-alive）。
  res.writeHead(200, { "content-type": "text/plain" });
  res.end("ipc-ok");
});
server.listen(sockPath, () => {
  const agent = new Agent({ keepAlive: true });
  http.get({ agent, socketPath: sockPath, path: "/first" }, (res) => {
    let b = "";
    res.on("data", (c) => (b += c));
    res.on("end", () => {
      // 3) keepAlive 复用：第二发同 socketPath 命中池（reusedSocket 观测）。
      // node 口径（真机 26.8.2 实测）：res 'end' 处理器内 socket 尚未回池
      //（nextTick 才入池，user-end 时 freeSockets 空）——end 内直发
      // reused=false，nextTick 后发才 true；故第二发挂 nextTick
      //（官方 agent-keepalive 套件同款时序）。
      process.nextTick(() => {
        const req2 = http.get({ agent, socketPath: sockPath, path: "/second" }, (res2) => {
          res2.resume();
          res2.on("end", () => {
            console.log("loop", b, seen.join(","), req2.reusedSocket);
            agent.destroy();
            server.close();
          });
        });
      });
    });
  });
});

// 4) 报错件：不存在的 UDS 路径 → 'error' ENOENT（无监听即抛，先挂监听）。
const reqBad = http.get({ socketPath: "/tmp/wjs-g3-nope.sock", path: "/" });
reqBad.on("error", (e) => { console.log("bad-path", e.code); });
reqBad.on("close", () => console.log("bad-close", reqBad.destroyed));

// 5) req 面自有属性（node 26.8.2 实测 keys 含 socketPath）。
const probe = http.get({ socketPath: "/tmp/pipe2", createConnection: () => new net.Socket() });
console.log("own", probe.socketPath === "/tmp/pipe2", probe.host, probe.port === undefined);
probe.on("error", () => {});
probe.destroy();

setTimeout(() => console.log("ipc-done"), 400);
"#,
    );
    for tag in [
        "loop ipc-ok /first,/second true",
        "bad-path ENOENT",
        "bad-close true",
        "own true localhost true",
        "ipc-done",
    ] {
        assert!(out.contains(tag), "missing `{tag}`; out:\n{out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10g_http_timeout_agent_surface() {
    // 欠账 G3：Agent({timeout})/req timeout 双级 + createSocket cb 错误 +
    // defaultPort 逐级（真机 26.8.2 逐项实测定标）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import http, { Agent, ClientRequest, createServer } from "node:http";
import net from "node:net";
import assert from "node:assert";

// 1) Agent({timeout})：'socket' 事件时 socket.timeout 已置位；监听形状
//    [onTimeout, emitRequestTimeout]（noop lookup → socket 永不连通）。
const r1 = http.get({ agent: new Agent({ timeout: 50 }), lookup: () => {} });
r1.on("socket", (s) => {
  console.log("agent-tmo", s.timeout, s.listeners("timeout").length,
    s.listeners("timeout")[1] === r1.timeoutCb);
});
r1.on("error", () => {});

// 2) 请求级 timeout 覆盖 agent 级（socket.timeout === 100）。
const r2 = http.get({ agent: new Agent({ timeout: 50 }), lookup: () => {}, timeout: 100 });
r2.on("socket", (s) => {
  console.log("req-tmo-wins", s.timeout, s.listeners("timeout")[1] === r2.timeoutCb);
});
r2.on("error", () => {});

// 3) timeout 校验双检（node validateNumber 口径，真机逐项）。
try { http.request({ timeout: null }); } catch (e) {
  console.log("tmo-null", e.code, e.message.startsWith('The "timeout" argument must be of type number'));
}
try { http.request({ timeout: NaN }); } catch (e) {
  console.log("tmo-nan", e.code);
}

// 4) req 'timeout' 事件：socket 空闲 1ms 单发（server 在场、请求挂起）。
const server = createServer(() => {});
server.listen(0, "127.0.0.1", () => {
  const req = http.request({ host: "127.0.0.1", port: server.address().port, timeout: 1 });
  req.on("error", () => {});
  let n = 0;
  req.on("timeout", () => { n++; });
  setTimeout(() => {
    console.log("tmo-event", n === 1);
    req.destroy();
    server.close();
  }, 100);
});

// 5) createSocket 覆写 cb(err) → req 'error'(原对象) + 'close'(destroyed)。
const agent = new Agent();
const boom = new Error("kaboom");
agent.createSocket = (req, options, cb) => { cb(boom); };
const r5 = http.request({ agent });
r5.on("error", (e) => console.log("cs-err", e === boom));
r5.on("close", () => console.log("cs-close", r5.destroyed));

// 6) defaultPort 逐级：globalAgent.defaultPort 改写生效 + host 头省端口。
const server2 = createServer((req2, res2) => {
  console.log("dp-host", req2.headers.host);
  res2.end("ok");
});
server2.listen(0, "127.0.0.1", () => {
  http.globalAgent.defaultPort = server2.address().port;
  http.get({ host: "127.0.0.1" }, (res) => {
    res.resume();
    res.on("end", () => { http.globalAgent.defaultPort = 80; server2.close(); });
  });
});

// 7) 裸 socket 塞 freeSockets + addRequest → 自动按请求选项补连。
const agent3 = new Agent({ keepAlive: true });
const bare = new net.Socket();
const server3 = createServer((req3, res3) => res3.end("bare-ok"));
server3.listen(0, "127.0.0.1", () => {
  // node 口径：addRequest({}) 的池键缺省 host 是 localhost——URL 须同形才能命中
  // 手塞的 freeSockets 槽（真机 addRequest({},) 对 127.0.0.1 请求同样 miss → 建连）。
  const req7 = new ClientRequest(`http://localhost:${server3.address().port}/`);
  agent3.freeSockets[agent3.getName(req7)] = [bare];
  agent3.addRequest(req7, {});
  req7.on("response", (res) => {
    let b = "";
    res.on("data", (c) => (b += c));
    res.on("end", () => { console.log("bare-reuse", b); server3.close(); });
  });
  req7.on("error", () => {});
  req7.end();
});

setTimeout(() => console.log("tmo-done"), 700);
"#,
    );
    for tag in [
        "agent-tmo 50 2 true",
        "req-tmo-wins 100 true",
        "tmo-null ERR_INVALID_ARG_TYPE true",
        "tmo-nan ERR_OUT_OF_RANGE",
        "tmo-event true",
        "cs-err true",
        "cs-close true",
        "dp-host 127.0.0.1",
        "bare-reuse bare-ok",
        "tmo-done",
    ] {
        assert!(out.contains(tag), "missing `{tag}`; out:\n{out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10g_http_validation_gates() {
    // 欠账 G3：校验长尾（真机 26.8.2 逐项对拍——错误码/名/消息原文）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import http, { Server, ClientRequest, Agent, createServer, ServerResponse } from "node:http";
import net from "node:net";
import assert from "node:assert";

// 1) method 门：非串 ARG_TYPE / '\0' token / 空串回落 GET。
try { http.request({ method: 1 }); } catch (e) {
  console.log("m-type", e.code, e.name,
    e.message === 'The "options.method" property must be of type string. Received type number (1)');
}
try { http.request({ method: "\0" }); } catch (e) {
  console.log("m-token", e.code, e.name,
    e.message === 'Method must be a valid HTTP token ["\0"]');
}
{
  const srv = createServer((req, res) => {
    console.log("m-empty", req.method);
    res.end();
    srv.close();
  });
  srv.listen(0, "127.0.0.1", () => {
    http.request({ port: srv.address().port, method: "" }).end();
  });
}

// 2) host/hostname 类型门（消息含 'or one of undefined or null' 原文）。
try { http.request({ hostname: {} }); } catch (e) {
  console.log("host-type", e.code, e.message ===
    'The "options.hostname" property must be of type string or one of undefined or null. Received an instance of Object');
}
try { http.request({ host: null }).on("error", () => {}).end(); console.log("host-null ok"); } catch { console.log("host-null ok"); }

// 3) agent 门（Agent-like Object/undefined/false；null 合法）。
for (const bad of [true, "agent", {}, 1, () => null]) {
  try { http.request({ agent: bad }); } catch (e) {
    console.log("agent-gate", e.code,
      e.message === 'The "options.agent" property must be one of Agent-like Object, undefined, or false. Received ' +
      (typeof bad === "function" ? "function ()" : `type ${typeof bad} (${String(bad)})`));
    break;
  }
}

// 4) path 赋值门（toctou）+ 协议门（对象形）。
{
  const req = new ClientRequest({ host: "127.0.0.1", port: 1, path: "/valid", createConnection: () => {} });
  let threw = false;
  try { req.path = "/evil\r\nX-Injected: true\r\n\r\n"; } catch (e) { threw = e.code === "ERR_UNESCAPED_CHARACTERS" && e.name === "TypeError"; }
  console.log("path-set", threw, req.path === "/valid");
  try { req.path = "/also-valid"; console.log("path-ok", req.path === "/also-valid"); } catch { console.log("path-ok false"); }
  const url = require("node:url");
  try { http.request(url.parse("ftp://x/")); } catch (e) { console.log("proto-obj", e.code, e.name); }
}

// 5) 头名字门（setHeader + 请求头）。
{
  const res = new ServerResponse({});
  try { res.setHeader("testing 123", 123); } catch (e) {
    console.log("hdr-name", e.code, e.name,
      e.message === 'Header name must be a valid HTTP token ["testing 123"]');
  }
  try { http.get({ headers: { "testing 123": 1 } }); } catch (e) { console.log("hdr-req", e.name); }
}

// 6) Server 选项门（'foo'/42/true/[] → ARG_TYPE；undefined/函数/对象合法）。
let srvGate = "";
for (const bad of ["foo", 42, true, []]) {
  try { new Server(bad); } catch (e) { srvGate += (e.code === "ERR_INVALID_ARG_TYPE" ? "y" : "n"); }
}
console.log("srv-gate", srvGate === "yyyy", typeof new Server(() => {}) === "object");

// 7) Agent maxTotalSockets 门（非串/NaN/0/-1 拒，Infinity 过）。
try { new Agent({ maxTotalSockets: "test" }); } catch (e) {
  console.log("mts-type", e.code, e.name === "TypeError");
}
let mtsRange = "";
for (const item of [-1, 0, NaN]) {
  try { new Agent({ maxTotalSockets: item }); } catch (e) { mtsRange += e.code === "ERR_OUT_OF_RANGE" && e.name === "RangeError" ? "y" : "n"; }
}
console.log("mts-range", mtsRange === "yyy", (new Agent({ maxTotalSockets: Infinity })).maxTotalSockets === Infinity);

// 8) 宽松解析旗类型门。
try { http.request({ insecureHTTPParser: 0 }); } catch (e) {
  console.log("ihp-gate", e.code,
    e.message === 'The "options.insecureHTTPParser" property must be of type boolean. Received type number (0)');
}

// 9) 自动 Date 头 + connection 缺省 keep-alive（automatic-headers 套件口径）。
{
  const srv = createServer((req, res) => {
    res.setHeader("X-Date", "foo");
    res.setHeader("X-Connection", "bar");
    res.setHeader("X-Content-Length", "baz");
    res.end();
  });
  srv.listen(0, "127.0.0.1", () => {
    http.get({ port: srv.address().port, path: "/hello" }, (res) => {
      console.log("auto-hdr", res.headers["x-date"] === "foo", res.headers["x-connection"] === "bar",
        res.headers["x-content-length"] === "baz", !!res.headers.date,
        res.headers.connection === "keep-alive", res.headers["content-length"] === "0");
      srv.close();
    });
  });
}

// 10) clientError 事件（严格头值门：\x08 控制 → 无监听落默认 400）。
{
  const srv = createServer((req, res) => { console.log("ihp-strict", "BAD"); res.end(); });
  let cerr = "";
  srv.on("clientError", (err, sock) => { cerr = err.message; sock.end("HTTP/1.1 400 x\r\n\r\n"); });
  srv.listen(0, "127.0.0.1", () => {
    const c = net.createConnection(srv.address().port, "127.0.0.1");
    c.on("connect", () => c.write("GET / HTTP/1.1\r\nHost: x\r\nHello: foo\x08foo\r\n\r\n"));
    c.on("data", () => {});
    c.on("close", () => { console.log("cerr", cerr === "invalid header value"); srv.close(); });
  });
}
setTimeout(() => console.log("gates-done"), 500);
"#,
    );
    for tag in [
        "m-type ERR_INVALID_ARG_TYPE TypeError true",
        "m-token ERR_INVALID_HTTP_TOKEN TypeError true",
        "m-empty GET",
        "host-type ERR_INVALID_ARG_TYPE true",
        "host-null ok",
        "path-set true true",
        "path-ok true",
        "proto-obj ERR_INVALID_PROTOCOL TypeError",
        "hdr-name ERR_INVALID_HTTP_TOKEN TypeError true",
        "hdr-req TypeError",
        "srv-gate true true",
        "mts-type ERR_INVALID_ARG_TYPE true",
        "mts-range true true",
        "ihp-gate ERR_INVALID_ARG_TYPE true",
        "auto-hdr true true true true true true",
        "cerr true",
        "gates-done",
    ] {
        assert!(out.contains(tag), "missing `{tag}`; out:\n{out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10g_http_parser_strict_client() {
    // 欠账 G3：客户端响应严格门（llhttp strict；真机 26.8.2 对拍）——TE+CL 并存
    // HPE_INVALID_TRANSFER_ENCODING、裸 CR HPE_LF_EXPECTED，response 回调不得触发。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import http from "node:http";
import net from "node:net";
function once(reqstr, label) {
  return new Promise((resolve) => {
    const server = net.createServer((socket) => {
      socket.write(reqstr);
    });
    server.listen(0, "127.0.0.1", () => {
      const req = http.get({ port: server.address().port }, () => {
        console.log(label, "response-BAD");
        server.close();
        resolve();
      });
      req.on("error", (err) => {
        console.log(label, err.code, /^Parse Error/.test(err.message));
        server.close();
        resolve();
      });
    });
  });
}
await once("HTTP/1.1 200 OK\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n", "te-cl");
await once("HTTP/1.1 200 OK\r\nFoo: Bar\rContent-Length: 1\r\n\r\n", "bare-cr");
console.log("strict-done");
"#,
    );
    for tag in [
        "te-cl HPE_INVALID_TRANSFER_ENCODING true",
        "bare-cr HPE_LF_EXPECTED true",
        "strict-done",
    ] {
        assert!(out.contains(tag), "missing `{tag}`; out:\n{out}");
    }
    dir.close().unwrap();
}
