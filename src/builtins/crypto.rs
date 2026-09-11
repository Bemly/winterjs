//! `crypto`：getRandomValues（`getrandom`）+ randomUUID（`uuid`）。
//! 全 safe（读/写均经 `as_*_slice_safe` + `NoGC` 令牌）；BigInt 视图暂不支持。

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::context::JSContext;
use mozjs::jsapi::JSObject;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;
use mozjs::typedarray::{CreateWith, TypedArray, Uint8};


use crate::jsapi_glue::{report_error, value_to_string, view_bytes, wrap_cx, Frame};

/// `__wjs_aesgcm_encrypt(keyU8, ivU8, aadU8?, plainU8)` → Uint8Array(ct‖tag)。
/// 仅 128-bit tag（JWK 常用线）；其余抛 NotSupportedError。
pub unsafe extern "C" fn aesgcm_encrypt(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 {
        report_error(&mut cx, "TypeError: AES-GCM encrypt needs key, iv, aad and data");
        return false;
    }
    let (k, iv, aad, plain) = (frame.arg(0), frame.arg(1), frame.arg(2), frame.arg(3));
    let (Some(key), Some(iv), Some(aad), Some(plain)) = (
        view_bytes(&mut cx, k, "AES-GCM key"),
        view_bytes(&mut cx, iv, "AES-GCM iv"),
        opt_view_bytes(&mut cx, aad, "AES-GCM aad"),
        view_bytes(&mut cx, plain, "AES-GCM data"),
    ) else {
        return false;
    };
    if iv.len() != 12 {
        report_error(&mut cx, "OperationError: AES-GCM iv must be 12 bytes");
        return false;
    }
    let aad_ref = aad.as_deref().unwrap_or(&[]);
    type Aes192Gcm = aes_gcm::AesGcm<aes::Aes192, aes_gcm::aead::consts::U12>;
    let out = if key.len() == 16 {
        encrypt_with::<aes_gcm::Aes128Gcm>(&key, &iv, aad_ref, &plain)
    } else if key.len() == 24 {
        encrypt_with::<Aes192Gcm>(&key, &iv, aad_ref, &plain)
    } else if key.len() == 32 {
        encrypt_with::<aes_gcm::Aes256Gcm>(&key, &iv, aad_ref, &plain)
    } else {
        report_error(&mut cx, "OperationError: AES-GCM key must be 16, 24 or 32 bytes");
        return false;
    };
    match out {
        Ok(ct) => set_rval_bytes(&mut cx, &frame, &ct),
        Err(e) => {
            report_error(&mut cx, &format!("OperationError: AES-GCM encrypt failed: {e}"));
            false
        }
    }
}

fn encrypt_with<A>(key: &[u8], iv: &[u8], aad: &[u8], plain: &[u8]) -> Result<Vec<u8>, String>
where
    A: aes_gcm::aead::KeyInit + aes_gcm::aead::Aead,
{
    use aes_gcm::aead::Payload;
    let cipher = A::new_from_slice(key).map_err(|e| e.to_string())?;
    let nonce =
        aes_gcm::aead::Nonce::<A>::try_from(iv).map_err(|_| "bad nonce".to_string())?;
    cipher
        .encrypt(&nonce, Payload { msg: plain, aad })
        .map_err(|e| e.to_string())
}

/// `__wjs_aesgcm_decrypt(keyU8, ivU8, aadU8?, dataU8)` → Uint8Array（认证失败抛 OperationError）。
pub unsafe extern "C" fn aesgcm_decrypt(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 {
        report_error(&mut cx, "TypeError: AES-GCM decrypt needs key, iv, aad and data");
        return false;
    }
    let (k, iv, aad, data) = (frame.arg(0), frame.arg(1), frame.arg(2), frame.arg(3));
    let (Some(key), Some(iv), Some(aad), Some(data)) = (
        view_bytes(&mut cx, k, "AES-GCM key"),
        view_bytes(&mut cx, iv, "AES-GCM iv"),
        opt_view_bytes(&mut cx, aad, "AES-GCM aad"),
        view_bytes(&mut cx, data, "AES-GCM data"),
    ) else {
        return false;
    };
    if iv.len() != 12 {
        report_error(&mut cx, "OperationError: AES-GCM iv must be 12 bytes");
        return false;
    }
    let aad_ref = aad.as_deref().unwrap_or(&[]);
    type Aes192Gcm = aes_gcm::AesGcm<aes::Aes192, aes_gcm::aead::consts::U12>;
    let out = if key.len() == 16 {
        decrypt_with::<aes_gcm::Aes128Gcm>(&key, &iv, aad_ref, &data)
    } else if key.len() == 24 {
        decrypt_with::<Aes192Gcm>(&key, &iv, aad_ref, &data)
    } else if key.len() == 32 {
        decrypt_with::<aes_gcm::Aes256Gcm>(&key, &iv, aad_ref, &data)
    } else {
        report_error(&mut cx, "OperationError: AES-GCM key must be 16, 24 or 32 bytes");
        return false;
    };
    match out {
        Ok(pt) => set_rval_bytes(&mut cx, &frame, &pt),
        Err(_) => {
            report_error(&mut cx, "OperationError: AES-GCM decrypt failed (bad key/iv/tag?)");
            false
        }
    }
}

fn decrypt_with<A>(key: &[u8], iv: &[u8], aad: &[u8], data: &[u8]) -> Result<Vec<u8>, String>
where
    A: aes_gcm::aead::KeyInit + aes_gcm::aead::Aead,
{
    use aes_gcm::aead::Payload;
    let cipher = A::new_from_slice(key).map_err(|e| e.to_string())?;
    let nonce =
        aes_gcm::aead::Nonce::<A>::try_from(iv).map_err(|_| "bad nonce".to_string())?;
    cipher
        .decrypt(&nonce, Payload { msg: data, aad })
        .map_err(|e| e.to_string())
}

macro_rules! hmac_with {
    ($hash:ty, $key:expr, $data:expr) => {{
        use hmac::{Hmac, Mac as _, digest::KeyInit as _};
        let mut mac =
            Hmac::<$hash>::new_from_slice($key).map_err(|e| format!("OperationError: {e}"))?;
        mac.update($data);
        mac.finalize().into_bytes().to_vec()
    }};
}

fn hmac_bytes(hash: &str, key: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
    match hash {
        "SHA-1" => Ok(hmac_with!(sha1::Sha1, key, data)),
        "SHA-256" => Ok(hmac_with!(sha2::Sha256, key, data)),
        "SHA-384" => Ok(hmac_with!(sha2::Sha384, key, data)),
        "SHA-512" => Ok(hmac_with!(sha2::Sha512, key, data)),
        other => Err(format!("NotSupportedError: unsupported HMAC hash '{other}'")),
    }
}

