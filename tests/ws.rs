//! WebSocket 黑盒测试(对齐 src/builtins/ws.rs)。

mod common;

use common::*;

use assert_fs::prelude::*;

#[test]
fn phase3_websocket_echo_and_close() {
    // 本机回显服务器（tokio，ephemeral 端口）：文本/二进制原样返回。
    // std listener 主线程建好后移交线程——backlog 接住先到的 SYN，无需轮询等待。
    let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std_listener.set_nonblocking(true).unwrap();
    let port = std_listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let listener = tokio::net::TcpListener::from_std(std_listener).unwrap();
                let (stream, _) = listener.accept().await.unwrap();
                let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
                use futures::{SinkExt as _, StreamExt as _};
                while let Some(msg) = ws.next().await {
                    let Ok(msg) = msg else { break };
                    if msg.is_text() || msg.is_binary() {
                        if ws.send(msg).await.is_err() {
                            break;
                        }
                    } else if msg.is_close() {
                        // 回 close 帧完成握手，再排空到对端 FIN（避免 RST 竞态）
                        let _ = ws
                            .send(tokio_tungstenite::tungstenite::Message::Close(None))
                            .await;
                        while ws.next().await.is_some() {}
                        break;
                    }
                }
            });
    });
    let code = format!(
        r#"const log = []; const ws = new WebSocket("ws://127.0.0.1:{port}/c"); ws.onopen = () => ws.send("ping"); ws.onmessage = (e) => {{ if (typeof e.data === "string") {{ log.push(e.data); ws.send(new Uint8Array([7, 8])); }} else {{ log.push("bin:" + new Uint8Array(e.data).join(",")); ws.close(1000, "bye"); }} }}; ws.onclose = (e) => console.log(log.join("|") + "|close:" + e.code + ":" + e.wasClean); undefined;"#
    );
    assert_eq!(
        stdout_of(&mut winterjs2().args(["--eval", &code])),
        "ping|bin:7,8|close:1000:true\n"
    );
}

#[test]
fn phase3_websocket_bad_url_and_send_while_connecting() {
    // 非 ws scheme 直接抛；CONNECTING 时 send 抛（连不上的端口测 readyState 报错面）
    let out = winterjs2()
        .args(["--eval", r#"new WebSocket("http://x/")"#])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let code = format!(
        r#"const ws = new WebSocket("ws://127.0.0.1:{port}/"); try {{ ws.send("early"); console.log("no-throw"); }} catch (e) {{ console.log("send-while-connecting-throws"); }}"#
    );
    assert_eq!(
        stdout_of(&mut winterjs2().args(["--eval", &code])),
        "send-while-connecting-throws\n"
    );
}

#[test]
fn phase3_websocket_wss_self_signed() {
    // rcgen 自签 127.0.0.1 → tokio-rustls wss 回显服务；客户端经
    // WINTERJS2_TEST_CA_PEMFILE 接缝信任（生产默认链不变，见 src/builtins/ws.rs）。
    use base64::Engine as _;
    let certified = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()]).unwrap();
    let cert_der = certified.cert.der().to_vec();
    let key_der = certified.signing_key.serialize_der();
    let pem = format!(
        "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
        base64::engine::general_purpose::STANDARD.encode(&cert_der)
    );
    let dir = assert_fs::TempDir::new().unwrap();
    let ca_file = dir.child("test-ca.pem");
    ca_file.write_str(&pem).unwrap();
    let ca_path = ca_file.path().to_owned();

    let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std_listener.set_nonblocking(true).unwrap();
    let port = std_listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let server_config = rustls::ServerConfig::builder()
                    .with_no_client_auth()
                    .with_single_cert(
                        vec![rustls::pki_types::CertificateDer::from(cert_der)],
                        rustls::pki_types::PrivateKeyDer::Pkcs8(
                            rustls::pki_types::PrivatePkcs8KeyDer::from(key_der),
                        ),
                    )
                    .unwrap();
                let acceptor = tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(server_config));
                let listener = tokio::net::TcpListener::from_std(std_listener).unwrap();
                let (stream, _) = listener.accept().await.unwrap();
                let tls = acceptor.accept(stream).await.unwrap();
                let mut ws = tokio_tungstenite::accept_async(tls).await.unwrap();
                use futures::{SinkExt as _, StreamExt as _};
                while let Some(msg) = ws.next().await {
                    let Ok(msg) = msg else { break };
                    if msg.is_text() || msg.is_binary() {
                        if ws.send(msg).await.is_err() {
                            break;
                        }
                    } else if msg.is_close() {
                        let _ = ws
                            .send(tokio_tungstenite::tungstenite::Message::Close(None))
                            .await;
                        while ws.next().await.is_some() {}
                        break;
                    }
                }
            });
    });
    let code = format!(
        r#"const log = []; const ws = new WebSocket("wss://127.0.0.1:{port}/c"); ws.onopen = () => ws.send("secure-ping"); ws.onmessage = (e) => {{ log.push(e.data); ws.close(1000, "bye"); }}; ws.onclose = (e) => console.log(log.join("|") + "|close:" + e.code + ":" + e.wasClean); undefined;"#
    );
    let out = winterjs2()
        .env("WINTERJS2_TEST_CA_PEMFILE", &ca_path)
        .args(["--eval", &code])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "secure-ping|close:1000:true\n"
    );
}
