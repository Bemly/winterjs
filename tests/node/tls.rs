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
// node 口径：无 key/cert 也可建服务（握手时才失败 → tlsClientError）。
try {{ tls.createServer({{}}); console.log("no-cert ok"); }} catch (e) {{ console.log("no-cert", e.constructor.name); }}
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
    assert!(out.contains("no-cert ok"), "out: {out}");
    assert!(out.contains("cli-secure true true true"), "out: {out}");
    assert!(out.contains("srv-secure true false"), "out: {out}"); // node：服务端未 requestCert 即 authorized=false（真机对拍）
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
// 坏 PEM 同步抛（真机 26.8.2：OpenSSL DECODER 错，非 TypeError）
try {{ tls.createServer({{ key: "nope", cert }}).listen(0); }} catch (e) {{ console.log("badkey", e.code, e.message); }}
const server = tls.createServer({{ key, cert }});
server.listen(0, "127.0.0.1", () => {{
  const port = server.address().port;
  // 自签无 ca：握手失败，错误提 certificate（不断具体码，hermetic 口径）
  const a = tls.connect({{ port, host: "127.0.0.1" }});
  a.on("error", (e) => {{
    console.log("selfsign", e.code, /certificate/i.test(e.message));
    // 拒连：code 为 string（具体码平台相关，不断言值）
    const b = tls.connect({{ port: 1, host: "127.0.0.1", rejectUnauthorized: false }});
    b.on("error", (e2) => {{
      console.log("refused", typeof e2.code, b.destroyed === true);
      server.close();
    }});
  }});
}});
server.on("close", () => console.log("srv-close"));
setTimeout(() => console.log("end-ok"), 1500);
"#
        ),
    );
    assert!(out.contains("badkey ERR_OSSL_UNSUPPORTED error:1E08010C:DECODER routines::unsupported"), "out: {out}");
    // 真机为 DEPTH_ZERO_SELF_SIGNED_CERT（rustls 统报 UnknownIssuer，自签/缺签发者不分——记档）。
    assert!(out.contains("selfsign UNABLE_TO_VERIFY_LEAF_SIGNATURE true"), "out: {out}");
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
      console.log("v1-badname", e.code === "ERR_TLS_CERT_ALTNAME_INVALID");
      // 边界：错的 ca（自身证书当 ca）→ 签发者不认识。
      const c = tls.connect({{ port, host: "127.0.0.1", servername: "localhost", ca: cert }});
      c.on("error", (e2) => {{ console.log("v1-badca", e2.code === "UNABLE_TO_VERIFY_LEAF_SIGNATURE"); server.close(); }});
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

