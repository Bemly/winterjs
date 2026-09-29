// 10f 四轮：曲线名表——内部统一存 JWK 名（crv）。真机 26：曲线名查找
// JWK 与 raw 同一套（OpenSSL 名族：'P-256' 与 'prime256v1' 均收，'secp256r1'
// /'banana' 拒 → INVALID_CURVE 'Invalid EC curve name'）。
const __EC_CURVES = {
  "prime256v1": "P-256", "P-256": "P-256",
  "secp384r1": "P-384", "P-384": "P-384",
  "secp521r1": "P-521", "P-521": "P-521",
  "secp256k1": "secp256k1",
};
const __EC_OPENSSL_NAMES = {
  "P-256": "prime256v1", "P-384": "secp384r1", "P-521": "secp521r1", "secp256k1": "secp256k1",
};
function __badEcCurve() {
  const err = new Error("Invalid EC curve name");
  err.code = "ERR_CRYPTO_INVALID_CURVE";
  throw err;
}
function __ecCurve(name) {
  const c = __EC_CURVES[name];
  if (c === undefined) __badEcCurve();
  return c;
}
// 10f crypto二轮：RSA pkcs1 内层提取（public 取 SPKI 的 BITSTRING 内层；
// private 取 PKCS#8 的 OCTET 内层，即 RSAPrivateKey 本体）。
function __rsaPkcs1(s) {
  const fail = () => {
    const err = new Error("Invalid RSA key material for pkcs1 export");
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  };
  let top;
  try {
    top = __derRead(s.material, 0);
  } catch { fail(); }
  if (top.tag !== 48) fail();
  const kids = __derChildren(top.body);
  const last = kids[kids.length - 1];
  if (s.kind === "public") {
    if (last.tag !== 3 || last.body.length < 1 || last.body[0] !== 0) fail();
    return last.body.slice(1);
  }
  if (last.tag !== 4) fail();
  return last.body;
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
  // OKP 私钥是裸 seed（非 DER），导出时包 PKCS#8/SPKI（真机口径；
  // 9e 存量曾直吐裸料，10e 起修正，ed25519/x25519/x448/ed448 同路径）。
  // 类型严格（真机口径：private 只收 pkcs8、public 只收 spki；sec1 私钥另码）。
  if (kobj.__keyType === "ed25519" || kobj.__keyType === "x25519" || kobj.__keyType === "x448" || kobj.__keyType === "ed448") {
    const want = kobj.__kind === "private" ? "pkcs8" : "spki";
    const t = options?.type;
    if (t !== undefined && t !== want) {
      if (t === "sec1" && kobj.__kind === "private") {
        const err = new Error("Incompatible key options");
        err.code = "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS";
        throw err;
      }
      const err = new TypeError(`Unknown export type ${t}`);
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    const kind = kobj.__keyType === "ed25519" ? "ED25519"
      : kobj.__keyType === "x25519" ? "X25519"
      : kobj.__keyType === "x448" ? "X448"
      : "ED448";
    if (kobj.__kind === "private") {
      return Buffer.from(__cryptCall(() => __wjs_okp_pkcs8_from_seed(kind, kobj.__material)));
    }
    return Buffer.from(__cryptCall(() => __wjs_okp_spki_from_pub(kind, kobj.__material)));
  }
  return kobj.__material;
}
function __exportJwk(kobj) {
  const b64u = (u8) => __b64enc(u8).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  if (kobj.__keyType === "secret") return { kty: "oct", k: b64u(kobj.__material) };
  // 10f crypto六轮：rsa-pss 无 JWK 面（真机逐项，key-objects.js 点名）。
  if (kobj.__keyType === "rsa-pss") {
    const err = new Error("Unsupported JWK Key Type.");
    err.code = "ERR_CRYPTO_JWK_UNSUPPORTED_KEY_TYPE";
    throw err;
  }
  const isPriv = kobj.__kind === "private";
  if (kobj.__keyType === "rsa") {
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
  if (kobj.__keyType === "ed25519" || kobj.__keyType === "x25519" || kobj.__keyType === "x448" || kobj.__keyType === "ed448") {
    // 10e Ed448：OKP crv 同形（57B，b64url）；10f X448（56B）同形。
    const crv = kobj.__keyType === "ed25519" ? "Ed25519"
      : kobj.__keyType === "x25519" ? "X25519"
      : kobj.__keyType === "x448" ? "X448"
      : "Ed448";
    const pubBytes = isPriv
      ? (kobj.__keyType === "ed25519"
        ? __wjs_ed_public(kobj.__material)
        : kobj.__keyType === "x25519"
          ? __wjs_x_public(kobj.__material)
          : kobj.__keyType === "x448"
            ? __wjs_x448_public(kobj.__material)
            : __wjs_ed448_public(kobj.__material))
      : kobj.__material;
    const jwk = { kty: "OKP", crv, x: b64u(pubBytes) };
    if (isPriv) jwk.d = b64u(kobj.__material);
    return jwk;
  }
  if (kobj.__keyType === "dsa") {
    // 10f 四轮（真机口径）：DSA 无 JWK 面（RFC 7518 无 DSA kty）——
    // 导出即 ERR_CRYPTO_JWK_UNSUPPORTED_KEY_TYPE 'Unsupported JWK Key Type.'。
    const err = new Error("Unsupported JWK Key Type.");
    err.code = "ERR_CRYPTO_JWK_UNSUPPORTED_KEY_TYPE";
    throw err;
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
  return new SecretKeyObject("secret", "secret", __cryptBytes(key, "key"));
}
function __parseKeyMaterial(key, format, type, want, options) {
  // → { keyType, material, detail }；want: 'private' | 'public'
  // 10f crypto二轮：KeyObject 互传规则（真机逐字）——public 侧私钥派生公钥，
  // 其余组合按码表拒绝（`createPrivateKey(privKO)` 同禁）。
  if (__isKeyObject(key)) {
    if (want === "public") {
      if (key.type === "private") return __derivePublic(key);
      const err = new TypeError(`Invalid key object type ${key.type}, expected private.`);
      err.code = "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE";
      throw err;
    }
    const err = new TypeError(
      `The "key" argument must be of type string or an instance of ArrayBuffer, Buffer, TypedArray, DataView, or URL. Received an instance of ${key.constructor.name}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  // 10f crypto六轮：`key.format`/`key.type` 入口门（真机 26 逐项）——
  // 显式未知值即 ARG_VALUE（null/数字同判；Received 為 inspect 形：
  // 字符串引号、其余裸值）。type 门仅限 DER 系（pem/der/缺省）路径，
  // jwk/raw 系无视 type（现行行为，套件外记档）。
  const __keyPropReceived = (v) => typeof v === "string" ? `'${v}'`
    : (v !== null && typeof v === "object")
      ? (() => { try { return JSON.stringify(v) ?? String(v); } catch { return String(v); } })()
      : String(v);
  if (format !== undefined &&
      !["pem", "der", "jwk", "raw-private", "raw-public", "raw-seed"].includes(format)) {
    const err = new TypeError(
      `The property 'key.format' is invalid. Received ${__keyPropReceived(format)}`);
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
  if (type !== undefined && (format === undefined || format === "pem" || format === "der") &&
      !["pkcs1", "pkcs8", "spki", "sec1"].includes(type)) {
    const err = new TypeError(
      `The property 'key.type' is invalid. Received ${__keyPropReceived(type)}`);
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
  // 10f crypto六轮：`key.key` 实例渲染（真机 26 逐项；字符串 28 码点截断
  // 与 raw 导入门同规则，函数回 `function <name>`）。
  const __keyReceived = (v) => {
    if (v === null) return "null";
    if (v === undefined) return "undefined";
    if (typeof v === "string") {
      const shown = v.length > 28 ? `${v.slice(0, 25)}...` : v;
      return `type string ('${shown}')`;
    }
    if (typeof v === "function") return `function ${v.name}`;
    if (typeof v === "object") {
      const n = v.constructor && v.constructor.name ? v.constructor.name : "Object";
      return `an instance of ${n}`;
    }
    return `type ${typeof v} (${String(v)})`;
  };
  if (format === "jwk") {
    if (typeof key !== "object" || key === null) {
      const err = new TypeError(
        `The "key.key" property must be of type object. Received ${__keyReceived(key)}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    if (key.kty === "oct") return new SecretKeyObject("secret", "secret", __b64urlDec(key.k));
    if (key.kty === "RSA") {
      const n = __b64urlDec(key.n), e = __b64urlDec(key.e);
      // 10f crypto二轮：JWK 私钥规则（真机逐字）——缺 d 即无私钥料；坏参数即 Invalid；
      // 公钥侧要私钥 JWK 即派生。
      const __badRsaJwk = () => {
        const err = new TypeError("Invalid JWK RSA key");
        err.code = "ERR_CRYPTO_INVALID_JWK";
        throw err;
      };
      if (key.d !== undefined) {
        const d = __b64urlDec(key.d);
        let privDer;
        try {
          privDer = __cryptCall(() => __wjs_rsa_import_priv(n, e, d));
        } catch { __badRsaJwk(); }
        const k = new PrivateKeyObject("private", "rsa", Buffer.from(privDer));
        if (want === "public") return __derivePublic(k);
        return k;
      }
      if (want === "private") {
        const err = new TypeError("JWK does not contain private key material");
        err.code = "ERR_CRYPTO_INVALID_JWK";
        throw err;
      }
      let pubDer;
      try {
        pubDer = __cryptCall(() => __wjs_rsa_import_pub(n, e));
      } catch { __badRsaJwk(); }
      return new PublicKeyObject("public", "rsa", Buffer.from(pubDer));
    }
    if (key.kty === "EC") {
      // 10f 四轮：JWK 校验矩阵（真机 26 逐项）——crv 缺失/非串 → INVALID_JWK
      // 'Invalid JWK EC key'；crv 非法 → INVALID_CURVE 'Invalid EC curve name'；
      // 私钥侧 d 坏/缺、x,y 与派生点不匹、点不在曲线 → INVALID_JWK；
      // public-only 作私钥 → INVALID_JWK 'JWK does not contain private key
      // material'。
      const badEc = () => {
        const err = new TypeError("Invalid JWK EC key");
        err.code = "ERR_CRYPTO_INVALID_JWK";
        throw err;
      };
      const noPriv = () => {
        const err = new TypeError("JWK does not contain private key material");
        err.code = "ERR_CRYPTO_INVALID_JWK";
        throw err;
      };
      if (typeof key.crv !== "string") badEc();
      const curve = __ecCurve(key.crv);
      if (typeof key.x !== "string" || typeof key.y !== "string") badEc();
      let dx, dy;
      try {
        dx = __b64urlDec(key.x);
        dy = __b64urlDec(key.y);
      } catch { badEc(); }
      if (key.d !== undefined) {
        if (typeof key.d !== "string") badEc();
        let privDer, pubDer, pt;
        try {
          privDer = __cryptCall(() => __wjs_ec_import_priv(curve, __b64urlDec(key.d)));
          pubDer = __cryptCall(() => __wjs_ec_public(curve, privDer));
          pt = JSON.parse(__cryptCall(() => __wjs_ec_jwk_pub(curve, pubDer)));
        } catch { badEc(); }
        try {
          if (!Buffer.from(dx).equals(Buffer.from(__b64urlDec(pt.x))) ||
              !Buffer.from(dy).equals(Buffer.from(__b64urlDec(pt.y)))) badEc();
        } catch (e) { if (e && e.code) throw e; badEc(); }
        if (want === "private") {
          const k = new PrivateKeyObject("private", "ec", Buffer.from(privDer));
          k.__detail = { namedCurve: curve };
          return k;
        }
        const k = new PublicKeyObject("public", "ec", Buffer.from(pubDer));
        k.__detail = { namedCurve: curve };
        return k;
      }
      if (want === "private") noPriv();
      let pubDer;
      try {
        pubDer = __cryptCall(() => __wjs_ec_import_pub(curve, dx, dy));
      } catch { badEc(); }
      const k = new PublicKeyObject("public", "ec", Buffer.from(pubDer));
      k.__detail = { namedCurve: curve };
      return k;
    }
    if (key.kty === "DSA") {
      // 10f 四轮（真机口径）：DSA JWK 导入即 INVALID_JWK（RFC 7518 无 DSA）。
      const err = new TypeError("DSA is not a supported JWK key type");
      err.code = "ERR_CRYPTO_INVALID_JWK";
      throw err;
    }
    if (key.kty === "OKP") {
      // 10e Ed448（OKP 同形）；10f X448 同形。10f 四轮：校验矩阵（真机 26
      // 逐项）——crv 缺失/非法 → INVALID_JWK 'Invalid JWK OKP key'（旧
      // NOT_SUPPORTED 文案退役）；d 坏（派生失败）/x 与派生公钥不匹 →
      // INVALID_JWK；public-only 作私钥 → INVALID_JWK 'JWK does not contain
      // private key material'；want public 且带 d → 由 d 派生。
      const badOkp = () => {
        const err = new TypeError("Invalid JWK OKP key");
        err.code = "ERR_CRYPTO_INVALID_JWK";
        throw err;
      };
      const noPriv = () => {
        const err = new TypeError("JWK does not contain private key material");
        err.code = "ERR_CRYPTO_INVALID_JWK";
        throw err;
      };
      const kt = key.crv === "Ed25519" ? "ed25519"
        : key.crv === "X25519" ? "x25519"
        : key.crv === "X448" ? "x448"
        : key.crv === "Ed448" ? "ed448" : null;
      if (kt === null) badOkp();
      const derive = (d) => (kt === "ed25519" ? __wjs_ed_public(d)
        : kt === "x25519" ? __wjs_x_public(d)
        : kt === "x448" ? __wjs_x448_public(d)
        : __wjs_ed448_public(d));
      if (key.d !== undefined) {
        if (typeof key.d !== "string" || typeof key.x !== "string") badOkp();
        let pubBytes, xBytes;
        try {
          pubBytes = __cryptCall(() => derive(__b64urlDec(key.d)));
          xBytes = __b64urlDec(key.x);
        } catch { badOkp(); }
        if (!Buffer.from(xBytes).equals(Buffer.from(pubBytes))) badOkp();
        if (want === "private") return new PrivateKeyObject("private", kt, __b64urlDec(key.d));
        return new PublicKeyObject("public", kt, Buffer.from(pubBytes));
      }
      if (want === "private") noPriv();
      if (typeof key.x !== "string") badOkp();
      let xBytes;
      try { xBytes = __b64urlDec(key.x); } catch { badOkp(); }
      return new PublicKeyObject("public", kt, Buffer.from(xBytes));
    }
    // 10f crypto六轮：PQC AKP JWK（`{kty:'AKP', alg:'ML-DSA-*/ML-KEM-*', priv, pub}`；
    // 真机 26 口径：alg 错/缺 → INVALID_JWK 'Unsupported JWK AKP "alg"'；
    // 料坏/长错/pub 不匹 → INVALID_JWK 'Invalid JWK AKP key'；无 priv 作私钥 →
    // INVALID_JWK 'JWK does not contain private key material'）。
    // 零新 native：种子形 PKCS#8 自拼 + 既有 seed_from_pkcs8 展开派生比对 pub。
    if (key.kty === "AKP") {
      const badAkp = () => {
        const err = new TypeError("Invalid JWK AKP key");
        err.code = "ERR_CRYPTO_INVALID_JWK";
        throw err;
      };
      const noPriv = () => {
        const err = new TypeError("JWK does not contain private key material");
        err.code = "ERR_CRYPTO_INVALID_JWK";
        throw err;
      };
      const algMap = {
        "ML-DSA-44": "ml-dsa-44", "ML-DSA-65": "ml-dsa-65", "ML-DSA-87": "ml-dsa-87",
        "ML-KEM-512": "ml-kem-512", "ML-KEM-768": "ml-kem-768", "ML-KEM-1024": "ml-kem-1024",
      };
      const kind = algMap[key.alg];
      if (kind === undefined) {
        const err = new TypeError('Unsupported JWK AKP "alg"');
        err.code = "ERR_CRYPTO_INVALID_JWK";
        throw err;
      }
      const set = __ML_SETS[kind];
      const isKem = kind.startsWith("ml-kem-");
      let seed, pub;
      try {
        if (key.priv !== undefined) {
          if (typeof key.priv !== "string") badAkp();
          seed = __b64urlDec(key.priv);
        }
        if (typeof key.pub !== "string") badAkp();
        pub = __b64urlDec(key.pub);
      } catch (e) { if (e && e.code) throw e; badAkp(); }
      if (seed !== undefined && seed.length !== set[1]) badAkp();
      if (pub.length !== set[2]) badAkp();
      const oidB = Buffer.from(set[0], "hex");
      if (seed === undefined) {
        if (want === "private") noPriv();
        const algSeq = __tlv(0x30, __tlv(0x06, oidB));
        const bit = Buffer.concat([Buffer.from([0]), Buffer.from(pub)]);
        const spki = Buffer.from(__tlv(0x30, Buffer.concat([Buffer.from(algSeq), Buffer.from(__tlv(0x03, bit))])));
        return new PublicKeyObject("public", kind, spki);
      }
      const pkcs8 = Buffer.from(__tlv(0x30, Buffer.concat([
        Buffer.from(__tlv(0x02, new Uint8Array([0]))),
        Buffer.from(__tlv(0x30, __tlv(0x06, oidB))),
        Buffer.from(__tlv(0x04, __tlv(0x80, Buffer.from(seed)))),
      ])));
      let info;
      try {
        info = JSON.parse(__cryptCall(() => (isKem
          ? __wjs_mlkem_seed_from_pkcs8(pkcs8)
          : __wjs_mldsa_seed_from_pkcs8(pkcs8))));
      } catch { badAkp(); }
      const spki = Buffer.from(info.spki, "base64");
      if (!spki.subarray(spki.length - set[2]).equals(Buffer.from(pub))) badAkp();
      if (want === "public") return new PublicKeyObject("public", kind, spki);
      return new PrivateKeyObject("private", kind, pkcs8);
    }
    const err = new TypeError("Unsupported JWK kty");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  // 10f X448：OKP raw 导入（真机口径：asymmetricKeyType 必带 string、
  // 裸料定长（ed25519/x25519 32B、x448 56B、ed448 57B）、类型错/坏长即
  // Invalid key data；raw-public 建私钥 → format 无效；raw-private 建公钥
  // 由 createPublicKey 派生收口。真机 26 逐项）。10f crypto五轮：raw-seed
  // 同走 akt 校验链（缺 akt→ARG_TYPE、坏 akt→ARG_VALUE），落定后
  // INCOMPATIBLE（本仓无 seed 键型）。
  if (format === "raw-private" || format === "raw-public" || format === "raw-seed") {
    if (want === "private" && format === "raw-public") {
      const err = new TypeError("The property 'key.format' is invalid. Received 'raw-public'");
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    // 10f crypto五轮：raw 导入不收字符串（真机 26 口径，kind 门之后、
    // akt 链之前；超 28 码点截前 25 + '...'，探针 /tmp/wjs-raw-probe9.mjs）。
    if (typeof key === "string") {
      const shown = key.length > 28 ? `${key.slice(0, 25)}...` : key;
      const err = new TypeError(
        `The "key.key" property must be an instance of ArrayBuffer, Buffer, TypedArray, or DataView. Received type string ('${shown}')`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    const akt = options?.asymmetricKeyType;
    if (typeof akt !== "string") {
      const recv = akt === undefined ? "undefined" : `type ${typeof akt} (${String(akt)})`;
      const err = new TypeError(`The "key.asymmetricKeyType" property must be of type string. Received ${recv}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    if (akt === "ec") {
      // 10f 四轮：EC raw 导入（真机 26 逐项）——namedCurve 必带 string
      //（ARG_TYPE 'key.namedCurve'）；curve 走 OpenSSL 名表（'P-256' 收、
      // 'secp256r1'/'banana' → INVALID_CURVE 'Invalid EC curve name'）；
      // raw-private = 定长标量（native 校验值域）、raw-public = 非压缩点
      //（04 头 + 1+2*size 长；压缩形/错长/不在曲线 → ARG_VALUE 'Invalid
      // key data'）。
      const nc = options?.namedCurve;
      if (typeof nc !== "string") {
        const recv = nc === undefined ? "undefined" : `type ${typeof nc} (${String(nc)})`;
        const err = new TypeError(`The "key.namedCurve" property must be of type string. Received ${recv}`);
        err.code = "ERR_INVALID_ARG_TYPE";
        throw err;
      }
      const curve = __ecCurve(nc);
      // 10f crypto五轮：EC raw-seed 导入即 INCOMPATIBLE（曲线门之后，
      // 坏曲线仍走 INVALID_CURVE，与 akt 链同序）。
      if (format === "raw-seed") {
        const err = new Error("The selected key encoding is incompatible with the key type");
        err.code = "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS";
        throw err;
      }
      const material = __cryptBytes(key, "key");
      const bad = () => {
        const err = new TypeError("Invalid key data");
        err.code = "ERR_INVALID_ARG_VALUE";
        throw err;
      };
      if (format === "raw-private") {
        let privDer;
        try { privDer = __cryptCall(() => __wjs_ec_import_priv(curve, material)); } catch { bad(); }
        const k = new PrivateKeyObject("private", "ec", Buffer.from(privDer));
        k.__detail = { namedCurve: curve };
        if (want === "public") return __derivePublic(k);
        return k;
      }
      const size = __curveSize(curve);
      // 10f crypto五轮：压缩点（02/03 + X）走轮子内解压 native；混合点
      //（06/07 + 全坐标）与 04 同道；坏前缀/错长落 bad()（真机逐项）。
      if ((material[0] === 2 || material[0] === 3) && material.length === 1 + size) {
        let pubDer;
        try { pubDer = __cryptCall(() => __wjs_ec_import_compressed(curve, material)); } catch { bad(); }
        const k = new PublicKeyObject("public", "ec", Buffer.from(pubDer));
        k.__detail = { namedCurve: curve };
        return k;
      }
      if (material.length !== 1 + 2 * size || ![4, 6, 7].includes(material[0])) bad();
      const x = material.slice(1, 1 + size), y = material.slice(1 + size);
      let pubDer;
      try { pubDer = __cryptCall(() => __wjs_ec_import_pub(curve, x, y)); } catch { bad(); }
      const k = new PublicKeyObject("public", "ec", Buffer.from(pubDer));
      k.__detail = { namedCurve: curve };
      return k;
    }
    // 10f crypto五轮：SLH-DSA raw 导入（raw-seed 先判 INCOMPATIBLE，
    // 与 OKP 同序；尺寸错位 → ARG_VALUE 'Invalid key data'；私钥 raw 建公钥
    // 走 __derivePublic（ERR_NOT_SUPPORTED，套件外记档）。
    if (akt === "slh-dsa-sha2-128f" || akt === "slh-dsa-sha2-192f") {
      if (format === "raw-seed") {
        const err = new Error("The selected key encoding is incompatible with the key type");
        err.code = "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS";
        throw err;
      }
      const wantLen = akt === "slh-dsa-sha2-128f"
        ? (format === "raw-private" ? 64 : 32)
        : (format === "raw-private" ? 96 : 48);
      const material = __cryptBytes(key, "key");
      if (material.length !== wantLen) {
        const err = new TypeError("Invalid key data");
        err.code = "ERR_INVALID_ARG_VALUE";
        throw err;
      }
      if (format === "raw-private") {
        const k = new PrivateKeyObject("private", akt, Buffer.from(material));
        if (want === "public") return __derivePublic(k);
        return k;
      }
      return new PublicKeyObject("public", akt, Buffer.from(material));
    }
    // 10f crypto五轮：ML raw 导入（真机 26 逐项）——raw-public 按集验长
    //（错位 → ARG_VALUE 'Invalid key data'），料回包 SPKI DER 存；
    // raw-seed 按种子长验，料回包种子形 PKCS#8 存（下游 seed_from_pkcs8/
    // derivePublic 全通）；raw-private 一律 INCOMPATIBLE（ml 无此面）。
    if (akt.startsWith("ml-kem-") || akt.startsWith("ml-dsa-")) {
      const set = __ML_SETS[akt];
      if (!set) {
        const err = new TypeError(`Invalid asymmetricKeyType: ${akt}`);
        err.code = "ERR_INVALID_ARG_VALUE";
        throw err;
      }
      const badData = () => {
        const err = new TypeError("Invalid key data");
        err.code = "ERR_INVALID_ARG_VALUE";
        throw err;
      };
      const material = __cryptBytes(key, "key");
      if (format === "raw-public") {
        if (material.length !== set[2]) badData();
        return new PublicKeyObject("public", akt, Buffer.from(__mlSpki(akt, material)));
      }
      if (format === "raw-private") {
        const err = new Error("The selected key encoding is incompatible with the key type");
        err.code = "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS";
        throw err;
      }
      if (material.length !== set[1]) badData();
      const k = new PrivateKeyObject("private", akt, Buffer.from(__mlSeedPkcs8(akt, material)));
      if (want === "public") return __derivePublic(k);
      return k;
    }
    const lens = { ed25519: 32, x25519: 32, x448: 56, ed448: 57 };
    if (lens[akt] === undefined) {
      // 10f 四轮：已知键型但 raw 不支持 → INCOMPATIBLE（真机逐项）；
      // 未知键型 → ARG_VALUE 'Invalid asymmetricKeyType: X'。
      // 注：ml 系已上分支，此处 startsWith(ml-*) 仅为集外名兜底。
      if (akt === "rsa" || akt === "rsa-pss" || akt === "dsa" || akt === "dh" ||
          akt.startsWith("ml-kem-") || akt.startsWith("ml-dsa-")) {
        const err = new Error("The selected key encoding is incompatible with the key type");
        err.code = "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS";
        throw err;
      }
      const err = new TypeError(`Invalid asymmetricKeyType: ${akt}`);
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    // 10f crypto五轮：OKP raw-seed 导入即 INCOMPATIBLE（akt 链之后，
    // 缺/坏 akt 仍走各自门）。
    if (format === "raw-seed") {
      const err = new Error("The selected key encoding is incompatible with the key type");
      err.code = "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS";
      throw err;
    }
    const material = __cryptBytes(key, "key");
    if (material.length !== lens[akt]) {
      const err = new TypeError("Invalid key data");
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    if (format === "raw-public") {
      return new PublicKeyObject("public", akt, Buffer.from(material));
    }
    const k = new PrivateKeyObject("private", akt, Buffer.from(material));
    if (want === "public") return __derivePublic(k);
    return k;
  }
  let der;
  let pem = null;
  if (typeof key === "string") {
    pem = __pemDecode(key);
    if (!pem) {
      // 10f crypto二轮：空串即 DECODER 原文（openssl 3.x，套件点名）。
      if (key === "") {
        const err = new Error("error:1E08010C:DECODER routines::unsupported");
        err.code = "ERR_INVALID_ARG_VALUE";
        throw err;
      }
      const err = new TypeError("PEM decode failed");
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    der = pem.der;
  } else {
    der = __cryptBytes(key, "key");
    // 10f crypto二轮：PEM 装甲裹在 Buffer 里同样嗅探解码（fixtures 无编码读回即此形）。
    if (der.length > 11 && String.fromCharCode(...der.subarray(0, 11)) === "-----BEGIN ") {
      pem = __pemDecode(Buffer.from(der).toString("utf8"));
      if (!pem) {
        const err = new TypeError("PEM decode failed");
        err.code = "ERR_INVALID_ARG_VALUE";
        throw err;
      }
      der = pem.der;
    }
  }
  if (pem !== null) {
    if (pem.label === "PRIVATE KEY") type = type ?? "pkcs8";
    else if (pem.label === "PUBLIC KEY") type = type ?? "spki";
    else if (pem.label === "RSA PRIVATE KEY") type = type ?? "pkcs1";
    else if (pem.label === "RSA PUBLIC KEY") type = type ?? "pkcs1-pub";
    else if (pem.label === "DSA PRIVATE KEY") type = type ?? "dsa-legacy";
    else if (pem.label === "EC PRIVATE KEY") type = type ?? "sec1";
    else if (pem.label === "ENCRYPTED PRIVATE KEY") type = type ?? "pbes2";
    else if (pem.label === "CERTIFICATE") {
      // 10f crypto二轮：证书提 SPKI 作公钥（既有 `__wjs_x509_parse` 轮子，真机口径）。
      const info = JSON.parse(__cryptCall(() => __wjs_x509_parse(Buffer.from(der))));
      der = Buffer.from(info.spkiB64, "base64");
      type = type ?? "spki";
    }
    // 10f crypto二轮：传统加密 PEM 即解密（口令经 options 透传）。
    if (pem.dek) {
      der = __pemDecryptTraditional(pem, options);
    }
  }
  type = type ?? (want === "private" ? "pkcs8" : "spki");
  // 10f crypto六轮：PBES2 先解密（`ENCRYPTED PRIVATE KEY` 标签；解密后按 pkcs8
  // 续解，options 透传；标签触发与显式 type 无关——显式 pkcs8 + 加密标签同走）。
  if (pem !== null && pem.label === "ENCRYPTED PRIVATE KEY") {
    der = __pbes2Decrypt(der, options);
    type = "pkcs8";
  }
  // 10f crypto二轮：pkcs1 导入（private 存 PKCS#8 / public 存 SPKI，正则存；
  // 坏 DER 报 Invalid PKCS#1（引擎 ASN1 文案不可比，记档）。
  const __RSA_OID = new Uint8Array([6, 9, 42, 134, 72, 134, 247, 13, 1, 1, 1]);
  const __DER_NULL = new Uint8Array([5, 0]);
  const __derSeq = (...parts) => {
    let total = 0;
    for (const p of parts) total += p.length;
    const head = __derLen(total);
    const out = new Uint8Array(1 + head.length + total);
    out[0] = 48;
    out.set(head, 1);
    let off = 1 + head.length;
    for (const p of parts) { out.set(p, off); off += p.length; }
    return out;
  };
  const __badPkcs1 = () => {
    const err = new TypeError("Invalid PKCS#1 key");
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  };
  if (type === "pkcs1") {
    // 10f crypto二轮：RSAPrivateKey（9 INT）/RSAPublicKey（2 INT）按内容区分
    //（真机口径；want 侧由 createPublicKey 派生收口）。
    let isPriv = null;
    try {
      const top = __derRead(der, 0);
      if (top.tag === 48) {
        const kids = __derChildren(top.body);
        if (kids.length === 9 && kids.every((t) => t.tag === 2)) isPriv = true;
        else if (kids.length === 2 && kids.every((t) => t.tag === 2)) isPriv = false;
      }
    } catch { isPriv = null; }
    if (isPriv === null) __badPkcs1();
    // 10f crypto二轮：公钥料作私钥即 DECODER 错（openssl 3.x 原文+library，套件点名）。
    if (!isPriv && want === "private") {
      const err = new Error("error:1E08010C:DECODER routines::unsupported");
      err.code = "ERR_OSSL_UNSUPPORTED";
      err.library = "DECODER routines";
      throw err;
    }
    if (isPriv) {
      const pkcs8 = __derSeq(__derInt(new Uint8Array([0])), __derSeq(__RSA_OID, __DER_NULL),
        new Uint8Array([4, ...__derLen(der.length), ...der]));
      __cryptCall(() => __wjs_rsa_public(pkcs8));
      return new PrivateKeyObject("private", "rsa", Buffer.from(pkcs8));
    }
    const bitStr = new Uint8Array([3, ...__derLen(der.length + 1), 0, ...der]);
    const spki = __derSeq(__derSeq(__RSA_OID, __DER_NULL), bitStr);
    __cryptCall(() => __wjs_rsa_jwk_pub(spki));
    return new PublicKeyObject("public", "rsa", Buffer.from(spki));
  }
  if (type === "pkcs1-pub") {
    let seq;
    try {
      const top = __derRead(der, 0);
      if (top.tag !== 48) __badPkcs1();
      seq = __derChildren(top.body);
      if (seq.length !== 2 || seq.some((t) => t.tag !== 2)) __badPkcs1();
    } catch (e) { if (e && e.code) throw e; __badPkcs1(); }
    const bitStr = new Uint8Array([3, ...__derLen(der.length + 1), 0, ...der]);
    const spki = __derSeq(__derSeq(__RSA_OID, __DER_NULL), bitStr);
    __cryptCall(() => __wjs_rsa_jwk_pub(spki));
    return new PublicKeyObject("public", "rsa", Buffer.from(spki));
  }
  if (type === "pkcs8") {
    // PBES2 DER 先解密（显式 pkcs8 + 口令 + 加密体嗅探；PEM 侧由标签触发见上）。
    if (options?.passphrase !== undefined && __sniffPbes2(der)) {
      der = __pbes2Decrypt(der, options);
    }
    // 以 RSA/EC/OKP 逐一试解（DER 自描述不足，顺序即优先级；失败信息统一）
    const tries = [
      ["rsa-pss", () => {
        // 10f crypto六轮：RSA-PSS PKCS#8（alg 直判 rsassaPss；归一化验料存料；
        // params 缺席即无约束，空/显式 params 落 detail；轮子只见 plain-RSA）。
        let kids;
        try {
          const top = __derRead(der, 0);
          if (top.tag !== 0x30) throw new Error("no");
          kids = __derChildren(top.body);
        } catch { throw new Error("no"); }
        if (kids.length !== 3 || kids[0].tag !== 2 || kids[1].tag !== 0x30 || kids[2].tag !== 0x04) throw new Error("no");
        if (kids[0].body.length !== 1 || kids[0].body[0] !== 0) throw new Error("no");
        const pa = __pssAlg(kids[1].body);
        if (!pa) throw new Error("no");
        const norm = __tlv(0x30, new Uint8Array([
          2, 1, 0, ...__rsaEncAlgSeq(), ...__tlv(0x04, kids[2].body),
        ]));
        __cryptCall(() => __wjs_rsa_public(norm));
        const k = new PrivateKeyObject("private", "rsa-pss", Buffer.from(norm));
        if (pa.restrictions) {
          const d = __rsaDetailsFromMaterial("private", norm);
          k.__detail = { ...(d ?? {}), ...pa.restrictions, pssParams: pa.paramsB64 };
        } else {
          // 无约束键同样落 detail（空约束 + 缺席标记；导出回贴裸 OID 用）。
          const d = __rsaDetailsFromMaterial("private", norm);
          k.__detail = { ...(d ?? {}), pssParams: null };
        }
        return k;
      }],
      ["rsa", () => { __cryptCall(() => __wjs_rsa_public(der)); return new PrivateKeyObject("private", "rsa", der); }],
      ["ec", () => {
        // SPKI 算法 OID 直判（试解靠坐标长度会把 secp256k1 误判成 P-256，同 32 字节）。
        const g = __cryptCall(() => __wjs_ec_guess_curve(der));
        if (g === "") throw new Error("no");
        __cryptCall(() => __wjs_ec_public(g, der));
        const k = new PrivateKeyObject("private", "ec", der);
        k.__detail = { namedCurve: g };
        return k;
      }],
      ["dsa", () => {
        const env = __parseDsaDer(der, "private");
        try {
          __cryptCall(() => __wjs_dsa_export(JSON.stringify(env)));
        } catch (e) {
          // 10f crypto六轮：轮子拒收非标准尺寸（如 1088/160，套件
          // dsa_private_encrypted_1025 指纹）——装载期纯解析建对象
          // （真机装载宽容；数学校验留待运算期）。
          // 注：`__cryptErr` 只认全大写码，DataError 系落裸文，此处按消息窄匹配；
          // 其余错误（坏 y 值等）照抛。
          const m = String((e && e.message) || e);
          if (m !== "DataError: bad DSA parameters") throw e;
        }
        return __dsaKeyObject(env, "private");
      }],
      ["okp", () => {
        for (const kt of ["ed25519", "x25519", "x448", "ed448"]) {
          try {
            const kind = kt === "ed25519" ? "ED25519" : kt === "x25519" ? "X25519" : kt === "x448" ? "X448" : "ED448";
            const seed = __cryptCall(() => __wjs_okp_seed_from_pkcs8(kind, der));
            return new PrivateKeyObject("private", kt, Buffer.from(seed));
          } catch {}
        }
        throw new Error("no");
      }],
      ["ml-kem", () => {
        // 9i-4：种子形 PKCS#8（LAMPS 口径，[0] 64B 种子；展开即校验）。
        const parts = JSON.parse(__cryptCall(() => __wjs_mlkem_seed_from_pkcs8(der)));
        return new PrivateKeyObject("private", parts.kind, der);
      }],
      ["ml-dsa", () => {
        // 9i-6：种子形 PKCS#8（[0] 32B 种子；展开即校验）。
        const parts = JSON.parse(__cryptCall(() => __wjs_mldsa_seed_from_pkcs8(der)));
        return new PrivateKeyObject("private", parts.kind, der);
      }],
      ["slh", () => {
        // 10f crypto五轮：SLH-DSA PKCS#8（OCTET 裸私钥直存，尺寸已验）。
        const info = __slhParse(der, "private");
        if (!info) throw new Error("no");
        return new PrivateKeyObject("private", info.name, Buffer.from(info.raw));
      }],
      ["dh", () => {
        // 10f crypto五轮：DH PKCS#8（OID 1.2.840.113549.1.3.1；material 存整 DER，
        // p/g 不校验——raw 门只认 keyType，运算期另案）。
        let seq;
        try {
          const top = __derRead(der, 0);
          if (top.tag !== 48) throw new Error("no");
          seq = __derChildren(top.body);
        } catch { throw new Error("no"); }
        if (seq.length !== 3 || seq[0].tag !== 2 || seq[1].tag !== 48 || seq[2].tag !== 4) throw new Error("no");
        let alg;
        try { alg = __derChildren(seq[1].body); } catch { throw new Error("no"); }
        if (alg.length < 1 || alg[0].tag !== 6 ||
            Buffer.from(alg[0].body).toString("hex") !== "2a864886f70d010301") throw new Error("no");
        return new PrivateKeyObject("private", "dh", Buffer.from(der));
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
      () => {
        // 10f crypto六轮：RSA-PSS SPKI（同私钥侧；归一化存料）。
        let kids;
        try {
          const top = __derRead(der, 0);
          if (top.tag !== 0x30) throw new Error("no");
          kids = __derChildren(top.body);
        } catch { throw new Error("no"); }
        if (kids.length !== 2 || kids[0].tag !== 0x30 || kids[1].tag !== 0x03) throw new Error("no");
        const pa = __pssAlg(kids[0].body);
        if (!pa) throw new Error("no");
        const norm = __tlv(0x30, new Uint8Array([
          ...__rsaEncAlgSeq(), ...__tlv(0x03, kids[1].body),
        ]));
        __cryptCall(() => __wjs_rsa_jwk_pub(norm));
        const k = new PublicKeyObject("public", "rsa-pss", Buffer.from(norm));
        if (pa.restrictions) {
          const d = __rsaDetailsFromMaterial("public", norm);
          k.__detail = { ...(d ?? {}), ...pa.restrictions, pssParams: pa.paramsB64 };
        } else {
          const d = __rsaDetailsFromMaterial("public", norm);
          k.__detail = { ...(d ?? {}), pssParams: null };
        }
        return k;
      },
      () => { __cryptCall(() => __wjs_rsa_jwk_pub(der)); return new PublicKeyObject("public", "rsa", der); },
      () => {
        const g = __cryptCall(() => __wjs_ec_guess_curve(der));
        if (g === "") throw new Error("no");
        __cryptCall(() => __wjs_ec_jwk_pub(g, der));
        const k = new PublicKeyObject("public", "ec", der);
        k.__detail = { namedCurve: g };
        return k;
      },
      () => {
        const env = __parseDsaDer(der, "public");
        try {
          __cryptCall(() => __wjs_dsa_export(JSON.stringify(env)));
        } catch (e) {
          // 10f crypto六轮：同私钥侧（`__cryptErr` 全大写门，见上）。
          const m = String((e && e.message) || e);
          if (m !== "DataError: bad DSA parameters" && m !== "DataError: bad DSA public key") throw e;
        }
        return __dsaKeyObject(env, "public");
      },
      () => {
        for (const kt of ["ed25519", "x25519", "x448", "ed448"]) {
          try {
            const kind = kt === "ed25519" ? "ED25519" : kt === "x25519" ? "X25519" : kt === "x448" ? "X448" : "ED448";
            const pub = __cryptCall(() => __wjs_okp_pub_from_spki(kind, der));
            return new PublicKeyObject("public", kt, Buffer.from(pub));
          } catch {}
        }
        throw new Error("no");
      },
      () => {
        const kind = __cryptCall(() => __wjs_mlkem_kind_from_spki(der));
        if (kind === "") throw new Error("no");
        return new PublicKeyObject("public", kind, der);
      },
      () => {
        const kind = __cryptCall(() => __wjs_mldsa_kind_from_spki(der));
        if (kind === "") throw new Error("no");
        return new PublicKeyObject("public", kind, der);
      },
      () => {
        // 10f crypto五轮：SLH-DSA SPKI（BITSTRING 裸公钥直存，尺寸已验）。
        const info = __slhParse(der, "public");
        if (!info) throw new Error("no");
        return new PublicKeyObject("public", info.name, Buffer.from(info.raw));
      },
    ];
    for (const fn of tries) {
      try { return fn(); } catch (e) { if (e && e.code && e.code !== "ERR_NOT_SUPPORTED") throw e; }
    }
    const err = new TypeError("Invalid SPKI key");
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
  // 10f crypto二轮：legacy DSA 私钥（`DSA PRIVATE KEY` 标签，
  // SEQ{ version, p, q, g, y, x }，转内部信封存）。
  if (type === "dsa-legacy") {
    const bad = () => {
      const err = new TypeError("Invalid DSA key");
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    };
    let kids;
    try {
      const top = __derRead(der, 0);
      if (top.tag !== 48) bad();
      kids = __derChildren(top.body);
      if (kids.length !== 6 || kids.some((t) => t.tag !== 2)) bad();
    } catch (e) { if (e && e.code) throw e; bad(); }
    const [p, q, g, y, x] = __dsaInts(kids.slice(1));
    const env = { p, q, g, y, x };
    __cryptCall(() => __wjs_dsa_export(JSON.stringify(env)));
    return __dsaKeyObject(env, "private");
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
        const k = new PrivateKeyObject("private", "ec", Buffer.from(privDer));
        k.__detail = { namedCurve: c };
        return k;
      } catch {}
    }
    const err = new TypeError("Invalid SEC1 key");
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
  const err = new TypeError(`Unsupported key type ${type} (der sec1/pkcs1/pkcs8/spki supported)`);
  err.code = "ERR_INVALID_ARG_VALUE";
  throw err;
}
export function createPrivateKey(key) {
  if (typeof key === "object" && key !== null && !(key instanceof Uint8Array) && !(key instanceof ArrayBuffer) && !ArrayBuffer.isView(key) && !__isKeyObject(key)) {
    // 10f crypto二轮：显式 type 门（真机逐字；spki/pkcs1-pub 非私钥形）。
    if (key.type === "spki" || key.type === "pkcs1-pub") {
      const err = new TypeError(`The property 'key.type' is invalid. Received '${key.type}'`);
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    // 10f crypto二轮：字符串 key 按 options.encoding 预解码（真机口径，dsa 套件点名）。
    // 10f crypto五轮：raw 系除外——raw 导入不收字符串（ARG_TYPE，__parseKeyMaterial
    // 内判；先解码即把字符串洗成 Buffer，门永不触发）。
    if (typeof key.key === "string" && typeof key.encoding === "string" &&
        key.format !== "raw-private" && key.format !== "raw-public" && key.format !== "raw-seed") {
      key = { ...key, key: Buffer.from(key.key, key.encoding) };
    }
    // 10f crypto六轮：null/undefined 直透（`?? key` 会把二者洗成 options 对象，
    // JWK 门即永不触发；缺 key 的旧回退语义退役，真机口径见 JWK/格式门）。
    return __parseKeyMaterial(key.key, key.format, key.type, "private", key);
  }
  return __parseKeyMaterial(key, undefined, undefined, "private", undefined);
}
export function createPublicKey(key) {
  if (typeof key === "object" && key !== null && !(key instanceof Uint8Array) && !(key instanceof ArrayBuffer) && !ArrayBuffer.isView(key) && !__isKeyObject(key)) {
    // 允许从私钥对象派生公钥（Node 同款）
    if (__isKeyObject(key.key) && key.key.type === "private") {
      return __derivePublic(key.key);
    }
    // 10f crypto二轮：字符串 key 按 options.encoding 预解码（真机口径）。
    // 10f crypto五轮：raw 系除外（同 createPrivateKey）。
    if (typeof key.key === "string" && typeof key.encoding === "string" &&
        key.format !== "raw-private" && key.format !== "raw-public" && key.format !== "raw-seed") {
      key = { ...key, key: Buffer.from(key.key, key.encoding) };
    }
    // 10f crypto二轮：DER 私钥材料一律派生公钥（真机口径，加密 PEM 同）。
    const k = __parseKeyMaterial(key.key, key.format, key.type, "public", key);
    if (k.type === "private") return __derivePublic(k);
    return k;
  }
  if (__isKeyObject(key) && key.type === "private") return __derivePublic(key);
  // 10f crypto二轮：DER 私钥材料一律派生公钥（真机口径）。
  const k = __parseKeyMaterial(key, undefined, undefined, "public", undefined);
  if (k.type === "private") return __derivePublic(k);
  return k;
}
