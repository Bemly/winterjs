//! crypto cipher 域（对称/CCM/GCM/ChaCha；对齐 crypto.rs；纯搬移）。

use std::cell::RefCell;
use std::collections::HashMap;
use mozjs::jsval::JSVal;

use crate::jsapi_glue::{report_error, value_to_string, view_bytes, wrap_cx, Frame};
use super::crypto::{arg_id, opt_view, set_rval_bytes, set_rval_str};

// ── 9e-1b 对称密码（CBC/CTR 真流式注册表；GCM/ChaCha 在 JS 侧 buffered）─────

// cipher 0.5 系经 `aes` 重导出直用（零新增，`digest` 同款口径）。
use aes::cipher::block::{BlockCipherDecrypt, BlockCipherEncrypt, BlockModeDecrypt, BlockModeEncrypt};
use aes::cipher::{Block, BlockSizeUser, Key};

/// CBC 加密作业（整块原地；`pending` 攒不足块，`final` 时 PKCS#7）。
trait CbcEncJob {
    fn enc_blocks(&mut self, data: &mut [u8]);
}
/// CBC 解密作业（解密时永远扣留最后一块，`final` 定夺填充）。
trait CbcDecJob {
    fn dec_blocks(&mut self, data: &mut [u8]);
}
/// CTR 作业（连续 keystream，无块边界概念）。
trait CtrJob {
    fn apply(&mut self, data: &mut [u8]);
}

struct CbcE<D: BlockCipherEncrypt>(cbc::Encryptor<D>);
struct CbcD<D: BlockCipherDecrypt>(cbc::Decryptor<D>);
/// CTR 内核（AES 三档单态枚举；泛型写法需 typenum 块约束，得不偿失）。
enum CtrInner {
    Aes128(ctr::Ctr128BE<aes::Aes128>),
    Aes192(ctr::Ctr128BE<aes::Aes192>),
    Aes256(ctr::Ctr128BE<aes::Aes256>),
}
struct CtrX(CtrInner);

/// 字节片 ↔ 块向量（拷贝一次；正确优先，块粒度下开销可忽略）。
/// `filter_map` 而非 `expect`：native 内禁 panic（见 §4.14），余块本就由 `pending` 持有。
pub(crate) fn to_blocks<D: BlockSizeUser>(data: &[u8]) -> Vec<Block<D>> {
    data.chunks_exact(D::block_size())
        .filter_map(|c| Block::<D>::try_from(c).ok())
        .collect()
}

pub(crate) fn from_blocks<D: BlockSizeUser>(blocks: &[Block<D>]) -> Vec<u8> {
    let bs = D::block_size();
    let mut out = Vec::with_capacity(blocks.len() * bs);
    for b in blocks {
        out.extend_from_slice(b);
    }
    out
}

impl<D: BlockCipherEncrypt> CbcEncJob for CbcE<D> {
    fn enc_blocks(&mut self, data: &mut [u8]) {
        let mut blocks = to_blocks::<D>(data);
        self.0.encrypt_blocks(&mut blocks);
        let flat = from_blocks::<D>(&blocks);
        data.copy_from_slice(&flat);
    }
}

impl<D: BlockCipherDecrypt> CbcDecJob for CbcD<D> {
    fn dec_blocks(&mut self, data: &mut [u8]) {
        let mut blocks = to_blocks::<D>(data);
        self.0.decrypt_blocks(&mut blocks);
        let flat = from_blocks::<D>(&blocks);
        data.copy_from_slice(&flat);
    }
}

impl CtrJob for CtrX {
    fn apply(&mut self, data: &mut [u8]) {
        use aes::cipher::StreamCipher as _;
        match &mut self.0 {
            CtrInner::Aes128(c) => c.apply_keystream(data),
            CtrInner::Aes192(c) => c.apply_keystream(data),
            CtrInner::Aes256(c) => c.apply_keystream(data),
        }
    }
}

enum CipherJob {
    CbcEnc { job: Box<dyn CbcEncJob>, pending: Vec<u8>, block: usize, autopad: bool },
    CbcDec { job: Box<dyn CbcDecJob>, pending: Vec<u8>, block: usize, autopad: bool },
    Ctr { job: Box<dyn CtrJob> },
    // 10f crypto首轮：ECB（`aes` 轮子已在树内；填充由 final 处理，解密 autopad 扣尾块）。
    Ecb { enc: bool, kind: u8, key: Vec<u8>, pending: Vec<u8>, autopad: bool },
}

thread_local! {
    static CIPHERS: RefCell<HashMap<u64, CipherJob>> = RefCell::new(HashMap::new());
    static CIPHER_NEXT: RefCell<u64> = RefCell::new(1);
}

