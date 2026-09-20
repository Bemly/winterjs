//! crypto X.509 面（DER/TBS/算法 OID/证书验签）。

use super::common::*;
use super::ec::ec_curve_name;
use super::rsa::{rsa_pss_verify_manual, rsa_pub_from_der, rsa_v15_verify_manual};

use mozjs::jsval::{JSVal};
use crate::jsapi_glue::{report_error, value_to_string, view_bytes, wrap_cx, Frame};

// ── 9i-3 X.509 证书验签（node:crypto `X509Certificate.verify` 底座）─────────
// 口径：取 TBS 裸段（含自身 TLV）按签名算法 OID 哈希，复用既有验签底座；
// RSA 全档走手工 EMSA-PKCS1-v1_5（digest 0.11 直算，绕开 rsa 0.9 的 digest 0.10
// 版本墙，§4.43 同源取舍）；ECDSA 走 with_curve! + verify_prehash；
// Ed25519 纯签名。未知 OID / 验签不过一律 false（真机口径：错钥回 false 不抛）。

/// 外层 TLV 头解析：`(tag, header_len, content_len)`（DER 定长，禁 indefinite）。
pub(crate) fn der_tlv(der: &[u8]) -> Option<(u8, usize, usize)> {
    if der.len() < 2 {
        return None;
    }
    let tag = der[0];
    let b = der[1] as usize;
    let (hl, cl) = if b < 0x80 {
        (2, b)
    } else if b == 0x80 {
        return None; // indefinite 不属 DER
    } else {
        let n = b & 0x7f;
        if n > 4 || der.len() < 2 + n {
            return None;
        }
        let mut len = 0usize;
        for &byte in &der[2..2 + n] {
            len = (len << 8) | byte as usize;
        }
        (2 + n, len)
    };
    if der.len() < hl + cl {
        return None;
    }
    Some((tag, hl, cl))
}

/// 证书 DER → TBS 裸段（外层 SEQUENCE 后的第一个 TLV，含自身头，验签对象）。
pub(crate) fn x509_tbs_bytes(der: &[u8]) -> Option<&[u8]> {
    let (_, ohl, _) = der_tlv(der)?;
    let rest = &der[ohl..];
    let (tag, thl, tcl) = der_tlv(rest)?;
    if tag != 0x30 {
        return None;
    }
    Some(&rest[..thl + tcl])
}

/// X.509 签名算法 OID → `(哈希名, EMSA-PKCS1-v1_5 DigestInfo 前缀)`。
/// 返回 None 即不支持（含 RSA-PSS：参数携哈希，单独形，`x509_pss_params` 处理）。
pub(crate) fn x509_rsa_sig_hash(oid: &str) -> Option<(&'static str, &'static [u8])> {
    match oid {
        // md5WithRSAEncryption
        "1.2.840.113549.1.1.4" => Some(("MD5", &[
            0x30, 0x20, 0x30, 0x0c, 0x06, 0x08, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x02, 0x05, 0x05, 0x00, 0x04, 0x10,
        ])),
        // sha1WithRSAEncryption
        "1.2.840.113549.1.1.5" => Some(("SHA-1", &[
            0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04, 0x14,
        ])),
        // sha224WithRSAEncryption
        "1.2.840.113549.1.1.14" => Some(("SHA-224", &[
            0x30, 0x2d, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x04, 0x05, 0x00, 0x04, 0x1c,
        ])),
        // sha256WithRSAEncryption
        "1.2.840.113549.1.1.11" => Some(("SHA-256", &[
            0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05, 0x00, 0x04, 0x20,
        ])),
        // sha384WithRSAEncryption
        "1.2.840.113549.1.1.12" => Some(("SHA-384", &[
            0x30, 0x41, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02, 0x05, 0x00, 0x04, 0x30,
        ])),
        // sha512WithRSAEncryption
        "1.2.840.113549.1.1.13" => Some(("SHA-512", &[
            0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03, 0x05, 0x00, 0x04, 0x40,
        ])),
        _ => None,
    }
}

/// ECDSA 签名算法 OID → 哈希名（ecdsa-with-SHA*）。
pub(crate) fn x509_ecdsa_sig_hash(oid: &str) -> Option<&'static str> {
    match oid {
        "1.2.840.10045.4.1" => Some("SHA-1"),
        "1.2.840.10045.4.3.1" => Some("SHA-224"),
        "1.2.840.10045.4.3.2" => Some("SHA-256"),
        "1.2.840.10045.4.3.3" => Some("SHA-384"),
        "1.2.840.10045.4.3.4" => Some("SHA-512"),
        _ => None,
    }
}

/// 哈希算法 OID（DER 内容字节）→ 哈希名（PSS 参数用）。
pub(crate) fn x509_hash_oid_name(oid: &[u8]) -> Option<&'static str> {
    match oid {
        [0x2b, 0x0e, 0x03, 0x02, 0x1a] => Some("SHA-1"),
        [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x04] => Some("SHA-224"),
        [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01] => Some("SHA-256"),
        [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02] => Some("SHA-384"),
        [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03] => Some("SHA-512"),
        _ => None,
    }
}

