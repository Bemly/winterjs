//! `node:tls` + `node:https` 的传输底座（tokio-rustls，dependencies2 §9d-剩余表口径）。
//! 复用 `node:net` 的事件通道与状态机（`NetEvent`/`NetCmd`/`state::net_*` + 泵），
//! TLS 只负责握手：client（Tcp connect → TlsConnector）与 server（Tcp accept →
//! TlsAcceptor），握完把流 split 后交 `net::spawn_pumps`（9d-6 泛化）。
//! 偏差记档：
//! - 证书校验：`ca`（PEM 串）→ 自建 roots；`rejectUnauthorized:false` → 跳过校验；
//!   缺省 → 系统 roots（rustls-native-certs）。hostname（SAN）校验由 rustls 全量做。
//! - 握手失败事件 code 为 `ERR_TLS_HANDSHAKE`（rustls 细粒度 cert 码未逐项映射，顺延）。
//! - 服务端 PEM 错误同步抛 TypeError（fail fast，Node 口径）；握手中单连接失败静默丢弃。
//! - 不做：`createSecureContext`/`getPeerCertificate`/客户端证书/`checkServerIdentity`
//!   自定义（顺延，另切片）。

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::context::JSContext;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::builtins::node::net::{spawn_pumps, NetEvent, NetKind};
use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};
use crate::state;