fn cipher_alloc(job: CipherJob) -> u64 {
    CIPHER_NEXT.with(|n| {
        CIPHERS.with(|m| {
            let mut n = n.borrow_mut();
            let id = *n;
            *n = n.wrapping_add(1).max(1);
            m.borrow_mut().insert(id, job);
            id
        })
    })
}

/// 对称算法表（纯函数，单元测试覆盖）：名 →（族，密钥长，iv 长，块）。
pub(crate) fn cipher_params(alg: &str) -> Option<(&'static str, usize, usize, usize)> {
    match alg.trim().to_ascii_lowercase().as_str() {
        "aes-128-cbc" => Some(("cbc-aes128", 16, 16, 16)),
        "aes-192-cbc" => Some(("cbc-aes192", 24, 16, 16)),
        "aes-256-cbc" => Some(("cbc-aes256", 32, 16, 16)),
        "aes-128-ctr" => Some(("ctr-aes128", 16, 16, 16)),
        "aes-192-ctr" => Some(("ctr-aes192", 24, 16, 16)),
        "aes-256-ctr" => Some(("ctr-aes256", 32, 16, 16)),
        "des-ede3-cbc" => Some(("cbc-des3", 24, 8, 8)),
        // 10f crypto首轮：ECB 三档（无 iv，iv 长记 0；nid 真机 418/422/426）。
        "aes-128-ecb" => Some(("ecb-aes128", 16, 0, 16)),
        "aes-192-ecb" => Some(("ecb-aes192", 24, 0, 16)),
        "aes-256-ecb" => Some(("ecb-aes256", 32, 0, 16)),
        _ => None,
    }
}

/// ECB 单块直通（`aes` 轮子已在树内，零新增；调用方保证整块，填充另行处理）。
fn ecb_blocks(kind: u8, key: &[u8], enc: bool, chunk: &mut [u8]) {
    use aes::cipher::KeyInit as _;
    debug_assert!(chunk.len() % 16 == 0);
    macro_rules! go {
        ($e:ty, $d:ty) => {{
            // 经 `[u8; 16]` 中转（`from_mut_slice` 已废弃、`TryFrom<&mut [u8]>`
            // 在所钉版本未实现；`From<[u8; 16]>` 为稳定 API）。
            if enc {
                let c = <$e>::new_from_slice(key).expect("ecb key length checked at new");
                for b in chunk.chunks_mut(16) {
                    let mut arr = [0u8; 16];
                    arr.copy_from_slice(b);
                    let mut block = Block::<$e>::from(arr);
                    c.encrypt_block(&mut block);
                    b.copy_from_slice(block.as_slice());
                }
            } else {
                let c = <$d>::new_from_slice(key).expect("ecb key length checked at new");
                for b in chunk.chunks_mut(16) {
                    let mut arr = [0u8; 16];
                    arr.copy_from_slice(b);
                    let mut block = Block::<$d>::from(arr);
                    c.decrypt_block(&mut block);
                    b.copy_from_slice(block.as_slice());
                }
            }
        }};
    }
    match kind {
        0 => go!(aes::Aes128, aes::Aes128),
        1 => go!(aes::Aes192, aes::Aes192),
        _ => go!(aes::Aes256, aes::Aes256),
    }
}

pub(crate) fn pkcs7_pad(block: usize, mut data: Vec<u8>) -> Vec<u8> {
    let pad = block - (data.len() % block);
    data.extend(std::iter::repeat(pad as u8).take(pad));
    data
}

/// PKCS#7 校验剥离（非恒定时间实现，见头注记档）。
pub(crate) fn pkcs7_unpad(block: usize, data: &[u8]) -> Option<Vec<u8>> {
    let n = *data.last()? as usize;
    if n == 0 || n > block || n > data.len() {
        return None;
    }
    if !data[data.len() - n..].iter().all(|&b| b as usize == n) {
        return None;
    }
    Some(data[..data.len() - n].to_vec())
}

