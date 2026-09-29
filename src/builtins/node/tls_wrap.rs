//! TLSSocket 包裹引擎（P2-tls-b，2026-09-26）：rustls Connection 由 JS 字节驱动
//! （内存 BIO 形）。`tls.connect({socket})` / `new TLSSocket(duplex[, opts])` 走此路：
//! wrapped socket 的密文由 JS 经 `__wjs2_tls_wrap_feed` 喂进 `process_new_packets`，
//! 回程 JSON 一次带密文（写回 wrapped）/明文（交付 TLSSocket）/握手完成旗/校验捕获/
//! 握手信息——握手节奏由 JS 侧事件序控制，pipe/IPC 等任意 duplex 包裹天然支持。
//! 与直拨路径（tls.rs tokio-rustls）的差异：node TLSSocket 口径下证书校验失败
//! **不**中止握手，错误经 CaptureVerifier 捕获后照常完成，由 JS 'secure' 侧处置
//! （onConnectSecure 按 rejectUnauthorized 决定 destroy；纯 TLSSocket 只上报）。
//! 引擎表在 `state::tls_wrap`（PlainState.tls_engines，无 GC 指针；native 同步进出，
//! 仅 JS 线程触碰，§4.24）。

use std::io::Read as _;

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::context::JSContext;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{report_error, value_to_string, view_bytes, wrap_cx, Frame};
use crate::state;

/// rustls 连接（client/server 二形；手动模式 API 全经本枚举分派）。
enum TlsSide {
    Client(Box<rustls::ClientConnection>),
    Server(Box<rustls::ServerConnection>),
}

pub(crate) struct TlsWrapEngine {
    side: TlsSide,
    /// 捕获的证书校验错（CaptureVerifier 写入；随首个握手完成点回传一次）。
    caught: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    /// 协议错后置位（rustls 连接已毒化）：后续 feed/write 一律回报 err。
    err: Option<String>,
    /// 握手信息只随首个完成点回传一次。
    info_sent: bool,
    /// TLS 层 EOF（close_notify 排空后 reader 恒 Ok(0)——0.23 无公开查询面，
    /// 以读行为判定；粘滞）。
    eof_seen: bool,
}

impl TlsWrapEngine {
    fn new(side: TlsSide, caught: std::sync::Arc<std::sync::Mutex<Option<String>>>) -> Self {
        Self { side, caught, err: None, info_sent: false, eof_seen: false }
    }

    fn poison(&mut self, msg: String) {
        if self.err.is_none() {
            self.err = Some(msg);
        }
    }

    fn read_tls(&mut self, rd: &mut dyn std::io::Read) -> std::io::Result<usize> {
        match &mut self.side {
            TlsSide::Client(c) => c.read_tls(rd),
            TlsSide::Server(c) => c.read_tls(rd),
        }
    }

    /// 产出密文（握手飞行包 + 加密应用数据；write_tls 排空发送缓冲）。
    fn drain_tls(&mut self, out: &mut Vec<u8>) {
        match &mut self.side {
            TlsSide::Client(c) => {
                let _ = c.write_tls(out);
            }
            TlsSide::Server(c) => {
                let _ = c.write_tls(out);
            }
        }
    }

