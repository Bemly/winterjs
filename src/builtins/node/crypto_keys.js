const __DH_GROUPS = {
  // 素数取自真 Node `getPrime('hex')`（modp1/2 为 RFC 2409，余为 RFC 7919），generator 恒 2
  // 10f crypto二轮：补 modp1（768B）/modp2（1024B）。
  "modp1": "ffffffffffffffffc90fdaa22168c234c4c6628b80dc1cd129024e088a67cc74020bbea63b139b22514a08798e3404ddef9519b3cd3a431b302b0a6df25f14374fe1356d6d51c245e485b576625e7ec6f44c42e9a63a3620ffffffffffffffff",
  "modp2": "ffffffffffffffffc90fdaa22168c234c4c6628b80dc1cd129024e088a67cc74020bbea63b139b22514a08798e3404ddef9519b3cd3a431b302b0a6df25f14374fe1356d6d51c245e485b576625e7ec6f44c42e9a637ed6b0bff5cb6f406b7edee386bfb5a899fa5ae9f24117c4b1fe649286651ece65381ffffffffffffffff",
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
// 10f crypto五轮：Node inspect 小口径（raw type 回显；套件钉字符串，
 // null/数字/布尔/对象按 inspect 形；深结构近似记档）。
const __inspectSmall = (v) => typeof v === "string" ? `'${v}'`
  : v === null || v === undefined ? String(v)
  : typeof v !== "object" ? String(v)
  : (() => { try { return JSON.stringify(v) ?? String(v); } catch { return String(v); } })();
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
// 10f crypto五轮：SLH-DSA 装载（套件指纹：仅 sha2-128f/192f， DER 指纹见
// /tmp/wjs-raw-probe 注释；余 10 集未实现，仍报 Invalid SPKI/PKCS#8）。
// SPKI = SEQ{ SEQ{ OID }, BITSTRING(00||pk) }；PKCS#8 = SEQ{ INT 0, SEQ{ OID },
// OCTET(sk) }。OID hex：128f=...0315（2.16.840.1.101.3.4.3.21）、
// 192f=...0317（...3.23）。[privLen, pubLen] = 128f:[64,32]、192f:[96,48]。
const __SLH_SETS = {
  "608648016503040315": ["slh-dsa-sha2-128f", 64, 32],
  "608648016503040317": ["slh-dsa-sha2-192f", 96, 48],
};
function __slhParse(der, want) {
  // want: 'public' | 'private'；不合即回 null（调用方落 "no"，沿试解链）。
  let kids;
  try {
    const top = __derRead(der, 0);
    if (top.tag !== 48) return null;
    kids = __derChildren(top.body);
  } catch { return null; }
  const oidOf = (algSeq) => {
    let inner;
    try { inner = __derChildren(algSeq.body); } catch { return null; }
    if (inner.length !== 1 || inner[0].tag !== 6) return null;
    return __SLH_SETS[Buffer.from(inner[0].body).toString("hex")] ?? null;
  };
  if (want === "public") {
    if (kids.length !== 2 || kids[0].tag !== 48 || kids[1].tag !== 3) return null;
    const set = oidOf(kids[0]);
    if (!set) return null;
    const bit = kids[1].body;
    if (bit.length !== set[2] + 1 || bit[0] !== 0) return null;
    return { name: set[0], raw: bit.subarray(1) };
  }
  if (kids.length !== 3 || kids[0].tag !== 2 || kids[1].tag !== 48 || kids[2].tag !== 4) return null;
  if (kids[0].body.length !== 1 || kids[0].body[0] !== 0) return null;
  const set = oidOf(kids[1]);
  if (!set) return null;
  if (kids[2].body.length !== set[1]) return null;
  return { name: set[0], raw: kids[2].body };
}
// 10f crypto五轮：ML 参数集表（[oidHex, seedLen, pubLen]，Rust
// mlkem_params/mldsa_params 同源指纹）+ 裸料回包 DER 构造
//（SPKI = SEQ{ SEQ{ OID }, BITSTRING(00||ek) }；种子 PKCS#8 =
// SEQ{ INT 0, SEQ{ OID }, OCTET{ [0] seed } }，Rust mlkem_pkcs8 同构）。
const __ML_SETS = {
  "ml-kem-512": ["608648016503040401", 64, 800],
  "ml-kem-768": ["608648016503040402", 64, 1184],
  "ml-kem-1024": ["608648016503040403", 64, 1568],
  "ml-dsa-44": ["608648016503040311", 32, 1312],
  "ml-dsa-65": ["608648016503040312", 32, 1952],
  "ml-dsa-87": ["608648016503040313", 32, 2592],
};
const __tlv = (tag, body) => new Uint8Array([tag, ...__derLen(body.length), ...body]);
// 10f crypto六轮：RSA-PSS（alg OID 直判 + 归一化 plain-RSA 存料；params 缺席即
// 无约束，空/显式 params 落 detail；未知哈希/mgf/非法 trailer 即拒解）。
const __PSS_OID_HEX = "2a864886f70d01010a";      // rsassaPss
const __RSA_ENC_OID_HEX = "2a864886f70d010101";  // rsaEncryption
const __MGF1_OID_HEX = "2a864886f70d010108";
const __PSS_HASH_BY_OID = {
  // 注：键为 OID body hex（不含 tag/len；SHA-1 即 1.3.14.3.2.26）。
  "2b0e03021a": "sha1",
  "608648016503040204": "sha224",
  "608648016503040201": "sha256",
  "608648016503040202": "sha384",
  "608648016503040203": "sha512",
};
function __pssHashNameBody(seqBody) {
  // HashAlgorithm SEQ 的 body → 名；非法即 null。
  let kids;
  try { kids = __derChildren(seqBody); } catch { return null; }
  if (kids.length !== 1 || kids[0].tag !== 0x06) return null;
  return __PSS_HASH_BY_OID[Buffer.from(kids[0].body).toString("hex")] ?? null;
}
function __pssParseParams(body) {
  // RSASSA-PSS-params body → {hashAlgorithm, mgf1HashAlgorithm, saltLength}；
  // 全缺省（空 SEQ）即 sha1/sha1/20；非法即 null。
  let items;
  try { items = __derChildren(body); } catch { return null; }
  let hash = "sha1", mgf = "sha1", salt = 20;
  const readInt = (tlvBytes) => {
    let t;
    try { t = __derRead(tlvBytes, 0); } catch { return null; }
    if (t.tag !== 0x02) return null;
    let v = 0;
    for (const b of t.body) v = v * 256 + b;
    return v;
  };
  for (const it of items) {
    if (it.tag === 0xa0) {
      // 显式标签内容即完整 HashAlgorithm TLV。
      let t;
      try { t = __derRead(it.body, 0); } catch { return null; }
      if (t.tag !== 0x30) return null;
      const h = __pssHashNameBody(t.body);
      if (!h) return null;
      hash = h;
    } else if (it.tag === 0xa1) {
      // [1] 显式标签内容即完整 MaskGenAlgorithm TLV（SEQ{ OID-mgf1, SEQ{ hash } }）——
      // 先读外层 SEQ，再取其 body 的 [OID, SEQ] 两元。
      let mgfSeq;
      try {
        const t = __derRead(it.body, 0);
        if (t.tag !== 0x30) return null;
        mgfSeq = __derChildren(t.body);
      } catch { return null; }
      if (mgfSeq.length !== 2 || mgfSeq[0].tag !== 0x06 ||
          Buffer.from(mgfSeq[0].body).toString("hex") !== __MGF1_OID_HEX) return null;
      if (mgfSeq[1].tag !== 0x30) return null;
      const h = __pssHashNameBody(mgfSeq[1].body);
      if (!h) return null;
      mgf = h;
    } else if (it.tag === 0xa2) {
      const s = readInt(it.body);
      if (s === null) return null;
      salt = s;
    } else if (it.tag === 0xa3) {
      if (readInt(it.body) !== 1) return null;
    } else return null;
  }
  return { hashAlgorithm: hash, mgf1HashAlgorithm: mgf, saltLength: salt };
}
function __pssAlg(algBody) {
  // 算法 SEQ body → {restrictions|null, paramsB64|null}；非 PSS 即 null。
  // paramsB64 = params SEQ 的 body 原文字节（导出回贴用；缺席即 null）。
  let alg;
  try { alg = __derChildren(algBody); } catch { return null; }
  if (alg.length < 1 || alg[0].tag !== 0x06 ||
      Buffer.from(alg[0].body).toString("hex") !== __PSS_OID_HEX) return null;
  if (alg.length === 1) return { restrictions: null, paramsB64: null };
  if (alg.length !== 2 || alg[1].tag !== 0x30) return null;
  const restrictions = __pssParseParams(alg[1].body);
  if (!restrictions) return null;
  return { restrictions, paramsB64: Buffer.from(alg[1].body).toString("base64") };
}
function __rsaEncAlgSeq() {
  return __tlv(0x30, new Uint8Array([
    ...__tlv(0x06, Buffer.from(__RSA_ENC_OID_HEX, "hex")),
    ...__tlv(0x05, new Uint8Array(0)),
  ]));
}
// 10f crypto六轮：rsa-pss 导出回贴算法标识（归一化存料→线形；params 缺席/
// 无 stash（生成键）即裸 OID——与 OpenSSL 省略 DEFAULT 参数同形，真机逐字节
// 对拍，round-trip 保 rsa-pss 型）。
// 前置：der 为归一化 plain-RSA 的 SPKI/PKCS#8。
function __pssReattach(kind, normDer, detail) {
  const paramsB64 = detail?.pssParams ?? null;
  const pssOid = Buffer.from(__PSS_OID_HEX, "hex");
  const alg = paramsB64 === null
    ? __tlv(0x30, __tlv(0x06, pssOid))
    : __tlv(0x30, new Uint8Array([
        ...__tlv(0x06, pssOid),
        ...__tlv(0x30, Buffer.from(paramsB64, "base64")),
      ]));
  const top = __derRead(normDer, 0);
  const kids = __derChildren(top.body);
  if (kind === "public") {
    return Buffer.from(__tlv(0x30, new Uint8Array([
      ...alg, ...__tlv(0x03, kids[1].body),
    ])));
  }
  return Buffer.from(__tlv(0x30, new Uint8Array([
    ...__tlv(0x02, kids[0].body), ...alg, ...__tlv(0x04, kids[2].body),
  ])));
}
function __mlSpki(akt, ek) {
  const oid = Buffer.from(__ML_SETS[akt][0], "hex");
  const alg = __tlv(0x30, __tlv(0x06, oid));
  const bit = new Uint8Array([0, ...ek]);
  return __tlv(0x30, new Uint8Array([...alg, ...__tlv(0x03, bit)]));
}
function __mlSeedPkcs8(akt, seed) {
  const oid = Buffer.from(__ML_SETS[akt][0], "hex");
  const alg = __tlv(0x30, __tlv(0x06, oid));
  const oct = __tlv(0x04, __tlv(0x80, seed));
  const zero = __tlv(0x02, new Uint8Array([0]));
  return __tlv(0x30, new Uint8Array([...zero, ...alg, ...oct]));
}
function __pemDecode(text) {
  const m = String(text).match(/-----BEGIN ([^-]+)-----([\s\S]*?)-----END \1-----/);
  if (!m) return null;
  // 10f crypto二轮：RFC1421 头行（Proc-Type/DEK-Info）跳过记取，空行跳过。
  let dek = null;
  const b64 = [];
  for (let line of m[2].split("\n")) {
    line = line.trim();
    if (line === "") continue;
    if (line.startsWith("Proc-Type:")) continue;
    if (line.startsWith("DEK-Info:")) {
      const rest = line.slice(9).trim();
      const comma = rest.indexOf(",");
      if (comma < 0) return null;
      dek = { cipher: rest.slice(0, comma).trim(), ivHex: rest.slice(comma + 1).trim() };
      continue;
    }
    b64.push(line.replace(/\s+/g, ""));
  }
  return { label: m[1].trim(), der: __b64dec(b64.join("")), dek };
}
function __pemEncode(label, der) {
  const b64 = __b64enc(der);
  let body = "";
  for (let i = 0; i < b64.length; i += 64) body += b64.slice(i, i + 64) + "\n";
  return `-----BEGIN ${label}-----\n${body}-----END ${label}-----\n`;
}
// 10f crypto二轮：传统 OpenSSL 加密 PEM（EVP_BytesToKey/MD5 + AES-CBC；轮子全在树内）。
// 引擎原文逐字（缺口令/坏解码）见各抛点注释——本仓报 openssl 3.x 版，与真机同条件对拍。
function __md5bytes(data) {
  const id = Number(__cryptCall(() => __wjs_crypto_hash_new("md5", "0")));
  __cryptCall(() => __wjs_crypto_hash_update(String(id), Buffer.from(data)));
  return Buffer.from(__cryptCall(() => __wjs_crypto_hash_digest(String(id))));
}
function __evpBytesToKey(pass, salt8, keyLen) {
  let out = Buffer.alloc(0), prev = Buffer.alloc(0);
  while (out.length < keyLen) {
    prev = __md5bytes(Buffer.concat([prev, Buffer.from(pass), Buffer.from(salt8)]));
    out = Buffer.concat([out, prev]);
  }
  return out.slice(0, keyLen);
}
function __pemCipherTable() {
  return {
    "AES-128-CBC": { native: "aes-128-cbc", keyLen: 16, ivLen: 16 },
    "AES-256-CBC": { native: "aes-256-cbc", keyLen: 32, ivLen: 16 },
    "DES-EDE3-CBC": { native: "des-ede3-cbc", keyLen: 24, ivLen: 8 },
  };
}
function __pemPassBytes(pass) {
  if (typeof pass === "string") return Buffer.from(pass, "utf8");
  if (pass instanceof Uint8Array) return Buffer.from(pass.buffer, pass.byteOffset, pass.byteLength);
  if (pass instanceof ArrayBuffer) return Buffer.from(pass);
  if (ArrayBuffer.isView(pass)) return Buffer.from(pass.buffer, pass.byteOffset, pass.byteLength);
  const err = new TypeError("The passphrase argument must be of type string or an instance of Buffer, TypedArray, or DataView");
  err.code = "ERR_INVALID_ARG_TYPE";
  throw err;
}
function __pemMissingPassphrase() {
  // 缺口令：openssl 3.x 原文（本仓报 3.x 版；1.x 的 ERR_MISSING_PASSPHRASE 见 plan3 记档）。
  const err = new Error("error:07880109:common libcrypto routines::interrupted or cancelled");
  err.code = "ERR_MISSING_PASSPHRASE";
  throw err;
}
function __pemBadDecrypt() {
  // 错口令/坏块：openssl 3.x 原文（套件 message 点名）。
  const err = new Error("error:1C800064:Provider routines::bad decrypt");
  err.code = "ERR_OSSL_BAD_DECRYPT";
  throw err;
}
function __osslInterrupted() {
  // 缺口令/超长口令：openssl 3.x 口径（`hasOpenSSL(3)` 为 true 的分支；
  // 1.x 的 ERR_MISSING_PASSPHRASE 见既有 `__pemMissingPassphrase` 记档）。
  const err = new Error("error:07880109:common libcrypto routines::interrupted or cancelled");
  err.code = "ERR_OSSL_CRYPTO_INTERRUPTED_OR_CANCELLED";
  throw err;
}
// 10f crypto六轮：PBES2 解密（`ENCRYPTED PRIVATE KEY`；PBKDF2 + AES-CBC/
// DES-EDE3，树内轮子零新增：`__wjs_kdf_pbkdf2` + cipher natives）。
// 错误一律 OpenSSL 3.x 形：缺口令/口令超 1024B → INTERRUPTED；
// 口令类型错 → ARG_TYPE（`__pemPassBytes`）；解密失败 → BAD_DECRYPT；
// 非 PBKDF2/AES/DES-EDE3 参数 → ERR_OSSL_UNSUPPORTED（暂定口径，无 fixture 覆盖）。
function __pbes2Decrypt(der, options) {
  const pass = options?.passphrase;
  if (pass === undefined) __osslInterrupted();
  const passB = __pemPassBytes(pass);
  if (passB.length > 1024) __osslInterrupted();
  const unsup = () => {
    const err = new Error("Unsupported PBES2 parameters");
    err.code = "ERR_OSSL_UNSUPPORTED";
    throw err;
  };
  let outer;
  try {
    const top = __derRead(der, 0);
    if (top.tag !== 0x30) unsup();
    outer = __derChildren(top.body);
  } catch (e) { if (e && e.code) throw e; unsup(); }
  if (!outer || outer.length !== 2 || outer[0].tag !== 0x30 || outer[1].tag !== 0x04) unsup();
  let algId, ct;
  try {
    algId = __derChildren(outer[0].body);
    ct = outer[1].body;
  } catch { unsup(); }
  if (algId.length !== 2 || algId[0].tag !== 0x06 ||
      Buffer.from(algId[0].body).toString("hex") !== "2a864886f70d01050d") unsup();
  let params;
  try { params = __derChildren(algId[1].body); } catch { unsup(); }
  if (params.length !== 2 || params[0].tag !== 0x30 || params[1].tag !== 0x30) unsup();
  // KDF：PBKDF2（SEQ{ OID, SEQ{ salt OCTET, iter INT, [prf SEQ] } }，
  // PRF 缺省 SHA-1）。
  let kdf;
  try { kdf = __derChildren(params[0].body); } catch { unsup(); }
  if (kdf.length !== 2 || kdf[0].tag !== 0x06 ||
      Buffer.from(kdf[0].body).toString("hex") !== "2a864886f70d01050c") unsup();
  let prf;
  try { prf = __derChildren(kdf[1].body); } catch { unsup(); }
  if ((prf.length !== 2 && prf.length !== 3) || prf[0].tag !== 0x04 || prf[1].tag !== 0x02) unsup();
  let hash = "SHA-1";
  if (prf.length === 3) {
    if (prf[2].tag !== 0x30) unsup();
    let prfSeq;
    try { prfSeq = __derChildren(prf[2].body); } catch { unsup(); }
    // PRF AlgorithmIdentifier = SEQ{ OID, [NULL] }（RFC 8018，套件带 NULL 两元）。
    const oidHex = (prfSeq.length >= 1 && prfSeq[0].tag === 0x06)
      ? Buffer.from(prfSeq[0].body).toString("hex") : null;
    hash = oidHex === "2a864886f70d0207" ? "SHA-1"
      : oidHex === "2a864886f70d0209" ? "SHA-256"
      : oidHex === "2a864886f70d020a" ? "SHA-384"
      : oidHex === "2a864886f70d020b" ? "SHA-512" : null;
    if (hash === null) unsup();
  }
  // ENC：AES-128/192/256-CBC 或 DES-EDE3-CBC（SEQ{ OID, OCTET iv }）。
  let enc;
  try { enc = __derChildren(params[1].body); } catch { unsup(); }
  if (enc.length !== 2 || enc[0].tag !== 0x06 || enc[1].tag !== 0x04) unsup();
  const encHex = Buffer.from(enc[0].body).toString("hex");
  const encEntry = encHex === "608648016503040102" ? { native: "aes-128-cbc", keyLen: 16, ivLen: 16 }
    : encHex === "608648016503040116" ? { native: "aes-192-cbc", keyLen: 24, ivLen: 16 }
    : encHex === "60864801650304012a" ? { native: "aes-256-cbc", keyLen: 32, ivLen: 16 }
    : encHex === "2a864886f70d0307" ? { native: "des-ede3-cbc", keyLen: 24, ivLen: 8 }
    : null;
  if (!encEntry || enc[1].body.length !== encEntry.ivLen) unsup();
  // iter：DER INT 转数（大数截断记档；套件 2048）。
  let iter = 0;
  for (const b of prf[1].body) iter = iter * 256 + b;
  const salt = Buffer.from(prf[0].body);
  const key = Buffer.from(__cryptCall(() =>
    __wjs_kdf_pbkdf2(hash, Buffer.from(passB), salt, iter, encEntry.keyLen)));
  try {
    const id = Number(__cryptCall(() =>
      __wjs_cipher_new(encEntry.native, key, Buffer.from(enc[1].body), 0, 1)));
    const head = __cryptCall(() => __wjs_cipher_update(String(id), Buffer.from(ct)));
    const tail = __cryptCall(() => __wjs_cipher_final(String(id)));
    const pt = Buffer.concat([Buffer.from(head), Buffer.from(tail)]);
    return pt;
  } catch {
    __pemBadDecrypt();
  }
}
function __pemEncryptTraditional(der, label, options) {
  const table = __pemCipherTable();
  const c = table[String(options.cipher).toUpperCase()];
  if (!c) {
    const err = new Error("Unknown cipher");
    err.code = "ERR_CRYPTO_UNKNOWN_CIPHER";
    throw err;
  }
  if (options.passphrase === undefined) __pemMissingPassphrase();
  const passB = __pemPassBytes(options.passphrase);
  const iv = __randFill(new Uint8Array(c.ivLen));
  const key = __evpBytesToKey(passB, iv.slice(0, 8), c.keyLen);
  const id = Number(__cryptCall(() => __wjs_cipher_new(c.native, Buffer.from(key), Buffer.from(iv), 1, 1)));
  const head = __cryptCall(() => __wjs_cipher_update(String(id), Buffer.from(der)));
  const tail = __cryptCall(() => __wjs_cipher_final(String(id)));
  const ct = Buffer.concat([Buffer.from(head), Buffer.from(tail)]);
  const b64 = ct.toString("base64");
  let body = "";
  for (let i = 0; i < b64.length; i += 64) body += b64.slice(i, i + 64) + "\n";
  const ivHex = Buffer.from(iv).toString("hex").toUpperCase();
  const dekName = String(options.cipher).toUpperCase();
  return `-----BEGIN ${label}-----\nProc-Type: 4,ENCRYPTED\nDEK-Info: ${dekName},${ivHex}\n\n${body}-----END ${label}-----\n`;
}
function __pemDecryptTraditional(pem, options) {
  const table = __pemCipherTable();
  const c = pem.dek && table[String(pem.dek.cipher).toUpperCase()];
  if (!c) {
    const err = new Error("Unknown cipher");
    err.code = "ERR_CRYPTO_UNKNOWN_CIPHER";
    throw err;
  }
  if (!options || options.passphrase === undefined) __pemMissingPassphrase();
  const passB = __pemPassBytes(options.passphrase);
  let iv;
  try {
    iv = Buffer.from(pem.dek.ivHex, "hex");
  } catch { __pemBadDecrypt(); }
  if (iv.length !== c.ivLen) __pemBadDecrypt();
  const key = __evpBytesToKey(passB, iv.slice(0, 8), c.keyLen);
  try {
    const id = Number(__cryptCall(() => __wjs_cipher_new(c.native, Buffer.from(key), Buffer.from(iv), 0, 1)));
    const head = __cryptCall(() => __wjs_cipher_update(String(id), Buffer.from(pem.der)));
    const tail = __cryptCall(() => __wjs_cipher_final(String(id)));
    return Buffer.concat([Buffer.from(head), Buffer.from(tail)]);
  } catch {
    __pemBadDecrypt();
  }
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
  const Cls = kind === "secret" ? SecretKeyObject : kind === "public" ? PublicKeyObject : PrivateKeyObject;
  const k = new Cls(kind, "dsa", Buffer.from(JSON.stringify(env)));
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
    "SHA1": "SHA-1", "DSS1": "SHA-1", "SHA256": "SHA-256", "SHA384": "SHA-384", "SHA512": "SHA-512",
    "MD5": "MD5", "SHA3256": "SHA3-256", "SHA3384": "SHA3-384", "SHA3512": "SHA3-512",
  };
  return table[bare];
}