/// `__wjs2_cipher_new(alg, keyU8, ivU8, encNum, autoPadNum)` → id 字符串。
pub unsafe extern "C" fn cipher_new(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 5 {
        report_error(&mut cx, "TypeError: cipher needs algorithm, key, iv, mode and padding");
        return false;
    }
    let alg = value_to_string(&mut cx, frame.arg(0));
    let (Some(key), Some(iv)) = (
        view_bytes(&mut cx, frame.arg(1), "cipher key"),
        view_bytes(&mut cx, frame.arg(2), "cipher iv"),
    ) else {
        return false;
    };
    let enc = frame.arg(3).is_number() && frame.arg(3).to_number() != 0.0;
    let autopad = !(frame.arg(4).is_number() && frame.arg(4).to_number() == 0.0);
    let Some((fam, klen, ivlen, block)) = cipher_params(&alg) else {
        report_error(&mut cx, "ERR_CRYPTO_UNKNOWN_CIPHER: Unknown cipher");
        return false;
    };
    if key.len() != klen {
        report_error(&mut cx, "ERR_CRYPTO_INVALID_KEYLEN: Invalid key length");
        return false;
    }
    if iv.len() != ivlen {
        report_error(&mut cx, "ERR_CRYPTO_INVALID_IV: Invalid initialization vector");
        return false;
    }
    use aes::cipher::KeyIvInit as _;
    macro_rules! cbc_pair {
        ($e:ty, $d:ty) => {{
            let (Ok(ke), Ok(ive), Ok(kd), Ok(ivd)) = (
                Key::<$e>::try_from(key.as_slice()),
                Block::<$e>::try_from(iv.as_slice()),
                Key::<$d>::try_from(key.as_slice()),
                Block::<$d>::try_from(iv.as_slice()),
            ) else {
                report_error(&mut cx, "ERR_CRYPTO_INVALID_KEYLEN: Invalid key length");
                return false;
            };
            if enc {
                CipherJob::CbcEnc {
                    job: Box::new(CbcE(<cbc::Encryptor<$e>>::new(&ke, &ive))),
                    pending: Vec::new(),
                    block,
                    autopad,
                }
            } else {
                CipherJob::CbcDec {
                    job: Box::new(CbcD(<cbc::Decryptor<$d>>::new(&kd, &ivd))),
                    pending: Vec::new(),
                    block,
                    autopad,
                }
            }
        }};
    }
    let job = match fam {
        "cbc-aes128" => cbc_pair!(aes::Aes128, aes::Aes128),
        "cbc-aes192" => cbc_pair!(aes::Aes192, aes::Aes192),
        "cbc-aes256" => cbc_pair!(aes::Aes256, aes::Aes256),
        "cbc-des3" => cbc_pair!(des::TdesEde3, des::TdesEde3),
        "ecb-aes128" => CipherJob::Ecb { enc, kind: 0, key: key.clone(), pending: Vec::new(), autopad },
        "ecb-aes192" => CipherJob::Ecb { enc, kind: 1, key: key.clone(), pending: Vec::new(), autopad },
        "ecb-aes256" => CipherJob::Ecb { enc, kind: 2, key: key.clone(), pending: Vec::new(), autopad },
        "ctr-aes128" => {
            let (Ok(ke), Ok(ive)) = (
                Key::<aes::Aes128>::try_from(key.as_slice()),
                Block::<aes::Aes128>::try_from(iv.as_slice()),
            ) else {
                report_error(&mut cx, "ERR_CRYPTO_INVALID_KEYLEN: Invalid key length");
                return false;
            };
            CipherJob::Ctr {
                job: Box::new(CtrX(CtrInner::Aes128(<ctr::Ctr128BE<aes::Aes128>>::new(&ke, &ive)))),
            }
        },
        "ctr-aes192" => {
            let (Ok(ke), Ok(ive)) = (
                Key::<aes::Aes192>::try_from(key.as_slice()),
                Block::<aes::Aes192>::try_from(iv.as_slice()),
            ) else {
                report_error(&mut cx, "ERR_CRYPTO_INVALID_KEYLEN: Invalid key length");
                return false;
            };
            CipherJob::Ctr {
                job: Box::new(CtrX(CtrInner::Aes192(<ctr::Ctr128BE<aes::Aes192>>::new(&ke, &ive)))),
            }
        },
        "ctr-aes256" => {
            let (Ok(ke), Ok(ive)) = (
                Key::<aes::Aes256>::try_from(key.as_slice()),
                Block::<aes::Aes256>::try_from(iv.as_slice()),
            ) else {
                report_error(&mut cx, "ERR_CRYPTO_INVALID_KEYLEN: Invalid key length");
                return false;
            };
            CipherJob::Ctr {
                job: Box::new(CtrX(CtrInner::Aes256(<ctr::Ctr128BE<aes::Aes256>>::new(&ke, &ive)))),
            }
        },
        _ => {
            report_error(&mut cx, "ERR_CRYPTO_UNKNOWN_CIPHER: Unknown cipher");
            return false;
        }
    };
    let id = cipher_alloc(job);
    set_rval_str(&mut cx, &frame, &id.to_string());
    true
}