/// X.509 RSA-PSS 参数（RFC 4055 `RSASSA-PSS-params`；openssl 实测 EXPLICIT
/// 上下文标签：[0] SEQ{OID,NULL}、[1] SEQ{OID mgf1, SEQ{OID,NULL}}、[2] INTEGER）。
/// 缺省 sha1 / mgf1(sha1) / 20；trailerField 忽略（恒 1）。解析不出即 None。
pub(crate) fn x509_pss_params(params: &der::Any) -> Option<(&'static str, &'static str, usize)> {
    let mut hash = "SHA-1";
    let mut mgf = "SHA-1";
    let mut salt = 20usize;
    let mut rest: &[u8] = params.value();
    while !rest.is_empty() {
        let (tag, hl, cl) = der_tlv(rest)?;
        let item = &rest[hl..hl + cl];
        rest = &rest[hl + cl..];
        match tag {
            0xa0 => {
                let (t, h2, c2) = der_tlv(item)?;
                if t != 0x30 {
                    return None;
                }
                let seq = &item[h2..h2 + c2];
                let (t2, h3, c3) = der_tlv(seq)?;
                if t2 != 0x06 {
                    return None;
                }
                hash = x509_hash_oid_name(&seq[h3..h3 + c3])?;
            }
            0xa1 => {
                let (t, h2, c2) = der_tlv(item)?;
                if t != 0x30 {
                    return None;
                }
                let seq = &item[h2..h2 + c2];
                let (t2, h3, c3) = der_tlv(seq)?;
                if t2 != 0x06 {
                    return None;
                }
                // mgf1（1.2.840.113549.1.1.8）之外不认
                if seq[h3..h3 + c3] != [0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x08] {
                    return None;
                }
                let (t3, h4, c4) = der_tlv(&seq[h3 + c3..])?;
                if t3 != 0x30 {
                    return None;
                }
                let inner = &seq[h3 + c3..][h4..h4 + c4];
                let (t4, h5, c5) = der_tlv(inner)?;
                if t4 != 0x06 {
                    return None;
                }
                mgf = x509_hash_oid_name(&inner[h5..h5 + c5])?;
            }
            0xa2 | 0x82 => {
                // saltLength：EXPLICIT INTEGER（openssl 形）或 IMPLICIT 原生
                let bytes: &[u8] = if tag == 0xa2 {
                    let (t, h2, c2) = der_tlv(item)?;
                    if t != 0x02 {
                        return None;
                    }
                    &item[h2..h2 + c2]
                } else {
                    item
                };
                if bytes.is_empty() || bytes.len() > 4 || (bytes[0] == 0 && bytes.len() > 1) {
                    return None;
                }
                let mut v = 0usize;
                for &b in bytes {
                    v = (v << 8) | b as usize;
                }
                salt = v;
            }
            _ => {}
        }
    }
    Some((hash, mgf, salt))
}

/// X.509 ECDSA 签名（DER `SEQ{r,s}`）→ 定长裸 `r‖s`（verify_prehash 用）。
pub(crate) fn der_ecdsa_sig_to_raw(sig: &[u8], size: usize) -> Option<Vec<u8>> {
    let (tag, hl, cl) = der_tlv(sig)?;
    if tag != 0x30 {
        return None;
    }
    let mut pos = hl;
    let end = hl + cl;
    let mut out = Vec::with_capacity(size * 2);
    for _ in 0..2 {
        let (t, ihl, icl) = der_tlv(&sig[pos..end])?;
        if t != 0x02 {
            return None;
        }
        let mut v = sig[pos + ihl..pos + ihl + icl].to_vec();
        while v.len() > 1 && v[0] == 0x00 {
            v.remove(0);
        }
        if v.is_empty() || v.len() > size {
            return None;
        }
        out.extend(std::iter::repeat_n(0u8, size - v.len()));
        out.extend_from_slice(&v);
        pos += ihl + icl;
    }
    if pos != end {
        return None;
    }
    Some(out)
}

