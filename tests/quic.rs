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
