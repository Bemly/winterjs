// ── 9i-4 封装面（KEM；真机口径：encapsulate 收公/私 KeyObject，decapsulate 仅私）──

const __MLKEM_KINDS = ["ml-kem-512", "ml-kem-768", "ml-kem-1024"];
export function encapsulate(key, ...rest) {
  // 真机第二参为异步回调形（本仓不做，给了即报 ERR_INVALID_ARG_TYPE）。
  if (rest.length > 0) {
    const err = new TypeError('The "callback" argument must be of type function');
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (__isKeyObject(key) && __MLKEM_KINDS.includes(key.__keyType)) {
    const isPriv = key.type === "private" ? 1 : 0;
    const r = JSON.parse(__cryptCall(() => __wjs_mlkem_encaps(key.__material, isPriv)));
    return { sharedKey: Buffer.from(__b64dec(r.sk)), ciphertext: Buffer.from(__b64dec(r.ct)) };
  }
  const err = new Error("unsupported key for encapsulation");
  err.code = "ERR_OSSL_UNSUPPORTED";
  throw err;
}
export function decapsulate(key, ciphertext) {
  if (!__isKeyObject(key)) {
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
    // node X509Certificate.toLegacyObject 键名（tls getPeerCertificate 同形）：valid_from/valid_to
    // 下划线形、fingerprint 三档、ca、raw。
    return {
      subject: this.__info.subjectObj,
      issuer: this.__info.issuerObj,
      subjectaltname: this.__info.subjectAltName,
      infoAccess: undefined,
      ca: this.ca,
      valid_from: this.__info.validFrom,
      valid_to: this.__info.validTo,
      fingerprint: this.fingerprint,
      fingerprint256: this.fingerprint256,
      fingerprint512: this.fingerprint512,
      serialNumber: this.__info.serialNumber,
      raw: Buffer.from(this.__der),
    };
  }
  verify(publicKey) {
    // 9i-3 真机口径：无参/非 KeyObject → ERR_INVALID_ARG_TYPE；私钥 → ERR_INVALID_ARG_VALUE；
    // 错钥/异族/不支持算法 → false 不抛。
    if (!__isKeyObject(publicKey)) {
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
    if (!__isKeyObject(privateKey)) {
      const err = new TypeError(`The "privateKey" argument must be an instance of KeyObject. Received ${privateKey === null ? "null" : typeof privateKey}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    if (privateKey.type !== "private") {
      const err = new TypeError(`Key type must be private for X509Certificate.checkPrivateKey. Received ${privateKey.type}`);
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    // 派生公钥后与证书 SPKI 逐字节比（ed25519/x25519 material 是裸 32B，手工包 SPKI；
    // 10e ed448 裸 57B 同形）。
    const pub = createPublicKey(privateKey);
    let spki;
    if (pub.__keyType === "ed25519" || pub.__keyType === "x25519" || pub.__keyType === "x448" || pub.__keyType === "ed448") {
      // 10e ed448 头 12B（`3043…033a00`，57B）；10f x448 头（`3042…033900`，56B）；
      // ed/x 头 12B（`302a…032100`，32B）。
      const headHex = pub.__keyType === "ed448"
        ? "3043300506032b6571033a00"
        : pub.__keyType === "x448"
          ? "3042300506032b656f033900"
          : `302a30050603${pub.__keyType === "ed25519" ? "2b6570" : "2b656e"}032100`;
      spki = Buffer.concat([Buffer.from(headHex, "hex"), Buffer.from(pub.__material)]);
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
// 10f crypto二轮：类式 API 具名导出（真机 `node:crypto` 导出表口径；
// `internal/util/types` 的 isKeyObject 静态引用亦需此门）。
export { Hash, Hmac, Cipheriv, Decipheriv, KeyObject, ECDH, DiffieHellman, DiffieHellmanGroup, Sign, Verify };
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
  createSign, createVerify, sign, verify, Sign, Verify,
  publicEncrypt, privateDecrypt, privateEncrypt, publicDecrypt,
  createECDH, ECDH, createDiffieHellman, createDiffieHellmanGroup, getDiffieHellman,
  DiffieHellman, DiffieHellmanGroup, diffieHellman, checkPrime, checkPrimeSync, generatePrime, generatePrimeSync,
  constants, getFips, setFips, setEngine, secureHeapUsed,
  pbkdf2, pbkdf2Sync, scrypt, scryptSync, hkdf, hkdfSync,
  argon2, argon2Sync, X509Certificate, Certificate,
  encapsulate, decapsulate,
};
export default __api;
