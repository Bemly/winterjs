//! quinn 接线实证（2026-09-13）：hermetic 回环——自签证书握手 + 双向流 echo。
//! 门控：`--features quinn`（默认启用）；关掉即 0 用例（`cargo test --no-default-features`
//! 照编照过，见 `quinn_gated_off_is_empty` 的存在性约定——此处无断言，空即过）。
#![cfg(feature = "quinn")]

use std::net::SocketAddr;
use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};

const ALPN: &[u8] = b"wjs-quic-probe";

fn server_config() -> (quinn::ServerConfig, CertificateDer<'static>) {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).expect("rcgen");
    let cert_der = CertificateDer::from(cert.cert.der().to_vec());
    let key_der = PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der());
    let mut crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der.clone()], key_der.into())
        .expect("server cert");
    crypto.alpn_protocols = vec![ALPN.to_vec()];
    (quinn::ServerConfig::with_crypto(Arc::new(
        quinn::crypto::rustls::QuicServerConfig::try_from(crypto).expect("quic server crypto"),
    )), cert_der)
}

fn client_config(server_cert: &CertificateDer<'_>) -> quinn::ClientConfig {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(server_cert.clone()).expect("trust self-signed");
    let mut crypto = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    crypto.alpn_protocols = vec![ALPN.to_vec()];
    quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto).expect("quic client crypto"),
    ))
}