/// `__wjs2_cipher_update(idStr, bytesU8)` → Uint8Array。
pub unsafe extern "C" fn cipher_update(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = arg_id(&frame, 0, "cipher update", &mut cx) else {
        return false;
    };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: cipher update needs data");
        return false;
    }
    let data = match view_bytes(&mut cx, frame.arg(1), "cipher update data") {
        Some(b) => b,
        None => return false,
    };
    let out = CIPHERS.with(|m| {
        let mut m = m.borrow_mut();
        let Some(job) = m.get_mut(&id) else {
            return None;
        };
        Some(match job {
            CipherJob::CbcEnc { job, pending, block, .. } => {
                pending.extend_from_slice(&data);
                let n = pending.len() / *block * *block;
                let mut chunk: Vec<u8> = pending.drain(..n).collect();
                job.enc_blocks(&mut chunk);
                chunk
            }
            CipherJob::CbcDec { job, pending, block, .. } => {
                pending.extend_from_slice(&data);
                // 永远扣留最后一块（`final` 定夺填充）
                let n = pending.len().saturating_sub(*block) / *block * *block;
                let n = n.min(pending.len().saturating_sub(*block));
                let mut chunk: Vec<u8> = pending.drain(..n).collect();
                job.dec_blocks(&mut chunk);
                chunk
            }
            CipherJob::Ctr { job } => {
                let mut chunk = data;
                job.apply(&mut chunk);
                chunk
            }
            CipherJob::Ecb { enc, kind, key, pending, autopad } => {
                pending.extend_from_slice(&data);
                // 解密 autopad 扣留尾块（final 定夺填充），余者整块直通。
                let n = if !*enc && *autopad {
                    pending.len().saturating_sub(16) / 16 * 16
                } else {
                    pending.len() / 16 * 16
                };
                let mut chunk: Vec<u8> = pending.drain(..n).collect();
                ecb_blocks(*kind, key, *enc, &mut chunk);
                chunk
            }
        })
    });
    match out {
        Some(bytes) => set_rval_bytes(&mut cx, &frame, &bytes),
        None => {
            report_error(&mut cx, "ERR_CRYPTO_INVALID_STATE: Invalid state");
            false
        }
    }
}

/// `__wjs2_cipher_final(idStr)` → Uint8Array（消费句柄）。
pub unsafe extern "C" fn cipher_final(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = arg_id(&frame, 0, "cipher final", &mut cx) else {
        return false;
    };
    let out: Option<Result<Vec<u8>, String>> = CIPHERS.with(|m| {
        m.borrow_mut().remove(&id).map(|mut job| match &mut job {
            CipherJob::CbcEnc { job, pending, block, autopad } => {
                if *autopad {
                    let mut chunk = pkcs7_pad(*block, std::mem::take(pending));
                    job.enc_blocks(&mut chunk);
                    Ok(chunk)
                } else if pending.len() % *block == 0 {
                    let mut chunk = std::mem::take(pending);
                    job.enc_blocks(&mut chunk);
                    Ok(chunk)
                } else {
                    Err("ERR_OSSL_WRONG_FINAL_BLOCK_LENGTH: wrong final block length".into())
                }
            }
            CipherJob::CbcDec { job, pending, block, autopad } => {
                if pending.len() % *block != 0 || (*autopad && pending.is_empty()) {
                    return Err("ERR_OSSL_WRONG_FINAL_BLOCK_LENGTH: wrong final block length".into());
                }
                let mut chunk = std::mem::take(pending);
                job.dec_blocks(&mut chunk);
                if *autopad {
                    // 填充内容坏（非长度问题）→ 真机口径 BAD_DECRYPT（与长度错区分，见套件 109 行）。
                    pkcs7_unpad(*block, &chunk).ok_or_else(|| {
                        "ERR_OSSL_BAD_DECRYPT: bad decrypt".to_string()
                    })
                } else {
                    Ok(chunk)
                }
            }
            CipherJob::Ctr { .. } => Ok(Vec::new()),
            CipherJob::Ecb { enc, kind, key, pending, autopad } => {
                if *enc {
                    let mut chunk = if *autopad {
                        pkcs7_pad(16, std::mem::take(pending))
                    } else if pending.len() % 16 == 0 {
                        std::mem::take(pending)
                    } else {
                        return Err("ERR_OSSL_WRONG_FINAL_BLOCK_LENGTH: wrong final block length".into());
                    };
                    ecb_blocks(*kind, key, true, &mut chunk);
                    Ok(chunk)
                } else {
                    if pending.len() % 16 != 0 || (*autopad && pending.is_empty()) {
                        return Err("ERR_OSSL_WRONG_FINAL_BLOCK_LENGTH: wrong final block length".into());
                    }
                    let mut chunk = std::mem::take(pending);
                    ecb_blocks(*kind, key, false, &mut chunk);
                    if *autopad {
                        pkcs7_unpad(16, &chunk).ok_or_else(|| {
                            "ERR_OSSL_BAD_DECRYPT: bad decrypt".to_string()
                        })
                    } else {
                        Ok(chunk)
                    }
                }
            }
        })
    });
    match out {
        Some(Ok(bytes)) => set_rval_bytes(&mut cx, &frame, &bytes),
        Some(Err(e)) => {
            report_error(&mut cx, &e);
            false
        }
        None => {
            report_error(&mut cx, "ERR_CRYPTO_INVALID_STATE: Invalid state");
            false
        }
    }
}

