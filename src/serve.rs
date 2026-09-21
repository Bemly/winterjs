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
    /// TLS 证书/私钥（PEM；必须同给同缺，见 `load_tls`；与 `acme` 互斥）。
    pub cert: Option<PathBuf>,
    pub key: Option<PathBuf>,
    /// ACME 自动证书（启用时签发/复用后转为内存 TLS；见 `acme`）。
    pub acme: Option<crate::acme::AcmeOpts>,
    /// JS handler 文件（`--handler`；None=纯静态。动态桥接见 plan4 T1）。
    pub handler: Option<PathBuf>,
}

/// TLS 建连（单证书 → `ServerConfig`；ALPN 挂 `h2` + `http/1.1` 供 axum auto 协商 T2）。
fn tls_config_with_single_cert(
    certs: Vec<rustls::pki_types::CertificateDer<'static>>,
    key: rustls::pki_types::PrivateKeyDer<'static>,
) -> Result<rustls::ServerConfig, Error> {
    // provider 与 fetch 侧同源（顶层 ring；重复 install 无害，见 §2 门控）。
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut cfg = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| Error::Other(format!("cannot build TLS config: {e}")))?;
    cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(cfg)
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
    tls_config_with_single_cert(certs, key)
}

/// ACME 证书转内存 TLS（`ensure_cert` 的 PEM 对 → `ServerConfig`；与 `load_tls` 同 provider）。
async fn load_tls_acme(acme: &crate::acme::AcmeOpts) -> Result<rustls::ServerConfig, Error> {
    use std::io::BufReader;
    let (cert_pem, key_pem) = crate::acme::ensure_cert(acme).await?;
    let certs = rustls_pemfile::certs(&mut BufReader::new(cert_pem.as_slice()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| Error::Other(format!("bad ACME cert PEM: {e}")))?;
    let key = rustls_pemfile::private_key(&mut BufReader::new(key_pem.as_slice()))
        .map_err(|e| Error::Other(format!("bad ACME key PEM: {e}")))?
        .ok_or_else(|| Error::Other("bad ACME key PEM: no private key found".into()))?;
    tls_config_with_single_cert(certs, key)
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
/// sd_notify 协议直写（`$NOTIFY_SOCKET` 数据报，含 `@` 抽象套接字），替代
/// `systemd` crate —— 后者的 pkg-config + C 链接挡死交叉编译矩阵，协议本身
/// 约 30 行（§13 手写件，2026-09-11 记偏离，dependencies §8 同步）。
#[cfg(target_os = "linux")]
pub(crate) fn notify_ready() {
    use std::os::linux::net::SocketAddrExt as _;
    use std::os::unix::net::{SocketAddr, UnixDatagram};
    let Some(raw) = std::env::var_os("NOTIFY_SOCKET") else {
        tracing::debug!(target: "winterjs::serve", "not running under systemd (no NOTIFY_SOCKET)");
        return;
    };
    let raw = raw.to_string_lossy().into_owned();
    let addr = if let Some(name) = raw.strip_prefix('@') {
        // 抽象命名空间套接字（systemd 默认形态）
        match SocketAddr::from_abstract_name(name.as_bytes()) {
            Ok(a) => a,
            Err(e) => {
                tracing::debug!(target: "winterjs::serve", "systemd notify: bad abstract socket: {e}");
                return;
            }
        }
    } else {
        match SocketAddr::from_pathname(std::path::Path::new(&raw)) {
            Ok(a) => a,
            Err(e) => {
                tracing::debug!(target: "winterjs::serve", "systemd notify: bad socket path: {e}");
                return;
            }
        }
    };
    match UnixDatagram::unbound().and_then(|sock| sock.send_to_addr(b"READY=1", &addr)) {
        Ok(_) => tracing::debug!(target: "winterjs::serve", "systemd READY notified"),
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

/// TS/JSX 家族 → JS MIME 判定（Vite 对等；`mime_guess` 把 `.ts`/`.mts`
/// 当 MPEG-TS 视频流，且 `tower-http 0.7` 无覆盖接口，见 §4.30）。
/// 命中返回 `text/javascript`，其余（无扩展/尾点/后缀非末尾）返回 `None`。
pub fn ts_family_js_mime(path: &str) -> Option<&'static str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    if !name.contains('.') || name.ends_with('.') {
        return None;
    }
    match name.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "ts" | "mts" | "cts" | "tsx" | "jsx" => Some("text/javascript"),
        _ => None,
    }
}

/// TS 家族 MIME 重写（`ServeDir` 之后最内层；只改成功响应，404 等不动）。
async fn rewrite_ts_mime(
    req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let want = ts_family_js_mime(req.uri().path()).is_some();
    let mut res = next.run(req).await;
    if want && res.status().is_success() {
        res.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("text/javascript"),
        );
    }
    res
}

