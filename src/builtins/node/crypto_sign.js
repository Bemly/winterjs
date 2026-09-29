export function generateKeyPair(type, options, ...rest) {
  // 真机 26.8.2 实测：二参（无 options）即抛 options-must-be-object（`Received undefined`），
  // 不默认 {}（旧 10e“二参合法”注释按 4.65 翻转，以实测为准）。
  if (typeof options === "function") {
    rest = [options, ...rest];
    options = undefined;
  }
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
  // 真机口径：type/options 同步抛（callback 不背锅），见 keygen 67/86 行。
  // 注意 async 口径 options 不容 undefined（sync 才容，见 generateKeyPairSync 归一）。
  if (options === undefined) {
    const err = new TypeError('The "options" argument must be of type object. Received undefined');
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  __checkKeyPairHead(type, options);
  __checkKeyPairTypeKnown(type);
  if (typeof options === "object" && options !== null) {
    __checkKeyPairEncs(type, options, options.publicKeyEncoding, options.privateKeyEncoding);
  }
  // RSA 参数同步预检（async 口径同样同步抛，见 keygen 303 行）。
  if ((type === "rsa" || type === "rsa-pss") && typeof options === "object" && options !== null) {
    __checkRsaKeyOptions(options);
  }
  // DSA 参数同步预检（同上，见 keygen.js 399 行）。
  if (type === "dsa" && typeof options === "object" && options !== null) {
    __checkDsaKeyOptions(options);
  }
  queueMicrotask(() => {
    try {
      // 单次生成再分发编码：公钥私钥必须同对（双调 __genPairSync 即对不上，
      // x-JWK 错配/RSA 解密失败/DSA 双倍慢超时同源，crypto3 keygen-async 簇）。
      const out = __applyEncoding(__genPairSync(type, options ?? {}), pubEnc, privEnc);
      cb(null, out.publicKey, out.privateKey);
    } catch (e) {
      cb(e);
    }
  });
}
// promisify 定制（keygen-promisify 套件）：默认 promisify 只取首个成功参，
// 真机以 { publicKey, privateKey } 双键决议。
generateKeyPair[Symbol.for("nodejs.util.promisify.custom")] = (type, options, ...rest) =>
  new Promise((resolve, reject) => {
    generateKeyPair(type, options, ...rest, (err, publicKey, privateKey) => {
      if (err) reject(err);
      else resolve({ publicKey, privateKey });
    });
  });
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
function __signCore(alg, data, keyObj, dsaEncoding, saltLength, padding) {
  const dataB = __cryptBytes(data, "data");
  const kt = keyObj.__keyType;
  if (kt === "ed448") {
    // 10e Ed448 纯签名（真机口径：非 null 即 ERR_OSSL_INVALID_DIGEST，ml-dsa 同款）。
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
    return __cryptCall(() => __wjs2_ed448_sign(keyObj.__material, dataB));
  }
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
    return __cryptCall(() => __wjs2_ed_sign(keyObj.__material, dataB));
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
    return __cryptCall(() => __wjs2_mldsa_sign(keyObj.__material, dataB));
  }
  const hash = __normHashName(alg);
  if (hash === undefined) {
    // 10f crypto二轮：x25519 无签名原语，报错优先于摘要校验（套件点名）。
    if (kt === "x25519" || kt === "x448") __osslKeytypeError();
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
    __checkPssPadding(kt, padding);
    // SHA-1/MD5 走手写 v1.5（digest 0.10 版本面）；SHA-2 走既有 natives
    // 10f crypto六轮：PSS 的 SHA-1 走 sha1_010 底座（MD5 仍不支持，无 0.10 可引）。
    if (hash === "SHA-1" || hash === "MD5") {
      if (kt === "rsa-pss") {
        if (hash === "MD5") {
          const err = new Error("RSA-PSS with MD5 not supported");
          err.code = "ERR_NOT_SUPPORTED";
          throw err;
        }
      } else {
        return __cryptCall(() => __wjs2_node_rsa_v15_sign(keyObj.__material, dataB, hash));
      }
    }
    const pss = kt === "rsa-pss";
    if (pss) {
      // 10f crypto六轮：rsa-pss 键约束执行（真机 26 逐项）——params 缺席的键
      // 无约束全放行；saltLength 为下限（小即 PSS_SALTLEN_TOO_SMALL），
      // hashAlgorithm 限摘要（错即 DIGEST_NOT_ALLOWED）。
      // 序：salt 先、digest 后（sha1+小 salt 落 salt 错，套件 1007 行钉住）。
      const det = keyObj.__detail ?? {};
      const defSalt = { "SHA-1": 20, "SHA-256": 32, "SHA-384": 48, "SHA-512": 64 }[hash] ?? 32;
      // 缺省 salt 取键约束值（真机实测：sha512_20 键缺省签出 salt 20，
      // 非摘要长 64；无约束键走摘要长）。
      const salt = saltLength === undefined
        ? (typeof det.saltLength === "number" ? det.saltLength : defSalt)
        : Number(saltLength);
      if (typeof det.saltLength === "number" && salt < det.saltLength) {
        const err = new Error("error:1C8000AC:Provider routines::pss saltlen too small");
        err.code = "ERR_OSSL_PSS_SALTLEN_TOO_SMALL";
        throw err;
      }
      if (typeof det.hashAlgorithm === "string" && __normHashName(det.hashAlgorithm) !== hash) {
        const err = new Error("error:1C8000AE:Provider routines::digest not allowed");
        err.code = "ERR_OSSL_DIGEST_NOT_ALLOWED";
        throw err;
      }
      // 10f crypto六轮：MGF1 自动切换（RFC4055 §3.1/3.3；真机逐项）——键约束
      // 的 mgf1 与消息摘要不同时手组 EMSA-PSS（零新 native）。
      const mgfDet = typeof det.mgf1HashAlgorithm === "string" ? __normHashName(det.mgf1HashAlgorithm) : null;
      if (mgfDet !== null && mgfDet !== hash) {
        const d = __rsaDetailsFromMaterial("private", keyObj.__material);
        if (d === null) {
          const err = new Error("OperationError: bad RSA-PSS key");
          err.code = "ERR_INVALID_ARG_VALUE";
          throw err;
        }
        const mHash = __dgstBytes(hash, dataB);
        const saltB = __randFill(new Uint8Array(salt));
        const em = __emsaPssEncode(hash, mgfDet, d.modulusLength - 1, mHash, saltB);
        if (!em) {
          const err = new Error("OperationError: EMSA-PSS encoding error");
          err.code = "ERR_INVALID_ARG_VALUE";
          throw err;
        }
        return Buffer.from(__cryptCall(() => __wjs2_rsa_raw(keyObj.__material, em, 1)));
      }
      return __cryptCall(() => __wjs2_pss_sign(hash, salt, keyObj.__material, dataB));
    }
    return __cryptCall(() => __wjs2_rsa_sign(hash, keyObj.__material, dataB));
  }
  if (kt === "ec") {
    if (keyObj.__kind !== "private") {
      const err = new TypeError("sign requires a private key");
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    const curve = keyObj.__detail.namedCurve;
    const raw = __cryptCall(() => __wjs2_ecdsa_sign(curve, hash, keyObj.__material, dataB));
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
    const der = __cryptCall(() => __wjs2_dsa_sign(hash, envStr, dataB));
    if ((dsaEncoding ?? "der") === "der") return Buffer.from(der);
    // ieee-p1363：r‖s 定长（q 长）；DER 解后拼。
    const qLen = Buffer.from(JSON.parse(envStr).q, "base64").length;
    return Buffer.from(__derToRawSig(Buffer.from(der), qLen));
  }
  if (kt === "x25519" || kt === "x448") __osslKeytypeError();
  const err = new Error(`sign not supported for ${kt}`);
  err.code = "ERR_NOT_SUPPORTED";
  throw err;
}
function __osslKeytypeError() {
  // 10f crypto二轮：X25519 等无签名原语的键（真机口径，套件正则点名）。
  const err = new Error("operation not supported for this keytype");
  err.code = "ERR_OSSL_EVP_OPERATION_NOT_SUPPORTED_FOR_THIS_KEYTYPE";
  throw err;
}
// rsa-pss 显式非 PSS padding 即非法（keygen-rsa-pss 套件；真机
// ERR_OSSL_ILLEGAL_OR_UNSUPPORTED_PADDING_MODE；RSA 键可 PSS padding 不限）。
function __checkPssPadding(kt, padding) {
  if (kt === "rsa-pss" && padding !== undefined && padding !== 6) {
    const err = new Error("error:1C8000A5:Provider routines::illegal or unsupported padding mode");
    err.code = "ERR_OSSL_ILLEGAL_OR_UNSUPPORTED_PADDING_MODE";
    throw err;
  }
}
function __verifyCore(alg, data, keyObj, sig, dsaEncoding, saltLength, padding) {
  const dataB = __cryptBytes(data, "data");
  const sigB = __cryptBytes(sig, "signature");
  const kt = keyObj.__keyType;
  // 10f crypto二轮：OKP 验签收私钥（派生公钥，真机口径；旧"verify requires a public key"记档修）。
  if ((kt === "ed448" || kt === "ed25519" || (typeof kt === "string" && kt.startsWith("ml-dsa-"))) &&
      keyObj.__kind === "private") {
    keyObj = __derivePublic(keyObj);
  }
  if (kt === "ed448") {
    // 10e Ed448 纯验签（真机口径同 sign：非 null 即 ERR_OSSL_INVALID_DIGEST）。
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
    // 10f crypto二轮：签名形态错（非 114B）回 false，不抛（空签名套件点名）。
    if (sigB.length !== 114) return false;
    return __cryptCall(() => __wjs2_ed448_verify(keyObj.__material, sigB, dataB));
  }
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
    // 10f crypto二轮：签名形态错（非 64B）回 false，不抛（空签名套件点名）。
    if (sigB.length !== 64) return false;
    return __cryptCall(() => __wjs2_ed_verify(keyObj.__material, sigB, dataB));
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
    return __cryptCall(() => __wjs2_mldsa_verify(keyObj.__material, sigB, dataB));
  }
  const hash = __normHashName(alg);
  if (hash === undefined) {
    // 10f crypto二轮：x25519 无签名原语，报错优先于摘要校验（套件点名）。
    if (kt === "x25519" || kt === "x448") __osslKeytypeError();
    const err = new Error("Invalid digest");
    err.code = "ERR_CRYPTO_INVALID_DIGEST";
    throw err;
  }
  // 公钥派生（私钥亦可验，Node 同款）
  const pubDer = keyObj.__kind === "private" ? __derivePublic(keyObj).__material : keyObj.__material;
  if (kt === "rsa" || kt === "rsa-pss") {
    __checkPssPadding(kt, padding);
    // 10f crypto二轮：签名长短 != 钥长即 false，不抛（空签名套件点名）。
    const det = __rsaDetailsFromMaterial("public", pubDer);
    if (det !== null && sigB.length !== det.modulusLength / 8) return false;
    if (hash === "SHA-1" || hash === "MD5") {
      if (kt === "rsa-pss") {
        // 10f crypto六轮：SHA-1 走底座，MD5 维持不支持（同 sign 侧）。
        if (hash === "MD5") {
          const err = new Error("RSA-PSS with MD5 not supported");
          err.code = "ERR_NOT_SUPPORTED";
          throw err;
        }
      } else {
        return __cryptCall(() => __wjs2_node_rsa_v15_verify(pubDer, sigB, dataB, hash));
      }
    }
    const pss = kt === "rsa-pss";
    if (pss) {
      const defSalt = { "SHA-1": 20, "SHA-256": 32, "SHA-384": 48, "SHA-512": 64 }[hash] ?? 32;
      // 缺省 salt 取键约束值（同 sign 侧；真机实测 sign/verify 双缺省互通）。
      const vdet0 = keyObj.__detail ?? {};
      const salt = saltLength === undefined
        ? (typeof vdet0.saltLength === "number" ? vdet0.saltLength : defSalt)
        : Number(saltLength);
      // 10f crypto六轮：验签侧 MGF1 自动切换（同 sign 侧；验不过回 false 不抛）。
      const vdet = keyObj.__detail ?? {};
      const vmgf = typeof vdet.mgf1HashAlgorithm === "string" ? __normHashName(vdet.mgf1HashAlgorithm) : null;
      if (vmgf !== null && vmgf !== hash) {
        if (det === null) return false;
        let em;
        try {
          em = Buffer.from(__cryptCall(() => __wjs2_rsa_raw(pubDer, Buffer.from(sigB), 0)));
        } catch {
          return false;
        }
        const mHash = __dgstBytes(hash, dataB);
        return __emsaPssVerify(hash, vmgf, det.modulusLength - 1, mHash, em,
          saltLength === undefined ? undefined : salt);
      }
      return __cryptCall(() => __wjs2_pss_verify(hash, salt, pubDer, sigB, dataB));
    }
    return __cryptCall(() => __wjs2_rsa_verify(hash, pubDer, sigB, dataB));
  }
  if (kt === "ec") {
    const curve = keyObj.__detail.namedCurve;
    const size = __curveSize(curve);
    // 10f crypto二轮：签名形态错回 false，不抛（空签名套件点名）。
    let raw;
    try {
      raw = (dsaEncoding ?? "der") === "der" ? __derToRawSig(sigB, size) : sigB;
    } catch {
      return false;
    }
    if (raw.length !== 2 * size) return false;
    return __cryptCall(() => __wjs2_ecdsa_verify(curve, hash, pubDer, raw, dataB));
  }
  if (kt === "dsa") {
    const envStr = Buffer.from(keyObj.__material).toString("utf8");
    const der = (dsaEncoding ?? "der") === "der" ? sigB : __rawToDerSig(sigB);
    return __cryptCall(() => __wjs2_dsa_verify(hash, envStr, der, dataB));
  }
  if (kt === "x25519" || kt === "x448") __osslKeytypeError();
  const err = new Error(`verify not supported for ${kt}`);
  err.code = "ERR_NOT_SUPPORTED";
  throw err;
}
function __keyArg(key, what) {
  if (__isKeyObject(key)) return key;
  if (typeof key === "string" || key instanceof Uint8Array || key instanceof ArrayBuffer || ArrayBuffer.isView(key)) {
    try { return createPrivateKey(key); } catch { return createPublicKey(key); }
  }
  if (typeof key === "object" && key !== null) {
    const inner = key.key ?? key;
    if (__isKeyObject(inner)) return inner;
    try { return createPrivateKey(key); } catch { return createPublicKey(key); }
  }
  const err = new TypeError(`The "${what}" argument must be a KeyObject or key material`);
  err.code = "ERR_INVALID_ARG_TYPE";
  throw err;
}
export function sign(alg, data, key, callback) {
  // 10f crypto二轮：callback 异步形（真机口径；同步核复用）。
  if (typeof callback === "function") {
    queueMicrotask(() => {
      try { callback(null, sign(alg, data, key)); }
      catch (e) { callback(e); }
    });
    return undefined;
  }
  const k = __keyArg(key, "key");
  const opts = (typeof key === "object" && key !== null && !(key instanceof Uint8Array) && !__isKeyObject(key)) ? key : {};
  const out = __signCore(alg, data, k, opts.dsaEncoding, opts.saltLength, opts.padding);
  return Buffer.from(out);
}
export function verify(alg, data, key, signature, callback) {
  // 10f crypto二轮：callback 异步形（真机口径；同步核复用）。
  if (typeof callback === "function") {
    queueMicrotask(() => {
      try { callback(null, verify(alg, data, key, signature)); }
      catch (e) { callback(e); }
    });
    return undefined;
  }
  const k = __keyArg(key, "key");
  const opts = (typeof key === "object" && key !== null && !(key instanceof Uint8Array) && !__isKeyObject(key)) ? key : {};
  return __verifyCore(alg, data, k, signature, opts.dsaEncoding, opts.saltLength, opts.padding);
}
class SignImpl {
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
    // 10f crypto二轮：rest 串 = 输出编码（真机口径；dsaEncoding/saltLength 只收
    // key-options 对象）；key-options 整体透传（passphrase 同）。
    let outputEncoding, dsaEncoding, saltLength;
    for (const r of rest) {
      if (typeof r === "string") outputEncoding = r;
      else if (typeof r === "number") saltLength = r;
      else if (r && typeof r === "object") { dsaEncoding = r.dsaEncoding ?? dsaEncoding; saltLength = r.saltLength ?? saltLength; }
    }
    const flat = __joinParts(this.__parts);
    if (typeof key === "object" && key !== null && !(key instanceof Uint8Array) && !__isKeyObject(key) && !ArrayBuffer.isView(key)) {
      const k = __keyArg(key, "key");
      const out = __signCore(this.__alg, flat, k, key.dsaEncoding ?? dsaEncoding, key.saltLength ?? saltLength, key.padding);
      const buf = Buffer.from(out);
      return outputEncoding === undefined ? buf : buf.toString(outputEncoding);
    }
    const out = __signCore(this.__alg, flat, __keyArg(key, "key"), dsaEncoding, saltLength, undefined);
    const buf = Buffer.from(out);
    return outputEncoding === undefined ? buf : buf.toString(outputEncoding);
  }
}
class VerifyImpl {
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
    // 10f crypto二轮：rest 串 = 签名编码（真机口径；余同 sign）。
    let signatureEncoding, dsaEncoding, saltLength;
    for (const r of rest) {
      if (typeof r === "string") signatureEncoding = r;
      else if (typeof r === "number") saltLength = r;
      else if (r && typeof r === "object") { dsaEncoding = r.dsaEncoding ?? dsaEncoding; saltLength = r.saltLength ?? saltLength; }
    }
    const flat = __joinParts(this.__parts);
    const sigB = signatureEncoding !== undefined ? Buffer.from(String(signature), signatureEncoding) : signature;
    if (typeof key === "object" && key !== null && !(key instanceof Uint8Array) && !__isKeyObject(key) && !ArrayBuffer.isView(key)) {
      const k = __keyArg(key, "key");
      return __verifyCore(this.__alg, flat, k, sigB, key.dsaEncoding ?? dsaEncoding, key.saltLength ?? saltLength, key.padding);
    }
    return __verifyCore(this.__alg, flat, __keyArg(key, "key"), sigB, dsaEncoding, saltLength, undefined);
  }
}
export function createSign(alg, options) { return new SignImpl(alg, options); }
export function createVerify(alg, options) { return new VerifyImpl(alg, options); }
// P2 crypto三件簇：真机 `crypto.Sign/Verify(...)` 可无 new 调用（legacy 函数形，
// 无废弃警告；Cipheriv/Decipheriv 同款，见 crypto_cipher.js）。
function Sign(...args) { return new SignImpl(...args); }
Object.setPrototypeOf(Sign, SignImpl);
Sign.prototype = SignImpl.prototype;
Sign.prototype.constructor = Sign;
function Verify(...args) { return new VerifyImpl(...args); }
Object.setPrototypeOf(Verify, VerifyImpl);
Verify.prototype = VerifyImpl.prototype;
Verify.prototype.constructor = Verify;
// 10f crypto二轮：混合 OAEP 编解码（oaepHash ≠ mgf1Hash；几何经真机预言机定案：
// 种子长取 oaep 哈希长、掩码走 mgf1Hash、界为 k-2*hLen-2；双向真机交叉见黑盒）。
const __OAEP_HLEN = { "SHA-1": 20, "SHA-224": 28, "SHA-256": 32, "SHA-384": 48, "SHA-512": 64 };
function __oaepTooLarge() {
  const err = new Error("error:0200006E:rsa routines::data too large for key size");
  err.code = "ERR_OSSL_RSA_DATA_TOO_LARGE_FOR_KEY_SIZE";
  throw err;
}
function __oaepDecodingError() {
  const err = new Error("error:02000079:rsa routines::oaep decoding error");
  err.code = "ERR_OSSL_RSA_OAEP_DECODING_ERROR";
  throw err;
}
function __oaepMixedEncode(oaepName, mgfName, k, data, labelB) {
  const hLen = __OAEP_HLEN[oaepName];
  if (data.length > k - 2 * hLen - 2) __oaepTooLarge();
  const lHash = __dgstBytes(oaepName, labelB ?? Buffer.alloc(0));
  const psLen = k - data.length - 2 * hLen - 2;
  const db = Buffer.concat([lHash, Buffer.alloc(psLen), Buffer.from([1]), Buffer.from(data)]);
  const seed = __randFill(new Uint8Array(hLen));
  const dbMask = __mgf1Bytes(mgfName, seed, db.length);
  const maskedDB = Buffer.from(db.map((b, i) => b ^ dbMask[i]));
  const seedMask = __mgf1Bytes(mgfName, maskedDB, hLen);
  const maskedSeed = Buffer.from(seed.map((b, i) => b ^ seedMask[i]));
  return Buffer.concat([Buffer.from([0]), maskedSeed, maskedDB]);
}
function __oaepMixedDecode(oaepName, mgfName, k, em, labelB) {
  const hLen = __OAEP_HLEN[oaepName];
  if (em.length !== k || em[0] !== 0) __oaepDecodingError();
  const maskedSeed = em.slice(1, 1 + hLen), maskedDB = em.slice(1 + hLen);
  const seedMask = __mgf1Bytes(mgfName, maskedDB, hLen);
  const seed = Buffer.from(maskedSeed.map((b, i) => b ^ seedMask[i]));
  const dbMask = __mgf1Bytes(mgfName, seed, maskedDB.length);
  const db = Buffer.from(maskedDB.map((b, i) => b ^ dbMask[i]));
  const lHash = __dgstBytes(oaepName, labelB ?? Buffer.alloc(0));
  if (!db.slice(0, hLen).equals(lHash)) __oaepDecodingError();
  const rest = db.slice(hLen);
  const one = rest.indexOf(1);
  if (one < 0 || !rest.slice(0, one).equals(Buffer.alloc(one))) __oaepDecodingError();
  return rest.slice(one + 1);
}
function __dgstBytes(name, data) {
  return createHash(name).update(Buffer.from(data)).digest();
}
function __mgf1Bytes(name, seed, outLen) {
  const hLen = __OAEP_HLEN[name];
  let out = Buffer.alloc(0);
  let counter = 0;
  while (out.length < outLen) {
    const c = Buffer.alloc(4);
    c.writeUInt32BE(counter++);
    out = Buffer.concat([out, __dgstBytes(name, Buffer.concat([Buffer.from(seed), c]))]);
  }
  return out.slice(0, outLen);
}
// 10f crypto六轮：EMSA-PSS 手组（MGF1 哈希可与消息摘要不同；RFC4055 §3.1/3.3
// 自动切换。MGF1/摘要走既有 __mgf1Bytes/__dgstBytes，模幂走 __wjs2_rsa_raw；
// 零新 native。侧信道记档：非恒定时间实现，与混合 OAEP 手写同口径）。
// 编码成功回 Buffer EM；长度不足回 null（调用方按 OpenSSL 口径报错）。
function __emsaPssEncode(msgName, mgfName, emBits, mHash, salt) {
  const hLen = __OAEP_HLEN[msgName];
  const emLen = Math.ceil(emBits / 8);
  if (emLen < hLen + salt.length + 2) return null;
  const h = __dgstBytes(msgName, Buffer.concat([Buffer.alloc(8), Buffer.from(mHash), Buffer.from(salt)]));
  const db = Buffer.concat([Buffer.alloc(emLen - salt.length - hLen - 2), Buffer.from([1]), Buffer.from(salt)]);
  const dbMask = __mgf1Bytes(mgfName, h, db.length);
  for (let i = 0; i < db.length; i++) db[i] ^= dbMask[i];
  db[0] &= 0xff >> (8 * emLen - emBits);
  return Buffer.concat([db, Buffer.from(h), Buffer.from([0xbc])]);
}
function __emsaPssVerify(msgName, mgfName, emBits, mHash, em, wantSaltLen) {
  const hLen = __OAEP_HLEN[msgName];
  const emLen = Math.ceil(emBits / 8);
  if (em.length !== emLen || em[emLen - 1] !== 0xbc) return false;
  const topBits = 8 * emLen - emBits;
  if (topBits > 0 && (em[0] & (0xff << (8 - topBits))) !== 0) return false;
  const maskedDB = em.slice(0, emLen - hLen - 1), h = em.slice(emLen - hLen - 1, emLen - 1);
  const dbMask = __mgf1Bytes(mgfName, h, maskedDB.length);
  const db = Buffer.from(maskedDB.map((b, i) => b ^ dbMask[i]));
  db[0] &= 0xff >> topBits;
  let salt;
  if (wantSaltLen === undefined) {
    let i = 0;
    while (i < db.length && db[i] === 0) i++;
    if (i >= db.length || db[i] !== 1) return false;
    salt = db.slice(i + 1);
  } else {
    const s = Number(wantSaltLen);
    const psLen = emLen - hLen - s - 2;
    if (psLen < 0 || !db.slice(0, psLen).equals(Buffer.alloc(psLen)) || db[psLen] !== 1) return false;
    salt = db.slice(psLen + 1);
  }
  const h2 = __dgstBytes(msgName, Buffer.concat([Buffer.alloc(8), Buffer.from(mHash), Buffer.from(salt)]));
  return Buffer.from(h).equals(Buffer.from(h2));
}
function __rsaCrypt(key, data, isPublic, isEncrypt) {
  let k = __keyArg(key, "key");
  // 10f crypto二轮：public 方向遇私钥即派生公钥（`publicEncrypt(privPem)` 真机口径）。
  if (isPublic && k.type === "private") k = __derivePublic(k);
  // rsa-pss 禁加解密（keygen-rsa-pss 套件：'operation not supported for this keytype'）。
  if (k.__keyType === "rsa-pss") __osslKeytypeError();
  const opts = (typeof key === "object" && key !== null && !(key instanceof Uint8Array) && !__isKeyObject(key)) ? key : {};
  // 10f crypto二轮：默认 padding 按方向（加解密 OAEP=4；签式 v1.5=1，真机口径）。
  const padding = opts.padding ?? (isEncrypt === isPublic ? 4 : 1);
  // 10f crypto二轮：key 对象带 encoding 时字符串 data 同解码（真机实证口径）。
  const dataB = (typeof data === "string" && typeof opts.encoding === "string")
    ? Buffer.from(data, opts.encoding)
    : __cryptBytes(data, "data");
  if (padding === 4) {
    // 10f crypto二轮：oaepHash/oaepLabel 校验（真机逐字，dsa 套件点名）。
    const __recvAny = (v) => {
      if (v === null || v === undefined) return String(v);
      const t = typeof v;
      if (t === "object" || t === "function") return `an instance of ${v.constructor?.name ?? "Object"}`;
      let s;
      try { s = (t === "string") ? `'${v}'` : String(v); } catch { s = t; }
      return `type ${t} (${s})`;
    };
    if (opts.oaepHash !== undefined && typeof opts.oaepHash !== "string") {
      const err = new TypeError(
        `The "key.oaepHash" property must be of type string. Received ${__recvAny(opts.oaepHash)}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    const hashFlat = opts.oaepHash ?? "sha1";
    const hash = __normHashName(hashFlat);
    if (hash === undefined) {
      const err = new Error("Invalid digest used");
      err.code = "ERR_OSSL_EVP_INVALID_DIGEST";
      throw err;
    }
    if (hash === "MD5") {
      const err = new Error(`Unsupported OAEP hash ${opts.oaepHash}`);
      err.code = "ERR_NOT_SUPPORTED";
      throw err;
    }
    let label = null;
    if (opts.oaepLabel !== undefined) {
      const ol = opts.oaepLabel;
      if (!(typeof ol === "string" || ol instanceof Uint8Array ||
            ol instanceof ArrayBuffer || ArrayBuffer.isView(ol))) {
        const err = new TypeError(
          `The "key.oaepLabel" property must be of type string or an instance of ArrayBuffer, Buffer, TypedArray, or DataView. Received ${__recvAny(ol)}`);
        err.code = "ERR_INVALID_ARG_TYPE";
        throw err;
      }
      label = __cryptBytes(ol, "label");
    }
    // 10f crypto二轮：mgf1Hash（真机口径；缺省跟随 oaepHash；未知即 INVALID_DIGEST）。
    let mgfHash = hash;
    if (opts.mgf1Hash !== undefined) {
      if (typeof opts.mgf1Hash !== "string") {
        const err = new TypeError(
          `The "key.mgf1Hash" property must be of type string. Received ${__recvAny(opts.mgf1Hash)}`);
        err.code = "ERR_INVALID_ARG_TYPE";
        throw err;
      }
      const mg = __normHashName(opts.mgf1Hash);
      if (mg === undefined || __OAEP_HLEN[mg] === undefined) {
        const err = new Error("Invalid digest used");
        err.code = "ERR_OSSL_EVP_INVALID_DIGEST";
        throw err;
      }
      mgfHash = mg;
    }
    // 混合哈希（mgf ≠ oaep）走 JS 编解码 + 裸 RSA（真机互操作另行交叉验证，见注释）。
    const mixedHash = mgfHash !== hash;
    // SHA-1 走手写 OAEP（digest 0.10 版本面，见 Rust 侧记）；SHA-2 走既有 natives
    if (isPublic && isEncrypt) {
      // 10f crypto二轮：混合哈希走 JS 编解码 + 裸 RSA（mgf1Hash 套件）。
      if (mixedHash) {
        const det = __rsaDetailsFromMaterial(k.type, k.__material);
        if (det !== null && __OAEP_HLEN[hash] !== undefined) {
          const em = __oaepMixedEncode(hash, mgfHash, det.modulusLength / 8, dataB, label);
          return Buffer.from(__cryptCall(() => __wjs2_rsa_raw(k.__material, em, 0)));
        }
      }
      if (hash === "SHA-1") {
        return Buffer.from(__cryptCall(() => __wjs2_node_rsa_oaep(k.__material, dataB, label, 1)));
      }
      return Buffer.from(__cryptCall(() => __wjs2_rsa_encrypt(hash, k.__material, dataB, label)));
    }
    if (!isPublic && !isEncrypt) {
      // 10f crypto二轮：混合哈希解码（同上）。
      if (mixedHash) {
        const det = __rsaDetailsFromMaterial(k.type, k.__material);
        if (det !== null && __OAEP_HLEN[hash] !== undefined) {
          const em = Buffer.from(__cryptCall(() => __wjs2_rsa_raw(k.__material, dataB, 1)));
          return __oaepMixedDecode(hash, mgfHash, det.modulusLength / 8, em, label);
        }
      }
      if (hash === "SHA-1") {
        return Buffer.from(__cryptCall(() => __wjs2_node_rsa_oaep(k.__material, dataB, label, 0)));
      }
      return Buffer.from(__cryptCall(() => __wjs2_rsa_decrypt(hash, k.__material, dataB, label)));
    }
    // 10f crypto二轮：私钥加密（OAEP-SHA1 反向 native；SHA-2 系另案）。
    if (!isPublic && isEncrypt) {
      if (hash === "SHA-1") {
        return Buffer.from(__cryptCall(() => __wjs2_node_rsa_oaep_flip(k.__material, dataB, label, 1)));
      }
      const err = new Error("RSA privateEncrypt not supported");
      err.code = "ERR_NOT_SUPPORTED";
      throw err;
    }
    // 10f crypto二轮：公钥解密（同上）。
    if (isPublic && !isEncrypt) {
      if (hash === "SHA-1") {
        return Buffer.from(__cryptCall(() => __wjs2_node_rsa_oaep_flip(k.__material, dataB, label, 0)));
      }
      const err = new Error("RSA publicDecrypt not supported");
      err.code = "ERR_NOT_SUPPORTED";
      throw err;
    }
  }
  if (padding === 1) {
    if (isPublic && isEncrypt) {
      return Buffer.from(__cryptCall(() => __wjs2_rsa_encrypt_v15(k.__material, dataB)));
    }
    if (!isPublic && !isEncrypt) {
      return Buffer.from(__cryptCall(() => __wjs2_rsa_decrypt_v15(k.__material, dataB)));
    }
    // 10f crypto二轮：v1.5 反向 native。
    if (!isPublic && isEncrypt) {
      return Buffer.from(__cryptCall(() => __wjs2_rsa_v15_flip(k.__material, dataB, 1)));
    }
    if (isPublic && !isEncrypt) {
      return Buffer.from(__cryptCall(() => __wjs2_rsa_v15_flip(k.__material, dataB, 0)));
    }
  }
  // 10f crypto二轮：NO_PADDING 裸运算（3；私钥 d 次幂/公钥 e 次幂）。
  if (padding === 3) {
    return Buffer.from(__cryptCall(() => __wjs2_rsa_raw(k.__material, dataB, isPublic ? 0 : 1)));
  }
  const err = new Error(`Unsupported RSA padding ${padding} for this operation`);
  err.code = "ERR_NOT_SUPPORTED";
  throw err;
}
export function publicEncrypt(key, data) { return __rsaCrypt(key, data, true, true); }
export function privateDecrypt(key, data) { return __rsaCrypt(key, data, false, false); }
export function privateEncrypt(key, data) { return __rsaCrypt(key, data, false, true); }
export function publicDecrypt(key, data) { return __rsaCrypt(key, data, true, false); }

class ECDHImpl {
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
    this.__priv = __cryptCall(() => __wjs2_ec_generate(this.__curve));
    this.__pub = __cryptCall(() => __wjs2_ec_public(this.__curve, this.__priv));
    return this.getPublicKey();
  }
  getPublicKey(encoding, format) {
    if (this.__pub === null) {
      const err = new Error("ECDH keys not generated");
      err.code = "ERR_CRYPTO_INVALID_STATE";
      throw err;
    }
    const parts = JSON.parse(__cryptCall(() => __wjs2_ec_jwk_pub(this.__curve, this.__pub)));
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
    const parts = JSON.parse(__cryptCall(() => __wjs2_ec_jwk(this.__curve, this.__priv, this.__pub)));
    const d = __b64urlDec(parts.d);
    const out = new Uint8Array(size);
    out.set(d, size - d.length);
    if (encoding === undefined) return Buffer.from(out);
    return Buffer.from(out).toString(encoding);
  }
  setPrivateKey(priv) {
    const scalar = __cryptBytes(priv, "private key");
    this.__priv = __cryptCall(() => __wjs2_ec_import_priv(this.__curve, scalar));
    this.__pub = __cryptCall(() => __wjs2_ec_public(this.__curve, this.__priv));
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
      peerDer = __cryptCall(() => __wjs2_ec_import_pub(this.__curve, peerB.slice(1, 1 + size), peerB.slice(1 + size)));
    } else {
      peerDer = peerB;
    }
    const secret = __cryptCall(() => __wjs2_ecdh_derive(this.__curve, this.__priv, peerDer));
    if (outputEncoding === undefined) return Buffer.from(secret);
    return Buffer.from(secret).toString(outputEncoding);
  }
}
export function createECDH(curve, format) { return new ECDHImpl(curve); }

// 10f crypto首轮：真机 `crypto.ECDH(...)` 可无 new 调用（无废弃警告）。
function ECDH(...args) { return new ECDHImpl(...args); }
Object.setPrototypeOf(ECDH, ECDHImpl);
ECDH.prototype = ECDHImpl.prototype;
ECDH.prototype.constructor = ECDH;

