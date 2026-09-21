//! tests/node/http/loopback.rs — 回环/客户端错（对齐 src/builtins/node/http.rs）。

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

