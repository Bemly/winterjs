//! crypto Edwards 面（Ed25519/Ed448 + OKP 编解码）。

use super::common::*;

use mozjs::context::JSContext;
use mozjs::jsval::{JSVal};
use crate::jsapi_glue::{report_error, value_to_string, view_bytes, wrap_cx, Frame};

/// RFC 8410 定长 DER 编解码（Ed25519 oid …70 / X25519 …6e；纯函数，可单测）。
/// PKCS#8: `30 2E 02 01 00 30 05 06 03 2B 65 OID 04 22 04 20 <32B>`；
/// SPKI: `30 2A 30 05 06 03 2B 65 OID 03 21 00 <32B>`。
/// 10e Ed448（oid …71，57B）：PKCS#8 `30 47 … 04 3B 04 39 <57B>`（73B）；
/// SPKI `30 43 … 03 3A 00 <57B>`（69B；真机逐字节对）。
fn okp_oid_byte(kind: &str) -> Result<u8, String> {
    // prelude 统一传大写名（generateKey 内 `toUpperCase`）；此处按大写匹配
    match kind.to_ascii_uppercase().as_str() {
        "ED25519" => Ok(0x70),
        "X25519" => Ok(0x6E),
        "ED448" => Ok(0x71),
        "X448" => Ok(0x6F),
        _ => Err(format!("NotSupportedError: unsupported OKP key '{kind}'")),
    }
}

/// OKP 裸密钥长（Ed25519/X25519 32B；X448 56B；Ed448 57B）。
fn okp_key_len(kind: &str) -> Result<usize, String> {
    match kind.to_ascii_uppercase().as_str() {
        "ED25519" | "X25519" => Ok(32),
        "X448" => Ok(56),
        "ED448" => Ok(57),
        _ => Err(format!("NotSupportedError: unsupported OKP key '{kind}'")),
    }
}

pub(crate) fn okp_wrap_pkcs8(kind: &str, seed: &[u8]) -> Result<Vec<u8>, String> {
    let oid = okp_oid_byte(kind)?;
    let n = okp_key_len(kind)?;
    if seed.len() != n {
        return Err(format!("DataError: bad {kind} seed (must be {n} bytes)"));
    }
    // 三档同构：`30 (14+n){02 01 00 30 05 06 03 2B 65 {oid} 04 (n+2) 04 n}`——
    // 32B 档 0x2E、56B 档 0x46（10f X448）、57B 档 0x47（10e Ed448）。
    let mut v = vec![
        0x30,
        (14 + n) as u8,
        0x02,
        0x01,
        0x00,
        0x30,
        0x05,
        0x06,
        0x03,
        0x2B,
        0x65,
        oid,
        0x04,
        (n + 2) as u8,
        0x04,
        n as u8,
    ];
    v.extend_from_slice(seed);
    Ok(v)
}

pub(crate) fn okp_wrap_spki(kind: &str, publ: &[u8]) -> Result<Vec<u8>, String> {
    let oid = okp_oid_byte(kind)?;
    let n = okp_key_len(kind)?;
    if publ.len() != n {
        return Err(format!("DataError: bad {kind} public key (must be {n} bytes)"));
    }
    // 三档同构：`30 (10+n){30 05 06 03 2B 65 {oid} 03 (n+1) 00}`（BIT STRING
    // 含 1 个 unused-bits 字节）。
    let mut v = vec![
        0x30,
        (10 + n) as u8,
        0x30,
        0x05,
        0x06,
        0x03,
        0x2B,
        0x65,
        oid,
        0x03,
        (n + 1) as u8,
        0x00,
    ];
    v.extend_from_slice(publ);
    Ok(v)
}

pub(crate) fn okp_unwrap_pkcs8(kind: &str, der: &[u8]) -> Result<Vec<u8>, String> {
    let oid = okp_oid_byte(kind)?;
    let n = okp_key_len(kind)?;
    const HEAD_LEN: usize = 16;
    let mut want = vec![
        0x30,
        (14 + n) as u8,
        0x02,
        0x01,
        0x00,
        0x30,
        0x05,
        0x06,
        0x03,
        0x2B,
        0x65,
        oid,
        0x04,
        (n + 2) as u8,
        0x04,
        n as u8,
    ];
    want.extend_from_slice(&vec![0u8; n]);
    if der.len() != HEAD_LEN + n || der[..HEAD_LEN] != want[..HEAD_LEN] {
        return Err(format!("DataError: bad {kind} private key (PKCS#8)"));
    }
    Ok(der[HEAD_LEN..].to_vec())
}

pub(crate) fn okp_unwrap_spki(kind: &str, der: &[u8]) -> Result<Vec<u8>, String> {
    let oid = okp_oid_byte(kind)?;
    let n = okp_key_len(kind)?;
    let head_len = 12;
    let prefix = vec![
        0x30,
        (10 + n) as u8,
        0x30,
        0x05,
        0x06,
        0x03,
        0x2B,
        0x65,
        oid,
        0x03,
        (n + 1) as u8,
        0x00,
    ];
    if der.len() != head_len + n || der[..head_len] != prefix[..] {
        return Err(format!("DataError: bad {kind} public key (SPKI)"));
    }
    Ok(der[head_len..].to_vec())
}