/// rustls ring provider（fetch/serve 同款；重复安装忽略）。
fn ensure_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// `rejectUnauthorized:false` 用的空校验器（测试/自签场景；生产缺省仍校验）。
#[derive(Debug)]
struct NoVerifier;

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
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// 握手结果 JSON（`tlsInfo` 事件体；JS 侧 getProtocol/getCipher/getPeerCertificate/alpnProtocol）。
fn tls_info_json(
    version: Option<rustls::ProtocolVersion>,
    suite: Option<rustls::SupportedCipherSuite>,
    alpn: Option<&[u8]>,
    peer: Option<&[rustls::pki_types::CertificateDer<'_>]>,
    servername: Option<&str>,
) -> String {
    use base64::Engine as _;
    let protocol = match version {
        Some(rustls::ProtocolVersion::TLSv1_3) => Some("TLSv1.3"),
        Some(rustls::ProtocolVersion::TLSv1_2) => Some("TLSv1.2"),
        _ => None,
    };
    let peer_chain: Vec<String> = peer
        .unwrap_or(&[])
        .iter()
        .map(|c| base64::engine::general_purpose::STANDARD.encode(c.as_ref()))
        .collect();
    serde_json::json!({
        "protocol": protocol,
        "cipher": suite.map(|s| format!("{:?}", s.suite())),
        "alpn": alpn.map(|a| String::from_utf8_lossy(a).into_owned()),
        "peerChain": peer_chain,
        "servername": servername,
    })
    .to_string()
}

/// 服务端配置（PEM 串 → ServerConfig；纯函数，单元测试覆盖）。
fn server_config(cert_pem: &str, key_pem: &str) -> Result<rustls::ServerConfig, String> {
    let certs: Vec<rustls::pki_types::CertificateDer<'static>> =
        rustls_pemfile::certs(&mut cert_pem.as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("TypeError: tls cert: bad PEM ({e})"))?;
    if certs.is_empty() {
        return Err("TypeError: tls cert: no certificate in PEM".into());
    }
    let key = rustls_pemfile::private_key(&mut key_pem.as_bytes())
        .map_err(|e| format!("TypeError: tls key: bad PEM ({e})"))?
        .ok_or_else(|| "TypeError: tls key: no private key in PEM".to_string())?;
    // 不走 `with_single_cert`：它经 webpki 解析终端证书做 keys_match，X.509 **v1** 证书
    // （node 测试 fixtures 的 agent*-cert 全是 v1，OpenSSL 照收）被拒成
    // `UnsupportedCertVersion`。服务端只需"出示"证书链，私钥仍由 provider 严格加载，
    // 公私钥不配对照样在 provider 装载/握手签名处失败——这里固定解析器直出。
    let provider = rustls::crypto::ring::default_provider();
    let signing = provider
        .key_provider
        .load_private_key(key)
        .map_err(|e| format!("TypeError: tls: bad key/cert pair ({e})"))?;
    let ck = std::sync::Arc::new(rustls::sign::CertifiedKey::new(certs, signing));
    Ok(rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(std::sync::Arc::new(FixedCert(ck))))
}

/// 无证书解析器（createServer 未给 key/cert：握手必败 → tlsClientError）。
#[derive(Debug)]
struct NoCert;

impl rustls::server::ResolvesServerCert for NoCert {
    fn resolve(&self, _hello: rustls::server::ClientHello<'_>) -> Option<std::sync::Arc<rustls::sign::CertifiedKey>> {
        None
    }
}

/// 固定证书解析器（见 `server_config`：绕开 v1 证书的 keys_match 拒收）。
#[derive(Debug)]
struct FixedCert(std::sync::Arc<rustls::sign::CertifiedKey>);

impl rustls::server::ResolvesServerCert for FixedCert {
    fn resolve(&self, _hello: rustls::server::ClientHello<'_>) -> Option<std::sync::Arc<rustls::sign::CertifiedKey>> {
        Some(self.0.clone())
    }
}

/// 客户端配置（纯函数，单元测试覆盖）。
/// `reject_unauthorized=false` → 跳过校验；`ca_pem` → 自建 roots；缺省系统 roots。
fn client_config(ca_pem: Option<&str>, reject_unauthorized: bool) -> Result<rustls::ClientConfig, String> {
    if !reject_unauthorized {
        return Ok(rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(std::sync::Arc::new(NoVerifier))
            .with_no_client_auth());
    }
    let mut roots = rustls::RootCertStore::empty();
    if let Some(pem) = ca_pem {
        let mut cas = Vec::new();
        for cert in rustls_pemfile::certs(&mut pem.as_bytes()) {
            let cert = cert.map_err(|e| format!("TypeError: tls ca: bad PEM ({e})"))?;
            roots.add(cert.clone()).map_err(|e| format!("TypeError: tls ca: rejected ({e})"))?;
            cas.push(cert);
        }
        if cas.is_empty() {
            return Err("TypeError: tls ca: no certificate in PEM".into());
        }
        // 用户 ca：标准 WebPki 校验 + X.509 v1 终端证书兜底（见 tls_v1）。
        let inner = rustls::client::WebPkiServerVerifier::builder(std::sync::Arc::new(roots))
            .build()
            .map_err(|e| format!("TypeError: tls ca: rejected ({e})"))?;
        return Ok(rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(std::sync::Arc::new(
                crate::builtins::node::tls_v1::V1FallbackVerifier::new(inner, cas),
            ))
            .with_no_client_auth());
    } else {
        let loaded = rustls_native_certs::load_native_certs();
        let mut added = 0usize;
        for cert in loaded.certs {
            if roots.add(cert).is_ok() {
                added += 1;
            }
        }
        if added == 0 {
            return Err(format!("TLS: no system roots ({} load errors)", loaded.errors.len()));
        }
    }
    Ok(rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth())
}

/// 服务端配置（HTTP/2，ALPN `h2`；`node:http2` 用）。
pub(crate) fn server_config_h2(cert_pem: &str, key_pem: &str) -> Result<rustls::ServerConfig, String> {
    let mut cfg = server_config(cert_pem, key_pem)?;
    cfg.alpn_protocols = vec![b"h2".to_vec()];
    Ok(cfg)
}

/// 客户端配置（HTTP/2，ALPN `h2`；`node:http2` 用）。
pub(crate) fn client_config_h2(
    ca_pem: Option<&str>,
    reject_unauthorized: bool,
) -> Result<rustls::ClientConfig, String> {
    let mut cfg = client_config(ca_pem, reject_unauthorized)?;
    cfg.alpn_protocols = vec![b"h2".to_vec()];
    Ok(cfg)
}

fn set_rval_str(cx: &mut JSContext, frame: &Frame, s: &str) {
    rooted!(&in(cx) let mut v = UndefinedValue());
    s.to_jsval(cx, v.handle_mut());
    frame.set_rval(v.get());
}

fn opt_num(frame: &Frame, i: u32) -> Option<f64> {
    let v = frame.arg(i);
    if v.is_number() { Some(v.to_number()) } else { None }
}

/// 连接选项 JSON（client）：`{servername?, ca?, rejectUnauthorized?}`。
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct ConnectOpts {
    servername: Option<String>,
    #[serde(rename = "ca")]
    ca_pem: Option<String>,
    #[serde(rename = "rejectUnauthorized")]
    reject_unauthorized: Option<bool>,
}

/// 监听选项 JSON（server）：`{cert, key}`（PEM 串；缺失即同步 TypeError）。
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct ListenOpts {
    cert: Option<String>,
    key: Option<String>,
}

/// `__wjs_tls_connect(host, port, optsJson, target)` → id。
pub unsafe extern "C" fn tls_connect(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 || !frame.arg(3).is_object() {
        report_error(&mut cx, "TypeError: tls connect internals missing target");
        return false;
    }
    let host = value_to_string(&mut cx, frame.arg(0));
    let Some(port) = opt_num(&frame, 1) else {
        report_error(&mut cx, "TypeError: tls connect: port must be a number");
        return false;
    };
    let opts: ConnectOpts = serde_json::from_str(&value_to_string(&mut cx, frame.arg(2)))
        .unwrap_or_default();
    let target = frame.arg(3);
    ensure_provider();
    let reject = opts.reject_unauthorized.unwrap_or(true);
    let cfg = match client_config(opts.ca_pem.as_deref(), reject) {
        Ok(c) => c,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let Some((id, ev_tx)) = state::net_alloc() else {
        report_error(&mut cx, "OperationError: net driver not installed");
        return false;
    };
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        report_error(&mut cx, "OperationError: no async runtime for tls connect");
        return false;
    };
    let cmd_rx = state::net_socket_add(id, target);
    set_rval_str(&mut cx, &frame, &id.to_string());
    let servername = opts.servername.unwrap_or_else(|| host.clone());
    handle.spawn(async move {
        let tcp = match crate::builtins::node::net_pumps::tcp_connect_resolved(host.as_str(), port as u16, None).await {
            Ok(s) => {
                // https 客户端默认 noDelay（Node https.js 口径）。
                let _ = s.set_nodelay(true);
                s
            }
            Err((code, msg)) => {
                let _ = ev_tx.send(NetEvent {
                    id,
                    kind: NetKind::Error { code: code.into(), msg },
                });
                let _ = ev_tx.send(NetEvent { id, kind: NetKind::Close });
                return;
            }
        };
        let name = match rustls::pki_types::ServerName::try_from(servername.clone()) {
            Ok(n) => n,
            Err(e) => {
                let _ = ev_tx.send(NetEvent {
                    id,
                    kind: NetKind::Error {
                        code: "ERR_TLS_HANDSHAKE".into(),
                        msg: format!("ERR_TLS_HANDSHAKE: bad servername '{servername}': {e}"),
                    },
                });
                let _ = ev_tx.send(NetEvent { id, kind: NetKind::Close });
                return;
            }
        };
        let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(cfg));
        match connector.connect(name, tcp).await {
            Err(e) => {
                let _ = ev_tx.send(NetEvent {
                    id,
                    kind: NetKind::Error {
                        code: "ERR_TLS_HANDSHAKE".into(),
                        msg: format!("ERR_TLS_HANDSHAKE: {e}"),
                    },
                });
                let _ = ev_tx.send(NetEvent { id, kind: NetKind::Close });
            }
            Ok(tls) => {
                let info = {
                    let c = tls.get_ref().1;
                    tls_info_json(
                        c.protocol_version(),
                        c.negotiated_cipher_suite(),
                        c.alpn_protocol(),
                        c.peer_certificates(),
                        Some(servername.as_str()),
                    )
                };
                let _ = ev_tx.send(NetEvent { id, kind: NetKind::TlsInfo { json: info } });
                let _ = ev_tx.send(NetEvent { id, kind: NetKind::Connect { local: None } });
                let (r, w) = tokio::io::split(tls);
                spawn_pumps(id, r, w, ev_tx, cmd_rx);
            }
        }
    });
    true
}

