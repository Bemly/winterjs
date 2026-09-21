//! crypto x509 域（证书解析/签发检查；对齐 crypto.rs；纯搬移）。

use mozjs::jsval::JSVal;

use crate::jsapi_glue::{report_error, view_bytes, wrap_cx, Frame};
use super::crypto::set_rval_str;

// ── 9e-1d X509（`x509-cert` 直用；9i-3 起验签面就位：verify/publicKey/ca，
//    checkIssued/checkPrivateKey/签发不做）────────────────────

/// unix 秒 → `MMM DD HH:MM:SS YYYY GMT`（openssl `ASN1_TIME_print` 口径，日空位补空格）。
pub(crate) fn fmt_asn1_time(secs: u64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    // Howard Hinnant days-from-civil 逆算法
    let days = (secs / 86400) as i64;
    let rem = (secs % 86400) as u64;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!(
        "{} {:>2} {:02}:{:02}:{:02} {} GMT",
        MONTHS[(m - 1) as usize],
        d,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60,
        year
    )
}

/// 属性 OID → 短名（Node `toLegacyObject` 口径；未知回 dotted）。
pub(crate) fn atv_short(oid: &str) -> &str {
    match oid {
        "2.5.4.3" => "CN",
        "2.5.4.4" => "SN",
        "2.5.4.42" => "GN",
        "2.5.4.5" => "serialNumber",
        "2.5.4.6" => "C",
        "2.5.4.7" => "L",
        "2.5.4.8" => "ST",
        "2.5.4.9" => "street",
        "2.5.4.10" => "O",
        "2.5.4.11" => "OU",
        "2.5.4.12" => "title",
        "2.5.4.13" => "description",
        "2.5.4.17" => "postalCode",
        "0.9.2342.19200300.100.1.25" => "DC",
        "1.2.840.113549.1.9.1" => "emailAddress",
        _ => oid,
    }
}

/// `der::Any` 属性值 → 字符串（常见串类型逐一试解，BMP 兜底）。
fn atv_string(any: &der::Any) -> String {
    if let Ok(s) = any.decode_as::<der::asn1::Utf8StringRef>() {
        return s.as_str().to_owned();
    }
    if let Ok(s) = any.decode_as::<der::asn1::PrintableString>() {
        return s.as_str().to_owned();
    }
    if let Ok(s) = any.decode_as::<der::asn1::TeletexString>() {
        return s.as_str().to_owned();
    }
    if let Ok(s) = any.decode_as::<der::asn1::Ia5String>() {
        return s.as_str().to_owned();
    }
    String::from_utf8_lossy(any.value()).into_owned()
}

fn hex_upper(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 15) as usize] as char);
    }
    out
}

fn hex_colon(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(bytes.len() * 3);
    for (i, &b) in bytes.iter().enumerate() {
        if i > 0 {
            out.push(':');
        }
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 15) as usize] as char);
    }
    out
}

