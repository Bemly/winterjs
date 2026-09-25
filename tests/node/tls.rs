//! tests/node/tls.rs — 对齐 src/builtins/node/tls.rs（node:tls）。

use crate::helpers::*;

#[test]
fn phase9d_tls_echo_loopback() {
    let dir = assert_fs::TempDir::new().unwrap();
    let (cert_path, key_path) = write_self_signed(&dir);
    let out = run_fs_file(
        &dir,
        "p.mjs",
        &format!(
            r#"
import tls from "node:tls";
import fs from "node:fs";
const key = fs.readFileSync({key_path:?}, "utf8");
const cert = fs.readFileSync({cert_path:?}, "utf8");
try {{ tls.createServer({{}}); }} catch (e) {{ console.log("no-cert", e.constructor.name); }}
const server = tls.createServer({{ key, cert }});
server.on("secureConnection", (sock) => {{
  console.log("srv-secure", sock.encrypted, sock.authorized);
  sock.on("data", (c) => sock.write("tls-echo:" + c));
}});
server.listen(0, "127.0.0.1", () => {{
  const port = server.address().port;
  // ca 校验路径：authorized 为 true
  const cli = tls.connect({{ port, host: "127.0.0.1", ca: cert }}, () => {{
    console.log("cli-secure", cli.encrypted, cli.authorized, cli.authorizationError === null);
    cli.write("hello-tls");
  }});
  cli.on("data", (c) => {{
    console.log("cli-data", String(c));
    cli.end();
  }});
  cli.on("close", () => {{
    // rejectUnauthorized:false 路径：连上但未授权
    const cli2 = tls.connect({{ port, host: "127.0.0.1", rejectUnauthorized: false }}, () => {{
      console.log("cli2-secure", cli2.encrypted, cli2.authorized, cli2.authorizationError !== null);
      cli2.end();
    }});
    cli2.on("close", () => server.close());
    cli2.on("error", () => {{}});
  }});
  cli.on("error", () => {{}});
}});
server.on("close", () => console.log("srv-close"));
setTimeout(() => console.log("end-ok"), 1500);
"#
        ),
    );
    assert!(out.contains("no-cert TypeError"), "out: {out}");
    assert!(out.contains("cli-secure true true true"), "out: {out}");
    assert!(out.contains("srv-secure true true"), "out: {out}");
    assert!(out.contains("cli-data tls-echo:hello-tls"), "out: {out}");
    assert!(out.contains("cli2-secure true false true"), "out: {out}");
    assert!(out.contains("srv-close"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9d_tls_errors() {
    let dir = assert_fs::TempDir::new().unwrap();
    let (cert_path, key_path) = write_self_signed(&dir);
    let out = run_fs_file(
        &dir,
        "p.mjs",
        &format!(
            r#"
import tls from "node:tls";
import fs from "node:fs";
const key = fs.readFileSync({key_path:?}, "utf8");
const cert = fs.readFileSync({cert_path:?}, "utf8");
// 坏 PEM 同步 TypeError（fail fast）
try {{ tls.createServer({{ key: "nope", cert }}).listen(0); }} catch (e) {{ console.log("badkey", e.message.startsWith("TypeError:")); }}
const server = tls.createServer({{ key, cert }});
server.listen(0, "127.0.0.1", () => {{
  const port = server.address().port;
  // 自签无 ca：握手失败，错误提 certificate（不断具体码，hermetic 口径）
  const a = tls.connect({{ port, host: "127.0.0.1" }});
  a.on("error", (e) => {{
    console.log("selfsign", e.code, /certificate|issuer|verify/i.test(e.message));
    // 拒连：code 为 string（具体码平台相关，不断言值）
    const b = tls.connect({{ port: 1, host: "127.0.0.1", rejectUnauthorized: false }});
    b.on("error", (e2) => {{
      console.log("refused", typeof e2.code, b.destroyed === false);
      server.close();
    }});
  }});
}});
server.on("close", () => console.log("srv-close"));
setTimeout(() => console.log("end-ok"), 1500);
"#
        ),
    );
    assert!(out.contains("badkey true"), "out: {out}");
    assert!(out.contains("selfsign ERR_TLS_HANDSHAKE true"), "out: {out}");
    assert!(out.contains("refused string true"), "out: {out}");
    assert!(out.contains("srv-close"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}
// ── Phase 9d-7：node:http2 ────

#[test]
fn phase11_tls_x509_v1_certificates() {
    // P1（2026-09-25）：X.509 v1 证书（node fixtures agent* 同形，OpenSSL 照收、webpki 拒）——
    // 服务端出示 + 客户端经 ca 校验（v1 兜底：issuer 验签 + 有效期 + CN 主机名）。
    let dir = assert_fs::TempDir::new().unwrap();
    let fx = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tls/");
    let out = run_fs_file(
        &dir,
        "v1.mjs",
        &format!(
            r#"
import tls from "node:tls";
import fs from "node:fs";
const key = fs.readFileSync("{fx}v1-key.pem", "utf8");
const cert = fs.readFileSync("{fx}v1-cert.pem", "utf8");
const ca = fs.readFileSync("{fx}ca-cert.pem", "utf8");
const server = tls.createServer({{ key, cert }}, (s) => s.end("v1-ok"));
server.listen(0, "127.0.0.1", () => {{
  const port = server.address().port;
  // 正常：servername=localhost 对上 CN，ca 验签通过。
  const a = tls.connect({{ port, host: "127.0.0.1", servername: "localhost", ca }}, () => {{
    console.log("v1-authorized", a.authorized);
  }});
  a.on("data", (c) => console.log("v1-data", String(c)));
  a.on("close", () => {{
    // 报错：主机名不符（CN=localhost vs other.test）。
    const b = tls.connect({{ port, host: "127.0.0.1", servername: "other.test", ca }});
    b.on("error", (e) => {{
      console.log("v1-badname", /NotValidForName/.test(e.message));
      // 边界：错的 ca（自身证书当 ca）→ 签发者不认识。
      const c = tls.connect({{ port, host: "127.0.0.1", servername: "localhost", ca: cert }});
      c.on("error", (e2) => {{ console.log("v1-badca", /UnknownIssuer/.test(e2.message)); server.close(); }});
    }});
  }});
}});
"#
        ),
    );
    for line in ["v1-authorized true", "v1-data v1-ok", "v1-badname true", "v1-badca true"] {
        assert!(out.contains(line), "missing {line}; out: {out}");
    }
    dir.close().unwrap();
}