/// DER → PEM（64 列折行，node `getCACertificates` 口径）。
fn der_to_pem(der: &[u8]) -> String {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(der);
    let mut out = String::from("-----BEGIN CERTIFICATE-----\n");
    for chunk in b64.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        out.push('\n');
    }
    out.push_str("-----END CERTIFICATE-----\n");
    out
}

/// CA 证书集（PEM 数组 JSON）。`system` = OS 信任库（rustls-native-certs）；`bundled` 暂同
/// system（node 为内置 Mozilla 表——树内 webpki-root-certs 直引待拍板，记档偏离）；
/// `extra` = `NODE_EXTRA_CA_CERTS` 文件内的证书。
fn ca_certs_json(kind: &str) -> String {
    let pems: Vec<String> = match kind {
        "extra" => std::env::var("NODE_EXTRA_CA_CERTS")
            .ok()
            .and_then(|p| std::fs::read(p).ok())
            .map(|bytes| {
                rustls_pemfile::certs(&mut bytes.as_slice())
                    .filter_map(Result::ok)
                    .map(|c| der_to_pem(c.as_ref()))
                    .collect()
            })
            .unwrap_or_default(),
        _ => rustls_native_certs::load_native_certs().certs.iter().map(|c| der_to_pem(c.as_ref())).collect(),
    };
    serde_json::to_string(&pems).unwrap_or_else(|_| "[]".into())
}

