//! 静态文件服务（plan Phase 6-d1）：`winterjs serve [dir] [--host] [--port]`。
//!
//! - `tower-http` `ServeDir` 直服目录：mime（`mime_guess` 内建）、etag、
//!   range（206）全由轮子提供；目录自动拼 `index.html`（行为由黑盒钉住）。
//! - 就绪后 stdout 打印 `serving <dir> on http://<addr>`（`--port 0` 时回显
//!   实际端口，黑盒据此连接）；LAN 行 best-effort（拿不到就跳过）。
//! - SIGINT/SIGTERM 到即优雅停机（d3 再补 in-flight 排空语义，此处即停）。
//! - 复用 main 的 current-thread runtime（serve 路径无 JS，无 §6 线程模型冲突）。

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use axum::Router;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use crate::error::Error;

/// 服务选项（CLI 直传）。
pub struct ServeOpts {
    pub dir: PathBuf,
    pub host: String,
    pub port: u16,
    /// 每秒请求上限（0 = 不限；`governor` 全局限流）。
    pub limit_rps: u32,
    /// TLS 证书/私钥（PEM；必须同给同缺，见 `load_tls`）。
    pub cert: Option<PathBuf>,
    pub key: Option<PathBuf>,
}

/// TLS 配置加载（PEM 解析；`rustls-pemfile` 轮子；`ring` provider）。
/// 纯 IO，单测覆盖坏输入。
pub fn load_tls(cert_path: &Path, key_path: &Path) -> Result<rustls::ServerConfig, Error> {
    use std::io::BufReader;
    let cert_file = std::fs::File::open(cert_path)
        .map_err(|e| Error::Other(format!("cannot read --cert '{}': {e}", cert_path.display())))?;
    let certs: Vec<rustls::pki_types::CertificateDer<'static>> =
        rustls_pemfile::certs(&mut BufReader::new(cert_file))
            .collect::<Result<_, _>>()
            .map_err(|e| Error::Other(format!("bad --cert PEM '{}': {e}", cert_path.display())))?;
    if certs.is_empty() {
        return Err(Error::Other(format!("bad --cert PEM '{}': no certificate found", cert_path.display())));
    }
    let key_file = std::fs::File::open(key_path)
        .map_err(|e| Error::Other(format!("cannot read --key '{}': {e}", key_path.display())))?;
    let key = rustls_pemfile::private_key(&mut BufReader::new(key_file))
        .map_err(|e| Error::Other(format!("bad --key PEM '{}': {e}", key_path.display())))?
        .ok_or_else(|| Error::Other(format!("bad --key PEM '{}': no private key found", key_path.display())))?;
    // provider 与 fetch 侧同源（顶层 ring；重复 install 无害，见 §2 门控）。
    let _ = rustls::crypto::ring::default_provider().install_default();
    rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| Error::Other(format!("cannot build TLS config: {e}")))
}

/// axum `Listener` 的 TLS 实现（TCP accept 后做服务端握手；握手失败记 warn
/// 并继续 accept——trait 签名无 Result 通道，只能内部消化）。
struct TlsListener {
    tcp: tokio::net::TcpListener,
    acceptor: tokio_rustls::TlsAcceptor,
}

impl axum::serve::Listener for TlsListener {
    type Io = tokio_rustls::server::TlsStream<tokio::net::TcpStream>;
    type Addr = std::net::SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let (tcp, addr) = match self.tcp.accept().await {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(target: "winterjs::serve", "accept failed: {e}");
                    continue;
                }
            };
            match self.acceptor.accept(tcp).await {
                Ok(tls) => return (tls, addr),
                Err(e) => {
                    tracing::warn!(target: "winterjs::serve", "TLS handshake failed: {e}");
                }
            }
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.tcp.local_addr()
    }
}

/// systemd 就绪通知（仅 linux；非 systemd 环境/失败一律忽略，只记 debug）。
#[cfg(target_os = "linux")]
fn notify_ready() {
    let state = [(systemd::daemon::STATE_READY.to_owned(), "1".to_owned())];
    match systemd::daemon::notify(false, state.iter()) {
        Ok(true) => tracing::debug!(target: "winterjs::serve", "systemd READY notified"),
        Ok(false) => tracing::debug!(target: "winterjs::serve", "not running under systemd"),
        Err(e) => tracing::debug!(target: "winterjs::serve", "systemd notify failed: {e}"),
    }
}

