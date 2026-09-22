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

#[test]
fn phase11_http_header_face_batch5() {
    // 头面 batch5（真机 26.8.2 逐项对拍）：数字头名 HTTP_TOKEN（"3840" 本身合法
    // token 故须 typeof 先判）+ 奇长 writeHead 数组 ARG_VALUE + 已发头再 write
    // 即 HEADERS_SENT + writeHead 覆写拼写 + 220 短语 unknown + 数组同键双行 +
    // 对形 writeHead + rejectNonStandardBodyWrites + Host 端口恒拼/IPv6 加框。
    // 正常 + 报错 + 边界三件。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r##"
import { createServer, get, request } from "node:http";
import net from "node:net";
import assert from "node:assert";

// 1) 报错三件：数字名/奇数组/重发头。
{
  const s = createServer((req, res) => {
    try { res.setHeader(0xf00, "bar"); console.log("num-name BAD"); }
    catch (e) { console.log("num-name", e.code); }
    try { res.writeHead(200, ["invalid", "headers", "args"]); console.log("odd BAD"); }
    catch (e) { console.log("odd", e.code); }
    res.writeHead(200, { Test: "2" });
    console.log("spell", res.getHeader("test"), JSON.stringify(res.getRawHeaderNames()));
    try { res.writeHead(100, {}); console.log("resend BAD"); }
    catch (e) { console.log("resend", e.code); }
    res.end();
  });
  await new Promise((r) => s.listen(0, "127.0.0.1", r));
  await new Promise((r) => {
    get({ port: s.address().port }, (res) => {
      res.resume().on("end", () => {
        console.log("spell-wire", res.headers.test, res.rawHeaders.includes("Test"));
        s.close(r);
      });
    });
  });
}
// 2) 220 未知码 + 数组双行 + 对形。
{
  const s = createServer((req, res) => {
    if (req.url === "/220") {
      res.writeHead(220, ["test", "1"]);
      console.log("msg220", res.statusMessage);
      try { res.writeHead(200, ["t2", "2"]); console.log("re220 BAD"); }
      catch (e) { console.log("re220", e.code); }
      res.end();
    } else if (req.url === "/dup") {
      res.writeHead(200, ["array-val", "1", "array-val", "2"]);
      res.end();
    } else {
      res.writeHead(200, [["content-type", "text/plain"]]);
      res.end("hi");
    }
  });
  await new Promise((r) => s.listen(0, "127.0.0.1", r));
  const port = s.address().port;
  await new Promise((r) => {
    get({ port, path: "/220" }, (res) => {
      res.resume().on("end", () => {
        console.log("wire220", res.statusCode, res.statusMessage, res.headers.test);
        r();
      });
    });
  });
  await new Promise((r) => {
    get({ port, path: "/dup" }, (res) => {
      res.resume().on("end", () => {
        console.log("dup", JSON.stringify(res.rawHeaders.slice(0, 4)));
        r();
      });
    });
  });
  await new Promise((r) => {
    get({ port, path: "/pair" }, (res) => {
      res.resume().on("end", () => {
        console.log("pair", res.statusCode, res.headers["content-type"]);
        r();
      });
    });
  });
  await new Promise((r) => s.close(r));
}
// 3) 拒写旗：204 write/end 同步抛，裸 end 正常结束。
{
  const s = createServer({ rejectNonStandardBodyWrites: true }, (req, res) => {
    res.writeHead(204);
    try { res.write("x"); console.log("rejw BAD"); }
    catch (e) { console.log("rejw", e.code); }
    try { res.end("x"); console.log("reje BAD"); }
    catch (e) { console.log("reje", e.code); }
    res.end();
    console.log("rejend ok");
  });
  await new Promise((r) => s.listen(0, "127.0.0.1", r));
  await new Promise((r) => {
    get({ port: s.address().port }, (res) => {
      res.resume().on("end", () => {
        console.log("rejcli", res.statusCode);
        s.close(r);
      });
    });
  });
}
// 4) Host 头：缺省恒拼端口 + IPv6 加框（raw 抓包断言）。
async function rawHost(opts) {
  return new Promise((resolve) => {
    const srv = net.createServer((sock) => {
      let buf = "";
      sock.on("data", (c) => {
        buf += c.toString();
        if (buf.includes("\r\n\r\n")) {
          const m = buf.match(/[Hh]ost: (.*)\r/);
          console.log("host", JSON.stringify(opts), JSON.stringify(m && m[1]));
          sock.end();
          srv.close(() => resolve());
        }
      });
    });
    srv.listen(0, "127.0.0.1", () => {
      const port = srv.address().port;
      get({ ...opts, createConnection: () => net.connect(port, "127.0.0.1") }, () => {}).on("error", () => {});
    });
  });
}
await rawHost({ host: "foo:1234" });
await rawHost({ host: "::1" });
// 5) trailer 随终结块。
{
  const s = createServer((req, res) => {
    res.writeHead(200, [["content-type", "text/plain"]]);
    res.addTrailers({ "x-foo": "bar" });
    res.end("stuff\n");
  });
  await new Promise((r) => s.listen(0, "127.0.0.1", r));
  await new Promise((r) => {
    get({ port: s.address().port, path: "/t" }, (res) => {
      res.resume().on("end", () => {
        console.log("trailer", res.trailers["x-foo"]);
        s.close(r);
      });
    });
  });
}
console.log("batch5-done");
"##,
    );
    for tag in [
        "num-name ERR_INVALID_HTTP_TOKEN",
        "odd ERR_INVALID_ARG_VALUE",
        "spell 2 [\"Test\"]",
        "resend ERR_HTTP_HEADERS_SENT",
        "spell-wire 2 true",
        "msg220 unknown",
        "re220 ERR_HTTP_HEADERS_SENT",
        "wire220 220 unknown 1",
        "dup [\"array-val\",\"1\",\"array-val\",\"2\"]",
        "pair 200 text/plain",
        "rejw ERR_HTTP_BODY_NOT_ALLOWED",
        "reje ERR_HTTP_BODY_NOT_ALLOWED",
        "rejend ok",
        "rejcli 204",
        "host {\"host\":\"foo:1234\"} \"foo:1234:80\"",
        "host {\"host\":\"::1\"} \"[::1]:80\"",
        "trailer bar",
        "batch5-done",
    ] {
        assert!(out.contains(tag), "missing `{tag}`; out:\n{out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase11_http_timeout_deep_host_auth_connect() {
    // TIMEOUT 深水第一铲（真机 26.8.2 逐项对拍）：url.parse 对象 hostname 优先
    // （host 含端口不再当主机名）+ options.auth 补 Basic（显式 Authorization
    // 恒赢）+ CONNECT authority-form（请求行不补斜杠、Host 取 path 本体）+
    // CONNECT 隧道 detach（两端 listenerCount 矩阵 + _httpMessage null +
    // req destroyed/close）+ req.protocol + 基类 setTimeout + socket HWM。
    // 正常 + 边界（IPv6/显式头/隧道回声）件。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r##"
import http, { createServer, get, request, OutgoingMessage } from "node:http";
import net from "node:net";
import url from "node:url";
import assert from "node:assert";

// 1) url.parse 对象：hostname 优先 + Host 带端口 + auth 补 Basic。
{
  const s = createServer((req, res) => {
    console.log("uparse", req.method, req.url, req.headers.host, req.headers.authorization);
    res.end("ok");
    s.close();
  });
  await new Promise((r) => s.listen(0, "127.0.0.1", r));
  const u = url.parse(`http://user:pass@localhost:${s.address().port}/p`);
  const q = request(u);
  q.on("response", (r) => r.resume());
  q.end();
}
// 2) 无 auth 不补（ClientRequest 查取表无 authorization）。
{
  const q = request({ port: 1, path: "/" });
  console.log("noauth", q.getHeader("authorization") ?? "none");
  q.destroy();
}
// 3) CONNECT：请求行 authority-form + Host 取 path + 隧道 detach 矩阵。
{
  const target = "tunnel.example:443";
  const srv = net.createServer((sock) => {
    sock.once("data", (d) => {
      const lines = d.toString().split("\r\n");
      console.log("conn-line", lines[0], "|", lines.includes(`Host: ${target}`));
      sock.end("HTTP/1.1 200 Connection established\r\n\r\n");
    });
  });
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  await new Promise((r) => {
    const q = request({ host: "127.0.0.1", port: srv.address().port, method: "CONNECT", path: target });
    q.on("connect", (res, sock) => {
      console.log("cli-sock",
        sock.listenerCount("close"), sock.listenerCount("data"), sock.listenerCount("end"));
      sock.destroy();
      srv.close(r);
    });
    q.end();
  });
}
{
  const s = createServer(() => {});
  s.on("connect", (req, sock) => {
    console.log("srv-sock",
      [sock.listenerCount("close"), sock.listenerCount("data"),
       sock.listenerCount("end"), sock.listenerCount("error"),
       sock.listenerCount("timeout")].join(","),
      !sock.ondata, !sock.onend);
    sock.write("HTTP/1.1 200 Connection established\r\n\r\n");
  });
  await new Promise((r) => s.listen(0, "127.0.0.1", r));
  await new Promise((r) => {
    const q = request({ port: s.address().port, method: "CONNECT", path: "g:443" });
    console.log("proto", q.protocol, "destroyed0", q.destroyed);
    q.on("connect", (res, sock) => {
      console.log("req-detach", q.destroyed);
      sock.destroy();
      s.close(r);
    });
    q.on("close", () => console.log("req-close", q.destroyed));
    q.end();
  });
}
// 4) 基类 setTimeout + socket HWM。
{
  const om = new OutgoingMessage();
  let got = 0;
  om.setTimeout(42);
  om.emit("socket", { setTimeout: (ms) => { got = ms; } });
  console.log("om-timeout", got);
  const sock = new net.Socket();
  console.log("sock-hwm", sock.writableHighWaterMark);
}
console.log("deep1-done");
"##,
    );
    for tag in [
        "uparse GET /p",
        "Basic dXNlcjpwYXNz",
        "noauth none",
        "conn-line CONNECT tunnel.example:443 HTTP/1.1 | true",
        "cli-sock 0 0 1",
        "srv-sock 0,0,1,0,0 true true",
        "proto http: destroyed0 false",
        "req-detach true",
        "req-close true",
        "om-timeout 42",
        "sock-hwm 65536",
        "deep1-done",
    ] {
        assert!(out.contains(tag), "missing `{tag}`; out:\n{out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase11_http_server_options_surface() {
    // TIMEOUT 深水第二铲：server IncomingMessage/ServerResponse 自定义类 +
    // 请求级 createConnection 透传 + socket/res HWM 对齐（真机逐项实测）。
    // 正常 + 边界（子类方法/无参 res 形/HWM 定制）件。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r##"
import http, { createServer, get, request, Server } from "node:http";
import net from "node:net";

// 1) 自定义 IncomingMessage：子类方法在 handler 可见。
{
  class MyIM extends http.IncomingMessage {
    getUserAgent() { return this.headers["user-agent"] || "unknown"; }
  }
  const s = createServer({ IncomingMessage: MyIM }, (req, res) => {
    console.log("im", req.constructor.name, req.getUserAgent());
    res.end("ok");
    s.close();
  });
  await new Promise((r) => s.listen(0, "127.0.0.1", r));
  await new Promise((r) => {
    get({ port: s.address().port, headers: { "User-Agent": "node-test" } }, (res) => {
      res.resume().on("end", r);
    });
  });
}
// 2) 自定义 ServerResponse（裸 Server 调用形）：子类方法发头。
{
  class MySR extends http.ServerResponse {
    status(code) { return this.writeHead(code, { "Content-Type": "text/plain" }); }
  }
  const s = Server({ ServerResponse: MySR }, (req, res) => {
    console.log("sr", res.constructor.name);
    res.status(200);
    res.end("ok");
    s.close();
  });
  await new Promise((r) => s.listen(0, "127.0.0.1", r));
  await new Promise((r) => {
    get({ port: s.address().port }, (res) => {
      console.log("sr-cli", res.statusCode, res.headers["content-type"]);
      res.resume().on("end", r);
    });
  });
}
// 3) 请求级 createConnection + HWM 对齐（res/socket 同值）。
{
  const s = createServer((req, res) => { res.end("x"); });
  await new Promise((r) => s.listen(0, "127.0.0.1", r));
  await new Promise((r) => {
    const q = request({
      port: s.address().port,
      createConnection(options) {
        options.readableHighWaterMark = 1024;
        return net.createConnection(options);
      },
    }, (res) => {
      console.log("hwm", res.socket === q.socket,
        res.socket.readableHighWaterMark, res.readableHighWaterMark);
      res.resume().on("end", () => s.close(r));
    });
    q.end();
  });
}
console.log("srvopt-done");
"##,
    );
    for tag in [
        "im MyIM node-test",
        "sr MySR",
        "sr-cli 200 text/plain",
        "hwm true 1024 1024",
        "srvopt-done",
    ] {
        assert!(out.contains(tag), "missing `{tag}`; out:\n{out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase11_http_invalid_char_key() {
    // splitting 套件：头值非法字符报错带 ["key"] 后缀（set/append/writeHead
    // 三路；无键走裸文案）。正常（合法值过）+ 报错三件。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r##"
import { createServer } from "node:http";
const s = createServer((req, res) => {
  for (const [fn, args] of [
    ["setHeader", ["foo", "a\rb"]],
    ["appendHeader", ["foo", "a\nb"]],
  ]) {
    try { res[fn](...args); console.log(fn, "BAD"); }
    catch (e) { console.log(fn, e.code, e.message); }
  }
  try { res.writeHead(200, { foo: "bar\r\nbaz" }); console.log("wh BAD"); }
  catch (e) { console.log("wh", e.code, e.message); }
  res.writeHead(200, { foo: "bar" });
  console.log("ok-path", res.getHeader("foo"));
  res.end("ok");
  s.close();
});
await new Promise((r) => s.listen(0, "127.0.0.1", r));
await new Promise((r) => {
  import("node:http").then(({ get }) => {
    get({ port: s.address().port }, (res) => res.resume().on("end", r));
  });
});
console.log("charkey-done");
"##,
    );
    for tag in [
        "setHeader ERR_INVALID_CHAR Invalid character in header content [\"foo\"]",
        "appendHeader ERR_INVALID_CHAR Invalid character in header content [\"foo\"]",
        "wh ERR_INVALID_CHAR Invalid character in header content [\"foo\"]",
        "ok-path bar",
        "charkey-done",
    ] {
        assert!(out.contains(tag), "missing `{tag}`; out:\n{out}");
    }
    dir.close().unwrap();
}
