class DiffieHellmanImpl {
  constructor(prime, generator, generatorEncoding) {
    // 10f crypto首轮：数值位长形同步生成素数（真机口径；`generatePrimeSync` 复用）。
    if (typeof prime === "number") {
      prime = generatePrimeSync(prime);
    }
    const primeB = (typeof prime === "string") ? __cryptBytes(prime, "prime", "hex") : __cryptBytes(prime, "prime");
    this.__prime = primeB;
    // 10f crypto二轮：generator 字节语义——数字即值；字符串按 generatorEncoding
    // （缺省 latin1）解码；Buffer/视图即裸字节（真机逐项对拍；旧 Number(串) 记档修）。
    this.__genBytes = __dhGenBytes(generator, generatorEncoding);
    this.__gen = __dhGenNum(this.__genBytes);
    this.__priv = null;
    this.__pub = null;
    this.__verifyError = 0;
  }
  static group(name) {
    const hex = __DH_GROUPS[String(name).toLowerCase()];
    if (hex === undefined) {
      const err = new Error(`Unknown DH group ${name} (modp1/2/5/14/15/16)`);
      err.code = "ERR_NOT_SUPPORTED";
      throw err;
    }
    return new DiffieHellmanImpl(__cryptBytes(hex, "prime", "hex"), 2);
  }
  generateKeys() {
    const r = JSON.parse(__cryptCall(() => __wjs2_dh_genkey(this.__prime, this.__gen, this.__prime.length)));
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
    // 10f crypto首轮：'buffer' 编码（大小写不敏感）即回 Buffer（真机口径）。
    if (encoding === undefined || String(encoding).toLowerCase() === "buffer") return Buffer.from(this.__pub);
    return Buffer.from(this.__pub).toString(encoding);
  }
  getPrivateKey(encoding) {
    if (this.__priv === null) {
      const err = new Error("DH keys not generated");
      err.code = "ERR_CRYPTO_INVALID_STATE";
      throw err;
    }
    if (encoding === undefined || String(encoding).toLowerCase() === "buffer") return Buffer.from(this.__priv);
    return Buffer.from(this.__priv).toString(encoding);
  }
  getPrime(encoding) {
    if (encoding === undefined || String(encoding).toLowerCase() === "buffer") return Buffer.from(this.__prime);
    return Buffer.from(this.__prime).toString(encoding);
  }
  getGenerator(encoding) {
    // 10f crypto二轮：回存的 generator 字节（非单字节截断）。
    const g = this.__genBytes;
    if (encoding === undefined || String(encoding).toLowerCase() === "buffer") return Buffer.from(g);
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
    const secret = __cryptCall(() => __wjs2_dh_secret(this.__prime, this.__priv, peerB));
    if (outEnc === undefined) return Buffer.from(secret);
    return Buffer.from(secret).toString(outEnc);
  }
  verifyError() { return this.__verifyError; }
}
export function createDiffieHellman(prime, generator, generatorEncoding) {
  if (typeof prime === "string" && __DH_GROUPS[prime.toLowerCase()] !== undefined && generator === undefined) {
    return DiffieHellmanImpl.group(prime);
  }
  return new DiffieHellmanImpl(prime, generator, generatorEncoding);
}
// 10f crypto二轮：generator 字节语义 helpers（见构造注释）。
function __dhGenBytes(gen, genEnc) {
  if (gen === undefined) return new Uint8Array([2]);
  if (typeof gen === "number") {
    if (!Number.isInteger(gen) || gen < 0) return new Uint8Array([2]);
    if (gen === 0) return new Uint8Array([0]);
    const out = [];
    let v = gen;
    while (v > 0) { out.unshift(v & 255); v = Math.floor(v / 256); }
    return new Uint8Array(out);
  }
  if (typeof gen === "string") return __cryptBytes(gen, "generator", genEnc ?? "latin1");
  return __cryptBytes(gen, "generator");
}
function __dhGenNum(bytes) {
  let v = 0n;
  for (const b of bytes) v = (v << 8n) | BigInt(b);
  return v <= 0xFFFFFFFFFFFFFFFFn ? Number(v) : NaN;
}
export function createDiffieHellmanGroup(name) { return __dhGroup(DiffieHellmanImpl.group(name)); }
export function getDiffieHellman(name) { return __dhGroup(DiffieHellmanImpl.group(name)); }
// 10f crypto首轮：真机 `crypto.DiffieHellman/DiffieHellmanGroup/ECDH` 均可无 new 调用。
function DiffieHellman(...args) { return new DiffieHellmanImpl(...args); }
Object.setPrototypeOf(DiffieHellman, DiffieHellmanImpl);
DiffieHellman.prototype = DiffieHellmanImpl.prototype;
DiffieHellman.prototype.constructor = DiffieHellman;
function DiffieHellmanGroup(name) { return __dhGroup(DiffieHellmanImpl.group(name)); }
Object.setPrototypeOf(DiffieHellmanGroup, DiffieHellmanImpl);
// 10f crypto二轮：Group 自立原型（链向 Impl 原型）：constructor 归 Group、
// setters 置 undefined（真机口径：Group 无 setters，其余方法继承可用）。
DiffieHellmanGroup.prototype = Object.create(DiffieHellmanImpl.prototype);
DiffieHellmanGroup.prototype.constructor = DiffieHellmanGroup;
DiffieHellmanGroup.prototype.setPrivateKey = undefined;
DiffieHellmanGroup.prototype.setPublicKey = undefined;
function __dhGroup(inst) { Object.setPrototypeOf(inst, DiffieHellmanGroup.prototype); return inst; }
export function diffieHellman(options) {
  const priv = options?.privateKey;
  const pub = options?.publicKey;
  const dh = (priv instanceof DiffieHellman) ? priv : null;
  if (dh !== null && pub instanceof DiffieHellman) {
    return dh.computeSecret(pub.getPublicKey());
  }
  if (__isKeyObject(priv) && __isKeyObject(pub)) {
    if (priv.__keyType === "x25519") {
      const out = __cryptCall(() => __wjs2_x_derive(priv.__material, pub.__material));
      return Buffer.from(out);
    }
    if (priv.__keyType === "x448") {
      const out = __cryptCall(() => __wjs2_x448_derive(priv.__material, pub.__material));
      return Buffer.from(out);
    }
    if (priv.__keyType === "ec") {
      const curve = priv.__detail.namedCurve;
      const pubDer = pub.__kind === "private" ? __derivePublic(pub).__material : pub.__material;
      const out = __cryptCall(() => __wjs2_ecdh_derive(curve, priv.__material, pubDer));
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
  return __cryptCall(() => __wjs2_prime_check(bytes, checks));
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
  const out = __cryptCall(() => __wjs2_prime_gen(bits, options?.checks ?? 64, options?.safe ? 1 : 0));
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
