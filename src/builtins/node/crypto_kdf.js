function __genKeyType(type) {
  if (typeof type !== "string") {
    const err = new TypeError(
      `The "type" argument must be of type string. Received type ${type === null ? "null" : typeof type} (${String(type)})`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (type !== "hmac" && type !== "aes") {
    const err = new TypeError(`The argument 'type' must be a supported key type. Received '${type}'`);
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
  return type;
}
function __genKeyLength(type, options) {
  if (typeof options !== "object" || options === null || Array.isArray(options)) {
    // 真机口径：数组亦拒（`Received an instance of Array`）。
    const recv = options === null ? "null"
      : Array.isArray(options) ? "an instance of Array"
      : `type ${typeof options} (${String(options)})`;
    const err = new TypeError(`The "options" argument must be of type object. Received ${recv}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const len = options.length;
  if (type === "aes") {
    if (len !== 128 && len !== 192 && len !== 256) {
      const err = new TypeError(
        `The property 'options.length' must be one of: 128, 192, 256. Received ${String(len)}`);
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    return len / 8;
  }
  // hmac：长度按位计，落盘 floor(len/8) 字节（真机 123→15）；aes 按位/8（128/192/256）。
  if (!Number.isInteger(len)) {
    const err = new TypeError(
      `The "options.length" property must be of type number. Received ${String(len)}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (len < 8 || len > 2147483647) {
    const err = new RangeError(
      `The value of "options.length" is out of range. It must be >= 8 && <= 2147483647. Received ${len}`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  return type === "aes" ? len / 8 : Math.floor(len / 8);
}
export function generateKey(type, options, callback) {
  const cb = typeof callback === "function" ? callback
    : typeof options === "function" ? options : undefined;
  if (typeof cb !== "function") {
    const err = new TypeError(
      `The "callback" argument must be of type function. Received ${callback === undefined ? "undefined" : typeof callback}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  // 真机口径：type/options 校验同步抛（callback 不背锅），生成本身排队。
  const t = __genKeyType(type);
  const n = __genKeyLength(t, options);
  queueMicrotask(() => {
    cb(null, createSecretKey(randomBytes(n)));
  });
}
export function generateKeySync(type, options) {
  const t = __genKeyType(type);
  const n = __genKeyLength(t, options);
  return createSecretKey(randomBytes(n));
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
  if (typeof algorithm !== "string") {
    const err = new TypeError(`The "algorithm" argument must be of type string. Received ${algorithm}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (!["argon2d", "argon2i", "argon2id"].includes(algorithm)) {
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
    const err = new RangeError(`The value of "parameters.nonce.byteLength" is out of range. It must be >= 8 && <= 4294967295. Received ${nonce.length}`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  const intArg = (v, name, min, max) => {
    // node 口径：缺参（undefined）即 ERR_INVALID_ARG_TYPE（argon2 套件删键轮），
    // 非法值才 ERR_OUT_OF_RANGE——Number(undefined)=NaN 直落区间门即错码。
    if (v === undefined) {
      const err = new TypeError(`The "parameters.${name}" property must be of type number.`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
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
  const passes = intArg(parameters.passes, "passes", 1, 4294967295);
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
  // node 口径 + 4.236：参数校验一律同步抛（含算法/参数门），microtask 只留 KDF 体力活；
  // 校验顺序先参数后回调（真机坏参+无回调即 OUT_OF_RANGE，非回调错）。
  const a = __argon2Args(algorithm, parameters);
  if (typeof callback !== "function") {
    const err = new TypeError("argon2 requires a callback for async form");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  queueMicrotask(() => {
    try {
      const out = __cryptCall(() => __wjs_kdf_argon2(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8]));
      callback(null, Buffer.from(out));
    } catch (e) {
      callback(e);
    }
  });
}

