//! serve 黑盒测试(对齐 src/serve.rs:静态/中间件/metrics/限流/TLS)。

mod common;

use common::*;

use assert_fs::prelude::*;

#[test]
fn phase6_serve_static_file() {
    // 正常：`/` 落到 index.html（content-type + etag），子路径按 mime，缺失 404。
    let dir = serve_fixture();
    let srv = spawn_serve(dir.path());
    let (st, h, body) = http_get(srv.port, "/", &[]);
    assert_eq!(st, 200);
    assert_eq!(body, b"<h1>hi</h1>");
    assert!(
        h.get("content-type")
            .is_some_and(|v| v.contains("text/html")),
        "headers: {h:?}"
    );
    assert!(h.contains_key("etag"), "etag missing: {h:?}");
    let (st, h, body) = http_get(srv.port, "/app.js", &[]);
    assert_eq!(st, 200);
    assert_eq!(body, b"console.log(1);\n");
    assert!(
        h.get("content-type")
            .is_some_and(|v| v.contains("javascript")),
        "headers: {h:?}"
    );
    let (st, _, _) = http_get(srv.port, "/nope.txt", &[]);
    assert_eq!(st, 404);
    dir.close().unwrap();
}

#[test]
fn phase6_serve_ts_mime_as_javascript() {
    // 正常：`.ts` 等 TS 家族按 JS MIME（Vite 对等），否则浏览器拒载模块；
    // 边界：不存在的 `.ts` 路径仍 404（重写只动成功响应）。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("main.ts")
        .write_str("export const x: number = 1;\n")
        .unwrap();
    let srv = spawn_serve(dir.path());
    let (st, h, body) = http_get(srv.port, "/main.ts", &[]);
    assert_eq!(st, 200);
    assert_eq!(body, b"export const x: number = 1;\n");
    assert!(
        h.get("content-type")
            .is_some_and(|v| v.contains("javascript")),
        "headers: {h:?}"
    );
    assert!(
        !h.get("content-type").is_some_and(|v| v.contains("video")),
        "headers: {h:?}"
    );
    let (st, _, _) = http_get(srv.port, "/nope.ts", &[]);
    assert_eq!(st, 404);
    dir.close().unwrap();
}

#[test]
fn phase6_serve_range() {
    // 正常：Range → 206 + Content-Range + 切片 body。
    let dir = serve_fixture();
    let srv = spawn_serve(dir.path());
    let (st, h, body) = http_get(srv.port, "/big.bin", &[("Range", "bytes=0-3")]);
    assert_eq!(st, 206);
    assert_eq!(body, b"0123");
    assert_eq!(
        h.get("content-range").map(String::as_str),
        Some("bytes 0-3/16"),
        "headers: {h:?}"
    );
    dir.close().unwrap();
}

#[test]
fn phase6_serve_bad_dir_errors() {
    // 报错：不存在的目录 exit=1 且可读。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .args(["--serve", "no-such-dir", "--port", "18099"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no-such-dir"), "stderr: {stderr}");
    dir.close().unwrap();
}

#[test]
fn phase6_serve_traversal_blocked() {
    // 边界：`/../` 越界读不到 root 之外的文件（非 200 且不泄露内容）。
    let dir = serve_fixture();
    let secret_name = format!("wjs-outside-secret-{}.txt", std::process::id());
    let secret = dir.path().join("..").join(&secret_name);
    std::fs::write(&secret, b"topsecret").unwrap();
    let srv = spawn_serve(dir.path());
    let (st, _, body) = http_get(srv.port, &format!("/../{secret_name}"), &[]);
    assert_ne!(st, 200, "traversal must not succeed");
    assert!(!body.windows(9).any(|w| w == b"topsecret"), "secret leaked");
    let _ = std::fs::remove_file(&secret);
    dir.close().unwrap();
}