/// 指标名（named 指标文档见 `docs/metrics.md`）。
pub const METRIC_REQUESTS: &str = "winterjs_serve_requests_total";
pub const METRIC_DURATION: &str = "winterjs_serve_request_duration_seconds";
pub const METRIC_IN_FLIGHT: &str = "winterjs_serve_in_flight";

/// RPS → 配额（0 表关闭；否则每 `1/rps` 秒补 1，burst=1）。
/// 纯函数，单测覆盖。
pub fn quota_for(rps: u32) -> Option<governor::Quota> {
    if rps == 0 {
        return None;
    }
    std::time::Duration::from_secs(1)
        .checked_div(rps)
        .and_then(governor::Quota::with_period)
}

/// 等待时长 → `Retry-After` 秒（向上取整，最小 1）。纯函数，单测覆盖。
pub fn retry_after_secs(wait: std::time::Duration) -> u64 {
    wait.as_secs().saturating_add(1).max(1)
}

/// 目录校验（存在 + 是目录；返回规范绝对路径，日志/banner 用）。
pub fn validate_dir(dir: &Path) -> Result<PathBuf, Error> {
    let meta = std::fs::metadata(dir).map_err(|e| {
        Error::Other(format!("cannot serve '{}': {e}", dir.display()))
    })?;
    if !meta.is_dir() {
        return Err(Error::Other(format!(
            "cannot serve '{}': not a directory",
            dir.display()
        )));
    }
    std::fs::canonicalize(dir)
        .map_err(|e| Error::Other(format!("cannot serve '{}': {e}", dir.display())))
}

/// 就绪行（纯函数，单测覆盖；TLS 时 scheme 为 `https`）。
pub fn ready_line_scheme(root: &Path, scheme: &str, addr: &SocketAddr) -> String {
    format!("serving {} on {scheme}://{addr}", root.display())
}

/// LAN 地址（best-effort；拿不到返回 `None`，不中断服务）。
pub fn lan_addr(port: u16) -> Option<SocketAddr> {
    let ip = local_ip_address::local_ip().ok()?;
    format!("{ip}:{port}").parse().ok()
}

/// LAN URL 的终端二维码（纯函数，单测覆盖；`qrcode` 只做矩阵，渲染内建 unicode）。
pub fn qr_block(url: &str) -> Option<String> {
    let code = qrcode::QrCode::new(url.as_bytes()).ok()?;
    Some(
        code.render::<qrcode::render::unicode::Dense1x2>()
            .quiet_zone(false)
            .module_dimensions(2, 1)
            .build(),
    )
}

