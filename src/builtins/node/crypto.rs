//! `node:crypto` 9e-1a/9e-1b/9e-1c：Hash/Hmac/随机 + 对称密码 + 非对称。
//! 复用全局 `__wjs_*`（随机/UUID/AES-GCM；零新 `UNSAFE-BOUNDARY`）；HMAC 经通用构造
//! 自架流式 Hash natives（sha3 与 hmac 0.13 的 block-API 不兼容，见修法记）；
//! 新增仅增量 Hash/Cipher 注册表（`encoding.rs` `STREAM_DECODERS` 同款线程本地表）。
//! 偏差记档（9e-1a）：
//! - 摘要集合 = RustCrypto 已接线：sha1/sha256/sha384/sha512/md5/sha3-256/384/512/
//!   blake2b512/blake2s256 + `ripemd160`（9h-2，`ripemd` crate）+ XOF
//!   `shake128/256`（9h-2，`tiny-keccak`；缺省输出长 shake128→16/shake256→32
//!   + DEP0198 警告，非法即 `ERR_INVALID_ARG_VALUE`）；其余 XOF（cshake/
//!   turboshake/kangaroo）不支持 → `createHash` 报原文
//!   `Digest method not supported`（无 code，Node 同款）；`getHashes` 只列已支持。
//! - 别名：大小写不敏感、`-`/`_` 可选、`RSA-` 前缀可剥（如 `RSA-SHA256`，Node 同款）。
//! - `Hmac` 更新攒 JS 侧（`digest` 时 oneshot），二次 `digest` 回空（Node 同款）；
//!   `digest` 后 `update`/`copy` 报 `ERR_CRYPTO_HASH_FINALIZED`。
//! - 未知输出编码的 `digest(enc)` 回 Buffer（Node 同款宽容）；未知输入编码按 utf8。
//! - 异步形态（`randomBytes(cb)`/`randomInt(cb)`/`randomFill`）经 `queueMicrotask`
//!   派发同步底层（`node:fs` 同款口径）；`getMacs`/`createMac` 真 Node 26 运行时
//!   不存在（仅 `lib/crypto.js` 残留导出），不做；`setEngine/getFips` 等随 9e-1c。
//! 偏差记档（9i-4 ml-kem）：
//! - 密钥 DER 用 LAMPS 种子形 PKCS#8（`[0]` 64B 种子，真机 26.8.2 同款逐字节同构，
//!   双向交叉互解）；`generateKey`（单面）不收 ml-kem（本仓 generateKey 仅为
//!   secret 面，既有口径）；`encapsulate` 异步回调形不做（给了第二参即
//!   ERR_INVALID_ARG_TYPE，与真机该路径报错同码）；`generateKeyPair` 未知类型
//!   仍报既有 ERR_NOT_SUPPORTED（真机为 ERR_INVALID_ARG_VALUE，pre-existing）。
//! 偏差记档（9i-6 ml-dsa）：
//! - 种子形 PKCS#8（`[0]` 32B 种子，真机同款）；顶层 `sign/verify` 收 ml-dsa
//!   （hash 必须 null，非 null 即真机码 ERR_OSSL_INVALID_DIGEST）；`Sign`/`Verify`
//!   流式类不收 ml-dsa（真机同款走顶层）；crate 的 Signer 为**确定性**签名档
//!   （真机 hedged，双方互验不受影响，双向交叉已验）；X.509 验签收 ml-dsa 证书
//!   （签名 OID 与密钥 OID 同族，openssl 3.6 实签证书真机/本仓同验）。
//! 偏差记档（9e-1b）：
//! - 对称集合：aes-128/192/256-cbc/ctr/gcm + chacha20-poly1305 + des-ede3-cbc。
//!   GCM/ChaCha 系 AEAD 无流式（buffered，`final` 时 oneshot；http 体整收同款口径）。
//! - GCM iv 限 12 字节（`__wjs_aesgcm_*` 既有约束；Node 接受任意长度，记档）。
//! - PKCS#7 填充校验非恒定时间实现（功能等价，侧信道记档）；`bf-cbc` 等 OpenSSL
//!   遗留算法不做；`ccm/ocb/wrap` 系不做。

use std::cell::RefCell;
use std::collections::HashMap;

use mozjs::context::JSContext;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{report_error, value_to_string, view_bytes, wrap_cx, Frame};

/// 增量摘要态（RustCrypto `Digest` 全员 `Clone`，`copy()` 语义天然；
/// SHAKE 系 `tiny_keccak::Shake` 同样 `Clone`，输出长存在注册时）。
enum HashJob {
    Sha1(sha1::Sha1),
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
fn norm_hash(name: &str, xof_len: usize) -> Option<HashJob> {
    use sha2::Digest as _; // 全员 digest 0.11 系（sha1/sha2/md5/sha3/blake2/ripemd 同 trait，无需直引 digest，见 §0.5 零新增）
    let flat: String = name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| *c != '-' && *c != '_')
        .collect();
    let flat = flat.strip_prefix("rsa").unwrap_or(&flat);
    match flat {
        "sha1" => Some(HashJob::Sha1(sha1::Sha1::new())),
        "sha256" => Some(HashJob::Sha256(sha2::Sha256::new())),
        "sha384" => Some(HashJob::Sha384(sha2::Sha384::new())),
        "sha512" => Some(HashJob::Sha512(sha2::Sha512::new())),
        "md5" => Some(HashJob::Md5(md5::Md5::new())),
        "sha3256" => Some(HashJob::Sha3_256(sha3::Sha3_256::new())),
        "sha3384" => Some(HashJob::Sha3_384(sha3::Sha3_384::new())),
        "sha3512" => Some(HashJob::Sha3_512(sha3::Sha3_512::new())),
        "blake2b512" => Some(HashJob::Blake2b512(blake2::Blake2b512::new())),
        "blake2s256" => Some(HashJob::Blake2s256(blake2::Blake2s256::new())),
        "ripemd160" => Some(HashJob::Ripemd160(ripemd::Ripemd160::new())),
        "shake128" => Some(HashJob::Shake128(tiny_keccak::Shake::v128(), xof_len)),
        "shake256" => Some(HashJob::Shake256(tiny_keccak::Shake::v256(), xof_len)),
        _ => None,
    }
}

