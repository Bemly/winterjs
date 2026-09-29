//! ACME 自动证书（plan Phase 6-d4 顺延收官；`instant-acme` 轮子 + `rcgen` CSR）。
//!
//! - 入口：`serve --acme-domain <d> --acme-email <e>`（domain 缺省
//!   `winterjs.bemly.moe`，staging 默认开，生产需显式 `--acme-production`）。
//! - 流程：缓存有效（notAfter > now+30d）即复用 → 否则账户（缓存，无则新建）→
//!   订单 → HTTP-01（临时 `:80` 应答 `/.well-known/acme-challenge/<token>`）→
//!   poll → CSR（rcgen）→ finalize → 存缓存（cert.pem/key.pem/account.json）。
//! - 约束（文档记录）：HTTP-01 要求公网 `:80` 可达 + DNS 指到本机；`:80` 绑不上
//!   即报可读错（unix 需 root/setcap）；当前 `winterjs.bemly.moe` 解析到
//!   `198.18.14.204`（benchmark 段，非公网可达）——真签发需先把 DNS 指到服务器。
//! - 测试：无网络部分全单测（缓存/有效期/挑战路径/目录选择）；网络路径经
//!   `--dry-run` 只校验打印（黑盒钉住），真签发靠 staging 手工验证。

use std::path::{Path, PathBuf};

use crate::error::Error;

/// 缺省域名（用户指定，见头注 DNS 约束）。
pub const DEFAULT_DOMAIN: &str = "winterjs.bemly.moe";
/// 提前续期窗（30 天；LE 证书 90 天有效）。
pub const RENEW_BEFORE_SECS: u64 = 30 * 24 * 3600;
/// HTTP-01 挑战端口（LE 只认 80）。
pub const CHALLENGE_PORT: u16 = 80;

/// ACME 选项（CLI 直传；`domain/email` 为 `None` 表未启用）。
#[derive(Debug, Clone, Default)]
pub struct AcmeOpts {
    pub domain: Option<String>,
    pub email: Option<String>,
    pub cache_dir: Option<PathBuf>,
    /// 生产环境（默认 staging；显式才走生产，防误触限流）。
    pub production: bool,
}

impl AcmeOpts {
    /// 是否启用（domain 或 email 任一出现即启用；domain 缺省官方域）。
    pub fn enabled(&self) -> bool {
        self.domain.is_some() || self.email.is_some()
    }

    /// 生效域名（显式 > 缺省）。
    pub fn effective_domain(&self) -> &str {
        self.domain.as_deref().unwrap_or(DEFAULT_DOMAIN)
    }

    /// 目录 URL（staging 默认防误触生产限流；production 显式才走生产）。
    pub fn directory_url(&self) -> &'static str {
        if self.production {
            instant_acme::LetsEncrypt::Production.url()
        } else {
            instant_acme::LetsEncrypt::Staging.url()
        }
    }
}

/// 缓存根：显式 `--acme-cache` > `$WINTERJS2_ACME_CACHE` > 系统缓存
///（`dirs::cache_dir/winterjs2/acme`；与 pkgs 缓存同源）。
pub fn cache_root(explicit: Option<&Path>) -> Result<PathBuf, Error> {
    if let Some(p) = explicit {
        return Ok(p.to_path_buf());
    }
    if let Ok(v) = std::env::var("WINTERJS2_ACME_CACHE")
        && !v.trim().is_empty()
    {
        return Ok(PathBuf::from(v));
    }
    let base = dirs::cache_dir().ok_or_else(|| Error::Other("cannot find cache directory".into()))?;
    Ok(base.join("winterjs2").join("acme"))
}

/// 域缓存三件（cert.pem/key.pem/account.json）。
pub fn cache_paths(root: &Path, domain: &str) -> (PathBuf, PathBuf, PathBuf) {
    let d = root.join(domain);
    (d.join("cert.pem"), d.join("key.pem"), d.join("account.json"))
}

/// 挑战应答路径（`/.well-known/acme-challenge/<token>`）。
pub fn challenge_path(token: &str) -> String {
    format!("/.well-known/acme-challenge/{token}")
}

