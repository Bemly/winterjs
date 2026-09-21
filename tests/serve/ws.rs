//! tests/serve/ws.rs — WS 回声/握手/静态优先（对齐 src/serve/ws.rs）。

use assert_fs::prelude::*;
use super::helpers::*;

#[test]
fn phase11_serve_ws_echo() {
    // 正常：WS 回声经 JS onmessage（文本 + 二进制）+ 干净关闭握手；随后 HTTP 照常。
    use futures::{SinkExt as _, StreamExt as _};
    let dir = serve_fixture();
    dir.child("handler.mjs").write_str(t4_handler_src()).unwrap();
    let handler = dir.path().join("handler.mjs").to_string_lossy().into_owned();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let srv = spawn_serve_args(dir.path(), &["--handler", handler.as_str()]);
        let url = format!("ws://127.0.0.1:{}/ws", srv.port);
        let (mut ws, _) = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            tokio_tungstenite::connect_async(&url),
        )
        .await
        .expect("ws handshake timeout")
        .expect("ws handshake");
        ws.send(tokio_tungstenite::tungstenite::Message::Text("hello".into()))
            .await
            .unwrap();
        let msg = tokio::time::timeout(std::time::Duration::from_secs(10), ws.next())
            .await
            .expect("echo timeout")
            .expect("stream end")
            .expect("ws error");
        assert_eq!(msg, tokio_tungstenite::tungstenite::Message::Text("hello".into()));
        ws.send(tokio_tungstenite::tungstenite::Message::Binary(bytes::Bytes::from(vec![1u8, 2, 3])))
            .await
            .unwrap();
        let msg = tokio::time::timeout(std::time::Duration::from_secs(10), ws.next())
            .await
            .expect("echo timeout")
            .expect("stream end")
            .expect("ws error");
        assert_eq!(msg, tokio_tungstenite::tungstenite::Message::Binary(bytes::Bytes::from(vec![1u8, 2, 3])));
        ws.close(None).await.unwrap();
        let msg = tokio::time::timeout(std::time::Duration::from_secs(10), ws.next())
            .await
            .expect("close timeout")
            .expect("stream end")
            .expect("ws error");
        assert!(matches!(msg, tokio_tungstenite::tungstenite::Message::Close(_)));
        let (st, _, body) = http_get(srv.port, "/dyn", &[]);
        assert_eq!(st, 200);
        assert_eq!(body, b"http");
    });
    dir.close().unwrap();
}

#[test]
fn phase11_serve_ws_bad_handshake() {
    // 报错：缺 key / 错版本 / 非 GET 升级即 400（不进 JS）。
    // 正常：非 WS 的 Upgrade 头（h2c）零干扰，走普通 HTTP。
    let dir = serve_fixture();
    dir.child("handler.mjs").write_str(t4_handler_src()).unwrap();
    let handler = dir.path().join("handler.mjs").to_string_lossy().into_owned();
    let srv = spawn_serve_args(dir.path(), &["--handler", handler.as_str()]);
    let base = "GET /ws HTTP/1.1\r\nHost: x\r\nConnection: Upgrade\r\n";
    let (st, _, _) = ws_handshake_raw(
        srv.port,
        format!("{base}Upgrade: websocket\r\nSec-WebSocket-Version: 13\r\n\r\n").as_bytes(),
    );
    assert_eq!(st, 400);
    let (st, _, _) = ws_handshake_raw(
        srv.port,
        format!("{base}Upgrade: websocket\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 12\r\n\r\n").as_bytes(),
    );
    assert_eq!(st, 400);
    let (st, _, _) = ws_handshake_raw(
        srv.port,
        b"POST /ws HTTP/1.1\r\nHost: x\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\nContent-Length: 0\r\n\r\n",
    );
    assert_eq!(st, 400);
    let (st, _, body) = ws_handshake_raw(
        srv.port,
        b"GET /dyn HTTP/1.1\r\nHost: x\r\nUpgrade: h2c\r\nConnection: Upgrade\r\n\r\n",
    );
    assert_eq!(st, 200);
    assert_eq!(body, b"http");
    dir.close().unwrap();
}

#[test]
fn phase11_serve_ws_static_first() {
    // 路由序：已存在静态文件路径的升级仍优先进 WS（`/` 有 index.html，照返 101）。
    let dir = serve_fixture();
    dir.child("handler.mjs").write_str(t4_handler_src()).unwrap();
    let handler = dir.path().join("handler.mjs").to_string_lossy().into_owned();
    let srv = spawn_serve_args(dir.path(), &["--handler", handler.as_str()]);
    let (st, h, _) = ws_handshake_raw(
        srv.port,
        b"GET / HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
    );
    assert_eq!(st, 101);
    assert_eq!(
        h.get("sec-websocket-accept").map(String::as_str),
        Some("s3pPLMBiTxaQ9kYGzzhZRbK+xOo="),
        "headers: {h:?}"
    );
    dir.close().unwrap();
}