#[test]
fn phase6_serve_gzip() {
    // 正常：大文件 + Accept-Encoding: gzip → content-encoding: gzip，解压一致。
    // （小 body 被轮子默认 predicate 跳过，见 §4.19，故用 5KB。）
    let dir = assert_fs::TempDir::new().unwrap();
    let payload: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(dir.path().join("data.bin"), &payload).unwrap();
    let srv = spawn_serve(dir.path());
    let (st, h, body) = http_get(srv.port, "/data.bin", &[("Accept-Encoding", "gzip")]);
    assert_eq!(st, 200);
    assert_eq!(
        h.get("content-encoding").map(String::as_str),
        Some("gzip"),
        "headers: {h:?}"
    );
    let decoded = {
        use std::io::Read;
        let mut d = flate2::read::GzDecoder::new(&body[..]);
        let mut out = Vec::new();
        d.read_to_end(&mut out).unwrap();
        out
    };
    assert_eq!(decoded, payload);
    dir.close().unwrap();
}

#[test]
fn phase6_serve_cors() {
    // 正常：带 Origin 请求 → access-control-allow-origin: *。
    let dir = serve_fixture();
    let srv = spawn_serve(dir.path());
    let (st, h, _) = http_get(srv.port, "/app.js", &[("Origin", "http://example.com")]);
    assert_eq!(st, 200);
    assert_eq!(
        h.get("access-control-allow-origin").map(String::as_str),
        Some("*"),
        "headers: {h:?}"
    );
    dir.close().unwrap();
}

#[test]
fn phase6_serve_request_trace() {
    // 正常：WINTERJS_LOG=winterjs=debug 下 stderr 有逐请求 method/uri/status 行。
    use std::io::Read;
    let dir = serve_fixture();
    let port = free_port();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_winterjs"))
        .args(["--serve", ".", "--port"])
        .arg(port.to_string())
        .env("WINTERJS_LOG", "winterjs=debug")
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
        assert!(std::time::Instant::now() < deadline, "serve never came up");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let (st, _, _) = http_get(port, "/app.js", &[]);
    assert_eq!(st, 200);
    let _ = child.kill();
    let _ = child.wait();
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(
        stderr.contains("method=GET") && stderr.contains("uri=/app.js"),
        "stderr:\n{stderr}"
    );
    assert!(stderr.contains("status=200"), "stderr:\n{stderr}");
    dir.close().unwrap();
}

#[test]
fn phase6_serve_metrics() {
    // 正常：打 2 个请求后 /metrics 含三指标，且计数行精确递增。
    let dir = serve_fixture();
    let srv = spawn_serve(dir.path());
    let (st, _, _) = http_get(srv.port, "/app.js", &[]);
    assert_eq!(st, 200);
    let (st, _, _) = http_get(srv.port, "/app.js", &[]);
    assert_eq!(st, 200);
    let (st, h, body) = http_get(srv.port, "/metrics", &[]);
    assert_eq!(st, 200);
    assert!(
        h.get("content-type")
            .is_some_and(|v| v.contains("text/plain")),
        "headers: {h:?}"
    );
    let text = String::from_utf8_lossy(&body).into_owned();
    assert!(
        text.contains("winterjs_serve_request_duration_seconds"),
        "metrics:\n{text}"
    );
    assert!(
        text.contains("winterjs_serve_in_flight"),
        "metrics:\n{text}"
    );
    let line = text
        .lines()
        .find(|l| {
            l.starts_with(
                "winterjs_serve_requests_total{method=\"GET\",path=\"/app.js\",status=\"200\"}",
            )
        })
        .expect("counter line present");
    let count: f64 = line.split_whitespace().nth(1).unwrap().parse().unwrap();
    assert!(count >= 2.0, "counter line: {line}");
    dir.close().unwrap();
}

#[test]
fn phase6_serve_rate_limit() {
    // 边界：`--limit-rps 1` 下连打两请求，第二个 429 + Retry-After。
    // （burst=1，第一发必过、第二发必限，时序确定；/metrics 本身也耗配额故不用它断言。）
    let dir = serve_fixture();
    let srv = spawn_serve_args(dir.path(), &["--limit-rps", "1"]);
    let (st1, _, _) = http_get(srv.port, "/app.js", &[]);
    let (st2, h2, body2) = http_get(srv.port, "/app.js", &[]);
    assert_eq!((st1, st2), (200, 429), "burst then limit");
    assert!(h2.contains_key("retry-after"), "headers: {h2:?}");
    assert_eq!(body2, b"rate limited\n");
    dir.close().unwrap();
}