/// 启动并跑到信号到来。调用方（main）已在 tokio runtime 内。
pub async fn serve(opts: &ServeOpts) -> Result<(), Error> {
    let root = validate_dir(&opts.dir)?;
    // `--handler` 缺文件即启动期可读错（plan4 §3 T1；动态桥接落子后此处转交接）。
    if let Some(h) = &opts.handler {
        if !h.is_file() {
            return Err(Error::Other(format!(
                "cannot read --handler '{}': no such file",
                h.display()
            )));
        }
    }
    // `--cert/--key` 必须同给同缺（单给即报，不静默降级为明文）；ACME 与之互斥
    //（dispatch 已拦，此处双保险）；ACME 命中即内存 TLS（无文件落地，只有缓存）。
    let tls = match (&opts.cert, &opts.key, &opts.acme) {
        (Some(c), Some(k), None) => Some(load_tls(c, k)?),
        (None, None, None) => None,
        (None, None, Some(acme)) => Some(load_tls_acme(acme).await?),
        _ => {
            return Err(Error::Other(
                "--cert and --key must be given together (PEM files), and not with --acme-*".into(),
            ));
        }
    };
    let scheme: &'static str = if tls.is_some() { "https" } else { "http" };
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
    // JS 会话线程（`--handler` 有值才起；16MB 栈 §4.24；与 axum 多线程经通道互通 §6）。
    // 起服 rendezvous：handler 就绪才起 axum，早失败即启动期错（不静默 503）。
    let js_session: Option<std::thread::JoinHandle<()>> = match &opts.handler {
        Some(path) => {
            let (serve_tx, serve_rx) =
                tokio::sync::mpsc::unbounded_channel::<crate::serve_bridge::ServeEvent>();
            let (start_tx, start_rx) = std::sync::mpsc::channel::<Result<(), String>>();
            let handler = path.clone();
            let thread = std::thread::Builder::new()
                .name("winterjs-serve-js".into())
                .stack_size(16 * 1024 * 1024)
                .spawn(move || {
                    let tokio_rt = match tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                    {
                        Ok(rt) => rt,
                        Err(e) => {
                            let _ = start_tx.send(Err(format!("cannot start async runtime: {e}")));
                            return;
                        }
                    };
                    let mut serve_rx = serve_rx;
                    let outcome = tokio::task::LocalSet::new().block_on(&tokio_rt, async {
                        crate::runtime::run_serve_session(handler, &mut serve_rx, start_tx).await
                    });
                    // Runtime 照 §4.8 在 end_session 泄漏；线程退出即清 CONTEXT/state TLS。
                    if let Err(e) = outcome {
                        tracing::warn!(target: "winterjs::serve", error = %e, "serve JS session ended with error");
                    }
                })
                .map_err(|e| Error::Other(format!("cannot spawn serve JS thread: {e}")))?;
            match start_rx.recv_timeout(std::time::Duration::from_secs(30)) {
                Ok(Ok(())) => {}
                Ok(Err(msg)) => return Err(Error::Other(msg)),
                Err(_) => return Err(Error::Other("serve JS session failed to start".into())),
            }
            tracing::info!(target: "winterjs::serve", handler = %path.display(), "handler ready");
            crate::serve_bridge::publish_serve_tx(serve_tx);
            Some(thread)
        }
        None => None,
    };
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
    // 动态 fallback（`--handler`）：静态命中即直接返回，未命中进 JS；
    // 无 handler 即现状纯静态（ServeDir 404）。`/metrics` 路由优先，不受影响。
    // 注意：不用 `not_found_service`（它经 `SetStatus` 恒改写 fallback 状态为 404，
    // tower-http 0.7.1；见 §4.165），`fallback` 保留 JS 状态原样；
    // 另开 `call_fallback_on_method_not_allowed(true)` 使 POST 等非 GET/HEAD
    // 也进 JS（缺省 405 直返，动态 handler 永够不着）。
    let serve_dir = ServeDir::new(root);
    let app = Router::new().route("/metrics", axum::routing::get(metrics_handler));
    let app = match &js_session {
        Some(_) => app.fallback_service(
            serve_dir
                .call_fallback_on_method_not_allowed(true)
                .fallback(tower::service_fn(move |req| js_fallback(scheme, req))),
        ),
        None => app.fallback_service(serve_dir),
    }
        // WS 拦截最内层（升级优先于静态/handler；T4 路由序）。
        .layer(axum::middleware::from_fn_with_state(scheme, ws_upgrade_intercept))
        .layer(axum::middleware::from_fn(rewrite_ts_mime))
        .layer(axum::middleware::from_fn_with_state(limiter, observe))
        .with_state(metrics)
        .layer(trace)
        .layer(CompressionLayer::new())
        .layer(CorsLayer::permissive());
    // `axum::serve` 返回类型随 Listener 而异，两分支各自收尾（逻辑同构）。
    // H3（T3）：TLS 分支同端口 UDP 起 QUIC（`serve_h3`）；无证书则跳过 + warn，
    // H1 照服；UDP 绑定失败同样 warn 跳过（H1/H2 不受影响）。
    let h3: Option<(quinn::Endpoint, tokio::task::JoinHandle<()>)> = match &tls {
        Some(cfg) => {
            let mut qcfg = cfg.clone();
            qcfg.alpn_protocols = vec![b"h3".to_vec()];
            match quinn::crypto::rustls::QuicServerConfig::try_from(qcfg) {
                Ok(q) => {
                    let udp_addr = SocketAddr::new(addr.ip(), addr.port());
                    let qserver = quinn::ServerConfig::with_crypto(std::sync::Arc::new(q));
                    match quinn::Endpoint::server(qserver, udp_addr) {
                        Ok(ep) => {
                            let task = tokio::spawn(serve_h3(app.clone(), ep.clone()));
                            tracing::info!(target: "winterjs::serve", %udp_addr, "h3 listening");
                            Some((ep, task))
                        }
                        Err(e) => {
                            tracing::warn!(target: "winterjs::serve", "H3 UDP bind failed: {e}");
                            None
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(target: "winterjs::serve", "H3 TLS config failed: {e}");
                    None
                }
            }
        }
        None => {
            tracing::warn!(target: "winterjs::serve", "H3 skipped (no --cert/--key)");
            None
        }
    };
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
    // H3 收尾：关 endpoint（accept 循环即退）再合任务；在飞 QUIC 连接随关收尾。
    if let Some((ep, task)) = h3 {
        ep.close(0u32.into(), b"shutdown");
        if task.await.is_err() {
            tracing::warn!(target: "winterjs::serve", "H3 task panicked");
        }
    }
    if let Some(thread) = js_session {
        // 优雅：停机旗后在飞请求排空线程自退；10s 未退即 warn（随进程退出回收）。
        // Wake 打断空闲 park（否则无事件到来时停机旗 10s 才收敛，见 §4.166）。
        crate::serve_bridge::set_serve_shutdown();
        if let Some(tx) = crate::serve_bridge::serve_tx_global() {
            let _ = tx.send(crate::serve_bridge::ServeEvent::Wake);
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !thread.is_finished() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if thread.is_finished() {
            if thread.join().is_err() {
                tracing::warn!(target: "winterjs::serve", "serve JS thread panicked");
            }
        } else {
            tracing::warn!(target: "winterjs::serve", open = crate::state::serve_open(), "serve JS session did not drain in time");
        }
        crate::serve_bridge::unpublish_serve_tx();
    }
    tracing::info!(target: "winterjs::serve", "stopped");
    Ok(())
}

/// 请求体流式前传（chunk 到即投，完即 End，错即 Fail；
/// 投递失败 = 会话已走即停；js_fallback 与 WS offer 共用）。
fn pump_req_body(
    tx: tokio::sync::mpsc::UnboundedSender<crate::serve_bridge::ServeEvent>,
    id: u64,
    body: axum::body::Body,
) {
    tokio::spawn(async move {
        use futures::StreamExt as _;
        let mut stream = http_body_util::BodyExt::into_data_stream(body);
        loop {
            match stream.next().await {
                Some(Ok(bytes)) => {
                    if bytes.is_empty() {
                        continue;
                    }
                    if tx
                        .send(crate::serve_bridge::ServeEvent::Chunk(id, bytes.to_vec()))
                        .is_err()
                    {
                        break;
                    }
                }
                Some(Err(e)) => {
                    let _ = tx.send(crate::serve_bridge::ServeEvent::Fail(
                        id,
                        format!("request body error: {e}"),
                    ));
                    break;
                }
                None => {
                    let _ = tx.send(crate::serve_bridge::ServeEvent::End(id));
                    break;
                }
            }
        }
    });
}

/// 动态 fallback（`--handler`，plan4 T1/T2）：axum 请求 → JS 会话 → WinterCG 响应。
/// 传输全委托（hyper 成帧；请求体流由 `http_body_util` 泵入通道；响应体流式写回）。
/// 无会话（早失败已拦，此处仅防御）/投递失败即 503；响应头 30s 未到即 504。
async fn js_fallback(
    scheme: &str,
    req: axum::http::Request<axum::body::Body>,
) -> Result<axum::response::Response<axum::body::Body>, std::convert::Infallible> {
    use axum::response::Response;
    fn empty(status: axum::http::StatusCode) -> Response<axum::body::Body> {
        Response::builder().status(status).body(axum::body::Body::empty()).unwrap_or_else(
            |_| {
                Response::builder()
                    .status(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
                    .body(axum::body::Body::empty())
                    .expect("static 500 builds")
            },
        )
    }
    let Some(tx) = crate::serve_bridge::serve_tx_global() else {
        tracing::warn!(target: "winterjs::serve", "serve session not ready");
        return Ok(empty(axum::http::StatusCode::SERVICE_UNAVAILABLE));
    };
    // scheme 随 TLS 分支（T2）：明文 http、TLS https，handler 侧 `req.url` 口径。
    let (parts, body) = req.into_parts();
    // H3 的 :authority 未必落 Host 头，uri.host 兜底（H1/H2 行为不变，T3）。
    let host = parts
        .headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .or_else(|| parts.uri.host().map(str::to_owned))
        .unwrap_or_else(|| "localhost".to_owned());
    let target = parts.uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    let url = format!("{scheme}://{host}{target}");
    let headers: Vec<(String, String)> = parts
        .headers
        .iter()
        .map(|(k, v)| (k.as_str().to_owned(), v.to_str().unwrap_or("").to_owned()))
        .collect();
    let id = crate::serve_bridge::next_serve_id();
    let (head_tx, head_rx) = tokio::sync::oneshot::channel();
    let (body_tx, body_rx) = tokio::sync::mpsc::unbounded_channel();
    let resp = crate::serve_bridge::ServeRespTx { head_tx: Some(head_tx), body_tx, upgrade_tx: None };
    if tx
        .send(crate::serve_bridge::ServeEvent::Head {
            head: crate::serve_bridge::ServeReqHead {
                id,
                method: parts.method.to_string(),
                url,
                headers,
                upgrade: false,
            },
            resp,
        })
        .is_err()
    {
        return Ok(empty(axum::http::StatusCode::SERVICE_UNAVAILABLE));
    }
    pump_req_body(tx, id, body);
    // 响应头 30s 未到即 504（handler 挂起不连累连接空转，网络最佳实践）。
    let head = match tokio::time::timeout(std::time::Duration::from_secs(30), head_rx).await {
        Ok(Ok(h)) => h,
        _ => {
            tracing::warn!(target: "winterjs::serve", id, "serve response head timeout");
            return Ok(empty(axum::http::StatusCode::GATEWAY_TIMEOUT));
        }
    };
    let mut builder = Response::builder().status(head.status);
    for (k, v) in &head.headers {
        match (
            k.parse::<axum::http::HeaderName>(),
            v.parse::<axum::http::HeaderValue>(),
        ) {
            (Ok(name), Ok(val)) => {
                builder = builder.header(name, val);
            }
            _ => tracing::warn!(target: "winterjs::serve", id, header = %k, "dropping invalid response header"),
        }
    }
    // 响应体流式写回（Fail 即提前截断记 warn；发送端随 handler 终结，流自收尾）。
    let stream = async_stream::stream! {
        let mut rx = body_rx;
        while let Some(msg) = rx.recv().await {
            match msg {
                crate::serve_bridge::ServeBodyMsg::Chunk(b) => {
                    yield Ok::<_, std::convert::Infallible>(bytes::Bytes::from(b))
                }
                crate::serve_bridge::ServeBodyMsg::End => break,
                crate::serve_bridge::ServeBodyMsg::Fail(e) => {
                    tracing::warn!(target: "winterjs::serve", id, error = %e, "serve response body failed");
                    break;
                }
            }
        }
    };
    Ok(builder
        .body(axum::body::Body::from_stream(stream))
        .unwrap_or_else(|_| empty(axum::http::StatusCode::INTERNAL_SERVER_ERROR)))
}

/// WS upgrade 判定 + 校验（T4）：仅 H1 + `Upgrade: websocket` 为尝试；
/// 尝试而非法（非 GET/缺 key/错版本/Connection 无 upgrade）即 400 文案。
/// 返回 `Ok(Some(key))` = 合法升级；`Ok(None)` = 普通请求；`Err` = 非法升级。
fn ws_attempt(
    req: &axum::http::Request<axum::body::Body>,
) -> Result<Option<String>, &'static str> {
    if req.version() != axum::http::Version::HTTP_11 {
        return Ok(None);
    }
    let is_ws = req
        .headers()
        .get(axum::http::header::UPGRADE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim().eq_ignore_ascii_case("websocket")));
    if !is_ws {
        return Ok(None);
    }
    if req.method() != axum::http::Method::GET {
        return Err("WebSocket upgrade needs GET");
    }
    let conn_upgrade = req
        .headers()
        .get(axum::http::header::CONNECTION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim().eq_ignore_ascii_case("upgrade")));
    if !conn_upgrade {
        return Err("WebSocket upgrade needs Connection: Upgrade");
    }
    let Some(key) = req
        .headers()
        .get(axum::http::header::SEC_WEBSOCKET_KEY)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
    else {
        return Err("WebSocket upgrade needs Sec-WebSocket-Key");
    };
    let version_ok = req
        .headers()
        .get(axum::http::header::SEC_WEBSOCKET_VERSION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim() == "13");
    if !version_ok {
        return Err("WebSocket upgrade needs Sec-WebSocket-Version: 13");
    }
    Ok(Some(key))
}

/// `Sec-WebSocket-Accept`（RFC 6455 §1.3：base64(sha1(key + GUID))）。纯函数，单测覆盖。
pub fn ws_accept_key(key: &str) -> String {
    use base64::Engine as _;
    use sha1::Digest as _;
    let mut h = sha1::Sha1::new();
    h.update(key.as_bytes());
    h.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    base64::engine::general_purpose::STANDARD.encode(h.finalize())
}

/// 服务端 WS 会话（`tokio-tungstenite` 直通；读写映射与 ws.rs `run_socket` 同构）。
/// Opened 先行（JS onopen）；终结经 dispatch 清表计数；握手失败走 Failed。
async fn run_server_socket(
    ws_id: u64,
    ev_tx: tokio::sync::mpsc::UnboundedSender<crate::builtins::ws::WsEvent>,
    mut out_rx: tokio::sync::mpsc::UnboundedReceiver<crate::builtins::ws::WsOut>,
    on_up: hyper::upgrade::OnUpgrade,
) {
    use crate::builtins::ws::{WsEvent, WsKind, WsOut};
    use futures::{SinkExt as _, StreamExt as _};
    let up = match on_up.await {
        Ok(u) => u,
        Err(e) => {
            let _ = ev_tx.send(WsEvent { id: ws_id, kind: WsKind::Failed(format!("websocket error: {e}")) });
            return;
        }
    };
    // 注意：hyper 已做完 HTTP 握手，此处必须 `from_raw_socket` 直接接管；
    // `accept_async` 会重读一次握手（等一个永不到的 HTTP 请求）而永挂。
    // `from_raw_socket` 为 infallible（async 仅为构造，无握手失败面）。
    let stream = tokio_tungstenite::WebSocketStream::from_raw_socket(
        hyper_util::rt::TokioIo::new(up),
        tokio_tungstenite::tungstenite::protocol::Role::Server,
        None,
    )
    .await;
    let _ = ev_tx.send(WsEvent { id: ws_id, kind: WsKind::Opened { protocol: String::new() } });
    tracing::debug!(target: "winterjs::serve", ws_id, "WS opened");
    let (mut sink, mut stream) = stream.split();
    loop {
        tokio::select! {
            msg = stream.next() => {
                match msg {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Text(t))) => {
                        tracing::trace!(target: "winterjs::serve", ws_id, bytes = t.len(), "WS text in");
                        let _ = ev_tx.send(WsEvent { id: ws_id, kind: WsKind::Text(t.to_string()) });
                    }
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(b))) => {
                        tracing::trace!(target: "winterjs::serve", ws_id, bytes = b.len(), "WS bin in");
                        let _ = ev_tx.send(WsEvent { id: ws_id, kind: WsKind::Bin(b.to_vec()) });
                    }
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Close(frame))) => {
                        let (code, reason) = frame
                            .as_ref()
                            .map(|f| (f.code.into(), f.reason.to_string()))
                            .unwrap_or((1005, String::new()));
                        // tungstenite 已在内部排队回 Close（自动应答），此处 flush 推出再结算；
                        // 手动再发会被状态机拒绝（ClosedByPeer）；见 tungstenite protocol/mod.rs。
                        // 排空策略与 tests/ws.rs stub 同族（回帧再收尾，避免 RST 竞态）。
                        let flushed = sink.flush().await.is_ok();
                        tracing::debug!(target: "winterjs::serve", ws_id, code, flushed, "WS peer close");
                        let _ = ev_tx.send(WsEvent { id: ws_id, kind: WsKind::Closed { code, reason, clean: true } });
                        break;
                    }
                    Some(Ok(_)) => {} // Ping/Pong/Frame：tungstenite 自动回 pong
                    Some(Err(e)) => {
                        let _ = ev_tx.send(WsEvent { id: ws_id, kind: WsKind::Closed { code: 1006, reason: e.to_string(), clean: false } });
                        break;
                    }
                    None => {
                        let _ = ev_tx.send(WsEvent { id: ws_id, kind: WsKind::Closed { code: 1006, reason: String::new(), clean: false } });
                        break;
                    }
                }
            }
            out = out_rx.recv() => {
                match out {
                    Some(WsOut::Text(t)) => {
                        if sink.send(tokio_tungstenite::tungstenite::Message::Text(t.into())).await.is_err() {
                            let _ = ev_tx.send(WsEvent { id: ws_id, kind: WsKind::Closed { code: 1006, reason: String::new(), clean: false } });
                            break;
                        }
                    }
                    Some(WsOut::Bin(b)) => {
                        if sink.send(tokio_tungstenite::tungstenite::Message::Binary(b.into())).await.is_err() {
                            let _ = ev_tx.send(WsEvent { id: ws_id, kind: WsKind::Closed { code: 1006, reason: String::new(), clean: false } });
                            break;
                        }
                    }
                    Some(WsOut::Close { code, reason }) => {
                        let frame = (code != 1005).then(|| tokio_tungstenite::tungstenite::protocol::frame::CloseFrame {
                            code: code.into(),
                            reason: reason.clone().into(),
                        });
                        let _ = sink.send(tokio_tungstenite::tungstenite::Message::Close(frame)).await;
                        // 等对端回 close，上限 5s（对端已关则直接结算）
                        match tokio::time::timeout(std::time::Duration::from_secs(5), stream.next()).await {
                            Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Close(frame)))) => {
                                let (code, reason) = frame
                                    .map(|f| (f.code.into(), f.reason.to_string()))
                                    .unwrap_or((code, reason));
                                let _ = ev_tx.send(WsEvent { id: ws_id, kind: WsKind::Closed { code, reason, clean: true } });
                            }
                            _ => {
                                let _ = ev_tx.send(WsEvent { id: ws_id, kind: WsKind::Closed { code, reason, clean: false } });
                            }
                        }
                        break;
                    }
                    None => break,
                }
            }
        }
    }
}

