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
  // 10e：AES-CCM 三档（iv 7–13 可变，`__needKeyIv` 另分支；nid 真机 896/897/898）
  "aes-128-ccm": { family: "ccm", key: 16, iv: 12, block: 16, mode: "ccm", nid: 896 },
  "aes-192-ccm": { family: "ccm", key: 24, iv: 12, block: 16, mode: "ccm", nid: 897 },
  "aes-256-ccm": { family: "ccm", key: 32, iv: 12, block: 16, mode: "ccm", nid: 898 },
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
function __badState() {
  const err = new Error("Invalid state");
  err.code = "ERR_CRYPTO_INVALID_STATE";
  throw err;
}
function __unsupportedState() {
  throw new Error("Trying to add data in unsupported state");
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
    if (info.family === "cbc" || info.family === "ctr" || info.family === "ecb") {
      this.__id = Number(__cryptCall(() =>
        __wjs_cipher_new(info.name, kb, ivb, 1, options && options.autoPadding === false ? 0 : 1)));
      this.__parts = null;
    } else {
      // AEAD 无流式：buffered，final 时 oneshot（头注记档）
      this.__id = null;
      this.__parts = [];
      this.__key = kb;
      this.__iv = ivb;
      // 10e CCM：authTagLength 必给（真机缺省即 ERR_CRYPTO_INVALID_AUTH_TAG）
      this.__tagLen = info.family === "ccm" ? __ccmTagLen(info, options) : 16;
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
    // 10f crypto首轮：超长输入（≥2^31-1）真机即抛无码错（套件点名，nodejs/node#45757）。
    if (bytes.length > 2147483646) __unsupportedState();
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
      // 10e-2：经 anyiv（12B 内走 crate，其余 J0 手工；iv 非空已在构造期校验）
      const pt = __joinParts(this.__parts);
      const tagged = __cryptCall(() =>
        __wjs_gcm_anyiv(1, this.__key, this.__iv, this.__aad ?? new Uint8Array(0), pt));
      out = tagged.slice(0, tagged.length - 16);
      this.__tag = Buffer.from(tagged.slice(tagged.length - 16));
    } else if (this.__info.family === "ccm") {
      // 10e CCM：tag 长按实例 authTagLength 切分
      const pt = __joinParts(this.__parts);
      const tagged = __cryptCall(() =>
        __wjs_ccm_crypt(1, this.__key, this.__iv, this.__aad ?? new Uint8Array(0), pt, null, this.__tagLen));
      out = tagged.slice(0, tagged.length - this.__tagLen);
      this.__tag = Buffer.from(tagged.slice(tagged.length - this.__tagLen));
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
    this.__autoPad = !(options && options.autoPadding === false);
    if (info.family === "cbc" || info.family === "ctr" || info.family === "ecb") {
      this.__id = Number(__cryptCall(() =>
        __wjs_cipher_new(info.name, kb, ivb, 0, this.__autoPad ? 1 : 0)));
      this.__parts = null;
    } else {
      this.__id = null;
      this.__parts = [];
      this.__key = kb;
      this.__iv = ivb;
      // 10e CCM：解密侧同样必给 authTagLength（真机口径）
      this.__tagLen = info.family === "ccm" ? __ccmTagLen(info, options) : 16;
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
    this.__tag = tb;
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
    // 10f crypto首轮：超长输入（≥2^31-1）真机即抛无码错（套件点名，nodejs/node#45757）。
    if (bytes.length > 2147483646) __unsupportedState();
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
        // 10e-2：经 anyiv（iv 非空已在构造期校验）
        out = __wjs_gcm_anyiv(0, this.__key, this.__iv, this.__aad ?? new Uint8Array(0), input);
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
        out = __wjs_ccm_crypt(0, this.__key, this.__iv,
          this.__aad ?? new Uint8Array(0), ct, this.__tag, this.__tagLen);
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
export function getCipherInfo(name) {
  const info = __cipherInfo(name);
  if (info === undefined) return undefined;
  // 10f crypto首轮：ECB 无 ivLength 键（真机口径）。
  return {
    name: info.name, mode: info.mode, keyLength: info.key,
    ...(info.iv === 0 ? {} : { ivLength: info.iv }),
    blockSize: info.block, nid: info.nid,
  };
}

// ── 9e-1c 非对称 ──────────────────────────────────────────────────────────