/// `__wjs_hmac_sign(hash, keyU8, dataU8)` → Uint8Array。
pub unsafe extern "C" fn hmac_sign(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: HMAC sign needs hash, key and data");
        return false;
    }
    let hash = value_to_string(&mut cx, frame.arg(0));
    let (Some(key), Some(data)) = (
        view_bytes(&mut cx, frame.arg(1), "HMAC key"),
        view_bytes(&mut cx, frame.arg(2), "HMAC data"),
    ) else {
        return false;
    };
    match hmac_bytes(&hash, &key, &data) {
        Ok(tag) => set_rval_bytes(&mut cx, &frame, &tag),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_hmac_verify(hash, keyU8, sigU8, dataU8)` → boolean。
pub unsafe extern "C" fn hmac_verify(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 {
        report_error(&mut cx, "TypeError: HMAC verify needs hash, key, signature and data");
        return false;
    }
    let hash = value_to_string(&mut cx, frame.arg(0));
    let (Some(key), Some(sig), Some(data)) = (
        view_bytes(&mut cx, frame.arg(1), "HMAC key"),
        view_bytes(&mut cx, frame.arg(2), "HMAC signature"),
        view_bytes(&mut cx, frame.arg(3), "HMAC data"),
    ) else {
        return false;
    };
    match hmac_bytes(&hash, &key, &data) {
        Ok(tag) => {
            // 定长比较（长度先行，内容恒时由 Vec 比较短路程度可忽略的侧信道面）
            let ok = tag.len() == sig.len()
                && tag.iter().zip(sig.iter()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0;
            frame.set_rval(mozjs::jsval::BooleanValue(ok));
            true
        }
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// 上限 64KiB（规范 QuotaExceededError）。
const MAX_BYTES: usize = 65536;

macro_rules! try_fill {
    ($cx:expr, $obj:expr, $Marker:ty, $Elem:ty, $SIZE:expr) => {{
        if let Ok(mut arr) = TypedArray::<$Marker, *mut JSObject>::from($obj) {
            if arr.is_shared() {
                report_error($cx, "TypeError: getRandomValues does not accept SharedArrayBuffer views yet");
                return false;
            }
            let detached = arr.as_slice_safe($cx.no_gc()).is_none();
            if detached {
                report_error($cx, "TypeError: getRandomValues view is detached");
                return false;
            }
            let n: usize = arr.len();
            let bytes = n.saturating_mul($SIZE);
            if bytes > MAX_BYTES {
                report_error($cx, "QuotaExceededError: getRandomValues quota is 64KiB");
                return false;
            }
            let mut raw = vec![0u8; bytes];
            if let Err(e) = getrandom::fill(&mut raw) {
                report_error($cx, &format!("OperationError: cannot get random values: {e}"));
                return false;
            }
            if let Some(slot) = arr.as_mut_slice_safe($cx.no_gc_mut()) {
                for (i, cell) in slot.iter_mut().enumerate() {
                    let chunk: [u8; $SIZE] = raw[i * $SIZE..i * $SIZE + $SIZE]
                        .try_into()
                        .unwrap_or([0u8; $SIZE]);
                    *cell = <$Elem>::from_ne_bytes(chunk);
                }
            }
            tracing::trace!(target: "winterjs::crypto", bytes, "getRandomValues filled");
            return true;
        }
    }};
}

/// `__wjs_fill_random(view)`：就地填充（prelude 原样返回 view）。
pub unsafe extern "C" fn fill_random(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: getRandomValues requires an argument");
        return false;
    }
    let v = frame.arg(0);
    if !v.is_object() {
        report_error(&mut cx, "TypeError: getRandomValues requires a typed array view");
        return false;
    }
    // SAFETY: is_object 已判定（to_object 为 safe API，见 §6 审计）
    let obj = v.to_object();
    // 浮点视图先判（必须抛，不可按整数填充）
    if TypedArray::<mozjs::typedarray::Float32, *mut JSObject>::from(obj).is_ok()
        || TypedArray::<mozjs::typedarray::Float64, *mut JSObject>::from(obj).is_ok()
    {
        report_error(&mut cx, "TypeError: getRandomValues requires an integer typed array");
        return false;
    }
    try_fill!(&mut cx, obj, mozjs::typedarray::Uint8, u8, 1);
    try_fill!(&mut cx, obj, mozjs::typedarray::Int8, i8, 1);
    try_fill!(&mut cx, obj, mozjs::typedarray::ClampedU8, u8, 1);
    try_fill!(&mut cx, obj, mozjs::typedarray::Uint16, u16, 2);
    try_fill!(&mut cx, obj, mozjs::typedarray::Int16, i16, 2);
    try_fill!(&mut cx, obj, mozjs::typedarray::Uint32, u32, 4);
    try_fill!(&mut cx, obj, mozjs::typedarray::Int32, i32, 4);
    let got = value_to_string(&mut cx, v);
    report_error(
        &mut cx,
        &format!("TypeError: getRandomValues needs an integer TypedArray, got {got}"),
    );
    false
}

/// `__wjs_random_uuid()` → v4 字符串。
pub unsafe extern "C" fn random_uuid(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let s = uuid::Uuid::new_v4().hyphenated().to_string();
    rooted!(&in(cx) let mut v = UndefinedValue());
    s.to_jsval(&mut cx, v.handle_mut());
    frame.set_rval(v.get());
    true
}

/// `__wjs_subtle_digest(alg, view)` → Uint8Array（SHA-1/256/384/512；`sha1`/`sha2` 轮子）。
/// prelude 包一层 async 即得规范的 Promise 返回（计算本身同步，无需事件循环改动）。
pub unsafe extern "C" fn subtle_digest(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: digest requires an algorithm and data");
        return false;
    }
    let alg = value_to_string(&mut cx, frame.arg(0));
    let data = frame.arg(1);
    let bytes = match view_bytes(&mut cx, data, "digest data") {
        Some(b) => b,
        None => return false,
    };
    let out: Vec<u8> = match alg.trim().to_ascii_lowercase().as_str() {
        "sha-1" => {
            use sha1::Digest as _;
            sha1::Sha1::digest(&bytes).to_vec()
        }
        "sha-256" => {
            use sha2::Digest as _;
            sha2::Sha256::digest(&bytes).to_vec()
        }
        "sha-384" => {
            use sha2::Digest as _;
            sha2::Sha384::digest(&bytes).to_vec()
        }
        "sha-512" => {
            use sha2::Digest as _;
            sha2::Sha512::digest(&bytes).to_vec()
        }
        other => {
            report_error(&mut cx, &format!("NotSupportedError: unsupported digest algorithm '{other}'"));
            return false;
        }
    };
    rooted!(&in(cx) let mut obj: *mut JSObject = std::ptr::null_mut());
    // SAFETY: realm 内创建；obj 为 rooted 出参；out 存活到调用返回（§6 审计：边界调用）
    let ok = unsafe {
        TypedArray::<Uint8, *mut JSObject>::create(&mut cx, CreateWith::Slice(&out), obj.handle_mut())
    };
    if ok.is_err() || obj.is_null() {
        report_error(&mut cx, "RangeError: cannot allocate digest output");
        return false;
    }
    frame.set_rval(mozjs::jsval::ObjectValue(obj.get()));
    true
}

/// 同上模式的 Uint8Array 返回（AES/HMAC 共用）。
fn set_rval_bytes(cx: &mut JSContext, frame: &Frame, out: &[u8]) -> bool {
    rooted!(&in(cx) let mut obj: *mut JSObject = std::ptr::null_mut());
    // SAFETY: 同上（§6 审计：边界调用）
    let ok = unsafe {
        TypedArray::<Uint8, *mut JSObject>::create(cx, CreateWith::Slice(out), obj.handle_mut())
    };
    if ok.is_err() || obj.is_null() {
        report_error(cx, "RangeError: cannot allocate output");
        return false;
    }
    frame.set_rval(mozjs::jsval::ObjectValue(obj.get()));
    true
}

/// 实参 Uint8Array（`what` 用于报错）；undefined/null → None（无 aad 时用）。
fn opt_view_bytes(cx: &mut JSContext, v: JSVal, what: &str) -> Option<Option<Vec<u8>>> {
    if v.is_undefined() || v.is_null() {
        return Some(None);
    }
    view_bytes(cx, v, what).map(Some)
}

// ── Phase 3 c-4：非对称（RSA/ECDSA/ECDH）──────────────────────────────────
// 密钥统一存 DER：私钥 PKCS#8、公钥 SPKI（prelude `CryptoKey.material` 即 DER 字节）。
// JWK 来回经 JSON 桥（复杂值走 JSON，见 §6 路线）；`der`/`pkcs8`/`spki` 经
// `rsa`/`p256` 系重导出直用，不另引 `rand_core` 0.6 / `signature`（零新增依赖）。
// RSA 哈希经 `sha2_010`（rsa 0.9 的签名/填充接口绑 `digest` 0.10，直引 0.11 传不进去）；
// EC 只做哈希（直引 sha2/sha1 0.11）+ `sign_prehash`，无版本面。

/// `getrandom` 垫底的 `rand_core` 0.6 RNG（给 `RsaPrivateKey::new` / OAEP 加密）。
/// 入口先探针（`SystemRng::probe`），此处失败不可达；兜底零填 + 文档记录（不用 panic，
/// native 内 panic 非 unwind 即 abort，见 §4.14）。
struct SystemRng;

impl rsa::rand_core::RngCore for SystemRng {
    fn next_u32(&mut self) -> u32 {
        let mut b = [0u8; 4];
        let _ = self.try_fill_bytes(&mut b);
        u32::from_ne_bytes(b)
    }
    fn next_u64(&mut self) -> u64 {
        let mut b = [0u8; 8];
        let _ = self.try_fill_bytes(&mut b);
        u64::from_ne_bytes(b)
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        let _ = self.try_fill_bytes(dest);
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rsa::rand_core::Error> {
        getrandom::fill(dest).map_err(|_| rsa::rand_core::Error::from(SYSTEM_RNG_ERR))
    }
}

impl rsa::rand_core::CryptoRng for SystemRng {}

/// 非零常量（`NonZeroU32::new` 为 const fn；`CUSTOM_START` 非零恒成立，`MIN` 兜底防 panic）。
const SYSTEM_RNG_ERR: core::num::NonZeroU32 = match core::num::NonZeroU32::new(rsa::rand_core::Error::CUSTOM_START) {
    Some(n) => n,
    None => core::num::NonZeroU32::MIN,
};

/// 入口探针：OS 熵源不可用即早报 `OperationError`（随后 `SystemRng` 不会再失败）。
fn rng_probe(cx: &mut JSContext) -> bool {
    if getrandom::fill(&mut [0u8; 1]).is_err() {
        report_error(cx, "OperationError: cannot get random values");
        return false;
    }
    true
}

/// RSA 哈希分发（SHA-256/384/512；SHA-1 已废弃，报 `NotSupportedError` 指到 SHA-2）。
macro_rules! rsa_hash_dispatch {
    ($hash:expr, $D:ident, $body:expr) => {{
        match $hash {
            "SHA-256" => {
                type $D = sha2_010::Sha256;
                $body
            }
            "SHA-384" => {
                type $D = sha2_010::Sha384;
                $body
            }
            "SHA-512" => {
                type $D = sha2_010::Sha512;
                $body
            }
            other => Err(format!("NotSupportedError: RSA with hash '{other}' needs SHA-256/384/512")),
        }
    }};
}

/// DER 字节 → `RsaPrivateKey`（`DataError` 口径）。
fn rsa_priv_from_der(der: &[u8]) -> Result<rsa::RsaPrivateKey, String> {
    use rsa::pkcs8::DecodePrivateKey as _;
    rsa::RsaPrivateKey::from_pkcs8_der(der).map_err(|_| "DataError: bad RSA private key (PKCS#8)".to_string())
}

/// DER 字节 → `RsaPublicKey`（`DataError` 口径）。
fn rsa_pub_from_der(der: &[u8]) -> Result<rsa::RsaPublicKey, String> {
    use rsa::pkcs8::DecodePublicKey as _;
    rsa::RsaPublicKey::from_public_key_der(der).map_err(|_| "DataError: bad RSA public key (SPKI)".to_string())
}

/// `__wjs_rsa_generate(bits, e)` → PKCS#8 DER 私钥（2048/3072/4096；e 常用 65537）。
pub unsafe extern "C" fn rsa_generate(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 || !frame.arg(0).is_number() || !frame.arg(1).is_number() {
        report_error(&mut cx, "TypeError: RSA generate needs modulusLength and publicExponent");
        return false;
    }
    let bits = frame.arg(0).to_number() as usize;
    let e = frame.arg(1).to_number() as u64;
    if ![2048, 3072, 4096].contains(&bits) {
        report_error(&mut cx, "NotSupportedError: RSA modulusLength must be 2048/3072/4096");
        return false;
    }
    if !(2..=(1 << 33) - 1).contains(&e) {
        report_error(&mut cx, "DataError: bad RSA publicExponent");
        return false;
    }
    if !rng_probe(&mut cx) {
        return false;
    }
    let exp = rsa::BigUint::from(e);
    let key = match rsa::RsaPrivateKey::new_with_exp(&mut SystemRng, bits, &exp) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &format!("OperationError: RSA key generation failed: {e}"));
            return false;
        }
    };
    use rsa::pkcs8::EncodePrivateKey as _;
    let der = match key.to_pkcs8_der() {
        Ok(d) => d.as_bytes().to_vec(),
        Err(e) => {
            report_error(&mut cx, &format!("OperationError: RSA key export failed: {e}"));
            return false;
        }
    };
    tracing::debug!(target: "winterjs::crypto", bits, "RSA key generated");
    set_rval_bytes(&mut cx, &frame, &der)
}

/// `__wjs_rsa_public(privDer)` → SPKI DER 公钥。
pub unsafe extern "C" fn rsa_public(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: RSA public needs a private key");
        return false;
    }
    let Some(der) = view_bytes(&mut cx, frame.arg(0), "RSA private key") else {
        return false;
    };
    let priv_key = match rsa_priv_from_der(&der) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    use rsa::pkcs8::EncodePublicKey as _;
    let spki = match priv_key.to_public_key().to_public_key_der() {
        Ok(d) => d.as_bytes().to_vec(),
        Err(e) => {
            report_error(&mut cx, &format!("OperationError: RSA public export failed: {e}"));
            return false;
        }
    };
    set_rval_bytes(&mut cx, &frame, &spki)
}

/// `__wjs_rsa_sign(hash, privDer, data)` → 签名（RSASSA-PKCS1-v1_5）。
pub unsafe extern "C" fn rsa_sign(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: RSA sign needs hash, key and data");
        return false;
    }
    let hash = value_to_string(&mut cx, frame.arg(0));
    let (Some(der), Some(data)) = (
        view_bytes(&mut cx, frame.arg(1), "RSA private key"),
        view_bytes(&mut cx, frame.arg(2), "RSA data"),
    ) else {
        return false;
    };
    let priv_key = match rsa_priv_from_der(&der) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let out: Result<Vec<u8>, String> = rsa_hash_dispatch!(hash.as_str(), D, {
        use rsa::signature::Signer as _;
        let sk = rsa::pkcs1v15::SigningKey::<D>::new(priv_key);
        Ok(Box::<[u8]>::from(sk.sign(&data)).into_vec())
    });
    match out {
        Ok(sig) => set_rval_bytes(&mut cx, &frame, &sig),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_rsa_verify(hash, pubDer, sig, data)` → boolean。
pub unsafe extern "C" fn rsa_verify(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 {
        report_error(&mut cx, "TypeError: RSA verify needs hash, key, signature and data");
        return false;
    }
    let hash = value_to_string(&mut cx, frame.arg(0));
    let (Some(der), Some(sig), Some(data)) = (
        view_bytes(&mut cx, frame.arg(1), "RSA public key"),
        view_bytes(&mut cx, frame.arg(2), "RSA signature"),
        view_bytes(&mut cx, frame.arg(3), "RSA data"),
    ) else {
        return false;
    };
    let pub_key = match rsa_pub_from_der(&der) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let out: Result<bool, String> = rsa_hash_dispatch!(hash.as_str(), D, {
        use rsa::signature::Verifier as _;
        let vk = rsa::pkcs1v15::VerifyingKey::<D>::new(pub_key);
        match rsa::pkcs1v15::Signature::try_from(sig.as_slice()) {
            Ok(s) => Ok(vk.verify(&data, &s).is_ok()),
            Err(_) => Err("OperationError: bad RSA signature length".to_string()),
        }
    });
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

/// `__wjs_rsa_encrypt(hash, pubDer, data, label?)` → 密文（RSA-OAEP，label 可选）。
pub unsafe extern "C" fn rsa_encrypt(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: RSA encrypt needs hash, key and data");
        return false;
    }
    let hash = value_to_string(&mut cx, frame.arg(0));
    let (Some(der), Some(data)) = (
        view_bytes(&mut cx, frame.arg(1), "RSA public key"),
        view_bytes(&mut cx, frame.arg(2), "RSA data"),
    ) else {
        return false;
    };
    let label: Option<String> = if frame.argc() > 3 {
        match opt_view_bytes(&mut cx, frame.arg(3), "RSA label") {
            Some(Some(bytes)) => match String::from_utf8(bytes) {
                Ok(s) => Some(s),
                Err(_) => {
                    report_error(&mut cx, "DataError: RSA label must be UTF-8");
                    return false;
                }
            },
            Some(None) => None,
            None => return false,
        }
    } else {
        None
    };
    let pub_key = match rsa_pub_from_der(&der) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    if !rng_probe(&mut cx) {
        return false;
    }
    let out: Result<Vec<u8>, String> = rsa_hash_dispatch!(hash.as_str(), D, {
        let padding = match label {
            Some(l) => rsa::Oaep::new_with_label::<D, _>(l),
            None => rsa::Oaep::new::<D>(),
        };
        pub_key
            .encrypt(&mut SystemRng, padding, &data)
            .map_err(|e| format!("OperationError: RSA encrypt failed: {e}"))
    });
    match out {
        Ok(ct) => set_rval_bytes(&mut cx, &frame, &ct),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_rsa_decrypt(hash, privDer, data, label?)` → 明文（RSA-OAEP）。
pub unsafe extern "C" fn rsa_decrypt(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: RSA decrypt needs hash, key and data");
        return false;
    }
    let hash = value_to_string(&mut cx, frame.arg(0));
    let (Some(der), Some(data)) = (
        view_bytes(&mut cx, frame.arg(1), "RSA private key"),
        view_bytes(&mut cx, frame.arg(2), "RSA data"),
    ) else {
        return false;
    };
    let label: Option<String> = if frame.argc() > 3 {
        match opt_view_bytes(&mut cx, frame.arg(3), "RSA label") {
            Some(Some(bytes)) => match String::from_utf8(bytes) {
                Ok(s) => Some(s),
                Err(_) => {
                    report_error(&mut cx, "DataError: RSA label must be UTF-8");
                    return false;
                }
            },
            Some(None) => None,
            None => return false,
        }
    } else {
        None
    };
    let priv_key = match rsa_priv_from_der(&der) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let out: Result<Vec<u8>, String> = rsa_hash_dispatch!(hash.as_str(), D, {
        let padding = match label {
            Some(l) => rsa::Oaep::new_with_label::<D, _>(l),
            None => rsa::Oaep::new::<D>(),
        };
        priv_key
            .decrypt(padding, &data)
            .map_err(|_| "OperationError: RSA decrypt failed (bad key/label/data?)".to_string())
    });
    match out {
        Ok(pt) => set_rval_bytes(&mut cx, &frame, &pt),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// base64url 无填充编码（JWK 用；解码走 prelude 已有 `__wjs_b64urlDecode`）。
fn b64url(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// 大整数 → 最短大端字节（JWK `n/e/d` 等；空即 `"AA"`）。
fn bignum(b: &rsa::BigUint) -> String {
    let v = b.to_bytes_be();
    let v = v.as_slice();
    let mut i = 0;
    while i + 1 < v.len() && v[i] == 0 {
        i += 1;
    }
    b64url(&v[i..])
}

/// `__wjs_rsa_jwk(privDer, pubDer)` → JWK 参数 JSON（含私钥段；prelude 组 JWK 对象）。
pub unsafe extern "C" fn rsa_jwk(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: RSA JWK needs private and public keys");
        return false;
    }
    let (Some(priv_der), Some(pub_der)) = (
        view_bytes(&mut cx, frame.arg(0), "RSA private key"),
        view_bytes(&mut cx, frame.arg(1), "RSA public key"),
    ) else {
        return false;
    };
    let priv_key = match rsa_priv_from_der(&priv_der) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let pub_key = match rsa_pub_from_der(&pub_der) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    use rsa::traits::PublicKeyParts as _;
    use rsa::traits::PrivateKeyParts as _;
    // CRT 参数经预计算取（生成/导入路径必跑 precompute；None 即密钥损坏）。
    let (Some(dp), Some(dq), Some(qi)) = (priv_key.dp(), priv_key.dq(), priv_key.qinv()) else {
        report_error(&mut cx, "DataError: bad RSA key (no CRT params)");
        return false;
    };
    let (_, qi_bytes) = qi.to_bytes_be();
    let (p, q) = match (priv_key.primes().first(), priv_key.primes().get(1)) {
        (Some(p), Some(q)) => (p, q),
        _ => {
            report_error(&mut cx, "DataError: RSA key missing primes");
            return false;
        }
    };
    let json = serde_json::json!({
        "n": bignum(pub_key.n()),
        "e": bignum(pub_key.e()),
        "d": bignum(priv_key.d()),
        "p": bignum(p),
        "q": bignum(q),
        "dp": bignum(dp),
        "dq": bignum(dq),
        "qi": b64url(&qi_bytes),
    })
    .to_string();
    rooted!(&in(cx) let mut v = UndefinedValue());
    json.to_jsval(&mut cx, v.handle_mut());
    frame.set_rval(v.get());
    true
}

/// `__wjs_rsa_jwk_pub(pubDer)` → 公钥参数 JSON（`{n,e}`；spki import 组 algorithm 用）。
pub unsafe extern "C" fn rsa_jwk_pub(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: RSA JWK needs a public key");
        return false;
    }
    let Some(pub_der) = view_bytes(&mut cx, frame.arg(0), "RSA public key") else {
        return false;
    };
    let pub_key = match rsa_pub_from_der(&pub_der) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    use rsa::traits::PublicKeyParts as _;
    let json = serde_json::json!({
        "n": bignum(pub_key.n()),
        "e": bignum(pub_key.e()),
    })
    .to_string();
    rooted!(&in(cx) let mut v = UndefinedValue());
    json.to_jsval(&mut cx, v.handle_mut());
    frame.set_rval(v.get());
    true
}

/// `__wjs_rsa_import_priv(nU8, eU8, dU8)` → PKCS#8 DER（p/q 按 SP 800-56B 恢复）。
pub unsafe extern "C" fn rsa_import_priv(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: RSA import needs n, e and d");
        return false;
    }
    let (Some(n), Some(e), Some(d)) = (
        view_bytes(&mut cx, frame.arg(0), "RSA n"),
        view_bytes(&mut cx, frame.arg(1), "RSA e"),
        view_bytes(&mut cx, frame.arg(2), "RSA d"),
    ) else {
        return false;
    };
    let key = match rsa::RsaPrivateKey::from_components(
        rsa::BigUint::from_bytes_be(&n),
        rsa::BigUint::from_bytes_be(&e),
        rsa::BigUint::from_bytes_be(&d),
        vec![],
    ) {
        Ok(k) => k,
        Err(_) => {
            report_error(&mut cx, "DataError: bad RSA JWK (n/e/d)");
            return false;
        }
    };
    use rsa::pkcs8::EncodePrivateKey as _;
    let der = match key.to_pkcs8_der() {
        Ok(d) => d.as_bytes().to_vec(),
        Err(e) => {
            report_error(&mut cx, &format!("OperationError: RSA import failed: {e}"));
            return false;
        }
    };
    set_rval_bytes(&mut cx, &frame, &der)
}

/// `__wjs_rsa_import_pub(nU8, eU8)` → SPKI DER。
pub unsafe extern "C" fn rsa_import_pub(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: RSA import needs n and e");
        return false;
    }
    let (Some(n), Some(e)) = (
        view_bytes(&mut cx, frame.arg(0), "RSA n"),
        view_bytes(&mut cx, frame.arg(1), "RSA e"),
    ) else {
        return false;
    };
    let key = match rsa::RsaPublicKey::new(
        rsa::BigUint::from_bytes_be(&n),
        rsa::BigUint::from_bytes_be(&e),
    ) {
        Ok(k) => k,
        Err(_) => {
            report_error(&mut cx, "DataError: bad RSA JWK (n/e)");
            return false;
        }
    };
    use rsa::pkcs8::EncodePublicKey as _;
    let der = match key.to_public_key_der() {
        Ok(d) => d.as_bytes().to_vec(),
        Err(e) => {
            report_error(&mut cx, &format!("OperationError: RSA import failed: {e}"));
            return false;
        }
    };
    set_rval_bytes(&mut cx, &frame, &der)
}

// ── 椭圆曲线（ECDSA/ECDH，P-256/384/521）──────────────────────────────────
// 三曲线同构：宏按曲线展开（`p256`/`p384`/`p521`，`ecdh`+`pkcs8` 特性已显式声明）。

/// 曲线名字典序（prelude 已归一化 `P-256` 等；此处再守一次）。
/// 绑定：曲线标记 `C`、私钥 `Secret`、公钥 `Public`、签名钥 `Signing`、验签钥 `Verifying`、签名 `Sig`。
macro_rules! with_curve {
    ($curve:expr, |$C:ident, $Secret:ident, $Public:ident, $Signing:ident, $Verifying:ident, $Sig:ident| $body:expr) => {{
        match $curve {
            "P-256" => {
                #[allow(dead_code)]
                type $C = p256::NistP256;
                #[allow(dead_code)]
                type $Secret = p256::SecretKey;
                #[allow(dead_code)]
                type $Public = p256::PublicKey;
                #[allow(dead_code)]
                type $Signing = p256::ecdsa::SigningKey;
                #[allow(dead_code)]
                type $Verifying = p256::ecdsa::VerifyingKey;
                #[allow(dead_code)]
                type $Sig = p256::ecdsa::Signature;
                $body
            }
            "P-384" => {
                #[allow(dead_code)]
                type $C = p384::NistP384;
                #[allow(dead_code)]
                type $Secret = p384::SecretKey;
                #[allow(dead_code)]
                type $Public = p384::PublicKey;
                #[allow(dead_code)]
                type $Signing = p384::ecdsa::SigningKey;
                #[allow(dead_code)]
                type $Verifying = p384::ecdsa::VerifyingKey;
                #[allow(dead_code)]
                type $Sig = p384::ecdsa::Signature;
                $body
            }
            "P-521" => {
                #[allow(dead_code)]
                type $C = p521::NistP521;
                #[allow(dead_code)]
                type $Secret = p521::SecretKey;
                #[allow(dead_code)]
                type $Public = p521::PublicKey;
                #[allow(dead_code)]
                type $Signing = p521::ecdsa::SigningKey;
                #[allow(dead_code)]
                type $Verifying = p521::ecdsa::VerifyingKey;
                #[allow(dead_code)]
                type $Sig = p521::ecdsa::Signature;
                $body
            }
            other => Err(format!("NotSupportedError: unsupported curve '{other}' (P-256/384/521)")),
        }
    }};
}

/// 曲线阶字节数（P-256→32，P-384→48，P-521→66；JWK 定长坐标用）。
fn curve_size(curve: &str) -> Option<usize> {
    match curve {
        "P-256" => Some(32),
        "P-384" => Some(48),
        "P-521" => Some(66),
        _ => None,
    }
}

/// `__wjs_ec_generate(curve)` → PKCS#8 DER 私钥（熵源 `getrandom`，失败即 `OperationError`）。
pub unsafe extern "C" fn ec_generate(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: EC generate needs a curve");
        return false;
    }
    let curve = value_to_string(&mut cx, frame.arg(0));
    let Some(size) = curve_size(&curve) else {
        report_error(&mut cx, &format!("NotSupportedError: unsupported curve '{curve}' (P-256/384/521)"));
        return false;
    };
    // 极小概率越界（随机标量 ≥ 阶）即重试，而非报错。
    for _ in 0..8 {
        let mut raw = vec![0u8; size];
        if getrandom::fill(&mut raw).is_err() {
            report_error(&mut cx, "OperationError: cannot get random values");
            return false;
        }
        let out: Result<Vec<u8>, String> = with_curve!(curve.as_str(), |C, Secret, Public, Signing, Verifying, Sig| {
            use p256::elliptic_curve::pkcs8::EncodePrivateKey as _;
            // C 仅作 `FieldBytes::<C>` 类型参（值位置用不上，`#[allow(dead_code)]` 在宏臂上）。
            p256::elliptic_curve::FieldBytes::<C>::try_from(raw.as_slice())
                .map_err(|_| String::new())
                .and_then(|fb| Secret::from_bytes(&fb).map_err(|_| String::new()))
                .and_then(|sk| sk.to_pkcs8_der().map_err(|_| String::new()))
                .map(|d| d.as_bytes().to_vec())
        });
        match out {
            Ok(der) => {
                tracing::debug!(target: "winterjs::crypto", curve = curve.as_str(), "EC key generated");
                return set_rval_bytes(&mut cx, &frame, &der);
            }
            Err(e) if e.is_empty() => continue,
            Err(e) => {
                report_error(&mut cx, &e);
                return false;
            }
        }
    }
    report_error(&mut cx, "OperationError: EC key generation failed");
    false
}