/// WS 拦截中间件（最内层，Router 之前）：升级尝试优先于静态/handler（T4 路由序）；
/// 非法即 400；合法交 JS 决策（101 接管 / Decline 走普通管线）。
/// Offer 只承载空体（非 GET 已 400）：Decline 重建空体重入管线，体泵照常先行。
async fn ws_upgrade_intercept(
    axum::extract::State(scheme): axum::extract::State<&'static str>,
    mut req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response<axum::body::Body> {
    fn bad(msg: &str) -> axum::response::Response<axum::body::Body> {
        axum::response::Response::builder()
            .status(axum::http::StatusCode::BAD_REQUEST)
            .body(axum::body::Body::from(msg.to_owned()))
            .unwrap_or_else(|_| {
                axum::response::Response::builder()
                    .status(axum::http::StatusCode::BAD_REQUEST)
                    .body(axum::body::Body::empty())
                    .expect("static 400 builds")
            })
    }
    fn down() -> axum::response::Response<axum::body::Body> {
        axum::response::Response::builder()
            .status(axum::http::StatusCode::SERVICE_UNAVAILABLE)
            .body(axum::body::Body::empty())
            .expect("static 503 builds")
    }
    let key = match ws_attempt(&req) {
        Ok(None) => return next.run(req).await,
        Ok(Some(k)) => k,
        Err(msg) => return bad(msg),
    };
    // 先占 upgrade future（101 后用），再拆头泵体交 JS 决策。
    let on_up = hyper::upgrade::on(&mut req);
    let (parts, body) = req.into_parts();
    let host = parts
        .headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .or_else(|| parts.uri.host().map(str::to_owned))
        .unwrap_or_else(|| "localhost".to_owned());
    let target = parts.uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    let url = format!("{scheme}://{host}{target}");
    let headers: Vec<(String, String)> = parts
        .headers
        .iter()
        .map(|(k, v)| (k.as_str().to_owned(), v.to_str().unwrap_or("").to_owned()))
        .collect();
    let id = crate::serve_bridge::next_serve_id();
    let Some(tx) = crate::serve_bridge::serve_tx_global() else {
        return down();
    };
    let (dec_tx, dec_rx) = tokio::sync::oneshot::channel();
    // 决策期无头/体泵（Decline 走普通管线重建）；体泵照常先行供 offer 期读体。
    let (body_tx, _body_rx) = tokio::sync::mpsc::unbounded_channel();
    let resp = crate::serve_bridge::ServeRespTx { head_tx: None, body_tx, upgrade_tx: Some(dec_tx) };
    if tx
        .send(crate::serve_bridge::ServeEvent::Head {
            head: crate::serve_bridge::ServeReqHead {
                id,
                method: parts.method.to_string(),
                url,
                headers,
                upgrade: true,
            },
            resp,
        })
        .is_err()
    {
        return down();
    }
    pump_req_body(tx, id, body);
    // 决策 30s 未到即 Decline（handler 挂起不连累升级空转）。
    let decision = tokio::time::timeout(std::time::Duration::from_secs(30), dec_rx).await;
    let crate::serve_bridge::ServeUpgrade::Accept { ws_id, ev_tx, out_rx } = (match decision {
        Ok(Ok(d)) => d,
        _ => crate::serve_bridge::ServeUpgrade::Decline,
    }) else {
        // Decline/超时/会话走：重建空体重入普通管线（静态 → handler）。
        let req = axum::http::Request::from_parts(parts, axum::body::Body::empty());
        return next.run(req).await;
    };
    let accept = ws_accept_key(&key);
    tokio::spawn(run_server_socket(ws_id, ev_tx, out_rx, on_up));
    axum::response::Response::builder()
        .status(axum::http::StatusCode::SWITCHING_PROTOCOLS)
        .header(axum::http::header::UPGRADE, "websocket")
        .header(axum::http::header::CONNECTION, "Upgrade")
        .header(axum::http::header::SEC_WEBSOCKET_ACCEPT, accept)
        .body(axum::body::Body::empty())
        .unwrap_or_else(|_| down())
}

/// H3（plan4 T3）：同一 Router 经 QUIC/UDP 同端口服务（`h3-axum` example 形态）。
/// 调用方保证 endpoint 存活；返回即 accept 循环结束（endpoint.close 后）。
/// 优雅关闭由调用方 `endpoint.close()` 驱动，在飞连接随 QUIC 关闭而收尾。
async fn serve_h3(app: axum::Router, endpoint: quinn::Endpoint) {
    while let Some(incoming) = endpoint.accept().await {
        let app = app.clone();
        tokio::spawn(async move {
            let conn = match incoming.await {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(target: "winterjs::serve", "QUIC handshake failed: {e}");
                    return;
                }
            };
            let peer = conn.remote_address();
            tracing::debug!(target: "winterjs::serve", %peer, "H3 QUIC connection");
            let h3_conn = match h3::server::builder().build(h3_quinn::Connection::new(conn)).await
            {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(target: "winterjs::serve", %peer, "H3 handshake failed: {e}");
                    return;
                }
            };
            tracing::debug!(target: "winterjs::serve", %peer, "H3 connection established");
            tokio::pin!(h3_conn);
            loop {
                match h3_conn.accept().await {
                    Ok(Some(resolver)) => {
                        tracing::debug!(target: "winterjs::serve", %peer, "H3 request accepted");
                        let app = app.clone();
                        tokio::spawn(async move {
                            if let Err(e) = h3_axum::serve_h3_with_axum(app, resolver).await {
                                tracing::warn!(target: "winterjs::serve", %peer, "H3 request failed: {e}");
                            }
                        });
                    }
                    Ok(None) => break,
                    Err(e) => {
                        if h3_axum::is_graceful_h3_close(&e) {
                            tracing::debug!(target: "winterjs::serve", %peer, "H3 closed gracefully");
                        } else {
                            tracing::warn!(target: "winterjs::serve", %peer, "H3 connection error: {e:?}");
                        }
                        break;
                    }
                }
            }
        });
    }
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
pub(crate) async fn shutdown_signal() {
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
    // sd_notify 属 linux 运行面（mac 无 std::os::linux），本测试仅在 linux 上编译运行
    #[test]
    #[cfg(target_os = "linux")]
    #[serial_test::serial]
    fn sd_notify_ready_sends_to_abstract_socket() {
        // 抽象套接字收 READY=1（@ 前缀路径与 systemd 默认形态一致）
        use std::os::linux::net::SocketAddrExt as _;
        use std::os::unix::net::UnixDatagram;
        let name = format!("winterjs-test-{}", std::process::id());
        let rx = UnixDatagram::bind_addr(&std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes()).unwrap()).unwrap();
        // SAFETY: #[serial] 防并行；进程级 env 仅本测试读写
        unsafe { std::env::set_var("NOTIFY_SOCKET", format!("@{name}")) };
        super::notify_ready();
        // SAFETY: 同上
        unsafe { std::env::remove_var("NOTIFY_SOCKET") };
        let mut buf = [0u8; 16];
        let (n, _) = rx.recv_from(&mut buf).expect("READY datagram");
        assert_eq!(&buf[..n], b"READY=1");
    }

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
    fn tls_config_carries_h2_alpn() {
        // T2：服务端 ALPN 挂 h2 + http/1.1，axum auto 按 ALPN 协商 H2（curl https 默认谈出 v=2）。
        let key = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()]).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let cert_path = dir.path().join("c.pem");
        let key_path = dir.path().join("k.pem");
        std::fs::write(&cert_path, key.cert.pem()).unwrap();
        std::fs::write(&key_path, key.signing_key.serialize_pem()).unwrap();
        let cfg = load_tls(&cert_path, &key_path).unwrap();
        assert_eq!(cfg.alpn_protocols, vec![b"h2".to_vec(), b"http/1.1".to_vec()]);
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
    fn ts_family_mime_table() {
        // 正常：TS 家族全命中（含大写，Vite 对等）。
        for p in ["/src/main.ts", "/a/b.TS", "/x.mts", "/x.cts", "/x.tsx", "/x.jsx"] {
            assert_eq!(ts_family_js_mime(p), Some("text/javascript"), "{p}");
        }
        // 边界：普通文件/无扩展/尾点/后缀非末尾/根与指标路径一律不碰。
        for p in ["/app.js", "/index.html", "/noext", "/a.", "/main.ts.bak", "/metrics", "/"] {
            assert_eq!(ts_family_js_mime(p), None, "{p}");
        }
    }

    #[test]
    fn qr_block_shape() {
        // 纯函数确定性：多行块字符，含深色模块。
        let qr = qr_block("http://192.168.1.5:3000").expect("qr renders");
        assert!(qr.lines().count() > 5, "qr:\n{qr}");
        assert!(qr.contains('█') || qr.contains('▀') || qr.contains('▄'), "qr:\n{qr}");
    }

    #[test]
    fn ws_accept_key_rfc_vector() {
        // RFC 6455 §1.3 标准向量：key `dGhlIHNhbXBsZSBub25jZQ==` → `s3pPLMBiTxaQ9kYGzzhZRbK+xOo=`。
        assert_eq!(
            ws_accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }
}