fn hash_alloc(job: HashJob) -> u64 {
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

fn set_rval_str(cx: &mut JSContext, frame: &Frame, s: &str) {
    rooted!(&in(cx) let mut v = UndefinedValue());
    {
        use mozjs::conversions::ToJSValConvertible as _;
        s.to_jsval(cx, v.handle_mut());
    }
    frame.set_rval(v.get());
}

/// Uint8Array 返回值（`node:fs` 同款小 helper，不跨模块引）。
fn set_rval_bytes(cx: &mut JSContext, frame: &Frame, out: &[u8]) -> bool {
    rooted!(&in(cx) let mut obj: *mut mozjs::jsapi::JSObject = std::ptr::null_mut());
    // SAFETY: realm 内创建；obj 为 rooted 出参；out 存活到调用返回
    let ok = unsafe {
        mozjs::typedarray::TypedArray::<mozjs::typedarray::Uint8, *mut mozjs::jsapi::JSObject>::create(
            cx,
            mozjs::typedarray::CreateWith::Slice(out),
            obj.handle_mut(),
        )
    };
    if ok.is_err() || obj.is_null() {
        report_error(cx, "RangeError: cannot allocate output");
        return false;
    }
    frame.set_rval(mozjs::jsval::ObjectValue(obj.get()));
    true
}

/// id 实参（native 数值统一走字符串，JS 侧 `Number()` 包装，见 §4.33）。
fn arg_id(frame: &Frame, i: u32, what: &str, cx: &mut JSContext) -> Option<u64> {
    if frame.argc() <= i {
        report_error(cx, &format!("TypeError: {what} needs a hash handle"));
        return None;
    }
    value_to_string(cx, frame.arg(i)).parse::<u64>().ok().or_else(|| {
        report_error(cx, &format!("TypeError: {what} needs a hash handle"));
        None
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
fn to_blocks<D: BlockSizeUser>(data: &[u8]) -> Vec<Block<D>> {
    data.chunks_exact(D::block_size())
        .filter_map(|c| Block::<D>::try_from(c).ok())
        .collect()
}

fn from_blocks<D: BlockSizeUser>(blocks: &[Block<D>]) -> Vec<u8> {
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
    CbcEnc { job: Box<dyn CbcEncJob>, pending: Vec<u8>, block: usize },
    CbcDec { job: Box<dyn CbcDecJob>, pending: Vec<u8>, block: usize, autopad: bool },
    Ctr { job: Box<dyn CtrJob> },
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
fn cipher_params(alg: &str) -> Option<(&'static str, usize, usize, usize)> {
    match alg.trim().to_ascii_lowercase().as_str() {
        "aes-128-cbc" => Some(("cbc-aes128", 16, 16, 16)),
        "aes-192-cbc" => Some(("cbc-aes192", 24, 16, 16)),
        "aes-256-cbc" => Some(("cbc-aes256", 32, 16, 16)),
        "aes-128-ctr" => Some(("ctr-aes128", 16, 16, 16)),
        "aes-192-ctr" => Some(("ctr-aes192", 24, 16, 16)),
        "aes-256-ctr" => Some(("ctr-aes256", 32, 16, 16)),
        "des-ede3-cbc" => Some(("cbc-des3", 24, 8, 8)),
        _ => None,
    }
}

fn pkcs7_pad(block: usize, mut data: Vec<u8>) -> Vec<u8> {
    let pad = block - (data.len() % block);
    data.extend(std::iter::repeat(pad as u8).take(pad));
    data
}

/// PKCS#7 校验剥离（非恒定时间实现，见头注记档）。
fn pkcs7_unpad(block: usize, data: &[u8]) -> Option<Vec<u8>> {
    let n = *data.last()? as usize;
    if n == 0 || n > block || n > data.len() {
        return None;
    }
    if !data[data.len() - n..].iter().all(|&b| b as usize == n) {
        return None;
    }
    Some(data[..data.len() - n].to_vec())
}

/// `__wjs_cipher_new(alg, keyU8, ivU8, encNum, autoPadNum)` → id 字符串。
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

/// `__wjs_cipher_update(idStr, bytesU8)` → Uint8Array。
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
            CipherJob::CbcEnc { job, pending, block } => {
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

/// `__wjs_cipher_final(idStr)` → Uint8Array（消费句柄）。
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
            CipherJob::CbcEnc { job, pending, block } => {
                let mut chunk = pkcs7_pad(*block, std::mem::take(pending));
                job.enc_blocks(&mut chunk);
                Ok(chunk)
            }
            CipherJob::CbcDec { job, pending, block, autopad } => {
                if pending.len() % *block != 0 || (*autopad && pending.is_empty()) {
                    return Err("ERR_OSSL_WRONG_FINAL_BLOCK_LENGTH: wrong final block length".into());
                }
                let mut chunk = std::mem::take(pending);
                job.dec_blocks(&mut chunk);
                if *autopad {
                    pkcs7_unpad(*block, &chunk).ok_or_else(|| {
                        "ERR_OSSL_WRONG_FINAL_BLOCK_LENGTH: wrong final block length".to_string()
                    })
                } else {
                    Ok(chunk)
                }
            }
            CipherJob::Ctr { .. } => Ok(Vec::new()),
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

/// nullable 视图实参（`aad`/`tag` 传 null 即缺省）。
fn opt_view(cx: &mut JSContext, v: JSVal, what: &str) -> Option<Option<Vec<u8>>> {
    if v.is_null_or_undefined() {
        return Some(None);
    }
    view_bytes(cx, v, what).map(Some)
}

/// `__wjs_cipher_chacha(encNum, keyU8, nonceU8, aadOrNull, dataU8, tagOrNull)`：
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

// ── 9e-1c 非对称（RSA v1.5 加解密 + DH/Miller-Rabin；密钥派生/签名复用既有 natives）

/// OS 熵 RNG（`crypto.rs::SystemRng` 同款，`rsa::rand_core` 0.6 口径；本模块自含）。
struct OsRng;

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

/// `__wjs_rsa_encrypt_v15(pubDerU8, dataU8)` → 密文（PKCS#1 v1.5）。
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

/// `__wjs_rsa_decrypt_v15(privDerU8, dataU8)` → 明文（PKCS#1 v1.5）。
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

// ── DH / 素性（`rsa::BigUint` 直用，`crypto.rs` 同款零新增口径）──────────────

/// 左补零到定长（DH 密钥/密钥交换的定长口径，Node 回环长度断言即此）。
fn pad_be(bytes: &[u8], len: usize) -> Vec<u8> {
    if bytes.len() >= len {
        return bytes[bytes.len() - len..].to_vec();
    }
    let mut out = vec![0u8; len - bytes.len()];
    out.extend_from_slice(bytes);
    out
}

fn dh_range(p: &rsa::BigUint, x: &rsa::BigUint) -> bool {
    let one = rsa::BigUint::from(1u32);
    x > &one && x < &(p - &one)
}

/// `__wjs_dh_genkey(primeU8, generatorNum, privLenNum)` → JSON `{priv,pub}`（b64）。
pub unsafe extern "C" fn dh_genkey(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: DH genkey needs prime, generator and length");
        return false;
    }
    let prime = match view_bytes(&mut cx, frame.arg(0), "DH prime") {
        Some(b) => b,
        None => return false,
    };
    let generator = if frame.arg(1).is_number() { frame.arg(1).to_number() as u64 } else { 2 };
    let plen = if frame.arg(2).is_number() && frame.arg(2).to_number() > 0.0 {
        frame.arg(2).to_number() as usize
    } else {
        prime.len()
    };
    if prime.len() < 64 || generator < 2 {
        report_error(&mut cx, "OperationError: bad DH parameters");
        return false;
    }
    let p = rsa::BigUint::from_bytes_be(&prime);
    let g = rsa::BigUint::from(generator);
    // 私钥 ∈ [2, p-2]（高位掩码 + 重试，恒终止）
    let mut priv_b = vec![0u8; plen];
    let x = loop {
        if getrandom::fill(&mut priv_b).is_err() {
            report_error(&mut cx, "OperationError: cannot get random values");
            return false;
        }
        let x = rsa::BigUint::from_bytes_be(&priv_b);
        if dh_range(&p, &x) {
            break x;
        }
    };
    let y = g.modpow(&x, &p);
    use base64::Engine as _;
    let json = serde_json::json!({
        "priv": base64::engine::general_purpose::STANDARD.encode(pad_be(&x.to_bytes_be(), plen)),
        "pub": base64::engine::general_purpose::STANDARD.encode(pad_be(&y.to_bytes_be(), prime.len())),
    })
    .to_string();
    set_rval_str(&mut cx, &frame, &json);
    true
}

/// `__wjs_dh_secret(primeU8, privU8, pubU8)` → 定长密钥（prime 长左补零，Node 同款）。
pub unsafe extern "C" fn dh_secret(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: DH secret needs prime, private and public values");
        return false;
    }
    let (Some(prime), Some(priv_b), Some(pub_b)) = (
        view_bytes(&mut cx, frame.arg(0), "DH prime"),
        view_bytes(&mut cx, frame.arg(1), "DH private key"),
        view_bytes(&mut cx, frame.arg(2), "DH public key"),
    ) else {
        return false;
    };
    let p = rsa::BigUint::from_bytes_be(&prime);
    let x = rsa::BigUint::from_bytes_be(&priv_b);
    let y = rsa::BigUint::from_bytes_be(&pub_b);
    if !dh_range(&p, &y) {
        report_error(&mut cx, "OperationError: invalid DH public key");
        return false;
    }
    let s = y.modpow(&x, &p);
    set_rval_bytes(&mut cx, &frame, &pad_be(&s.to_bytes_be(), prime.len()))
}

/// Miller-Rabin（`checks` 轮随机基；小素数先试除。纯函数，单元测试覆盖）。
fn is_prime(n: &rsa::BigUint, checks: u32) -> bool {
    let zero = rsa::BigUint::from(0u32);
    let two = rsa::BigUint::from(2u32);
    if *n < two {
        return false;
    }
    for p in [2u32, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        let pp = rsa::BigUint::from(p);
        if *n == pp {
            return true;
        }
        if n % &pp == zero {
            return false;
        }
    }
    // n-1 = d·2^s（尾零字节数 + 末字节尾零位）
    let one = rsa::BigUint::from(1u32);
    let nm1 = n - &one;
    let nm1_bytes = nm1.to_bytes_be();
    let mut s = 0u32;
    for &b in nm1_bytes.iter().rev() {
        if b != 0 {
            s += b.trailing_zeros();
            break;
        }
        s += 8;
    }
    let mut d = nm1.clone();
    d >>= s as usize;
    let mut base = vec![0u8; n.to_bytes_be().len()];
    let two_b = rsa::BigUint::from(2u32);
    for _ in 0..checks.max(1) {
        if getrandom::fill(&mut base).is_err() {
            return false;
        }
        // 基 ∈ [2, n-2]
        let range = &nm1 - &two_b - &one;
        let a = rsa::BigUint::from_bytes_be(&base) % &range + &two_b;
        let mut x = a.modpow(&d, n);
        if x == one || x == nm1 {
            continue;
        }
        let mut composite = true;
        for _ in 1..s {
            x = x.modpow(&two_b, n);
            if x == nm1 {
                composite = false;
                break;
            }
        }
        if composite {
            return false;
        }
    }
    true
}

/// `__wjs_prime_check(bytesU8, checksNum)` → boolean。
pub unsafe extern "C" fn prime_check(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: prime check needs a candidate");
        return false;
    }
    let bytes = match view_bytes(&mut cx, frame.arg(0), "prime candidate") {
        Some(b) => b,
        None => return false,
    };
    let checks = if frame.argc() > 1 && frame.arg(1).is_number() {
        frame.arg(1).to_number() as u32
    } else {
        64
    };
    let n = rsa::BigUint::from_bytes_be(&bytes);
    frame.set_rval(mozjs::jsval::BooleanValue(is_prime(&n, checks)));
    true
}

/// `__wjs_prime_gen(bitsNum, checksNum, safeNum)` → 素数 Uint8Array（定长）。
pub unsafe extern "C" fn prime_gen(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 || !frame.arg(0).is_number() {
        report_error(&mut cx, "TypeError: prime generation needs a bit length");
        return false;
    }
    let bits = frame.arg(0).to_number() as usize;
    if bits < 32 || bits > 4096 {
        report_error(&mut cx, "ERR_OUT_OF_RANGE: prime bit length out of range (32..4096)");
        return false;
    }
    let checks = if frame.argc() > 1 && frame.arg(1).is_number() {
        frame.arg(1).to_number() as u32
    } else {
        64
    };
    let safe = frame.argc() > 2 && frame.arg(2).is_number() && frame.arg(2).to_number() != 0.0;
    let len = bits.div_ceil(8);
    let mut cand = vec![0u8; len];
    for _ in 0..(bits as u64 * 1000 + 1000) {
        if getrandom::fill(&mut cand).is_err() {
            report_error(&mut cx, "OperationError: cannot get random values");
            return false;
        }
        // 顶位置位（定长）+ 奇数
        cand[0] |= 0x80;
        let last = cand.len() - 1;
        cand[last] |= 1;
        let n = rsa::BigUint::from_bytes_be(&cand);
        if !is_prime(&n, checks) {
            continue;
        }
        if safe {
            let half = (&n - rsa::BigUint::from(1u32)) >> 1usize;
            if !is_prime(&half, checks) {
                continue;
            }
        }
        return set_rval_bytes(&mut cx, &frame, &pad_be(&n.to_bytes_be(), len));
    }
    report_error(&mut cx, "OperationError: prime generation failed to converge");
    false
}

// ── RSA-SHA1/MD5 手工件（rsa 0.9 的 OAEP/签名接口绑 digest 0.10，
// sha1/sha2_010 0.10 系无直引行（§0.5 未批），故 OAEP-SHA1 与 v1.5-SHA1/MD5
// 签名在此手写：MGF1-SHA1 + BigUint 模幂 + 定长编码，全经真 Node 交叉验证。
// OAEP-SHA256/384/512 与 v1.5-SHA256/384/512 继续走既有 natives。）─────────

/// MGF1-SHA1（OAEP 编解码用）。
fn mgf1_sha1(seed: &[u8], len: usize) -> Vec<u8> {
    use sha1::Digest as _;
    let mut out = Vec::with_capacity(len);
    let mut ctr = 0u32;
    while out.len() < len {
        let mut h = sha1::Sha1::new();
        h.update(seed);
        h.update(ctr.to_be_bytes());
        out.extend_from_slice(&h.finalize());
        ctr += 1;
    }
    out.truncate(len);
    out
}

fn sha1_bytes(data: &[u8]) -> Vec<u8> {
    use sha1::Digest as _;
    sha1::Sha1::digest(data).to_vec()
}

/// `__wjs_node_rsa_oaep(pubDerU8, dataU8, labelOrNull, encNum)`：
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
            report_error(&mut cx, "OperationError: RSA encrypt failed: message too long");
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

/// v1.5 签名 DigestInfo 前缀（SHA-1/MD5；SHA-2 系走既有 natives）。
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

/// `__wjs_node_rsa_v15_legacy(privDerU8, dataU8, hashStr)` → 签名（SHA-1/MD5）。
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

/// `__wjs_node_rsa_v15_verify(pubDerU8, sigU8, dataU8, hashStr)` → boolean。
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

// ── 9e-1d KDF（pbkdf2/scrypt/hkdf/argon2；全员树内轮子，零新增）──────────────

/// KDF 摘要分发（HMAC 系：sha1/sha256/sha384/sha512/md5；sha3 无 block-API，记档）。
macro_rules! kdf_hash_dispatch {
    ($hash:expr, $D:ident, $body:expr) => {{
        match $hash {
            "SHA-1" => {
                type $D = sha1::Sha1;
                $body
            }
            "SHA-256" => {
                type $D = sha2::Sha256;
                $body
            }
            "SHA-384" => {
                type $D = sha2::Sha384;
                $body
            }
            "SHA-512" => {
                type $D = sha2::Sha512;
                $body
            }
            "MD5" => {
                type $D = md5::Md5;
                $body
            }
            other => Err(format!("NotSupportedError: KDF hash '{other}' needs SHA-1/256/384/512/MD5")),
        }
    }};
}

/// `__wjs_kdf_pbkdf2(hashStr, passU8, saltU8, roundsNum, lenNum)` → 派生密钥。
pub unsafe extern "C" fn kdf_pbkdf2(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 5 {
        report_error(&mut cx, "TypeError: PBKDF2 needs hash, password, salt, rounds and length");
        return false;
    }
    let hash = value_to_string(&mut cx, frame.arg(0));
    let (Some(pass), Some(salt)) = (
        view_bytes(&mut cx, frame.arg(1), "PBKDF2 password"),
        view_bytes(&mut cx, frame.arg(2), "PBKDF2 salt"),
    ) else {
        return false;
    };
    if !frame.arg(3).is_number() || !frame.arg(4).is_number() {
        report_error(&mut cx, "TypeError: PBKDF2 rounds/length must be numbers");
        return false;
    }
    let rounds = frame.arg(3).to_number();
    let len = frame.arg(4).to_number();
    if !(1.0..=2147483647.0).contains(&rounds) {
        report_error(&mut cx, "ERR_OUT_OF_RANGE: PBKDF2 iterations out of range");
        return false;
    }
    if !(0.0..=1073741824.0).contains(&len) {
        report_error(&mut cx, "ERR_OUT_OF_RANGE: PBKDF2 key length out of range");
        return false;
    }
    let mut out = vec![0u8; len as usize];
    let r: Result<(), String> = kdf_hash_dispatch!(hash.as_str(), D, {
        pbkdf2::pbkdf2_hmac::<D>(&pass, &salt, rounds as u32, &mut out);
        Ok(())
    });
    match r {
        Ok(()) => set_rval_bytes(&mut cx, &frame, &out),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_kdf_scrypt(passU8, saltU8, nNum, rNum, pNum, lenNum, maxmemNum)` → 派生密钥。
pub unsafe extern "C" fn kdf_scrypt(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 7 {
        report_error(&mut cx, "TypeError: scrypt needs password, salt, N, r, p, length and maxmem");
        return false;
    }
    let (Some(pass), Some(salt)) = (
        view_bytes(&mut cx, frame.arg(0), "scrypt password"),
        view_bytes(&mut cx, frame.arg(1), "scrypt salt"),
    ) else {
        return false;
    };
    let nums: Option<Vec<f64>> = (2..7)
        .map(|i| {
            let v = frame.arg(i);
            if v.is_number() { Some(v.to_number()) } else { None }
        })
        .collect();
    let Some(nums) = nums else {
        report_error(&mut cx, "TypeError: scrypt N/r/p/length/maxmem must be numbers");
        return false;
    };
    let (n_f, r_f, p_f, len_f, maxmem_f) = (nums[0], nums[1], nums[2], nums[3], nums[4]);
    if n_f <= 1.0 || n_f.fract() != 0.0 || (n_f.log2().fract() != 0.0 && n_f > 1.0) {
        report_error(&mut cx, "ERR_CRYPTO_INVALID_SCRYPT_PARAMS: Invalid scrypt params");
        return false;
    }
    let (n, r, p) = (n_f as u64, r_f as u32, p_f as u32);
    // 内存上限（默认 32MiB，Node 同款；128·N·r·p 口径）
    let mem = (n as u128) * (r as u128) * (p as u128) * 128;
    if r == 0 || p == 0 || mem > maxmem_f as u128 {
        report_error(&mut cx, "ERR_CRYPTO_INVALID_SCRYPT_PARAMS: Invalid scrypt params");
        return false;
    }
    if !(1.0..=1073741824.0).contains(&len_f) {
        report_error(&mut cx, "ERR_OUT_OF_RANGE: scrypt key length out of range");
        return false;
    }
    let log_n = n_f.log2() as u8;
    let params = match scrypt::Params::new(log_n, r, p) {
        Ok(p) => p,
        Err(_) => {
            report_error(&mut cx, "ERR_CRYPTO_INVALID_SCRYPT_PARAMS: Invalid scrypt params");
            return false;
        }
    };
    let mut out = vec![0u8; len_f as usize];
    match scrypt::scrypt(&pass, &salt, &params, &mut out) {
        Ok(()) => set_rval_bytes(&mut cx, &frame, &out),
        Err(_) => {
            report_error(&mut cx, "ERR_CRYPTO_INVALID_SCRYPT_PARAMS: Invalid scrypt params");
            false
        }
    }
}

/// `__wjs_kdf_hkdf(hashStr, ikmU8, saltU8, infoU8, lenNum)` → OKM（空 salt 即零串，RFC 口径）。
pub unsafe extern "C" fn kdf_hkdf(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 5 {
        report_error(&mut cx, "TypeError: HKDF needs hash, ikm, salt, info and length");
        return false;
    }
    let hash = value_to_string(&mut cx, frame.arg(0));
    let (Some(ikm), Some(salt), Some(info)) = (
        view_bytes(&mut cx, frame.arg(1), "HKDF ikm"),
        view_bytes(&mut cx, frame.arg(2), "HKDF salt"),
        view_bytes(&mut cx, frame.arg(3), "HKDF info"),
    ) else {
        return false;
    };
    if !frame.arg(4).is_number() {
        report_error(&mut cx, "TypeError: HKDF length must be a number");
        return false;
    }
    let len = frame.arg(4).to_number();
    if !(0.0..=1073741824.0).contains(&len) {
        report_error(&mut cx, "ERR_OUT_OF_RANGE: HKDF length out of range");
        return false;
    }
    let mut out = vec![0u8; len as usize];
    let r: Result<(), String> = kdf_hash_dispatch!(hash.as_str(), D, {
        let hk = hkdf::Hkdf::<D>::new(if salt.is_empty() { None } else { Some(&salt) }, &ikm);
        match hk.expand(&info, &mut out) {
            Ok(()) => Ok(()),
            Err(e) => Err(format!("OperationError: HKDF expand failed: {e}")),
        }
    });
    match r {
        Ok(()) => set_rval_bytes(&mut cx, &frame, &out),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_kdf_argon2(algoStr, msgU8, nonceU8, secretU8, adU8, parNum, tagNum, memNum, passNum)` → tag。
pub unsafe extern "C" fn kdf_argon2(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 9 {
        report_error(&mut cx, "TypeError: Argon2 needs algorithm, message, nonce, secret, ad and params");
        return false;
    }
    let algo = value_to_string(&mut cx, frame.arg(0));
    let (Some(msg), Some(nonce), Some(secret), Some(ad)) = (
        view_bytes(&mut cx, frame.arg(1), "Argon2 message"),
        view_bytes(&mut cx, frame.arg(2), "Argon2 nonce"),
        view_bytes(&mut cx, frame.arg(3), "Argon2 secret"),
        view_bytes(&mut cx, frame.arg(4), "Argon2 associated data"),
    ) else {
        return false;
    };
    let nums: Option<Vec<f64>> = (5..9)
        .map(|i| {
            let v = frame.arg(i);
            if v.is_number() { Some(v.to_number()) } else { None }
        })
        .collect();
    let Some(nums) = nums else {
        report_error(&mut cx, "TypeError: Argon2 params must be numbers");
        return false;
    };
    let algorithm = match algo.as_str() {
        "argon2d" => argon2::Algorithm::Argon2d,
        "argon2i" => argon2::Algorithm::Argon2i,
        "argon2id" => argon2::Algorithm::Argon2id,
        _ => {
            report_error(&mut cx, "ERR_INVALID_ARG_VALUE: Argon2 algorithm must be argon2d/argon2i/argon2id");
            return false;
        }
    };
    let mut builder = argon2::ParamsBuilder::new();
    if !ad.is_empty() {
        match argon2::AssociatedData::new(&ad) {
            Ok(data) => {
                builder.data(data);
            }
            Err(e) => {
                report_error(&mut cx, &format!("ERR_OUT_OF_RANGE: bad Argon2 associatedData ({e})"));
                return false;
            }
        }
    }
    builder.m_cost(nums[2] as u32);
    builder.t_cost(nums[3] as u32);
    builder.p_cost(nums[0] as u32);
    builder.output_len(nums[1] as usize);
    let params = match builder.build() {
        Ok(p) => p,
        Err(e) => {
            report_error(&mut cx, &format!("ERR_OUT_OF_RANGE: bad Argon2 params ({e})"));
            return false;
        }
    };
    let ctx = if secret.is_empty() {
        argon2::Argon2::new(algorithm, argon2::Version::V0x13, params)
    } else {
        match argon2::Argon2::new_with_secret(&secret, algorithm, argon2::Version::V0x13, params) {
            Ok(c) => c,
            Err(e) => {
                report_error(&mut cx, &format!("ERR_OUT_OF_RANGE: bad Argon2 secret ({e})"));
                return false;
            }
        }
    };
    let mut out = vec![0u8; nums[1] as usize];
    match ctx.hash_password_into(&msg, &nonce, &mut out) {
        Ok(()) => set_rval_bytes(&mut cx, &frame, &out),
        Err(e) => {
            report_error(&mut cx, &format!("OperationError: Argon2 failed ({e})"));
            false
        }
    }
}

// ── 9i-4 ml-kem（FIPS 203；`ml-kem` crate 直用，零版本墙）──────────────────
// 真机口径（node 26.8.2 实测）：SPKI 头定长 22B（ek 裸字节 = SPKI[22..]，三档同构）；
// PKCS#8 = SEQ{INT 0, SEQ{OID}, OCTET{ [0](0x80) 64B 种子 }}（LAMPS 种子形，总长 86）；
// JWK kty "AKP"（pub=ek/priv=种子，b64url）；encapsulate(pub|priv) 均收；
// decapsulate 长度不对 → ERR_CRYPTO_OPERATION_FAILED（FIPS 203 隐式拒绝：等长坏文
// 不报错回伪随机密钥）；decapsulate 非 ml-kem 私钥 → 无码错；异步封装形不做。

/// 参数集 → `(OID DER 内容, ek/ct 裸字节数)`。
fn mlkem_params(kind: &str) -> Option<(&'static [u8], usize, usize)> {
    match kind {
        "ml-kem-512" => Some((&[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x04, 0x01], 800, 768)),
        "ml-kem-768" => Some((&[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x04, 0x02], 1184, 1088)),
        "ml-kem-1024" => Some((&[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x04, 0x03], 1568, 1568)),
        _ => None,
    }
}

fn mlkem_kind_by_oid(oid: &[u8]) -> Option<&'static str> {
    match oid {
        [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x04, 0x01] => Some("ml-kem-512"),
        [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x04, 0x02] => Some("ml-kem-768"),
        [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x04, 0x03] => Some("ml-kem-1024"),
        _ => None,
    }
}

/// 小型 DER TLV 拼装（长度 <65536，键封装足够）。
fn mlkem_tlv(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    if body.len() < 128 {
        out.push(body.len() as u8);
    } else if body.len() < 256 {
        out.extend_from_slice(&[0x81, body.len() as u8]);
    } else {
        out.extend_from_slice(&[0x82, (body.len() >> 8) as u8, (body.len() & 0xff) as u8]);
    }
    out.extend_from_slice(body);
    out
}

/// PKCS#8 私钥（种子形）：`SEQ{INT 0, SEQ{OID}, OCTET{[0] 种子64}}`。
fn mlkem_pkcs8(oid: &[u8], seed: &[u8]) -> Vec<u8> {
    let mut body = mlkem_tlv(0x02, &[0]);
    body.extend_from_slice(&mlkem_tlv(0x30, &mlkem_tlv(0x06, oid)));
    body.extend_from_slice(&mlkem_tlv(0x04, &mlkem_tlv(0x80, seed)));
    mlkem_tlv(0x30, &body)
}

/// SPKI 公钥：`SEQ{ SEQ{OID}, BITSTRING(00‖ek) }`。
fn mlkem_spki(oid: &[u8], ek: &[u8]) -> Vec<u8> {
    let mut bit = vec![0u8];
    bit.extend_from_slice(ek);
    let mut body = mlkem_tlv(0x30, &mlkem_tlv(0x06, oid));
    body.extend_from_slice(&mlkem_tlv(0x03, &bit));
    mlkem_tlv(0x30, &body)
}

/// PKCS#8 → `(OID, 种子)`（结构不合规即 None；种子长按参数集，ml-kem 64 / ml-dsa 32）。
fn mlkem_pkcs8_seed(der: &[u8], seed_len: usize) -> Option<(&[u8], &[u8])> {
    let (t, hl, cl) = crate::builtins::crypto::der_tlv(der)?;
    if t != 0x30 {
        return None;
    }
    let body = &der[hl..hl + cl];
    let (t1, h1, c1) = crate::builtins::crypto::der_tlv(body)?;
    if t1 != 0x02 || body[h1..h1 + c1] != [0] {
        return None;
    }
    let rest = &body[h1 + c1..];
    let (t2, h2, c2) = crate::builtins::crypto::der_tlv(rest)?;
    if t2 != 0x30 {
        return None;
    }
    let alg = &rest[h2..h2 + c2];
    let (t3, h3, c3) = crate::builtins::crypto::der_tlv(alg)?;
    if t3 != 0x06 {
        return None;
    }
    let oid = &alg[h3..h3 + c3];
    let rest2 = &rest[h2 + c2..];
    let (t4, h4, c4) = crate::builtins::crypto::der_tlv(rest2)?;
    if t4 != 0x04 {
        return None;
    }
    let inner = &rest2[h4..h4 + c4];
    let (t5, h5, c5) = crate::builtins::crypto::der_tlv(inner)?;
    if t5 != 0x80 || c5 != seed_len {
        return None;
    }
    Some((oid, &inner[h5..h5 + c5]))
}

/// SPKI → `(OID, ek)`（BIT STRING 首字节须为 0 未用位）。
fn mlkem_spki_ek(der: &[u8]) -> Option<(&[u8], &[u8])> {
    let (t, hl, cl) = crate::builtins::crypto::der_tlv(der)?;
    if t != 0x30 {
        return None;
    }
    let body = &der[hl..hl + cl];
    let (t1, h1, c1) = crate::builtins::crypto::der_tlv(body)?;
    if t1 != 0x30 {
        return None;
    }
    let alg = &body[h1..h1 + c1];
    let (t2, h2, c2) = crate::builtins::crypto::der_tlv(alg)?;
    if t2 != 0x06 {
        return None;
    }
    let oid = &alg[h2..h2 + c2];
    let rest = &body[h1 + c1..];
    let (t3, h3, c3) = crate::builtins::crypto::der_tlv(rest)?;
    if t3 != 0x03 || c3 < 2 || rest[h3] != 0 {
        return None;
    }
    Some((oid, &rest[h3 + 1..h3 + c3]))
}

/// 64B 种子 → `(PKCS#8, SPKI)`（展开即校验，坏种子报错）。
fn mlkem_expand(kind: &str, seed: &[u8; 64]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let (oid, ek_len, _) = mlkem_params(kind)
        .ok_or_else(|| "NotSupportedError: unsupported ml-kem parameter set".to_string())?;
    macro_rules! case {
        ($P:ty, $name:expr) => {
            if kind == $name {
                use ml_kem::KeyExport as _;
                let dk = ml_kem::DecapsulationKey::<$P>::from_seed((*seed).into());
                let ek = dk.encapsulation_key().to_bytes();
                if ek.as_slice().len() != ek_len {
                    return Err("OperationError: ml-kem ek length mismatch".into());
                }
                return Ok((mlkem_pkcs8(oid, seed), mlkem_spki(oid, ek.as_slice())));
            }
        };
    }
    case!(ml_kem::MlKem512, "ml-kem-512");
    case!(ml_kem::MlKem768, "ml-kem-768");
    case!(ml_kem::MlKem1024, "ml-kem-1024");
    Err("NotSupportedError: unsupported ml-kem parameter set".into())
}

/// `__wjs_mlkem_gen(kind)` → JSON `{pkcs8, spki}`（b64；种子 getrandom 自造）。
pub unsafe extern "C" fn mlkem_gen(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let kind = if frame.argc() > 0 { value_to_string(&mut cx, frame.arg(0)) } else { String::new() };
    if mlkem_params(&kind).is_none() {
        report_error(&mut cx, "NotSupportedError: unsupported ml-kem parameter set");
        return false;
    }
    let mut seed = [0u8; 64];
    if getrandom::fill(&mut seed).is_err() {
        report_error(&mut cx, "OperationError: cannot get random values");
        return false;
    }
    match mlkem_expand(&kind, &seed) {
        Ok((pkcs8, spki)) => {
            use base64::Engine as _;
            let json = serde_json::json!({
                "pkcs8": base64::engine::general_purpose::STANDARD.encode(pkcs8),
                "spki": base64::engine::general_purpose::STANDARD.encode(spki),
            })
            .to_string();
            set_rval_str(&mut cx, &frame, &json);
            true
        }
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_mlkem_seed_from_pkcs8(der)` → JSON `{kind, spki}`（b64；导入即展开校验）。
pub unsafe extern "C" fn mlkem_seed_from_pkcs8(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(der) = (if frame.argc() > 0 { view_bytes(&mut cx, frame.arg(0), "ml-kem key") } else { None }) else {
        return false;
    };
    let parsed = match mlkem_pkcs8_seed(&der, 64) {
        Some(p) => p,
        None => {
            report_error(&mut cx, "TypeError: Invalid PKCS#8 key");
            return false;
        }
    };
    let Some(kind) = mlkem_kind_by_oid(parsed.0) else {
        report_error(&mut cx, "TypeError: Invalid PKCS#8 key");
        return false;
    };
    let mut seed = [0u8; 64];
    seed.copy_from_slice(parsed.1);
    match mlkem_expand(kind, &seed) {
        Ok((_, spki)) => {
            use base64::Engine as _;
            let json = serde_json::json!({
                "kind": kind,
                "seed": base64::engine::general_purpose::STANDARD.encode(seed),
                "spki": base64::engine::general_purpose::STANDARD.encode(spki),
            })
            .to_string();
            set_rval_str(&mut cx, &frame, &json);
            true
        }
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_mlkem_kind_from_spki(der)` → kind 串（OID + ek 长度校验，失败回空串）。
pub unsafe extern "C" fn mlkem_kind_from_spki(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(der) = (if frame.argc() > 0 { view_bytes(&mut cx, frame.arg(0), "ml-kem key") } else { None }) else {
        return false;
    };
    let kind = mlkem_spki_ek(&der)
        .and_then(|(oid, ek)| {
            mlkem_kind_by_oid(oid).filter(|k| mlkem_params(k).is_some_and(|(_, ek_len, _)| ek_len == ek.len()))
        })
        .unwrap_or("");
    use mozjs::conversions::ToJSValConvertible as _;
    kind.to_jsval(&mut cx, frame.rval_mut());
    true
}

/// 三参数集分发：封装（ek 裸字节 → 借用 `&Key` 构造 → `encapsulate_with_rng`）。
macro_rules! mlkem_encaps_case {
    ($kind:expr, $ek:expr, $P:ty, $name:expr) => {
        if $kind == $name {
            Some((|| -> Result<(Vec<u8>, Vec<u8>), String> {
                let arr = <&ml_kem::Key<ml_kem::EncapsulationKey<$P>>>::try_from($ek)
                    .map_err(|_| "ERR_CRYPTO_OPERATION_FAILED: Encapsulation failed".to_string())?;
                let key = ml_kem::EncapsulationKey::<$P>::new(arr)
                    .map_err(|_| "ERR_CRYPTO_OPERATION_FAILED: Encapsulation failed".to_string())?;
                use ml_kem::Encapsulate as _;
                let (ct, sk) = key.encapsulate_with_rng(&mut rand::rng());
                Ok((ct.as_slice().to_vec(), sk.as_slice().to_vec()))
            })())
        } else {
            None
        }
    };
}

/// 三参数集分发：解封装（种子 → `KeyInit::new` → `decapsulate_slice`，长度内建校验）。
macro_rules! mlkem_decaps_case {
    ($kind:expr, $seed:expr, $ct:expr, $P:ty, $name:expr) => {
        if $kind == $name {
            Some((|| -> Result<Vec<u8>, String> {
                let arr = <&ml_kem::Key<ml_kem::DecapsulationKey<$P>>>::try_from($seed)
                    .map_err(|_| "ERR_CRYPTO_OPERATION_FAILED: Decapsulation failed".to_string())?;
                use ml_kem::KeyInit as _;
                let dk = ml_kem::DecapsulationKey::<$P>::new(arr);
                use ml_kem::Decapsulate as _;
                dk.decapsulate_slice($ct)
                    .map(|sk| sk.as_slice().to_vec())
                    .map_err(|_| "ERR_CRYPTO_OPERATION_FAILED: Decapsulation failed".to_string())
            })())
        } else {
            None
        }
    };
}

/// `__wjs_mlkem_encaps(keyDer, isPriv)` → JSON `{ct, sk}`（b64）。
/// keyDer：isPriv=0 为 SPKI（ek 直用），=1 为 PKCS#8（种子展开）。
pub unsafe extern "C" fn mlkem_encaps(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "ERR_CRYPTO_OPERATION_FAILED: Encapsulation failed");
        return false;
    }
    let Some(der) = view_bytes(&mut cx, frame.arg(0), "ml-kem key") else {
        return false;
    };
    let is_priv = frame.arg(1).to_number() != 0.0;
    let parsed = if is_priv {
        mlkem_pkcs8_seed(&der, 64).and_then(|(oid, seed)| mlkem_kind_by_oid(oid).map(|k| (k, seed.to_vec(), Vec::new())))
    } else {
        mlkem_spki_ek(&der).and_then(|(oid, ek)| mlkem_kind_by_oid(oid).map(|k| (k, Vec::new(), ek.to_vec())))
    };
    let Some((kind, seed, ek)) = parsed else {
        report_error(&mut cx, "ERR_CRYPTO_OPERATION_FAILED: Encapsulation failed");
        return false;
    };
    let Some((_, ek_len, _)) = mlkem_params(kind) else {
        report_error(&mut cx, "ERR_CRYPTO_OPERATION_FAILED: Encapsulation failed");
        return false;
    };
    // 私钥入参：先展开种子得 ek（一并完成校验）。
    let ek_bytes: Vec<u8> = if is_priv {
        let mut s = [0u8; 64];
        s.copy_from_slice(&seed);
        match mlkem_expand(kind, &s) {
            Ok((_, spki)) => match mlkem_spki_ek(&spki) {
                Some((_, e)) => e.to_vec(),
                None => {
                    report_error(&mut cx, "ERR_CRYPTO_OPERATION_FAILED: Encapsulation failed");
                    return false;
                }
            },
            Err(e) => {
                report_error(&mut cx, &e);
                return false;
            }
        }
    } else {
        ek
    };
    if ek_bytes.len() != ek_len {
        report_error(&mut cx, "ERR_CRYPTO_OPERATION_FAILED: Encapsulation failed");
        return false;
    }
    let enc: Result<(Vec<u8>, Vec<u8>), String> =
        mlkem_encaps_case!(kind, ek_bytes.as_slice(), ml_kem::MlKem512, "ml-kem-512")
            .or_else(|| mlkem_encaps_case!(kind, ek_bytes.as_slice(), ml_kem::MlKem768, "ml-kem-768"))
            .or_else(|| mlkem_encaps_case!(kind, ek_bytes.as_slice(), ml_kem::MlKem1024, "ml-kem-1024"))
            .unwrap_or_else(|| Err("ERR_CRYPTO_OPERATION_FAILED: Encapsulation failed".into()));
    match enc {
        Ok((ct, sk)) => {
            use base64::Engine as _;
            let json = serde_json::json!({
                "ct": base64::engine::general_purpose::STANDARD.encode(ct),
                "sk": base64::engine::general_purpose::STANDARD.encode(sk),
            })
            .to_string();
            set_rval_str(&mut cx, &frame, &json);
            true
        }
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_mlkem_decaps(pkcs8Der, ct)` → 32B 共享密钥；长度不对报
/// `ERR_CRYPTO_OPERATION_FAILED`（等长坏文按 FIPS 203 走隐式拒绝，不报错）。
pub unsafe extern "C" fn mlkem_decaps(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "ERR_CRYPTO_OPERATION_FAILED: Decapsulation failed");
        return false;
    }
    let (Some(der), Some(ct)) = (
        view_bytes(&mut cx, frame.arg(0), "ml-kem key"),
        view_bytes(&mut cx, frame.arg(1), "ciphertext"),
    ) else {
        return false;
    };
    let out: Result<Vec<u8>, String> = (|| {
        let Some((oid, seed)) = mlkem_pkcs8_seed(&der, 64)
            .and_then(|(o, s)| mlkem_kind_by_oid(o).map(|k| (k, s)))
        else {
            return Err("ERR_CRYPTO_OPERATION_FAILED: Decapsulation failed".into());
        };
        let Some((_, _, ct_len)) = mlkem_params(oid) else {
            return Err("ERR_CRYPTO_OPERATION_FAILED: Decapsulation failed".into());
        };
        if ct.len() != ct_len {
            return Err("ERR_CRYPTO_OPERATION_FAILED: Decapsulation failed".into());
        }
        mlkem_decaps_case!(oid, seed, ct.as_slice(), ml_kem::MlKem512, "ml-kem-512")
            .or_else(|| mlkem_decaps_case!(oid, seed, ct.as_slice(), ml_kem::MlKem768, "ml-kem-768"))
            .or_else(|| mlkem_decaps_case!(oid, seed, ct.as_slice(), ml_kem::MlKem1024, "ml-kem-1024"))
            .unwrap_or_else(|| Err("ERR_CRYPTO_OPERATION_FAILED: Decapsulation failed".into()))
    })();
    match out {
        Ok(sk) => set_rval_bytes(&mut cx, &frame, &sk),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

// ── 9i-6 ml-dsa（FIPS 204；`ml-dsa` crate，纯签名 hash=null）───────────────
// 真机口径（node 26.8.2 实测）：SPKI = 22B 头 + 裸 pk（1312/1952/2592）；
// PKCS#8 = SEQ{INT 0, SEQ{OID}, OCTET{[0] 32B 种子}}（总长 54，OID …3.4.3.17/18/19）；
// `crypto.sign(null, data, key)` 纯签名（非 null 即 ERR_OSSL_INVALID_DIGEST）；
// 签名长 2420/3309/4627；JWK kty "AKP"（priv=32B 种子）。本仓 Signer 走 crate 的
// 确定性档（真机为 hedged，双方互验不受影响）。

fn mldsa_params(kind: &str) -> Option<(&'static [u8], usize, usize)> {
    match kind {
        "ml-dsa-44" => Some((&[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x03, 0x11], 1312, 2420)),
        "ml-dsa-65" => Some((&[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x03, 0x12], 1952, 3309)),
        "ml-dsa-87" => Some((&[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x03, 0x13], 2592, 4627)),
        _ => None,
    }
}

fn mldsa_kind_by_oid(oid: &[u8]) -> Option<&'static str> {
    match oid {
        [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x03, 0x11] => Some("ml-dsa-44"),
        [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x03, 0x12] => Some("ml-dsa-65"),
        [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x03, 0x13] => Some("ml-dsa-87"),
        _ => None,
    }
}

/// dotted 串 OID → kind（X.509 证书签名算法用）。
pub(crate) fn mldsa_kind_by_oid_str(oid: &str) -> Option<&'static str> {
    match oid {
        "2.16.840.1.101.3.4.3.17" => Some("ml-dsa-44"),
        "2.16.840.1.101.3.4.3.18" => Some("ml-dsa-65"),
        "2.16.840.1.101.3.4.3.19" => Some("ml-dsa-87"),
        _ => None,
    }
}

/// SPKI → `(kind, 裸 pk)`（X.509 验签用；OID/pk 长度不符即 None）。
pub(crate) fn mldsa_spki_pk(spki: &[u8]) -> Option<(&'static str, Vec<u8>)> {
    let (oid, pk) = mlkem_spki_ek(spki)?;
    let kind = mldsa_kind_by_oid(oid)?;
    mldsa_params(kind).is_some_and(|(_, pk_len, _)| pk_len == pk.len()).then(|| (kind, pk.to_vec()))
}

/// 32B 种子 → `(PKCS#8, SPKI)`（展开即校验）。
fn mldsa_expand(kind: &str, seed: &[u8; 32]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let (oid, pk_len, _) = mldsa_params(kind)
        .ok_or_else(|| "NotSupportedError: unsupported ml-dsa parameter set".to_string())?;
    macro_rules! case {
        ($P:ty, $name:expr) => {
            if kind == $name {
                use ml_dsa::{Keypair as _, KeyExport as _};
                let sk = ml_dsa::SigningKey::<$P>::from_seed(&(*seed).into());
                let pk = sk.verifying_key().to_bytes();
                if pk.as_slice().len() != pk_len {
                    return Err("OperationError: ml-dsa pk length mismatch".into());
                }
                return Ok((mlkem_pkcs8(oid, seed), mlkem_spki(oid, pk.as_slice())));
            }
        };
    }
    case!(ml_dsa::MlDsa44, "ml-dsa-44");
    case!(ml_dsa::MlDsa65, "ml-dsa-65");
    case!(ml_dsa::MlDsa87, "ml-dsa-87");
    Err("NotSupportedError: unsupported ml-dsa parameter set".into())
}

/// `__wjs_mldsa_gen(kind)` → JSON `{pkcs8, spki}`（b64；种子 getrandom 自造）。
pub unsafe extern "C" fn mldsa_gen(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let kind = if frame.argc() > 0 { value_to_string(&mut cx, frame.arg(0)) } else { String::new() };
    if mldsa_params(&kind).is_none() {
        report_error(&mut cx, "NotSupportedError: unsupported ml-dsa parameter set");
        return false;
    }
    let mut seed = [0u8; 32];
    if getrandom::fill(&mut seed).is_err() {
        report_error(&mut cx, "OperationError: cannot get random values");
        return false;
    }
    match mldsa_expand(&kind, &seed) {
        Ok((pkcs8, spki)) => {
            use base64::Engine as _;
            let json = serde_json::json!({
                "pkcs8": base64::engine::general_purpose::STANDARD.encode(pkcs8),
                "spki": base64::engine::general_purpose::STANDARD.encode(spki),
            })
            .to_string();
            set_rval_str(&mut cx, &frame, &json);
            true
        }
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_mldsa_seed_from_pkcs8(der)` → JSON `{kind, seed, spki}`（b64；导入即展开校验）。
pub unsafe extern "C" fn mldsa_seed_from_pkcs8(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(der) = (if frame.argc() > 0 { view_bytes(&mut cx, frame.arg(0), "ml-dsa key") } else { None }) else {
        return false;
    };
    let Some((oid, seed)) = mlkem_pkcs8_seed(&der, 32).and_then(|(o, s)| mldsa_kind_by_oid(o).map(|k| (k, s))) else {
        report_error(&mut cx, "TypeError: Invalid PKCS#8 key");
        return false;
    };
    let mut s = [0u8; 32];
    s.copy_from_slice(seed);
    match mldsa_expand(oid, &s) {
        Ok((_, spki)) => {
            use base64::Engine as _;
            let json = serde_json::json!({
                "kind": oid,
                "seed": base64::engine::general_purpose::STANDARD.encode(s),
                "spki": base64::engine::general_purpose::STANDARD.encode(spki),
            })
            .to_string();
            set_rval_str(&mut cx, &frame, &json);
            true
        }
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_mldsa_kind_from_spki(der)` → kind 串（OID + pk 长度校验，失败回空串）。
pub unsafe extern "C" fn mldsa_kind_from_spki(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(der) = (if frame.argc() > 0 { view_bytes(&mut cx, frame.arg(0), "ml-dsa key") } else { None }) else {
        return false;
    };
    let kind = mlkem_spki_ek(&der)
        .and_then(|(oid, pk)| mldsa_kind_by_oid(oid).filter(|k| mldsa_params(k).is_some_and(|(_, pk_len, _)| pk_len == pk.len())))
        .unwrap_or("");
    use mozjs::conversions::ToJSValConvertible as _;
    kind.to_jsval(&mut cx, frame.rval_mut());
    true
}

/// `__wjs_mldsa_public(pkcs8Der)` → SPKI DER（私钥派生公钥，createPublicKey 链用）。
pub unsafe extern "C" fn mldsa_public(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(der) = (if frame.argc() > 0 { view_bytes(&mut cx, frame.arg(0), "ml-dsa key") } else { None }) else {
        return false;
    };
    let Some((oid, seed)) = mlkem_pkcs8_seed(&der, 32).and_then(|(o, s)| mldsa_kind_by_oid(o).map(|k| (k, s))) else {
        report_error(&mut cx, "TypeError: Invalid PKCS#8 key");
        return false;
    };
    let mut s = [0u8; 32];
    s.copy_from_slice(seed);
    match mldsa_expand(oid, &s) {
        Ok((_, spki)) => set_rval_bytes(&mut cx, &frame, &spki),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_mldsa_sign(pkcs8Der, data)` → 签名（确定性档，FIPS 204 可选形；空上下文）。
pub unsafe extern "C" fn mldsa_sign(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: ml-dsa sign needs key and data");
        return false;
    }
    let (Some(der), Some(data)) = (
        view_bytes(&mut cx, frame.arg(0), "ml-dsa key"),
        view_bytes(&mut cx, frame.arg(1), "data"),
    ) else {
        return false;
    };
    let Some((oid, seed)) = mlkem_pkcs8_seed(&der, 32).and_then(|(o, s)| mldsa_kind_by_oid(o).map(|k| (k, s))) else {
        report_error(&mut cx, "TypeError: Invalid PKCS#8 key");
        return false;
    };
    let mut s = [0u8; 32];
    s.copy_from_slice(seed);
    use ml_dsa::Signer as _;
    let out: Result<Vec<u8>, String> = match oid {
        "ml-dsa-44" => {
            let sig = ml_dsa::SigningKey::<ml_dsa::MlDsa44>::from_seed(&s.into()).sign(&data);
            Ok(sig.encode().as_slice().to_vec())
        }
        "ml-dsa-65" => {
            let sig = ml_dsa::SigningKey::<ml_dsa::MlDsa65>::from_seed(&s.into()).sign(&data);
            Ok(sig.encode().as_slice().to_vec())
        }
        _ => {
            let sig = ml_dsa::SigningKey::<ml_dsa::MlDsa87>::from_seed(&s.into()).sign(&data);
            Ok(sig.encode().as_slice().to_vec())
        }
    };
    match out {
        Ok(sig) => set_rval_bytes(&mut cx, &frame, &sig),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_mldsa_verify(pubDer, sig, data)` → boolean。
pub unsafe extern "C" fn mldsa_verify(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: ml-dsa verify needs key, signature and data");
        return false;
    }
    let (Some(der), Some(sig), Some(data)) = (
        view_bytes(&mut cx, frame.arg(0), "ml-dsa key"),
        view_bytes(&mut cx, frame.arg(1), "signature"),
        view_bytes(&mut cx, frame.arg(2), "data"),
    ) else {
        return false;
    };
    let Some((oid, pk)) = mlkem_spki_ek(&der).and_then(|(o, p)| mldsa_kind_by_oid(o).map(|k| (k, p))) else {
        report_error(&mut cx, "TypeError: Invalid SPKI key");
        return false;
    };
    let ok = mldsa_verify_core(oid, pk, &sig, &data);
    frame.set_rval(mozjs::jsval::BooleanValue(ok));
    true
}

/// 验签核（native 与 X.509 验签共用；pub/kind/sig/data 逐项校验，不过即 false）。
pub(crate) fn mldsa_verify_core(kind: &str, pk: &[u8], sig: &[u8], data: &[u8]) -> bool {
    use ml_dsa::KeyInit as _;
    macro_rules! case {
        ($P:ty, $name:expr) => {
            if kind == $name {
                let (Some(arr), Some(sig)) = (
                    <&ml_dsa::common::Key<ml_dsa::VerifyingKey<$P>>>::try_from(pk).ok(),
                    ml_dsa::Signature::<$P>::try_from(sig).ok(),
                ) else {
                    return false;
                };
                let vk = ml_dsa::VerifyingKey::<$P>::new(arr);
                use ml_dsa::Verifier as _;
                return vk.verify(data, &sig).is_ok();
            }
        };
    }
    case!(ml_dsa::MlDsa44, "ml-dsa-44");
    case!(ml_dsa::MlDsa65, "ml-dsa-65");
    case!(ml_dsa::MlDsa87, "ml-dsa-87");
    false
}

// ── 9e-1d X509（`x509-cert` 直用；9i-3 起验签面就位：verify/publicKey/ca，
//    checkIssued/checkPrivateKey/签发不做）────────────────────

/// unix 秒 → `MMM DD HH:MM:SS YYYY GMT`（openssl `ASN1_TIME_print` 口径，日空位补空格）。
fn fmt_asn1_time(secs: u64) -> String {
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
fn atv_short(oid: &str) -> &str {
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
fn x509_names_raw(der: &[u8]) -> Option<(&[u8], &[u8])> {
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

/// 内嵌 ESM 源（`node:crypto` 9e-1a 面）。
pub const SOURCE: &str = r#"
function __cryptErr(e) {
  const m = String((e && e.message) || e);
  const code = (m.match(/^([A-Z][A-Z0-9_]*): /) || [])[1];
  if (!code) throw new Error(m);
  const rest = m.replace(/^[A-Z][A-Z0-9_]*: /, "");
  const err = new Error(rest || code);
  err.code = code;
  throw err;
}
function __cryptCall(fn) {
  try {
    return fn();
  } catch (e) {
    __cryptErr(e);
  }
}
function __cryptBytes(input, what, inputEncoding) {
  if (typeof input === "string") {
    try {
      return Buffer.from(input, inputEncoding ?? "utf8");
    } catch {
      return Buffer.from(input, "utf8");
    }
  }
  if (input instanceof Uint8Array) return input;
  if (input instanceof ArrayBuffer) return new Uint8Array(input);
  if (ArrayBuffer.isView(input)) {
    return new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
  }
  const err = new TypeError(
    `The "${what}" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received ${input === null ? "null" : typeof input}`);
  err.code = "ERR_INVALID_ARG_TYPE";
  throw err;
}
function __outBuf(u8, encoding) {
  const b = Buffer.from(u8.buffer, u8.byteOffset, u8.byteLength);
  if (encoding === undefined) return b;
  try {
    return b.toString(encoding);
  } catch {
    return b;
  }
}
function __needStr(v, what) {
  if (typeof v !== "string") {
    const err = new TypeError(
      `The "${what}" argument must be of type string. Received type ${typeof v} (${String(v)})`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  return v;
}

class Hash {
  constructor(algorithm, options) {
    __needStr(algorithm, "algorithm");
    // XOF 输出长（真机口径：缺省 shake128→16/shake256→32 + DEP0198 警告）。
    let xofLen = 0;
    const flat = String(algorithm).trim().toLowerCase().replace(/[-_]/g, "");
    if (flat === "shake128" || flat === "shake256") {
      const dflt = flat === "shake128" ? 16 : 32;
      if (options?.outputLength === undefined) {
        xofLen = dflt;
        try {
          process.emitWarning(
            "Creating SHAKE128/256 digests without an explicit options.outputLength is deprecated.",
            { type: "DeprecationWarning", code: "DEP0198" }
          );
        } catch {}
      } else {
        xofLen = Number(options.outputLength);
        if (!Number.isInteger(xofLen) || xofLen < 0) {
          const err = new TypeError(`The "options.outputLength" property must be a non-negative integer.`);
          err.code = "ERR_INVALID_ARG_VALUE";
          throw err;
        }
      }
    }
    this.__id = Number(__cryptCall(() => __wjs_crypto_hash_new(algorithm, String(xofLen))));
    this.__finalized = false;
  }
  update(data, inputEncoding) {
    if (this.__finalized) {
      const err = new Error("Digest already called");
      err.code = "ERR_CRYPTO_HASH_FINALIZED";
      throw err;
    }
    if (data === undefined) {
      const err = new TypeError(
        'The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received undefined');
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    const bytes = __cryptBytes(data, "data", inputEncoding);
    __cryptCall(() => __wjs_crypto_hash_update(String(this.__id), bytes));
    return this;
  }
  digest(encoding) {
    if (this.__finalized) {
      const err = new Error("Digest already called");
      err.code = "ERR_CRYPTO_HASH_FINALIZED";
      throw err;
    }
    this.__finalized = true;
    const out = __cryptCall(() => __wjs_crypto_hash_digest(String(this.__id)));
    return __outBuf(out, encoding);
  }
  copy() {
    if (this.__finalized) {
      const err = new Error("Digest already called");
      err.code = "ERR_CRYPTO_HASH_FINALIZED";
      throw err;
    }
    const h = Object.create(Hash.prototype);
    h.__id = Number(__cryptCall(() => __wjs_crypto_hash_copy(String(this.__id))));
    h.__finalized = false;
    return h;
  }
}

function __hmacBlockLen(flat) {
  switch (flat) {
    case "sha384": case "sha512": case "blake2b512": return 128;
    case "sha3256": return 136;
    case "sha3384": return 104;
    case "sha3512": return 72;
    default: return 64;
  }
}
// 通用 HMAC 构造（RFC 2104），架在自家流式 Hash natives 上：
// sha3 系与 hmac 0.13 的 block-API 不兼容，故 SHA-2 系同样走此路（输出与
// `__wjs_hmac_sign` 逐字节一致，黑盒以真 Node 向量钉住）。
function __hmacGeneric(flat, keyBytes, dataBytes) {
  const block = __hmacBlockLen(flat);
  let key = keyBytes;
  if (key.length > block) {
    const h = Number(__wjs_crypto_hash_new(flat));
    __wjs_crypto_hash_update(String(h), key);
    key = __wjs_crypto_hash_digest(String(h));
  }
  const padded = new Uint8Array(block);
  padded.set(key);
  const ipad = new Uint8Array(block);
  const opad = new Uint8Array(block);
  for (let i = 0; i < block; i++) { ipad[i] = padded[i] ^ 0x36; opad[i] = padded[i] ^ 0x5c; }
  const inner = new Uint8Array(block + dataBytes.length);
  inner.set(ipad, 0); inner.set(dataBytes, block);
  const hi = Number(__wjs_crypto_hash_new(flat));
  __wjs_crypto_hash_update(String(hi), inner);
  const innerDigest = __wjs_crypto_hash_digest(String(hi));
  const outer = new Uint8Array(block + innerDigest.length);
  outer.set(opad, 0); outer.set(innerDigest, block);
  const ho = Number(__wjs_crypto_hash_new(flat));
  __wjs_crypto_hash_update(String(ho), outer);
  return __wjs_crypto_hash_digest(String(ho));
}

class Hmac {
  constructor(hamc, key, options) {
    __needStr(hamc, "algorithm");
    if (key === undefined || key === null ||
        !(typeof key === "string" || key instanceof Uint8Array ||
          key instanceof ArrayBuffer || ArrayBuffer.isView(key))) {
      const err = new TypeError(
        'The "key" argument must be of type string or an instance of ArrayBuffer, Buffer, TypedArray, DataView, KeyObject, or CryptoKey. Received ' +
        (key === undefined ? "undefined" : (key === null ? "null" : typeof key)));
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    // Node 口径：未知摘要直接 ERR_CRYPTO_INVALID_DIGEST（真机取证）
    const flat = String(hamc).trim().toLowerCase().replace(/[-_]/g, "");
    if (flat === "shake128" || flat === "shake256") {
      // 真机同款：OpenSSL 底层抛无码错（HMAC 不支持 XOF）。
      throw new Error(`Invalid digest: ${hamc}`);
    }
    const table = {
      "sha1": "sha1", "sha256": "sha256", "sha384": "sha384", "sha512": "sha512",
      "md5": "md5", "sha3256": "sha3256", "sha3384": "sha3384", "sha3512": "sha3512",
      "blake2b512": "blake2b512", "blake2s256": "blake2s256",
      "ripemd160": "ripemd160",
    };
    const norm = table[flat];
    if (norm === undefined) {
      const err = new Error(`Invalid digest: ${hamc}`);
      err.code = "ERR_CRYPTO_INVALID_DIGEST";
      throw err;
    }
    this.__alg = norm;
    this.__key = __cryptBytes(key, "key");
    this.__parts = [];
    this.__finalized = false;
  }
  update(data, inputEncoding) {
    if (this.__finalized) {
      const err = new Error("Digest already called");
      err.code = "ERR_CRYPTO_HASH_FINALIZED";
      throw err;
    }
    if (data === undefined) {
      const err = new TypeError(
        'The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received undefined');
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    this.__parts.push(__cryptBytes(data, "data", inputEncoding));
    return this;
  }
  digest(encoding) {
    if (this.__finalized) {
      return encoding === undefined ? Buffer.alloc(0) : "";
    }
    this.__finalized = true;
    let total = 0;
    for (const p of this.__parts) total += p.length;
    const flat = new Uint8Array(total);
    let off = 0;
    for (const p of this.__parts) { flat.set(p, off); off += p.length; }
    this.__parts = [];
    const out = __cryptCall(() => __hmacGeneric(this.__alg, this.__key, flat));
    return __outBuf(out, encoding);
  }
}

export function createHash(algorithm, options) {
  return new Hash(algorithm, options);
}
export function createHmac(hamc, key, options) {
  return new Hmac(hamc, key, options);
}
export function hash(algorithm, data, outputEncoding) {
  __needStr(algorithm, "algorithm");
  const bytes = __cryptBytes(data, "data");
  const probe = __cryptCall(() => __wjs_crypto_hash_new(algorithm));
  __cryptCall(() => __wjs_crypto_hash_update(probe, bytes));
  const out = __cryptCall(() => __wjs_crypto_hash_digest(probe));
  if (outputEncoding === undefined) return __outBuf(out, undefined);
  const valid = ["hex", "base64", "base64url", "latin1", "binary", "ascii", "utf8", "utf-8", "ucs2", "utf16le"];
  if (!valid.includes(String(outputEncoding).toLowerCase().replace(/[-_]/g, "")) &&
      !valid.includes(String(outputEncoding).toLowerCase())) {
    const err = new TypeError(`The argument 'outputEncoding' is invalid. Received '${outputEncoding}'`);
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
  return __outBuf(out, outputEncoding);
}

function __randSize(size) {
  if (typeof size !== "number" || !Number.isInteger(size)) {
    const err = new TypeError(`The "size" argument must be of type number. Received type ${typeof size} (${String(size)})`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (size < 0 || size > 2147483647) {
    const err = new RangeError(`The value of "size" is out of range. It must be >= 0 && <= 2147483647. Received ${size}`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  return size;
}
function __randFill(view) {
  __wjs_fill_random(view);
  return view;
}
export function randomBytes(size, callback) {
  const n = __randSize(size);
  const buf = Buffer.from(__randFill(new Uint8Array(n)).buffer);
  if (callback === undefined) return buf;
  if (typeof callback !== "function") {
    const err = new TypeError("The \"callback\" argument must be of type function");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  queueMicrotask(() => callback(null, buf));
  return undefined;
}
export function randomFillSync(buf, offset = 0, size) {
  if (!(buf instanceof ArrayBuffer) && !ArrayBuffer.isView(buf)) {
    const err = new TypeError(`The "buf" argument must be an instance of ArrayBuffer or ArrayBufferView. Received type ${typeof buf}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const len = buf.byteLength;
  offset = Number(offset);
  if (!Number.isInteger(offset) || offset < 0 || offset > len) {
    const err = new RangeError("The value of \"offset\" is out of range");
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  if (size === undefined) size = len - offset;
  size = Number(size);
  if (!Number.isInteger(size) || size < 0 || offset + size > len) {
    const err = new RangeError(`The value of "size + offset" is out of range. It must be <= ${len}. Received ${offset + size}`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  const view = new Uint8Array(buf.buffer ?? buf, (buf.byteOffset ?? 0) + offset, size);
  __randFill(view);
  return buf;
}
export function randomFill(buf, offset, size, callback) {
  if (typeof offset === "function") { callback = offset; offset = 0; size = undefined; }
  else if (typeof size === "function") { callback = size; size = undefined; }
  if (typeof callback !== "function") {
    const err = new TypeError("The \"callback\" argument must be of type function");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  queueMicrotask(() => {
    try {
      callback(null, randomFillSync(buf, offset, size));
    } catch (e) {
      callback(e);
    }
  });
  return undefined;
}
export function randomInt(min, max, callback) {
  if (callback === undefined && typeof max === "function") { callback = max; max = undefined; }
  if (max === undefined) { max = min; min = 0; }
  const done = (err, val) => (callback === undefined ? undefined : callback(err, val));
  const fail = (err) => {
    if (callback === undefined) throw err;
    queueMicrotask(() => callback(err));
    return undefined;
  };
  for (const [name, v] of [["min", min], ["max", max]]) {
    if (typeof v !== "number" || !Number.isSafeInteger(v)) {
      const err = new TypeError(`The "${name}" argument must be a safe integer. Received ${v === undefined ? "undefined" : `type number (${String(v)})`}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      return fail(err);
    }
  }
  if (max <= min) {
    const err = new RangeError(`The value of "max" is out of range. It must be greater than the value of "min" (${min}). Received ${max}`);
    err.code = "ERR_OUT_OF_RANGE";
    return fail(err);
  }
  const range = max - min;
  if (range > 281474976710656) {
    const err = new RangeError("The value of \"range\" is out of range. It must be <= 281474976710656");
    err.code = "ERR_OUT_OF_RANGE";
    return fail(err);
  }
  // 拒绝采样（偏差可忽略；大区间恒终止，单测断言分布形状不测时序）
  const bytes = range <= 256 ? 1 : range <= 65536 ? 2 : range <= 16777216 ? 3 : range <= 4294967296 ? 4 : 6;
  const limit = Math.floor(256 ** bytes / range) * range;
  const pick = () => {
    const u8 = __randFill(new Uint8Array(bytes));
    let v = 0;
    for (const b of u8) v = v * 256 + b;
    if (v >= limit) return pick();
    return min + (v % range);
  };
  if (callback === undefined) return pick();
  queueMicrotask(() => {
    try {
      callback(null, pick());
    } catch (e) {
      callback(e);
    }
  });
  return undefined;
}
export function randomUUID() {
  return __wjs_random_uuid();
}
export function randomUUIDv7() {
  const ms = Date.now();
  const u8 = __randFill(new Uint8Array(16));
  u8[0] = Math.floor(ms / 2 ** 40) & 255;
  u8[1] = Math.floor(ms / 2 ** 32) & 255;
  u8[2] = Math.floor(ms / 2 ** 24) & 255;
  u8[3] = Math.floor(ms / 2 ** 16) & 255;
  u8[4] = Math.floor(ms / 2 ** 8) & 255;
  u8[5] = ms & 255;
  u8[6] = (u8[6] & 15) | 112;
  u8[8] = (u8[8] & 63) | 128;
  const hex = [...u8].map((x) => x.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
export function timingSafeEqual(a, b) {
  const isView = (v) => v instanceof Uint8Array || v instanceof ArrayBuffer || ArrayBuffer.isView(v);
  if (!isView(a)) {
    const err = new TypeError('The "buf1" argument must be an instance of ArrayBuffer, Buffer, TypedArray, or DataView.');
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (!isView(b)) {
    const err = new TypeError('The "buf2" argument must be an instance of ArrayBuffer, Buffer, TypedArray, or DataView.');
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const x = a instanceof Uint8Array ? a : new Uint8Array(a.buffer ?? a, a.byteOffset ?? 0, a.byteLength);
  const y = b instanceof Uint8Array ? b : new Uint8Array(b.buffer ?? b, b.byteOffset ?? 0, b.byteLength);
  if (x.length !== y.length) {
    const err = new Error("Input buffers must have the same byte length");
    err.code = "ERR_CRYPTO_TIMING_SAFE_EQUAL_LENGTH";
    throw err;
  }
  let acc = 0;
  for (let i = 0; i < x.length; i++) acc |= x[i] ^ y[i];
  return acc === 0;
}
export function getHashes() {
  return ["sha1", "sha256", "sha384", "sha512", "md5", "sha3-256", "sha3-384", "sha3-512", "blake2b512", "blake2s256", "ripemd160", "shake128", "shake256"];
}
export function getCurves() {
  return ["prime256v1", "secp384r1", "secp521r1", "secp256k1", "ed25519", "x25519"];
}
export const webcrypto = globalThis.crypto;
// Node 19+ 模块级 getRandomValues（webcrypto 同一实现；vite webSocketToken 用）。
export function getRandomValues(typedArray) {
  return globalThis.crypto.getRandomValues(typedArray);
}

const __CIPHERS = {
  "aes-128-cbc": { family: "cbc", key: 16, iv: 16, block: 16, mode: "cbc", nid: 419 },
  "aes-192-cbc": { family: "cbc", key: 24, iv: 16, block: 16, mode: "cbc", nid: 423 },
  "aes-256-cbc": { family: "cbc", key: 32, iv: 16, block: 16, mode: "cbc", nid: 427 },
  "aes-128-ctr": { family: "ctr", key: 16, iv: 16, block: 16, mode: "ctr", nid: 904 },
  "aes-192-ctr": { family: "ctr", key: 24, iv: 16, block: 16, mode: "ctr", nid: 905 },
  "aes-256-ctr": { family: "ctr", key: 32, iv: 16, block: 16, mode: "ctr", nid: 906 },
  "aes-128-gcm": { family: "gcm", key: 16, iv: 12, block: 16, mode: "gcm", nid: 961 },
  "aes-192-gcm": { family: "gcm", key: 24, iv: 12, block: 16, mode: "gcm", nid: 962 },
  "aes-256-gcm": { family: "gcm", key: 32, iv: 12, block: 16, mode: "gcm", nid: 963 },
  "chacha20-poly1305": { family: "chacha", key: 32, iv: 12, block: 16, mode: "chacha20-poly1305", nid: 1018 },
  "des-ede3-cbc": { family: "cbc", key: 24, iv: 8, block: 8, mode: "cbc", nid: 44 },
};
function __cipherInfo(cipher) {
  const info = __CIPHERS[String(cipher).toLowerCase()];
  return info === undefined ? undefined : { name: String(cipher).toLowerCase(), ...info };
}
function __needCipher(cipher) {
  const info = __cipherInfo(cipher);
  if (info === undefined) {
    const err = new Error("Unknown cipher");
    err.code = "ERR_CRYPTO_UNKNOWN_CIPHER";
    throw err;
  }
  return info;
}
function __needKeyIv(info, key, iv, what) {
  const kb = __cryptBytes(key, "key");
  if (kb.length !== info.key) {
    const err = new Error("Invalid key length");
    err.code = "ERR_CRYPTO_INVALID_KEYLEN";
    throw err;
  }
  const ivb = __cryptBytes(iv, "iv");
  if (ivb.length !== info.iv) {
    const err = new Error("Invalid initialization vector");
    err.code = "ERR_CRYPTO_INVALID_IV";
    throw err;
  }
  return [kb, ivb];
}
function __badState() {
  const err = new Error("Invalid state");
  err.code = "ERR_CRYPTO_INVALID_STATE";
  throw err;
}
function __unsupportedState() {
  throw new Error("Trying to add data in unsupported state");
}

class Cipheriv {
  constructor(cipher, key, iv, options) {
    const info = __needCipher(cipher);
    const [kb, ivb] = __needKeyIv(info, key, iv, "cipher");
    this.__info = info;
    this.__aad = null;
    this.__aadDone = false;
    this.__tag = null;
    this.__finalized = false;
    if (info.family === "cbc" || info.family === "ctr") {
      this.__id = Number(__cryptCall(() =>
        __wjs_cipher_new(info.name, kb, ivb, 1, options && options.autoPadding === false ? 0 : 1)));
      this.__parts = null;
    } else {
      // AEAD 无流式：buffered，final 时 oneshot（头注记档）
      this.__id = null;
      this.__parts = [];
      this.__key = kb;
      this.__iv = ivb;
    }
  }
  setAAD(aad, options) {
    if (this.__info.family !== "gcm" && this.__info.family !== "chacha") {
      const err = new Error("Trying to add data in unsupported state");
      throw err;
    }
    if (this.__finalized || (this.__parts !== null && this.__aadDone)) __badState();
    this.__aad = __cryptBytes(aad, "aad");
    this.__aadDone = true;
    return this;
  }
  setAutoPadding(autoPad) {
    if (this.__id !== null) {
      // CBC native 侧创建期已定；此处仅守卫时序（final 后调即错，Node 同款）
      if (this.__finalized) __badState();
    }
    return this;
  }
  getAuthTag() {
    if (this.__tag === null) __badState();
    return this.__tag;
  }
  update(data, inputEncoding, outputEncoding) {
    if (this.__finalized) __unsupportedState();
    const bytes = data === undefined
      ? (() => { const err = new TypeError('The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView.'); err.code = "ERR_INVALID_ARG_TYPE"; throw err; })()
      : __cryptBytes(data, "data", inputEncoding);
    let out;
    if (this.__id !== null) {
      out = __cryptCall(() => __wjs_cipher_update(String(this.__id), bytes));
    } else {
      this.__parts.push(bytes);
      out = new Uint8Array(0);
    }
    return __outBuf(out, outputEncoding);
  }
  final(outputEncoding) {
    if (this.__finalized) __badState();
    this.__finalized = true;
    let out;
    if (this.__id !== null) {
      out = __cryptCall(() => __wjs_cipher_final(String(this.__id)));
    } else if (this.__info.family === "gcm") {
      if (this.__iv.length !== 12) {
        const err = new Error("Invalid initialization vector");
        err.code = "ERR_CRYPTO_INVALID_IV";
        throw err;
      }
      const pt = __joinParts(this.__parts);
      const tagged = __cryptCall(() =>
        __wjs_aesgcm_encrypt(this.__key, this.__iv, this.__aad ?? new Uint8Array(0), pt));
      out = tagged.slice(0, tagged.length - 16);
      this.__tag = Buffer.from(tagged.slice(tagged.length - 16));
    } else {
      const pt = __joinParts(this.__parts);
      const aad = this.__aad ?? new Uint8Array(0);
      const tagged = __cryptCall(() =>
        __wjs_cipher_chacha(1, this.__key, this.__iv, aad, pt, null));
      out = tagged.slice(0, tagged.length - 16);
      this.__tag = Buffer.from(tagged.slice(tagged.length - 16));
    }
    return __outBuf(out, outputEncoding);
  }
}

class Decipheriv {
  constructor(cipher, key, iv, options) {
    const info = __needCipher(cipher);
    const [kb, ivb] = __needKeyIv(info, key, iv, "decipher");
    this.__info = info;
    this.__aad = null;
    this.__tag = null;
    this.__finalized = false;
    this.__autoPad = !(options && options.autoPadding === false);
    if (info.family === "cbc" || info.family === "ctr") {
      this.__id = Number(__cryptCall(() =>
        __wjs_cipher_new(info.name, kb, ivb, 0, this.__autoPad ? 1 : 0)));
      this.__parts = null;
    } else {
      this.__id = null;
      this.__parts = [];
      this.__key = kb;
      this.__iv = ivb;
    }
  }
  setAAD(aad, options) {
    if (this.__info.family !== "gcm" && this.__info.family !== "chacha") {
      throw new Error("Trying to add data in unsupported state");
    }
    if (this.__finalized) __badState();
    this.__aad = __cryptBytes(aad, "aad");
    return this;
  }
  setAuthTag(tag) {
    this.__tag = __cryptBytes(tag, "tag");
    return this;
  }
  setAutoPadding(autoPad) {
    if (this.__finalized) __badState();
    this.__autoPad = !!autoPad;
    return this;
  }
  update(data, inputEncoding, outputEncoding) {
    if (this.__finalized) __unsupportedState();
    if (data === undefined) {
      const err = new TypeError('The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView.');
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    const bytes = __cryptBytes(data, "data", inputEncoding);
    let out;
    if (this.__id !== null) {
      out = __cryptCall(() => __wjs_cipher_update(String(this.__id), bytes));
    } else {
      this.__parts.push(bytes);
      out = new Uint8Array(0);
    }
    return __outBuf(out, outputEncoding);
  }
  final(outputEncoding) {
    if (this.__finalized) __badState();
    this.__finalized = true;
    let out;
    if (this.__id !== null) {
      out = __cryptCall(() => __wjs_cipher_final(String(this.__id)));
    } else if (this.__info.family === "gcm") {
      const ct = __joinParts(this.__parts);
      if (this.__tag === null || this.__tag.length !== 16) {
        throw new Error("Unsupported state or unable to authenticate data");
      }
      const input = new Uint8Array(ct.length + 16);
      input.set(ct, 0); input.set(this.__tag, ct.length);
      try {
        out = __wjs_aesgcm_decrypt(this.__key, this.__iv, this.__aad ?? new Uint8Array(0), input);
      } catch {
        throw new Error("Unsupported state or unable to authenticate data");
      }
    } else {
      const ct = __joinParts(this.__parts);
      if (this.__tag === null || this.__tag.length !== 16) {
        throw new Error("Unsupported state or unable to authenticate data");
      }
      out = __cryptCall(() => __wjs_cipher_chacha(
        0, this.__key, this.__iv, this.__aad ?? new Uint8Array(0), ct, this.__tag));
    }
    return __outBuf(out, outputEncoding);
  }
}

function __joinParts(parts) {
  let total = 0;
  for (const p of parts) total += p.length;
  const out = new Uint8Array(total);
  let off = 0;
  for (const p of parts) { out.set(p, off); off += p.length; }
  return out;
}

export function createCipheriv(cipher, key, iv, options) {
  return new Cipheriv(cipher, key, iv, options);
}
export function createDecipheriv(cipher, key, iv, options) {
  return new Decipheriv(cipher, key, iv, options);
}
export function getCiphers() {
  return Object.keys(__CIPHERS);
}
export function getCipherInfo(name) {
  const info = __cipherInfo(name);
  if (info === undefined) return undefined;
  return {
    name: info.name, mode: info.mode, keyLength: info.key,
    ivLength: info.iv, blockSize: info.block, nid: info.nid,
  };
}

// ── 9e-1c 非对称 ──────────────────────────────────────────────────────────

const __DH_GROUPS = {
  // 素数取自真 Node `getPrime('hex')`（RFC 7919），generator 恒 2
  "modp5": "ffffffffffffffffc90fdaa22168c234c4c6628b80dc1cd129024e088a67cc74020bbea63b139b22514a08798e3404ddef9519b3cd3a431b302b0a6df25f14374fe1356d6d51c245e485b576625e7ec6f44c42e9a637ed6b0bff5cb6f406b7edee386bfb5a899fa5ae9f24117c4b1fe649286651ece45b3dc2007cb8a163bf0598da48361c55d39a69163fa8fd24cf5f83655d23dca3ad961c62f356208552bb9ed529077096966d670c354e4abc9804f1746c08ca237327ffffffffffffffff",
  "modp14": "ffffffffffffffffc90fdaa22168c234c4c6628b80dc1cd129024e088a67cc74020bbea63b139b22514a08798e3404ddef9519b3cd3a431b302b0a6df25f14374fe1356d6d51c245e485b576625e7ec6f44c42e9a637ed6b0bff5cb6f406b7edee386bfb5a899fa5ae9f24117c4b1fe649286651ece45b3dc2007cb8a163bf0598da48361c55d39a69163fa8fd24cf5f83655d23dca3ad961c62f356208552bb9ed529077096966d670c354e4abc9804f1746c08ca18217c32905e462e36ce3be39e772c180e86039b2783a2ec07a28fb5c55df06f4c52c9de2bcbf6955817183995497cea956ae515d2261898fa051015728e5a8aacaa68ffffffffffffffff",
  "modp15": "ffffffffffffffffc90fdaa22168c234c4c6628b80dc1cd129024e088a67cc74020bbea63b139b22514a08798e3404ddef9519b3cd3a431b302b0a6df25f14374fe1356d6d51c245e485b576625e7ec6f44c42e9a637ed6b0bff5cb6f406b7edee386bfb5a899fa5ae9f24117c4b1fe649286651ece45b3dc2007cb8a163bf0598da48361c55d39a69163fa8fd24cf5f83655d23dca3ad961c62f356208552bb9ed529077096966d670c354e4abc9804f1746c08ca18217c32905e462e36ce3be39e772c180e86039b2783a2ec07a28fb5c55df06f4c52c9de2bcbf6955817183995497cea956ae515d2261898fa051015728e5a8aaac42dad33170d04507a33a85521abdf1cba64ecfb850458dbef0a8aea71575d060c7db3970f85a6e1e4c7abf5ae8cdb0933d71e8c94e04a25619dcee3d2261ad2ee6bf12ffa06d98a0864d87602733ec86a64521f2b18177b200cbbe117577a615d6c770988c0bad946e208e24fa074e5ab3143db5bfce0fd108e4b82d120a93ad2caffffffffffffffff",
  "modp16": "ffffffffffffffffc90fdaa22168c234c4c6628b80dc1cd129024e088a67cc74020bbea63b139b22514a08798e3404ddef9519b3cd3a431b302b0a6df25f14374fe1356d6d51c245e485b576625e7ec6f44c42e9a637ed6b0bff5cb6f406b7edee386bfb5a899fa5ae9f24117c4b1fe649286651ece45b3dc2007cb8a163bf0598da48361c55d39a69163fa8fd24cf5f83655d23dca3ad961c62f356208552bb9ed529077096966d670c354e4abc9804f1746c08ca18217c32905e462e36ce3be39e772c180e86039b2783a2ec07a28fb5c55df06f4c52c9de2bcbf6955817183995497cea956ae515d2261898fa051015728e5a8aaac42dad33170d04507a33a85521abdf1cba64ecfb850458dbef0a8aea71575d060c7db3970f85a6e1e4c7abf5ae8cdb0933d71e8c94e04a25619dcee3d2261ad2ee6bf12ffa06d98a0864d87602733ec86a64521f2b18177b200cbbe117577a615d6c770988c0bad946e208e24fa074e5ab3143db5bfce0fd108e4b82d120a93ad2caffffffffffffffff",
};

function __b64enc(u8) {
  let s = "";
  for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000));
  return btoa(s);
}
function __b64dec(s) {
  const bin = atob(String(s));
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}
function __b64urlDec(s) {
  let t = String(s).replace(/-/g, "+").replace(/_/g, "/");
  while (t.length % 4) t += "=";
  return __b64dec(t);
}
// 最小 DER 读器（SEC1/DH-PKCS8 解析用；完整 ASN.1 不做）
function __derRead(buf, pos) {
  const tag = buf[pos];
  let len = buf[pos + 1];
  let off = pos + 2;
  if (len & 128) {
    const n = len & 127;
    len = 0;
    for (let i = 0; i < n; i++) len = len * 256 + buf[off++];
  }
  return { tag, len, head: off, body: buf.slice(off, off + len), next: off + len };
}
function __derChildren(buf) {
  const out = [];
  let pos = 0;
  while (pos < buf.length) {
    const t = __derRead(buf, pos);
    out.push(t);
    pos = t.next;
  }
  return out;
}
function __pemDecode(text) {
  const m = String(text).match(/-----BEGIN ([^-]+)-----([\s\S]*?)-----END \1-----/);
  if (!m) return null;
  return { label: m[1].trim(), der: __b64dec(m[2].replace(/\s+/g, "")) };
}
function __pemEncode(label, der) {
  const b64 = __b64enc(der);
  let body = "";
  for (let i = 0; i < b64.length; i += 64) body += b64.slice(i, i + 64) + "\n";
  return `-----BEGIN ${label}-----\n${body}-----END ${label}-----\n`;
}
function __normCurve(name) {
  const s = String(name ?? "").trim().toLowerCase().replace(/[-_]/g, "");
  const table = {
    "prime256v1": "P-256", "secp256r1": "P-256", "p256": "P-256",
    "secp384r1": "P-384", "p384": "P-384",
    "secp521r1": "P-521", "p521": "P-521",
    "secp256k1": "secp256k1", "k256": "secp256k1",
    "ed25519": "Ed25519", "x25519": "X25519",
  };
  const c = table[s];
  if (c === undefined) {
    const err = new Error(`Unknown curve ${name}`);
    err.code = "ERR_CRYPTO_INVALID_CURVE";
    throw err;
  }
  return c;
}
function __curveSize(curve) {
  return curve === "P-256" ? 32 : curve === "P-384" ? 48 : curve === "secp256k1" ? 32 : 66;
}
// DSA DER 解析（PKCS#8/SPKI；轮子无公开 Decode，JS 侧走 __derRead）。
// DSA OID 1.2.840.10040.4.1（hex 2a8648ce380401）；整数前导零剥掉（零值保一位）。
function __dsaInts(kids) {
  return kids.map((t) => {
    if (t.tag !== 2) throw new Error("no");
    let v = t.body;
    while (v.length > 1 && v[0] === 0) v = v.slice(1);
    return Buffer.from(v).toString("base64");
  });
}
function __parseDsaDer(der, want) {
  const fail = () => { throw new Error("no"); };
  const top = __derRead(der, 0);
  if (top.tag !== 48) fail();
  const kids = __derChildren(top.body);
  const dsaOid = "2a8648ce380401";
  const hex = (u8) => Buffer.from(u8).toString("hex");
  if (want === "private") {
    // SEQ{ INT 0, SEQ{ OID dsa, SEQ{ p,q,g } }, OCTET{ INT x } }
    if (kids.length < 3 || kids[0].tag !== 2) fail();
    const alg = __derChildren(kids[1].body);
    if (alg.length < 2 || alg[0].tag !== 6 || hex(alg[0].body) !== dsaOid) fail();
    const params = __derChildren(alg[1].body);
    if (params.length !== 3) fail();
    if (kids[2].tag !== 4) fail();
    const xTop = __derRead(kids[2].body, 0);
    if (xTop.tag !== 2) fail();
    const [p, q, g] = __dsaInts(params);
    const [x] = __dsaInts([xTop]);
    return { p, q, g, x };
  }
  // SEQ{ SEQ{ OID dsa, SEQ{ p,q,g } }, BITSTRING{ INT y } }
  if (kids.length < 2) fail();
  const alg = __derChildren(kids[0].body);
  if (alg.length < 2 || alg[0].tag !== 6 || hex(alg[0].body) !== dsaOid) fail();
  const params = __derChildren(alg[1].body);
  if (params.length !== 3) fail();
  if (kids[1].tag !== 3 || kids[1].body.length < 2 || kids[1].body[0] !== 0) fail();
  const yTop = __derRead(kids[1].body.slice(1), 0);
  if (yTop.tag !== 2) fail();
  const [p, q, g] = __dsaInts(params);
  const [y] = __dsaInts([yTop]);
  return { p, q, g, y };
}
function __dsaKeyObject(env, kind) {
  const k = new KeyObject(kind, "dsa", Buffer.from(JSON.stringify(env)));
  const pLen = Buffer.from(env.p, "base64").length, qLen = Buffer.from(env.q, "base64").length;
  k.__detail = { modulusLength: pLen * 8, divisorLength: qLen * 8 };
  return k;
}
function __normHashName(alg) {
  // 'RSA-SHA256' / 'sha256' → 'SHA-256'（WebCrypto 口径既有表）
  const s = String(alg ?? "");
  const up = s.trim().toUpperCase().replace(/[-_]/g, "");
  const bare = up.startsWith("RSA") ? up.slice(3) : up;
  const table = {
    "SHA1": "SHA-1", "SHA256": "SHA-256", "SHA384": "SHA-384", "SHA512": "SHA-512",
    "MD5": "MD5", "SHA3256": "SHA3-256", "SHA3384": "SHA3-384", "SHA3512": "SHA3-512",
  };
  return table[bare];
}

class KeyObject {
  constructor(kind, keyType, material) {
    this.__kind = kind; // 'secret' | 'public' | 'private'
    this.__keyType = keyType; // 'secret' | 'rsa' | 'rsa-pss' | 'ec' | 'ed25519' | 'x25519' | 'dh'
    this.__material = material; // Uint8Array（DER 或裸密钥字节）
    this.__detail = null; // 附加参数（namedCurve / prime+g / pss）
  }
  get type() { return this.__kind; }
  get asymmetricKeyType() { return this.__kind === "secret" ? undefined : this.__keyType; }
  get symmetricKeySize() {
    return this.__kind === "secret" ? this.__material.length : undefined;
  }
  export(options) {
    const format = options?.format ?? "pem";
    if (format === "jwk") return __exportJwk(this);
    const der = __exportDer(this, options);
    if (format === "der") return Buffer.from(der);
    if (format === "pem") {
      const label = this.__kind === "private" ? "PRIVATE KEY" : "PUBLIC KEY";
      return __pemEncode(label, der);
    }
    const err = new TypeError(`Unknown export format ${format}`);
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
}
function __exportDer(kobj, options) {
  // DH 私钥是 JS 侧三元组（无 DER 形态，记档）；其余直接吐 material
  if (kobj.__keyType === "dh") {
    const err = new Error("DH keys export as 'der'/'pem' is not supported (use getPrime/getPrivateKey)");
    err.code = "ERR_NOT_SUPPORTED";
    throw err;
  }
  // DSA material 是信封 JSON（非 DER），经轮子编 pkcs8/spki
  if (kobj.__keyType === "dsa") {
    const envStr = Buffer.from(kobj.__material).toString("utf8");
    const parts = JSON.parse(__cryptCall(() => __wjs_dsa_export(envStr)));
    if (kobj.__kind === "private") {
      if (!parts.privDer) {
        const err = new Error("DSA private key has no private material");
        err.code = "ERR_INVALID_ARG_VALUE";
        throw err;
      }
      return __b64dec(parts.privDer);
    }
    return __b64dec(parts.pubDer);
  }
  return kobj.__material;
}
function __exportJwk(kobj) {
  const b64u = (u8) => __b64enc(u8).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  if (kobj.__keyType === "secret") return { kty: "oct", k: b64u(kobj.__material) };
  const isPriv = kobj.__kind === "private";
  if (kobj.__keyType === "rsa" || kobj.__keyType === "rsa-pss") {
    const parts = isPriv
      ? JSON.parse(__wjs_rsa_jwk(kobj.__material, __wjs_rsa_public(kobj.__material)))
      : JSON.parse(__wjs_rsa_jwk_pub(kobj.__material));
    const jwk = { kty: "RSA", n: parts.n, e: parts.e };
    if (isPriv) { jwk.d = parts.d; jwk.p = parts.p; jwk.q = parts.q; jwk.dp = parts.dp; jwk.dq = parts.dq; jwk.qi = parts.qi; }
    return jwk;
  }
  if (kobj.__keyType === "ec") {
    const curve = kobj.__detail.namedCurve;
    const parts = isPriv
      ? JSON.parse(__wjs_ec_jwk(curve, kobj.__material, __wjs_ec_public(curve, kobj.__material)))
      : JSON.parse(__wjs_ec_jwk_pub(curve, kobj.__material));
    const jwk = { kty: "EC", crv: curve, x: parts.x, y: parts.y };
    if (isPriv) jwk.d = parts.d;
    return jwk;
  }
  if (kobj.__keyType === "ed25519" || kobj.__keyType === "x25519") {
    const crv = kobj.__keyType === "ed25519" ? "Ed25519" : "X25519";
    const pubBytes = isPriv
      ? (kobj.__keyType === "ed25519" ? __wjs_ed_public(kobj.__material) : __wjs_x_public(kobj.__material))
      : kobj.__material;
    const jwk = { kty: "OKP", crv, x: b64u(pubBytes) };
    if (isPriv) jwk.d = b64u(kobj.__material);
    return jwk;
  }
  if (kobj.__keyType === "dsa") {
    const env = JSON.parse(Buffer.from(kobj.__material).toString("utf8"));
    const jwk = { kty: "DSA", p: b64u(__b64dec(env.p)), q: b64u(__b64dec(env.q)), g: b64u(__b64dec(env.g)), y: b64u(__b64dec(env.y)) };
    if (isPriv) {
      if (!env.x) {
        const err = new Error("DSA private key has no private material");
        err.code = "ERR_INVALID_ARG_VALUE";
        throw err;
      }
      jwk.x = b64u(__b64dec(env.x));
    }
    return jwk;
  }
  if (typeof kobj.__keyType === "string" && (kobj.__keyType.startsWith("ml-kem-") || kobj.__keyType.startsWith("ml-dsa-"))) {
    // 9i-4/9i-6 真机口径：kty "AKP"，alg 参数集名，pub=裸公钥 / priv=种子（均 b64url）。
    const isKem = kobj.__keyType.startsWith("ml-kem-");
    const num = kobj.__keyType.split("-")[2];
    const alg = isKem ? "ML-KEM-" + num : "ML-DSA-" + num;
    const top = __derRead(kobj.__material, 0);
    const kids = __derChildren(top.body);
    const raw = kids[1].body.subarray(1);
    const jwk = { kty: "AKP", alg, pub: b64u(raw) };
    if (isPriv) {
      const parts = JSON.parse(__cryptCall(() => (isKem
        ? __wjs_mlkem_seed_from_pkcs8(kobj.__material)
        : __wjs_mldsa_seed_from_pkcs8(kobj.__material))));
      jwk.priv = b64u(__b64dec(parts.seed));
    }
    return jwk;
  }
  const err = new Error("JWK export not supported for this key type");
  err.code = "ERR_NOT_SUPPORTED";
  throw err;
}
export function createSecretKey(key) {
  return new KeyObject("secret", "secret", __cryptBytes(key, "key"));
}
function __parseKeyMaterial(key, format, type, want) {
  // → { keyType, material, detail }；want: 'private' | 'public'
  if (key instanceof KeyObject) {
    if (key.type !== want && !(want === "private" && key.type === "private")) {
      const err = new TypeError("KeyObject type mismatch");
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    return key;
  }
  if (format === "jwk") {
    if (typeof key !== "object" || key === null) {
      const err = new TypeError("JWK key must be an object");
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    if (key.kty === "oct") return new KeyObject("secret", "secret", __b64urlDec(key.k));
    if (key.kty === "RSA") {
      const n = __b64urlDec(key.n), e = __b64urlDec(key.e);
      if (key.d !== undefined) {
        const d = __b64urlDec(key.d);
        const privDer = __cryptCall(() => __wjs_rsa_import_priv(n, e, d));
        const k = new KeyObject("private", "rsa", Buffer.from(privDer));
        return k;
      }
      const pubDer = __cryptCall(() => __wjs_rsa_import_pub(n, e));
      return new KeyObject("public", "rsa", Buffer.from(pubDer));
    }
    if (key.kty === "EC") {
      const curve = __normCurve(key.crv);
      const x = __b64urlDec(key.x), y = __b64urlDec(key.y);
      if (key.d !== undefined) {
        const privDer = __cryptCall(() => __wjs_ec_import_priv(curve, __b64urlDec(key.d)));
        const k = new KeyObject("private", "ec", Buffer.from(privDer));
        k.__detail = { namedCurve: curve };
        return k;
      }
      const pubDer = __cryptCall(() => __wjs_ec_import_pub(curve, x, y));
      const k = new KeyObject("public", "ec", Buffer.from(pubDer));
      k.__detail = { namedCurve: curve };
      return k;
    }
    if (key.kty === "DSA") {
      // JWK → 信封（b64url 转标准 b64 存；长度定档）。
      const std = (s) => Buffer.from(__b64urlDec(s)).toString("base64");
      for (const f of ["p", "q", "g", "y"]) {
        if (typeof key[f] !== "string") {
          const err = new TypeError(`Invalid DSA JWK (missing ${f})`);
          err.code = "ERR_INVALID_ARG_VALUE";
          throw err;
        }
      }
      const env = { p: std(key.p), q: std(key.q), g: std(key.g), y: std(key.y) };
      if (key.x !== undefined) env.x = std(key.x);
      // 信封合法性经轮子校验（坐标对参数）。
      __cryptCall(() => __wjs_dsa_export(JSON.stringify(env)));
      const k = new KeyObject(key.x !== undefined ? "private" : "public", "dsa", Buffer.from(JSON.stringify(env)));
      const pLen = Buffer.from(env.p, "base64").length, qLen = Buffer.from(env.q, "base64").length;
      k.__detail = { modulusLength: pLen * 8, divisorLength: qLen * 8 };
      return k;
    }
    if (key.kty === "OKP") {
      const kt = key.crv === "Ed25519" ? "ed25519" : key.crv === "X25519" ? "x25519" : null;
      if (kt === null) {
        const err = new Error(`Unsupported OKP curve ${key.crv}`);
        err.code = "ERR_NOT_SUPPORTED";
        throw err;
      }
      if (key.d !== undefined) {
        const k = new KeyObject("private", kt, __b64urlDec(key.d));
        return k;
      }
      return new KeyObject("public", kt, __b64urlDec(key.x));
    }
    const err = new TypeError("Unsupported JWK kty");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  let der;
  if (typeof key === "string") {
    const pem = __pemDecode(key);
    if (!pem) {
      const err = new TypeError("PEM decode failed");
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    der = pem.der;
    if (pem.label === "PRIVATE KEY") type = type ?? "pkcs8";
    else if (pem.label === "PUBLIC KEY") type = type ?? "spki";
    else if (pem.label === "RSA PRIVATE KEY") type = type ?? "pkcs1";
    else if (pem.label === "RSA PUBLIC KEY") type = type ?? "pkcs1-pub";
    else if (pem.label === "EC PRIVATE KEY") type = type ?? "sec1";
  } else {
    der = __cryptBytes(key, "key");
  }
  type = type ?? (want === "private" ? "pkcs8" : "spki");
  if (type === "pkcs8") {
    // 以 RSA/EC/OKP 逐一试解（DER 自描述不足，顺序即优先级；失败信息统一）
    const tries = [
      ["rsa", () => { __cryptCall(() => __wjs_rsa_public(der)); return new KeyObject("private", "rsa", der); }],
      ["ec", () => {
        // SPKI 算法 OID 直判（试解靠坐标长度会把 secp256k1 误判成 P-256，同 32 字节）。
        const g = __cryptCall(() => __wjs_ec_guess_curve(der));
        if (g === "") throw new Error("no");
        __cryptCall(() => __wjs_ec_public(g, der));
        const k = new KeyObject("private", "ec", der);
        k.__detail = { namedCurve: g };
        return k;
      }],
      ["dsa", () => {
        const env = __parseDsaDer(der, "private");
        __cryptCall(() => __wjs_dsa_export(JSON.stringify(env)));
        return __dsaKeyObject(env, "private");
      }],
      ["okp", () => {
        for (const kt of ["ed25519", "x25519"]) {
          try {
            const seed = __cryptCall(() => __wjs_okp_seed_from_pkcs8(kt === "ed25519" ? "ED25519" : "X25519", der));
            return new KeyObject("private", kt, Buffer.from(seed));
          } catch {}
        }
        throw new Error("no");
      }],
      ["ml-kem", () => {
        // 9i-4：种子形 PKCS#8（LAMPS 口径，[0] 64B 种子；展开即校验）。
        const parts = JSON.parse(__cryptCall(() => __wjs_mlkem_seed_from_pkcs8(der)));
        return new KeyObject("private", parts.kind, der);
      }],
      ["ml-dsa", () => {
        // 9i-6：种子形 PKCS#8（[0] 32B 种子；展开即校验）。
        const parts = JSON.parse(__cryptCall(() => __wjs_mldsa_seed_from_pkcs8(der)));
        return new KeyObject("private", parts.kind, der);
      }],
    ];
    for (const [, fn] of tries) {
      try { return fn(); } catch (e) { if (e && e.code && e.code !== "ERR_NOT_SUPPORTED") throw e; }
    }
    const err = new TypeError("Invalid PKCS#8 key");
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
  if (type === "spki") {
    const tries = [
      () => { __cryptCall(() => __wjs_rsa_jwk_pub(der)); return new KeyObject("public", "rsa", der); },
      () => {
        const g = __cryptCall(() => __wjs_ec_guess_curve(der));
        if (g === "") throw new Error("no");
        __cryptCall(() => __wjs_ec_jwk_pub(g, der));
        const k = new KeyObject("public", "ec", der);
        k.__detail = { namedCurve: g };
        return k;
      },
      () => {
        const env = __parseDsaDer(der, "public");
        __cryptCall(() => __wjs_dsa_export(JSON.stringify(env)));
        return __dsaKeyObject(env, "public");
      },
      () => {
        for (const kt of ["ed25519", "x25519"]) {
          try {
            const pub = __cryptCall(() => __wjs_okp_pub_from_spki(kt === "ed25519" ? "ED25519" : "X25519", der));
            return new KeyObject("public", kt, Buffer.from(pub));
          } catch {}
        }
        throw new Error("no");
      },
      () => {
        const kind = __cryptCall(() => __wjs_mlkem_kind_from_spki(der));
        if (kind === "") throw new Error("no");
        return new KeyObject("public", kind, der);
      },
      () => {
        const kind = __cryptCall(() => __wjs_mldsa_kind_from_spki(der));
        if (kind === "") throw new Error("no");
        return new KeyObject("public", kind, der);
      },
    ];
    for (const fn of tries) {
      try { return fn(); } catch (e) { if (e && e.code && e.code !== "ERR_NOT_SUPPORTED") throw e; }
    }
    const err = new TypeError("Invalid SPKI key");
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
  if (type === "sec1") {
    // SEC1 EC 私钥：SEQ{ INTEGER 1, OCTET scalar, [0] curveOID?, [1] pub BITSTRING? }
    const top = __derChildren(__derRead(der, 0).body);
    const scalar = top.find((t) => t.tag === 4);
    if (!scalar) {
      const err = new TypeError("Invalid SEC1 key");
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    // [0] 显式曲线 OID 直判（无 OID 才试解；32 字节标量试解无法区分 P-256/secp256k1，记档）。
    const curveOid = top.find((t) => t.tag === 160);
    const oidMap = {
      "2a8648ce3d030107": "P-256", "2b81040022": "P-384",
      "2b81040023": "P-521", "2b8104000a": "secp256k1",
    };
    let curves = ["P-256", "P-384", "P-521", "secp256k1"];
    if (curveOid && curveOid.body.length >= 2 && curveOid.body[0] === 6) {
      // 显式标签内为完整 OID TLV（06 len bytes），剥掉再比。
      let inner = curveOid.body.slice(2);
      if (curveOid.body[1] >= 128) inner = curveOid.body.slice(3);
      const hit = oidMap[Buffer.from(inner).toString("hex")];
      if (hit) curves = [hit];
    }
    for (const c of curves) {
      try {
        const privDer = __cryptCall(() => __wjs_ec_import_priv(c, scalar.body));
        const k = new KeyObject("private", "ec", Buffer.from(privDer));
        k.__detail = { namedCurve: c };
        return k;
      } catch {}
    }
    const err = new TypeError("Invalid SEC1 key");
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
  const err = new TypeError(`Unsupported key type ${type} (der sec1/pkcs8/spki supported)`);
  err.code = "ERR_INVALID_ARG_VALUE";
  throw err;
}
export function createPrivateKey(key) {
  if (typeof key === "object" && key !== null && !(key instanceof Uint8Array) && !(key instanceof ArrayBuffer) && !ArrayBuffer.isView(key) && !(key instanceof KeyObject)) {
    return __parseKeyMaterial(key.key ?? key, key.format, key.type, "private");
  }
  return __parseKeyMaterial(key, undefined, undefined, "private");
}
export function createPublicKey(key) {
  if (typeof key === "object" && key !== null && !(key instanceof Uint8Array) && !(key instanceof ArrayBuffer) && !ArrayBuffer.isView(key) && !(key instanceof KeyObject)) {
    // 允许从私钥对象派生公钥（Node 同款）
    if (key.key instanceof KeyObject && key.key.type === "private") {
      return __derivePublic(key.key);
    }
    return __parseKeyMaterial(key.key ?? key, key.format, key.type, "public");
  }
  if (key instanceof KeyObject && key.type === "private") return __derivePublic(key);
  return __parseKeyMaterial(key, undefined, undefined, "public");
}
function __derivePublic(priv) {
  if (priv.__keyType === "rsa" || priv.__keyType === "rsa-pss") {
    return new KeyObject("public", priv.__keyType, Buffer.from(__cryptCall(() => __wjs_rsa_public(priv.__material))));
  }
  if (priv.__keyType === "ec") {
    const c = priv.__detail.namedCurve;
    return Object.assign(new KeyObject("public", "ec", Buffer.from(__cryptCall(() => __wjs_ec_public(c, priv.__material)))), { __detail: { namedCurve: c } });
  }
  if (priv.__keyType === "ed25519") {
    return new KeyObject("public", "ed25519", Buffer.from(__cryptCall(() => __wjs_ed_public(priv.__material))));
  }
  if (priv.__keyType === "x25519") {
    return new KeyObject("public", "x25519", Buffer.from(__cryptCall(() => __wjs_x_public(priv.__material))));
  }
  if (priv.__keyType === "dsa") {
    const env = JSON.parse(Buffer.from(priv.__material).toString("utf8"));
    const pubEnv = { p: env.p, q: env.q, g: env.g, y: env.y };
    const k = new KeyObject("public", "dsa", Buffer.from(JSON.stringify(pubEnv)));
    k.__detail = priv.__detail;
    return k;
  }
  if (typeof priv.__keyType === "string" && priv.__keyType.startsWith("ml-kem-")) {
    const parts = JSON.parse(__cryptCall(() => __wjs_mlkem_seed_from_pkcs8(priv.__material)));
    return new KeyObject("public", priv.__keyType, Buffer.from(__b64dec(parts.spki)));
  }
  if (typeof priv.__keyType === "string" && priv.__keyType.startsWith("ml-dsa-")) {
    const spki = __cryptCall(() => __wjs_mldsa_public(priv.__material));
    return new KeyObject("public", priv.__keyType, Buffer.from(spki));
  }
  const err = new Error("Cannot derive public key for this key type");
  err.code = "ERR_NOT_SUPPORTED";
  throw err;
}
function __genPairSync(type, options) {
  options = options ?? {};
  if (type === "rsa" || type === "rsa-pss") {
    const bits = options.modulusLength ?? 2048;
    if (![2048, 3072, 4096].includes(bits)) {
      const err = new Error("RSA modulusLength must be 2048/3072/4096");
      err.code = "ERR_NOT_SUPPORTED";
      throw err;
    }
    let e = options.publicExponent ?? 65537;
    if (e instanceof Uint8Array) {
      let n = 0;
      for (const b of e) n = n * 256 + b;
      e = n;
    }
    const privDer = __cryptCall(() => __wjs_rsa_generate(bits, Number(e)));
    const pubDer = __cryptCall(() => __wjs_rsa_public(privDer));
    const kt = type === "rsa-pss" ? "rsa-pss" : "rsa";
    const priv = new KeyObject("private", kt, Buffer.from(privDer));
    const pub = new KeyObject("public", kt, Buffer.from(pubDer));
    if (type === "rsa-pss") {
      priv.__detail = { hash: options.hash ?? "sha256", saltLength: options.saltLength };
      pub.__detail = priv.__detail;
    }
    return { privateKey: priv, publicKey: pub };
  }
  if (type === "ec") {
    const curve = __normCurve(options.namedCurve);
    if (curve === "Ed25519" || curve === "X25519") {
      const err = new Error(`Use '${curve.toLowerCase()}' key type for OKP`);
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    const privDer = __cryptCall(() => __wjs_ec_generate(curve));
    const pubDer = __cryptCall(() => __wjs_ec_public(curve, privDer));
    const priv = new KeyObject("private", "ec", Buffer.from(privDer));
    priv.__detail = { namedCurve: curve };
    const pub = new KeyObject("public", "ec", Buffer.from(pubDer));
    pub.__detail = { namedCurve: curve };
    return { privateKey: priv, publicKey: pub };
  }
  if (type === "ed25519" || type === "x25519") {
    const isEd = type === "ed25519";
    const seed = __cryptCall(() => isEd ? __wjs_ed_generate() : __wjs_x_generate());
    const pubB = __cryptCall(() => isEd ? __wjs_ed_public(seed) : __wjs_x_public(seed));
    return {
      privateKey: new KeyObject("private", type, Buffer.from(seed)),
      publicKey: new KeyObject("public", type, Buffer.from(pubB)),
    };
  }
  if (type === "dsa") {
    // Node 缺省：divisorLength 按 modulus 取（1024→160，其余→256；2048/224 须显式）。
    let modulusLength = options.modulusLength ?? 2048;
    let divisorLength = options.divisorLength;
    if (divisorLength === undefined) divisorLength = modulusLength === 1024 ? 160 : 256;
    const env = JSON.parse(__cryptCall(() => __wjs_dsa_generate(modulusLength, divisorLength)));
    const priv = new KeyObject("private", "dsa", Buffer.from(JSON.stringify(env)));
    priv.__detail = { modulusLength, divisorLength };
    const pubEnv = { p: env.p, q: env.q, g: env.g, y: env.y };
    const pub = new KeyObject("public", "dsa", Buffer.from(JSON.stringify(pubEnv)));
    pub.__detail = priv.__detail;
    return { privateKey: priv, publicKey: pub };
  }
  if (type === "ml-kem-512" || type === "ml-kem-768" || type === "ml-kem-1024") {
    // 9i-4：FIPS 203 PQ KEM（真机 generateKey 不收 ml-kem，此处 generateKeyPair 专属）。
    const parts = JSON.parse(__cryptCall(() => __wjs_mlkem_gen(type)));
    return {
      privateKey: new KeyObject("private", type, Buffer.from(__b64dec(parts.pkcs8))),
      publicKey: new KeyObject("public", type, Buffer.from(__b64dec(parts.spki))),
    };
  }
  if (type === "ml-dsa-44" || type === "ml-dsa-65" || type === "ml-dsa-87") {
    // 9i-6：FIPS 204 PQ 签名（纯签名，hash=null）。
    const parts = JSON.parse(__cryptCall(() => __wjs_mldsa_gen(type)));
    return {
      privateKey: new KeyObject("private", type, Buffer.from(__b64dec(parts.pkcs8))),
      publicKey: new KeyObject("public", type, Buffer.from(__b64dec(parts.spki))),
    };
  }
  const err = new Error(`generateKeyPair type '${type}' not supported (rsa/rsa-pss/ec/ed25519/x25519/dsa/ml-kem-512/768/1024/ml-dsa-44/65/87)`);
  err.code = "ERR_NOT_SUPPORTED";
  throw err;
}
function __applyEncoding(pair, publicEncoding, privateEncoding) {
  const out = {};
  if (publicEncoding !== undefined) {
    out.publicKey = pair.publicKey.export(publicEncoding);
  } else out.publicKey = pair.publicKey;
  if (privateEncoding !== undefined) {
    out.privateKey = pair.privateKey.export(privateEncoding);
  } else out.privateKey = pair.privateKey;
  return out;
}
export function generateKeyPairSync(type, options, publicEncoding, privateEncoding) {
  if (typeof options === "string" || options === undefined) options = {};
  return __applyEncoding(__genPairSync(type, options), publicEncoding, privateEncoding);
}
export function generateKeyPair(type, options, ...rest) {
  let cb = rest.find((a) => typeof a === "function");
  let pubEnc, privEnc;
  if (typeof options === "object" && options !== null) {
    pubEnc = options.publicKeyEncoding;
    privEnc = options.privateKeyEncoding;
  }
  if (!cb) {
    if (typeof rest[0] === "function") cb = rest[0];
  }
  if (typeof cb !== "function") {
    const err = new TypeError("generateKeyPair requires a callback for async form (use Sync variant otherwise)");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  queueMicrotask(() => {
    try {
      cb(null, __applyEncoding(__genPairSync(type, options ?? {}), pubEnc, privEnc).publicKey,
        __applyEncoding(__genPairSync(type, options ?? {}), pubEnc, privEnc).privateKey);
    } catch (e) {
      cb(e);
    }
  });
}
// DER 编解码（ECDSA der 签名用）
function __derLen(n) {
  if (n < 128) return new Uint8Array([n]);
  const bytes = [];
  while (n > 0) { bytes.unshift(n & 255); n = Math.floor(n / 256); }
  return new Uint8Array([128 | bytes.length, ...bytes]);
}
function __derInt(raw) {
  let v = raw;
  while (v.length > 1 && v[0] === 0) v = v.slice(1);
  const neg = (v[0] & 128) !== 0;
  const body = neg ? new Uint8Array([0, ...v]) : v;
  return new Uint8Array([2, ...__derLen(body.length), ...body]);
}
function __rawToDerSig(raw) {
  const half = raw.length / 2;
  const seq = new Uint8Array([...__derInt(raw.slice(0, half)), ...__derInt(raw.slice(half))]);
  return new Uint8Array([48, ...__derLen(seq.length), ...seq]);
}
function __derToRawSig(der, size) {
  const top = __derRead(der, 0);
  if (top.tag !== 48) throw new Error("bad signature");
  const kids = __derChildren(top.body);
  if (kids.length !== 2 || kids[0].tag !== 2 || kids[1].tag !== 2) throw new Error("bad signature");
  const norm = (t) => {
    let v = t.body;
    while (v.length > 1 && v[0] === 0) v = v.slice(1);
    if (v.length > size) throw new Error("bad signature");
    const out = new Uint8Array(size);
    out.set(v, size - v.length);
    return out;
  };
  const out = new Uint8Array(size * 2);
  out.set(norm(kids[0]), 0);
  out.set(norm(kids[1]), size);
  return out;
}
function __signCore(alg, data, keyObj, dsaEncoding, saltLength) {
  const dataB = __cryptBytes(data, "data");
  const kt = keyObj.__keyType;
  if (kt === "ed25519") {
    // EdDSA 无摘要算法（真 Node：非 null 即 ERR_OSSL_INVALID_DIGEST；
    // 此处用 CRYPTO 扁平码，Sign 类同口径，记档）
    if (alg !== null && alg !== undefined) {
      const err = new Error("Invalid digest");
      err.code = "ERR_CRYPTO_INVALID_DIGEST";
      throw err;
    }
    if (keyObj.__kind !== "private") {
      const err = new TypeError("sign requires a private key");
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    return __cryptCall(() => __wjs_ed_sign(keyObj.__material, dataB));
  }
  if (typeof kt === "string" && kt.startsWith("ml-dsa-")) {
    // 9i-6：纯签名（真机口径：非 null 即 ERR_OSSL_INVALID_DIGEST）。
    if (alg !== null && alg !== undefined) {
      const err = new Error("Invalid digest");
      err.code = "ERR_OSSL_INVALID_DIGEST";
      throw err;
    }
    if (keyObj.__kind !== "private") {
      const err = new TypeError("sign requires a private key");
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    return __cryptCall(() => __wjs_mldsa_sign(keyObj.__material, dataB));
  }
  const hash = __normHashName(alg);
  if (hash === undefined) {
    const err = new Error("Invalid digest");
    err.code = "ERR_CRYPTO_INVALID_DIGEST";
    throw err;
  }
  if (kt === "rsa" || kt === "rsa-pss") {
    if (keyObj.__kind !== "private") {
      const err = new TypeError("sign requires a private key");
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    // SHA-1/MD5 走手写 v1.5（digest 0.10 版本面）；SHA-2 走既有 natives
    if (hash === "SHA-1" || hash === "MD5") {
      if (kt === "rsa-pss") {
        const err = new Error("RSA-PSS with SHA-1/MD5 not supported");
        err.code = "ERR_NOT_SUPPORTED";
        throw err;
      }
      return __cryptCall(() => __wjs_node_rsa_v15_sign(keyObj.__material, dataB, hash));
    }
    const pss = kt === "rsa-pss";
    if (pss) {
      const defSalt = { "SHA-256": 32, "SHA-384": 48, "SHA-512": 64 }[hash] ?? 32;
      const salt = saltLength === undefined ? defSalt : Number(saltLength);
      return __cryptCall(() => __wjs_pss_sign(hash, salt, keyObj.__material, dataB));
    }
    return __cryptCall(() => __wjs_rsa_sign(hash, keyObj.__material, dataB));
  }
  if (kt === "ec") {
    if (keyObj.__kind !== "private") {
      const err = new TypeError("sign requires a private key");
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    const curve = keyObj.__detail.namedCurve;
    const raw = __cryptCall(() => __wjs_ecdsa_sign(curve, hash, keyObj.__material, dataB));
    if ((dsaEncoding ?? "der") === "der") return __rawToDerSig(Buffer.from(raw));
    return Buffer.from(raw);
  }
  if (kt === "dsa") {
    if (keyObj.__kind !== "private") {
      const err = new TypeError("sign requires a private key");
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    const envStr = Buffer.from(keyObj.__material).toString("utf8");
    const der = __cryptCall(() => __wjs_dsa_sign(hash, envStr, dataB));
    if ((dsaEncoding ?? "der") === "der") return Buffer.from(der);
    // ieee-p1363：r‖s 定长（q 长）；DER 解后拼。
    const qLen = Buffer.from(JSON.parse(envStr).q, "base64").length;
    return Buffer.from(__derToRawSig(Buffer.from(der), qLen));
  }
  const err = new Error(`sign not supported for ${kt}`);
  err.code = "ERR_NOT_SUPPORTED";
  throw err;
}
function __verifyCore(alg, data, keyObj, sig, dsaEncoding, saltLength) {
  const dataB = __cryptBytes(data, "data");
  const sigB = __cryptBytes(sig, "signature");
  const kt = keyObj.__keyType;
  if (kt === "ed25519") {
    if (alg !== null && alg !== undefined) {
      const err = new Error("Invalid digest");
      err.code = "ERR_CRYPTO_INVALID_DIGEST";
      throw err;
    }
    if (keyObj.__kind === "private") {
      const err = new TypeError("verify requires a public key");
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    return __cryptCall(() => __wjs_ed_verify(keyObj.__material, sigB, dataB));
  }
  if (typeof kt === "string" && kt.startsWith("ml-dsa-")) {
    // 9i-6：纯签名（真机口径同 sign：非 null 即 ERR_OSSL_INVALID_DIGEST）。
    if (alg !== null && alg !== undefined) {
      const err = new Error("Invalid digest");
      err.code = "ERR_OSSL_INVALID_DIGEST";
      throw err;
    }
    if (keyObj.__kind === "private") {
      const err = new TypeError("verify requires a public key");
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    return __cryptCall(() => __wjs_mldsa_verify(keyObj.__material, sigB, dataB));
  }
  const hash = __normHashName(alg);
  if (hash === undefined) {
    const err = new Error("Invalid digest");
    err.code = "ERR_CRYPTO_INVALID_DIGEST";
    throw err;
  }
  // 公钥派生（私钥亦可验，Node 同款）
  const pubDer = keyObj.__kind === "private" ? __derivePublic(keyObj).__material : keyObj.__material;
  if (kt === "rsa" || kt === "rsa-pss") {
    if (hash === "SHA-1" || hash === "MD5") {
      if (kt === "rsa-pss") {
        const err = new Error("RSA-PSS with SHA-1/MD5 not supported");
        err.code = "ERR_NOT_SUPPORTED";
        throw err;
      }
      return __cryptCall(() => __wjs_node_rsa_v15_verify(pubDer, sigB, dataB, hash));
    }
    const pss = kt === "rsa-pss";
    if (pss) {
      const defSalt = { "SHA-256": 32, "SHA-384": 48, "SHA-512": 64 }[hash] ?? 32;
      const salt = saltLength === undefined ? defSalt : Number(saltLength);
      return __cryptCall(() => __wjs_pss_verify(hash, salt, pubDer, sigB, dataB));
    }
    return __cryptCall(() => __wjs_rsa_verify(hash, pubDer, sigB, dataB));
  }
  if (kt === "ec") {
    const curve = keyObj.__detail.namedCurve;
    const size = __curveSize(curve);
    const raw = (dsaEncoding ?? "der") === "der" ? __derToRawSig(sigB, size) : sigB;
    return __cryptCall(() => __wjs_ecdsa_verify(curve, hash, pubDer, raw, dataB));
  }
  if (kt === "dsa") {
    const envStr = Buffer.from(keyObj.__material).toString("utf8");
    const der = (dsaEncoding ?? "der") === "der" ? sigB : __rawToDerSig(sigB);
    return __cryptCall(() => __wjs_dsa_verify(hash, envStr, der, dataB));
  }
  const err = new Error(`verify not supported for ${kt}`);
  err.code = "ERR_NOT_SUPPORTED";
  throw err;
}
function __keyArg(key, what) {
  if (key instanceof KeyObject) return key;
  if (typeof key === "string" || key instanceof Uint8Array || key instanceof ArrayBuffer || ArrayBuffer.isView(key)) {
    try { return createPrivateKey(key); } catch { return createPublicKey(key); }
  }
  if (typeof key === "object" && key !== null) {
    const inner = key.key ?? key;
    if (inner instanceof KeyObject) return inner;
    try { return createPrivateKey(key); } catch { return createPublicKey(key); }
  }
  const err = new TypeError(`The "${what}" argument must be a KeyObject or key material`);
  err.code = "ERR_INVALID_ARG_TYPE";
  throw err;
}
export function sign(alg, data, key) {
  const k = __keyArg(key, "key");
  const opts = (typeof key === "object" && key !== null && !(key instanceof Uint8Array) && !(key instanceof KeyObject)) ? key : {};
  const out = __signCore(alg, data, k, opts.dsaEncoding, opts.saltLength);
  return Buffer.from(out);
}
export function verify(alg, data, key, signature) {
  const k = __keyArg(key, "key");
  const opts = (typeof key === "object" && key !== null && !(key instanceof Uint8Array) && !(key instanceof KeyObject)) ? key : {};
  return __verifyCore(alg, data, k, signature, opts.dsaEncoding, opts.saltLength);
}
class Sign {
  constructor(alg, options) {
    if (alg !== null && __normHashName(alg) === undefined && alg !== "ed25519") {
      const err = new Error("Invalid digest");
      err.code = "ERR_CRYPTO_INVALID_DIGEST";
      throw err;
    }
    if (alg === "ed25519") {
      const err = new Error("Invalid digest");
      err.code = "ERR_CRYPTO_INVALID_DIGEST";
      throw err;
    }
    this.__alg = alg;
    this.__parts = [];
  }
  update(data, enc) {
    this.__parts.push(__cryptBytes(data, "data", enc));
    return this;
  }
  sign(key, ...rest) {
    let dsaEncoding, saltLength;
    for (const r of rest) {
      if (typeof r === "string") dsaEncoding = r;
      else if (typeof r === "number") saltLength = r;
      else if (r && typeof r === "object") { dsaEncoding = r.dsaEncoding ?? dsaEncoding; saltLength = r.saltLength ?? saltLength; }
    }
    const flat = __joinParts(this.__parts);
    if (typeof key === "object" && key !== null && !(key instanceof Uint8Array) && !(key instanceof KeyObject) && !ArrayBuffer.isView(key)) {
      const inner = key.key ?? key;
      const k = __keyArg(inner, "key");
      const out = __signCore(this.__alg, flat, k, key.dsaEncoding ?? dsaEncoding, key.saltLength ?? saltLength);
      return Buffer.from(out);
    }
    const out = __signCore(this.__alg, flat, __keyArg(key, "key"), dsaEncoding, saltLength);
    return Buffer.from(out);
  }
}
class Verify {
  constructor(alg, options) {
    if (alg !== null && __normHashName(alg) === undefined && alg !== "ed25519") {
      const err = new Error("Invalid digest");
      err.code = "ERR_CRYPTO_INVALID_DIGEST";
      throw err;
    }
    if (alg === "ed25519") {
      const err = new Error("Invalid digest");
      err.code = "ERR_CRYPTO_INVALID_DIGEST";
      throw err;
    }
    this.__alg = alg;
    this.__parts = [];
  }
  update(data, enc) {
    this.__parts.push(__cryptBytes(data, "data", enc));
    return this;
  }
  verify(key, signature, ...rest) {
    let dsaEncoding, saltLength;
    for (const r of rest) {
      if (typeof r === "string") dsaEncoding = r;
      else if (typeof r === "number") saltLength = r;
      else if (r && typeof r === "object") { dsaEncoding = r.dsaEncoding ?? dsaEncoding; saltLength = r.saltLength ?? saltLength; }
    }
    const flat = __joinParts(this.__parts);
    if (typeof key === "object" && key !== null && !(key instanceof Uint8Array) && !(key instanceof KeyObject) && !ArrayBuffer.isView(key)) {
      const inner = key.key ?? key;
      const k = __keyArg(inner, "key");
      return __verifyCore(this.__alg, flat, k, signature, key.dsaEncoding ?? dsaEncoding, key.saltLength ?? saltLength);
    }
    return __verifyCore(this.__alg, flat, __keyArg(key, "key"), signature, dsaEncoding, saltLength);
  }
}
export function createSign(alg, options) { return new Sign(alg, options); }
export function createVerify(alg, options) { return new Verify(alg, options); }
function __rsaCrypt(key, data, isPublic, isEncrypt) {
  const k = __keyArg(key, "key");
  const opts = (typeof key === "object" && key !== null && !(key instanceof Uint8Array) && !(key instanceof KeyObject)) ? key : {};
  const padding = opts.padding ?? 4;
  const dataB = __cryptBytes(data, "data");
  if (padding === 4) {
    const hashFlat = opts.oaepHash ?? "sha1";
    const hash = __normHashName(hashFlat);
    if (hash === undefined || hash === "MD5") {
      const err = new Error(`Unsupported OAEP hash ${opts.oaepHash}`);
      err.code = "ERR_NOT_SUPPORTED";
      throw err;
    }
    const label = opts.oaepLabel !== undefined ? __cryptBytes(opts.oaepLabel, "label") : null;
    // SHA-1 走手写 OAEP（digest 0.10 版本面，见 Rust 侧记）；SHA-2 走既有 natives
    if (isPublic && isEncrypt) {
      if (hash === "SHA-1") {
        return Buffer.from(__cryptCall(() => __wjs_node_rsa_oaep(k.__material, dataB, label, 1)));
      }
      return Buffer.from(__cryptCall(() => __wjs_rsa_encrypt(hash, k.__material, dataB, label)));
    }
    if (!isPublic && !isEncrypt) {
      if (hash === "SHA-1") {
        return Buffer.from(__cryptCall(() => __wjs_node_rsa_oaep(k.__material, dataB, label, 0)));
      }
      return Buffer.from(__cryptCall(() => __wjs_rsa_decrypt(hash, k.__material, dataB, label)));
    }
  }
  if (padding === 1) {
    if (isPublic && isEncrypt) {
      return Buffer.from(__cryptCall(() => __wjs_rsa_encrypt_v15(k.__material, dataB)));
    }
    if (!isPublic && !isEncrypt) {
      return Buffer.from(__cryptCall(() => __wjs_rsa_decrypt_v15(k.__material, dataB)));
    }
    if (!isPublic && isEncrypt) {
      // 私钥加密（签名式填充）：本仓无 RSA 私钥加密 native，经 sign 原语不适用；
      // 此处明确不支持（Node 允许，记档缺口）
      const err = new Error("RSA privateEncrypt not supported");
      err.code = "ERR_NOT_SUPPORTED";
      throw err;
    }
    if (isPublic && !isEncrypt) {
      const err = new Error("RSA publicDecrypt not supported");
      err.code = "ERR_NOT_SUPPORTED";
      throw err;
    }
  }
  const err = new Error(`Unsupported RSA padding ${padding} for this operation`);
  err.code = "ERR_NOT_SUPPORTED";
  throw err;
}
export function publicEncrypt(key, data) { return __rsaCrypt(key, data, true, true); }
export function privateDecrypt(key, data) { return __rsaCrypt(key, data, false, false); }
export function privateEncrypt(key, data) { return __rsaCrypt(key, data, false, true); }
export function publicDecrypt(key, data) { return __rsaCrypt(key, data, true, false); }

class ECDH {
  constructor(curve) {
    this.__curve = __normCurve(curve);
    if (this.__curve !== "P-256" && this.__curve !== "P-384" && this.__curve !== "P-521" && this.__curve !== "secp256k1") {
      const err = new Error(`ECDH curve ${curve} not supported (P-256/384/521/secp256k1)`);
      err.code = "ERR_NOT_SUPPORTED";
      throw err;
    }
    this.__priv = null;
    this.__pub = null;
  }
  generateKeys() {
    this.__priv = __cryptCall(() => __wjs_ec_generate(this.__curve));
    this.__pub = __cryptCall(() => __wjs_ec_public(this.__curve, this.__priv));
    return this.getPublicKey();
  }
  getPublicKey(encoding, format) {
    if (this.__pub === null) {
      const err = new Error("ECDH keys not generated");
      err.code = "ERR_CRYPTO_INVALID_STATE";
      throw err;
    }
    const parts = JSON.parse(__cryptCall(() => __wjs_ec_jwk_pub(this.__curve, this.__pub)));
    const x = __b64urlDec(parts.x), y = __b64urlDec(parts.y);
    const raw = new Uint8Array(1 + x.length + y.length);
    raw[0] = 4; raw.set(x, 1); raw.set(y, 1 + x.length);
    if (format === "der" || format === "pem") return this.__pubToDer(format);
    if (encoding === undefined) return Buffer.from(raw);
    return Buffer.from(raw).toString(encoding);
  }
  __pubToDer(format) {
    if (format === "der") return Buffer.from(this.__pub);
    return __pemEncode("PUBLIC KEY", this.__pub);
  }
  getPrivateKey(encoding) {
    if (this.__priv === null) {
      const err = new Error("ECDH keys not generated");
      err.code = "ERR_CRYPTO_INVALID_STATE";
      throw err;
    }
    const size = __curveSize(this.__curve);
    const parts = JSON.parse(__cryptCall(() => __wjs_ec_jwk(this.__curve, this.__priv, this.__pub)));
    const d = __b64urlDec(parts.d);
    const out = new Uint8Array(size);
    out.set(d, size - d.length);
    if (encoding === undefined) return Buffer.from(out);
    return Buffer.from(out).toString(encoding);
  }
  setPrivateKey(priv) {
    const scalar = __cryptBytes(priv, "private key");
    this.__priv = __cryptCall(() => __wjs_ec_import_priv(this.__curve, scalar));
    this.__pub = __cryptCall(() => __wjs_ec_public(this.__curve, this.__priv));
    return this;
  }
  computeSecret(peer, inputEncoding, outputEncoding) {
    if (this.__priv === null) {
      const err = new Error("ECDH keys not generated");
      err.code = "ERR_CRYPTO_INVALID_STATE";
      throw err;
    }
    let peerB = (typeof peer === "string") ? __cryptBytes(peer, "peer", inputEncoding) : __cryptBytes(peer, "peer");
    let peerDer;
    if (peerB.length > 0 && peerB[0] === 4) {
      // 裸非压缩点 → 经 import 转 SPKI
      const size = (peerB.length - 1) / 2;
      peerDer = __cryptCall(() => __wjs_ec_import_pub(this.__curve, peerB.slice(1, 1 + size), peerB.slice(1 + size)));
    } else {
      peerDer = peerB;
    }
    const secret = __cryptCall(() => __wjs_ecdh_derive(this.__curve, this.__priv, peerDer));
    if (outputEncoding === undefined) return Buffer.from(secret);
    return Buffer.from(secret).toString(outputEncoding);
  }
}
export function createECDH(curve, format) { return new ECDH(curve); }

class DiffieHellman {
  constructor(prime, generator) {
    if (typeof prime === "number") {
      const err = new Error("DH numeric size form needs parameter generation (use group or explicit prime)");
      err.code = "ERR_NOT_SUPPORTED";
      throw err;
    }
    const primeB = (typeof prime === "string") ? __cryptBytes(prime, "prime", "hex") : __cryptBytes(prime, "prime");
    this.__prime = primeB;
    this.__gen = generator === undefined ? 2 : Number(generator);
    this.__priv = null;
    this.__pub = null;
    this.__verifyError = 0;
  }
  static group(name) {
    const hex = __DH_GROUPS[String(name).toLowerCase()];
    if (hex === undefined) {
      const err = new Error(`Unknown DH group ${name} (modp5/14/15/16)`);
      err.code = "ERR_NOT_SUPPORTED";
      throw err;
    }
    return new DiffieHellman(__cryptBytes(hex, "prime", "hex"), 2);
  }
  generateKeys() {
    const r = JSON.parse(__cryptCall(() => __wjs_dh_genkey(this.__prime, this.__gen, this.__prime.length)));
    this.__priv = __b64dec(r.priv);
    this.__pub = __b64dec(r.pub);
    return this.getPublicKey();
  }
  getPublicKey(encoding) {
    if (this.__pub === null) {
      const err = new Error("DH keys not generated");
      err.code = "ERR_CRYPTO_INVALID_STATE";
      throw err;
    }
    if (encoding === undefined) return Buffer.from(this.__pub);
    return Buffer.from(this.__pub).toString(encoding);
  }
  getPrivateKey(encoding) {
    if (this.__priv === null) {
      const err = new Error("DH keys not generated");
      err.code = "ERR_CRYPTO_INVALID_STATE";
      throw err;
    }
    if (encoding === undefined) return Buffer.from(this.__priv);
    return Buffer.from(this.__priv).toString(encoding);
  }
  getPrime(encoding) {
    if (encoding === undefined) return Buffer.from(this.__prime);
    return Buffer.from(this.__prime).toString(encoding);
  }
  getGenerator(encoding) {
    const g = new Uint8Array([this.__gen & 255]);
    if (encoding === undefined) return Buffer.from(g);
    return Buffer.from(g).toString(encoding);
  }
  setPublicKey(pub) { this.__pub = __cryptBytes(pub, "public key"); return this; }
  setPrivateKey(priv) { this.__priv = __cryptBytes(priv, "private key"); return this; }
  computeSecret(peer, inEnc, outEnc) {
    if (this.__priv === null) {
      const err = new Error("DH keys not generated");
      err.code = "ERR_CRYPTO_INVALID_STATE";
      throw err;
    }
    const peerB = (typeof peer === "string") ? __cryptBytes(peer, "peer", inEnc) : __cryptBytes(peer, "peer");
    const secret = __cryptCall(() => __wjs_dh_secret(this.__prime, this.__priv, peerB));
    if (outEnc === undefined) return Buffer.from(secret);
    return Buffer.from(secret).toString(outEnc);
  }
  verifyError() { return this.__verifyError; }
}
export function createDiffieHellman(prime, generator) {
  if (typeof prime === "string" && __DH_GROUPS[prime.toLowerCase()] !== undefined && generator === undefined) {
    return DiffieHellman.group(prime);
  }
  return new DiffieHellman(prime, generator);
}
export function createDiffieHellmanGroup(name) { return DiffieHellman.group(name); }
export function getDiffieHellman(name) { return DiffieHellman.group(name); }
export function diffieHellman(options) {
  const priv = options?.privateKey;
  const pub = options?.publicKey;
  const dh = (priv instanceof DiffieHellman) ? priv : null;
  if (dh !== null && pub instanceof DiffieHellman) {
    return dh.computeSecret(pub.getPublicKey());
  }
  if (priv instanceof KeyObject && pub instanceof KeyObject) {
    if (priv.__keyType === "x25519") {
      const out = __cryptCall(() => __wjs_x_derive(priv.__material, pub.__material));
      return Buffer.from(out);
    }
    if (priv.__keyType === "ec") {
      const curve = priv.__detail.namedCurve;
      const pubDer = pub.__kind === "private" ? __derivePublic(pub).__material : pub.__material;
      const out = __cryptCall(() => __wjs_ecdh_derive(curve, priv.__material, pubDer));
      return Buffer.from(out);
    }
    const err = new Error("diffieHellman needs DH/ECDH/X25519 keys");
    err.code = "ERR_NOT_SUPPORTED";
    throw err;
  }
  const err = new TypeError("diffieHellman needs { privateKey, publicKey }");
  err.code = "ERR_INVALID_ARG_TYPE";
  throw err;
}
function __bigintToBytes(v) {
  if (typeof v === "bigint") {
    let hex = v.toString(16);
    if (hex.length % 2) hex = "0" + hex;
    return __cryptBytes(hex, "candidate", "hex");
  }
  if (typeof v === "number") {
    if (!Number.isSafeInteger(v) || v < 0) {
      const err = new TypeError("candidate must be a non-negative safe integer or Buffer");
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    let hex = v.toString(16);
    if (hex.length % 2) hex = "0" + hex;
    return __cryptBytes(hex, "candidate", "hex");
  }
  return __cryptBytes(v, "candidate");
}
export function checkPrimeSync(candidate, options) {
  const bytes = __bigintToBytes(candidate);
  const checks = options?.checks ?? 64;
  return __cryptCall(() => __wjs_prime_check(bytes, checks));
}
export function checkPrime(candidate, options, callback) {
  if (typeof options === "function") { callback = options; options = undefined; }
  if (typeof callback !== "function") {
    const err = new TypeError("checkPrime requires a callback for async form");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  queueMicrotask(() => {
    try {
      callback(null, checkPrimeSync(candidate, options));
    } catch (e) {
      callback(e);
    }
  });
}
export function generatePrimeSync(size, options) {
  const bits = Number(size);
  if (!Number.isInteger(bits)) {
    const err = new TypeError("size must be an integer");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const out = __cryptCall(() => __wjs_prime_gen(bits, options?.checks ?? 64, options?.safe ? 1 : 0));
  // bigint 经 16 进制桥（`BigInt("0x…")`，零 native 改动；§4.44 同类绕行）。
  if (options?.bigint === true) return BigInt("0x" + Buffer.from(out).toString("hex"));
  return Buffer.from(out);
}
export function generatePrime(size, options, callback) {
  if (typeof options === "function") { callback = options; options = undefined; }
  if (typeof callback !== "function") {
    const err = new TypeError("generatePrime requires a callback for async form");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  queueMicrotask(() => {
    try {
      callback(null, generatePrimeSync(size, options));
    } catch (e) {
      callback(e);
    }
  });
}
export function generateKey(options, ...rest) {
  const cb = rest.find((a) => typeof a === "function");
  if (typeof cb !== "function") {
    const err = new TypeError("generateKey requires a callback for async form");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  queueMicrotask(() => {
    try {
      const len = options?.length ?? 32;
      cb(null, createSecretKey(randomBytes(len)));
    } catch (e) {
      cb(e);
    }
  });
}
export function generateKeySync(options) {
  const len = options?.length ?? 32;
  if (!Number.isInteger(len) || len <= 0) {
    const err = new TypeError("generateKey length must be a positive integer");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  return createSecretKey(randomBytes(len));
}
export const constants = {
  RSA_PKCS1_PADDING: 1, RSA_SSLV23_PADDING: 2, RSA_NO_PADDING: 3,
  RSA_PKCS1_OAEP_PADDING: 4, RSA_X931_PADDING: 5, RSA_PKCS1_PSS_PADDING: 6,
  RSA_PSS_SALTLEN_DIGEST: -1, RSA_PSS_SALTLEN_MAX_SIGN: -2, RSA_PSS_SALTLEN_AUTO: -2,
  RSA_PSS_SALTLEN_AUTO_DIGEST_MAX: -2,
  POINT_CONVERSION_COMPRESSED: 2, POINT_CONVERSION_UNCOMPRESSED: 4, POINT_CONVERSION_HYBRID: 6,
  DH_CHECK_P_NOT_PRIME: 2, DH_CHECK_P_NOT_SAFE_PRIME: 4,
  DH_UNABLE_TO_CHECK_GENERATOR: 8, DH_NOT_SUITABLE_GENERATOR: 16,
  DH_CHECK_Q_NOT_PRIME: 1, DH_CHECK_INVALID_Q_VALUE: 32, DH_CHECK_INVALID_J_VALUE: 64,
  defaultCipherList: "ECDHE+AESGCM:ECDHE+CHACHA20",
};
export function getFips() { return 0; }
export function setFips() { return undefined; }
export function setEngine() { return undefined; }
export function secureHeapUsed() { return { total: 0, min: 0, max: 0, used: 0 }; }

// ── 9e-1d KDF + X509 ──────────────────────────────────────────────────────

const __KDF_HASHES = ["sha1", "sha256", "sha384", "sha512", "md5"];
function __kdfHash(digest) {
  if (typeof digest !== "string") {
    const err = new TypeError(`The "digest" argument must be of type string. Received type ${typeof digest}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const flat = digest.trim().toLowerCase().replace(/[-_]/g, "");
  const table = { "sha1": "SHA-1", "sha256": "SHA-256", "sha384": "SHA-384", "sha512": "SHA-512", "md5": "MD5" };
  const norm = table[flat];
  if (norm === undefined) {
    const err = new Error(`Invalid digest: ${digest}`);
    err.code = "ERR_CRYPTO_INVALID_DIGEST";
    throw err;
  }
  return norm;
}
export function pbkdf2Sync(password, salt, iterations, keylen, digest) {
  const it = Number(iterations);
  if (!Number.isInteger(it) || it < 1 || it > 2147483647) {
    const err = new RangeError(`The value of "iterations" is out of range. It must be >= 1 && <= 2147483647. Received ${iterations}`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  const out = __cryptCall(() => __wjs_kdf_pbkdf2(
    __kdfHash(digest), __cryptBytes(password, "password"), __cryptBytes(salt, "salt"), it, Number(keylen)));
  return Buffer.from(out);
}
export function pbkdf2(password, salt, iterations, keylen, digest, callback) {
  if (typeof callback !== "function") {
    const err = new TypeError("pbkdf2 requires a callback for async form");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  queueMicrotask(() => {
    try {
      callback(null, pbkdf2Sync(password, salt, iterations, keylen, digest));
    } catch (e) {
      callback(e);
    }
  });
}
const __SCRYPT_DEFAULTS = { N: 16384, r: 8, p: 1, maxmem: 33554432 };
function __scryptArgs(password, salt, keylen, options) {
  const o = options ?? {};
  const N = o.N ?? __SCRYPT_DEFAULTS.N;
  const r = o.r ?? __SCRYPT_DEFAULTS.r;
  const p = o.p ?? __SCRYPT_DEFAULTS.p;
  const maxmem = o.maxmem ?? __SCRYPT_DEFAULTS.maxmem;
  return [__cryptBytes(password, "password"), __cryptBytes(salt, "salt"),
    Number(keylen), Number(N), Number(r), Number(p), Number(maxmem)];
}
export function scryptSync(password, salt, keylen, options) {
  const [pw, sa, kl, N, r, p, maxmem] = __scryptArgs(password, salt, keylen, options);
  const out = __cryptCall(() => __wjs_kdf_scrypt(pw, sa, N, r, p, kl, maxmem));
  return Buffer.from(out);
}
export function scrypt(password, salt, keylen, options, callback) {
  if (typeof options === "function") { callback = options; options = undefined; }
  if (typeof callback !== "function") {
    const err = new TypeError("scrypt requires a callback for async form");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  queueMicrotask(() => {
    try {
      callback(null, scryptSync(password, salt, keylen, options));
    } catch (e) {
      callback(e);
    }
  });
}
export function hkdfSync(hash, ikm, salt, info, keylen) {
  const out = __cryptCall(() => __wjs_kdf_hkdf(
    __kdfHash(hash), __cryptBytes(ikm, "ikm"),
    salt === undefined || salt === null ? new Uint8Array(0) : __cryptBytes(salt, "salt"),
    info === undefined || info === null ? new Uint8Array(0) : __cryptBytes(info, "info"),
    Number(keylen)));
  // Node 回 ArrayBuffer（非 Buffer），同口径
  return Buffer.from(out).buffer;
}
export function hkdf(hash, ikm, salt, info, keylen, callback) {
  if (typeof callback !== "function") {
    const err = new TypeError("hkdf requires a callback for async form");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  queueMicrotask(() => {
    try {
      callback(null, hkdfSync(hash, ikm, salt, info, keylen));
    } catch (e) {
      callback(e);
    }
  });
}
function __argon2Args(algorithm, parameters) {
  if (typeof algorithm !== "string" || !["argon2d", "argon2i", "argon2id"].includes(algorithm)) {
    const err = new TypeError(`The argument 'algorithm' must be one of: 'argon2d', 'argon2i', 'argon2id'. Received '${algorithm}'`);
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
  if (typeof parameters !== "object" || parameters === null) {
    const err = new TypeError("The \"parameters\" argument must be of type object");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const needView = (v, name) => {
    if (typeof v !== "string" && !(v instanceof Uint8Array) && !(v instanceof ArrayBuffer) && !ArrayBuffer.isView(v)) {
      const err = new TypeError(`The "parameters.${name}" property must be of type string or an instance of ArrayBuffer, Buffer, TypedArray, or DataView.`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    return __cryptBytes(v, name);
  };
  const message = needView(parameters.message, "message");
  const nonce = needView(parameters.nonce, "nonce");
  if (nonce.length < 8) {
    const err = new RangeError("parameters.nonce must have byteLength >= 8");
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  const intArg = (v, name, min, max) => {
    const n = Number(v);
    if (!Number.isInteger(n) || n < min || n > max) {
      const err = new RangeError(`The value of "parameters.${name}" is out of range. It must be >= ${min} && <= ${max}.`);
      err.code = "ERR_OUT_OF_RANGE";
      throw err;
    }
    return n;
  };
  const parallelism = intArg(parameters.parallelism, "parallelism", 1, 16777215);
  const tagLength = intArg(parameters.tagLength, "tagLength", 4, 4294967295);
  const memory = intArg(parameters.memory, "memory", 8 * parallelism, 4294967295);
  const passes = intArg(parameters.passes, "passes", 0, 4294967295);
  const secret = parameters.secret === undefined ? new Uint8Array(0) : needView(parameters.secret, "secret");
  const ad = parameters.associatedData === undefined ? new Uint8Array(0) : needView(parameters.associatedData, "associatedData");
  return [algorithm, message, nonce, secret, ad, parallelism, tagLength, memory, passes];
}
export function argon2Sync(algorithm, parameters) {
  const a = __argon2Args(algorithm, parameters);
  const out = __cryptCall(() => __wjs_kdf_argon2(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8]));
  return Buffer.from(out);
}
export function argon2(algorithm, parameters, callback) {
  if (typeof callback !== "function") {
    const err = new TypeError("argon2 requires a callback for async form");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  queueMicrotask(() => {
    try {
      callback(null, argon2Sync(algorithm, parameters));
    } catch (e) {
      callback(e);
    }
  });
}

// ── 9i-4 封装面（KEM；真机口径：encapsulate 收公/私 KeyObject，decapsulate 仅私）──

const __MLKEM_KINDS = ["ml-kem-512", "ml-kem-768", "ml-kem-1024"];
export function encapsulate(key, ...rest) {
  // 真机第二参为异步回调形（本仓不做，给了即报 ERR_INVALID_ARG_TYPE）。
  if (rest.length > 0) {
    const err = new TypeError('The "callback" argument must be of type function');
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (key instanceof KeyObject && __MLKEM_KINDS.includes(key.__keyType)) {
    const isPriv = key.type === "private" ? 1 : 0;
    const r = JSON.parse(__cryptCall(() => __wjs_mlkem_encaps(key.__material, isPriv)));
    return { sharedKey: Buffer.from(__b64dec(r.sk)), ciphertext: Buffer.from(__b64dec(r.ct)) };
  }
  const err = new Error("unsupported key for encapsulation");
  err.code = "ERR_OSSL_UNSUPPORTED";
  throw err;
}
export function decapsulate(key, ciphertext) {
  if (!(key instanceof KeyObject)) {
    const err = new Error("unsupported key for decapsulation");
    err.code = "ERR_OSSL_UNSUPPORTED";
    throw err;
  }
  if (key.type !== "private") {
    const err = new TypeError("Invalid key object type public, expected private.");
    err.code = "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE";
    throw err;
  }
  if (!__MLKEM_KINDS.includes(key.__keyType)) {
    // 真机：非 ml-kem 私钥 → 无码 Error。
    throw new Error("Decapsulation failed");
  }
  const out = __cryptCall(() => __wjs_mlkem_decaps(key.__material, __cryptBytes(ciphertext, "ciphertext")));
  return Buffer.from(out);
}

class X509Certificate {
  constructor(pemOrDer) {
    let der;
    if (typeof pemOrDer === "string") {
      const pem = __pemDecode(pemOrDer);
      if (!pem || pem.label !== "CERTIFICATE") {
        const err = new TypeError("X509 needs a CERTIFICATE PEM or DER");
        err.code = "ERR_INVALID_ARG_VALUE";
        throw err;
      }
      der = pem.der;
    } else {
      der = __cryptBytes(pemOrDer, "cert");
    }
    this.__der = Buffer.from(der);
    this.__info = JSON.parse(__cryptCall(() => __wjs_x509_parse(der)));
  }
  get subject() { return this.__info.subject; }
  get issuer() { return this.__info.issuer; }
  get subjectAltName() { return this.__info.subjectAltName; }
  get infoAccess() { return undefined; }
  get serialNumber() { return this.__info.serialNumber; }
  get validFrom() { return this.__info.validFrom; }
  get validTo() { return this.__info.validTo; }
  get fingerprint() { return this.__info.fingerprint; }
  get fingerprint256() { return this.__info.fingerprint256; }
  get fingerprint512() { return this.__info.fingerprint512; }
  get keyUsage() { return this.__info.keyUsage; }
  get extKeyUsage() { return this.__info.extKeyUsage; }
  get raw() { return Buffer.from(this.__der); }
  get publicKey() {
    // 9i-3：由 SPKI DER 重建公钥 KeyObject（createPublicKey 试解链 rsa/ec/dsa/okp/ml-kem）
    return createPublicKey({ key: Buffer.from(this.__info.spkiB64, "base64"), format: "der", type: "spki" });
  }
  get ca() { return this.__info.ca === true; }
  toString() { return __pemEncode("CERTIFICATE", this.__der); }
  toJSON() { return this.toLegacyObject(); }
  toLegacyObject() {
    return {
      subject: this.__info.subjectObj,
      issuer: this.__info.issuerObj,
      subjectaltname: this.__info.subjectAltName,
      infoAccess: undefined,
      serialNumber: this.__info.serialNumber,
      validFrom: this.__info.validFrom,
      validTo: this.__info.validTo,
    };
  }
  verify(publicKey) {
    // 9i-3 真机口径：无参/非 KeyObject → ERR_INVALID_ARG_TYPE；私钥 → ERR_INVALID_ARG_VALUE；
    // 错钥/异族/不支持算法 → false 不抛。
    if (!(publicKey instanceof KeyObject)) {
      const err = new TypeError(`The "publicKey" argument must be an instance of KeyObject. Received ${publicKey === null ? "null" : typeof publicKey}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    if (publicKey.type !== "public") {
      const err = new TypeError(`Key type must be public for X509Certificate.verify. Received ${publicKey.type}`);
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    const kt = publicKey.__keyType === "rsa-pss" ? "rsa" : publicKey.__keyType;
    return __cryptCall(() => __wjs_x509_verify(this.__der, Buffer.from(publicKey.__material), kt)) === true;
  }
  checkHost(name) { return __x509Match(name, this.__info.sanDns, this.__info.sanIp, this.__info.subjectObj?.CN); }
  checkIssued(otherCert) {
    // 9i-7 真机口径：非 X509Certificate → ERR_INVALID_ARG_TYPE（noarg 同码）。
    if (!(otherCert instanceof X509Certificate)) {
      const err = new TypeError(`The "otherCert" argument must be an instance of X509Certificate. Received ${otherCert === null ? "null" : typeof otherCert}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    return __cryptCall(() => __wjs_x509_check_issued(this.__der, otherCert.__der)) === true;
  }
  checkPrivateKey(privateKey) {
    // 9i-7 真机口径：非 KeyObject → ERR_INVALID_ARG_TYPE；公钥 → ERR_INVALID_ARG_VALUE。
    if (!(privateKey instanceof KeyObject)) {
      const err = new TypeError(`The "privateKey" argument must be an instance of KeyObject. Received ${privateKey === null ? "null" : typeof privateKey}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    if (privateKey.type !== "private") {
      const err = new TypeError(`Key type must be private for X509Certificate.checkPrivateKey. Received ${privateKey.type}`);
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    // 派生公钥后与证书 SPKI 逐字节比（ed25519/x25519 material 是裸 32B，手工包 SPKI）。
    const pub = createPublicKey(privateKey);
    let spki;
    if (pub.__keyType === "ed25519" || pub.__keyType === "x25519") {
      const oidHex = pub.__keyType === "ed25519" ? "2b6570" : "2b656e";
      spki = Buffer.concat([Buffer.from(`302a30050603${oidHex}032100`, "hex"), Buffer.from(pub.__material)]);
    } else if (pub.__keyType === "dsa") {
      const env = JSON.parse(Buffer.from(pub.__material).toString("utf8"));
      const parts = JSON.parse(__cryptCall(() => __wjs_dsa_export(JSON.stringify(env))));
      spki = Buffer.from(__b64dec(parts.pubDer));
    } else {
      spki = Buffer.from(pub.__material);
    }
    return Buffer.compare(spki, Buffer.from(this.__info.spkiB64, "base64")) === 0;
  }
  checkEmail(email) {
    if (this.__info.sanEmail.length > 0) {
      return this.__info.sanEmail.includes(String(email)) ? String(email) : undefined;
    }
    return undefined;
  }
  checkIP(ip) {
    return this.__info.sanIp.includes(String(ip)) ? String(ip) : undefined;
  }
}
function __x509Match(name, dns, ips, cn) {
  name = String(name);
  // IP 字面量走 SAN-iP 精确匹配
  if (/^[0-9a-fA-F:.]+$/.test(name) && (name.includes(":") || /^\d+\.\d+\.\d+\.\d+$/.test(name))) {
    return ips.includes(name) ? name : undefined;
  }
  const lower = name.toLowerCase();
  for (const pattern of dns) {
    if (__dnsMatch(lower, pattern.toLowerCase())) return pattern;
  }
  if (cn && __dnsMatch(lower, String(cn).toLowerCase())) return cn;
  return undefined;
}
function __dnsMatch(host, pattern) {
  if (!pattern.includes("*")) return host === pattern;
  // 单标签通配（RFC 6125 口径子集）
  if (!pattern.startsWith("*.")) return false;
  const suffix = pattern.slice(2);
  if (!host.endsWith(suffix) || host.length <= suffix.length) return false;
  const left = host.slice(0, host.length - suffix.length);
  return left.length > 0 && !left.includes(".");
}
export { X509Certificate };
export class Certificate {
  constructor() {
    const err = new Error("legacy Certificate/SPKAC not supported");
    err.code = "ERR_NOT_SUPPORTED";
    throw err;
  }
}

const __api = {
  createHash, createHmac, Hash, Hmac, hash,
  randomBytes, randomFill, randomFillSync, randomInt, randomUUID, randomUUIDv7,
  timingSafeEqual, getHashes, getCurves, webcrypto, getRandomValues,
  createCipheriv, createDecipheriv, Cipheriv, Decipheriv, getCiphers, getCipherInfo,
  KeyObject, createSecretKey, createPrivateKey, createPublicKey,
  generateKeyPair, generateKeyPairSync, generateKey, generateKeySync,
  createSign, createVerify, sign, verify,
  publicEncrypt, privateDecrypt, privateEncrypt, publicDecrypt,
  createECDH, ECDH, createDiffieHellman, createDiffieHellmanGroup, getDiffieHellman,
  DiffieHellman, diffieHellman, checkPrime, checkPrimeSync, generatePrime, generatePrimeSync,
  constants, getFips, setFips, setEngine, secureHeapUsed,
  pbkdf2, pbkdf2Sync, scrypt, scryptSync, hkdf, hkdfSync,
  argon2, argon2Sync, X509Certificate, Certificate,
  encapsulate, decapsulate,
};
export default __api;
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crypto_hash_norm_table() {
        assert!(norm_hash("sha256", 0).is_some());
        assert!(norm_hash("SHA256", 0).is_some());
        assert!(norm_hash("sha-256", 0).is_some());
        assert!(norm_hash("RSA-SHA256", 0).is_some());
        assert!(norm_hash("sha3-256", 0).is_some());
        assert!(norm_hash("blake2b512", 0).is_some());
        assert!(norm_hash("blake2s256", 0).is_some());
        assert!(norm_hash("md5", 0).is_some());
        // 9h-2 落地：ripemd160 + SHAKE（长度注册时带）。
        assert!(norm_hash("ripemd160", 0).is_some());
        assert!(norm_hash("shake128", 16).is_some());
        assert!(norm_hash("shake256", 32).is_some());
        assert!(norm_hash("nope", 0).is_none());
        assert!(norm_hash("", 0).is_none());
    }

    #[test]
    fn crypto_hash_known_vectors() {        use sha2::Digest as _; // 全员 digest 0.11 系（sha1/sha2/md5/sha3/blake2 同 trait，无需直引 digest，见 §0.5 零新增）
        assert_eq!(
            const_hex::encode(sha2::Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            const_hex::encode(md5::Md5::digest(b"abc")),
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            const_hex::encode(sha3::Sha3_256::digest(b"abc")),
            "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532"
        );
        // 9h-2：ripemd160 + SHAKE（真机向量，见 9h-2 黑盒）。
        assert_eq!(
            const_hex::encode(ripemd::Ripemd160::digest(b"abc")),
            "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc"
        );
        {
            use tiny_keccak::Hasher as _;
            let mut h = tiny_keccak::Shake::v128();
            h.update(b"abc");
            let mut out = [0u8; 32];
            h.finalize(&mut out);
            assert_eq!(
                const_hex::encode(out),
                "5881092dd818bf5cf8a3ddb793fbcba74097d5c526a6d35f97b83351940f2cc8"
            );
            let mut h = tiny_keccak::Shake::v256();
            h.update(b"abc");
            h.finalize(&mut out);
            assert_eq!(
                const_hex::encode(out),
                "483366601360a8771c6863080cc4114d8db44530f8f1e1ee4f94ea37e78b5739"
            );
        }
    }

    #[test]
    fn crypto_cipher_params_table() {
        assert_eq!(cipher_params("aes-256-cbc"), Some(("cbc-aes256", 32, 16, 16)));
        assert_eq!(cipher_params("AES-256-GCM"), None); // AEAD 不走流式注册表
        assert_eq!(cipher_params("aes-128-ctr"), Some(("ctr-aes128", 16, 16, 16)));
        assert_eq!(cipher_params("des-ede3-cbc"), Some(("cbc-des3", 24, 8, 8)));
        assert!(cipher_params("aes-999-cbc").is_none());
        assert!(cipher_params("").is_none());
    }

    #[test]
    fn crypto_pkcs7_roundtrip() {
        assert_eq!(pkcs7_pad(16, vec![]), vec![16u8; 16]);
        assert_eq!(pkcs7_pad(16, vec![1, 2, 3]).len(), 16);
        assert_eq!(pkcs7_unpad(16, &pkcs7_pad(16, b"hello".to_vec())), Some(b"hello".to_vec()));
        assert!(pkcs7_unpad(16, &[1, 2, 3]).is_none());
        assert!(pkcs7_unpad(16, &[]).is_none());
        assert!(pkcs7_unpad(16, &[16u8; 15]).is_none());
    }

    #[test]
    fn crypto_cbc_known_vector() {        // 真 Node 取证：aes-256-cbc(key=01×32, iv=02×16, "hello world")
        use aes::cipher::KeyIvInit as _;
        use aes::cipher::block::BlockModeEncrypt as _;
        let key = aes::cipher::Key::<aes::Aes256>::from_slice(&[1u8; 32]);
        let iv = aes::cipher::Block::<aes::Aes256>::from_slice(&[2u8; 16]);
        let mut enc = cbc::Encryptor::<aes::Aes256>::new(key, iv);
        let mut blocks = to_blocks::<aes::Aes256>(&pkcs7_pad(16, b"hello world".to_vec()));
        enc.encrypt_blocks(&mut blocks);
        assert_eq!(
            const_hex::encode(from_blocks::<aes::Aes256>(&blocks)),
            "f563737a376afbed282274255a7fcabd"
        );
    }

    #[test]
    fn crypto_miller_rabin_small() {
        for p in [2u32, 3, 5, 7, 11, 13, 7919, 104729] {
            assert!(is_prime(&rsa::BigUint::from(p), 8), "{p} should be prime");
        }
        for n in [0u32, 1, 4, 9, 15, 21, 25, 27, 561, 2047] {
            assert!(!is_prime(&rsa::BigUint::from(n), 8), "{n} should be composite");
        }
    }

    #[test]
    fn crypto_mgf1_sha1_vector() {
        // MGF1-SHA1("test", 20) = SHA1("test" ‖ 0x00000000)
        assert_eq!(
            const_hex::encode(mgf1_sha1(b"test", 20)),
            "b67344dc7dea343795faaba3bc4d4508bf6766b1"
        );
        assert_eq!(mgf1_sha1(b"test", 24).len(), 24);
        assert_eq!(
            const_hex::encode(mgf1_sha1(b"test", 24)[20..].to_vec()),
            "b45ef443"
        );
    }

    #[test]
    fn crypto_pad_be_shapes() {
        assert_eq!(pad_be(&[1, 2], 4), vec![0, 0, 1, 2]);
        assert_eq!(pad_be(&[1, 2, 3, 4, 5], 3), vec![3, 4, 5]);
    }

    #[test]
    fn crypto_asn1_time_shapes() {
        assert_eq!(fmt_asn1_time(0), "Jan  1 00:00:00 1970 GMT");
        // 2026-09-12T07:03:16Z（真机证书有效期同形）
        assert_eq!(fmt_asn1_time(1789196596), "Sep 12 07:03:16 2026 GMT");
    }

    #[test]
    fn crypto_atv_short_table() {
        assert_eq!(atv_short("2.5.4.3"), "CN");
        assert_eq!(atv_short("2.5.4.10"), "O");
        assert_eq!(atv_short("1.2.840.113549.1.9.1"), "emailAddress");
        assert_eq!(atv_short("1.2.3.4"), "1.2.3.4");
    }

    // ── 9i-4 ml-kem ────────────────────────────────────────────────────────

    #[test]
    fn mlkem_tables_and_tlv() {
        assert_eq!(mlkem_params("ml-kem-512").map(|p| (p.1, p.2)), Some((800, 768)));
        assert_eq!(mlkem_params("ml-kem-768").map(|p| (p.1, p.2)), Some((1184, 1088)));
        assert_eq!(mlkem_params("ml-kem-1024").map(|p| (p.1, p.2)), Some((1568, 1568)));
        assert!(mlkem_params("ml-kem").is_none());
        assert!(mlkem_params("nope").is_none());
        for kind in ["ml-kem-512", "ml-kem-768", "ml-kem-1024"] {
            let (oid, _, _) = mlkem_params(kind).unwrap();
            assert_eq!(mlkem_kind_by_oid(oid), Some(kind));
        }
        assert!(mlkem_kind_by_oid(&[0u8; 9]).is_none());
        // 长形长度（800 = 0x0320 → 0x82 双字节形）
        let t = mlkem_tlv(0x04, &vec![0u8; 800]);
        assert_eq!(&t[..4], &[0x04, 0x82, 0x03, 0x20]);
        assert_eq!(t.len(), 804);
        let s = mlkem_tlv(0x04, &[1, 2, 3]);
        assert_eq!(s, vec![0x04, 0x03, 1, 2, 3]);
    }

    #[test]
    fn mlkem_wrap_parse_roundtrip() {
        let seed = [7u8; 64];
        let (oid, _, _) = mlkem_params("ml-kem-768").unwrap();
        // PKCS#8：总长 86（真机同款），种子逐字节还原；[0] 标签破坏即 None。
        let pkcs8 = mlkem_pkcs8(oid, &seed);
        assert_eq!(pkcs8.len(), 86);
        let (oid2, parsed) = mlkem_pkcs8_seed(&pkcs8, 64).unwrap();
        assert_eq!(oid2, oid);
        assert_eq!(parsed, &seed[..]);
        let mut bad = pkcs8.clone();
        bad[20] = 0x81; // 内层 [0] → 0x81（上下文构造形），结构不符
        assert!(mlkem_pkcs8_seed(&bad, 64).is_none());
        assert!(mlkem_pkcs8_seed(&pkcs8[..40], 64).is_none());
        // SPKI：768 档总长 1206（真机同款），ek 原样还原；未用位非零即 None。
        let ek = vec![3u8; 1184];
        let spki = mlkem_spki(oid, &ek);
        assert_eq!(spki.len(), 1206);
        let (oid3, ek2) = mlkem_spki_ek(&spki).unwrap();
        assert_eq!(oid3, oid);
        assert_eq!(ek2, &ek[..]);
        let mut bad2 = spki.clone();
        bad2[21] = 1; // BIT STRING 未用位
        assert!(mlkem_spki_ek(&bad2).is_none());
        assert!(mlkem_spki_ek(&[0x04, 0x00]).is_none());
    }

    #[test]
    fn x509_names_raw_walk() {
        // 手搭 TBS：[0]版本 + serial + sig + issuer(A) + validity + subject(B)，
        // 名字裸 TLV 必须逐字节还原（issuer 取 A、subject 取 B）。
        let name_a: &[u8] = &[0x30, 0x05, 0x0c, 0x03, b'a', b'b', b'c'];
        let name_b: &[u8] = &[0x30, 0x05, 0x0c, 0x03, b'x', b'y', b'z'];
        let utc = |s: &[u8]| -> Vec<u8> {
            let mut t = vec![0x17u8, 0x0d];
            t.extend_from_slice(s);
            t
        };
        let v1 = utc(b"260912000000Z");
        let v2 = utc(b"261012000000Z");
        let mut validity = vec![0x30u8, (v1.len() + v2.len()) as u8];
        validity.extend_from_slice(&v1);
        validity.extend_from_slice(&v2);
        let mut t: Vec<u8> = vec![0xa0, 0x03, 0x02, 0x01, 0x02, 0x02, 0x01, 0x03, 0x30, 0x00];
        t.extend_from_slice(name_a);
        t.extend_from_slice(&validity);
        t.extend_from_slice(name_b);
        let wrap = |content: &[u8]| -> Vec<u8> {
            let mut tbs = vec![0x30u8, 0x81, content.len() as u8];
            tbs.extend_from_slice(content);
            let mut cert = vec![0x30u8, 0x81, tbs.len() as u8];
            cert.extend_from_slice(&tbs);
            cert
        };
        let der = wrap(&t);
        let (issuer, subject) = x509_names_raw(&der).unwrap();
        assert_eq!(issuer, name_a);
        assert_eq!(subject, name_b);
        // 无 [0] 版本头也走通（v1 证书形态）
        let v1_der = wrap(&t[5..]);
        let (issuer2, subject2) = x509_names_raw(&v1_der).unwrap();
        assert_eq!(issuer2, name_a);
        assert_eq!(subject2, name_b);
        // 截断
        assert!(x509_names_raw(&der[..der.len() - 1]).is_none());
    }

    #[test]
    fn mldsa_tables_and_wrap() {
        assert_eq!(mldsa_params("ml-dsa-44").map(|p| (p.1, p.2)), Some((1312, 2420)));
        assert_eq!(mldsa_params("ml-dsa-65").map(|p| (p.1, p.2)), Some((1952, 3309)));
        assert_eq!(mldsa_params("ml-dsa-87").map(|p| (p.1, p.2)), Some((2592, 4627)));
        assert!(mldsa_params("ml-dsa").is_none());
        for kind in ["ml-dsa-44", "ml-dsa-65", "ml-dsa-87"] {
            let (oid, _, _) = mldsa_params(kind).unwrap();
            assert_eq!(mldsa_kind_by_oid(oid), Some(kind));
        }
        // dotted 串往返（X.509 证书签名算法 OID）
        assert_eq!(mldsa_kind_by_oid_str("2.16.840.1.101.3.4.3.17"), Some("ml-dsa-44"));
        assert_eq!(mldsa_kind_by_oid_str("2.16.840.1.101.3.4.3.18"), Some("ml-dsa-65"));
        assert_eq!(mldsa_kind_by_oid_str("2.16.840.1.101.3.4.3.19"), Some("ml-dsa-87"));
        assert_eq!(mldsa_kind_by_oid_str("2.16.840.1.101.3.4.3.99"), None);
        // PKCS#8 54B / SPKI 1974B（65 档，真机同款）；种子逐字节还原
        let seed = [9u8; 32];
        let (pkcs8, spki) = mldsa_expand("ml-dsa-65", &seed).unwrap();
        assert_eq!(pkcs8.len(), 54);
        assert_eq!(spki.len(), 1974);
        let (oid, parsed) = mlkem_pkcs8_seed(&pkcs8, 32).unwrap();
        assert_eq!(mldsa_kind_by_oid(oid), Some("ml-dsa-65"));
        assert_eq!(parsed, &seed[..]);
        // 结构破坏（内层 [0] 标签改写）即 None
        let mut bad = pkcs8.clone();
        bad[20] = 0x81;
        assert!(mlkem_pkcs8_seed(&bad, 32).is_none());
        // SPKI pk 原样还原；未用位非零即 None
        let (_, pk) = mlkem_spki_ek(&spki).unwrap();
        assert_eq!(pk.len(), 1952);
        let mut bad2 = spki.clone();
        bad2[21] = 1;
        assert!(mlkem_spki_ek(&bad2).is_none());
    }

    #[test]
    fn mldsa_sign_verify_core_roundtrip() {
        // 真 crate：from_seed → sign（确定性档）→ verify_core 过；篡改/换消息不过。
        let seed = [0x42u8; 32];
        let (pkcs8, spki) = mldsa_expand("ml-dsa-65", &seed).unwrap();
        let (_, seed_b) = mlkem_pkcs8_seed(&pkcs8, 32).unwrap();
        let mut s = [0u8; 32];
        s.copy_from_slice(seed_b);
        let sig = {
            use ml_dsa::Signer as _;
            ml_dsa::SigningKey::<ml_dsa::MlDsa65>::from_seed(&s.into())
                .sign(b"unit-msg")
                .encode()
                .as_slice()
                .to_vec()
        };
        assert_eq!(sig.len(), 3309);
        let (_, pk) = mlkem_spki_ek(&spki).unwrap();
        assert!(mldsa_verify_core("ml-dsa-65", pk, &sig, b"unit-msg"));
        assert!(!mldsa_verify_core("ml-dsa-65", pk, &sig, b"other-msg"));
        let mut bad = sig.clone();
        bad[500] ^= 0xff;
        assert!(!mldsa_verify_core("ml-dsa-65", pk, &bad, b"unit-msg"));
        // 档位错配（44 公钥验 65 签名）→ false
        let (_, pk44_spki) = mldsa_expand("ml-dsa-44", &seed).unwrap();
        let (_, pk44_raw) = mlkem_spki_ek(&pk44_spki).unwrap();
        assert!(!mldsa_verify_core("ml-dsa-44", pk44_raw, &sig, b"unit-msg"));
    }

    #[test]
    fn mlkem_expand_real_crate() {
        let mut seed = [0u8; 64];
        getrandom::fill(&mut seed).unwrap();
        let (pkcs8, spki) = mlkem_expand("ml-kem-768", &seed).unwrap();
        assert_eq!(pkcs8.len(), 86);
        assert_eq!(spki.len(), 1206);
        let (_, parsed) = mlkem_pkcs8_seed(&pkcs8, 64).unwrap();
        assert_eq!(parsed, &seed[..]);
        let (_, ek) = mlkem_spki_ek(&spki).unwrap();
        assert_eq!(ek.len(), 1184);
        // 三档尺寸表（真机对齐：SPKI 822/1206/1590）
        for (kind, spki_len) in [("ml-kem-512", 822), ("ml-kem-1024", 1590)] {
            let (_, sp) = mlkem_expand(kind, &seed).unwrap();
            assert_eq!(sp.len(), spki_len);
        }
    }
}
