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
    let out = winterjs2()
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
c.on("error", (e) => console.log("q-cli-err", e.message));
const secured = new Promise((r) => c.on("secure", (name, alpn) => {
  console.log("q-cli-secure", name === "localhost", alpn === "qq");
  r();
}));
const closed = new Promise((r) => c.on("close", (code) => {
  console.log("q-cli-close", code === 0);
  r();
}));
await secured;
console.log("q-info", c.alpnProtocol === "qq", c.encrypted === true, c.remoteAddress.port === ep.address().port, c.servername === "localhost");
const st = c.stats();
console.log("q-stats", typeof st.rttMs === "number" && st.rttMs >= 0, st.udpRxBytes > 0, st.udpTxBytes > 0);
c.close();
await closed;
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
    let out = winterjs2().arg("--run").arg(bad.path()).current_dir(dir.path()).output().unwrap();
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

#[test]
fn phase9g_quic_stream_echo() {
    let dir = assert_fs::TempDir::new().unwrap();
    let (_c, _k) = write_self_signed(&dir);
    let out = run_quic_file(
        &dir,
        "s.mjs",
        r#"
import { listen, connect } from "node:quic";
import fs from "node:fs";
const key = fs.readFileSync("k.pem", "utf8");
const cert = fs.readFileSync("c.pem", "utf8");
const ep = await listen(
  (sess) => {
    sess.on("secure", () => {});
    sess.on("stream", (st) => {
      console.log("t-srvdir", st.direction === "bidi", st.id !== null);
      if (st.direction !== "bidi") return;
      st.on("data", (d) => { st.write("echo:" + d.toString()); st.end(); });
      st.on("end", () => console.log("t-srvend", true));
      st.on("close", (c) => console.log("t-srvclose", c === 0));
      st.on("error", (e) => console.log("t-srverr", e.message));
    });
    sess.on("datagram", (d) => console.log("t-srvdg", d.toString() === "ping-dg"));
    sess.on("close", () => {});
    sess.on("error", () => {});
  },
  { port: 0, alpn: ["qq"], key, cert }
);
const c = await connect(`localhost:${ep.address().port}`, { alpn: "qq", ca: cert });
await new Promise((r) => c.on("secure", r));
c.on("close", () => {});
c.on("error", (e) => console.log("t-clierr", e.message));
console.log("t-maxdg", c.maxDatagramSize > 0);
c.sendDatagram(Buffer.from("ping-dg"));
const st = await c.createBidirectionalStream();
console.log("t-open", st.direction === "bidi", st.id !== null);
let tCliDone = false, tUniDone = false;
const tMaybeClose = () => { if (tCliDone && tUniDone) c.close(); };
st.on("data", (d) => console.log("t-data", d.toString() === "echo:hello"));
st.on("end", () => console.log("t-end", true));
st.on("close", (cc) => { console.log("t-close", cc === 0); tCliDone = true; tMaybeClose(); });
st.on("error", (e) => console.log("t-cserr", e.message));
st.write("hello");
st.end();
const u = await c.createUnidirectionalStream();
console.log("t-uopen", u.direction === "send");
u.on("close", (cc) => { console.log("t-uclose", cc === 0); tUniDone = true; tMaybeClose(); });
u.on("error", (e) => console.log("t-uerr", e.message));
u.write("one-way");
u.end();
await new Promise((r) => c.on("close", r));
ep.close();
await new Promise((r) => setTimeout(r, 300));
console.log("t-done", true);
"#,
    );
    assert!(out.contains("t-maxdg true"), "out: {out}");
    assert!(out.contains("t-srvdir true true"), "out: {out}");
    assert!(out.contains("t-srvdg true"), "out: {out}");
    assert!(out.contains("t-open true true"), "out: {out}");
    assert!(out.contains("t-data true"), "out: {out}");
    assert!(out.contains("t-end true"), "out: {out}");
    assert!(out.contains("t-close true"), "out: {out}");
    assert!(out.contains("t-srvend true"), "out: {out}");
    assert!(out.contains("t-srvclose true"), "out: {out}");
    assert!(out.contains("t-uopen true"), "out: {out}");
    assert!(out.contains("t-uclose true"), "out: {out}");
    assert!(out.contains("t-done true"), "out: {out}");
    assert!(!out.contains("t-cserr"), "out: {out}");
    assert!(!out.contains("t-srverr"), "out: {out}");
    assert!(!out.contains("t-clierr"), "out: {out}");
    assert!(!out.contains("t-uerr"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9g_quic_stream_boundary() {
    let dir = assert_fs::TempDir::new().unwrap();
    let (_c, _k) = write_self_signed(&dir);
    let out = run_quic_file(
        &dir,
        "b.mjs",
        r#"
import { listen, connect } from "node:quic";
import fs from "node:fs";
const key = fs.readFileSync("k.pem", "utf8");
const cert = fs.readFileSync("c.pem", "utf8");
const ep = await listen(
  (sess) => {
    sess.on("stream", (st) => {
      if (st.direction === "receive") {
        try { st.write("x"); console.log("tu-nowrite-never", false); }
        catch (e) { console.log("tu-nowrite", e.code === "ERR_INVALID_STATE"); }
      }
      st.on("data", () => {});
      st.on("end", () => {});
      st.on("close", () => {});
      st.on("error", () => {});
    });
    sess.on("close", () => {});
    sess.on("error", () => {});
  },
  { port: 0, alpn: ["qq"], key, cert }
);
const c = await connect(`localhost:${ep.address().port}`, { alpn: "qq", ca: cert });
await new Promise((r) => c.on("secure", r));
c.on("close", () => {});
c.on("error", () => {});
// reset 码透传
const r = await c.createBidirectionalStream();
r.on("close", (cc) => console.log("tu-reset", cc === 42));
r.on("error", (e) => console.log("tu-rerr", e.message));
r.resetStream(42);
// 写后写即错
const w = await c.createBidirectionalStream();
w.on("close", () => {});
w.on("error", () => {});
w.write("a");
w.end();
try { w.write("b"); console.log("tu-wae-never", false); }
catch (e) { console.log("tu-wae", e.code === "ERR_STREAM_WRITE_AFTER_END"); }
// 单向流测量 + 超大报静默丢
const u = await c.createUnidirectionalStream();
u.on("close", () => {});
u.on("error", () => {});
u.end();
await new Promise((rr) => setTimeout(rr, 400));
// 收尾：会话关带走全流
c.close();
await new Promise((rr) => setTimeout(rr, 300));
ep.close();
await new Promise((rr) => setTimeout(rr, 300));
try {
  await c.createBidirectionalStream();
  console.log("tu-closed-never", false);
} catch (e) { console.log("tu-closed", true); }
try {
  c.sendDatagram("x".repeat(65535));
  console.log("tu-big", true);
} catch (e) { console.log("tu-big-never", false); }
try {
  r.resetStream(-1);
  console.log("tu-badcode-never", false);
} catch (e) { console.log("tu-badcode", e.code === "ERR_OUT_OF_RANGE"); }
console.log("tu-done", true);
"#,
    );
    assert!(out.contains("tu-reset true"), "out: {out}");
    assert!(!out.contains("tu-rerr"), "out: {out}");
    assert!(out.contains("tu-wae true"), "out: {out}");
    assert!(out.contains("tu-nowrite true"), "out: {out}");
    assert!(out.contains("tu-closed true"), "out: {out}");
    assert!(out.contains("tu-big true"), "out: {out}");
    assert!(out.contains("tu-badcode true"), "out: {out}");
    assert!(out.contains("tu-done true"), "out: {out}");
    dir.close().unwrap();
}

// ── 9i-5 h3 over quinn 实证（headers 交换；h3/h3-quinn 在 quinn 特性组内）────

const H3_ALPN: &[u8] = b"h3";

fn h3_server_config() -> (quinn::ServerConfig, CertificateDer<'static>) {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).expect("rcgen");
    let cert_der = CertificateDer::from(cert.cert.der().to_vec());
    let key_der = PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der());
    let mut crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der.clone()], key_der.into())
        .expect("server cert");
    crypto.alpn_protocols = vec![H3_ALPN.to_vec()];
    (quinn::ServerConfig::with_crypto(Arc::new(
        quinn::crypto::rustls::QuicServerConfig::try_from(crypto).expect("quic server crypto"),
    )), cert_der)
}

