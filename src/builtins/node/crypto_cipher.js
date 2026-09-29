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
  // 10e：AES-CCM 三档（iv 7–13 可变，`__needKeyIv` 另分支；nid 真机 896/899/902）
  "aes-128-ccm": { family: "ccm", key: 16, iv: 12, block: 16, mode: "ccm", nid: 896 },
  "aes-192-ccm": { family: "ccm", key: 24, iv: 12, block: 16, mode: "ccm", nid: 899 },
  "aes-256-ccm": { family: "ccm", key: 32, iv: 12, block: 16, mode: "ccm", nid: 902 },
  "chacha20-poly1305": { family: "chacha", key: 32, iv: 12, block: 16, mode: "chacha20-poly1305", nid: 1018 },
  "des-ede3-cbc": { family: "cbc", key: 24, iv: 8, block: 8, mode: "cbc", nid: 44 },
  // 10f crypto首轮：ECB 三档（无 iv；nid 真机 418/422/426）。
  "aes-128-ecb": { family: "ecb", key: 16, iv: 0, block: 16, mode: "ecb", nid: 418 },
  "aes-192-ecb": { family: "ecb", key: 24, iv: 0, block: 16, mode: "ecb", nid: 422 },
  "aes-256-ecb": { family: "ecb", key: 32, iv: 0, block: 16, mode: "ecb", nid: 426 },
};
function __cipherInfo(cipher) {
  const info = __CIPHERS[String(cipher).toLowerCase()];
  return info === undefined ? undefined : { name: String(cipher).toLowerCase(), ...info };
}
// P2 crypto三件簇：getCipherInfo 元数据扩展（真机 nid 958/959/960；iv 窗 1–15）。
// 仅元数据面（create 系仍走 __CIPHERS，ocb 创建保持 Unknown cipher），故
// getCiphers() 不含 ocb（siv 系同理按需再加）。
const __CIPHER_INFO_EXTRA = {
  "aes-128-ocb": { family: "ocb", key: 16, iv: 12, block: 16, mode: "ocb", nid: 958 },
  "aes-192-ocb": { family: "ocb", key: 24, iv: 12, block: 16, mode: "ocb", nid: 959 },
  "aes-256-ocb": { family: "ocb", key: 32, iv: 12, block: 16, mode: "ocb", nid: 960 },
};
function __needCipher(cipher) {
  // 10f crypto首轮：非串 cipher 先报 ARG_TYPE（真机口径，null 即 Received null）。
  __needStr(cipher, "cipher");
  const info = __cipherInfo(cipher);
  if (info === undefined) {
    const err = new Error("Unknown cipher");
    err.code = "ERR_CRYPTO_UNKNOWN_CIPHER";
    throw err;
  }
  return info;
}
function __needKeyIv(info, key, iv, what) {
  // 10f crypto二轮：secret KeyObject 可作对称钥（裸字节；真机口径）。
  const kb = __isKeyObject(key)
    ? (__koBrand(key).kind === "secret" ? Buffer.from(__koBrand(key).material) : __cryptBytes(key, "key"))
    : __cryptBytes(key, "key");
  if (kb.length !== info.key) {
    const err = new Error("Invalid key length");
    err.code = "ERR_CRYPTO_INVALID_KEYLEN";
    throw err;
  }
  // 10f crypto首轮：iv undefined 真机文案逐字（各族一致）；null 视为空（长短由各族判定）。
  // 注意：undefined 须在 __cryptBytes 之前拦截（其 Received 形态与真机不同）。
  if (iv === undefined) {
    const err = new TypeError('The "iv" argument must be of type string or an instance of ArrayBuffer, Buffer, TypedArray, or DataView. Received undefined');
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const ivb = iv === null ? new Uint8Array(0) : __cryptBytes(iv, "iv");
  // 10f ECB：仅空 iv 合法。
  if (info.family === "ecb") {
    if (ivb.length !== 0) {
      const err = new Error("Invalid initialization vector");
      err.code = "ERR_CRYPTO_INVALID_IV";
      throw err;
    }
  } else if (info.family === "ccm") {
    // 10e CCM：iv 7–13 可变（NIST SP 800-38D；`info.iv` 仅名义 12）
    // 10e-2 GCM：任意非空 iv（12B 外走 J0 手工路径；`info.iv` 仅名义 12）
    if (ivb.length < 7 || ivb.length > 13) {
      const err = new Error("Invalid initialization vector");
      err.code = "ERR_CRYPTO_INVALID_IV";
      throw err;
    }
  } else if (info.family === "gcm") {
    if (ivb.length === 0) {
      const err = new Error("Invalid initialization vector");
      err.code = "ERR_CRYPTO_INVALID_IV";
      throw err;
    }
  } else if (ivb.length !== info.iv) {
    const err = new Error("Invalid initialization vector");
    err.code = "ERR_CRYPTO_INVALID_IV";
    throw err;
  }
  return [kb, ivb];
}
function __ccmTagLen(info, options) {
  const tl = options && options.authTagLength !== undefined ? Number(options.authTagLength) : NaN;
  if (![4, 6, 8, 10, 12, 14, 16].includes(tl)) {
    const err = new Error(`authTagLength required for ${info.name}`);
    err.code = "ERR_CRYPTO_INVALID_AUTH_TAG";
    throw err;
  }
  return tl;
}
// GCM tag 长（真机 26.8.2 实测）：缺省 16；有效集 {4,8,12,13,14,15,16}；
// 非整数 → ERR_INVALID_ARG_VALUE（Received inspect 形），整数越界 →
// ERR_CRYPTO_INVALID_AUTH_TAG（`Invalid authentication tag length: N`）。
const __GCM_TAG_LENS = new Set([4, 8, 12, 13, 14, 15, 16]);
function __gcmTagLen(options) {
  const v = options ? options.authTagLength : undefined;
  if (v === undefined) return 16;
  if (!Number.isInteger(v)) {
    const err = new TypeError(
      `The property 'options.authTagLength' is invalid. Received ${typeof v === "string" ? `'${v}'` : String(v)}`);
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
  if (!__GCM_TAG_LENS.has(v)) {
    const err = new TypeError(`Invalid authentication tag length: ${v}`);
    err.code = "ERR_CRYPTO_INVALID_AUTH_TAG";
    throw err;
  }
  return v;
}
function __badState() {
  const err = new Error("Invalid state");
  err.code = "ERR_CRYPTO_INVALID_STATE";
  throw err;
}
function __unsupportedState() {
  throw new Error("Trying to add data in unsupported state");
}
// Cipher/Decipher 输出编码门（node lib/internal/crypto/cipher.js getDecoder 口径）：
// 首个非 buffer 输出编码粘住（'utf-8' 归一为 'utf8'），再换即
// ERR_INVALID_ARG_VALUE（`cannot be changed from 'xxx'`）；未知编码即
// ERR_UNKNOWN_ENCODING；'buffer'/缺省不粘、直回 Buffer。
function __normCipherEnc(enc) {
  const s = String(enc).toLowerCase();
  if (s === "utf8" || s === "utf-8") return "utf8";
  if (s === "utf16le" || s === "utf-16le" || s === "ucs2" || s === "ucs-2") return "utf16le";
  if (s === "latin1" || s === "binary") return "latin1";
  if (s === "ascii" || s === "base64" || s === "base64url" || s === "hex") return s;
  return undefined;
}
function __cipherOut(inst, u8, outputEncoding) {
  const b = Buffer.from(u8.buffer, u8.byteOffset, u8.byteLength);
  if (outputEncoding === undefined || outputEncoding === "buffer") return b;
  const norm = __normCipherEnc(outputEncoding);
  if (norm === undefined) {
    const err = new TypeError(`Unknown encoding: ${outputEncoding}`);
    err.code = "ERR_UNKNOWN_ENCODING";
    throw err;
  }
  if (inst.__decoder === null || inst.__decoder === undefined) {
    inst.__decoder = norm;
  } else if (inst.__decoder !== norm) {
    const err = new TypeError(
      `The argument 'outputEncoding' cannot be changed from '${inst.__decoder}'. Received '${outputEncoding}'`);
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
  return b.toString(norm);
}

class CipherivImpl {
  constructor(cipher, key, iv, options) {
    const info = __needCipher(cipher);
    const [kb, ivb] = __needKeyIv(info, key, iv, "cipher");
    this.__info = info;
    this.__aad = null;
    this.__aadDone = false;
    this.__tag = null;
    this.__finalized = false;
    this.__decoder = null;
    this.__autoPad = !(options && options.autoPadding === false);
    if (info.family === "cbc" || info.family === "ctr" || info.family === "ecb") {
      this.__id = Number(__cryptCall(() =>
        __wjs2_cipher_new(info.name, kb, ivb, 1, this.__autoPad ? 1 : 0)));
      this.__parts = null;
    } else {
      // AEAD 无流式：buffered，final 时 oneshot（头注记档）
      this.__id = null;
      this.__parts = [];
      this.__key = kb;
      this.__iv = ivb;
      // 10e CCM：authTagLength 必给（真机缺省即 ERR_CRYPTO_INVALID_AUTH_TAG）
      this.__tagLen = info.family === "ccm" ? __ccmTagLen(info, options)
        : info.family === "gcm" ? __gcmTagLen(options) : 16;
    }
  }
  setAAD(aad, options) {
    if (this.__info.family !== "gcm" && this.__info.family !== "chacha" && this.__info.family !== "ccm") {
      const err = new Error("Trying to add data in unsupported state");
      throw err;
    }
    // 10e CCM：setAAD 恒要 options.plaintextLength（空 AAD 亦然，真机口径）
    if (this.__info.family === "ccm" && !(options && options.plaintextLength !== undefined)) {
      const err = new TypeError("options.plaintextLength required for CCM mode with AAD");
      err.code = "ERR_MISSING_ARGS";
      throw err;
    }
    if (this.__finalized || (this.__parts !== null && this.__aadDone)) __badState();
    this.__aad = __cryptBytes(aad, "aad");
    this.__aadDone = true;
    return this;
  }
  setAutoPadding(autoPad) {
    this.__autoPad = !!autoPad;
    if (this.__id !== null) {
      // final 后调即错（Node 同款时序守卫），余者透传 native 改 flag。
      if (this.__finalized) __badState();
      __cryptCall(() => __wjs2_cipher_set_autopad(String(this.__id), this.__autoPad ? 1 : 0));
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
    // 10f crypto首轮：超长输入（≥2^31-1）真机即抛无码错（套件点名，nodejs/node#45757）。
    if (bytes.length > 2147483646) __unsupportedState();
    let out;
    if (this.__id !== null) {
      out = __cryptCall(() => __wjs2_cipher_update(String(this.__id), bytes));
    } else {
      this.__parts.push(bytes);
      out = new Uint8Array(0);
    }
    return __cipherOut(this, out, outputEncoding);
  }
  final(outputEncoding) {
    if (this.__finalized) __badState();
    this.__finalized = true;
    let out;
    if (this.__id !== null) {
      out = __cryptCall(() => __wjs2_cipher_final(String(this.__id)));
    } else if (this.__info.family === "gcm") {
      // 10e-2：经 anyiv（12B 内走 crate，其余 J0 手工；iv 非空已在构造期校验）
      const pt = __joinParts(this.__parts);
      const tagged = __cryptCall(() =>
        __wjs2_gcm_anyiv(1, this.__key, this.__iv, this.__aad ?? new Uint8Array(0), pt));
      out = tagged.slice(0, tagged.length - 16);
      // 短 tag 取前导字节（GCM 截断口径；16 时与旧切片恒等）。
      this.__tag = Buffer.from(tagged.slice(tagged.length - 16, tagged.length - 16 + this.__tagLen));
    } else if (this.__info.family === "ccm") {
      // 10e CCM：tag 长按实例 authTagLength 切分
      const pt = __joinParts(this.__parts);
      const tagged = __cryptCall(() =>
        __wjs2_ccm_crypt(1, this.__key, this.__iv, this.__aad ?? new Uint8Array(0), pt, null, this.__tagLen));
      out = tagged.slice(0, tagged.length - this.__tagLen);
      this.__tag = Buffer.from(tagged.slice(tagged.length - this.__tagLen));
    } else {
      const pt = __joinParts(this.__parts);
      const aad = this.__aad ?? new Uint8Array(0);
      const tagged = __cryptCall(() =>
        __wjs2_cipher_chacha(1, this.__key, this.__iv, aad, pt, null));
      out = tagged.slice(0, tagged.length - 16);
      this.__tag = Buffer.from(tagged.slice(tagged.length - 16));
    }
    return __cipherOut(this, out, outputEncoding);
  }
  // 10f crypto首轮：最小流式鸭子面（同 HashImpl 记档）。
  // 注意 Cipher/Decipher 系 update 即增量吐块（CBC/CTR 真流式），end 须拼
  // update 输出（head）+ final 输出（tail），丢 head 即少块（10f 首轮现形）。
  write(chunk, inputEncoding) { this.update(chunk, inputEncoding); return true; }
  end(chunk, inputEncoding) {
    if (this.__finalized) return this;
    let head = Buffer.alloc(0);
    if (chunk !== undefined) {
      const r = this.update(chunk, inputEncoding);
      head = Buffer.isBuffer(r) ? r : Buffer.from(String(r ?? ""));
    }
    const tail = this.final();
    this.__streamOut = Buffer.concat([head, tail]);
    return this;
  }
  read() {
    const out = this.__streamOut ?? null;
    this.__streamOut = null;
    return out;
  }
  get readableLength() { return this.__streamOut ? this.__streamOut.length : 0; }
}

class DecipherivImpl {
  constructor(cipher, key, iv, options) {
    const info = __needCipher(cipher);
    const [kb, ivb] = __needKeyIv(info, key, iv, "decipher");
    this.__info = info;
    this.__aad = null;
    this.__tag = null;
    this.__finalized = false;
    this.__decoder = null;
    this.__autoPad = !(options && options.autoPadding === false);
    if (info.family === "cbc" || info.family === "ctr" || info.family === "ecb") {
      this.__id = Number(__cryptCall(() =>
        __wjs2_cipher_new(info.name, kb, ivb, 0, this.__autoPad ? 1 : 0)));
      this.__parts = null;
    } else {
      this.__id = null;
      this.__parts = [];
      this.__key = kb;
      this.__iv = ivb;
      // 10e CCM：解密侧同样必给 authTagLength（真机口径）
      this.__tagLen = info.family === "ccm" ? __ccmTagLen(info, options)
        : info.family === "gcm" ? __gcmTagLen(options) : 16;
    }
  }
  setAAD(aad, options) {
    if (this.__info.family !== "gcm" && this.__info.family !== "chacha" && this.__info.family !== "ccm") {
      throw new Error("Trying to add data in unsupported state");
    }
    // 10e CCM：解密侧 setAAD 同样恒要 options.plaintextLength
    if (this.__info.family === "ccm" && !(options && options.plaintextLength !== undefined)) {
      const err = new TypeError("options.plaintextLength required for CCM mode with AAD");
      err.code = "ERR_MISSING_ARGS";
      throw err;
    }
    if (this.__finalized) __badState();
    this.__aad = __cryptBytes(aad, "aad");
    return this;
  }
  setAuthTag(tag) {
    const tb = __cryptBytes(tag, "tag");
    // 10e CCM：tag 长错配在 set 时即抛（真机口径；GCM/ChaCha 维持 final 期检查）
    if (this.__info.family === "ccm" && tb.length !== this.__tagLen) {
      const err = new TypeError(`Invalid authentication tag length: ${tb.length}`);
      err.code = "ERR_CRYPTO_INVALID_AUTH_TAG";
      throw err;
    }
    // GCM：tag 长在 set 时即校验实例长度（真机 C++ 层口径；缺省 16）。
    if (this.__info.family === "gcm" && tb.length !== this.__tagLen) {
      const err = new TypeError(`Invalid authentication tag length: ${tb.length}`);
      err.code = "ERR_CRYPTO_INVALID_AUTH_TAG";
      throw err;
    }
    this.__tag = tb;
    return this;
  }
  setAutoPadding(autoPad) {
    if (this.__finalized) __badState();
    this.__autoPad = !!autoPad;
    if (this.__id !== null) {
      __cryptCall(() => __wjs2_cipher_set_autopad(String(this.__id), this.__autoPad ? 1 : 0));
    }
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
    // 10f crypto首轮：超长输入（≥2^31-1）真机即抛无码错（套件点名，nodejs/node#45757）。
    if (bytes.length > 2147483646) __unsupportedState();
    let out;
    if (this.__id !== null) {
      out = __cryptCall(() => __wjs2_cipher_update(String(this.__id), bytes));
    } else {
      this.__parts.push(bytes);
      out = new Uint8Array(0);
    }
    return __cipherOut(this, out, outputEncoding);
  }
  final(outputEncoding) {
    if (this.__finalized) __badState();
    this.__finalized = true;
    let out;
    if (this.__id !== null) {
      out = __cryptCall(() => __wjs2_cipher_final(String(this.__id)));
    } else if (this.__info.family === "gcm") {
      const ct = __joinParts(this.__parts);
      if (this.__tag === null || this.__tag.length !== this.__tagLen) {
        throw new Error("Unsupported state or unable to authenticate data");
      }
      const input = new Uint8Array(ct.length + this.__tagLen);
      input.set(ct, 0); input.set(this.__tag, ct.length);
      try {
        // 10e-2：经 anyiv（iv 非空已在构造期校验）
        out = __wjs2_gcm_anyiv(0, this.__key, this.__iv, this.__aad ?? new Uint8Array(0), input);
      } catch {
        throw new Error("Unsupported state or unable to authenticate data");
      }
    } else if (this.__info.family === "ccm") {
      // 10e CCM：tag 长按实例 authTagLength（set 时已校验等长）
      const ct = __joinParts(this.__parts);
      if (this.__tag === null || this.__tag.length !== this.__tagLen) {
        throw new Error("Unsupported state or unable to authenticate data");
      }
      try {
        out = __wjs2_ccm_crypt(0, this.__key, this.__iv,
          this.__aad ?? new Uint8Array(0), ct, this.__tag, this.__tagLen);
      } catch {
        throw new Error("Unsupported state or unable to authenticate data");
      }
    } else {
      const ct = __joinParts(this.__parts);
      if (this.__tag === null || this.__tag.length !== 16) {
        throw new Error("Unsupported state or unable to authenticate data");
      }
      out = __cryptCall(() => __wjs2_cipher_chacha(
        0, this.__key, this.__iv, this.__aad ?? new Uint8Array(0), ct, this.__tag));
    }
    return __cipherOut(this, out, outputEncoding);
  }
  // 10f crypto首轮：最小流式鸭子面（同 HashImpl 记档）。
  write(chunk, inputEncoding) { this.update(chunk, inputEncoding); return true; }
  end(chunk, inputEncoding) {
    if (this.__finalized) return this;
    let head = Buffer.alloc(0);
    if (chunk !== undefined) {
      const r = this.update(chunk, inputEncoding);
      head = Buffer.isBuffer(r) ? r : Buffer.from(String(r ?? ""));
    }
    const tail = this.final();
    this.__streamOut = Buffer.concat([head, tail]);
    return this;
  }
  read() {
    const out = this.__streamOut ?? null;
    this.__streamOut = null;
    return out;
  }
  get readableLength() { return this.__streamOut ? this.__streamOut.length : 0; }
}

// 10f crypto首轮：真机 `crypto.Cipheriv/Decipheriv(...)` 可无 new 调用（无废弃警告）。
function Cipheriv(...args) { return new CipherivImpl(...args); }
Object.setPrototypeOf(Cipheriv, CipherivImpl);
Cipheriv.prototype = CipherivImpl.prototype;
Cipheriv.prototype.constructor = Cipheriv;
function Decipheriv(...args) { return new DecipherivImpl(...args); }
Object.setPrototypeOf(Decipheriv, DecipherivImpl);
Decipheriv.prototype = DecipherivImpl.prototype;
Decipheriv.prototype.constructor = Decipheriv;

function __joinParts(parts) {
  let total = 0;
  for (const p of parts) total += p.length;
  const out = new Uint8Array(total);
  let off = 0;
  for (const p of parts) { out.set(p, off); off += p.length; }
  return out;
}

export function createCipheriv(cipher, key, iv, options) {
  return new CipherivImpl(cipher, key, iv, options);
}
export function createDecipheriv(cipher, key, iv, options) {
  return new DecipherivImpl(cipher, key, iv, options);
}
export function getCiphers() {
  return Object.keys(__CIPHERS);
}
export function getCipherInfo(nameOrNid, options) {
  // P2 crypto三件簇：逐字移植 internal/crypto/cipher.js getCipherInfo
  //（string 空串→undefined；number 非整数/越界→undefined；其余非串非数→ARG_TYPE；
  // options 非对象→ARG_TYPE；keyLength/ivLength 非 uint32→ARG_TYPE；
  // 长短错配→undefined；ccm iv 7–13、ocb iv 1–15 为真机可变窗）。
  if (options === undefined) options = {};
  if (typeof options !== "object" || options === null) {
    const err = new TypeError(
      `The "options" argument must be of type object. Received ${options === null ? "null" : typeof options === "string" ? `type string ('${options}')` : `type ${typeof options} (${String(options)})`}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  let { keyLength, ivLength } = options;
  const __uint32 = (v, name) => {
    if (v === undefined) return undefined;
    if (typeof v !== "number" || !Number.isInteger(v) || v < 0 || v > 4294967295) {
      const recv = v === null ? "null"
        : typeof v === "string" ? `type string ('${v}')`
        : Array.isArray(v) ? `an instance of Array (${JSON.stringify(v)})`
        : typeof v === "object" ? `an instance of ${v.constructor?.name ?? "Object"}`
        : `type ${typeof v} (${String(v)})`;
      const err = new TypeError(
        `The "options.${name}" property must be of type number. Received ${recv}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    return v + 0;
  };
  keyLength = __uint32(keyLength, "keyLength");
  ivLength = __uint32(ivLength, "ivLength");
  const t = typeof nameOrNid;
  let entry = null;
  let canon = null;
  if (t === "string") {
    if (nameOrNid.length === 0) return undefined;
    const k = nameOrNid.toLowerCase();
    canon = k;
    entry = __CIPHERS[k] ?? __CIPHER_INFO_EXTRA[k] ?? null;
    if (entry === null) return undefined;
  } else if (t === "number") {
    if (!Number.isInteger(nameOrNid) || nameOrNid < 1 || nameOrNid > 2147483647) return undefined;
    for (const [k, v] of Object.entries(__CIPHERS)) {
      if (v.nid === nameOrNid) { entry = v; canon = k; break; }
    }
    if (entry === null) {
      for (const [k, v] of Object.entries(__CIPHER_INFO_EXTRA)) {
        if (v.nid === nameOrNid) { entry = v; canon = k; break; }
      }
    }
    if (entry === null) return undefined;
  } else {
    const recv = nameOrNid === null ? "null"
      : Array.isArray(nameOrNid) ? `an instance of Array (${JSON.stringify(nameOrNid)})`
      : t === "object" ? `an instance of ${nameOrNid.constructor?.name ?? "Object"}`
      : `type ${t} (${String(nameOrNid)})`;
    const err = new TypeError(
      `The "nameOrNid" argument must be of type string or number. Received ${recv}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (keyLength !== undefined && keyLength !== entry.key) return undefined;
  if (ivLength !== undefined) {
    if (entry.family === "ccm") {
      if (ivLength < 7 || ivLength > 13) return undefined;
    } else if (entry.family === "ocb") {
      if (ivLength < 1 || ivLength > 15) return undefined;
    } else if (ivLength !== entry.iv) {
      return undefined;
    }
  }
  // 10f crypto首轮：ECB 无 ivLength 键（真机口径）。
  return {
    name: canon, mode: entry.mode, keyLength: entry.key,
    ...(entry.iv === 0 ? {} : { ivLength: entry.iv }),
    blockSize: entry.block, nid: entry.nid,
  };
}

// ── 9e-1c 非对称 ──────────────────────────────────────────────────────────