/// 启动并跑到信号到来。调用方（main）已在 tokio runtime 内。
pub async fn serve(opts: &ServeOpts) -> Result<(), Error> {
    let root = validate_dir(&opts.dir)?;
    // `--cert/--key` 必须同给同缺（单给即报，不静默降级为明文）。
    let tls = match (&opts.cert, &opts.key) {
        (Some(c), Some(k)) => Some(load_tls(c, k)?),
        (None, None) => None,
        _ => {
            return Err(Error::Other(
                "--cert and --key must be given together (PEM files)".into(),
            ));
        }
    };
    let scheme = if tls.is_some() { "https" } else { "http" };
    let tcp = tokio::net::TcpListener::bind((opts.host.as_str(), opts.port))
        .await
        .map_err(|e| {
            Error::Other(format!("cannot bind {}:{}: {e}", opts.host, opts.port))
        })?;
    let addr = tcp
        .local_addr()
        .map_err(|e| Error::Other(format!("cannot read bound address: {e}")))?;
    println!("{}", ready_line_scheme(&root, scheme, &addr));
    if let Some(lan) = lan_addr(addr.port()) {
        println!("lan: {scheme}://{lan}");
        if let Some(qr) = qr_block(&format!("{scheme}://{lan}")) {
            println!("{qr}");
        }
    }
    #[cfg(target_os = "linux")]
    notify_ready();
    tracing::info!(target: "winterjs::serve", %addr, scheme, dir = %root.display(), "serving");
    // Prometheus 注册为全局 recorder（同进程只许一次；双 serve 本就撞端口）。
    let metrics = metrics_exporter_prometheus::PrometheusBuilder::new()
        .install_recorder()
        .map_err(|e| Error::Other(format!("cannot install metrics recorder: {e}")))?;
    let limiter = quota_for(opts.limit_rps).map(|q| {
        std::sync::Arc::new(governor::RateLimiter::direct(q))
    });
    // 层（后调用者居外，即外→内：CORS → 压缩 → 观测 → 追踪 → 路由）。
    // CORS 取 permissive（本地静态 dev 服务；上线反代后由网关收紧，文档记录）。
    // 追踪回调手写 target（默认回调打 `tower_http::trace`，会被默认 filter
    // `winterjs=<level>` 静默，见 §4.19）。
    let trace = TraceLayer::new_for_http()
        .on_request(|req: &http::Request<axum::body::Body>, _span: &tracing::Span| {
            tracing::debug!(
                target: "winterjs::serve",
                method = %req.method(),
                uri = %req.uri(),
                "request"
            );
        })
        .on_response(
            |res: &http::Response<axum::body::Body>, latency: std::time::Duration, _span: &tracing::Span| {
                tracing::debug!(
                    target: "winterjs::serve",
                    status = res.status().as_u16(),
                    latency_ms = latency.as_millis() as u64,
                    "response"
                );
            },
        )
        .on_failure(
            |err: tower_http::classify::ServerErrorsFailureClass, latency: std::time::Duration, _span: &tracing::Span| {
                tracing::warn!(
                    target: "winterjs::serve",
                    %err,
                    latency_ms = latency.as_millis() as u64,
                    "request failed"
                );
            },
        );
    let app = Router::new()
        .route("/metrics", axum::routing::get(metrics_handler))
        .fallback_service(ServeDir::new(root))
        .layer(axum::middleware::from_fn_with_state(limiter, observe))
        .with_state(metrics)
        .layer(trace)
        .layer(CompressionLayer::new())
        .layer(CorsLayer::permissive());
    // `axum::serve` 返回类型随 Listener 而异，两分支各自收尾（逻辑同构）。
    if let Some(cfg) = tls {
        let acceptor = tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(cfg));
        axum::serve(TlsListener { tcp, acceptor }, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .map_err(|e| Error::Other(format!("serve failed: {e}")))?;
    } else {
        axum::serve(PlainListener { tcp }, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .map_err(|e| Error::Other(format!("serve failed: {e}")))?;
    }
    tracing::info!(target: "winterjs::serve", "stopped");
    Ok(())
}

/// 明文 listener（与 `TlsListener` 同构，使 serve 尾部类型统一）。
struct PlainListener {
    tcp: tokio::net::TcpListener,
}

impl axum::serve::Listener for PlainListener {
    type Io = tokio::net::TcpStream;
    type Addr = std::net::SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            match self.tcp.accept().await {
                Ok(t) => return t,
                Err(e) => {
                    tracing::warn!(target: "winterjs::serve", "accept failed: {e}");
                }
            }
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.tcp.local_addr()
    }
}

/// 共享限流器（`None` = 不限流；`governor` 直接式，全局统一配额）。
type SharedLimiter = Option<std::sync::Arc<governor::DefaultDirectRateLimiter>>;

/// 观测中间件：限流（429）→ in-flight gauge → 计数/耗时指标。
/// 指标无全局 recorder 时为 no-op（单测/嵌入场景安全）。
async fn observe(
    axum::extract::State(limiter): axum::extract::State<SharedLimiter>,
    req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if let Some(lim) = &limiter {
        if let Err(wait) = lim.check() {
            use governor::clock::Clock as _;
            let secs = retry_after_secs(wait.wait_time_from(lim.clock().now()));
            tracing::debug!(target: "winterjs::serve", "rate limited");
            return axum::response::Response::builder()
                .status(axum::http::StatusCode::TOO_MANY_REQUESTS)
                .header("retry-after", secs.to_string())
                .body(axum::body::Body::from("rate limited\n"))
                .expect("static 429 builds");
        }
    }
    let method = req.method().to_string();
    let path = req.uri().path().to_owned();
    let gauge = metrics::gauge!(METRIC_IN_FLIGHT);
    gauge.increment(1.0);
    let start = std::time::Instant::now();
    let res = next.run(req).await;
    gauge.decrement(1.0);
    let status = res.status().as_u16().to_string();
    metrics::counter!(METRIC_REQUESTS, "method" => method.clone(), "path" => path.clone(), "status" => status).increment(1);
    metrics::histogram!(METRIC_DURATION, "method" => method, "path" => path)
        .record(start.elapsed().as_secs_f64());
    res
}

