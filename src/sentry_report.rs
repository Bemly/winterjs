//! 崩溃上报（opt-in，plan Phase 8-c）。
//!
//! 开关：`WINTERJS2_SENTRY_DSN` 环境变量。未设/空 = 完全不初始化（零成本：无
//! 线程、无 panic hook、无网络）；设了合法 DSN = panic 事件经 sentry 自带
//! `ReqwestHttpTransport` 上报（`reqwest` 特性即含——§2 禁的是 `transport`
//! 捆绑包，它拖 native-tls；TLS 走全图统一的 ring provider）。
//!
//! transport 自带后台线程 + 独立 current-thread tokio runtime（与 JS 线程零
//! 交互）；panic 路径 `PanicIntegration` 捕获后自身 `flush(None)`（阻塞到发完
//! 才继续 unwind → 前一个 hook：release 下 human-panic，debug 下标准输出），
//! main 的 `process::exit` 前再防御性 `flush(2s)`（覆盖未来非 panic 事件源，
//! §4.8 退出顺序兼容）。上报失败绝不影响 CLI 本身（黑盒钉住）。

use std::time::Duration;

use sentry::types::Dsn;

/// DSN 环境变量名。
pub const DSN_ENV: &str = "WINTERJS2_SENTRY_DSN";

/// DSN 开关读取（trim 后非空才算设置）。
fn dsn_from_env() -> Option<String> {
    match std::env::var(DSN_ENV) {
        Ok(v) if !v.trim().is_empty() => Some(v.trim().to_string()),
        _ => None,
    }
}

/// opt-in 初始化（main 早期调用一次）。返回是否启用。
pub fn init() -> bool {
    let Some(dsn) = dsn_from_env() else {
        tracing::debug!(target: "winterjs2::sentry", "crash reporting disabled (no {DSN_ENV})");
        return false;
    };
    let Ok(parsed) = dsn.parse::<Dsn>() else {
        // 可读告警但不阻断（上报配置错误不该拦住用户的脚本）
        eprintln!("warning: {DSN_ENV} is not a valid Sentry DSN; crash reporting disabled");
        tracing::warn!(target: "winterjs2::sentry", "invalid DSN, crash reporting disabled");
        return false;
    };
    // ring provider：reqwest-no-provider 要求首次用 TLS 前 install_default；
    // fetch 是懒装，sentry 可能更早建 client，此处提前装（重复装返回 Err，忽略）。
    let _ = rustls::crypto::ring::default_provider().install_default();

    // ClientOptions 是 #[non_exhaustive]（结构体字面量禁用），default() 后字段赋值。
    // sentry::init 内部 apply_defaults：默认集成（attach-stacktrace / context /
    // panic / process-stacktrace，均随已启用特性）+ DefaultTransportFactory
    // （reqwest 特性下即 ReqwestHttpTransport，自带后台 tokio 线程）。
    let mut options = sentry::ClientOptions::default();
    options.dsn = Some(parsed);
    options.release = Some(env!("CARGO_PKG_VERSION").into());
    let guard = sentry::init(options);
    // guard 生命周期 = 进程（main 末尾 process::exit 跳过 teardown，§4.8 同哲学）
    std::mem::forget(guard);
    tracing::info!(target: "winterjs2::sentry", "crash reporting enabled");
    true
}

