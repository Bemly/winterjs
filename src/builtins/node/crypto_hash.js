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
  // 10f crypto首轮：encoding 先 String() 显式转（用户 toString 抛错须透传，
  // 真机口径）；非法编码串仍回 Buffer（§4.45 记档）。
  const enc = String(encoding);
  // 10f crypto首轮：digest/hmac 的 'buffer' 编码（大小写不敏感）即回 Buffer。
  if (enc.toLowerCase() === "buffer") return b;
  try {
    return b.toString(enc);
  } catch {
    return b;
  }
}
function __needStr(v, what) {
  if (typeof v !== "string") {
    // 10f crypto首轮：null/undefined 的 Received 无 type 前缀（真机逐字）。
    const recv = v === null ? "Received null"
      : v === undefined ? "Received undefined"
      : `Received type ${typeof v} (${String(v)})`;
    const err = new TypeError(`The "${what}" argument must be of type string. ${recv}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  return v;
}

// 10f crypto首轮：outputLength 校验（XOF/非 XOF 共用，真机逐字）。
// 非数 → ARG_TYPE；非整数 → OUT_OF_RANGE（an integer）；越界 → OUT_OF_RANGE（范围）。
function __checkOutputLength(v) {
  if (typeof v !== "number") {
    const recv = v === null ? "null"
      : typeof v === "string" ? `type string ('${v}')`
      : typeof v === "boolean" ? `type boolean (${String(v)})`
      : typeof v === "undefined" ? "undefined"
      : `an instance of ${v.constructor?.name ?? "Object"}`;
    const err = new TypeError(
      `The "options.outputLength" property must be of type number. Received ${recv}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (!Number.isInteger(v)) {
    const err = new RangeError(
      `The value of "options.outputLength" is out of range. It must be an integer. Received ${String(v)}`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  if (v < 0 || v > 4294967295) {
    const err = new RangeError(
      `The value of "options.outputLength" is out of range. It must be >= 0 && <= 4294967295. Received ${String(v)}`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  return v;
}
// 10f crypto首轮：定长摘要字节数（flat 名归一后查；未知回 undefined 走原生报错）。
function __digestSize(flat) {
  const table = {
    "sha1": 20, "dss1": 20, "sha224": 28, "sha256": 32, "sha384": 48, "sha512": 64,
    "md5": 16, "sha3256": 32, "sha3384": 48, "sha3512": 64,
    "blake2b512": 64, "blake2s256": 32, "ripemd160": 20, "ripemd": 20,
  };
  return table[flat];
}
class HashImpl {
  constructor(algorithm, options) {
    __needStr(algorithm, "algorithm");
    const flat = String(algorithm).trim().toLowerCase().replace(/[-_]/g, "");
    const isXof = flat === "shake128" || flat === "shake256";
    // XOF 输出长（真机口径：缺省 shake128→16/shake256→32 + DEP0198 警告）。
    let xofLen = 0;
    if (isXof) {
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
        xofLen = __checkOutputLength(options.outputLength);
      }
    } else {
      // 非 XOF：outputLength 须恰为摘要长，否则 NOT_XOF 错（未知算法跳过，原生报错）。
      const size = __digestSize(flat);
      if (size !== undefined && options?.outputLength !== undefined) {
        const v = __checkOutputLength(options.outputLength);
        if (v !== size) {
          const err = new Error("error:030000B2:digital envelope routines::not XOF or invalid length");
          err.code = "ERR_OSSL_EVP_NOT_XOF_OR_INVALID_LENGTH";
          throw err;
        }
      }
    }
    this.__id = Number(__cryptCall(() => __wjs_crypto_hash_new(algorithm, String(xofLen))));
    this.__finalized = false;
    this.__xof = isXof;
    this.__flat = flat;
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
  copy(options) {
    if (this.__finalized) {
      const err = new Error("Digest already called");
      err.code = "ERR_CRYPTO_HASH_FINALIZED";
      throw err;
    }
    // 10f crypto首轮：copy 可带 outputLength 改长（XOF 经原生 setter；
    // 非 XOF 沿构造口径须恰为摘要长；先验后克隆，不泄漏句柄）。
    let v;
    if (options?.outputLength !== undefined) {
      v = __checkOutputLength(options.outputLength);
      if (!this.__xof && v !== __digestSize(this.__flat)) {
        const err = new Error("error:030000B2:digital envelope routines::not XOF or invalid length");
        err.code = "ERR_OSSL_EVP_NOT_XOF_OR_INVALID_LENGTH";
        throw err;
      }
    }
    const h = Object.create(HashImpl.prototype);
    h.__id = Number(__cryptCall(() => __wjs_crypto_hash_copy(String(this.__id))));
    h.__finalized = false;
    h.__xof = this.__xof;
    h.__flat = this.__flat;
    // XOF：无参 copy 回默认长（真机口径，套件点名）；有参则用给定值。
    if (h.__xof) {
      const dflt = h.__flat === "shake128" ? 16 : 32;
      __cryptCall(() => __wjs_crypto_hash_set_len(String(h.__id), v === undefined ? dflt : v));
    }
    return h;
  }
  // 10f crypto首轮：最小流式鸭子面（write/end/read/readableLength；
  // 真机为 Duplex，此处仅覆盖套件所用的同步形，pipe 等另案）。
  write(chunk, encoding) { this.update(chunk, encoding); return true; }
  end(chunk, encoding) {
    if (this.__finalized) return this;
    if (chunk !== undefined) this.update(chunk, encoding);
    this.__finalized = true;
    const out = __cryptCall(() => __wjs_crypto_hash_digest(String(this.__id)));
    this.__streamOut = __outBuf(out, undefined);
    return this;
  }
  read() {
    const out = this.__streamOut ?? null;
    this.__streamOut = null;
    return out;
  }
  get readableLength() { return this.__streamOut ? this.__streamOut.length : 0; }
}

// 10f crypto首轮：真机 `crypto.Hash(...)` 可无 new 调用（DEP0179 一次性警告）。
let __hashCtorWarned = false;
function Hash(...args) {
  if (!__hashCtorWarned) {
    __hashCtorWarned = true;
    try {
      process.emitWarning("crypto.Hash constructor is deprecated.",
        { type: "DeprecationWarning", code: "DEP0179" });
    } catch {}
  }
  return new HashImpl(...args);
}
Object.setPrototypeOf(Hash, HashImpl);
Hash.prototype = HashImpl.prototype;
Hash.prototype.constructor = Hash;

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

class HmacImpl {
  constructor(hamc, key, options) {
    // 10f crypto首轮：参数名真机为 "hmac"（非 "algorithm"）。
    __needStr(hamc, "hmac");
    // 10f crypto首轮：secret KeyObject 可作 key（裸字节即 material）。
    // 10f crypto二轮：品牌检查（原型伪造即落回 ARG_TYPE，brand-check 套件）。
    if (__isKeyObject(key)) {
      if (key.type !== "secret") {
        const err = new Error(`Invalid key object type ${key.type}, expected secret.`);
        err.code = "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE";
        throw err;
      }
      key = key.__material;
    }
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
      "sha1": "sha1", "dss1": "sha1", "sha224": "sha224",
      "sha256": "sha256", "sha384": "sha384", "sha512": "sha512",
      "md5": "md5", "sha3256": "sha3256", "sha3384": "sha3384", "sha3512": "sha3512",
      "blake2b512": "blake2b512", "blake2s256": "blake2s256",
      "ripemd160": "ripemd160", "ripemd": "ripemd160",
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
  __finishBytes() {
    let total = 0;
    for (const p of this.__parts) total += p.length;
    const flat = new Uint8Array(total);
    let off = 0;
    for (const p of this.__parts) { flat.set(p, off); off += p.length; }
    this.__parts = [];
    return __cryptCall(() => __hmacGeneric(this.__alg, this.__key, flat));
  }
  digest(encoding) {
    if (this.__finalized) {
      // 10f crypto首轮：二次 digest 形态（真机逐字：undefined/'buffer' 精确小写回空
      // Buffer，其余回 ""；Hash 系恒抛，见 HashImpl）。
      return (encoding === undefined || encoding === "buffer") ? Buffer.alloc(0) : "";
    }
    this.__finalized = true;
    const out = this.__finishBytes();
    return __outBuf(out, encoding);
  }
  // 10f crypto首轮：最小流式鸭子面（同 HashImpl 记档）。
  write(chunk, encoding) { this.update(chunk, encoding); return true; }
  end(chunk, encoding) {
    if (this.__finalized) return this;
    if (chunk !== undefined) this.update(chunk, encoding);
    this.__finalized = true;
    this.__streamOut = __outBuf(this.__finishBytes(), undefined);
    return this;
  }
  read() {
    const out = this.__streamOut ?? null;
    this.__streamOut = null;
    return out;
  }
  get readableLength() { return this.__streamOut ? this.__streamOut.length : 0; }
}

// 10f crypto首轮：真机 `crypto.Hmac(...)` 可无 new 调用（DEP0181 一次性警告）。
let __hmacCtorWarned = false;
function Hmac(...args) {
  if (!__hmacCtorWarned) {
    __hmacCtorWarned = true;
    try {
      process.emitWarning("crypto.Hmac constructor is deprecated.",
        { type: "DeprecationWarning", code: "DEP0181" });
    } catch {}
  }
  return new HmacImpl(...args);
}
Object.setPrototypeOf(Hmac, HmacImpl);
Hmac.prototype = HmacImpl.prototype;
Hmac.prototype.constructor = Hmac;

export function createHash(algorithm, options) {
  return new HashImpl(algorithm, options);
}
export function createHmac(hamc, key, options) {
  return new HmacImpl(hamc, key, options);
}
export function hash(algorithm, data, outputEncoding) {
  __needStr(algorithm, "algorithm");
  const bytes = __cryptBytes(data, "data");
  const probe = __cryptCall(() => __wjs_crypto_hash_new(algorithm));
  __cryptCall(() => __wjs_crypto_hash_update(probe, bytes));
  const out = __cryptCall(() => __wjs_crypto_hash_digest(probe));
  if (outputEncoding === undefined) return __outBuf(out, undefined);
  // 10f crypto二轮：'buffer' 编码（大小写不敏感）即回 Buffer（真机口径）。
  if (String(outputEncoding).toLowerCase() === "buffer") return __outBuf(out, undefined);
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
// 10f crypto首轮：randomUUID/v7 的 options 校验（真机 26.8.2 文案逐字）。
function __uuidReceived(v) {
  if (v === null) return "Received null";
  if (v === undefined) return "Received undefined";
  const shown = typeof v === "string" ? `'${v}'` : String(v);
  return `Received type ${typeof v} (${shown})`;
}
function __checkUuidOptions(options) {
  if (options === undefined) return;
  if (typeof options !== "object" || options === null) {
    const err = new TypeError(
      `The "options" argument must be of type object. ${__uuidReceived(options)}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const v = options.disableEntropyCache;
  if (v !== undefined && typeof v !== "boolean") {
    const err = new TypeError(
      `The "options.disableEntropyCache" property must be of type boolean. ${__uuidReceived(v)}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
}
export function randomUUID(options) {
  __checkUuidOptions(options);
  return __wjs_random_uuid();
}
export function randomUUIDv7(options) {
  __checkUuidOptions(options);
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
  // 10f crypto六轮：仅 NIST 四曲线（真机 getCurves 无 OKP 名；自家 ECDH
  // 同样拒 OKP，前后一致。JWK-unsupported-curve 块系能力偏离，见 bun-parity）。
  return ["prime256v1", "secp384r1", "secp521r1", "secp256k1"];
}
export const webcrypto = globalThis.crypto;
// Node 19+ 模块级 getRandomValues（webcrypto 同一实现；vite webSocketToken 用）。
export function getRandomValues(typedArray) {
  return globalThis.crypto.getRandomValues(typedArray);
}

