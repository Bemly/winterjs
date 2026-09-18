//! tests/node/http2.rs — 对齐 src/builtins/node/http2.rs（node:http2）。

use crate::helpers::*;

#[test]
fn phase9d_http2_cleartext() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import http2, { createServer, connect, constants } from "node:http2";
console.log("const", constants.HTTP2_HEADER_METHOD === ":method", constants.NGHTTP2_NO_ERROR === 0);
const server = createServer();
server.on("request", (req, res) => {
  let b = "";
  req.on("data", (c) => (b += c));
  req.on("end", () => {
    res.setHeader("x-r", req.url);
    res.writeHead(req.url === "/missing" ? 404 : 200);
    res.end("h2:" + req.method + ":" + b);
  });
});
server.listen(0, "127.0.0.1", () => {
  const port = server.address().port;
  const sess = connect(`http://127.0.0.1:${port}`);
  sess.on("error", () => {});
  sess.on("connect", () => {
    // 两流同 session 并发（多路复用；到达序不定，收集排序后断言）
    const got = [];
    const maybeDone = () => {
      if (got.length === 4) {
        got.sort();
        console.log("mux", got.join("|"));
        sess.close();
      }
    };
    for (const [path, body] of [["/a", "one"], ["/missing", "two"]]) {
      const st = sess.request({ ":method": "POST", ":path": path });
      st.on("response", (h) => got.push(`h${h[":status"]}`));
      let b = "";
      st.on("data", (c) => (b += c));
      st.on("end", () => { got.push(`b${b}`); maybeDone(); });
      st.on("error", () => {});
      st.end(body);
    }
  });
  sess.on("close", () => server.close());
});
server.on("close", () => console.log("srv-close"));
setTimeout(() => console.log("end-ok"), 1500);
"#,
    );
    assert!(out.contains("const true true"), "out: {out}");
    assert!(out.contains("mux bh2:POST:one|bh2:POST:two|h200|h404"), "out: {out}");
    assert!(out.contains("srv-close"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9d_http2_secure() {
    let dir = assert_fs::TempDir::new().unwrap();
    let (cert_path, key_path) = write_self_signed(&dir);
    let out = run_fs_file(
        &dir,
        "p.mjs",
        &format!(
            r#"
import {{ createSecureServer, connect }} from "node:http2";
import fs from "node:fs";
const key = fs.readFileSync({key_path:?}, "utf8");
const cert = fs.readFileSync({cert_path:?}, "utf8");
try {{ createSecureServer({{}}); }} catch (e) {{ console.log("no-cert", e.constructor.name); }}
const server = createSecureServer({{ key, cert }}, (req, res) => {{
  res.end("secure-h2:" + req.url);
}});
server.listen(0, "127.0.0.1", () => {{
  const port = server.address().port;
  const sess = connect(`https://127.0.0.1:${{port}}`, {{ ca: cert }});
  sess.on("error", (e) => console.log("sess-err", e.code));
  sess.on("connect", () => {{
    const st = sess.request({{ ":path": "/s" }});
    st.on("response", (h) => console.log("h", h[":status"]));
    let b = "";
    st.on("data", (c) => (b += c));
    st.on("end", () => {{ console.log("b", b); sess.close(); }});
    st.on("error", () => {{}});
    st.end();
  }});
  sess.on("close", () => server.close());
}});
server.on("close", () => console.log("srv-close"));
setTimeout(() => console.log("end-ok"), 1500);
"#
        ),
    );
    assert!(out.contains("no-cert TypeError"), "out: {out}");
    assert!(out.contains("h 200"), "out: {out}");
    assert!(out.contains("b secure-h2:/s"), "out: {out}");
    assert!(out.contains("srv-close"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9d_http2_errors_boundary() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { createServer, connect } from "node:http2";
// 拒连：code 为 string（平台相关，不断值）
const bad = connect("http://127.0.0.1:1");
bad.on("error", (e) => {
  console.log("refused", typeof e.code);
  const server = createServer((req, res) => {
    let b = "";
    req.on("data", (c) => (b += c));
    req.on("end", () => res.end("ok:" + b.length));
  });
  server.listen(0, "127.0.0.1", () => {
    const port = server.address().port;
    const sess = connect(`http://127.0.0.1:${port}`);
    sess.on("error", () => {});
    sess.on("connect", () => {
      // 边界：空体 GET + 1MB 体往返
      const g = sess.request({ ":path": "/e" });
      g.on("response", (h) => console.log("empty-h", h[":status"]));
      let eb = "";
      g.on("data", (c) => (eb += c));
      g.on("end", () => {
        console.log("empty-b", eb);
        const big = "ab".repeat(524288);
        const st = sess.request({ ":method": "POST", ":path": "/big" });
        let rb = "";
        st.on("data", (c) => (rb += c));
        st.on("end", () => {
          console.log("big", rb === "ok:" + big.length);
          sess.close();
        });
        st.on("error", () => {});
        st.end(big);
      });
      g.on("error", () => {});
      g.end();
    });
    sess.on("close", () => server.close());
  });
  server.on("close", () => console.log("srv-close"));
});
setTimeout(() => console.log("end-ok"), 2500);
"#,
    );
    assert!(out.contains("refused string"), "out: {out}");
    assert!(out.contains("empty-h 200"), "out: {out}");
    assert!(out.contains("empty-b ok:0"), "out: {out}");
    assert!(out.contains("big true"), "out: {out}");
    assert!(out.contains("srv-close"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase10f_http2_streaming() {
    // 10f http2 流式化：服务端分块写（write/write/end 增量下发）+ 客户端 POST 体
    // + trailer 往返 + 报错（writeHead 双调 ERR_HTTP2_HEADERS_SENT）+ 边界空体。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { createServer, connect } from "node:http2";
const server = createServer();
server.on("request", (req, res) => {
  if (req.url === "/stream") {
    res.writeHead(200, { "x-a": "1" });
    res.write("chunk1-");
    setTimeout(() => { res.write("chunk2-"); setTimeout(() => res.end("chunk3"), 20); }, 20);
  } else if (req.url === "/echo") {
    let b = "";
    req.on("data", (c) => (b += c));
    req.on("end", () => res.end("echo:" + b));
  } else if (req.url === "/trailer") {
    res.writeHead(200);
    res.addTrailers({ "x-t": "1" });
    res.end("t-body");
  } else if (req.url === "/double") {
    // flushHeaders 即发头（__sendHead 置 headersSent），其后 writeHead 必抛 HEADERS_SENT。
    res.flushHeaders();
    let code = "NO-THROW";
    try { res.writeHead(200); } catch (e) { code = e.code ?? "no-code"; }
    res.end("gate:" + code);
  } else {
    let b = "";
    req.on("data", (c) => (b += c));
    req.on("end", () => res.end("empty:" + b.length));
  }
});
server.listen(0, "127.0.0.1", () => {
  const port = server.address().port;
  const sess = connect(`http://127.0.0.1:${port}`);
  sess.on("error", () => {});
  const get = (path, wantTrailers) => new Promise((resolve, reject) => {
    const st = sess.request({ ":path": path });
    st.on("response", (h) => {});
    let b = "";
    let trailers = null;
    st.on("data", (c) => (b += c));
    st.on("trailers", (t) => { trailers = t; });
    st.on("end", () => resolve({ b, trailers }));
    st.on("error", reject);
    st.end();
  });
  const post = (path, body) => new Promise((resolve, reject) => {
    const st = sess.request({ ":method": "POST", ":path": path });
    let b = "";
    st.on("data", (c) => (b += c));
    st.on("end", () => resolve(b));
    st.on("error", reject);
    st.end(body);
  });
  sess.on("connect", async () => {
    try {
      const s = await get("/stream");
      console.log("stream", s.b === "chunk1-chunk2-chunk3");
      const e = await post("/echo", "hello-h2");
      console.log("echo", e === "echo:hello-h2");
      const t = await get("/trailer");
      console.log("trailer", t.b === "t-body", t.trailers !== null && t.trailers["x-t"] === "1");
      const d = await get("/double");
      console.log("gate", d.b === "gate:ERR_HTTP2_HEADERS_SENT");
      const z = await get("/empty");
      console.log("empty", z.b === "empty:0");
      console.log("done");
      sess.close();
    } catch (e) {
      console.log("fail", e && e.code, e && e.message);
      sess.close();
    }
  });
  sess.on("close", () => server.close());
});
server.on("close", () => console.log("srv-close"));
setTimeout(() => console.log("end-ok"), 2500);
"#,
    );
    for line in [
        "stream true",
        "echo true",
        "trailer true true",
        "gate true",
        "empty true",
        "done",
        "srv-close",
        "end-ok",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}