#[test]
fn phase11_tls_socket_surface() {
    // P2（2026-09-25）：TLSSocket 建在 net.Socket 上（流面继承）+ 握手信息查询 + 顶层 API。
    let dir = assert_fs::TempDir::new().unwrap();
    let (cert_path, key_path) = write_self_signed(&dir);
    let out = run_fs_file(
        &dir,
        "s.mjs",
        &format!(
            r#"
import tls from "node:tls";
import net from "node:net";
import fs from "node:fs";
const key = fs.readFileSync({key_path:?}, "utf8");
const cert = fs.readFileSync({cert_path:?}, "utf8");
const server = tls.createServer({{ key, cert }}, (s) => s.pipe(s));
server.listen(0, "127.0.0.1", () => {{
  const c = tls.connect({{ port: server.address().port, host: "127.0.0.1", ca: cert }}, () => {{
    console.log("isnet", c instanceof net.Socket, typeof c.pipe, typeof c.setTimeout);
    console.log("proto", c.getProtocol(), c.getCipher().version === c.getProtocol(), typeof c.getCipher().name);
    const pc = c.getPeerCertificate();
    console.log("peer", Buffer.isBuffer(pc.raw), typeof pc.fingerprint256, /127\.0\.0\.1/.test(pc.subjectaltname ?? ""));
    c.end("ping");
  }});
  c.setEncoding("utf8");
  c.on("data", (d) => console.log("echo", d));
  c.on("close", () => server.close());
}});
// 正常：CA 列表 / SecureContext / 身份校验
const cas = tls.getCACertificates("system");
console.log("ca", Array.isArray(cas), cas.every((p) => p.startsWith("-----BEGIN CERTIFICATE-----")));
console.log("ctx", typeof tls.createSecureContext({{ key, cert }}).context);
console.log("ident-ok", tls.checkServerIdentity("a.example.com", {{ subjectaltname: "DNS:*.example.com" }}) === undefined);
// 报错：身份不符 / 非法 type
console.log("ident-bad", tls.checkServerIdentity("b.other.com", {{ subjectaltname: "DNS:a.example.com" }}).code);
try {{ tls.getCACertificates("nope"); }} catch (e) {{ console.log("ca-bad", e.code); }}
// 边界：未给 key/cert 也可建服务（node 口径）
console.log("nocert", tls.createServer({{}}) instanceof tls.Server);
"#
        ),
    );
    for line in [
        "isnet true function function",
        "proto TLSv1.3 true string",
        "peer true string true",
        "echo ping",
        "ca true true",
        "ctx object",
        "ident-ok true",
        "ident-bad ERR_TLS_CERT_ALTNAME_INVALID",
        "ca-bad ERR_INVALID_ARG_VALUE",
        "nocert true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing {line}; out: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase11_tls_secure_context_validation() {
    // P2（2026-09-26）：SecureContext/createSecureContext/configSecureContext 按 node 逐字移植——
    // 选项校验、OpenSSL 可观察报错（未知方法 / no cipher match / 密钥不配对）、Server 构造器校验、
    // tls.Server 无 new 可调、rootCertificates 只读、setDefaultCACertificates 报错面。
    let dir = assert_fs::TempDir::new().unwrap();
    let (cert_path, key_path) = write_self_signed(&dir);
    let out = run_fs_file(
        &dir,
        "v.js",
        &format!(
            r#"
"use strict";
const tls = require("tls");
const fs = require("fs");
const crypto = require("crypto");
const key = fs.readFileSync({key_path:?}, "utf8");
const cert = fs.readFileSync({cert_path:?}, "utf8");
const code = (f) => {{ try {{ f(); return "ok"; }} catch (e) {{ return e.code ?? e.message; }} }};
// 正常：合法选项建上下文，旧读取面仍给 PEM；Server 无 new 可调。
const sc = tls.createSecureContext({{ key, cert, ciphers: "ECDHE-RSA-AES128-GCM-SHA256:RSA@SECLEVEL=0" }});
console.log("ctx", typeof sc.context.key, sc.context.cert.includes("BEGIN CERTIFICATE"), tls.Server({{ key, cert }}) instanceof tls.Server);
// 报错：类型/方法/套件/密钥不配对/SNI 回调/servername 为 IP。
const other = crypto.generateKeyPairSync("rsa", {{ modulusLength: 2048 }}).privateKey.export({{ type: "pkcs8", format: "pem" }});
console.log("errs", [
  code(() => tls.createSecureContext({{ ciphers: 1 }})),
  code(() => tls.createSecureContext({{ secureProtocol: "blargh" }})),
  code(() => tls.createSecureContext({{ ciphers: "FOOBARBAZ" }})),
  code(() => tls.createSecureContext({{ key: other, cert }})),
  code(() => tls.createServer({{ SNICallback: 42 }})),
  code(() => tls.connect({{ port: 1, servername: "127.0.0.1" }})),
].join(","));
// 边界：rootCertificates 只读且无尾换行；bundled 即同一数组；非 PEM 全无效 / PEM 坏块两种报错。
console.log("root", code(() => {{ tls.rootCertificates = 0; }}), tls.getCACertificates("bundled") === tls.rootCertificates,
  tls.rootCertificates.every((c) => c.endsWith("\n-----END CERTIFICATE-----")));
console.log("setca", code(() => tls.setDefaultCACertificates(["nope"])),
  code(() => tls.setDefaultCACertificates(["-----BEGIN CERTIFICATE-----\nxx\n-----END CERTIFICATE-----"])),
  code(() => tls.setDefaultCACertificates([1])));
// EC PARAMETERS 块在前的密钥 PEM 可解析（OpenSSL 跳过参数块）。
const ec = crypto.generateKeyPairSync("ec", {{ namedCurve: "prime256v1" }}).privateKey.export({{ type: "sec1", format: "pem" }});
console.log("ecparams", crypto.createPrivateKey("-----BEGIN EC PARAMETERS-----\nBggqhkjOPQMBBw==\n-----END EC PARAMETERS-----\n" + ec).asymmetricKeyType);
"#
        ),
    );
    assert!(out.contains("ctx string true true"), "out: {out}");
    assert!(
        out.contains("errs ERR_INVALID_ARG_TYPE,ERR_TLS_INVALID_PROTOCOL_METHOD,ERR_SSL_NO_CIPHER_MATCH,ERR_OSSL_X509_KEY_VALUES_MISMATCH,ERR_INVALID_ARG_TYPE,ERR_INVALID_ARG_VALUE"),
        "out: {out}"
    );
    assert!(out.lines().any(|l| l.starts_with("root ") && l.ends_with(" true true") && !l.starts_with("root ok")), "out: {out}");
    assert!(out.contains("setca ERR_CRYPTO_OPERATION_FAILED ERR_OSSL_PEM_ASN1_LIB ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("ecparams ec"), "out: {out}");
}
