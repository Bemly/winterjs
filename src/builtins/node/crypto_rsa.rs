//! crypto rsa 域（v1.5/OAEP/签名；对齐 crypto.rs；纯搬移）。

use mozjs::jsval::JSVal;

use crate::jsapi_glue::{report_error, value_to_string, view_bytes, wrap_cx, Frame};
use super::crypto::{mgf1_sha1, opt_view, pad_be, set_rval_bytes, sha1_bytes};
use super::crypto_cipher::OsRng;

fn rsa_pub_from_der(der: &[u8]) -> Result<rsa::RsaPublicKey, String> {
    use rsa::pkcs8::DecodePublicKey as _;
    rsa::RsaPublicKey::from_public_key_der(der)
        .map_err(|_| "DataError: bad RSA public key (SPKI)".to_string())
}

fn rsa_priv_from_der(der: &[u8]) -> Result<rsa::RsaPrivateKey, String> {
    use rsa::pkcs8::DecodePrivateKey as _;
    rsa::RsaPrivateKey::from_pkcs8_der(der)
        .map_err(|_| "DataError: bad RSA private key (PKCS#8)".to_string())
}

/// `__wjs2_rsa_encrypt_v15(pubDerU8, dataU8)` → 密文（PKCS#1 v1.5）。
pub unsafe extern "C" fn rsa_encrypt_v15(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: RSA v1.5 encrypt needs key and data");
        return false;
    }
    let (Some(der), Some(data)) = (
        view_bytes(&mut cx, frame.arg(0), "RSA public key"),
        view_bytes(&mut cx, frame.arg(1), "RSA data"),
    ) else {
        return false;
    };
    let key = match rsa_pub_from_der(&der) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    match key.encrypt(&mut OsRng, rsa::Pkcs1v15Encrypt, &data) {
        Ok(ct) => set_rval_bytes(&mut cx, &frame, &ct),
        Err(e) => {
            report_error(&mut cx, &format!("OperationError: RSA encrypt failed: {e}"));
            false
        }
    }
}

/// `__wjs2_rsa_decrypt_v15(privDerU8, dataU8)` → 明文（PKCS#1 v1.5）。
pub unsafe extern "C" fn rsa_decrypt_v15(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: RSA v1.5 decrypt needs key and data");
        return false;
    }
    let (Some(der), Some(data)) = (
        view_bytes(&mut cx, frame.arg(0), "RSA private key"),
        view_bytes(&mut cx, frame.arg(1), "RSA data"),
    ) else {
        return false;
    };
    let key = match rsa_priv_from_der(&der) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    match key.decrypt(rsa::Pkcs1v15Encrypt, &data) {
        Ok(pt) => set_rval_bytes(&mut cx, &frame, &pt),
        Err(e) => {
            report_error(&mut cx, &format!("OperationError: RSA decrypt failed: {e}"));
            false
        }
    }
}

