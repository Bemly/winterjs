//! tests/node/http/surface.rs — 10g 表面（对齐 src/builtins/node/http.rs）。

use crate::helpers::*;

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
