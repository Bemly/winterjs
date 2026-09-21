//! tests/serve/helpers.rs — serve 黑盒共享脚手架（起服/裸 socket/自签证书）。

use assert_fs::prelude::*;

/// 空闲端口（bind :0 取号即放；被抢概率极低，抢了则 connect 轮询超时即红）。
pub(crate) fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// 存活 serve 子进程（Drop 即 kill + wait，不泄漏）。
pub(crate) struct ServeGuard {
    child: std::process::Child,
    pub(crate) port: u16,
}

impl Drop for ServeGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 起 `winterjs serve . --port <free> [extra]`，轮询到 connect 成功（5s 超时）。
pub(crate) fn spawn_serve(root: &std::path::Path) -> ServeGuard {
    spawn_serve_args(root, &[])
}

pub(crate) fn spawn_serve_args(root: &std::path::Path, extra: &[&str]) -> ServeGuard {
    let port = free_port();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_winterjs"))
        .args(["--serve", ".", "--port"])
        .arg(port.to_string())
        .args(extra)
        .current_dir(root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("serve spawns");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return ServeGuard { child, port };
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            panic!("serve on :{port} never came up");
        }
        // 子进程早退（如 bind 失败）直接把 stderr 捞出来当失败信息。
        if let Ok(Some(st)) = child.try_wait() {
            panic!("serve exited early: {st}");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// 裸 socket GET（hermetic，不依赖外部 client；`extra` 为附加请求头）。
pub(crate) fn http_get(
    port: u16,
    path: &str,
    extra: &[(&str, &str)],
) -> (u16, std::collections::HashMap<String, String>, Vec<u8>) {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let mut req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n");
    for (k, v) in extra {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes()).unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).unwrap();
    parse_response(&raw)
}

/// 原始 HTTP 响应解析（明文/TLS 共用）。
pub(crate) fn parse_response(raw: &[u8]) -> (u16, std::collections::HashMap<String, String>, Vec<u8>) {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("http response has head");
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let body = raw[split + 4..].to_vec();
    let mut lines = head.lines();
    let status: u16 = lines
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let mut headers = std::collections::HashMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_lowercase(), v.trim().to_owned());
        }
    }
    // 压缩响应走 chunked（tower-http 默认），此处解帧再返回。
    let body = if headers
        .get("transfer-encoding")
        .is_some_and(|v| v.contains("chunked"))
    {
        dechunk(&body)
    } else {
        body
    };
    (status, headers, body)
}

/// 解 HTTP chunked 帧（测试 helper；非法帧即 panic，属测试失败）。
pub(crate) fn dechunk(mut body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let end = body
            .windows(2)
            .position(|w| w == b"\r\n")
            .expect("chunk size line");
        let size_line = std::str::from_utf8(&body[..end]).expect("chunk size utf8");
        let size = usize::from_str_radix(size_line.split(';').next().unwrap().trim(), 16)
            .expect("chunk size hex");
        body = &body[end + 2..];
        if size == 0 {
            break;
        }
        out.extend_from_slice(&body[..size]);
        body = &body[size + 2..];
    }
    out
}

pub(crate) fn serve_fixture() -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("index.html").write_str("<h1>hi</h1>").unwrap();
    dir.child("app.js").write_str("console.log(1);\n").unwrap();
    std::fs::write(dir.path().join("big.bin"), b"0123456789abcdef").unwrap();
    dir
}

/// rcgen 自签证书（SAN 127.0.0.1；返回 cert/key 路径 + 信任用 DER）。
pub(crate) fn make_self_signed(
    dir: &std::path::Path,
) -> (
    std::path::PathBuf,
    std::path::PathBuf,
    rustls::pki_types::CertificateDer<'static>,
) {
    let key = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()]).unwrap();
    let cert_pem = key.cert.pem();
    let key_pem = key.signing_key.serialize_pem();
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    std::fs::write(&cert_path, &cert_pem).unwrap();
    std::fs::write(&key_path, &key_pem).unwrap();
    (cert_path, key_path, key.cert.der().clone())
}

/// TLS GET（rustls client 信任自签根； noble negotiates http/1.1 by default）。
pub(crate) fn https_get(
    port: u16,
    path: &str,
    trust: &rustls::pki_types::CertificateDer<'static>,
) -> (u16, std::collections::HashMap<String, String>, Vec<u8>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let mut roots = rustls::RootCertStore::empty();
        roots.add(trust.clone()).unwrap();
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(config));
        let tcp = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        let name = rustls::pki_types::ServerName::try_from("127.0.0.1").unwrap();
        let mut tls = connector.connect(name, tcp).await.unwrap();
        tls.write_all(
            format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes(),
        )
        .await
        .unwrap();
        let mut raw = Vec::new();
        tls.read_to_end(&mut raw).await.unwrap();
        parse_response(&raw)
    })
}

/// 裸 socket POST（handler 回声/大体用；hermetic，与 `http_get` 同族）。
pub(crate) fn http_post(
    port: u16,
    path: &str,
    body: &[u8],
) -> (u16, std::collections::HashMap<String, String>, Vec<u8>) {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    s.write_all(head.as_bytes()).unwrap();
    s.write_all(body).unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).unwrap();
    parse_response(&raw)
}