/// `__wjs_tls_ca_certs(kind)` → PEM 数组 JSON 串。
///
/// UNSAFE-BOUNDARY: 前置——引擎回调 cx 有效；覆盖测试——`tests/node/tls.rs::phase11_tls_socket_surface`。
pub unsafe extern "C" fn tls_ca_certs(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let kind = if frame.argc() > 0 { value_to_string(&mut cx, frame.arg(0)) } else { "default".into() };
    set_rval_str(&mut cx, &frame, &ca_certs_json(&kind));
    true
}

/// `__wjs_tls_listen(port, host, optsJson, target)` → id。
pub unsafe extern "C" fn tls_listen(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 || !frame.arg(3).is_object() {
        report_error(&mut cx, "TypeError: tls listen internals missing target");
        return false;
    }
    let Some(port) = opt_num(&frame, 0) else {
        report_error(&mut cx, "TypeError: tls listen: port must be a number");
        return false;
    };
    let host = value_to_string(&mut cx, frame.arg(1));
    let opts: ListenOpts = serde_json::from_str(&value_to_string(&mut cx, frame.arg(2)))
        .unwrap_or_default();
    let target = frame.arg(3);
    ensure_provider();
    // node 口径：无 key/cert 也可起服务（SNICallback/后续 context 场景）——握手时无证书可出示，
    // 该连接握手失败走 'tlsClientError'，服务本身照常监听。
    let cfg = match (opts.cert, opts.key) {
        (Some(cert_pem), Some(key_pem)) => match server_config(&cert_pem, &key_pem) {
            Ok(c) => std::sync::Arc::new(c),
            Err(e) => {
                report_error(&mut cx, &e);
                return false;
            }
        },
        _ => std::sync::Arc::new(
            rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_cert_resolver(std::sync::Arc::new(NoCert)),
        ),
    };
    let Some((id, ev_tx)) = state::net_alloc() else {
        report_error(&mut cx, "OperationError: net driver not installed");
        return false;
    };
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        report_error(&mut cx, "OperationError: no async runtime for tls listen");
        return false;
    };
    let mut cmd_rx = state::net_socket_add(id, target);
    set_rval_str(&mut cx, &frame, &id.to_string());
    handle.spawn(async move {
        let acceptor = tokio_rustls::TlsAcceptor::from(cfg);
        let bound = tokio::net::TcpListener::bind((host.as_str(), port as u16)).await;
        let Ok(listener) = bound else {
            let e = bound.unwrap_err();
            let code = crate::builtins::node::fs::io_code(&e);
            let _ = ev_tx.send(NetEvent {
                id,
                kind: NetKind::ServerError { code: code.into(), msg: format!("{code}: {e}") },
            });
            let _ = ev_tx.send(NetEvent { id, kind: NetKind::ServerClose });
            return;
        };
        let local = listener
            .local_addr()
            .unwrap_or_else(|_| "0.0.0.0:0".parse::<std::net::SocketAddr>().expect("literal addr"));
        let _ = ev_tx.send(NetEvent {
            id,
            kind: NetKind::Listening { addr: local.ip().to_string(), port: local.port() },
        });
        loop {
            tokio::select! {
                acc = listener.accept() => {
                    let Ok((stream, peer)) = acc else { continue };
                    // 握手逐连接起任务（修前在 accept 循环里串行 await——慢/坏握手堵住后续连接），
                    // 失败发 'tlsClientError'（修前静默丢弃）。
                    let acceptor = acceptor.clone();
                    let ev_tx = ev_tx.clone();
                    tokio::spawn(async move {
                        let tls = match acceptor.accept(stream).await {
                            Ok(t) => t,
                            Err(e) => {
                                let _ = ev_tx.send(NetEvent {
                                    id,
                                    kind: NetKind::TlsClientError {
                                        code: "ERR_SSL_HANDSHAKE_FAILURE".into(),
                                        msg: format!("{e}"),
                                    },
                                });
                                return;
                            }
                        };
                        let conn_local = tls
                            .get_ref()
                            .0
                            .local_addr()
                            .unwrap_or_else(|_| "0.0.0.0:0".parse::<std::net::SocketAddr>().expect("literal addr"));
                        let info = {
                            let c = tls.get_ref().1;
                            tls_info_json(
                                c.protocol_version(),
                                c.negotiated_cipher_suite(),
                                c.alpn_protocol(),
                                c.peer_certificates(),
                                c.server_name(),
                            )
                        };
                        let (conn_id, conn_cmd_rx) = state::net_conn_add();
                        // 序：Connection（server 侧 attach socket）→ TlsInfo（此时 conn target 已在）
                        // → 起泵（数据事件必在 attach 之后，修前泵先起可能先于 attach 到达即丢）。
                        let _ = ev_tx.send(NetEvent {
                            id,
                            kind: NetKind::Connection {
                                conn_id,
                                remote_addr: peer.ip().to_string(),
                                remote_port: peer.port(),
                                local_addr: conn_local.ip().to_string(),
                                local_port: conn_local.port(),
                            },
                        });
                        let _ = ev_tx.send(NetEvent { id: conn_id, kind: NetKind::TlsInfo { json: info } });
                        let (r, w) = tokio::io::split(tls);
                        spawn_pumps(conn_id, r, w, ev_tx, conn_cmd_rx);
                    });
                }
                _ = cmd_rx.recv() => break,
            }
        }
        let _ = ev_tx.send(NetEvent { id, kind: NetKind::ServerClose });
    });
    true
}

/// 内嵌 ESM 源（`node:tls`；P2 起 TLSSocket 建在 net.Socket 之上，JS 见 `tls.js`）。
pub const SOURCE: &str = include_str!("tls.js");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls_server_config_rejects_garbage() {
        assert!(server_config("nope", "nope").is_err());
        assert!(server_config("", "").is_err());
    }

    #[test]
    fn tls_client_config_no_verify_builds() {
        // rejectUnauthorized:false 不碰系统 roots，纯构造不断言网络
        assert!(client_config(None, false).is_ok());
        assert!(client_config(Some("definitely not pem"), true).is_err());
    }
}