#[test]
fn phase6_serve_tls() {
    // 正常：自签 PEM 起 https，真握手后静态 + /metrics 皆 200。
    let dir = serve_fixture();
    let (cert, key, trust) = make_self_signed(dir.path());
    let srv = spawn_serve_args(
        dir.path(),
        &[
            "--cert",
            cert.to_str().unwrap(),
            "--key",
            key.to_str().unwrap(),
        ],
    );
    let (st, _, body) = https_get(srv.port, "/", &trust);
    assert_eq!(st, 200);
    assert_eq!(body, b"<h1>hi</h1>");
    let (st, _, _) = https_get(srv.port, "/metrics", &trust);
    assert_eq!(st, 200);
    dir.close().unwrap();
}

#[test]
fn phase6_serve_tls_half_args() {
    // 报错：只给 --cert 不给 --key，exit=1 且指路（不静默降级明文）。
    let dir = serve_fixture();
    let (cert, _, _) = make_self_signed(dir.path());
    let out = winterjs()
        .args(["--serve", ".", "--port", "18098", "--cert"])
        .arg(&cert)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--cert and --key"), "stderr: {stderr}");
    dir.close().unwrap();
}

#[test]
fn phase6_serve_tls_bad_pem() {
    // 报错：坏 PEM exit=1 且可读（cert/key 双给但内容非法）。
    let dir = serve_fixture();
    let cert = dir.path().join("c.pem");
    let key = dir.path().join("k.pem");
    std::fs::write(&cert, b"not a pem\n").unwrap();
    std::fs::write(&key, b"not a pem\n").unwrap();
    let out = winterjs()
        .args(["--serve", ".", "--port", "18097", "--cert"])
        .arg(&cert)
        .args(["--key"])
        .arg(&key)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("bad --cert"), "stderr: {stderr}");
    dir.close().unwrap();
}

/// 空闲端口（bind :0 取号即放；被抢概率极低，抢了则 connect 轮询超时即红）。
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// 存活 serve 子进程（Drop 即 kill + wait，不泄漏）。
struct ServeGuard {
    child: std::process::Child,
    port: u16,
}

impl Drop for ServeGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 起 `winterjs serve . --port <free> [extra]`，轮询到 connect 成功（5s 超时）。
fn spawn_serve(root: &std::path::Path) -> ServeGuard {
    spawn_serve_args(root, &[])
}