/// `__wjs_x509_parse(derU8)` → 证书 JSON（字段见 9e-1d；SAN/用法齐备；
/// 9i-3 增 `ca`（BasicConstraints）与 `spkiB64`（公钥重建），验签底座走 `__wjs_x509_verify`）。
pub unsafe extern "C" fn x509_parse(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: X509 needs DER bytes");
        return false;
    }
    let der = match view_bytes(&mut cx, frame.arg(0), "X509 DER") {
        Some(b) => b,
        None => return false,
    };
    use der::Decode as _;
    let cert = match x509_cert::Certificate::from_der(&der) {
        Ok(c) => c,
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: bad X.509 certificate ({e})"));
            return false;
        }
    };
    let tbs = cert.tbs_certificate();
    let mut subj_pairs: Vec<(String, String)> = Vec::new();
    for atv in tbs.subject().iter() {
        subj_pairs.push((atv_short(&atv.oid.to_string()).to_owned(), atv_string(&atv.value)));
    }
    let mut iss_pairs: Vec<(String, String)> = Vec::new();
    for atv in tbs.issuer().iter() {
        iss_pairs.push((atv_short(&atv.oid.to_string()).to_owned(), atv_string(&atv.value)));
    }
    let subject = subj_pairs.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("\n");
    let issuer = iss_pairs.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("\n");
    let serial = hex_upper(tbs.serial_number().as_bytes());
    let valid_from = fmt_asn1_time(tbs.validity().not_before.to_unix_duration().as_secs());
    let valid_to = fmt_asn1_time(tbs.validity().not_after.to_unix_duration().as_secs());
    use sha1::Digest as _; // 同 trait（digest 0.11），一次导入覆盖 sha2 系
    let fp = hex_colon(&sha1::Sha1::digest(&der));
    let fp256 = hex_colon(&sha2::Sha256::digest(&der));
    let fp512 = hex_colon(&sha2::Sha512::digest(&der));
    // 扩展：SAN / KeyUsage / ExtKeyUsage（缺席即 null/空，Node 同款）
    let mut san_dns: Vec<String> = Vec::new();
    let mut san_ip: Vec<String> = Vec::new();
    let mut san_email: Vec<String> = Vec::new();
    let mut san_uri: Vec<String> = Vec::new();
    let mut key_usage: Option<Vec<String>> = None;
    let mut ext_key_usage: Option<Vec<String>> = None;
    let mut ca = false;
    if let Some(exts) = tbs.extensions() {
        for ext in exts.iter() {
            let oid = ext.extn_id.to_string();
            let bytes = ext.extn_value.as_bytes();
            if oid == "2.5.29.17" {
                if let Ok(san) = x509_cert::ext::pkix::SubjectAltName::from_der(bytes) {
                    for name in san.0.iter() {
                        use x509_cert::ext::pkix::name::GeneralName;
                        match name {
                            GeneralName::DnsName(s) => san_dns.push(s.to_string()),
                            GeneralName::IpAddress(o) => {
                                let b = o.as_bytes();
                                if b.len() == 4 {
                                    san_ip.push(format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3]));
                                } else if b.len() == 16 {
                                    san_ip.push(b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(":"));
                                }
                            }
                            GeneralName::Rfc822Name(s) => san_email.push(s.to_string()),
                            GeneralName::UniformResourceIdentifier(s) => san_uri.push(s.to_string()),
                            _ => {}
                        }
                    }
                }
            } else if oid == "2.5.29.15" {
                if let Ok(ku) = x509_cert::ext::pkix::KeyUsage::from_der(bytes) {
                    use x509_cert::ext::pkix::KeyUsages;
                    let mut names = Vec::new();
                    if ku.0.contains(KeyUsages::DigitalSignature) {
                        names.push("Digital Signature".to_owned());
                    }
                    if ku.0.contains(KeyUsages::NonRepudiation) {
                        names.push("Non Repudiation".to_owned());
                    }
                    if ku.0.contains(KeyUsages::KeyEncipherment) {
                        names.push("Key Encipherment".to_owned());
                    }
                    if ku.0.contains(KeyUsages::DataEncipherment) {
                        names.push("Data Encipherment".to_owned());
                    }
                    if ku.0.contains(KeyUsages::KeyAgreement) {
                        names.push("Key Agreement".to_owned());
                    }
                    if ku.0.contains(KeyUsages::KeyCertSign) {
                        names.push("Key Cert Sign".to_owned());
                    }
                    if ku.0.contains(KeyUsages::CRLSign) {
                        names.push("CRL Sign".to_owned());
                    }
                    if ku.0.contains(KeyUsages::EncipherOnly) {
                        names.push("Encipher Only".to_owned());
                    }
                    if ku.0.contains(KeyUsages::DecipherOnly) {
                        names.push("Decipher Only".to_owned());
                    }
                    key_usage = Some(names);
                }
            } else if oid == "2.5.29.19" {
                // BasicConstraints（cA 缺省 false，Node `x509.ca` 同口径）
                if let Ok(bc) = x509_cert::ext::pkix::BasicConstraints::from_der(bytes) {
                    ca = bc.ca;
                }
            } else if oid == "2.5.29.37" {
                if let Ok(eku) = Vec::<der::asn1::ObjectIdentifier>::from_der(bytes) {
                    ext_key_usage = Some(eku.iter().map(|o| o.to_string()).collect());
                }
            }
        }
    }
    let mut san_parts: Vec<String> = Vec::new();
    for d in &san_dns {
        san_parts.push(format!("DNS:{d}"));
    }
    for e in &san_email {
        san_parts.push(format!("EMAIL:{e}"));
    }
    for u in &san_uri {
        san_parts.push(format!("URI:{u}"));
    }
    for i in &san_ip {
        san_parts.push(format!("IP Address:{i}"));
    }
    let obj_of = |pairs: &[(String, String)]| -> serde_json::Map<String, serde_json::Value> {
        pairs.iter().map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone()))).collect()
    };
    // SPKI DER（`x509.publicKey` 重建公钥 KeyObject 用；der::Encode 忠实重编）
    use der::Encode as _;
    let spki_b64 = {
        use base64::Engine as _;
        match tbs.subject_public_key_info().to_der() {
            Ok(d) => base64::engine::general_purpose::STANDARD.encode(d),
            Err(_) => {
                report_error(&mut cx, "TypeError: bad X.509 certificate");
                return false;
            }
        }
    };
    let json = serde_json::json!({
        "subject": subject,
        "issuer": issuer,
        "subjectObj": obj_of(&subj_pairs),
        "issuerObj": obj_of(&iss_pairs),
        "serialNumber": serial,
        "validFrom": valid_from,
        "validTo": valid_to,
        "fingerprint": fp,
        "fingerprint256": fp256,
        "fingerprint512": fp512,
        "sanDns": san_dns,
        "sanIp": san_ip,
        "sanEmail": san_email,
        "sanUri": san_uri,
        "subjectAltName": if san_parts.is_empty() { None } else { Some(san_parts.join(", ")) },
        "keyUsage": key_usage,
        "extKeyUsage": ext_key_usage,
        "ca": ca,
        "spkiB64": spki_b64,
    })
    .to_string();
    set_rval_str(&mut cx, &frame, &json);
    true
}

