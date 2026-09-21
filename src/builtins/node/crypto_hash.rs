//! crypto hash 域（流式 Hash/HMAC/XOF；对齐 crypto.rs；纯搬移）。

use std::cell::RefCell;
use std::collections::HashMap;
use mozjs::jsval::{JSVal, UndefinedValue};
use crate::jsapi_glue::{report_error, value_to_string, view_bytes, wrap_cx, Frame};
use super::crypto::{arg_id, set_rval_bytes, set_rval_str};

/// 增量摘要态（RustCrypto `Digest` 全员 `Clone`，`copy()` 语义天然；
/// SHAKE 系 `tiny_keccak::Shake` 同样 `Clone`，输出长存在注册时）。
pub(crate) enum HashJob {
    Sha1(sha1::Sha1),
    Sha224(sha2::Sha224),
    Sha256(sha2::Sha256),
    Sha384(sha2::Sha384),
    Sha512(sha2::Sha512),
    Md5(md5::Md5),
    Sha3_256(sha3::Sha3_256),
    Sha3_384(sha3::Sha3_384),
    Sha3_512(sha3::Sha3_512),
    Blake2b512(blake2::Blake2b512),
    Blake2s256(blake2::Blake2s256),
    Ripemd160(ripemd::Ripemd160),
    Shake128(tiny_keccak::Shake, usize),
    Shake256(tiny_keccak::Shake, usize),
}

thread_local! {
    static HASHERS: RefCell<HashMap<u64, HashJob>> = RefCell::new(HashMap::new());
    static HASH_NEXT: RefCell<u64> = RefCell::new(1);
}

/// 算法归一（纯函数，单元测试覆盖）：小写去 `-_`，可剥 `RSA-` 前缀。
/// `xof_len` 只对 shake 系有效（定长算法忽略；长度校验在 JS 侧，见 DEP0198）。
pub(crate) fn norm_hash(name: &str, xof_len: usize) -> Option<HashJob> {
    use sha2::Digest as _; // 全员 digest 0.11 系（sha1/sha2/md5/sha3/blake2/ripemd 同 trait，无需直引 digest，见 §0.5 零新增）
    let flat: String = name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| *c != '-' && *c != '_')
        .collect();
    let flat = flat.strip_prefix("rsa").unwrap_or(&flat);
    match flat {
        "sha1" | "dss1" => Some(HashJob::Sha1(sha1::Sha1::new())),
        "sha224" => Some(HashJob::Sha224(sha2::Sha224::new())),
        "sha256" => Some(HashJob::Sha256(sha2::Sha256::new())),
        "sha384" => Some(HashJob::Sha384(sha2::Sha384::new())),
        "sha512" => Some(HashJob::Sha512(sha2::Sha512::new())),
        "md5" => Some(HashJob::Md5(md5::Md5::new())),
        "sha3256" => Some(HashJob::Sha3_256(sha3::Sha3_256::new())),
        "sha3384" => Some(HashJob::Sha3_384(sha3::Sha3_384::new())),
        "sha3512" => Some(HashJob::Sha3_512(sha3::Sha3_512::new())),
        "blake2b512" => Some(HashJob::Blake2b512(blake2::Blake2b512::new())),
        "blake2s256" => Some(HashJob::Blake2s256(blake2::Blake2s256::new())),
        "ripemd160" | "ripemd" => Some(HashJob::Ripemd160(ripemd::Ripemd160::new())),
        "shake128" => Some(HashJob::Shake128(tiny_keccak::Shake::v128(), xof_len)),
        "shake256" => Some(HashJob::Shake256(tiny_keccak::Shake::v256(), xof_len)),
        _ => None,
    }
}