/// `__wjs_ec_public(curve, privDer)` → SPKI DER 公钥。
pub unsafe extern "C" fn ec_public(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: EC public needs curve and private key");
        return false;
    }
    let curve = value_to_string(&mut cx, frame.arg(0));
    let Some(der) = view_bytes(&mut cx, frame.arg(1), "EC private key") else {
        return false;
    };
    let out: Result<Vec<u8>, String> = with_curve!(curve.as_str(), |C, Secret, Public, Signing, Verifying, Sig| {
        use p256::elliptic_curve::pkcs8::{DecodePrivateKey as _, EncodePublicKey as _};
        Secret::from_pkcs8_der(&der)
            .map_err(|_| "DataError: bad EC private key (PKCS#8)".to_string())
            .and_then(|sk: Secret| {
                Public::from_secret_scalar(&sk.to_nonzero_scalar())
                    .to_public_key_der()
                    .map_err(|e| format!("OperationError: EC public export failed: {e}"))
                    .map(|d| d.as_bytes().to_vec())
            })
    });
    match out {
        Ok(spki) => set_rval_bytes(&mut cx, &frame, &spki),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// 数据哈希（ECDSA 预哈希；SHA-1/256/384/512，直引 0.11，无版本面）。
fn ec_hash(hash: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    match hash {
        "SHA-1" => {
            use sha1::Digest as _;
            Ok(sha1::Sha1::digest(data).to_vec())
        }
        "SHA-256" => {
            use sha2::Digest as _;
            Ok(sha2::Sha256::digest(data).to_vec())
        }
        "SHA-384" => {
            use sha2::Digest as _;
            Ok(sha2::Sha384::digest(data).to_vec())
        }
        "SHA-512" => {
            use sha2::Digest as _;
            Ok(sha2::Sha512::digest(data).to_vec())
        }
        other => Err(format!("NotSupportedError: unsupported ECDSA hash '{other}'")),
    }
}

/// `__wjs_ecdsa_sign(curve, hash, privDer, data)` → 裸 `r‖s` 签名（WebCrypto 口径，非 DER）。
pub unsafe extern "C" fn ecdsa_sign(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 {
        report_error(&mut cx, "TypeError: ECDSA sign needs curve, hash, key and data");
        return false;
    }
    let curve = value_to_string(&mut cx, frame.arg(0));
    let hash = value_to_string(&mut cx, frame.arg(1));
    let (Some(der), Some(data)) = (
        view_bytes(&mut cx, frame.arg(2), "ECDSA private key"),
        view_bytes(&mut cx, frame.arg(3), "ECDSA data"),
    ) else {
        return false;
    };
    let digest = match ec_hash(&hash, &data) {
        Ok(h) => h,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let out: Result<Vec<u8>, String> = with_curve!(curve.as_str(), |C, Secret, Public, Signing, Verifying, Sig| {
        // IIFE：`?`/early-return 作用于闭包（外层函数返 bool，`?` 直写即 E0277）。
        (|| -> Result<Vec<u8>, String> {
            use p256::ecdsa::signature::hazmat::PrehashSigner as _;
            use p256::elliptic_curve::pkcs8::DecodePrivateKey as _;
            let sk = Secret::from_pkcs8_der(&der)
                .map_err(|_| "DataError: bad ECDSA private key (PKCS#8)".to_string())?;
            let signer = Signing::from_bytes(&sk.to_bytes())
                .map_err(|_| "DataError: bad ECDSA private key".to_string())?;
            // `sign_prehash` 有裸/ DER 双实现（`Signature` vs `der::Signature`），结果类型注解消歧。
            let sig: Sig = signer
                .sign_prehash(&digest)
                .map_err(|_| "OperationError: ECDSA sign failed".to_string())?;
            Ok(sig.to_bytes().to_vec())
        })()
    });
    match out {
        Ok(sig) => set_rval_bytes(&mut cx, &frame, &sig),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_ecdsa_verify(curve, hash, pubDer, sigRaw, data)` → boolean（裸 `r‖s` 口径）。
pub unsafe extern "C" fn ecdsa_verify(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 5 {
        report_error(&mut cx, "TypeError: ECDSA verify needs curve, hash, key, signature and data");
        return false;
    }
    let curve = value_to_string(&mut cx, frame.arg(0));
    let hash = value_to_string(&mut cx, frame.arg(1));
    let (Some(der), Some(sig), Some(data)) = (
        view_bytes(&mut cx, frame.arg(2), "ECDSA public key"),
        view_bytes(&mut cx, frame.arg(3), "ECDSA signature"),
        view_bytes(&mut cx, frame.arg(4), "ECDSA data"),
    ) else {
        return false;
    };
    let digest = match ec_hash(&hash, &data) {
        Ok(h) => h,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let out: Result<bool, String> = with_curve!(curve.as_str(), |C, Secret, Public, Signing, Verifying, Sig| {
        (|| -> Result<bool, String> {
            use p256::ecdsa::signature::hazmat::PrehashVerifier as _;
            use p256::elliptic_curve::pkcs8::DecodePublicKey as _;
            let pk = Public::from_public_key_der(&der)
                .map_err(|_| "DataError: bad ECDSA public key (SPKI)".to_string())?;
            let vk = Verifying::from_sec1_bytes(&pk.to_sec1_bytes())
                .map_err(|_| "DataError: bad ECDSA public key".to_string())?;
            let sig = Sig::try_from(sig.as_slice())
                .map_err(|_| "OperationError: bad ECDSA signature length".to_string())?;
            Ok(vk.verify_prehash(&digest, &sig).is_ok())
        })()
    });
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

/// `__wjs_ecdh_derive(curve, privDer, pubDer)` → 原始共享秘密（定长：32/48/66）。
pub unsafe extern "C" fn ecdh_derive(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: ECDH derive needs curve, private and public keys");
        return false;
    }
    let curve = value_to_string(&mut cx, frame.arg(0));
    let (Some(priv_der), Some(pub_der)) = (
        view_bytes(&mut cx, frame.arg(1), "ECDH private key"),
        view_bytes(&mut cx, frame.arg(2), "ECDH public key"),
    ) else {
        return false;
    };
    let out: Result<Vec<u8>, String> = with_curve!(curve.as_str(), |C, Secret, Public, Signing, Verifying, Sig| {
        (|| -> Result<Vec<u8>, String> {
            use p256::elliptic_curve::pkcs8::{DecodePrivateKey as _, DecodePublicKey as _};
            let sk = Secret::from_pkcs8_der(&priv_der)
                .map_err(|_| "DataError: bad ECDH private key (PKCS#8)".to_string())?;
            let pk = Public::from_public_key_der(&pub_der)
                .map_err(|_| "DataError: bad ECDH public key (SPKI)".to_string())?;
            let shared = p256::elliptic_curve::ecdh::diffie_hellman(sk.to_nonzero_scalar(), pk.as_affine());
            Ok(shared.raw_secret_bytes().as_slice().to_vec())
        })()
    });
    match out {
        Ok(secret) => set_rval_bytes(&mut cx, &frame, &secret),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// 定长大端（JWK `x/y/d`；短即左补零，长即报错）。
fn fixed_pad(bytes: &[u8], size: usize) -> Result<Vec<u8>, String> {
    if bytes.len() > size {
        return Err("DataError: bad EC key (oversize coordinate)".to_string());
    }
    let mut out = vec![0u8; size];
    out[size - bytes.len()..].copy_from_slice(bytes);
    Ok(out)
}

/// `__wjs_ec_jwk(curve, privDer, pubDer)` → JWK 坐标 JSON（`{x,y,d?}`，base64url 定长）。
pub unsafe extern "C" fn ec_jwk(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: EC JWK needs curve, private and public keys");
        return false;
    }
    let curve = value_to_string(&mut cx, frame.arg(0));
    let (Some(priv_der), Some(pub_der)) = (
        view_bytes(&mut cx, frame.arg(1), "EC private key"),
        view_bytes(&mut cx, frame.arg(2), "EC public key"),
    ) else {
        return false;
    };
    let Some(size) = curve_size(&curve) else {
        report_error(&mut cx, &format!("NotSupportedError: unsupported curve '{curve}'"));
        return false;
    };
    let out: Result<String, String> = with_curve!(curve.as_str(), |C, Secret, Public, Signing, Verifying, Sig| {
        (|| -> Result<String, String> {
            use p256::elliptic_curve::pkcs8::{DecodePrivateKey as _, DecodePublicKey as _};
            use p256::elliptic_curve::sec1::ToSec1Point as _;
            let sk = Secret::from_pkcs8_der(&priv_der)
                .map_err(|_| "DataError: bad EC private key (PKCS#8)".to_string())?;
            let pk = Public::from_public_key_der(&pub_der)
                .map_err(|_| "DataError: bad EC public key (SPKI)".to_string())?;
            let point = pk.to_sec1_point(false);
            let (Some(x), Some(y)) = (point.x(), point.y()) else {
                return Err("DataError: bad EC public key (no coordinates)".to_string());
            };
            Ok(serde_json::json!({
                "x": b64url(&fixed_pad(x.as_slice(), size)?),
                "y": b64url(&fixed_pad(y.as_slice(), size)?),
                "d": b64url(&fixed_pad(sk.to_bytes().as_slice(), size)?),
            })
            .to_string())
        })()
    });
    match out {
        Ok(json) => {
            rooted!(&in(cx) let mut v = UndefinedValue());
            json.to_jsval(&mut cx, v.handle_mut());
            frame.set_rval(v.get());
            true
        }
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_ec_jwk_pub(curve, pubDer)` → 公钥坐标 JSON（`{x,y}`；非导出私钥时用）。
pub unsafe extern "C" fn ec_jwk_pub(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: EC JWK needs curve and public key");
        return false;
    }
    let curve = value_to_string(&mut cx, frame.arg(0));
    let Some(pub_der) = view_bytes(&mut cx, frame.arg(1), "EC public key") else {
        return false;
    };
    let Some(size) = curve_size(&curve) else {
        report_error(&mut cx, &format!("NotSupportedError: unsupported curve '{curve}'"));
        return false;
    };
    let out: Result<String, String> = with_curve!(curve.as_str(), |C, Secret, Public, Signing, Verifying, Sig| {
        (|| -> Result<String, String> {
            use p256::elliptic_curve::pkcs8::DecodePublicKey as _;
            use p256::elliptic_curve::sec1::ToSec1Point as _;
            let pk = Public::from_public_key_der(&pub_der)
                .map_err(|_| "DataError: bad EC public key (SPKI)".to_string())?;
            let point = pk.to_sec1_point(false);
            let (Some(x), Some(y)) = (point.x(), point.y()) else {
                return Err("DataError: bad EC public key (no coordinates)".to_string());
            };
            Ok(serde_json::json!({
                "x": b64url(&fixed_pad(x.as_slice(), size)?),
                "y": b64url(&fixed_pad(y.as_slice(), size)?),
            })
            .to_string())
        })()
    });
    match out {
        Ok(json) => {
            rooted!(&in(cx) let mut v = UndefinedValue());
            json.to_jsval(&mut cx, v.handle_mut());
            frame.set_rval(v.get());
            true
        }
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_ec_import_priv(curve, dU8)` → PKCS#8 DER（JWK `d` 进）。
pub unsafe extern "C" fn ec_import_priv(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: EC import needs curve and key");
        return false;
    }
    let curve = value_to_string(&mut cx, frame.arg(0));
    let Some(d) = view_bytes(&mut cx, frame.arg(1), "EC key") else {
        return false;
    };
    let Some(size) = curve_size(&curve) else {
        report_error(&mut cx, &format!("NotSupportedError: unsupported curve '{curve}'"));
        return false;
    };
    if d.len() != size {
        report_error(&mut cx, "DataError: bad EC JWK (d length)");
        return false;
    }
    let out: Result<Vec<u8>, String> = with_curve!(curve.as_str(), |C, Secret, Public, Signing, Verifying, Sig| {
        use p256::elliptic_curve::pkcs8::EncodePrivateKey as _;
        p256::elliptic_curve::FieldBytes::<C>::try_from(d.as_slice())
            .map_err(|_| "DataError: bad EC JWK (d)".to_string())
            .and_then(|fb| Secret::from_bytes(&fb).map_err(|_| "DataError: bad EC JWK (d)".to_string()))
            .and_then(|sk| {
                sk.to_pkcs8_der()
                    .map_err(|e| format!("OperationError: EC import failed: {e}"))
                    .map(|doc| doc.as_bytes().to_vec())
            })
    });
    match out {
        Ok(der) => set_rval_bytes(&mut cx, &frame, &der),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_ec_import_pub(curve, xU8, yU8)` → SPKI DER（JWK `x/y` 或 raw 公钥进）。
pub unsafe extern "C" fn ec_import_pub(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: EC import needs curve, x and y");
        return false;
    }
    let curve = value_to_string(&mut cx, frame.arg(0));
    let (Some(x), Some(y)) = (
        view_bytes(&mut cx, frame.arg(1), "EC x"),
        view_bytes(&mut cx, frame.arg(2), "EC y"),
    ) else {
        return false;
    };
    let Some(size) = curve_size(&curve) else {
        report_error(&mut cx, &format!("NotSupportedError: unsupported curve '{curve}'"));
        return false;
    };
    if x.len() != size || y.len() != size {
        report_error(&mut cx, "DataError: bad EC JWK (x/y length)");
        return false;
    }
    let out: Result<Vec<u8>, String> = with_curve!(curve.as_str(), |C, Secret, Public, Signing, Verifying, Sig| {
        use p256::elliptic_curve::pkcs8::EncodePublicKey as _;
        let mut prefixed = Vec::with_capacity(1 + 2 * size);
        prefixed.push(0x04);
        prefixed.extend_from_slice(&x);
        prefixed.extend_from_slice(&y);
        Public::from_sec1_bytes(&prefixed)
            .map_err(|_| "DataError: bad EC JWK (point not on curve)".to_string())
            .and_then(|pk| {
                pk.to_public_key_der()
                    .map_err(|e| format!("OperationError: EC import failed: {e}"))
                    .map(|d| d.as_bytes().to_vec())
            })
    });
    match out {
        Ok(der) => set_rval_bytes(&mut cx, &frame, &der),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

// ── Phase c-4x：RSA-PSS / Ed25519 / X25519 / AES-192 ─────────────────────
// 边界惯例同文件头：每 native 固定 `wrap_cx` + `Frame::from_raw` 两块
//（UNSAFE-BOUNDARY，结构性计数；黑盒见 tests/cli.rs `subtle_c4x_*`）。
// AES-192 经泛型 `AesGcm<Aes192, U12>`（aes-gcm 只给 128/256 起别名，无新依赖）。

/// `__wjs_pss_sign(hash, saltLen, privDer, data)` → 签名（RSA-PSS，salt 随机）。
pub unsafe extern "C" fn pss_sign(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 || !frame.arg(1).is_number() {
        report_error(&mut cx, "TypeError: RSA-PSS sign needs hash, saltLength, key and data");
        return false;
    }
    let hash = value_to_string(&mut cx, frame.arg(0));
    let salt = frame.arg(1).to_number();
    if !salt.is_finite() || salt < 0.0 || salt > 512.0 || salt.fract() != 0.0 {
        report_error(&mut cx, "OperationError: bad RSA-PSS saltLength");
        return false;
    }
    let (Some(der), Some(data)) = (
        view_bytes(&mut cx, frame.arg(2), "RSA private key"),
        view_bytes(&mut cx, frame.arg(3), "RSA data"),
    ) else {
        return false;
    };
    let priv_key = match rsa_priv_from_der(&der) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let out: Result<Vec<u8>, String> = rsa_hash_dispatch!(hash.as_str(), D, {
        use rsa::signature::{RandomizedSigner as _, SignatureEncoding as _};
        let sk = rsa::pss::SigningKey::<D>::new_with_salt_len(priv_key, salt as usize);
        sk.try_sign_with_rng(&mut SystemRng, &data)
            .map(|s| s.to_vec())
            .map_err(|e| format!("OperationError: RSA-PSS sign failed: {e}"))
    });
    match out {
        Ok(sig) => set_rval_bytes(&mut cx, &frame, &sig),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_pss_verify(hash, saltLen, pubDer, sig, data)` → boolean。
pub unsafe extern "C" fn pss_verify(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 5 || !frame.arg(1).is_number() {
        report_error(&mut cx, "TypeError: RSA-PSS verify needs hash, saltLength, key, signature and data");
        return false;
    }
    let hash = value_to_string(&mut cx, frame.arg(0));
    let salt = frame.arg(1).to_number();
    if !salt.is_finite() || salt < 0.0 || salt > 512.0 || salt.fract() != 0.0 {
        report_error(&mut cx, "OperationError: bad RSA-PSS saltLength");
        return false;
    }
    let (Some(der), Some(sig), Some(data)) = (
        view_bytes(&mut cx, frame.arg(2), "RSA public key"),
        view_bytes(&mut cx, frame.arg(3), "RSA signature"),
        view_bytes(&mut cx, frame.arg(4), "RSA data"),
    ) else {
        return false;
    };
    let pub_key = match rsa_pub_from_der(&der) {
        Ok(k) => k,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let out: Result<bool, String> = rsa_hash_dispatch!(hash.as_str(), D, {
        use rsa::signature::Verifier as _;
        let vk = rsa::pss::VerifyingKey::<D>::new_with_salt_len(pub_key, salt as usize);
        match rsa::pss::Signature::try_from(sig.as_slice()) {
            Ok(s) => Ok(vk.verify(&data, &s).is_ok()),
            Err(_) => Err("OperationError: bad RSA-PSS signature length".to_string()),
        }
    });
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

/// RFC 8410 定长 DER 编解码（Ed25519 oid …70 / X25519 …6e；纯函数，可单测）。
/// PKCS#8: `30 2E 02 01 00 30 05 06 03 2B 65 OID 04 22 04 20 <32B>`；
/// SPKI: `30 2A 30 05 06 03 2B 65 OID 03 21 00 <32B>`。
fn okp_oid_byte(kind: &str) -> Result<u8, String> {
    // prelude 统一传大写名（generateKey 内 `toUpperCase`）；此处按大写匹配
    match kind.to_ascii_uppercase().as_str() {
        "ED25519" => Ok(0x70),
        "X25519" => Ok(0x6E),
        _ => Err(format!("NotSupportedError: unsupported OKP key '{kind}'")),
    }
}

fn okp_wrap_pkcs8(kind: &str, seed: &[u8]) -> Result<Vec<u8>, String> {
    let oid = okp_oid_byte(kind)?;
    if seed.len() != 32 {
        return Err(format!("DataError: bad {kind} seed (must be 32 bytes)"));
    }
    let mut v = vec![
        0x30, 0x2E, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2B, 0x65, oid, 0x04, 0x22,
        0x04, 0x20,
    ];
    v.extend_from_slice(seed);
    Ok(v)
}

fn okp_wrap_spki(kind: &str, publ: &[u8]) -> Result<Vec<u8>, String> {
    let oid = okp_oid_byte(kind)?;
    if publ.len() != 32 {
        return Err(format!("DataError: bad {kind} public key (must be 32 bytes)"));
    }
    let mut v = vec![0x30, 0x2A, 0x30, 0x05, 0x06, 0x03, 0x2B, 0x65, oid, 0x03, 0x21, 0x00];
    v.extend_from_slice(publ);
    Ok(v)
}

fn okp_unwrap_pkcs8(kind: &str, der: &[u8]) -> Result<[u8; 32], String> {
    let oid = okp_oid_byte(kind)?;
    let mut want = vec![
        0x30, 0x2E, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2B, 0x65, oid, 0x04, 0x22,
        0x04, 0x20,
    ];
    want.extend_from_slice(&[0u8; 32]);
    if der.len() != 48 || der[..16] != want[..16] {
        return Err(format!("DataError: bad {kind} private key (PKCS#8)"));
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&der[16..]);
    Ok(seed)
}

fn okp_unwrap_spki(kind: &str, der: &[u8]) -> Result<[u8; 32], String> {
    let oid = okp_oid_byte(kind)?;
    let prefix = [0x30, 0x2A, 0x30, 0x05, 0x06, 0x03, 0x2B, 0x65, oid, 0x03, 0x21, 0x00];
    if der.len() != 44 || der[..12] != prefix {
        return Err(format!("DataError: bad {kind} public key (SPKI)"));
    }
    let mut publ = [0u8; 32];
    publ.copy_from_slice(&der[12..]);
    Ok(publ)
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

/// `__wjs_okp_pkcs8_from_seed(kind, seedU8)` → PKCS#8 DER。
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

/// `__wjs_okp_spki_from_pub(kind, pubU8)` → SPKI DER。
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

/// `__wjs_okp_seed_from_pkcs8(kind, derU8)` → 32B seed。
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

/// `__wjs_okp_pub_from_spki(kind, derU8)` → 32B pub。
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

/// `__wjs_ed_generate()` → 32B seed。
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
    tracing::debug!(target: "winterjs::crypto", "Ed25519 key generated");
    set_rval_bytes(&mut cx, &frame, &seed)
}

/// `__wjs_ed_public(seedU8)` → 32B pub。
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

/// `__wjs_ed_sign(seedU8, dataU8)` → 64B 签名。
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

/// `__wjs_ed_verify(pubU8, sigU8, dataU8)` → boolean。
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

#[cfg(test)]
mod c4x_tests {
    use super::{okp_unwrap_pkcs8, okp_unwrap_spki, okp_wrap_pkcs8, okp_wrap_spki};

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    // openssl 生成的独立向量（ED_SEED/ED_PUB/XA_*，见 plan c-4x）。
    const ED_SEED: &str = "b12d94858bb317baa5d40f669a784aa878bb17ad25e149e89594d7d9855b58a0";
    const ED_PUB: &str = "a9a53ddffd0e9b2d2b83eb442fac6a95391d07160fe1926f51386d31e786c869";
    const ED_PKCS8: &str = "302e020100300506032b657004220420b12d94858bb317baa5d40f669a784aa878bb17ad25e149e89594d7d9855b58a0";
    const ED_SPKI: &str = "302a300506032b6570032100a9a53ddffd0e9b2d2b83eb442fac6a95391d07160fe1926f51386d31e786c869";
    const XA_PRIV: &str = "a0e63ac582ee05d53337ba21c948389dc4e3bc0825fd506e2fa0719e038cc84d";
    const XA_PUB: &str = "8751eff746df600cb3f29b4e608b76d4c7cf6f08311f294bc9d0160af4354936";
    const XA_PKCS8: &str = "302e020100300506032b656e04220420a0e63ac582ee05d53337ba21c948389dc4e3bc0825fd506e2fa0719e038cc84d";
    const XA_SPKI: &str = "302a300506032b656e0321008751eff746df600cb3f29b4e608b76d4c7cf6f08311f294bc9d0160af4354936";

    #[test]
    fn okp_der_matches_openssl_fixtures() {
        assert_eq!(okp_wrap_pkcs8("Ed25519", &hex(ED_SEED)).unwrap(), hex(ED_PKCS8));
        assert_eq!(okp_wrap_spki("Ed25519", &hex(ED_PUB)).unwrap(), hex(ED_SPKI));
        assert_eq!(okp_unwrap_pkcs8("Ed25519", &hex(ED_PKCS8)).unwrap(), hex(ED_SEED).as_slice());
        assert_eq!(okp_unwrap_spki("Ed25519", &hex(ED_SPKI)).unwrap(), hex(ED_PUB).as_slice());
        assert_eq!(okp_wrap_pkcs8("X25519", &hex(XA_PRIV)).unwrap(), hex(XA_PKCS8));
        assert_eq!(okp_wrap_spki("X25519", &hex(XA_PUB)).unwrap(), hex(XA_SPKI));
        assert_eq!(okp_unwrap_pkcs8("X25519", &hex(XA_PKCS8)).unwrap(), hex(XA_PRIV).as_slice());
        assert_eq!(okp_unwrap_spki("X25519", &hex(XA_SPKI)).unwrap(), hex(XA_PUB).as_slice());
    }

    #[test]
    fn okp_der_rejects_wrong_oid_and_truncation() {
        // Ed 的 DER 喂给 X 解析：OID 对不上即 DataError
        assert!(okp_unwrap_pkcs8("X25519", &hex(ED_PKCS8)).is_err());
        assert!(okp_unwrap_spki("X25519", &hex(ED_SPKI)).is_err());
        // 截断/未知 kind
        assert!(okp_unwrap_pkcs8("Ed25519", &hex(ED_PKCS8)[..40]).is_err());
        assert!(okp_unwrap_spki("Ed25519", &hex(ED_SPKI)[..40]).is_err());
        assert!(okp_wrap_pkcs8("ED448", &[0u8; 32]).is_err());
        assert!(okp_wrap_pkcs8("Ed25519", &[0u8; 31]).is_err());
    }
}