// ── 9i-7 X509 checkIssued（OpenSSL X509_check_issued 主体口径）─────────────
// 真机探针（node 26.8.2 + openssl 3.6 链固件）：同名不同钥 CA → false（AKID/SKID
// 是判别器）；leaf.checkIssued(leaf) → false（名字不匹）；无 AKID 的 leaf 回落名字。

/// TBS 内容按序取 `(issuer 裸 TLV, subject 裸 TLV)`（跳过可选 [0] 版本；
/// 裸 DER 名字比较 = X509_NAME_cmp 的 canonical 等价）。
pub(crate) fn x509_names_raw(der: &[u8]) -> Option<(&[u8], &[u8])> {
    let (_, ohl, _) = crate::builtins::crypto::der_tlv(der)?;
    let tbs = &der[ohl..];
    let (_, thl, tcl) = crate::builtins::crypto::der_tlv(tbs)?;
    let mut rest = &tbs[thl..thl + tcl];
    let mut children: Vec<&[u8]> = Vec::new();
    while !rest.is_empty() {
        let (_, hl, cl) = crate::builtins::crypto::der_tlv(rest)?;
        children.push(&rest[..hl + cl]);
        rest = &rest[hl + cl..];
    }
    let mut it = children.iter().copied();
    let mut serial: &[u8] = it.next()?;
    if serial[0] == 0xa0 {
        serial = it.next()?;
    }
    if serial[0] != 0x02 {
        return None;
    }
    let sig: &[u8] = it.next()?;
    if sig.first() != Some(&0x30) {
        return None;
    }
    let issuer: &[u8] = it.next()?;
    if issuer.first() != Some(&0x30) {
        return None;
    }
    it.next()?; // validity
    let subject: &[u8] = it.next()?;
    if subject.first() != Some(&0x30) {
        return None;
    }
    Some((issuer, subject))
}

/// SKI 扩展值（extnValue 内层 OCTET STRING 即 keyid）。
fn x509_ski(cert: &x509_cert::Certificate) -> Option<Vec<u8>> {
    use der::Decode as _;
    for ext in cert.tbs_certificate().extensions()?.iter() {
        if ext.extn_id.to_string() == "2.5.29.14" {
            if let Ok(os) = der::asn1::OctetString::from_der(ext.extn_value.as_bytes()) {
                return Some(os.as_bytes().to_vec());
            }
        }
    }
    None
}

/// checkIssued 实现：名字 DER 相等 + AKID.keyid 对 issuer SKI + issuer keyUsage 允许。
fn x509_check_issued_impl(der: &[u8], issuer_der: &[u8]) -> Result<bool, String> {
    use der::Decode as _;
    let cert = x509_cert::Certificate::from_der(der)
        .map_err(|_| "TypeError: bad X.509 certificate".to_string())?;
    let issuer_cert = x509_cert::Certificate::from_der(issuer_der)
        .map_err(|_| "TypeError: bad X.509 certificate".to_string())?;
    match (x509_names_raw(der), x509_names_raw(issuer_der)) {
        (Some((li, _)), Some((_, is))) if li == is => {}
        _ => return Ok(false),
    }
    if let Some(exts) = cert.tbs_certificate().extensions() {
        for ext in exts.iter() {
            if ext.extn_id.to_string() == "2.5.29.35" {
                if let Ok(akid) =
                    x509_cert::ext::pkix::AuthorityKeyIdentifier::from_der(ext.extn_value.as_bytes())
                {
                    if let Some(kid) = akid.key_identifier {
                        let Some(ski) = x509_ski(&issuer_cert) else {
                            return Ok(false);
                        };
                        if ski != kid.as_bytes() {
                            return Ok(false);
                        }
                    }
                }
            }
        }
    }
    if let Some(exts) = issuer_cert.tbs_certificate().extensions() {
        for ext in exts.iter() {
            if ext.extn_id.to_string() == "2.5.29.15" {
                if let Ok(ku) = x509_cert::ext::pkix::KeyUsage::from_der(ext.extn_value.as_bytes()) {
                    use x509_cert::ext::pkix::KeyUsages;
                    if !ku.0.contains(KeyUsages::KeyCertSign) {
                        return Ok(false);
                    }
                }
            }
        }
    }
    Ok(true)
}

/// `__wjs_x509_check_issued(certDer, issuerDer)` → boolean。
pub unsafe extern "C" fn x509_check_issued(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: X509 checkIssued needs cert and issuer");
        return false;
    }
    let (Some(der), Some(issuer_der)) = (
        view_bytes(&mut cx, frame.arg(0), "X509 cert"),
        view_bytes(&mut cx, frame.arg(1), "X509 issuer"),
    ) else {
        return false;
    };
    match x509_check_issued_impl(&der, &issuer_der) {
        Ok(ok) => {
            frame.set_rval(mozjs::jsval::BooleanValue(ok));
            true
        }
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}