/// 防御性排空（main 的 process::exit 前调用；panic 路径自身已 flush(None)）。
pub fn flush() {
    if let Some(client) = sentry::Hub::current().client() {
        let _ = client.flush(Some(Duration::from_secs(2)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sentry::protocol::Event;
    use std::io::{Read as _, Write as _};
    use std::sync::Arc;

    /// 最小 HTTP stub：收一个 POST，回 200，返回 (路径, 头, body)。
    fn stub_server() -> (std::net::SocketAddr, std::thread::JoinHandle<(String, String, Vec<u8>)>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("stub bind");
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().expect("stub accept");
            let mut buf = Vec::new();
            // 读到 \r\n\r\n（头结束）
            let mut byte = [0u8; 1];
            loop {
                match sock.read(&mut byte) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        buf.push(byte[0]);
                        if buf.ends_with(b"\r\n\r\n") {
                            break;
                        }
                    }
                }
            }
            let head = String::from_utf8_lossy(&buf).into_owned();
            let content_length = head
                .lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.eq_ignore_ascii_case("content-length")
                        .then(|| v.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            let mut body = vec![0u8; content_length];
            sock.read_exact(&mut body).ok();
            let _ = sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
            let mut lines = head.lines();
            let request_line = lines.next().unwrap_or("").to_string();
            let headers = lines.map(|l| l.to_string()).collect::<Vec<_>>().join("\n");
            (request_line, headers, body)
        });
        (addr, handle)
    }

    #[test]
    fn dsn_gating() {
        // 空/空白 = 未设置
        assert_eq!(dsn_from_env(), None);
        assert!(parse_optional_dsn("  ").is_none());
        assert!(parse_optional_dsn("not-a-dsn").is_none());
        let d = parse_optional_dsn("http://key@127.0.0.1:9/42").expect("valid");
        let s = d.to_string();
        assert!(s.contains("127.0.0.1:9/42"), "dsn: {s}");
    }

    /// init 的 DSN 解析路径抽出（可测；环境变量读取在 serial 测试里走）。
    fn parse_optional_dsn(s: &str) -> Option<Dsn> {
        if s.trim().is_empty() {
            return None;
        }
        s.parse::<Dsn>().ok()
    }

    #[test]
    fn transport_sends_envelope_to_stub() {
        // provider 安装是进程级：单独跑本测试也须自给（init 路径外不依赖别的测试）
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (addr, handle) = stub_server();
        let dsn: Dsn = format!("http://testkey@{addr}/42").parse().unwrap();
        let mut options = sentry::ClientOptions::default();
        options.dsn = Some(dsn);
        let transport = sentry::transports::ReqwestHttpTransportOptions::from(
            sentry::TransportOptions::try_from_client_options(&options).unwrap(),
        )
        .build();
        // Client::from 不走 apply_defaults，手工装 transport：
        // 内层保持具体类型（Arc<T: Transport> 实现 TransportFactory，T 须 Sized，
        // dyn 对象不行），外层 Arc 做 trait 对象收缩
        let factory: Arc<dyn sentry::TransportFactory> = Arc::new(Arc::new(transport));
        options.transport = Some(factory);
        let client = sentry::Client::from(options);
        client.capture_event(
            Event {
                message: Some("winterjs2-transport-test".into()),
                level: sentry::Level::Error,
                ..Default::default()
            },
            None,
        );
        assert!(client.flush(Some(Duration::from_secs(5))), "flush must drain");
        let (request_line, headers, body) = handle.join().unwrap();
        // 路径：/api/<project>/envelope/；鉴权头带 sentry_key
        assert!(request_line.starts_with("POST /api/42/envelope/ "), "req: {request_line}");
        assert!(headers.to_lowercase().contains("x-sentry-auth:"), "headers: {headers}");
        assert!(headers.contains("sentry_key=testkey"), "headers: {headers}");
        // envelope 首行 headers + event item 内含消息
        let body_str = String::from_utf8_lossy(&body);
        assert!(body_str.contains("winterjs2-transport-test"), "body: {body_str}");
    }

    #[test]
    #[serial_test::serial]
    fn panic_integration_reports_via_hook() {
        let (addr, handle) = stub_server();
        // 经环境变量走真实 init 路径（hook 链、hub 绑定全真）
        // SAFETY: 测试进程单实例读取；#[serial] 防并行竞态
        unsafe { std::env::set_var(DSN_ENV, format!("http://panickey@{addr}/7")) };
        assert!(init());
        // panic hook 捕获 + flush(None)；catch_unwind 吞掉 unwind
        let _ = std::panic::catch_unwind(|| panic!("winterjs2-panic-test"));
        let (request_line, _headers, body) = handle.join().unwrap();
        assert!(request_line.starts_with("POST /api/7/envelope/ "), "req: {request_line}");
        let body_str = String::from_utf8_lossy(&body);
        assert!(body_str.contains("winterjs2-panic-test"), "body: {body_str}");
        // 清理：未设 = 关闭（后续测试不受影响）
        // SAFETY: 同上
        unsafe { std::env::remove_var(DSN_ENV) };
    }
}
