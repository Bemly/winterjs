//! crypto XDH 面（X25519/X448）。

use super::common::*;

use mozjs::jsval::{JSVal};
use crate::jsapi_glue::{report_error, view_bytes, wrap_cx, Frame};

/// `__wjs_x_generate()` → 32B 私钥。
pub unsafe extern "C" fn x_generate(
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
    let mut privb = [0u8; 32];
    if getrandom::fill(&mut privb).is_err() {
        report_error(&mut cx, "OperationError: cannot get random values");
        return false;
    }
    tracing::debug!(target: "winterjs::crypto", "X25519 key generated");
    set_rval_bytes(&mut cx, &frame, &privb)
}

/// `__wjs_x_public(privU8)` → 32B pub。
pub unsafe extern "C" fn x_public(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: X25519 public needs a private key");
        return false;
    }
    let Some(privb) = view_bytes(&mut cx, frame.arg(0), "X25519 private key") else {
        return false;
    };
    let Ok(privb) = <[u8; 32]>::try_from(privb.as_slice()) else {
        report_error(&mut cx, "DataError: bad X25519 private key (must be 32 bytes)");
        return false;
    };
    let publ = x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::from(privb));
    set_rval_bytes(&mut cx, &frame, publ.as_bytes())
}

/// `__wjs_x_derive(privU8, pubU8)` → 32B 共享秘密（u 坐标原样，WebCrypto 口径）。
pub unsafe extern "C" fn x_derive(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: X25519 derive needs private and public keys");
        return false;
    }
    let (Some(privb), Some(publ)) = (
        view_bytes(&mut cx, frame.arg(0), "X25519 private key"),
        view_bytes(&mut cx, frame.arg(1), "X25519 public key"),
    ) else {
        return false;
    };
    let (Ok(privb), Ok(publ)) = (
        <[u8; 32]>::try_from(privb.as_slice()),
        <[u8; 32]>::try_from(publ.as_slice()),
    ) else {
        report_error(&mut cx, "DataError: bad X25519 key length");
        return false;
    };
    let secret = x25519_dalek::StaticSecret::from(privb)
        .diffie_hellman(&x25519_dalek::PublicKey::from(publ));
    set_rval_bytes(&mut cx, &frame, secret.as_bytes())
}

/// `__wjs_x448_generate()` → 56B 私钥（node 口径：生成即 RFC 7748 clamp，
/// raw-private 导出与真机同形——`b[0] &= 252; b[55] |= 128`）。
pub unsafe extern "C" fn x448_generate(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let mut privb = [0u8; 56];
    if getrandom::fill(&mut privb).is_err() {
        report_error(&mut cx, "OperationError: cannot get random values");
        return false;
    }
    privb[0] &= 252;
    privb[55] |= 128;
    tracing::debug!(target: "winterjs::crypto", "X448 key generated");
    set_rval_bytes(&mut cx, &frame, &privb)
}

/// `__wjs_x448_public(privU8)` → 56B pub。
pub unsafe extern "C" fn x448_public(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: X448 public needs a private key");
        return false;
    }
    let Some(privb) = view_bytes(&mut cx, frame.arg(0), "X448 private key") else {
        return false;
    };
    let Ok(privb) = <[u8; 56]>::try_from(privb.as_slice()) else {
        report_error(&mut cx, "DataError: bad X448 private key (must be 56 bytes)");
        return false;
    };
    let publ = x448::PublicKey::from(&x448::StaticSecret::from(privb));
    set_rval_bytes(&mut cx, &frame, publ.as_bytes())
}

/// 纯函数 X448 DH（RFC 7748：clamp 在内；低阶点/长度错 → None）。
/// native 与单测共用（`x448` 轮子自带低阶点检查）。
pub(crate) fn x448_dh(privb: &[u8], publ: &[u8]) -> Option<[u8; 56]> {
    let privb = <[u8; 56]>::try_from(privb).ok()?;
    let publ = <[u8; 56]>::try_from(publ).ok()?;
    x448::x448(privb, publ)
}

/// `__wjs_x448_derive(privU8, pubU8)` → 56B 共享秘密；低阶点/全零输出 None →
/// node 口径 FAILED_DURING_DERIVATION（真机 26 实测文案）。
pub unsafe extern "C" fn x448_derive(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: X448 derive needs private and public keys");
        return false;
    }
    let (Some(privb), Some(publ)) = (
        view_bytes(&mut cx, frame.arg(0), "X448 private key"),
        view_bytes(&mut cx, frame.arg(1), "X448 public key"),
    ) else {
        return false;
    };
    let (Ok(privb), Ok(publ)) = (
        <[u8; 56]>::try_from(privb.as_slice()),
        <[u8; 56]>::try_from(publ.as_slice()),
    ) else {
        report_error(&mut cx, "DataError: bad X448 key length");
        return false;
    };
    match x448_dh(&privb, &publ) {
        Some(secret) => set_rval_bytes(&mut cx, &frame, &secret),
        None => {
            report_error(
                &mut cx,
                "ERR_OSSL_FAILED_DURING_DERIVATION: error:1C8000A4:Provider routines::failed during derivation",
            );
            false
        }
    }
}