/// H3 回环：握手 → 请求头 `GET /probe` → 响应头 200 + 自定头（全 hermetic，port 0）。
#[tokio::test]
async fn phase9i_h3_over_quinn_loopback() {
    let (server_cfg, server_cert) = h3_server_config();
    let server = quinn::Endpoint::server(server_cfg, "127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .expect("quinn server endpoint");
    let server_addr = server.local_addr().unwrap();
    let mut client = quinn::Endpoint::client("127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .expect("quinn client endpoint");
    let mut client_crypto = rustls::ClientConfig::builder()
        .with_root_certificates({
            let mut roots = rustls::RootCertStore::empty();
            roots.add(server_cert).expect("trust self-signed");
            roots
        })
        .with_no_client_auth();
    client_crypto.alpn_protocols = vec![H3_ALPN.to_vec()];
    client.set_default_client_config(quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(client_crypto).expect("quic client crypto"),
    )));

    // accept 与 connect 并发（QUIC 无重试投递，见 phase9g 注）。
    // 服务端循环 accept 守到客户端关连接——发完响应即退出会连带着关连接，
    // 客户端还没收到响应帧（conn drop 即 ApplicationClose）。
    let server_task = tokio::spawn(async move {
        let incoming = server.accept().await.expect("incoming");
        let conn = incoming.await.expect("server handshake");
        let mut h3_conn: h3::server::Connection<h3_quinn::Connection, bytes::Bytes> =
            h3::server::Connection::new(h3_quinn::Connection::new(conn))
                .await
                .expect("h3 server accept");
        while let Ok(Some(resolver)) = h3_conn.accept().await {
            let Ok((req, mut stream)) = resolver.resolve_request().await else {
                continue;
            };
            assert_eq!(req.method(), http::Method::GET, "method");
            assert_eq!(req.uri().path(), "/probe", "path");
            let resp = http::Response::builder()
                .status(200)
                .header("x-wjs", "h3-ok")
                .body(())
                .expect("response build");
            stream.send_response(resp).await.expect("send response");
            stream.finish().await.expect("finish stream");
        }
    });

    let conn = client
        .connect(server_addr, "localhost")
        .expect("connect shape")
        .await
        .expect("quic handshake");
    let (mut driver, mut send_request) = h3::client::new(h3_quinn::Connection::new(conn))
        .await
        .expect("h3 client");
    // 驱动必须被轮询（wait_idle 常驻），否则响应帧无人解。
    let driver_task = tokio::spawn(async move {
        driver.wait_idle().await;
    });
    let req = http::Request::builder()
        .method("GET")
        .uri("https://localhost/probe")
        .body(())
        .expect("request build");
    let mut stream = send_request.send_request(req).await.expect("send request");
    stream.finish().await.expect("finish upload side");
    let resp = stream.recv_response().await.expect("recv response");
    assert_eq!(resp.status().as_u16(), 200, "status");
    assert_eq!(resp.headers().get("x-wjs").map(|v| v.as_bytes()), Some(&b"h3-ok"[..]), "header");
    driver_task.abort();
    server_task.await.expect("server task");
    client.close(0u32.into(), b"bye");
}