pub(crate) fn hash_alloc(job: HashJob) -> u64 {
    HASH_NEXT.with(|n| {
        HASHERS.with(|m| {
            let mut n = n.borrow_mut();
            let id = *n;
            *n = n.wrapping_add(1).max(1);
            m.borrow_mut().insert(id, job);
            id
        })
    })
}
/// `__wjs_crypto_hash_new(alg)` → id 字符串；未知算法报无码原文（Node 同款）。
pub unsafe extern "C" fn crypto_hash_new(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: hash needs an algorithm");
        return false;
    }
    let alg = value_to_string(&mut cx, frame.arg(0));
    // 可选第 2 参：XOF 输出长（定长算法忽略；JS 侧已落默认值，见 DEP0198）。
    let xof_len = if frame.argc() >= 2 {
        value_to_string(&mut cx, frame.arg(1)).parse::<usize>().unwrap_or(0)
    } else {
        0
    };
    match norm_hash(&alg, xof_len) {
        Some(job) => {
            let id = hash_alloc(job);
            set_rval_str(&mut cx, &frame, &id.to_string());
            true
        }
        // 无 ERR_ 前缀：JS 侧直抛无码 Error（真 Node `Digest method not supported` 同款）
        None => {
            report_error(&mut cx, "Digest method not supported");
            false
        }
    }
}

/// `__wjs_crypto_hash_update(idStr, bytes)`；句柄已消费报 FINALIZED（Node 同款码）。
pub unsafe extern "C" fn crypto_hash_update(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = arg_id(&frame, 0, "hash update", &mut cx) else {
        return false;
    };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: hash update needs data");
        return false;
    }
    let data = match view_bytes(&mut cx, frame.arg(1), "hash update data") {
        Some(b) => b,
        None => return false,
    };
    use sha2::Digest as _; // 全员 digest 0.11 系（sha1/sha2/md5/sha3/blake2/ripemd 同 trait，无需直引 digest，见 §0.5 零新增）
    use tiny_keccak::Hasher as _; // SHAKE 系独立 trait（XOF 变长）
    let ok = HASHERS.with(|m| {
        let mut m = m.borrow_mut();
        let Some(job) = m.get_mut(&id) else {
            return false;
        };
        match job {
            HashJob::Sha1(h) => h.update(&data),
            HashJob::Sha224(h) => h.update(&data),
            HashJob::Sha256(h) => h.update(&data),
            HashJob::Sha384(h) => h.update(&data),
            HashJob::Sha512(h) => h.update(&data),
            HashJob::Md5(h) => h.update(&data),
            HashJob::Sha3_256(h) => h.update(&data),
            HashJob::Sha3_384(h) => h.update(&data),
            HashJob::Sha3_512(h) => h.update(&data),
            HashJob::Blake2b512(h) => h.update(&data),
            HashJob::Blake2s256(h) => h.update(&data),
            HashJob::Ripemd160(h) => h.update(&data),
            HashJob::Shake128(h, _) => h.update(&data),
            HashJob::Shake256(h, _) => h.update(&data),
        }
        true
    });
    if !ok {
        report_error(&mut cx, "ERR_CRYPTO_HASH_FINALIZED: Digest already called");
        return false;
    }
    frame.set_rval(UndefinedValue());
    true
}

/// `__wjs_crypto_hash_digest(idStr)` → Uint8Array（消费句柄；二次调报 FINALIZED）。
pub unsafe extern "C" fn crypto_hash_digest(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = arg_id(&frame, 0, "hash digest", &mut cx) else {
        return false;
    };
    use sha2::Digest as _; // 全员 digest 0.11 系（sha1/sha2/md5/sha3/blake2/ripemd 同 trait，无需直引 digest，见 §0.5 零新增）
    use tiny_keccak::Hasher as _; // SHAKE 系独立 trait（XOF 变长）
    let out: Option<Vec<u8>> = HASHERS.with(|m| {
        m.borrow_mut().remove(&id).map(|job| match job {
            HashJob::Sha1(h) => h.finalize().to_vec(),
            HashJob::Sha224(h) => h.finalize().to_vec(),
            HashJob::Sha256(h) => h.finalize().to_vec(),
            HashJob::Sha384(h) => h.finalize().to_vec(),
            HashJob::Sha512(h) => h.finalize().to_vec(),
            HashJob::Md5(h) => h.finalize().to_vec(),
            HashJob::Sha3_256(h) => h.finalize().to_vec(),
            HashJob::Sha3_384(h) => h.finalize().to_vec(),
            HashJob::Sha3_512(h) => h.finalize().to_vec(),
            HashJob::Blake2b512(h) => h.finalize().to_vec(),
            HashJob::Blake2s256(h) => h.finalize().to_vec(),
            HashJob::Ripemd160(h) => h.finalize().to_vec(),
            HashJob::Shake128(h, n) => {
                let mut out = vec![0u8; n];
                h.finalize(&mut out);
                out
            }
            HashJob::Shake256(h, n) => {
                let mut out = vec![0u8; n];
                h.finalize(&mut out);
                out
            }
        })
    });
    match out {
        Some(bytes) => set_rval_bytes(&mut cx, &frame, &bytes),
        None => {
            report_error(&mut cx, "ERR_CRYPTO_HASH_FINALIZED: Digest already called");
            false
        }
    }
}

