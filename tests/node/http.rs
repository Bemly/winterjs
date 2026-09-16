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
  try { http.get("https://127.0.0.1/x"); } catch (e2) { console.log("proto-err", e2.message.startsWith("ERR_INVALID_PROTOCOL")); }
  // write after end
  const req = http.request({ port: 1, host: "127.0.0.1" }, () => {});
  req.on("error", () => {}); // 无监听的 error 事件即抛错（Node 口径），此处静默
  req.end();
  try { req.write("x"); } catch (e3) { console.log("wae", e3.message.startsWith("ERR_STREAM_WRITE_AFTER_END")); }
  setTimeout(() => console.log("end-ok"), 30);
});
"#,
    );
    let out = out;
    assert!(out.contains("conn-err ECONNREFUSED"), "out: {out}");
    assert!(out.contains("proto-err true"), "out: {out}");
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
        const req2 = http.request({ port: p2 });
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