/// `__wjs2_node_rsa_oaep(pubDerU8, dataU8, labelOrNull, encNum)`：
/// OAEP-SHA1 加解密（enc=1 公钥加密 / enc=0 私钥解密）。
pub unsafe extern "C" fn node_rsa_oaep(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 {
        report_error(&mut cx, "TypeError: RSA-OAEP-SHA1 needs key, data, label and mode");
        return false;
    }
    let (Some(der), Some(data), Some(label)) = (
        view_bytes(&mut cx, frame.arg(0), "RSA key"),
        view_bytes(&mut cx, frame.arg(1), "RSA data"),
        opt_view(&mut cx, frame.arg(2), "RSA label"),
    ) else {
        return false;
    };
    let enc = !(frame.arg(3).is_number() && frame.arg(3).to_number() == 0.0);
    let label_ref = label.as_deref().unwrap_or(&[]);
    const HLEN: usize = 20;
    if enc {
        let key = match rsa_pub_from_der(&der) {
            Ok(k) => k,
            Err(e) => {
                report_error(&mut cx, &e);
                return false;
            }
        };
        use rsa::traits::PublicKeyParts as _;
        let k = key.size();
        if data.len() > k - 2 * HLEN - 2 {
            report_error(&mut cx, "ERR_OSSL_RSA_DATA_TOO_LARGE_FOR_KEY_SIZE: error:0200006E:rsa routines::data too large for key size");
            return false;
        }
        let lhash = sha1_bytes(label_ref);
        let ps_len = k - data.len() - 2 * HLEN - 2;
        let mut db = Vec::with_capacity(k - HLEN - 1);
        db.extend_from_slice(&lhash);
        db.extend(std::iter::repeat(0u8).take(ps_len));
        db.push(1);
        db.extend_from_slice(&data);
        let mut seed = vec![0u8; HLEN];
        if getrandom::fill(&mut seed).is_err() {
            report_error(&mut cx, "OperationError: cannot get random values");
            return false;
        }
        let db_mask = mgf1_sha1(&seed, k - HLEN - 1);
        let masked_db: Vec<u8> = db.iter().zip(db_mask.iter()).map(|(a, b)| a ^ b).collect();
        let seed_mask = mgf1_sha1(&masked_db, HLEN);
        let masked_seed: Vec<u8> =
            seed.iter().zip(seed_mask.iter()).map(|(a, b)| a ^ b).collect();
        let mut em = Vec::with_capacity(k);
        em.push(0);
        em.extend_from_slice(&masked_seed);
        em.extend_from_slice(&masked_db);
        let m = rsa::BigUint::from_bytes_be(&em);
        let c = m.modpow(key.e(), key.n());
        set_rval_bytes(&mut cx, &frame, &pad_be(&c.to_bytes_be(), k))
    } else {
        let key = match rsa_priv_from_der(&der) {
            Ok(k) => k,
            Err(e) => {
                report_error(&mut cx, &e);
                return false;
            }
        };
        use rsa::traits::{PrivateKeyParts as _, PublicKeyParts as _};
        let k = key.size();
        if data.len() != k {
            report_error(&mut cx, "OperationError: RSA decrypt failed: decryption error");
            return false;
        }
        let m = rsa::BigUint::from_bytes_be(&data);
        let em = pad_be(&m.modpow(key.d(), key.n()).to_bytes_be(), k);
        if em[0] != 0 {
            report_error(&mut cx, "OperationError: RSA decrypt failed: decryption error");
            return false;
        }
        let (masked_seed, masked_db) = (&em[1..1 + HLEN], &em[1 + HLEN..]);
        let seed_mask = mgf1_sha1(masked_db, HLEN);
        let seed: Vec<u8> =
            masked_seed.iter().zip(seed_mask.iter()).map(|(a, b)| a ^ b).collect();
        let db_mask = mgf1_sha1(&seed, k - HLEN - 1);
        let db: Vec<u8> =
            masked_db.iter().zip(db_mask.iter()).map(|(a, b)| a ^ b).collect();
        let lhash = sha1_bytes(label_ref);
        if db[..HLEN] != lhash {
            report_error(&mut cx, "OperationError: RSA decrypt failed: decryption error");
            return false;
        }
        let rest = &db[HLEN..];
        let one = rest.iter().position(|&b| b == 1);
        match one {
            Some(i) if rest[..i].iter().all(|&b| b == 0) => {
                set_rval_bytes(&mut cx, &frame, &rest[i + 1..])
            }
            _ => {
                report_error(&mut cx, "OperationError: RSA decrypt failed: decryption error");
                false
            }
        }
    }
}

/// OAEP-SHA1 编码（EME-OAEP-ENCODE）；返回 k 字节 EM（10f crypto二轮抽取，
/// 既有 `node_rsa_oaep` 主干不动，反向 native 复用）。
fn oaep_sha1_encode(k: usize, data: &[u8], label: &[u8]) -> Result<Vec<u8>, String> {
    const HLEN: usize = 20;
    if data.len() > k - 2 * HLEN - 2 {
        return Err("ERR_OSSL_RSA_DATA_TOO_LARGE_FOR_KEY_SIZE: error:0200006E:rsa routines::data too large for key size".to_string());
    }
    let lhash = sha1_bytes(label);
    let ps_len = k - data.len() - 2 * HLEN - 2;
    let mut db = Vec::with_capacity(k - HLEN - 1);
    db.extend_from_slice(&lhash);
    db.extend(std::iter::repeat(0u8).take(ps_len));
    db.push(1);
    db.extend_from_slice(data);
    let mut seed = vec![0u8; HLEN];
    if getrandom::fill(&mut seed).is_err() {
        return Err("OperationError: cannot get random values".to_string());
    }
    let db_mask = mgf1_sha1(&seed, k - HLEN - 1);
    let masked_db: Vec<u8> = db.iter().zip(db_mask.iter()).map(|(a, b)| a ^ b).collect();
    let seed_mask = mgf1_sha1(&masked_db, HLEN);
    let masked_seed: Vec<u8> =
        seed.iter().zip(seed_mask.iter()).map(|(a, b)| a ^ b).collect();
    let mut em = Vec::with_capacity(k);
    em.push(0);
    em.extend_from_slice(&masked_seed);
    em.extend_from_slice(&masked_db);
    Ok(em)
}