/// `/metrics`（Prometheus 文本；`docs/metrics.md` 有 named 指标文档）。
async fn metrics_handler(
    axum::extract::State(handle): axum::extract::State<metrics_exporter_prometheus::PrometheusHandle>,
) -> ([(&'static str, &'static str); 1], String) {
    ([("content-type", "text/plain; version=0.0.4")], handle.render())
}

/// SIGINT（Ctrl-C）或 SIGTERM（unix）到即返回；注册失败则只等 Ctrl-C。
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = ctrl_c => {},
                    _ = term.recv() => {},
                }
            }
            Err(e) => {
                tracing::warn!(target: "winterjs::serve", "SIGTERM handler unavailable: {e}");
                ctrl_c.await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        ctrl_c.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_line_shape() {
        let addr: SocketAddr = "127.0.0.1:3000".parse().unwrap();
        assert_eq!(
            ready_line_scheme(Path::new("/tmp/site"), "http", &addr),
            "serving /tmp/site on http://127.0.0.1:3000"
        );
        assert_eq!(
            ready_line_scheme(Path::new("/tmp/site"), "https", &addr),
            "serving /tmp/site on https://127.0.0.1:3000"
        );
    }

    #[test]
    fn validate_dir_table() {
        let dir = tempfile::tempdir().unwrap();
        assert!(validate_dir(dir.path()).is_ok());
        // 不存在 → 错；文件 → 错（边界两件）。
        assert!(validate_dir(&dir.path().join("nope")).is_err());
        let f = dir.path().join("f.txt");
        std::fs::write(&f, b"x").unwrap();
        let err = validate_dir(&f).unwrap_err().to_string();
        assert!(err.contains("not a directory"), "err: {err}");
    }

    #[test]
    fn lan_addr_shape_or_none() {
        // 本机一定有回环之外的判断不稳定：只断言形状（有则必带端口）。
        if let Some(addr) = lan_addr(1234) {
            assert_eq!(addr.port(), 1234);
        }
    }

    #[test]
    fn tls_rejects_bad_pem() {
        let dir = tempfile::tempdir().unwrap();
        let cert = dir.path().join("c.pem");
        let key = dir.path().join("k.pem");
        std::fs::write(&cert, b"not a pem\n").unwrap();
        std::fs::write(&key, b"not a pem\n").unwrap();
        // 坏 cert / 空 cert / 缺 key 三件（纯 IO，不碰网络）。
        assert!(load_tls(&cert, &key).is_err());
        std::fs::write(&cert, b"").unwrap();
        assert!(load_tls(&cert, &key).unwrap_err().to_string().contains("no certificate"));
        assert!(load_tls(&dir.path().join("missing.pem"), &key).is_err());
    }

    #[test]
    fn quota_and_retry_table() {
        assert!(quota_for(0).is_none());
        assert!(quota_for(10).is_some());
        // u32::MAX 下 `1s / rps` 下溢为 0 → None（不断言 panic，只断言不炸）。
        let _ = quota_for(u32::MAX);
        assert_eq!(retry_after_secs(std::time::Duration::from_millis(0)), 1);
        assert_eq!(retry_after_secs(std::time::Duration::from_millis(1001)), 2);
        assert_eq!(retry_after_secs(std::time::Duration::from_secs(5)), 6);
    }

    #[test]
    fn qr_block_shape() {
        // 纯函数确定性：多行块字符，含深色模块。
        let qr = qr_block("http://192.168.1.5:3000").expect("qr renders");
        assert!(qr.lines().count() > 5, "qr:\n{qr}");
        assert!(qr.contains('█') || qr.contains('▀') || qr.contains('▄'), "qr:\n{qr}");
    }
}