/// `__wjs2_cipher_set_autopad(idStr, flagNum)` → undefined（原位改 flag，不消费句柄）。
/// UNSAFE-BOUNDARY：前置条件 = 引擎回调提供的 raw cx 有效 + `Frame::from_raw(vp, argc)`
/// 的调用约定成立（与本文件其余 cipher 系 natives 同）；覆盖测试
/// `tests/node/crypto/cipher.rs::p2_crypto_cipher_setautopadding`（含非法 id 的
/// panic 路径用例：`ERR_CRYPTO_INVALID_STATE`）。
pub unsafe extern "C" fn cipher_set_autopad(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = arg_id(&frame, 0, "cipher set autopad", &mut cx) else {
        return false;
    };
    let flag = !(frame.argc() > 1 && frame.arg(1).is_number() && frame.arg(1).to_number() == 0.0);
    let found = CIPHERS.with(|m| {
        let mut m = m.borrow_mut();
        match m.get_mut(&id) {
            Some(CipherJob::CbcEnc { autopad, .. }) => {
                *autopad = flag;
                true
            }
            Some(CipherJob::CbcDec { autopad, .. }) => {
                *autopad = flag;
                true
            }
            Some(CipherJob::Ecb { autopad, .. }) => {
                *autopad = flag;
                true
            }
            Some(CipherJob::Ctr { .. }) => true,
            None => false,
        }
    });
    if !found {
        report_error(&mut cx, "ERR_CRYPTO_INVALID_STATE: Invalid state");
        return false;
    }
    frame.set_rval(mozjs::jsval::UndefinedValue());
    true
}

/// `__wjs2_cipher_chacha(encNum, keyU8, nonceU8, aadOrNull, dataU8, tagOrNull)`：
/// enc=1 → ct‖tag16；enc=0 → pt（tag 必给，认证失败报原文无码错，Node 同款）。
pub unsafe extern "C" fn cipher_chacha(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 6 {
        report_error(&mut cx, "TypeError: chacha needs mode, key, nonce, aad, data and tag");
        return false;
    }
    let enc = !(frame.arg(0).is_number() && frame.arg(0).to_number() == 0.0);
    let (Some(key), Some(nonce), Some(aad), Some(data), Some(tag)) = (
        view_bytes(&mut cx, frame.arg(1), "chacha key"),
        view_bytes(&mut cx, frame.arg(2), "chacha nonce"),
        opt_view(&mut cx, frame.arg(3), "chacha aad"),
        view_bytes(&mut cx, frame.arg(4), "chacha data"),
        opt_view(&mut cx, frame.arg(5), "chacha tag"),
    ) else {
        return false;
    };
    if key.len() != 32 {
        report_error(&mut cx, "ERR_CRYPTO_INVALID_KEYLEN: Invalid key length");
        return false;
    }
    if nonce.len() != 12 {
        report_error(&mut cx, "ERR_CRYPTO_INVALID_IV: Invalid initialization vector");
        return false;
    }
    use chacha20poly1305::aead::{Aead as _, KeyInit as _, Payload};
    let cipher = chacha20poly1305::ChaCha20Poly1305::new_from_slice(&key)
        .map_err(|e| format!("OperationError: {e}"));
    let cipher = match cipher {
        Ok(c) => c,
        Err(e) => {
            report_error(&mut cx, &e);
            return false;
        }
    };
    let nonce = match chacha20poly1305::Nonce::try_from(nonce.as_slice()) {
        Ok(n) => n,
        Err(_) => {
            report_error(&mut cx, "ERR_CRYPTO_INVALID_IV: Invalid initialization vector");
            return false;
        }
    };
    let aad_ref = aad.as_deref().unwrap_or(&[]);
    if enc {
        match cipher.encrypt(&nonce, Payload { msg: &data, aad: aad_ref }) {
            Ok(out) => set_rval_bytes(&mut cx, &frame, &out),
            Err(e) => {
                report_error(&mut cx, &format!("OperationError: chacha encrypt failed: {e}"));
                false
            }
        }
    } else {
        let Some(tag) = tag else {
            report_error(&mut cx, "Unsupported state or unable to authenticate data");
            return false;
        };
        let mut input = data;
        input.extend_from_slice(&tag);
        match cipher.decrypt(&nonce, Payload { msg: &input, aad: aad_ref }) {
            Ok(out) => set_rval_bytes(&mut cx, &frame, &out),
            Err(_) => {
                report_error(&mut cx, "Unsupported state or unable to authenticate data");
                false
            }
        }
    }
}