/// OAEP-SHA1 解码（em 须已定长 k）；返回明文（10f crypto二轮抽取，同上）。
fn oaep_sha1_decode(k: usize, em: &[u8], label: &[u8]) -> Result<Vec<u8>, String> {
    const HLEN: usize = 20;
    let err = || "OperationError: RSA decrypt failed: decryption error".to_string();
    if em.len() != k || em[0] != 0 {
        return Err(err());
    }
    let (masked_seed, masked_db) = (&em[1..1 + HLEN], &em[1 + HLEN..]);
    let seed_mask = mgf1_sha1(masked_db, HLEN);
    let seed: Vec<u8> =
        masked_seed.iter().zip(seed_mask.iter()).map(|(a, b)| a ^ b).collect();
    let db_mask = mgf1_sha1(&seed, k - HLEN - 1);
    let db: Vec<u8> =
        masked_db.iter().zip(db_mask.iter()).map(|(a, b)| a ^ b).collect();
    let lhash = sha1_bytes(label);
    if db[..HLEN] != lhash {
        return Err(err());
    }
    let rest = &db[HLEN..];
    let one = rest.iter().position(|&b| b == 1);
    match one {
        Some(i) if rest[..i].iter().all(|&b| b == 0) => Ok(rest[i + 1..].to_vec()),
        _ => Err(err()),
    }
}

/// v1.5 type-1 填充（签名式；10f crypto二轮，`privateEncrypt` 用）。
fn v15_type1_pad(k: usize, data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() > k - 11 {
        return Err("ERR_OSSL_RSA_DATA_TOO_LARGE_FOR_KEY_SIZE: error:0200006E:rsa routines::data too large for key size".to_string());
    }
    let mut em = vec![0u8; k];
    em[1] = 1;
    let ps_len = k - data.len() - 3;
    for b in &mut em[2..2 + ps_len] {
        *b = 0xFF;
    }
    em[k - data.len()..].copy_from_slice(data);
    Ok(em)
}

/// v1.5 type-1 去填充（10f crypto二轮，`publicDecrypt` 用）。
fn v15_type1_unpad(em: &[u8]) -> Result<Vec<u8>, String> {
    let err = || "OperationError: RSA decrypt failed: decryption error".to_string();
    if em.len() < 11 || em[0] != 0 || em[1] != 1 {
        return Err(err());
    }
    let rest = &em[2..];
    let one = rest.iter().position(|&b| b == 0).ok_or_else(err)?;
    if one < 8 {
        return Err(err());
    }
    Ok(rest[one + 1..].to_vec())
}

/// `__wjs2_node_rsa_oaep_flip(keyDerU8, dataU8, labelOrNull, privEncNum)`：
/// OAEP-SHA1 反向（privEnc=1 私钥加密 / 0 公钥解密；10f crypto二轮）。
pub unsafe extern "C" fn node_rsa_oaep_flip(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 {
        report_error(&mut cx, "TypeError: RSA-OAEP-SHA1 flip needs key, data, label and mode");
        return false;
    }
    let (Some(der), Some(data), Some(label)) = (
        view_bytes(&mut cx, frame.arg(0), "RSA key"),
        view_bytes(&mut cx, frame.arg(1), "RSA data"),
        opt_view(&mut cx, frame.arg(2), "RSA label"),
    ) else {
        return false;
    };
    let priv_enc = !(frame.arg(3).is_number() && frame.arg(3).to_number() == 0.0);
    let label_ref = label.as_deref().unwrap_or(&[]);
    if priv_enc {
        let key = match rsa_priv_from_der(&der) {
            Ok(k) => k,
            Err(e) => {
                report_error(&mut cx, &e);
                return false;
            }
        };
        use rsa::traits::{PrivateKeyParts as _, PublicKeyParts as _};
        let k = key.size();
        let em = match oaep_sha1_encode(k, &data, label_ref) {
            Ok(em) => em,
            Err(e) => {
                report_error(&mut cx, &e);
                return false;
            }
        };
        let m = rsa::BigUint::from_bytes_be(&em);
        let c = m.modpow(key.d(), key.n());
        set_rval_bytes(&mut cx, &frame, &pad_be(&c.to_bytes_be(), k))
    } else {
        let key = match rsa_pub_from_der(&der) {
            Ok(k) => k,
            Err(e) => {
                report_error(&mut cx, &e);
                return false;
            }
        };
        use rsa::traits::PublicKeyParts as _;
        let k = key.size();
        if data.len() != k {
            report_error(&mut cx, "OperationError: RSA decrypt failed: decryption error");
            return false;
        }
        let m = rsa::BigUint::from_bytes_be(&data);
        let em = pad_be(&m.modpow(key.e(), key.n()).to_bytes_be(), k);
        match oaep_sha1_decode(k, &em, label_ref) {
            Ok(pt) => set_rval_bytes(&mut cx, &frame, &pt),
            Err(e) => {
                report_error(&mut cx, &e);
                false
            }
        }
    }
}