fn okp_kind_arg(cx: &mut JSContext, frame: &Frame, idx: u32) -> Option<String> {
    let kind = value_to_string(cx, frame.arg(idx));
    match okp_oid_byte(&kind) {
        Ok(_) => Some(kind),
        Err(e) => {
            report_error(cx, &e);
            None
        }
    }
}

/// `__wjs2_okp_pkcs8_from_seed(kind, seedU8)` → PKCS#8 DER。
pub unsafe extern "C" fn okp_pkcs8_from_seed(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: OKP export needs kind and seed");
        return false;
    }
    let (Some(kind), Some(seed)) = (
        okp_kind_arg(&mut cx, &frame, 0),
        view_bytes(&mut cx, frame.arg(1), "OKP seed"),
    ) else {
        return false;
    };
    match okp_wrap_pkcs8(&kind, &seed) {
        Ok(der) => set_rval_bytes(&mut cx, &frame, &der),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs2_okp_spki_from_pub(kind, pubU8)` → SPKI DER。
pub unsafe extern "C" fn okp_spki_from_pub(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: OKP export needs kind and public key");
        return false;
    }
    let (Some(kind), Some(publ)) = (
        okp_kind_arg(&mut cx, &frame, 0),
        view_bytes(&mut cx, frame.arg(1), "OKP public key"),
    ) else {
        return false;
    };
    match okp_wrap_spki(&kind, &publ) {
        Ok(der) => set_rval_bytes(&mut cx, &frame, &der),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs2_okp_seed_from_pkcs8(kind, derU8)` → 32B seed。
pub unsafe extern "C" fn okp_seed_from_pkcs8(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: OKP import needs kind and key bytes");
        return false;
    }
    let (Some(kind), Some(der)) = (
        okp_kind_arg(&mut cx, &frame, 0),
        view_bytes(&mut cx, frame.arg(1), "OKP private key"),
    ) else {
        return false;
    };
    match okp_unwrap_pkcs8(&kind, &der) {
        Ok(seed) => set_rval_bytes(&mut cx, &frame, &seed),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs2_okp_pub_from_spki(kind, derU8)` → 32B pub。
pub unsafe extern "C" fn okp_pub_from_spki(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: OKP import needs kind and key bytes");
        return false;
    }
    let (Some(kind), Some(der)) = (
        okp_kind_arg(&mut cx, &frame, 0),
        view_bytes(&mut cx, frame.arg(1), "OKP public key"),
    ) else {
        return false;
    };
    match okp_unwrap_spki(&kind, &der) {
        Ok(publ) => set_rval_bytes(&mut cx, &frame, &publ),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs2_ed_generate()` → 32B seed。
pub unsafe extern "C" fn ed_generate(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if !rng_probe(&mut cx) {
        return false;
    }
    let mut seed = [0u8; 32];
    if getrandom::fill(&mut seed).is_err() {
        report_error(&mut cx, "OperationError: cannot get random values");
        return false;
    }
    tracing::debug!(target: "winterjs2::crypto", "Ed25519 key generated");
    set_rval_bytes(&mut cx, &frame, &seed)
}

/// `__wjs2_ed_public(seedU8)` → 32B pub。
pub unsafe extern "C" fn ed_public(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: Ed25519 public needs a seed");
        return false;
    }
    let Some(seed) = view_bytes(&mut cx, frame.arg(0), "Ed25519 seed") else {
        return false;
    };
    let Ok(seed) = <[u8; 32]>::try_from(seed.as_slice()) else {
        report_error(&mut cx, "DataError: bad Ed25519 seed (must be 32 bytes)");
        return false;
    };
    let publ = ed25519_dalek::SigningKey::from_bytes(&seed).verifying_key().to_bytes();
    set_rval_bytes(&mut cx, &frame, &publ)
}

/// `__wjs2_ed_sign(seedU8, dataU8)` → 64B 签名。
pub unsafe extern "C" fn ed_sign(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: Ed25519 sign needs seed and data");
        return false;
    }
    let (Some(seed), Some(data)) = (
        view_bytes(&mut cx, frame.arg(0), "Ed25519 seed"),
        view_bytes(&mut cx, frame.arg(1), "Ed25519 data"),
    ) else {
        return false;
    };
    let Ok(seed) = <[u8; 32]>::try_from(seed.as_slice()) else {
        report_error(&mut cx, "DataError: bad Ed25519 seed (must be 32 bytes)");
        return false;
    };
    use ed25519_dalek::Signer as _;
    let sig = ed25519_dalek::SigningKey::from_bytes(&seed).sign(&data);
    set_rval_bytes(&mut cx, &frame, &sig.to_bytes())
}