/// 回环：握手 + 双向流 echo（全 127.0.0.1，ephemeral 端口，不碰外网）。
#[tokio::test]
async fn quinn_loopback_handshake_and_bidi_echo() {
    let (server_cfg, server_cert) = server_config();
    let server = quinn::Endpoint::server(server_cfg, "127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .expect("quinn server endpoint");
    let server_addr = server.local_addr().unwrap();

    let mut client = quinn::Endpoint::client("127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .expect("quinn client endpoint");
    client.set_default_client_config(client_config(&server_cert));

    // accept 与 connect 必须并发（一先一后即握手超时，QUIC 无重试投递）。
    let server_task = tokio::spawn(async move {
        let incoming = server.accept().await.expect("incoming");
        let conn = incoming.await.expect("server handshake");
        let (mut send, mut recv) = conn.accept_bi().await.expect("accept bi");
        let got = recv.read_to_end(1024).await.expect("read");
        send.write_all(&got).await.expect("echo");
        send.finish().expect("finish");
        // 等对端先关（立刻 close 会 race 掉客户端未读完的流数据）。
        conn.closed().await;
    });

    let conn = client
        .connect(server_addr, "localhost")
        .expect("connect shape")
        .await
        .expect("quic handshake");
    assert_eq!(conn.remote_address(), server_addr);

    let (mut send, mut recv) = conn.open_bi().await.expect("open bi");
    send.write_all(b"hello-quic").await.expect("write");
    send.finish().expect("finish");
    let echo = recv.read_to_end(1024).await.expect("read echo");
    assert_eq!(echo, b"hello-quic");
    conn.close(0u32.into(), b"bye");
    server_task.await.expect("server task");
    client.close(0u32.into(), b"bye");
}

/// 自签信任缺失即握手失败（负路径：默认 roots 不认 rcgen 自签）。
#[tokio::test]
async fn quinn_untrusted_cert_fails_handshake() {
    let (server_cfg, _cert) = server_config();
    let server = quinn::Endpoint::server(server_cfg, "127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .expect("quinn server endpoint");
    let server_addr = server.local_addr().unwrap();
    let mut client = quinn::Endpoint::client("127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .expect("quinn client endpoint");
    // 空 roots 默认配置：connect 形态照走，握手阶段验签失败（负路径即测此）。
    client.set_default_client_config(quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(
            rustls::ClientConfig::builder()
                .with_root_certificates(rustls::RootCertStore::empty())
                .with_no_client_auth(),
        )
        .expect("empty-roots client crypto"),
    )));
    // 服务端同样要驱动 accept（否则 Initial 无人响应，客户端干等超时）。
    let server_task = tokio::spawn(async move {
        if let Some(incoming) = server.accept().await {
            let _ = incoming.await;
        }
    });
    let r = client
        .connect(server_addr, "localhost")
        .expect("connect shape")
        .await;
    let e = r.expect_err("must fail");
    let dbg = format!("{e:?}");
    assert!(dbg.contains("certificate") || dbg.contains("Certificate") || dbg.contains("alert") || dbg.contains("crypto") || dbg.contains("Crypto") || dbg.contains("UnknownIssuer"), "wrong failure: {dbg}");
    client.close(0u32.into(), b"bye");
    server_task.await.expect("server task");
}

// ── node:quic 黑盒（经 CLI，rcgen 自签 hermetic）─────────────────────────────

mod common;

use assert_fs::prelude::*;
use common::*;

/// tempdir 内跑模块，返回 stdout（失败即 panic 附 stderr）。
fn run_quic_file(dir: &assert_fs::TempDir, name: &str, source: &str) -> String {
    let file = dir.child(name);
    file.write_str(source).unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(file.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// 写自签 PEM（rcgen end-entity；serve 黑盒同款），返回 (cert_pem, key_pem) 文件名。
fn write_self_signed(dir: &assert_fs::TempDir) -> (String, String) {
    let key = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    dir.child("c.pem").write_str(&key.cert.pem()).unwrap();
    dir.child("k.pem").write_str(&key.signing_key.serialize_pem()).unwrap();
    ("c.pem".into(), "k.pem".into())
}

/// 再签一张（错配 ca 负路径用）。
fn write_other_signed(dir: &assert_fs::TempDir) {
    let key = rcgen::generate_simple_self_signed(vec!["other.invalid".into()]).unwrap();
    dir.child("other.pem").write_str(&key.cert.pem()).unwrap();
}

#[test]
fn phase9g_quic_secure_loopback() {
    let dir = assert_fs::TempDir::new().unwrap();
    let (_c, _k) = write_self_signed(&dir);
    let out = run_quic_file(
        &dir,
        "q.mjs",
        r#"
import { listen, connect } from "node:quic";
import fs from "node:fs";
const key = fs.readFileSync("k.pem", "utf8");
const cert = fs.readFileSync("c.pem", "utf8");
const ep = await listen(
  (sess) => {
    sess.on("secure", (name, alpn) => console.log("q-srv-secure", name === "localhost", alpn === "qq"));
    sess.on("close", (code) => console.log("q-srv-close", code === 0));
    sess.on("error", (e) => console.log("q-srv-err", e.message));
  },
  { port: 0, alpn: ["qq"], key, cert }
);
console.log("q-ep", ep.address().port > 0, ep.address().family === "IPv4");
const c = await connect(`localhost:${ep.address().port}`, { alpn: "qq", ca: cert });
c.on("secure", (name, alpn) => console.log("q-cli-secure", name === "localhost", alpn === "qq"));
c.on("close", (code) => console.log("q-cli-close", code === 0));
c.on("error", (e) => console.log("q-cli-err", e.message));
await new Promise((r) => setTimeout(r, 300));
console.log("q-info", c.alpnProtocol === "qq", c.encrypted === true, c.remoteAddress.port === ep.address().port, c.servername === "localhost");
const st = c.stats();
console.log("q-stats", typeof st.rttMs === "number" && st.rttMs >= 0, st.udpRxBytes > 0, st.udpTxBytes > 0);
c.close();
await new Promise((r) => setTimeout(r, 300));
ep.close();
await new Promise((r) => setTimeout(r, 300));
console.log("q-done", true);
"#,
    );
    assert!(out.contains("q-ep true true"), "out: {out}");
    assert!(out.contains("q-srv-secure true true"), "out: {out}");
    assert!(out.contains("q-cli-secure true true"), "out: {out}");
    assert!(out.contains("q-info true true true true"), "out: {out}");
    assert!(out.contains("q-stats true true true"), "out: {out}");
    assert!(out.contains("q-cli-close true"), "out: {out}");
    assert!(out.contains("q-srv-close true"), "out: {out}");
    assert!(out.contains("q-done true"), "out: {out}");
    assert!(!out.contains("q-srv-err"), "out: {out}");
    assert!(!out.contains("q-cli-err"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9g_quic_errors_boundary() {
    let dir = assert_fs::TempDir::new().unwrap();
    let (_c, _k) = write_self_signed(&dir);
    write_other_signed(&dir);
    // 校验错：缺 alpn / 坏 PEM / 坏地址，同步抛（listen async 拒因，进程 exit 1）。
    let bad = dir.child("bad.mjs");
    bad.write_str("import { listen } from \"node:quic\";\nawait listen(() => {}, { port: 0 });\n").unwrap();
    let out = winterjs().arg("--run").arg(bad.path()).current_dir(dir.path()).output().unwrap();
    assert!(!out.status.success(), "missing alpn must fail");
    assert!(String::from_utf8_lossy(&out.stderr).contains("alpn"), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    // 握手失败：ca 错配 → error + close(-1)，进程照活照退。
    let out = run_quic_file(
        &dir,
        "e.mjs",
        r#"
import { listen, connect } from "node:quic";
import fs from "node:fs";
const key = fs.readFileSync("k.pem", "utf8");
const cert = fs.readFileSync("c.pem", "utf8");
try {
  await listen(() => {}, { port: 0, alpn: ["qq"], key: "nope", cert });
  console.log("q-badkey", false);
} catch (e) { console.log("q-badkey", true); }
try {
  await connect("127.0.0.1:1", { alpn: ["x", "y"] });
  console.log("q-badaddr-never", false);
} catch (e) { console.log("q-badaddr", e.code === "ERR_INVALID_ARG_TYPE"); }
const refused = await connect("127.0.0.1:1", { alpn: "qq", rejectUnauthorized: false, idleTimeout: 1500 });
refused.on("error", () => console.log("q-refused-err", true));
refused.on("close", (c) => console.log("q-refused-close", c === -1));
const ep = await listen((sess) => { sess.on("secure", () => sess.close()); }, { port: 0, alpn: ["qq"], key, cert, cc: "bbr" });
const c = await connect(`127.0.0.1:${ep.address().port}`, { alpn: "wrong-alpn", rejectUnauthorized: false });
c.on("error", (e) => console.log("q-hs-err", e.code === "ERR_QUIC_HANDSHAKE"));
c.on("close", (code) => {
  console.log("q-hs-close", code === -1);
});
const otherCa = fs.readFileSync("other.pem", "utf8");
const bad = await connect(`127.0.0.1:${ep.address().port}`, { alpn: "qq", ca: otherCa });
bad.on("error", (e) => console.log("q-ca-err", e.code === "ERR_QUIC_HANDSHAKE"));
bad.on("close", (c) => {
  console.log("q-ca-close", c === -1);
  ep.close();
});
try {
  await connect("127.0.0.1:1", { alpn: "qq", cc: "nope" });
} catch (e) { console.log("q-badcc", e.code === "ERR_INVALID_ARG_VALUE"); }
try {
  await listen(() => {}, { port: 0, alpn: "qq", key, cert, idleTimeout: -1 });
} catch (e) { console.log("q-badidle", e.code === "ERR_OUT_OF_RANGE"); }
"#,
    );
    assert!(out.contains("q-badkey true"), "out: {out}");
    assert!(out.contains("q-badaddr true"), "out: {out}");
    assert!(out.contains("q-refused-err true"), "out: {out}");
    assert!(out.contains("q-refused-close true"), "out: {out}");
    assert!(out.contains("q-ca-err true"), "out: {out}");
    assert!(out.contains("q-ca-close true"), "out: {out}");
    assert!(out.contains("q-hs-err true"), "out: {out}");
    assert!(out.contains("q-hs-close true"), "out: {out}");
    assert!(out.contains("q-badcc true"), "out: {out}");
    assert!(out.contains("q-badidle true"), "out: {out}");
    dir.close().unwrap();
}