/// 缓存证书是否有效（cert 首块 notAfter > now+30d，且 key 可解析；任一失败即 false）。
/// 纯逻辑（除 fs 外），单测覆盖（rcgen 自签不同有效期）。
pub fn cached_cert_valid(cert_pem: &[u8], key_pem: &[u8]) -> bool {
    use std::io::BufReader;
    let Ok(certs) = rustls_pemfile::certs(&mut BufReader::new(cert_pem)).collect::<Result<Vec<_>, _>>() else {
        return false;
    };
    let Some(first) = certs.first() else {
        return false;
    };
    if rustls_pemfile::private_key(&mut BufReader::new(key_pem)).is_err() {
        return false;
    }
    use der::Decode as _;
    let Ok(cert) = x509_cert::Certificate::from_der(first.as_ref()) else {
        return false;
    };
    let not_after = cert.tbs_certificate().validity().not_after.to_system_time();
    not_after
        > std::time::SystemTime::now() + std::time::Duration::from_secs(RENEW_BEFORE_SECS)
}

/// 取证（缓存命中即复用，否则走 ACME 全流程；返回 PEM 字节对）。
/// 调用方（serve）在 tokio runtime 内；challenge 小服务器同 runtime。
pub async fn ensure_cert(opts: &AcmeOpts) -> Result<(Vec<u8>, Vec<u8>), Error> {
    let domain = opts.effective_domain().to_owned();
    if domain.trim().is_empty() || domain.contains('/') || domain.contains(' ') {
        return Err(Error::Other(format!("bad --acme-domain '{domain}'")));
    }
    let root = cache_root(opts.cache_dir.as_deref())?;
    let (cert_path, key_path, account_path) = cache_paths(&root, &domain);
    if let (Ok(cert), Ok(key)) = (std::fs::read(&cert_path), std::fs::read(&key_path)) {
        if cached_cert_valid(&cert, &key) {
            tracing::info!(target: "winterjs2::acme", domain = domain.as_str(), "using cached certificate");
            return Ok((cert, key));
        }
        tracing::info!(target: "winterjs2::acme", domain = domain.as_str(), "cached certificate expired, renewing");
    }
    let email = opts.email.clone().filter(|e| !e.trim().is_empty()).ok_or_else(|| {
        Error::Other("ACME needs --acme-email <addr> for a new account (cached account not found)".into())
    })?;
    if !email.contains('@') {
        return Err(Error::Other(format!("bad --acme-email '{email}'")));
    }
    tracing::info!(target: "winterjs2::acme", domain = domain.as_str(), directory = opts.directory_url(), "requesting certificate");
    let (cert_pem, key_pem) = issue(opts.directory_url(), &domain, &email, &account_path).await?;
    if let Some(parent) = cert_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::Other(format!("cannot create ACME cache: {e}")))?;
    }
    // 原子落盘（tmp+rename；沿 pm cache 同模式，防半写）。
    for (path, bytes) in [(&cert_path, &cert_pem), (&key_path, &key_pem)] {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, bytes)
            .map_err(|e| Error::Other(format!("cannot write {}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, path)
            .map_err(|e| Error::Other(format!("cannot commit {}: {e}", path.display())))?;
    }
    tracing::info!(target: "winterjs2::acme", domain = domain.as_str(), "certificate issued and cached");
    Ok((cert_pem, key_pem))
}

/// `instant-acme` 的 HTTP 客户端（`reqwest` 实现；`hyper-rustls` 特性拖
/// aws-lc，§2 禁用，故手写约 30 行桥接。TLS provider 与全图同源 ring）。
/// 线程模型：无状态 clone 客户端，只经 channel 与 JS 线程通信之外独立跑（§6 合规）。
#[derive(Clone)]
struct ReqwestHttp {
    client: reqwest::Client,
}

impl ReqwestHttp {
    fn new() -> Result<Self, Error> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| Error::Other(format!("ACME http client failed: {e}")))?;
        Ok(Self { client })
    }
}

