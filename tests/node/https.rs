//! tests/node/https.rs — 对齐 src/builtins/node/https.rs（node:https）。

use crate::helpers::*;

#[test]
fn phase9d_https_loopback() {
    let dir = assert_fs::TempDir::new().unwrap();
    let (cert_path, key_path) = write_self_signed(&dir);
    let out = run_fs_file(
        &dir,
        "p.mjs",
        &format!(
            r#"
import https from "node:https";
import http from "node:http";
import fs from "node:fs";
const key = fs.readFileSync({key_path:?}, "utf8");
const cert = fs.readFileSync({cert_path:?}, "utf8");
const server = https.createServer({{ key, cert }}, (req, res) => {{
  let b = "";
  req.on("data", (c) => (b += c));
  req.on("end", () => res.end("secure:" + req.method + ":" + b));
}});
server.listen(0, "127.0.0.1", () => {{
  const port = server.address().port;
  const r = https.request(
    {{ port, host: "127.0.0.1", path: "/s", method: "POST", ca: cert }},
    (res) => {{
      let b = "";
      res.on("data", (c) => (b += c));
      res.on("end", () => {{
        console.log("post", res.statusCode, b);
        https.get(`https://127.0.0.1:${{port}}/g?x=1`, {{ ca: cert }}, (res2) => {{
          let b2 = "";
          res2.on("data", (c) => (b2 += c));
          res2.on("end", () => {{
            console.log("get", res2.statusCode, b2);
            server.close();
          }});
        }}).on("error", () => {{}});
      }});
    }}
  );
  r.on("error", () => {{}});
  r.end("secret");
  // 10f G3：协议错已错误码化（message 为 node 原文），断言改走 e.code（§4.36）。
  try {{ http.get("https://x/"); }} catch (e) {{ console.log("proto", e.code, e.name); }}
}});
server.on("close", () => console.log("srv-close"));
setTimeout(() => console.log("end-ok"), 1500);
"#
        ),
    );
    assert!(out.contains("proto ERR_INVALID_PROTOCOL TypeError"), "out: {out}");
    assert!(out.contains("post 200 secure:POST:secret"), "out: {out}");
    assert!(out.contains("get 200 secure:GET:"), "out: {out}");
    assert!(out.contains("srv-close"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase10b_https_keepalive_reuse() {
    // 10b：https 随行（同帧层 + tls 底座）——2 请求 1 连接。
    let dir = assert_fs::TempDir::new().unwrap();
    let (cert_path, key_path) = write_self_signed(&dir);
    let out = run_fs_file(
        &dir,
        "k.mjs",
        &format!(
            r#"
import https, {{ Agent }} from "node:https";
import fs from "node:fs";
const key = fs.readFileSync({key_path:?}, "utf8");
const cert = fs.readFileSync({cert_path:?}, "utf8");
let conns = 0;
const server = https.createServer({{ key, cert }}, (req, res) => {{
  req.on("data", () => {{}});
  req.on("end", () => res.end("s-ok"));
}});
server.on("connection", () => {{ conns++; }});
server.listen(0, "127.0.0.1", async () => {{
  const port = server.address().port;
  const agent = new Agent({{ keepAlive: true }});
  const one = () => new Promise((resolve, reject) => {{
    const r = https.get({{ port, host: "127.0.0.1", path: "/", ca: cert, agent }}, (res) => {{
      let b = "";
      res.on("data", (c) => (b += c));
      res.on("end", () => resolve(b + ":" + r.reusedSocket));
    }});
    r.on("error", reject);
  }});
  console.log("first", await one());
  console.log("second", await one());
  console.log("conns", conns);
  agent.destroy();
  server.close();
}});
server.on("close", () => console.log("srv-close"));
setTimeout(() => console.log("end-ok"), 1500);
"#
        ),
    );
    for line in [
        "first s-ok:false",
        "second s-ok:true",
        "conns 1",
        "srv-close",
        "end-ok",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}
