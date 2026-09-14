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
  try {{ http.get("https://x/"); }} catch (e) {{ console.log("proto", /node:https/.test(e.message)); }}
}});
server.on("close", () => console.log("srv-close"));
setTimeout(() => console.log("end-ok"), 1500);
"#
        ),
    );
    assert!(out.contains("proto true"), "out: {out}");
    assert!(out.contains("post 200 secure:POST:secret"), "out: {out}");
    assert!(out.contains("get 200 secure:GET:"), "out: {out}");
    assert!(out.contains("srv-close"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}