/// `__wjs2_rsa_v15_flip(keyDerU8, dataU8, privEncNum)`：
/// v1.5 反向（1 私钥加密 / 0 公钥解密；10f crypto二轮）。
pub unsafe extern "C" fn rsa_v15_flip(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: RSA v1.5 flip needs key, data and mode");
        return false;
    }
    let (Some(der), Some(data)) = (
        view_bytes(&mut cx, frame.arg(0), "RSA key"),
        view_bytes(&mut cx, frame.arg(1), "RSA data"),
    ) else {
        return false;
    };
    let priv_enc = !(frame.arg(2).is_number() && frame.arg(2).to_number() == 0.0);
    if priv_enc {
        let key = match rsa_priv_from_der(&der) {
            Ok(k) => k,
            Err(e) => {
                report_error(&mut cx, &e);
                return false;
            }
        };
        use rsa::traits::{PrivateKeyParts as _, PublicKeyParts as _};
        let k = key.size();
        let em = match v15_type1_pad(k, &data) {
            Ok(em) => em,
            Err(e) => {
                report_error(&mut cx, &e);
                return false;
            }
        };
        let m = rsa::BigUint::from_bytes_be(&em);
        let c = m.modpow(key.d(), key.n());
        set_rval_bytes(&mut cx, &frame, &pad_be(&c.to_bytes_be(), k))
    } else {
        let key = match rsa_pub_from_der(&der) {
            Ok(k) => k,
            Err(e) => {
                report_error(&mut cx, &e);
                return false;
            }
        };
        use rsa::traits::PublicKeyParts as _;
        let k = key.size();
        if data.len() != k {
            report_error(&mut cx, "OperationError: RSA decrypt failed: decryption error");
            return false;
        }
        let m = rsa::BigUint::from_bytes_be(&data);
        let em = pad_be(&m.modpow(key.e(), key.n()).to_bytes_be(), k);
        match v15_type1_unpad(&em) {
            Ok(pt) => set_rval_bytes(&mut cx, &frame, &pt),
            Err(e) => {
                report_error(&mut cx, &e);
                false
            }
        }
    }
}

/// `__wjs2_rsa_raw(keyDerU8, dataU8, privNum)`：RSA 无填充裸运算
///（priv=1 私钥 d 次幂 / 0 公钥 e 次幂；10f crypto二轮，NO_PADDING 用）。
pub unsafe extern "C" fn rsa_raw(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: RSA raw needs key, data and mode");
        return false;
    }
    let (Some(der), Some(data)) = (
        view_bytes(&mut cx, frame.arg(0), "RSA key"),
        view_bytes(&mut cx, frame.arg(1), "RSA data"),
    ) else {
        return false;
    };
    let is_priv = !(frame.arg(2).is_number() && frame.arg(2).to_number() == 0.0);
    if is_priv {
        let key = match rsa_priv_from_der(&der) {
            Ok(k) => k,
            Err(e) => {
                report_error(&mut cx, &e);
                return false;
            }
        };
        use rsa::traits::{PrivateKeyParts as _, PublicKeyParts as _};
        let k = key.size();
        if data.len() > k {
            report_error(&mut cx, "ERR_OSSL_RSA_DATA_TOO_LARGE_FOR_KEY_SIZE: error:0200006E:rsa routines::data too large for key size");
            return false;
        }
        let m = rsa::BigUint::from_bytes_be(&data);
        let c = m.modpow(key.d(), key.n());
        set_rval_bytes(&mut cx, &frame, &pad_be(&c.to_bytes_be(), k))
    } else {
        let key = match rsa_pub_from_der(&der) {
            Ok(k) => k,
            Err(e) => {
                report_error(&mut cx, &e);
                return false;
            }
        };
        use rsa::traits::PublicKeyParts as _;
        let k = key.size();
        if data.len() > k {
            report_error(&mut cx, "ERR_OSSL_RSA_DATA_TOO_LARGE_FOR_KEY_SIZE: error:0200006E:rsa routines::data too large for key size");
            return false;
        }
        let m = rsa::BigUint::from_bytes_be(&data);
        let c = m.modpow(key.e(), key.n());
        set_rval_bytes(&mut cx, &frame, &pad_be(&c.to_bytes_be(), k))
    }
}
fn v15_prefix(hash: &str) -> Option<&'static [u8]> {
    match hash {
        "SHA-1" => Some(&[
            0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04,
            0x14,
        ]),
        "MD5" => Some(&[
            0x30, 0x20, 0x30, 0x0c, 0x06, 0x08, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x02, 0x05,
            0x05, 0x00, 0x04, 0x10,
        ]),
        _ => None,
    }
}