// 10f crypto二轮：KeyObject 四层原型链（真机口径）+ WeakMap 状态（实例零自有属性）。
// 链：SecretKeyObject→KeyObject；PublicKeyObject→AsymmetricKeyObject→KeyObject
// （PrivateKeyObject 同）。`__kind` 系经原型访问器读写，旧 `x.__foo` 文本零改动；
// 品牌 = WeakMap 成员（原型伪造/自有属性伪造一律不认，§4.23 同款）。
const __koState = new WeakMap();
function __koBrand(o) {
  if ((typeof o !== "object" && typeof o !== "function") || o === null) return null;
  return __koState.get(o) ?? null;
}
function __isKeyObject(o) {
  return KeyObject.__brandCheck(o);
}
function __invalidThis() {
  const err = new TypeError('Value of "this" must be of type KeyObject');
  err.code = "ERR_INVALID_THIS";
  throw err;
}
function __koReceived(v) {
  if (v === null) return "null";
  if (v === undefined) return "undefined";
  const t = typeof v;
  if (t === "object" || t === "function") return `an instance of ${v.constructor?.name ?? "Object"}`;
  return `type ${t} (${String(v)})`;
}
class KeyObject {
  // 品牌静态检查（供 `util.types.isKeyObject`；手动走链，`Symbol.hasInstance`
  // 覆盖期同样有效）。
  static __brandCheck(o) {
    if (__koBrand(o) === null) return false;
    let p = Object.getPrototypeOf(Object(o));
    while (p !== null) {
      if (p === KeyObject.prototype) return true;
      p = Object.getPrototypeOf(p);
    }
    return false;
  }
  // 10f crypto二轮：`KeyObject.from`（CryptoKey 互转；宿主无 CryptoKey 品牌，
  // 非法输入口径先行，有效输入另案）。
  static from(key) {
    const recv = typeof key === "string" ? `type string ('${key}')`
      : key === null ? "null"
      : key === undefined ? "undefined"
      : `an instance of ${key.constructor?.name ?? "Object"}`;
    const err = new TypeError(
      `The "key" argument must be an instance of CryptoKey. Received ${recv}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  constructor(kind, keyType, material) {
    // 10f crypto二轮：直接构造校验（真机口径；子类内部构造经 new.target 放行）。
    if (new.target === KeyObject) {
      if (kind !== "secret" && kind !== "public" && kind !== "private") {
        const recv = kind === undefined ? "undefined"
          : typeof kind === "string" ? `'${kind}'`
          : String(kind);
        const err = new TypeError(`The argument 'type' is invalid. Received ${recv}`);
        err.code = "ERR_INVALID_ARG_VALUE";
        throw err;
      }
      // handle 须为原生句柄对象，用户值恒非法。
      const hrecv = keyType === undefined ? "undefined"
        : typeof keyType === "string" ? `type string ('${keyType}')`
        : `an instance of ${keyType.constructor?.name ?? "Object"}`;
      const herr = new TypeError(
        `The "handle" argument must be of type object. Received ${hrecv}`);
      herr.code = "ERR_INVALID_ARG_TYPE";
      throw herr;
    }
    __koState.set(this, { kind: undefined, keyType: undefined, material: undefined, detail: null });
    this.__kind = kind;
    this.__keyType = keyType;
    this.__material = material;
  }
  get type() {
    const s = __koBrand(this);
    if (s === null) __invalidThis();
    return s.kind;
  }
  // 10f crypto二轮：`Object.prototype.toString` 口径（全 flavor 同一）。
  get [Symbol.toStringTag]() { return "KeyObject"; }
  export(options) {
    const s = __koBrand(this);
    if (s === null) __invalidThis();
    // 真机口径：secret 无参即回裸 Buffer；其余 options 须为对象。
    if (s.kind === "secret" && options === undefined) return Buffer.from(s.material);
    if (typeof options !== "object" || options === null) {
      const recv = options === undefined ? "undefined"
        : options === null ? "null"
        : `type ${typeof options} (${typeof options === "string" ? `'${options}'` : String(options)})`;
      const err = new TypeError(`The "options" argument must be of type object. Received ${recv}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    // 10f crypto四轮：format 门（真机 26 逐项）——secret ∈ {undefined→buffer,
    // 'buffer', 'jwk'}；非对称 ∈ {'pem','der','jwk','raw-private','raw-public',
    // 'raw-seed'}，其余（含 undefined/'buffer'）→ ARG_VALUE 'options.format'
    // is invalid。
    const format = options?.format;
    const __recvArg = (v) => v === undefined ? "undefined"
      : v === null ? "null"
      : typeof v === "string" ? `'${v}'`
      : `type ${typeof v} (${String(v)})`;
    if (s.kind === "secret") {
      if (format !== undefined && format !== "buffer" && format !== "jwk") {
        const err = new TypeError(
          `The property 'options.format' must be one of: undefined, 'buffer', 'jwk'. Received ${__recvArg(format)}`);
        err.code = "ERR_INVALID_ARG_VALUE";
        throw err;
      }
      if (format === "jwk") return __exportJwk(this);
      return Buffer.from(s.material);
    }
    // 10f crypto五轮：私钥加密门（真机 26 逐项）——passphrase 有即只有 pem/der
    // 能加密；jwk/raw-*/未知/缺 format 一律 INCOMPATIBLE
    // 'The selected key encoding <format> does not support encryption.'，
    // 先于 format/type 门（'banana'+pp 同错，undefined→"undefined"）；
    // 公钥/secret 侧忽略 passphrase；cipher 单给忽略（须 passphrase 才触发）。
    if (s.kind === "private" && options?.passphrase !== undefined &&
        format !== "pem" && format !== "der") {
      const err = new Error(
        `The selected key encoding ${String(format)} does not support encryption.`);
      err.code = "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS";
      throw err;
    }
    if (format !== "pem" && format !== "der" && format !== "jwk" &&
        format !== "raw-private" && format !== "raw-public" && format !== "raw-seed") {
      const err = new TypeError(`The property 'options.format' is invalid. Received ${__recvArg(format)}`);
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    // 10f crypto六轮：私钥加密导出缺 cipher 门（真机 26 逐项；公钥/secret 侧
    // 忽略 passphrase，见五轮探针）。
    if (s.kind === "private" && options?.passphrase !== undefined && options?.cipher === undefined &&
        (format === "pem" || format === "der")) {
      const err = new TypeError(
        "The property 'options.cipher' is required when a passphrase is specified. Received undefined");
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    if (format === "jwk") return __exportJwk(this);
    // 10f X448：OKP raw 格式（真机口径：裸料 Buffer；kind 错位 → format 无效，
    // 非 OKP → INCOMPATIBLE；真机 26 逐项）。10f 四轮：EC raw 同门——
    // raw-private = 定长标量（曲线字节长）、raw-public = 非压缩点 04||X||Y。
    // 10f crypto五轮：raw-seed 同为私钥专属（公钥侧 ARG_VALUE，真机同），
    // 私钥 raw-seed 走 ml-seed / blanket INCOMPATIBLE。
    if (format === "raw-private" || format === "raw-public" || format === "raw-seed") {
      const wantKind = format === "raw-public" ? "public" : "private";
      if (s.kind !== wantKind) {
        const err = new TypeError(`The property 'options.format' is invalid. Received '${format}'`);
        err.code = "ERR_INVALID_ARG_VALUE";
        throw err;
      }
      // 10f crypto五轮：ml raw 导出（真机 26 口径）——公钥 raw-public = SPKI
      // BIT STRING 裸料（ml-kem-768 1184B 等）；私钥 raw-seed = PKCS#8 种子
      //（既有 seed_from_pkcs8 natives，无新增）；其余组合 INCOMPATIBLE。
      if (s.keyType.startsWith("ml-kem-") || s.keyType.startsWith("ml-dsa-")) {
        const isKem = s.keyType.startsWith("ml-kem-");
        if (format === "raw-public" && s.kind === "public") {
          try {
            const top = __derRead(s.material, 0);
            const kids = __derChildren(top.body);
            return Buffer.from(kids[1].body.subarray(1));
          } catch {
            const err = new Error("The selected key encoding is incompatible with the key type");
            err.code = "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS";
            throw err;
          }
        }
        if (format === "raw-seed" && s.kind === "private") {
          const parts = JSON.parse(__cryptCall(() => (isKem
            ? __wjs_mlkem_seed_from_pkcs8(s.material)
            : __wjs_mldsa_seed_from_pkcs8(s.material))));
          return Buffer.from(__b64dec(parts.seed));
        }
        const err = new Error("The selected key encoding is incompatible with the key type");
        err.code = "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS";
        throw err;
      }
      // 10f crypto五轮：slh raw 导出（装载期已验尺寸，material 即裸料直返）。
      if ((s.keyType === "slh-dsa-sha2-128f" || s.keyType === "slh-dsa-sha2-192f") &&
          format !== "raw-seed") {
        return Buffer.from(s.material);
      }
      // 10f crypto五轮：raw-seed 兜底（ml 私钥已上处理；EC/OKP/slh 不得下漏
      // 取料——ec 私钥料调 jwk_pub 即 DataError 原形毕露）。
      if (format === "raw-seed") {
        const err = new Error("The selected key encoding is incompatible with the key type");
        err.code = "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS";
        throw err;
      }
      if (s.keyType === "ec") {
        const curve = s.detail?.namedCurve;
        if (typeof curve !== "string") {
          const err = new Error("The selected key encoding is incompatible with the key type");
          err.code = "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS";
          throw err;
        }
        if (format === "raw-private") {
          const pubDer = __cryptCall(() => __wjs_ec_public(curve, s.material));
          const parts = JSON.parse(__cryptCall(() => __wjs_ec_jwk(curve, s.material, pubDer)));
          return Buffer.from(__b64urlDec(parts.d));
        }
        // 10f crypto五轮：raw-public 的 type 选项（真机 26 口径）——缺省/
        // uncompressed 回 65B 非压缩；compressed 回 33B（y 末字节奇偶定前缀）；
        // 其余一律 ARG_VALUE（inspect 小口径回显）；raw-private 无视 type。
        const t = options?.type;
        if (t !== undefined && t !== "uncompressed" && t !== "compressed") {
          const err = new TypeError(
            `The property 'options.type' must be one of: 'compressed', 'uncompressed'. Received ${__inspectSmall(t)}`);
          err.code = "ERR_INVALID_ARG_VALUE";
          throw err;
        }
        const parts = JSON.parse(__cryptCall(() => __wjs_ec_jwk_pub(curve, s.material)));
        const x = __b64urlDec(parts.x), y = __b64urlDec(parts.y);
        if (t === "compressed") return Buffer.concat([Buffer.from([(y[y.length - 1] & 1) ? 3 : 2]), x]);
        return Buffer.concat([Buffer.from([4]), x, y]);
      }
      if (!["ed25519", "x25519", "x448", "ed448"].includes(s.keyType)) {
        const err = new Error("The selected key encoding is incompatible with the key type");
        err.code = "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS";
        throw err;
      }
      return Buffer.from(s.material);
    }
    // 10f 四轮：pem/der type 门矩阵（真机 26 逐项）——未知/缺 type →
    // ARG_VALUE 'options.type' is invalid；kind 错位（public+pkcs8/sec1、
    // private+spki）→ ARG_VALUE 同形；家族错位（pkcs1 非 RSA、sec1 非 EC 私钥）
    // → INCOMPATIBLE 'can only be used for … keys.'。
    const t = options?.type;
    if (t !== "pkcs1" && t !== "spki" && t !== "pkcs8" && t !== "sec1") {
      const err = new TypeError(`The property 'options.type' is invalid. Received ${__recvArg(t)}`);
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    if (t === "pkcs1" && s.keyType !== "rsa") {
      const err = new Error("The selected key encoding pkcs1 can only be used for RSA keys.");
      err.code = "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS";
      throw err;
    }
    if (t === "sec1" && s.kind === "private" && s.keyType !== "ec") {
      const err = new Error("The selected key encoding sec1 can only be used for EC keys.");
      err.code = "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS";
      throw err;
    }
    if ((t === "pkcs8" || t === "sec1") && s.kind === "public") {
      const err = new TypeError(`The property 'options.type' is invalid. Received '${t}'`);
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    if (t === "spki" && s.kind === "private") {
      const err = new TypeError("The property 'options.type' is invalid. Received 'spki'");
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    // 10f 四轮：EC 私钥 sec1（SEQ{ INT 1, OCTET d, [0] OID, [1] BITSTRING 点 }，
    // 真机逐字节口径；导入侧同构，见 __parseKeyMaterial sec1）。
    if (s.keyType === "ec" && t === "sec1") {
      const der = __ecSec1Der(s);
      if (format === "der") return Buffer.from(der);
      if (options.cipher !== undefined) {
        return __pemEncryptTraditional(der, "EC PRIVATE KEY", options);
      }
      return __pemEncode("EC PRIVATE KEY", der);
    }
    // 10f crypto二轮：RSA pkcs1（RSAPublicKey/RSAPrivateKey，真机口径）。
    if (s.keyType === "rsa" && t === "pkcs1") {
      const der = __rsaPkcs1(s);
      if (format === "der") return Buffer.from(der);
      if (format === "pem") {
        const label = s.kind === "private" ? "RSA PRIVATE KEY" : "RSA PUBLIC KEY";
        // 10f crypto二轮：传统加密 PEM（cipher+passphrase）。
        if (options.cipher !== undefined && s.kind === "private") {
          return __pemEncryptTraditional(der, label, options);
        }
        return __pemEncode(label, der);
      }
    }
    let der = __exportDer(this, options);
    // 10f crypto六轮：rsa-pss 回贴（归一化存料→PSS 线形，见 __pssReattach；
    // 生成键无 stash 即裸 OID，真机同形）。
    if (s.keyType === "rsa-pss") {
      der = __pssReattach(s.kind, der, s.detail);
    }
    if (format === "der") return Buffer.from(der);
    if (format === "pem") {
      // 10f crypto二轮：pkcs8 私钥加密导出（dsa-legacy 沿 label）。
      if (options.cipher !== undefined && s.kind === "private") {
        const label = t === "pkcs1" ? "RSA PRIVATE KEY" : "PRIVATE KEY";
        return __pemEncryptTraditional(der, label, options);
      }
      const label = s.kind === "private" ? "PRIVATE KEY" : "PUBLIC KEY";
      return __pemEncode(label, der);
    }
    const err = new TypeError(`Unknown export format ${format}`);
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
  equals(other) {
    const a = __koBrand(this);
    if (a === null) __invalidThis();
    if (!__isKeyObject(other)) {
      const err = new TypeError(
        `The "otherKeyObject" argument must be an instance of KeyObject. Received ${__koReceived(other)}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    const b = __koBrand(other);
    return a.kind === b.kind && a.keyType === b.keyType &&
      Buffer.from(a.material).equals(Buffer.from(b.material));
  }
}
class SecretKeyObject extends KeyObject {
  get symmetricKeySize() {
    const s = __koBrand(this);
    if (s === null || s.kind !== "secret") __invalidThis();
    return s.material.length;
  }
}
class AsymmetricKeyObject extends KeyObject {
  get asymmetricKeyType() {
    const s = __koBrand(this);
    if (s === null || s.kind === "secret") __invalidThis();
    return s.keyType;
  }
  get asymmetricKeyDetails() {
    const s = __koBrand(this);
    if (s === null || s.kind === "secret") __invalidThis();
    // RSA/DSA 系回 {modulusLength, publicExponent|divisorLength}（generation
    // 期落 __detail；RSA 导入键无 __detail 即从材料现算）。键按有无拼装
    //（DSA 无 publicExponent；undefined 值键 deepStrictEqual 判不等）。
    if ((s.keyType === "rsa" || s.keyType === "rsa-pss" || s.keyType === "dsa") &&
        s.detail !== null && typeof s.detail === "object" &&
        typeof s.detail.modulusLength === "number") {
      const out = { modulusLength: s.detail.modulusLength };
      if (s.detail.publicExponent !== undefined) out.publicExponent = BigInt(s.detail.publicExponent);
      if (s.detail.divisorLength !== undefined) out.divisorLength = s.detail.divisorLength;
      // 10f crypto六轮：rsa-pss 约束面（有即透传，无即不拼——deepStrictEqual 精确形）。
      if (s.keyType === "rsa-pss") {
        for (const rk of ["hashAlgorithm", "mgf1HashAlgorithm", "saltLength"]) {
          if (s.detail[rk] !== undefined) out[rk] = s.detail[rk];
        }
      }
      return out;
    }
    if (s.keyType === "rsa" || s.keyType === "rsa-pss") {
      const d = __rsaDetailsFromMaterial(s.kind, s.material);
      if (d !== null) return d;
    }
    // 10f 四轮（真机 26 逐项）：EC → { namedCurve: <OpenSSL 名> }
    //（prime256v1 形，非 JWK 名）；OKP → {}（空对象，非 undefined）。
    if (s.keyType === "ec") {
      const c = s.detail?.namedCurve;
      if (typeof c !== "string") return undefined;
      return { namedCurve: __EC_OPENSSL_NAMES[c] ?? c };
    }
    if (s.keyType === "ed25519" || s.keyType === "x25519" ||
        s.keyType === "x448" || s.keyType === "ed448") {
      return {};
    }
    return undefined;
  }
}
// 10f crypto二轮：RSA 材料现算 details（private 取 PKCS#8 内层 n/e；
// public 取 SPKI 内层 n/e；坏料回 null）。
function __rsaDetailsFromMaterial(kind, material) {
  try {
    const top = __derRead(material, 0);
    if (top.tag !== 48) return null;
    const kids = __derChildren(top.body);
    const last = kids[kids.length - 1];
    let seq;
    if (kind === "private") {
      if (last.tag !== 4) return null;
      seq = __derChildren(__derRead(last.body, 0).body);
      if (seq.length < 3) return null;
      seq = [seq[1], seq[2]];
    } else if (kind === "public") {
      if (last.tag !== 3 || last.body.length < 1 || last.body[0] !== 0) return null;
      seq = __derChildren(__derRead(last.body.slice(1), 0).body);
      if (seq.length < 2) return null;
    } else {
      return null;
    }
    const strip = (t) => {
      let v = t.body;
      while (v.length > 1 && v[0] === 0) v = v.slice(1);
      return v;
    };
    const n = strip(seq[0]), e = strip(seq[1]);
    let exp = 0n;
    for (const b of e) exp = (exp << 8n) | BigInt(b);
    return { modulusLength: n.length * 8, publicExponent: exp };
  } catch {
    return null;
  }
}
class PublicKeyObject extends AsymmetricKeyObject {}
class PrivateKeyObject extends AsymmetricKeyObject {}
// `__kind` 系原型访问器（实例零自有属性；`Object(this)` 防原始值 receiver 抛错）。
for (const __k of ["__kind", "__keyType", "__material", "__detail"]) {
  Object.defineProperty(KeyObject.prototype, __k, {
    configurable: true,
    get() { return __koState.get(Object(this))?.[__k.slice(2)]; },
    set(v) { const s = __koState.get(Object(this)); if (s !== undefined) s[__k.slice(2)] = v; },
  });
}
// 10f 四轮：EC 私钥 sec1 DER 构造（真机逐字节口径；OID 表与导入侧 sec1 同源）。
function __ecSec1Der(s) {
  const curve = s.detail?.namedCurve;
  const oids = {
    "P-256": "2a8648ce3d030107", "P-384": "2b81040022",
    "P-521": "2b81040023", "secp256k1": "2b8104000a",
  };
  const oidHex = oids[curve];
  if (oidHex === undefined) {
    const err = new Error("Invalid EC key material for sec1 export");
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
  const pubDer = __cryptCall(() => __wjs_ec_public(curve, s.material));
  const jwk = JSON.parse(__cryptCall(() => __wjs_ec_jwk(curve, s.material, pubDer)));
  const d = __b64urlDec(jwk.d);
  const point = Buffer.concat([Buffer.from([4]), __b64urlDec(jwk.x), __b64urlDec(jwk.y)]);
  const oid = Buffer.from(oidHex, "hex");
  const oidTlv = Buffer.concat([Buffer.from([6, oid.length]), oid]);
  const bitStr = Buffer.concat([Buffer.from([3, ...__derLen(point.length + 1), 0]), point]);
  const inner = Buffer.concat([
    __derInt(new Uint8Array([1])),
    Buffer.concat([Buffer.from([4, ...__derLen(d.length)]), d]),
    Buffer.concat([Buffer.from([160, oidTlv.length]), oidTlv]),
    Buffer.concat([Buffer.from([161, ...__derLen(bitStr.length)]), bitStr]),
  ]);
  return Buffer.concat([Buffer.from([48, ...__derLen(inner.length)]), inner]);
}