// ── 10e AES-CCM（`ccm` 0.6 直引；NIST SP 800-38D；ghash/J0 见 GCM 节）───────

/// CCM 组合分发（密钥 16/24/32 × nonce 7–13 × tag 4/6/…/16 全档；147 单态，
/// 薄泛型，编译期展开）。
/// enc=true → ct‖tag；enc=false → pt（input 为 ct‖tag；失败报原文无码错，Node 同款）。
pub(crate) fn ccm_crypt(
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    input: &[u8],
    tag_len: usize,
    enc: bool,
) -> Result<Vec<u8>, String> {
    use ccm::aead::{Aead as _, KeyInit as _, Payload};
    // `Ccm<C, M, N>`：M = tag 长，N = nonce 长（上游注记，非直觉顺序）。
    macro_rules! run {
        ($aes:ty, $tlen:ty, $nlen:ty) => {{
            let cipher = ccm::Ccm::<$aes, $tlen, $nlen>::new_from_slice(key)
                .map_err(|e| format!("OperationError: {e}"))?;
            let n = ccm::Nonce::<$nlen>::try_from(nonce)
                .map_err(|_| "ERR_CRYPTO_INVALID_IV: Invalid initialization vector".to_string())?;
            if enc {
                cipher
                    .encrypt(&n, Payload { msg: input, aad })
                    .map_err(|e| format!("OperationError: ccm encrypt failed: {e}"))
            } else {
                cipher
                    .decrypt(&n, Payload { msg: input, aad })
                    .map_err(|_| "Unsupported state or unable to authenticate data".to_string())
            }
        }};
    }
    macro_rules! on_nonce {
        ($aes:ty, $tlen:ty) => {
            match nonce.len() {
                7 => run!($aes, $tlen, ccm::consts::U7),
                8 => run!($aes, $tlen, ccm::consts::U8),
                9 => run!($aes, $tlen, ccm::consts::U9),
                10 => run!($aes, $tlen, ccm::consts::U10),
                11 => run!($aes, $tlen, ccm::consts::U11),
                12 => run!($aes, $tlen, ccm::consts::U12),
                13 => run!($aes, $tlen, ccm::consts::U13),
                _ => Err("ERR_CRYPTO_INVALID_IV: Invalid initialization vector".to_string()),
            }
        };
    }
    macro_rules! on_tag {
        ($aes:ty) => {
            match tag_len {
                4 => on_nonce!($aes, ccm::consts::U4),
                6 => on_nonce!($aes, ccm::consts::U6),
                8 => on_nonce!($aes, ccm::consts::U8),
                10 => on_nonce!($aes, ccm::consts::U10),
                12 => on_nonce!($aes, ccm::consts::U12),
                14 => on_nonce!($aes, ccm::consts::U14),
                16 => on_nonce!($aes, ccm::consts::U16),
                _ => Err("ERR_CRYPTO_INVALID_AUTH_TAG: Invalid authentication tag length".to_string()),
            }
        };
    }
    match key.len() {
        16 => on_tag!(aes::Aes128),
        24 => on_tag!(aes::Aes192),
        32 => on_tag!(aes::Aes256),
        _ => Err("ERR_CRYPTO_INVALID_KEYLEN: Invalid key length".to_string()),
    }
}

/// `__wjs2_ccm_crypt(encNum, keyU8, nonceU8, aadU8, dataU8, tagU8OrNull, tagLenNum)`：
/// enc=1 → ct‖tag；enc=0 → pt（tag 必给；tag 长须等于 tagLen，认证失败原文无码错）。
pub unsafe extern "C" fn ccm_crypt_native(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 7 {
        report_error(&mut cx, "TypeError: ccm needs mode, key, nonce, aad, data, tag and tag length");
        return false;
    }
    let enc = !(frame.arg(0).is_number() && frame.arg(0).to_number() == 0.0);
    let (Some(key), Some(nonce), Some(aad), Some(data), Some(tag)) = (
        view_bytes(&mut cx, frame.arg(1), "ccm key"),
        view_bytes(&mut cx, frame.arg(2), "ccm nonce"),
        opt_view(&mut cx, frame.arg(3), "ccm aad"),
        view_bytes(&mut cx, frame.arg(4), "ccm data"),
        opt_view(&mut cx, frame.arg(5), "ccm tag"),
    ) else {
        return false;
    };
    let tag_len = if frame.arg(6).is_number() {
        frame.arg(6).to_number() as usize
    } else {
        report_error(&mut cx, "ERR_CRYPTO_INVALID_AUTH_TAG: Invalid authentication tag length");
        return false;
    };
    if enc {
        match ccm_crypt(&key, &nonce, aad.as_deref().unwrap_or(&[]), &data, tag_len, true) {
            Ok(out) => set_rval_bytes(&mut cx, &frame, &out),
            Err(e) => {
                report_error(&mut cx, &e);
                false
            }
        }
    } else {
        let Some(tag) = tag else {
            report_error(&mut cx, "Unsupported state or unable to authenticate data");
            return false;
        };
        if tag.len() != tag_len {
            report_error(
                &mut cx,
                &format!("ERR_CRYPTO_INVALID_AUTH_TAG: Invalid authentication tag length: {}", tag.len()),
            );
            return false;
        }
        let mut input = data;
        input.extend_from_slice(&tag);
        match ccm_crypt(&key, &nonce, aad.as_deref().unwrap_or(&[]), &input, tag_len, false) {
            Ok(out) => set_rval_bytes(&mut cx, &frame, &out),
            Err(e) => {
                report_error(&mut cx, &e);
                false
            }
        }
    }
}