    /// 排空已解密明文。reader 语义（rustls 0.23）：Ok(0)=已收 close_notify 且排空；
    /// Err(WouldBlock)=连接在等更多数据；Err(UnexpectedEof)=对端 FIN 无 close_notify。
    /// 返回是否 TLS 层 EOF（粘滞存 eof_seen）。
    fn drain_plain(&mut self, out: &mut Vec<u8>) {
        let drain_one = |r: &mut rustls::Reader<'_>, out: &mut Vec<u8>, eof: &mut bool| {
            let mut buf = [0u8; 16 * 1024];
            loop {
                match r.read(&mut buf) {
                    Ok(0) => {
                        *eof = true;
                        break;
                    }
                    Ok(n) => out.extend_from_slice(&buf[..n]),
                    Err(_) => break,
                }
            }
        };
        match &mut self.side {
            TlsSide::Client(c) => drain_one(&mut c.reader(), out, &mut self.eof_seen),
            TlsSide::Server(c) => drain_one(&mut c.reader(), out, &mut self.eof_seen),
        }
    }

    fn process(&mut self) -> Result<(), rustls::Error> {
        match &mut self.side {
            TlsSide::Client(c) => c.process_new_packets().map(|_| ()),
            TlsSide::Server(c) => c.process_new_packets().map(|_| ()),
        }
    }

    fn is_handshaking(&self) -> bool {
        match &self.side {
            TlsSide::Client(c) => c.is_handshaking(),
            TlsSide::Server(c) => c.is_handshaking(),
        }
    }

    fn eof(&self) -> bool {
        self.eof_seen
    }

    fn alpn(&self) -> Option<Vec<u8>> {
        match &self.side {
            TlsSide::Client(c) => c.alpn_protocol().map(|a| a.to_vec()),
            TlsSide::Server(c) => c.alpn_protocol().map(|a| a.to_vec()),
        }
    }

    fn version(&self) -> Option<rustls::ProtocolVersion> {
        match &self.side {
            TlsSide::Client(c) => c.protocol_version(),
            TlsSide::Server(c) => c.protocol_version(),
        }
    }

    fn suite(&self) -> Option<rustls::SupportedCipherSuite> {
        match &self.side {
            TlsSide::Client(c) => c.negotiated_cipher_suite(),
            TlsSide::Server(c) => c.negotiated_cipher_suite(),
        }
    }

    fn peer(&self) -> Option<Vec<rustls::pki_types::CertificateDer<'static>>> {
        let certs = match &self.side {
            TlsSide::Client(c) => c.peer_certificates(),
            TlsSide::Server(c) => c.peer_certificates(),
        }?;
        Some(certs.iter().map(|c| c.clone().into_owned()).collect())
    }

    fn sni(&self) -> Option<String> {
        match &self.side {
            TlsSide::Server(c) => c.server_name().map(String::from),
            _ => None,
        }
    }

    fn queue_plain(&mut self, data: &[u8]) -> Result<(), String> {
        use std::io::Write as _;
        match &mut self.side {
            TlsSide::Client(c) => c.writer().write_all(data).map_err(|e| e.to_string()),
            TlsSide::Server(c) => c.writer().write_all(data).map_err(|e| e.to_string()),
        }
    }

    fn send_close_notify(&mut self) {
        match &mut self.side {
            TlsSide::Client(c) => c.send_close_notify(),
            TlsSide::Server(c) => c.send_close_notify(),
        }
    }
}