/// `__wjs_x509_verify(certDer, keyBytes, keyType)` → boolean。
/// keyBytes：rsa/rsa-pss/ec 为 SPKI DER，ed25519 为裸 32B，ed448 为裸 57B（10e）；
/// 其余 keyType 一律 false
/// （真机口径：错钥/异族 → false 不抛，private 入参的拒绝在 JS 壳做）。
pub unsafe extern "C" fn x509_verify(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: X509 verify needs cert, key and key type");
        return false;
    }
    let (Some(der), Some(key)) = (
        view_bytes(&mut cx, frame.arg(0), "X509 cert"),
        view_bytes(&mut cx, frame.arg(1), "X509 key"),
    ) else {
        return false;
    };
    let key_type = value_to_string(&mut cx, frame.arg(2));
    let out = x509_verify_impl(&der, &key, &key_type);
    match out {
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

/// 验签实现（native 的纯逻辑核，单测覆盖）。
fn x509_verify_impl(der: &[u8], key: &[u8], key_type: &str) -> Result<bool, String> {
    use der::Decode as _;
    let cert = x509_cert::Certificate::from_der(der)
        .map_err(|_| "TypeError: bad X.509 certificate".to_string())?;
    let Some(tbs) = x509_tbs_bytes(der) else {
        return Err("TypeError: bad X.509 certificate".into());
    };
    if cert.signature().unused_bits() != 0 {
        return Ok(false);
    }
    // der BitString::as_bytes：unused_bits 非 8 的倍数时 None（上面已挡）
    let Some(sig) = cert.signature().as_bytes() else {
        return Ok(false);
    };
    let oid = cert.signature_algorithm().oid.to_string();
    match key_type {
        "rsa" | "rsa-pss" => {
            let pub_key = rsa_pub_from_der(key)?;
            if oid == "1.2.840.113549.1.1.10" {
                // 9i-8：rsassaPss——哈希/MGF/salt 全从证书参数取。
                let Some(ref params) = cert.signature_algorithm().parameters else {
                    return Ok(false);
                };
                let Some((hash, mgf_hash, salt_len)) = x509_pss_params(&params) else {
                    return Ok(false);
                };
                let m_hash = x509_digest(hash, tbs)?;
                return Ok(rsa_pss_verify_manual(&pub_key, hash, mgf_hash, salt_len, &m_hash, sig));
            }
            let Some((hash, prefix)) = x509_rsa_sig_hash(&oid) else {
                return Ok(false);
            };
            let digest = x509_digest(hash, tbs)?;
            Ok(rsa_v15_verify_manual(&pub_key, prefix, &digest, sig))
        }
        "ec" => {
            let Some(hash) = x509_ecdsa_sig_hash(&oid) else {
                return Ok(false);
            };
            let curve = ec_curve_name(key);
            if curve.is_empty() {
                return Ok(false);
            }
            let Some(size) = curve_size(curve) else {
                return Ok(false);
            };
            let Some(raw) = der_ecdsa_sig_to_raw(sig, size) else {
                return Ok(false);
            };
            let digest = x509_digest(hash, tbs)?;
            let out: Result<bool, String> = with_curve!(curve, |C, Secret, Public, Signing, Verifying, Sig, K| {
                (|| -> Result<bool, String> {
                    use K::ecdsa::signature::hazmat::PrehashVerifier as _;
                    use K::elliptic_curve::pkcs8::DecodePublicKey as _;
                    let pk = Public::from_public_key_der(key)
                        .map_err(|_| "DataError: bad ECDSA public key (SPKI)".to_string())?;
                    let vk = Verifying::from_sec1_bytes(&pk.to_sec1_bytes())
                        .map_err(|_| "DataError: bad ECDSA public key".to_string())?;
                    let sig = Sig::try_from(raw.as_slice())
                        .map_err(|_| "OperationError: bad ECDSA signature".to_string())?;
                    // 高 S 归一（OpenSSL 接受可锻造签名，§4.55 同口径）。
                    let sig = sig.normalize_s();
                    Ok(vk.verify_prehash(&digest, &sig).is_ok())
                })()
            });
            out
        }
        "ed25519" => {
            if oid != "1.3.101.112" {
                return Ok(false);
            }
            let Ok(publ) = <[u8; 32]>::try_from(key) else {
                return Ok(false);
            };
            let Ok(sigb) = <[u8; 64]>::try_from(sig) else {
                return Ok(false);
            };
            Ok((|| {
                use ed25519_dalek::Verifier as _;
                let vk = ed25519_dalek::VerifyingKey::from_bytes(&publ).ok()?;
                let sig = ed25519_dalek::Signature::from(sigb);
                vk.verify(tbs, &sig).is_ok().then_some(true)
            })()
            .unwrap_or(false))
        }
        // 10e Ed448（OID 1.3.101.113；57B 钥/114B 签，纯签名）。
        "ed448" => {
            if oid != "1.3.101.113" {
                return Ok(false);
            }
            let (Ok(publ), Ok(sigb)) = (
                <[u8; 57]>::try_from(key),
                <[u8; 114]>::try_from(sig),
            ) else {
                return Ok(false);
            };
            Ok((|| {
                let vk = ed448_goldilocks::VerifyingKey::from_bytes(&publ).ok()?;
                let sig = ed448_goldilocks::Signature::from_slice(&sigb).ok()?;
                vk.verify_raw(&sig, tbs).ok()
            })()
            .is_some())
        }
        kt if kt.starts_with("ml-dsa-") => {
            // 9i-6：证书签名 OID 与密钥 OID 同族（2.16.840.1.101.3.4.3.17/18/19），纯签名。
            let Some((_, pk)) = crate::builtins::node::crypto::mldsa_spki_pk(key) else {
                return Ok(false);
            };
            let cert_oid = crate::builtins::node::crypto::mldsa_kind_by_oid_str(&oid);
            Ok(cert_oid == Some(kt) && crate::builtins::node::crypto::mldsa_verify_core(kt, &pk, sig, tbs))
        }
        _ => Ok(false),
    }
}
