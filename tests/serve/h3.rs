//! tests/serve/h3.rs — H3 同 Router（对齐 src/serve/h3.rs）。

use assert_fs::prelude::*;
use super::helpers::*;

#[test]
fn phase11_serve_h3_same_router() {
    // 正常：QUIC + H3 同端口同 Router 回声（scheme=https:）。
    // 环境注：本机 curl 无 http3（SecureTransport 版），以 harness 探针验收（plan4 §3 T3）。
    let dir = serve_fixture();
    let (cert, key, trust) = make_self_signed(dir.path());
    dir.child("handler.mjs").write_str(t2_handler_src()).unwrap();
    let handler = dir.path().join("handler.mjs").to_string_lossy().into_owned();
    let cert_s = cert.to_string_lossy().into_owned();
    let key_s = key.to_string_lossy().into_owned();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let srv = spawn_serve_args(
            dir.path(),
            &["--cert", cert_s.as_str(), "--key", key_s.as_str(), "--handler", handler.as_str()],
        );
        let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(trust.clone()).unwrap();
        let mut crypto = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        crypto.alpn_protocols = vec![b"h3".to_vec()];
        let qc = quinn::crypto::rustls::QuicClientConfig::try_from(crypto).unwrap();
        endpoint.set_default_client_config(quinn::ClientConfig::new(std::sync::Arc::new(qc)));
        let addr: std::net::SocketAddr =
            format!("127.0.0.1:{}", srv.port).parse().unwrap();
        let conn = endpoint.connect(addr, "127.0.0.1").unwrap().await.unwrap();
        let (mut driver, mut send) = h3::client::builder()
            .build::<h3_quinn::Connection, _, bytes::Bytes>(h3_quinn::Connection::new(conn))
            .await
            .unwrap();
        tokio::spawn(async move {
            let _ = driver.wait_idle().await;
        });
        let req = http::Request::builder().uri("https://127.0.0.1/dyn").body(()).unwrap();
        let mut stream = send.send_request(req).await.unwrap();
        // H3 半关闭纪律：HEADERS 后必须 finish（FIN），否则服务端等 body 结束永挂。
        stream.finish().await.unwrap();
        let rsp = stream.recv_response().await.unwrap();
        assert_eq!(rsp.status(), 200);
        let mut body = Vec::new();
        while let Some(chunk) = stream.recv_data().await.unwrap() {
            use bytes::Buf;
            body.extend_from_slice(chunk.chunk());
        }
        assert_eq!(&body[..], b"proto=https:");
        endpoint.close(0u32.into(), b"done");
    });
    dir.close().unwrap();
}

#[test]
fn phase11_serve_h3_skipped_without_cert() {
    // 边界：无证书即 H3 跳过 + warn，H1 照服（plan4 §3 T3）。
    use std::io::Read;
    let dir = serve_fixture();
    let port = free_port();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_winterjs2"))
        .args(["--serve", ".", "--port"])
        .arg(port.to_string())
        .current_dir(dir.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("serve spawns");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            panic!("serve on :{port} never came up");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let (st, _, _) = http_get(port, "/nope.txt", &[]);
    assert_eq!(st, 404);
    let _ = child.kill();
    let mut stderr = String::new();
    child.stderr.take().unwrap().read_to_string(&mut stderr).unwrap();
    let _ = child.wait();
    assert!(stderr.contains("H3 skipped"), "stderr: {stderr}");
    dir.close().unwrap();
}