impl instant_acme::HttpClient for ReqwestHttp {
    fn request(
        &self,
        req: http::Request<instant_acme::BodyWrapper<bytes::Bytes>>,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<instant_acme::BytesResponse, instant_acme::Error>> + Send>>
    {
        use http_body_util::BodyExt as _;
        let client = self.client.clone();
        Box::pin(async move {
            let (parts, body) = req.into_parts();
            let bytes = body
                .collect()
                .await
                .map_err(|e| instant_acme::Error::Other(e.into()))?
                .to_bytes();
            let mut rb = client.request(parts.method, parts.uri.to_string());
            for (k, v) in parts.headers.iter() {
                rb = rb.header(k, v);
            }
            let resp = rb
                .body(bytes)
                .send()
                .await
                .map_err(|e| instant_acme::Error::Other(Box::new(e)))?;
            let status = resp.status();
            let headers = resp.headers().clone();
            let body = resp
                .bytes()
                .await
                .map_err(|e| instant_acme::Error::Other(Box::new(e)))?;
            let mut builder = http::Response::builder().status(status);
            for (k, v) in headers.iter() {
                builder = builder.header(k, v);
            }
            let http_resp = builder
                .body(http_body_util::Full::new(body))
                .map_err(instant_acme::Error::Http)?;
            Ok(instant_acme::BytesResponse::from(http_resp))
        })
    }
}

/// ACME 全流程（账户→订单→HTTP-01→CSR→下证；account.json 复用）。
async fn issue(
    directory_url: &str,
    domain: &str,
    email: &str,
    account_path: &Path,
) -> Result<(Vec<u8>, Vec<u8>), Error> {
    use instant_acme::{ChallengeType, Identifier, NewOrder};
    // 账户（缓存复用，无则新建；contact mailto）。
    let contact = format!("mailto:{email}");
    let http = ReqwestHttp::new()?;
    let account = if let Ok(raw) = std::fs::read(account_path) {
        match serde_json::from_slice::<instant_acme::AccountCredentials>(&raw) {
            Ok(creds) => instant_acme::Account::builder_with_http(Box::new(http.clone()))
                .from_credentials(creds)
                .await
                .map_err(|e| Error::Other(format!("ACME account load failed: {e}")))?,
            Err(_) => {
                let (account, creds) = new_account(directory_url, &contact, &http).await?;
                save_creds(account_path, &creds);
                account
            }
        }
    } else {
        let (account, creds) = new_account(directory_url, &contact, &http).await?;
        save_creds(account_path, &creds);
        account
    };
    // 订单 + HTTP-01 挑战收集。
    let identifiers = [Identifier::Dns(domain.to_string())];
    let mut order = account
        .new_order(&NewOrder::new(&identifiers))
        .await
        .map_err(|e| Error::Other(format!("ACME order failed: {e}")))?;
    let mut challenges: Vec<(String, String)> = Vec::new();
    {
        let mut auths = order.authorizations();
        while let Some(auth) = auths.next().await {
            let mut auth = auth.map_err(|e| Error::Other(format!("ACME authorization failed: {e}")))?;
            let Some(mut chall) = auth.challenge(ChallengeType::Http01) else {
                return Err(Error::Other(format!(
                    "ACME server offers no HTTP-01 challenge for {domain}"
                )));
            };
            challenges.push((chall.token.clone(), chall.key_authorization().as_str().to_owned()));
            chall.set_ready().await.map_err(|e| Error::Other(format!("ACME challenge failed: {e}")))?;
        }
    }
    if challenges.is_empty() {
        return Err(Error::Other(format!("ACME server gave no authorizations for {domain}")));
    }
    // 挑战应答服务器（:80；LE 只认 80，绑不上即报）。
    let app = {
        let mut router = axum::Router::new();
        for (token, key_auth) in &challenges {
            router = router.route(
                &challenge_path(token),
                axum::routing::get(key_auth.clone()),
            );
        }
        router
    };
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", CHALLENGE_PORT))
        .await
        .map_err(|e| {
            Error::Other(format!(
                "cannot bind :{CHALLENGE_PORT} for HTTP-01 (need root/setcap or --cert/--key instead): {e}"
            ))
        })?;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await;
    });
    // 等就绪 → CSR → 下证（停机挑战服务器）。
    let out = async {
        order
            .poll_ready(&instant_acme::RetryPolicy::default())
            .await
            .map_err(|e| Error::Other(format!("ACME challenge not ready: {e}")))?;
        let mut params = rcgen::CertificateParams::new(vec![domain.to_string()])
            .map_err(|e| Error::Other(format!("CSR params failed: {e}")))?;
        params.distinguished_name.push(rcgen::DnType::CommonName, domain);
        let key_pair = rcgen::KeyPair::generate()
            .map_err(|e| Error::Other(format!("key generation failed: {e}")))?;
        let csr = params
            .serialize_request(&key_pair)
            .map_err(|e| Error::Other(format!("CSR failed: {e}")))?;
        order
            .finalize_csr(csr.der())
            .await
            .map_err(|e| Error::Other(format!("ACME finalize failed: {e}")))?;
        let cert_chain = order
            .poll_certificate(&instant_acme::RetryPolicy::default())
            .await
            .map_err(|e| Error::Other(format!("ACME certificate failed: {e}")))?;
        let key_pem = key_pair
            .serialize_pem()
            .as_bytes()
            .to_vec();
        Ok::<_, Error>((cert_chain.into_bytes(), key_pem))
    }
    .await;
    let _ = shutdown_tx.send(());
    let _ = server.await;
    out
}