/// 9i-9：node:quic H3 面（本仓自定 API，真机无 node:quic 可对）——
/// 服务端 request 事件 + respond；客户端 request() promise；POST 体回显。
#[test]
fn phase9i_quic_h3_headers() {
    let dir = assert_fs::TempDir::new().unwrap();
    let (_c, _k) = write_self_signed(&dir);
    let out = run_quic_file(
        &dir,
        "h.mjs",
        r#"
import { listen, connect } from "node:quic";
import fs from "node:fs";
const key = fs.readFileSync("k.pem", "utf8");
const cert = fs.readFileSync("c.pem", "utf8");
const ep = await listen(
  (sess) => {
    sess.on("request", (req) => {
      if (req.path === "/echo") {
        console.log("h3-srv-post", req.method === "POST", req.headers["x-req"] === "1", req.body.toString() === "h3-body");
        req.respond({ status: 201, headers: { "x-wjs": "h3-ok" }, body: Buffer.concat([Buffer.from("echo:"), req.body]) });
      } else {
        console.log("h3-srv-get", req.method === "GET", Object.keys(req.headers).length >= 0);
        req.respond({ status: 200, headers: { "content-type": "text/plain" }, body: "hello-h3" });
      }
    });
    sess.on("close", () => {});
    sess.on("error", (e) => console.log("h3-srv-err", e.message));
  },
  { port: 0, alpn: ["h3"], key, cert }
);
const c = await connect(`localhost:${ep.address().port}`, { alpn: "h3", ca: cert });
await new Promise((r) => c.on("secure", r));
c.on("close", () => {});
c.on("error", (e) => console.log("h3-cli-err", e.message));
const r1 = await c.request({ path: "/", headers: { accept: "text/plain" } });
console.log("h3-cli-get", r1.status === 200, r1.headers["content-type"] === "text/plain", r1.body.toString() === "hello-h3");
const r2 = await c.request({ method: "POST", path: "/echo", headers: { "x-req": "1" }, body: Buffer.from("h3-body") });
console.log("h3-cli-post", r2.status === 201, r2.headers["x-wjs"] === "h3-ok", r2.body.toString() === "echo:h3-body");
// 非 h3 会话 request() 即 ERR_INVALID_PROTOCOL
const ep2 = await listen((sess) => { sess.on("close", () => {}); sess.on("error", () => {}); }, { port: 0, alpn: ["raw"], key, cert });
const c2 = await connect(`localhost:${ep2.address().port}`, { alpn: "raw", ca: cert });
await new Promise((r) => c2.on("secure", r));
c2.on("close", () => {});
try { c2.request({ path: "/" }); console.log("h3-noh3 NEVER"); }
catch (e) { console.log("h3-noh3", e.code === "ERR_INVALID_PROTOCOL"); }
c2.close();
ep2.close();
c.close();
ep.close();
await new Promise((r) => setTimeout(r, 300));
console.log("h3-done", true);
"#,
    );
    let out_str = out;
    for line in [
        "h3-srv-get true true",
        "h3-cli-get true true true",
        "h3-srv-post true true true",
        "h3-cli-post true true true",
        "h3-noh3 true",
        "h3-done true",
    ] {
        assert!(out_str.lines().any(|l| l == line), "missing line: {line}\nout: {out_str}");
    }
    assert!(!out_str.contains("h3-srv-err"), "out: {out_str}");
    assert!(!out_str.contains("h3-cli-err"), "out: {out_str}");
    dir.close().unwrap();
}