/// node TLSSocket 口径：证书校验失败**不**中止握手——错误捕获后照常完成，
/// 由 JS 'secure' 侧（onConnectSecure / ssl.verifyError）决定处置。
#[derive(Debug)]
struct CaptureVerifier {
    inner: std::sync::Arc<crate::builtins::node::tls_v1::V1FallbackVerifier>,
    caught: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

impl rustls::client::danger::ServerCertVerifier for CaptureVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        intermediates: &[rustls::pki_types::CertificateDer<'_>],
        server_name: &rustls::pki_types::ServerName<'_>,
        ocsp_response: &[u8],
        now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        match self
            .inner
            .verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
        {
            Ok(v) => Ok(v),
            Err(e) => {
                if let Ok(mut slot) = self.caught.lock() {
                    *slot = Some(e.to_string());
                }
                Ok(rustls::client::danger::ServerCertVerified::assertion())
            }
        }
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

/// 包裹配置 JSON：`{servername?, ca?, rejectUnauthorized?, alpnB64?, key?, cert?}`。
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct WrapCfg {
    servername: Option<String>,
    ca: Option<String>,
    #[serde(rename = "rejectUnauthorized")]
    reject_unauthorized: Option<bool>,
    #[serde(rename = "alpnB64")]
    alpn_b64: Option<String>,
    key: Option<String>,
    cert: Option<String>,
}

/// ALPN wire 形（node convertALPNProtocols 产物：每名 u8 长度前缀）→ rustls 列表。
pub(crate) fn parse_alpn(b64: Option<&str>) -> Vec<Vec<u8>> {
    use base64::Engine as _;
    let Some(b64) = b64 else { return Vec::new() };
    let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(b64) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < raw.len() {
        let l = raw[i] as usize;
        if l == 0 || i + 1 + l > raw.len() {
            break;
        }
        out.push(raw[i + 1..i + 1 + l].to_vec());
        i += 1 + l;
    }
    out
}

/// 捕获式客户端配置（node TLSSocket 口径：校验照跑、失败不中止握手——错误捕获后
/// 随握手完成点回传 JS 处置；WebPki + X.509 v1 兜底，见 tls_v1）。
pub(crate) fn capture_client_config(
    ca_pem: Option<&str>,
) -> Result<(rustls::ClientConfig, std::sync::Arc<std::sync::Mutex<Option<String>>>), String> {
    let caught: std::sync::Arc<std::sync::Mutex<Option<String>>> = Default::default();
    let mut roots = rustls::RootCertStore::empty();
    let mut cas: Vec<rustls::pki_types::CertificateDer<'static>> = Vec::new();
    match ca_pem {
        Some(pem) => {
            for cert in rustls_pemfile::certs(&mut pem.as_bytes()) {
                let cert = cert.map_err(|e| format!("TypeError: tls ca: bad PEM ({e})"))?;
                roots.add(cert.clone()).map_err(|e| format!("TypeError: tls ca: rejected ({e})"))?;
                cas.push(cert);
            }
        }
        None => {
            for cert in rustls_native_certs::load_native_certs().certs {
                if roots.add(cert.clone()).is_ok() {
                    cas.push(cert);
                }
            }
            if roots.is_empty() {
                return Err("TLS: no system roots".into());
            }
        }
    }
    let inner = rustls::client::WebPkiServerVerifier::builder(std::sync::Arc::new(roots))
        .build()
        .map_err(|e| format!("TypeError: tls ca: rejected ({e})"))?;
    // v1 证书兜底（node fixtures 大量 v1 终端证书，OpenSSL 照收；见 tls_v1）。
    let v1 = std::sync::Arc::new(crate::builtins::node::tls_v1::V1FallbackVerifier::new(inner, cas));
    Ok((
        rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(std::sync::Arc::new(CaptureVerifier {
                inner: v1,
                caught: caught.clone(),
            }))
            .with_no_client_auth(),
        caught,
    ))
}

/// 包裹客户端配置：reject=false 走 NoVerifier（纯跳过）；true 走捕获层。
fn wrap_client_config(
    ca_pem: Option<&str>,
    reject: bool,
) -> Result<(rustls::ClientConfig, std::sync::Arc<std::sync::Mutex<Option<String>>>), String> {
    if !reject {
        return Ok((
            rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(std::sync::Arc::new(
                    crate::builtins::node::tls::NoVerifier,
                ))
                .with_no_client_auth(),
            Default::default(),
        ));
    }
    capture_client_config(ca_pem)
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

fn b64_opt(v: &[u8]) -> Option<String> {
    use base64::Engine as _;
    if v.is_empty() {
        None
    } else {
        Some(base64::engine::general_purpose::STANDARD.encode(v))
    }
}

/// feed/eof/write/shutdown 共用回程：`{out, plain, hs, eof, info, sni, verifyErr, err}`。
/// feed 循环内按轮排空的明文经此并入回程（rustls received_plaintext 上限防护）。
fn engine_result(e: &mut TlsWrapEngine, pre_plain: Vec<u8>) -> String {
    let mut out = Vec::new();
    let mut plain = pre_plain;
    e.drain_tls(&mut out);
    e.drain_plain(&mut plain);
    let hs = !e.is_handshaking();
    let eof = e.eof();
    let (mut info, mut sni, mut verify_err): (Option<serde_json::Value>, Option<String>, Option<String>) =
        (None, None, None);
    if hs && !e.info_sent && e.err.is_none() {
        e.info_sent = true;
        sni = e.sni();
        if let Ok(mut slot) = e.caught.lock() {
            verify_err = slot.take();
        }
        let peer = e.peer();
        let info_str = crate::builtins::node::tls::tls_info_json(
            e.version(),
            e.suite(),
            e.alpn().as_deref(),
            peer.as_deref(),
            sni.as_deref(),
            None,
        );
        info = serde_json::from_str(&info_str).ok();
    }
    serde_json::json!({
        "out": b64_opt(&out),
        "plain": b64_opt(&plain),
        "hs": hs,
        "eof": eof,
        "info": info,
        "sni": sni,
        "verifyErr": verify_err,
        "err": e.err,
    })
    .to_string()
}

/// `__wjs2_tls_wrap_open(isServer, cfgJson)` → id。
///
/// UNSAFE-BOUNDARY: 前置——引擎回调 cx 有效；覆盖测试——`tests/node/tls.rs`（wrap 面）。
pub unsafe extern "C" fn tls_wrap_open(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let is_server = opt_num(&frame, 0) == Some(1.0);
    let cfg: WrapCfg = serde_json::from_str(&value_to_string(&mut cx, frame.arg(1)))
        .unwrap_or_default();
    crate::builtins::node::tls::ensure_provider();
    let alpn = parse_alpn(cfg.alpn_b64.as_deref());
    let engine = if is_server {
        let sc = match (cfg.key.clone(), cfg.cert.clone()) {
            (Some(key), Some(cert)) => {
                let mut c = match crate::builtins::node::tls::server_config(&cert, &key) {
                    Ok(c) => c,
                    Err(e) => {
                        report_error(&mut cx, &e);
                        return false;
                    }
                };
                c.alpn_protocols = alpn;
                c
            }
            // 无证书（STARTTLS 包装期缺省形）：握手无证书可出示 → 协议错回传 JS。
            _ => {
                let mut c = crate::builtins::node::tls::server_config_no_cert();
                c.alpn_protocols = alpn;
                c
            }
        };
        match rustls::ServerConnection::new(std::sync::Arc::new(sc)) {
            Ok(c) => {
                let caught: std::sync::Arc<std::sync::Mutex<Option<String>>> = Default::default();
                TlsWrapEngine::new(TlsSide::Server(Box::new(c)), caught)
            }
            Err(e) => {
                report_error(&mut cx, &format!("TypeError: tls wrap server: {e}"));
                return false;
            }
        }
    } else {
        let servername = cfg.servername.clone().unwrap_or_else(|| "localhost".into());
        let name = match rustls::pki_types::ServerName::try_from(servername.clone()) {
            Ok(n) => n,
            Err(e) => {
                report_error(
                    &mut cx,
                    &format!("TypeError: tls wrap: bad servername '{servername}': {e}"),
                );
                return false;
            }
        };
        let reject = cfg.reject_unauthorized.unwrap_or(true);
        let (cc, caught) = match wrap_client_config(cfg.ca.as_deref(), reject) {
            Ok(v) => v,
            Err(e) => {
                report_error(&mut cx, &e);
                return false;
            }
        };
        let mut cc = cc;
        cc.alpn_protocols = alpn;
        match rustls::ClientConnection::new(std::sync::Arc::new(cc), name) {
            Ok(c) => TlsWrapEngine::new(TlsSide::Client(Box::new(c)), caught),
            Err(e) => {
                report_error(&mut cx, &format!("TypeError: tls wrap client: {e}"));
                return false;
            }
        }
    };
    let id = state::tls_wrap_add(engine);
    set_rval_str(&mut cx, &frame, &id.to_string());
    true
}

/// `__wjs2_tls_wrap_feed(id, u8)` → 结果 JSON。
pub unsafe extern "C" fn tls_wrap_feed(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = opt_num(&frame, 0) else {
        report_error(&mut cx, "TypeError: tls feed: id must be a number");
        return false;
    };
    let data = if frame.arg(1).is_undefined() || frame.arg(1).is_null() {
        Vec::new()
    } else {
        match view_bytes(&mut cx, frame.arg(1), "tls feed data") {
            Some(b) => b,
            None => return false,
        }
    };
    let json = state::tls_wrap_with_engine(id as u64, |e| {
        let mut pre_plain = Vec::new();
        if e.err.is_none() {
            let mut cur = std::io::Cursor::new(&data[..]);
            loop {
                match e.read_tls(&mut cur) {
                    Ok(0) => break,
                    Ok(_) => {
                        if let Err(err) = e.process() {
                            e.poison(err.to_string());
                            break;
                        }
                        // 逐轮排空明文（大馈入防 rustls received_plaintext 撑满）
                        e.drain_plain(&mut pre_plain);
                    }
                    Err(err) => {
                        e.poison(format!("bad TLS record: {err}"));
                        break;
                    }
                }
            }
        }
        engine_result(e, pre_plain)
    });
    match json {
        Some(j) => {
            set_rval_str(&mut cx, &frame, &j);
            true
        }
        None => {
            report_error(&mut cx, "TypeError: tls feed: engine gone");
            false
        }
    }
}

/// `__wjs2_tls_wrap_write(id, u8)` → 结果 JSON（明文入引擎，回程带密文飞行包）。
pub unsafe extern "C" fn tls_wrap_write(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = opt_num(&frame, 0) else {
        report_error(&mut cx, "TypeError: tls write: id must be a number");
        return false;
    };
    let data = match view_bytes(&mut cx, frame.arg(1), "tls write data") {
        Some(b) => b,
        None => return false,
    };
    let json = state::tls_wrap_with_engine(id as u64, |e| {
        if e.err.is_none() {
            if let Err(msg) = e.queue_plain(&data) {
                e.poison(msg);
            }
        }
        engine_result(e, Vec::new())
    });
    match json {
        Some(j) => {
            set_rval_str(&mut cx, &frame, &j);
            true
        }
        None => {
            report_error(&mut cx, "TypeError: tls write: engine gone");
            false
        }
    }
}

/// `__wjs2_tls_wrap_eof(id)` → 结果 JSON（wrapped EOF 后排空残余明文）。
pub unsafe extern "C" fn tls_wrap_eof(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    unsafe { tls_wrap_call1(cx_raw, argc, vp, "eof") }
}

/// `__wjs2_tls_wrap_shutdown(id)` → 结果 JSON（close_notify 出站）。
pub unsafe extern "C" fn tls_wrap_shutdown(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    unsafe { tls_wrap_call1(cx_raw, argc, vp, "shutdown") }
}

/// eof/shutdown 共用：无输入处理，仅排空/close_notify 后回结果。
unsafe fn tls_wrap_call1(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
    op: &str,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = opt_num(&frame, 0) else {
        report_error(&mut cx, &format!("TypeError: tls {op}: id must be a number"));
        return false;
    };
    let json = state::tls_wrap_with_engine(id as u64, |e| {
        if op == "shutdown" && e.err.is_none() {
            e.send_close_notify();
        }
        engine_result(e, Vec::new())
    });
    match json {
        Some(j) => {
            set_rval_str(&mut cx, &frame, &j);
            true
        }
        None => {
            report_error(&mut cx, &format!("TypeError: tls {op}: engine gone"));
            false
        }
    }
}

/// `__wjs2_tls_wrap_kill(id)`：摘表（连接随即弃）。
pub unsafe extern "C" fn tls_wrap_kill(
    _cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if let Some(id) = opt_num(&frame, 0) {
        state::tls_wrap_del(id as u64);
    }
    true
}