/// 账户凭证落盘（失败只记 debug，下次重建；best-effort）。
fn save_creds(account_path: &Path, creds: &instant_acme::AccountCredentials) {
    let Ok(raw) = serde_json::to_vec_pretty(&creds) else {
        return;
    };
    if let Some(parent) = account_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = account_path.with_extension("tmp");
    if std::fs::write(&tmp, &raw).is_ok() {
        let _ = std::fs::rename(&tmp, account_path);
    }
}

/// 新账户（contact mailto + ToS 同意；同时返回可落盘的凭证）。
async fn new_account(
    directory_url: &str,
    contact: &str,
    http: &ReqwestHttp,
) -> Result<(instant_acme::Account, instant_acme::AccountCredentials), Error> {
    use instant_acme::{Account, NewAccount};
    let (account, creds) = Account::builder_with_http(Box::new(http.clone()))
        .create(
            &NewAccount { contact: &[contact], terms_of_service_agreed: true, only_return_existing: false },
            directory_url.to_string(),
            None,
        )
        .await
        .map_err(|e| Error::Other(format!("ACME account creation failed: {e}")))?;
    Ok((account, creds))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn self_signed(not_after: (i32, u8, u8)) -> (Vec<u8>, Vec<u8>) {
        // rcgen 公开的日期构造（免为测试引入 time 轮子，见 §0.5）。
        let mut params = rcgen::CertificateParams::new(vec!["example.com".to_string()]).unwrap();
        params.not_before = rcgen::date_time_ymd(2020, 1, 1);
        params.not_after = rcgen::date_time_ymd(not_after.0, not_after.1, not_after.2);
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = params.self_signed(&key).unwrap();
        (cert.pem().into_bytes(), key.serialize_pem().into_bytes())
    }

    #[test]
    fn cache_layout_pinned() {
        let (c, k, a) = cache_paths(Path::new("/x"), "winterjs.bemly.moe");
        assert_eq!(c, PathBuf::from("/x/winterjs.bemly.moe/cert.pem"));
        assert_eq!(k, PathBuf::from("/x/winterjs.bemly.moe/key.pem"));
        assert_eq!(a, PathBuf::from("/x/winterjs.bemly.moe/account.json"));
    }

    #[test]
    fn validity_window() {
        let (c, k) = self_signed((2030, 1, 1));
        assert!(cached_cert_valid(&c, &k));
        // 过期/临期（<30d 窗）一律续期
        let (c, k) = self_signed((2020, 1, 1));
        assert!(!cached_cert_valid(&c, &k), "expired must renew");
        assert!(!cached_cert_valid(b"nope", &k));
        assert!(!cached_cert_valid(&c, b"nope"));
        assert!(!cached_cert_valid(b"", b""));
    }

    #[test]
    fn challenge_path_shape() {
        assert_eq!(challenge_path("tok"), "/.well-known/acme-challenge/tok");
    }

    #[test]
    fn directory_selection() {
        let o = AcmeOpts::default();
        assert!(o.directory_url().contains("staging"), "{}", o.directory_url());
        let o = AcmeOpts { production: true, ..Default::default() };
        assert!(!o.directory_url().contains("staging"), "{}", o.directory_url());
    }

    #[test]
    fn default_domain_pinned() {
        assert_eq!(DEFAULT_DOMAIN, "winterjs.bemly.moe");
        let o = AcmeOpts::default();
        assert_eq!(o.effective_domain(), "winterjs.bemly.moe");
        assert!(!o.enabled());
        let o = AcmeOpts { email: Some("a@b.c".into()), ..Default::default() };
        assert!(o.enabled());
    }
}