fn v15_digest(hash: &str, data: &[u8]) -> Option<Vec<u8>> {
    match hash {
        "SHA-1" => Some(sha1_bytes(data)),
        "MD5" => {
            use sha2::Digest as _;
            Some(md5::Md5::digest(data).to_vec())
        }
        _ => None,
    }
}

/// `__wjs2_node_rsa_v15_legacy(privDerU8, dataU8, hashStr)` → 签名（SHA-1/MD5）。
pub unsafe extern "C" fn node_rsa_v15_sign(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: RSA v1.5 legacy sign needs key, data and hash");
        return false;
    }
    let (Some(der), Some(data)) = (
        view_bytes(&mut cx, frame.arg(0), "RSA private key"),
        view_bytes(&mut cx, frame.arg(1), "RSA data"),
    ) else {
        return false;
    };
    let hash = value_to_string(&mut cx, frame.arg(2));
    let (Some(prefix), Some(digest)) = (v15_prefix(&hash), v15_digest(&hash, &data)) else {
        report_error(&mut cx, &format!("NotSupportedError: RSA legacy sign needs SHA-1/MD5"));
        return false;
    };
    let key = match rsa_priv_from_der(&der) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    use rsa::traits::{PrivateKeyParts as _, PublicKeyParts as _};
    let k = key.size();
    let mut t = Vec::with_capacity(prefix.len() + digest.len());
    t.extend_from_slice(prefix);
    t.extend_from_slice(&digest);
    if t.len() + 11 > k {
        report_error(&mut cx, "OperationError: RSA sign failed: key too short");
        return false;
    }
    let mut em = vec![0xffu8; k];
    em[0] = 0;
    em[1] = 1;
    em[k - t.len() - 1] = 0;
    em[k - t.len()..].copy_from_slice(&t);
    let s = rsa::BigUint::from_bytes_be(&em).modpow(key.d(), key.n());
    set_rval_bytes(&mut cx, &frame, &pad_be(&s.to_bytes_be(), k))
}

/// `__wjs2_node_rsa_v15_verify(pubDerU8, sigU8, dataU8, hashStr)` → boolean。
pub unsafe extern "C" fn node_rsa_v15_verify(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 {
        report_error(&mut cx, "TypeError: RSA v1.5 legacy verify needs key, signature, data and hash");
        return false;
    }
    let (Some(der), Some(sig), Some(data)) = (
        view_bytes(&mut cx, frame.arg(0), "RSA public key"),
        view_bytes(&mut cx, frame.arg(1), "RSA signature"),
        view_bytes(&mut cx, frame.arg(2), "RSA data"),
    ) else {
        return false;
    };
    let hash = value_to_string(&mut cx, frame.arg(3));
    let (Some(prefix), Some(digest)) = (v15_prefix(&hash), v15_digest(&hash, &data)) else {
        report_error(&mut cx, &format!("NotSupportedError: RSA legacy verify needs SHA-1/MD5"));
        return false;
    };
    let key = match rsa_pub_from_der(&der) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    use rsa::traits::PublicKeyParts as _;
    let k = key.size();
    if sig.len() != k {
        frame.set_rval(mozjs::jsval::BooleanValue(false));
        return true;
    }
    let m = rsa::BigUint::from_bytes_be(&sig);
    let em = pad_be(&m.modpow(key.e(), key.n()).to_bytes_be(), k);
    let mut t = Vec::with_capacity(prefix.len() + digest.len());
    t.extend_from_slice(prefix);
    t.extend_from_slice(&digest);
    let ok = em[0] == 0
        && em[1] == 1
        && em[k - t.len() - 1] == 0
        && em[2..k - t.len() - 1].iter().all(|&b| b == 0xff)
        && em[k - t.len()..] == t;
    frame.set_rval(mozjs::jsval::BooleanValue(ok));
    true
}