/// TLS 任意方法请求（信任自签根；无 ALPN 即 HTTP/1.1，与既有 `https_get` 同族）。
pub(crate) fn https_req(
    port: u16,
    trust: &rustls::pki_types::CertificateDer<'static>,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
) -> (u16, std::collections::HashMap<String, String>, Vec<u8>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let mut roots = rustls::RootCertStore::empty();
        roots.add(trust.clone()).unwrap();
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(config));
        let tcp = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        let name = rustls::pki_types::ServerName::try_from("127.0.0.1").unwrap();
        let mut tls = connector.connect(name, tcp).await.unwrap();
        let mut head =
            format!("{method} {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n");
        if let Some(b) = body {
            head.push_str(&format!("Content-Length: {}\r\n", b.len()));
        }
        head.push_str("\r\n");
        tls.write_all(head.as_bytes()).await.unwrap();
        if let Some(b) = body {
            tls.write_all(b).await.unwrap();
        }
        let mut raw = Vec::new();
        tls.read_to_end(&mut raw).await.unwrap();
        parse_response(&raw)
    })
}

/// keep-alive 首包分帧读（动态响应恒 chunked，以终结块 `0\r\n\r\n` 判尾）。
pub(crate) fn read_framed(s: &std::net::TcpStream) -> Vec<u8> {
    use std::io::Read;
    let mut s = s;
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = s.read(&mut buf).expect("framed read");
        assert!(n > 0, "eof before frame end");
        raw.extend_from_slice(&buf[..n]);
        let done = raw.windows(4).any(|w| w == b"\r\n\r\n")
            && raw.ends_with(b"0\r\n\r\n");
        if done {
            return raw;
        }
    }
}

/// h2 响应体收齐（0.4 的 `RecvStream` 未实现 `http_body::Body`，手工 `data()` 循环）。
pub(crate) async fn h2_bytes(mut b: h2::RecvStream) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(chunk) = b.data().await {
        out.extend_from_slice(&chunk.expect("h2 data"));
    }
    out
}

/// T2 handler 形状：`/dyn` 回显 scheme，`/echo` POST 回声 201。
pub(crate) fn t2_handler_src() -> &'static str {
    "export default { async fetch(req) { const u = new URL(req.url); \
     if (u.pathname === '/echo' && req.method === 'POST') { \
     const b = await req.text(); return new Response('echo:' + b, { status: 201 }); } \
     return new Response('proto=' + u.protocol, { status: 200 }); } };"
}

/// T4 upgrade handler 形状：upgrade 请求配对 socket 回声，其余走 HTTP。
pub(crate) fn t4_handler_src() -> &'static str {
    "export default { async fetch(req) { \
     if ((req.headers.get('upgrade') || '').toLowerCase() === 'websocket') { \
     const ws = __wjs_serve_socket(req); ws.onmessage = (e) => { ws.send(e.data); }; return ws; } \
     return new Response('http', { status: 200 }); } };"
}

/// 裸 socket 读完整 HTTP 消息（分帧：chunked 终结块 / Content-Length / 关写即尾）。
/// keep-alive + 小体（无长度头即 chunked）不靠运气等分包（§4.122 TCP 分包姊妹篇）。
pub(crate) fn read_http_message(s: &std::net::TcpStream) -> Vec<u8> {
    use std::io::Read;
    let mut s = s;
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    let head_end = loop {
        let n = s.read(&mut buf).expect("hs read");
        assert!(n > 0, "eof before head end");
        raw.extend_from_slice(&buf[..n]);
        if let Some(p) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break p;
        }
    };
    let head = String::from_utf8_lossy(&raw[..head_end]).into_owned();
    let status: u16 = head
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    // 101 后连接保持开放（WS 会话）→ 只返回头，不等体。
    if status == 101 {
        return raw;
    }
    let chunked = head.lines().skip(1).any(|l| {
        l.to_lowercase().starts_with("transfer-encoding") && l.to_lowercase().contains("chunked")
    });
    let content_len = head.lines().skip(1).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        if k.trim().eq_ignore_ascii_case("content-length") {
            v.trim().parse::<usize>().ok()
        } else {
            None
        }
    });
    if chunked {
        loop {
            if raw.ends_with(b"0\r\n\r\n") {
                return raw;
            }
            let n = s.read(&mut buf).expect("chunked read");
            assert!(n > 0, "eof before chunk end");
            raw.extend_from_slice(&buf[..n]);
        }
    }
    if let Some(n) = content_len {
        while raw.len() < head_end + 4 + n {
            let m = s.read(&mut buf).expect("fixed read");
            assert!(m > 0, "eof before body end");
            raw.extend_from_slice(&buf[..m]);
        }
        return raw;
    }
    loop {
        let n = s.read(&mut buf).expect("close read");
        if n == 0 {
            return raw;
        }
        raw.extend_from_slice(&buf[..n]);
    }
}

/// 裸 socket WS 握手（400 三件 + 非 WS Upgrade 零干扰 + 静态优先 101）。
pub(crate) fn ws_handshake_raw(port: u16, req: &[u8]) -> (u16, std::collections::HashMap<String, String>, Vec<u8>) {
    use std::io::Write;
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    s.write_all(req).unwrap();
    let raw = read_http_message(&s);
    parse_response(&raw)
}
