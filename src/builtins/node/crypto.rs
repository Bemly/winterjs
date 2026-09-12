//! `node:crypto` 9e-1a：Hash 流式 + Hmac/随机/杂项。
//! 复用全局 `__wjs_*`（随机/UUID；零新 `UNSAFE-BOUNDARY`）；HMAC 经通用构造
//! 自架流式 Hash natives（sha3 与 hmac 0.13 的 block-API 不兼容，见修法记）；
//! 新增仅增量 Hash 注册表（`encoding.rs` `STREAM_DECODERS` 同款线程本地表）。
//! 偏差记档（9e-1a）：
//! - 摘要集合 = RustCrypto 已接线：sha1/sha256/sha384/sha512/md5/sha3-256/384/512/
//!   blake2b512/blake2s256。`ripemd160`（无 crate）、XOF（shake/cshake/turboshake/
//!   kangaroo：`Digest` 接口无 XOF 形态）不支持 → `createHash` 报原文
//!   `Digest method not supported`（无 code，Node 同款）；`getHashes` 只列已支持。
//! - 别名：大小写不敏感、`-`/`_` 可选、`RSA-` 前缀可剥（如 `RSA-SHA256`，Node 同款）。
//! - `Hmac` 更新攒 JS 侧（`digest` 时 oneshot），二次 `digest` 回空（Node 同款）；
//!   `digest` 后 `update`/`copy` 报 `ERR_CRYPTO_HASH_FINALIZED`。
//! - 未知输出编码的 `digest(enc)` 回 Buffer（Node 同款宽容）；未知输入编码按 utf8。
//! - 异步形态（`randomBytes(cb)`/`randomInt(cb)`/`randomFill`）经 `queueMicrotask`
//!   派发同步底层（`node:fs` 同款口径）；`getMacs`/`createMac` 真 Node 26 运行时
//!   不存在（仅 `lib/crypto.js` 残留导出），不做；`setEngine/getFips` 等随 9e-1c。

use std::cell::RefCell;
use std::collections::HashMap;

use mozjs::context::JSContext;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{report_error, value_to_string, view_bytes, wrap_cx, Frame};

/// 增量摘要态（RustCrypto `Digest` 全员 `Clone`，`copy()` 语义天然）。
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
}

thread_local! {
    static HASHERS: RefCell<HashMap<u64, HashJob>> = RefCell::new(HashMap::new());
    static HASH_NEXT: RefCell<u64> = RefCell::new(1);
}

/// 算法归一（纯函数，单元测试覆盖）：小写去 `-_`，可剥 `RSA-` 前缀。
fn norm_hash(name: &str) -> Option<HashJob> {
    use sha2::Digest as _; // 全员 digest 0.11 系（sha1/sha2/md5/sha3/blake2 同 trait，无需直引 digest，见 §0.5 零新增）
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
    match norm_hash(&alg) {
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
    use sha2::Digest as _; // 全员 digest 0.11 系（sha1/sha2/md5/sha3/blake2 同 trait，无需直引 digest，见 §0.5 零新增）
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
    use sha2::Digest as _; // 全员 digest 0.11 系（sha1/sha2/md5/sha3/blake2 同 trait，无需直引 digest，见 §0.5 零新增）
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
    this.__id = Number(__cryptCall(() => __wjs_crypto_hash_new(algorithm)));
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
    const table = {
      "sha1": "sha1", "sha256": "sha256", "sha384": "sha384", "sha512": "sha512",
      "md5": "md5", "sha3256": "sha3256", "sha3384": "sha3384", "sha3512": "sha3512",
      "blake2b512": "blake2b512", "blake2s256": "blake2s256",
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
  return ["sha1", "sha256", "sha384", "sha512", "md5", "sha3-256", "sha3-384", "sha3-512", "blake2b512", "blake2s256"];
}
export function getCurves() {
  return ["prime256v1", "secp384r1", "secp521r1", "ed25519", "x25519"];
}
export const webcrypto = globalThis.crypto;

const __api = {
  createHash, createHmac, Hash, Hmac, hash,
  randomBytes, randomFill, randomFillSync, randomInt, randomUUID, randomUUIDv7,
  timingSafeEqual, getHashes, getCurves, webcrypto,
};
export default __api;
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crypto_hash_norm_table() {
        assert!(norm_hash("sha256").is_some());
        assert!(norm_hash("SHA256").is_some());
        assert!(norm_hash("sha-256").is_some());
        assert!(norm_hash("RSA-SHA256").is_some());
        assert!(norm_hash("sha3-256").is_some());
        assert!(norm_hash("blake2b512").is_some());
        assert!(norm_hash("blake2s256").is_some());
        assert!(norm_hash("md5").is_some());
        // 缺口记档：ripemd160 无 crate、shake 系无 XOF 接口
        assert!(norm_hash("ripemd160").is_none());
        assert!(norm_hash("shake128").is_none());
        assert!(norm_hash("nope").is_none());
        assert!(norm_hash("").is_none());
    }

    #[test]
    fn crypto_hash_known_vectors() {
        use sha2::Digest as _; // 全员 digest 0.11 系（sha1/sha2/md5/sha3/blake2 同 trait，无需直引 digest，见 §0.5 零新增）
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
    }
}
