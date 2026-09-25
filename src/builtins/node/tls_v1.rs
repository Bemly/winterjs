//! X.509 **v1** 证书的客户端校验兜底（P1 http2/tls，2026-09-25）。
//! webpki 只收 v3，node 测试 fixtures（agent*-cert）大量是 v1、OpenSSL 照收。
//! 策略：先走标准 WebPki 校验；仅当报 `UnsupportedCertVersion` 时改走手工路径——
//! ① 终端证书的 issuer 在用户给的 `ca` 里（Name DER 全等）且签名经该 CA 公钥验过
//! （复用 `node:crypto` X509 验签核 `x509_verify_impl`）；② 有效期覆盖当前时刻；
//! ③ 主机名对 subject CN 匹配（v1 无扩展、无 SAN；支持 `*.` 单层通配）。
//! 握手签名：TLS 1.3 经 SPKI 走 `verify_tls13_signature_with_raw_key`；TLS 1.2 无
//! 原始公钥变体（rustls 限制），v1 + 1.2 + 校验 组合如实失败（记档）。

use std::sync::Arc;

use der::{Decode as _, Encode as _};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, SubjectPublicKeyInfoDer, UnixTime};
use rustls::{CertificateError, DigitallySignedStruct, Error, OtherError, SignatureScheme};

#[derive(Debug)]
pub(crate) struct V1FallbackVerifier {
    inner: Arc<rustls::client::WebPkiServerVerifier>,
    cas: Vec<CertificateDer<'static>>,
}

impl V1FallbackVerifier {
    pub(crate) fn new(inner: Arc<rustls::client::WebPkiServerVerifier>, cas: Vec<CertificateDer<'static>>) -> Self {
        Self { inner, cas }
    }
}

/// 是否为 webpki 的"证书版本不支持"错（v1 证书的唯一拒因）。
fn is_unsupported_version(e: &Error) -> bool {
    matches!(e, Error::InvalidCertificate(CertificateError::Other(OtherError(o)))
        if format!("{o:?}").contains("UnsupportedCertVersion"))
}

/// SPKI → `x509_verify_impl` 的 (key, key_type)。
fn key_for_verify(spki: &x509_cert::spki::SubjectPublicKeyInfoOwned) -> Option<(Vec<u8>, &'static str)> {
    let der = spki.to_der().ok()?;
    match spki.algorithm.oid.to_string().as_str() {
        "1.2.840.113549.1.1.1" => Some((der, "rsa")),
        "1.2.840.10045.2.1" => Some((der, "ec")),
        "1.3.101.112" => Some((spki.subject_public_key.raw_bytes().to_vec(), "ed25519")),
        _ => None,
    }
}

/// subject CN（v1 证书的唯一主机名来源）。
fn subject_cn(cert: &x509_cert::Certificate) -> Option<String> {
    // 最后一个 CN（OpenSSL/node 取最具体者）；直接取值字节，兼容 UTF8/Printable/IA5 各串型。
    let mut out = None;
    for atv in cert.tbs_certificate().subject().iter() {
        if atv.oid.to_string() == "2.5.4.3" {
            out = std::str::from_utf8(atv.value.value()).ok().map(str::to_owned);
        }
    }
    out
}

/// 主机名匹配（大小写不敏感；`*.x.y` 只匹配单层标签）。
pub(crate) fn host_matches(pattern: &str, host: &str) -> bool {
    let (p, h) = (pattern.to_ascii_lowercase(), host.to_ascii_lowercase());
    if let Some(rest) = p.strip_prefix("*.") {
        return h.split_once('.').is_some_and(|(label, tail)| !label.is_empty() && tail == rest);
    }
    p == h
}

fn bad(msg: &str) -> Error {
    Error::General(format!("invalid peer certificate: {msg}"))
}

impl V1FallbackVerifier {
    fn verify_v1(&self, ee: &CertificateDer<'_>, name: &ServerName<'_>, now: UnixTime) -> Result<(), Error> {
        let cert = x509_cert::Certificate::from_der(ee.as_ref()).map_err(|_| bad("BadEncoding"))?;
        let issuer = cert.tbs_certificate().issuer().to_der().map_err(|_| bad("BadEncoding"))?;
        let mut signed = false;
        for ca in &self.cas {
            let Ok(c) = x509_cert::Certificate::from_der(ca.as_ref()) else { continue };
            if c.tbs_certificate().subject().to_der().ok().as_deref() != Some(issuer.as_slice()) {
                continue;
            }
            let Some((key, kt)) = key_for_verify(&c.tbs_certificate().subject_public_key_info()) else { continue };
            if crate::builtins::crypto::x509::x509_verify_impl(ee.as_ref(), &key, kt).unwrap_or(false) {
                signed = true;
                break;
            }
        }
        if !signed {
            return Err(Error::InvalidCertificate(CertificateError::UnknownIssuer));
        }
        let v = &cert.tbs_certificate().validity();
        let t = now.as_secs();
        if t < v.not_before.to_unix_duration().as_secs() {
            return Err(Error::InvalidCertificate(CertificateError::NotValidYet));
        }
        if t > v.not_after.to_unix_duration().as_secs() {
            return Err(Error::InvalidCertificate(CertificateError::Expired));
        }
        let host = match name {
            ServerName::DnsName(d) => d.as_ref().to_owned(),
            ServerName::IpAddress(ip) => std::net::IpAddr::from(*ip).to_string(),
            _ => return Err(Error::InvalidCertificate(CertificateError::NotValidForName)),
        };
        match subject_cn(&cert) {
            Some(cn) if host_matches(&cn, &host) => Ok(()),
            _ => Err(Error::InvalidCertificate(CertificateError::NotValidForName)),
        }
    }
}

impl ServerCertVerifier for V1FallbackVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        match self.inner.verify_server_cert(end_entity, intermediates, server_name, ocsp, now) {
            Err(e) if is_unsupported_version(&e) => {
                self.verify_v1(end_entity, server_name, now)?;
                Ok(ServerCertVerified::assertion())
            }
            r => r,
        }
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        match self.inner.verify_tls13_signature(message, cert, dss) {
            Err(e) if is_unsupported_version(&e) => {
                let c = x509_cert::Certificate::from_der(cert.as_ref()).map_err(|_| bad("BadEncoding"))?;
                let spki = c.tbs_certificate().subject_public_key_info().to_der().map_err(|_| bad("BadEncoding"))?;
                let algs = rustls::crypto::ring::default_provider().signature_verification_algorithms;
                rustls::crypto::verify_tls13_signature_with_raw_key(
                    message,
                    &SubjectPublicKeyInfoDer::from(spki.as_slice()),
                    dss,
                    &algs,
                )
            }
            r => r,
        }
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_matching() {
        assert!(host_matches("localhost", "LOCALHOST"));
        assert!(host_matches("*.example.com", "a.example.com"));
        // 报错：通配不跨层、不匹配裸域。
        assert!(!host_matches("*.example.com", "a.b.example.com"));
        assert!(!host_matches("*.example.com", "example.com"));
        // 边界：空标签。
        assert!(!host_matches("*.example.com", ".example.com"));
    }
}