/// `__wjs_crypto_hash_copy(idStr)` → 新 id 字符串（中间态克隆；已消费报 FINALIZED）。
pub unsafe extern "C" fn crypto_hash_copy(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = arg_id(&frame, 0, "hash copy", &mut cx) else {
        return false;
    };
    let cloned: Option<HashJob> = HASHERS.with(|m| {
        m.borrow()
            .get(&id)
            .map(|job| match job {
                HashJob::Sha1(h) => HashJob::Sha1(h.clone()),
                HashJob::Sha224(h) => HashJob::Sha224(h.clone()),
                HashJob::Sha256(h) => HashJob::Sha256(h.clone()),
                HashJob::Sha384(h) => HashJob::Sha384(h.clone()),
                HashJob::Sha512(h) => HashJob::Sha512(h.clone()),
                HashJob::Md5(h) => HashJob::Md5(h.clone()),
                HashJob::Sha3_256(h) => HashJob::Sha3_256(h.clone()),
                HashJob::Sha3_384(h) => HashJob::Sha3_384(h.clone()),
                HashJob::Sha3_512(h) => HashJob::Sha3_512(h.clone()),
                HashJob::Blake2b512(h) => HashJob::Blake2b512(h.clone()),
                HashJob::Blake2s256(h) => HashJob::Blake2s256(h.clone()),
                HashJob::Ripemd160(h) => HashJob::Ripemd160(h.clone()),
                HashJob::Shake128(h, n) => HashJob::Shake128(h.clone(), *n),
                HashJob::Shake256(h, n) => HashJob::Shake256(h.clone(), *n),
            })
    });
    match cloned {
        Some(job) => {
            let nid = hash_alloc(job);
            set_rval_str(&mut cx, &frame, &nid.to_string());
            true
        }
        None => {
            report_error(&mut cx, "ERR_CRYPTO_HASH_FINALIZED: Digest already called");
            false
        }
    }
}

/// `__wjs_crypto_hash_set_len(idStr, lenNum)` → "1"（仅 Shake 改输出长；其余报态错）。
/// 10f crypto首轮：`copy({ outputLength })` 改长通道（JS 侧已校验，见 `__checkOutputLength`）。
pub unsafe extern "C" fn crypto_hash_set_len(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = arg_id(&frame, 0, "hash set length", &mut cx) else {
        return false;
    };
    if frame.argc() < 2 || !frame.arg(1).is_number() {
        report_error(&mut cx, "TypeError: hash set length needs a length");
        return false;
    }
    let len = frame.arg(1).to_number() as usize;
    let ok = HASHERS.with(|m| {
        let mut m = m.borrow_mut();
        match m.get_mut(&id) {
            Some(HashJob::Shake128(_, n)) => {
                *n = len;
                true
            }
            Some(HashJob::Shake256(_, n)) => {
                *n = len;
                true
            }
            Some(_) => false,
            None => false,
        }
    });
    if ok {
        set_rval_str(&mut cx, &frame, "1");
        true
    } else {
        report_error(&mut cx, "ERR_CRYPTO_INVALID_STATE: Invalid state");
        false
    }
}