/// `__wjs2_ed_verify(pubU8, sigU8, dataU8)` → boolean。
pub unsafe extern "C" fn ed_verify(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: Ed25519 verify needs key, signature and data");
        return false;
    }
    let (Some(publ), Some(sig), Some(data)) = (
        view_bytes(&mut cx, frame.arg(0), "Ed25519 public key"),
        view_bytes(&mut cx, frame.arg(1), "Ed25519 signature"),
        view_bytes(&mut cx, frame.arg(2), "Ed25519 data"),
    ) else {
        return false;
    };
    let (Ok(publ), Ok(sig)) = (
        <[u8; 32]>::try_from(publ.as_slice()),
        <[u8; 64]>::try_from(sig.as_slice()),
    ) else {
        report_error(&mut cx, "DataError: bad Ed25519 key/signature length");
        return false;
    };
    let ok = (|| {
        use ed25519_dalek::Verifier as _;
        let vk = ed25519_dalek::VerifyingKey::from_bytes(&publ).ok()?;
        let sig = ed25519_dalek::Signature::try_from(sig.as_slice()).ok()?;
        vk.verify(&data, &sig).is_ok().then_some(true)
    })();
    match ok {
        Some(true) => {
            frame.set_rval(mozjs::jsval::BooleanValue(true));
            true
        }
        // 非法点/验签失败一律 false（WebCrypto 口径：verify 不抛，只回布尔）
        _ => {
            frame.set_rval(mozjs::jsval::BooleanValue(false));
            true
        }
    }
}

// ── 10e Ed448（`ed448-goldilocks =0.14.0-pre.15` 特批钉版；RFC 8032 纯签名）───
// 口径：seed 57B（`ed25519` 32B 的放大版）；签名 114B 确定性档；验签失败回
// false（`ed_verify` 同款）；DER 经上方 `okp_*` 57B 分支（真机逐字节对）。

/// `__wjs2_ed448_generate()` → 57B seed。
pub unsafe extern "C" fn ed448_generate(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if !rng_probe(&mut cx) {
        return false;
    }
    let mut seed = [0u8; 57];
    if getrandom::fill(&mut seed).is_err() {
        report_error(&mut cx, "OperationError: cannot get random values");
        return false;
    }
    tracing::debug!(target: "winterjs2::crypto", "Ed448 key generated");
    set_rval_bytes(&mut cx, &frame, &seed)
}

/// `__wjs2_ed448_public(seedU8)` → 57B pub。
pub unsafe extern "C" fn ed448_public(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: Ed448 public needs a seed");
        return false;
    }
    let Some(seed) = view_bytes(&mut cx, frame.arg(0), "Ed448 seed") else {
        return false;
    };
    let Ok(sk) = ed448_goldilocks::SigningKey::try_from(seed.as_slice()) else {
        report_error(&mut cx, "DataError: bad Ed448 seed (must be 57 bytes)");
        return false;
    };
    let vk = sk.verifying_key();
    let publ: &[u8] = vk.as_ref();
    set_rval_bytes(&mut cx, &frame, publ)
}

/// `__wjs2_ed448_sign(seedU8, dataU8)` → 114B 签名（确定性纯签名，真机同款）。
pub unsafe extern "C" fn ed448_sign(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: Ed448 sign needs seed and data");
        return false;
    }
    let (Some(seed), Some(data)) = (
        view_bytes(&mut cx, frame.arg(0), "Ed448 seed"),
        view_bytes(&mut cx, frame.arg(1), "Ed448 data"),
    ) else {
        return false;
    };
    let Ok(sk) = ed448_goldilocks::SigningKey::try_from(seed.as_slice()) else {
        report_error(&mut cx, "DataError: bad Ed448 seed (must be 57 bytes)");
        return false;
    };
    let sig = sk.sign_raw(&data);
    set_rval_bytes(&mut cx, &frame, &sig.to_bytes())
}

/// `__wjs2_ed448_verify(pubU8, sigU8, dataU8)` → boolean。
pub unsafe extern "C" fn ed448_verify(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: Ed448 verify needs key, signature and data");
        return false;
    }
    let (Some(publ), Some(sig), Some(data)) = (
        view_bytes(&mut cx, frame.arg(0), "Ed448 public key"),
        view_bytes(&mut cx, frame.arg(1), "Ed448 signature"),
        view_bytes(&mut cx, frame.arg(2), "Ed448 data"),
    ) else {
        return false;
    };
    let (Ok(publ), Ok(sig)) = (
        <[u8; 57]>::try_from(publ.as_slice()),
        <[u8; 114]>::try_from(sig.as_slice()),
    ) else {
        report_error(&mut cx, "DataError: bad Ed448 key/signature length");
        return false;
    };
    let ok = (|| {
        let vk = ed448_goldilocks::VerifyingKey::from_bytes(&publ).ok()?;
        let sig = ed448_goldilocks::Signature::from_slice(&sig).ok()?;
        vk.verify_raw(&sig, &data).ok()
    })();
    match ok {
        Some(()) => {
            frame.set_rval(mozjs::jsval::BooleanValue(true));
            true
        }
        // 非法点/验签失败一律 false（`ed_verify` 同款口径）
        _ => {
            frame.set_rval(mozjs::jsval::BooleanValue(false));
            true
        }
    }
}