// ── 10e GCM 任意 iv（NIST SP 800-38D J0 构造；`ghash` 0.6 直引）────────────
// 说明：12B 走 `aes-gcm` crate 原路径（`__wjs2_aesgcm_*`，WebCrypto 共用，spec 本就
// 只收 12B）；node 侧非 12B 走本节手工路径（`aes` ECB 单块 + `ghash`，约 60 行）。
// 计时侧信道与既有 PKCS#7 注记同口径（非恒定时间比较，功能等价）。

/// AES-ECB 单块加密（密钥三档分发）。
pub(crate) fn gcm_block_enc(key: &[u8], block: &[u8; 16]) -> Result<[u8; 16], String> {
    use aes::cipher::{BlockCipherEncrypt, KeyInit as _};
    macro_rules! one {
        ($aes:ty) => {{
            let c = <$aes>::new_from_slice(key).map_err(|e| format!("OperationError: {e}"))?;
            let mut b = aes::cipher::Block::<$aes>::default();
            b.copy_from_slice(block);
            c.encrypt_block(&mut b);
            let mut out = [0u8; 16];
            out.copy_from_slice(&b);
            out
        }};
    }
    match key.len() {
        16 => Ok(one!(aes::Aes128)),
        24 => Ok(one!(aes::Aes192)),
        32 => Ok(one!(aes::Aes256)),
        _ => Err("ERR_CRYPTO_INVALID_KEYLEN: Invalid key length".to_string()),
    }
}

/// GCM J0（SP 800-38D §7.1）：12B 直接后缀；其余 GHASH 全量构造。
pub(crate) fn gcm_j0(h: &[u8; 16], iv: &[u8]) -> Result<[u8; 16], String> {
    // `universal_hash` 经 `ghash` 重导出直用（零新增依赖，`rsa::BigUint` 同款口径）。
    use ghash::universal_hash::UniversalHash as _;
    if iv.len() == 12 {
        let mut j0 = [0u8; 16];
        j0[..12].copy_from_slice(iv);
        j0[15] = 1;
        return Ok(j0);
    }
    let key = ghash::Key::try_from(&h[..]).map_err(|_| "OperationError: bad GHASH key".to_string())?;
    let mut g = ghash::GHash::new(&key);
    g.update_padded(iv);
    // 64 零位 + 64 位大端 iv 位长
    let mut lens = [0u8; 16];
    let bits = (iv.len() as u64).wrapping_mul(8);
    lens[8..].copy_from_slice(&bits.to_be_bytes());
    g.update(&[lens.into()]);
    Ok(g.finalize().into())
}

/// inc32（大端低 32 位递增，回绕；SP 800-38D §7.2）。
pub(crate) fn gcm_inc32(y: &[u8; 16], n: u32) -> [u8; 16] {
    let mut out = *y;
    let ctr = u32::from_be_bytes([y[12], y[13], y[14], y[15]]).wrapping_add(n);
    out[12..].copy_from_slice(&ctr.to_be_bytes());
    out
}