fn spawn_serve_args(root: &std::path::Path, extra: &[&str]) -> ServeGuard {
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
fn http_get(
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
fn parse_response(raw: &[u8]) -> (u16, std::collections::HashMap<String, String>, Vec<u8>) {
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
fn dechunk(mut body: &[u8]) -> Vec<u8> {
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

fn serve_fixture() -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("index.html").write_str("<h1>hi</h1>").unwrap();
    dir.child("app.js").write_str("console.log(1);\n").unwrap();
    std::fs::write(dir.path().join("big.bin"), b"0123456789abcdef").unwrap();
    dir
}

/// rcgen 自签证书（SAN 127.0.0.1；返回 cert/key 路径 + 信任用 DER）。
fn make_self_signed(
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
fn https_get(
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

#[test]
fn phase11_serve_handler_missing_file_errors() {
    // 报错：`--handler` 缺文件即启动期可读错（plan4 §3 T1），exit=1。
    let dir = serve_fixture();
    let out = winterjs()
        .args(["--serve", ".", "--port", "18098", "--handler", "nope.js"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("cannot read --handler"),
        "stderr: {stderr}"
    );
    dir.close().unwrap();
}

/// 裸 socket POST（handler 回声/大体用；hermetic，与 `http_get` 同族）。
fn http_post(
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

#[test]
fn phase11_serve_dynamic_fallback_status_preserved() {
    // 正常：静态命中走 ServeDir（不进 JS）；缺失进 handler，JS 状态原样保留
    // （§4.165：`not_found_service` 恒改写 404 的反面）；POST 等非 GET/HEAD
    // 同样进 JS（`call_fallback_on_method_not_allowed`）。
    // 报错：handler 抛错 → 500 短路；边界：handler 自返 404 即 404 透传。
    let dir = serve_fixture();
    dir.child("handler.mjs")
        .write_str(
            "export default { async fetch(req) { const u = new URL(req.url); \
             if (u.pathname === '/echo' && req.method === 'POST') { \
             const b = await req.text(); \
             return new Response('echo:' + b, { status: 201 }); } \
             if (u.pathname === '/boom') throw new Error('boom-handler'); \
             if (u.pathname === '/nf') return new Response('nf', { status: 404 }); \
             return new Response('hello-t1', { status: 200 }); } };",
        )
        .unwrap();
    let handler = dir.path().join("handler.mjs").to_string_lossy().into_owned();
    let srv = spawn_serve_args(dir.path(), &["--handler", handler.as_str()]);
    let (st, _, body) = http_get(srv.port, "/", &[]);
    assert_eq!(st, 200);
    assert_eq!(body, b"<h1>hi</h1>");
    let (st, _, body) = http_get(srv.port, "/dyn-missing", &[]);
    assert_eq!(st, 200);
    assert_eq!(body, b"hello-t1");
    let (st, _, body) = http_post(srv.port, "/echo", b"abc");
    assert_eq!(st, 201);
    assert_eq!(body, b"echo:abc");
    let (st, _, _) = http_get(srv.port, "/boom", &[]);
    assert_eq!(st, 500);
    let (st, _, body) = http_get(srv.port, "/nf", &[]);
    assert_eq!(st, 404);
    assert_eq!(body, b"nf");
    dir.close().unwrap();
}

#[test]
fn phase11_serve_large_body_streaming() {
    // 正常：POST 1MB 回声逐字节一致（请求体多 Chunk 上行）；
    // GET 2MB 分带下行（响应 64KB 分片多 Chunk，§4.166），内容逐带校验。
    let dir = serve_fixture();
    dir.child("handler.mjs")
        .write_str(
            "export default { async fetch(req) { const u = new URL(req.url); \
             if (u.pathname === '/bigecho' && req.method === 'POST') { \
             const b = await req.text(); return new Response(b, { status: 200 }); } \
             if (u.pathname === '/bigdown') { const out = new Uint8Array(2097152); \
             for (let i = 0; i < 32; i++) out.fill(i & 0xff, i * 65536, (i + 1) * 65536); \
             return new Response(out, { status: 200 }); } \
             return new Response('hello-t1', { status: 200 }); } };",
        )
        .unwrap();
    let handler = dir.path().join("handler.mjs").to_string_lossy().into_owned();
    let srv = spawn_serve_args(dir.path(), &["--handler", handler.as_str()]);
    let up = vec![0x41u8; 1 << 20];
    let (st, _, back) = http_post(srv.port, "/bigecho", &up);
    assert_eq!(st, 200);
    assert_eq!(back, up);
    let (st, _, down) = http_get(srv.port, "/bigdown", &[]);
    assert_eq!(st, 200);
    assert_eq!(down.len(), 2 << 20, "down len");
    for (i, b) in down.iter().enumerate() {
        assert_eq!(*b, ((i / 65536) & 0xff) as u8, "band at {i}");
    }
    dir.close().unwrap();
}

#[test]
fn phase11_serve_concurrent_20x10() {
    // 正常：20 线程 × 10 串行 GET = 200 请求全 200 且内容对（T1 并发验收）。
    let dir = serve_fixture();
    dir.child("handler.mjs")
        .write_str("export default { async fetch() { return new Response('hello-t1', { status: 200 }); } };")
        .unwrap();
    let handler = dir.path().join("handler.mjs").to_string_lossy().into_owned();
    let srv = spawn_serve_args(dir.path(), &["--handler", handler.as_str()]);
    let port = srv.port;
    let handles: Vec<_> = (0..20)
        .map(|_| {
            std::thread::spawn(move || {
                for _ in 0..10 {
                    let (st, _, body) = http_get(port, "/dyn", &[]);
                    assert_eq!(st, 200);
                    assert_eq!(body, b"hello-t1");
                }
            })
        })
        .collect();
    for h in handles {
        h.join().expect("worker green");
    }
    dir.close().unwrap();
}
