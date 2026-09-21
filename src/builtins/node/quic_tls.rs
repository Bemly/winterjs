//! quic TLS/传输配置域（监听/连接选项、证书校验、传输参数；对齐 quic.rs；纯搬移）。

use std::sync::Arc;
use std::time::Duration;

/// 监听选项（JS 侧校验形态，Rust 侧解析；alpn 非空由 JS 保证，Rust 再断言）。
#[derive(serde::Deserialize)]
pub(crate) struct ListenOpts {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) alpn: Vec<String>,
    pub(crate) key_pem: String,
    pub(crate) cert_pem: String,
    #[serde(default)]
    pub(crate) idle_timeout_ms: Option<u64>,
    #[serde(default)]
    pub(crate) cc: Option<String>,
}

/// 连接选项（alpn 单串；servername 缺省取 host）。
#[derive(serde::Deserialize)]
pub(crate) struct ConnectOpts {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) alpn: String,
    #[serde(default)]
    pub(crate) servername: Option<String>,
    #[serde(default)]
    pub(crate) ca_pem: Option<String>,
    #[serde(default)]
    pub(crate) reject_unauthorized: Option<bool>,
    #[serde(default)]
    pub(crate) idle_timeout_ms: Option<u64>,
    #[serde(default)]
    pub(crate) cc: Option<String>,
}

/// 验签跳过（tls.rs `NoVerifier` 同款；`rejectUnauthorized:false` 用）。
#[derive(Debug)]
pub(crate) struct NoVerifier;

impl rustls::client::danger::ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider().signature_verification_algorithms.supported_schemes()
    }
}

/// 传输配置（idle 超时 + 拥塞控制三档；`cc` 非法即 Err）。
pub(crate) fn build_transport(idle_timeout_ms: Option<u64>, cc: Option<&str>) -> Result<Arc<quinn::TransportConfig>, String> {
    let mut t = quinn::TransportConfig::default();
    match idle_timeout_ms {
        Some(0) | None => {}
        Some(ms) => {
            let to = quinn::IdleTimeout::try_from(Duration::from_millis(ms))
                .map_err(|_| "TypeError: options.idleTimeout out of range".to_string())?;
            t.max_idle_timeout(Some(to));
        }
    }
    match cc {
        None => {}
        Some("reno") => {
            t.congestion_controller_factory(Arc::new(quinn::congestion::NewRenoConfig::default()));
        }
        Some("cubic") => {
            t.congestion_controller_factory(Arc::new(quinn::congestion::CubicConfig::default()));
        }
        Some("bbr") => {
            t.congestion_controller_factory(Arc::new(quinn::congestion::BbrConfig::default()));
        }
        Some(other) => return Err(format!("TypeError: options.cc must be reno/cubic/bbr (got {other})")),
    }
    Ok(Arc::new(t))
}

/// 服务端 TLS（PEM 双件；零证书/坏 key 即 fail fast，§4.38 同口径）。
pub(crate) fn build_server_tls(key_pem: &str, cert_pem: &str, alpn: &[Vec<u8>]) -> Result<rustls::ServerConfig, String> {
    if key_pem.is_empty() || cert_pem.is_empty() {
        return Err("TypeError: listen needs options.key and options.cert (PEM)".into());
    }
    let certs: Vec<rustls::pki_types::CertificateDer<'static>> =
        rustls_pemfile::certs(&mut cert_pem.as_bytes())
            .collect::<Result<_, _>>()
            .map_err(|_| "TypeError: options.cert is not valid PEM".to_string())?;
    if certs.is_empty() {
        return Err("TypeError: options.cert has no certificate".into());
    }
    let key = rustls_pemfile::private_key(&mut key_pem.as_bytes())
        .map_err(|_| "TypeError: options.key is not valid PEM".to_string())?
        .ok_or_else(|| "TypeError: options.key has no private key".to_string())?;
    let mut cfg = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| format!("TypeError: bad key/cert pair ({e})"))?;
    cfg.alpn_protocols = alpn.to_vec();
    Ok(cfg)
}

/// 客户端 TLS（`ca` 缺省走系统 roots；`rejectUnauthorized:false` 跳校验）。
pub(crate) fn build_client_tls(
    ca_pem: Option<&str>,
    reject_unauthorized: bool,
    alpn: Vec<u8>,
) -> Result<rustls::ClientConfig, String> {
    let mut roots = rustls::RootCertStore::empty();
    if let Some(pem) = ca_pem {
        let mut added = 0usize;
        for cert in rustls_pemfile::certs(&mut pem.as_bytes()) {
            match cert {
                Ok(c) => {
                    roots.add(c).map_err(|e| format!("TypeError: bad ca cert ({e})"))?;
                    added += 1;
                }
                Err(_) => return Err("TypeError: options.ca is not valid PEM".into()),
            }
        }
        if added == 0 {
            return Err("TypeError: options.ca has no certificate".into());
        }
    } else {
        // tls.rs 同款：系统 roots 逐个装，坏的跳过；零命中即错。
        let loaded = rustls_native_certs::load_native_certs();
        let mut added = 0usize;
        for cert in loaded.certs {
            if roots.add(cert).is_ok() {
                added += 1;
            }
        }
        if added == 0 {
            return Err(format!(
                "OperationError: no system roots ({} load errors)",
                loaded.errors.len()
            ));
        }
    }
    let mut cfg = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    if !reject_unauthorized {
        cfg.dangerous().set_certificate_verifier(Arc::new(NoVerifier));
    }
    cfg.alpn_protocols = vec![alpn];
    Ok(cfg)
}

/// 握手信息（真协商值；取不到即空串，不编造）。
pub(crate) fn handshake_info(conn: &quinn::Connection) -> (String, String) {
    let mut alpn = String::new();
    let mut servername = String::new();
    if let Some(data) = conn.handshake_data() {
        if let Ok(h) = data.downcast::<quinn::crypto::rustls::HandshakeData>() {
            if let Some(p) = h.protocol {
                alpn = String::from_utf8_lossy(&p).into_owned();
            }
            if let Some(s) = h.server_name {
                servername = s;
            }
        }
    }
    (alpn, servername)
}

/// 关闭原因映射（应用码透出；本地关按 0；其余 -1 + 文案）。
pub(crate) fn close_info(err: quinn::ConnectionError) -> (i64, String) {
    match err {
        quinn::ConnectionError::ApplicationClosed(app) => {
            (app.error_code.into_inner() as i64, String::from_utf8_lossy(&app.reason).into_owned())
        }
        quinn::ConnectionError::LocallyClosed => (0, String::new()),
        quinn::ConnectionError::ConnectionClosed(frame) => {
            (-1, format!("closed by peer: {frame}"))
        }
        other => (-1, other.to_string()),
    }
}