/// 手工 GCM（非 12B iv 专用）：enc=true 输入 pt 输出 ct‖tag16；
/// enc=false 输入 ct‖tag16 输出 pt（tag 错报原文无码错，Node 同款）。
pub(crate) fn gcm_manual(key: &[u8], iv: &[u8], aad: &[u8], input: &[u8], enc: bool) -> Result<Vec<u8>, String> {
    use ghash::universal_hash::UniversalHash as _;
    if iv.is_empty() {
        return Err("ERR_CRYPTO_INVALID_IV: Invalid initialization vector".to_string());
    }
    let zero = [0u8; 16];
    let h = gcm_block_enc(key, &zero)?;
    let j0 = gcm_j0(&h, iv)?;
    let (ct_in, tag_in) = if enc {
        (input, None)
    } else {
        if input.len() < 16 {
            return Err("Unsupported state or unable to authenticate data".to_string());
        }
        let (c, t) = input.split_at(input.len() - 16);
        (c, Some(t))
    };
    // CTR（首计数器 J0+1）
    let mut out = Vec::with_capacity(ct_in.len());
    for (i, chunk) in ct_in.chunks(16).enumerate() {
        let ks = gcm_block_enc(key, &gcm_inc32(&j0, (i as u32).wrapping_add(1)))?;
        for (j, b) in chunk.iter().enumerate() {
            out.push(b ^ ks[j]);
        }
    }
    // GHASH(AAD‖CT‖lens) XOR E(K,J0)——解密分支同样 over 密文（上轮曾误 over 明文）。
    let (gct, glen) = if enc { (&out[..], out.len()) } else { (ct_in, ct_in.len()) };
    let gkey = ghash::Key::try_from(&h[..]).map_err(|_| "OperationError: bad GHASH key".to_string())?;
    let mut g = ghash::GHash::new(&gkey);
    g.update_padded(aad);
    g.update_padded(gct);
    let mut lens = [0u8; 16];
    lens[..8].copy_from_slice(&((aad.len() as u64).wrapping_mul(8).to_be_bytes()));
    lens[8..].copy_from_slice(&((glen as u64).wrapping_mul(8).to_be_bytes()));
    g.update(&[lens.into()]);
    let s: [u8; 16] = g.finalize().into();
    let e0 = gcm_block_enc(key, &j0)?;
    let mut tag = [0u8; 16];
    for i in 0..16 {
        tag[i] = s[i] ^ e0[i];
    }
    if enc {
        out.extend_from_slice(&tag);
        Ok(out)
    } else {
        let want = tag_in.expect("checked");
        let mut diff = 0u8;
        for i in 0..16 {
            diff |= tag[i] ^ want[i];
        }
        if diff != 0 {
            return Err("Unsupported state or unable to authenticate data".to_string());
        }
        Ok(out)
    }
}

/// `__wjs2_gcm_anyiv(encNum, keyU8, ivU8, aadU8, dataU8)`：12B 走 `aes-gcm`
/// crate（与 `__wjs2_aesgcm_*` 同语义），其余走手工 J0 路径；输出形状与 CCM 对齐
/// （enc → ct‖tag16；dec 输入 ct‖tag16 → pt）。
pub unsafe extern "C" fn gcm_anyiv(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 5 {
        report_error(&mut cx, "TypeError: gcm needs mode, key, iv, aad and data");
        return false;
    }
    let enc = !(frame.arg(0).is_number() && frame.arg(0).to_number() == 0.0);
    let (Some(key), Some(iv), Some(aad), Some(data)) = (
        view_bytes(&mut cx, frame.arg(1), "gcm key"),
        view_bytes(&mut cx, frame.arg(2), "gcm iv"),
        opt_view(&mut cx, frame.arg(3), "gcm aad"),
        view_bytes(&mut cx, frame.arg(4), "gcm data"),
    ) else {
        return false;
    };
    if key.len() != 16 && key.len() != 24 && key.len() != 32 {
        report_error(&mut cx, "ERR_CRYPTO_INVALID_KEYLEN: Invalid key length");
        return false;
    }
    if iv.is_empty() {
        report_error(&mut cx, "ERR_CRYPTO_INVALID_IV: Invalid initialization vector");
        return false;
    }
    let aad_ref = aad.as_deref().unwrap_or(&[]);
    let out = if iv.len() == 12 {
        // 原 crate 路径（12B；错误文案与 `__wjs2_aesgcm_*` 对齐）
        if enc {
            crate::builtins::crypto::gcm_encrypt_raw(&key, &iv, aad_ref, &data)
        } else {
            crate::builtins::crypto::gcm_decrypt_raw(&key, &iv, aad_ref, &data)
        }
    } else {
        gcm_manual(&key, &iv, aad_ref, &data, enc)
    };
    match out {
        Ok(v) => set_rval_bytes(&mut cx, &frame, &v),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

// ── 9e-1c 非对称（RSA v1.5 加解密 + DH/Miller-Rabin；密钥派生/签名复用既有 natives）

/// OS 熵 RNG（`crypto.rs::SystemRng` 同款，`rsa::rand_core` 0.6 口径；本模块自含）。
pub(crate) struct OsRng;

impl rsa::rand_core::RngCore for OsRng {
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
        getrandom::fill(dest).map_err(|_| rsa::rand_core::Error::from(OS_RNG_ERR))
    }
}

impl rsa::rand_core::CryptoRng for OsRng {}

/// 非零常量（`crypto.rs::SystemRng` 同款）。
const OS_RNG_ERR: core::num::NonZeroU32 =
    match core::num::NonZeroU32::new(rsa::rand_core::Error::CUSTOM_START) {
        Some(n) => n,
        None => core::num::NonZeroU32::MIN,
    };
