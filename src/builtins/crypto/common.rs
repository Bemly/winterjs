//! crypto 共用小件（字节/视图/曲线/哈希分发；跨族复用，pub(crate)）。

use mozjs::context::JSContext;
use mozjs::jsapi::JSObject;
use mozjs::jsval::{JSVal};
use mozjs::rooted;
use mozjs::typedarray::{CreateWith, TypedArray, Uint8};
use crate::jsapi_glue::{report_error, view_bytes, Frame};

/// 同上模式的 Uint8Array 返回（AES/HMAC 共用）。
pub(crate) fn set_rval_bytes(cx: &mut JSContext, frame: &Frame, out: &[u8]) -> bool {
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
pub(crate) fn opt_view_bytes(cx: &mut JSContext, v: JSVal, what: &str) -> Option<Option<Vec<u8>>> {
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
pub(crate) struct SystemRng;

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
pub(crate) fn rng_probe(cx: &mut JSContext) -> bool {
    if getrandom::fill(&mut [0u8; 1]).is_err() {
        report_error(cx, "OperationError: cannot get random values");
        return false;
    }
    true
}

/// base64url 无填充编码（JWK 用；解码走 prelude 已有 `__wjs2_b64urlDecode`）。
pub(crate) fn b64url(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// 大整数 → 最短大端字节（JWK `n/e/d` 等；空即 `"AA"`）。
pub(crate) fn bignum(b: &rsa::BigUint) -> String {
    let v = b.to_bytes_be();
    let v = v.as_slice();
    let mut i = 0;
    while i + 1 < v.len() && v[i] == 0 {
        i += 1;
    }
    b64url(&v[i..])
}

/// 曲线阶字节数（P-256→32，P-384→48，P-521→66，secp256k1→32；JWK 定长坐标用）。
pub(crate) fn curve_size(curve: &str) -> Option<usize> {
    match curve {
        "P-256" => Some(32),
        "P-384" => Some(48),
        "P-521" => Some(66),
        "secp256k1" => Some(32),
        _ => None,
    }
}

/// 数据哈希（ECDSA 预哈希；SHA-1/256/384/512，直引 0.11，无版本面）。
pub(crate) fn ec_hash(hash: &str, data: &[u8]) -> Result<Vec<u8>, String> {
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

/// 定长大端（JWK `x/y/d`；短即左补零，长即报错）。
pub(crate) fn fixed_pad(bytes: &[u8], size: usize) -> Result<Vec<u8>, String> {
    if bytes.len() > size {
        return Err("DataError: bad EC key (oversize coordinate)".to_string());
    }
    let mut out = vec![0u8; size];
    out[size - bytes.len()..].copy_from_slice(bytes);
    Ok(out)
}

/// MGF1（RFC 8017 B.2.1；哈希经 `x509_digest` 直算，digest 0.11 系无版本面）。
pub(crate) fn mgf1_with(hash: &str, seed: &[u8], mask_len: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(mask_len);
    let mut counter: u32 = 0;
    while out.len() < mask_len {
        let mut input = seed.to_vec();
        input.extend_from_slice(&counter.to_be_bytes());
        out.extend_from_slice(&x509_digest(hash, &input)?);
        counter = counter.wrapping_add(1);
    }
    out.truncate(mask_len);
    Ok(out)
}



// ── 椭圆曲线（ECDSA/ECDH，P-256/384/521 + secp256k1）─────────────────────────
// 三曲线同构：宏按曲线展开（`p256`/`p384`/`p521`，`ecdh`+`pkcs8` 特性已显式声明）。

/// 曲线名字典序（prelude 已归一化 `P-256` 等；此处再守一次）。
/// 绑定：曲线标记 `C`、私钥 `Secret`、公钥 `Public`、签名钥 `Signing`、验签钥 `Verifying`、签名 `Sig`。
macro_rules! with_curve {
    ($curve:expr, |$C:ident, $Secret:ident, $Public:ident, $Signing:ident, $Verifying:ident, $Sig:ident, $K:ident| $body:expr) => {{
        match $curve {
            "P-256" => {
                use p256 as $K;
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
                use p384 as $K;
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
                use p521 as $K;
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
            // 9h-1：secp256k1（k256；API 与 P-* 同构，`$K` 统一轮子路径）。
            "secp256k1" => {
                use k256 as $K;
                #[allow(dead_code)]
                type $C = k256::Secp256k1;
                #[allow(dead_code)]
                type $Secret = k256::SecretKey;
                #[allow(dead_code)]
                type $Public = k256::PublicKey;
                #[allow(dead_code)]
                type $Signing = k256::ecdsa::SigningKey;
                #[allow(dead_code)]
                type $Verifying = k256::ecdsa::VerifyingKey;
                #[allow(dead_code)]
                type $Sig = k256::ecdsa::Signature;
                $body
            }
            other => Err(format!("NotSupportedError: unsupported curve '{other}' (P-256/384/521/secp256k1)")),
        }
    }};
}


/// X.509 证书签名哈希（digest 0.11 系直算：sha1/sha2/md5 同 trait）。
pub(crate) fn x509_digest(hash: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    use sha2::Digest as _;
    Ok(match hash {
        "MD5" => md5::Md5::digest(data).to_vec(),
        "SHA-1" => sha1::Sha1::digest(data).to_vec(),
        "SHA-224" => sha2::Sha224::digest(data).to_vec(),
        "SHA-256" => sha2::Sha256::digest(data).to_vec(),
        "SHA-384" => sha2::Sha384::digest(data).to_vec(),
        "SHA-512" => sha2::Sha512::digest(data).to_vec(),
        other => return Err(format!("NotSupportedError: unsupported cert hash '{other}'")),
    })
}
