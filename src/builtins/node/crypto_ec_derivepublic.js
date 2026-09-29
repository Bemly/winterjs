function __derivePublic(priv) {
  if (priv.__keyType === "rsa" || priv.__keyType === "rsa-pss") {
    return new PublicKeyObject("public", priv.__keyType, Buffer.from(__cryptCall(() => __wjs_rsa_public(priv.__material))));
  }
  if (priv.__keyType === "ec") {
    const c = priv.__detail.namedCurve;
    return Object.assign(new PublicKeyObject("public", "ec", Buffer.from(__cryptCall(() => __wjs_ec_public(c, priv.__material)))), { __detail: { namedCurve: c } });
  }
  if (priv.__keyType === "ed25519") {
    return new PublicKeyObject("public", "ed25519", Buffer.from(__cryptCall(() => __wjs_ed_public(priv.__material))));
  }
  // 10e Ed448（OKP 同形）。
  if (priv.__keyType === "ed448") {
    return new PublicKeyObject("public", "ed448", Buffer.from(__cryptCall(() => __wjs_ed448_public(priv.__material))));
  }
  if (priv.__keyType === "x25519") {
    return new PublicKeyObject("public", "x25519", Buffer.from(__cryptCall(() => __wjs_x_public(priv.__material))));
  }
  if (priv.__keyType === "x448") {
    return new PublicKeyObject("public", "x448", Buffer.from(__cryptCall(() => __wjs_x448_public(priv.__material))));
  }
  if (priv.__keyType === "dsa") {
    const env = JSON.parse(Buffer.from(priv.__material).toString("utf8"));
    const pubEnv = { p: env.p, q: env.q, g: env.g, y: env.y };
    const k = new PublicKeyObject("public", "dsa", Buffer.from(JSON.stringify(pubEnv)));
    k.__detail = priv.__detail;
    return k;
  }
  if (typeof priv.__keyType === "string" && priv.__keyType.startsWith("ml-kem-")) {
    const parts = JSON.parse(__cryptCall(() => __wjs_mlkem_seed_from_pkcs8(priv.__material)));
    return new PublicKeyObject("public", priv.__keyType, Buffer.from(__b64dec(parts.spki)));
  }
  if (typeof priv.__keyType === "string" && priv.__keyType.startsWith("ml-dsa-")) {
    const spki = __cryptCall(() => __wjs_mldsa_public(priv.__material));
    return new PublicKeyObject("public", priv.__keyType, Buffer.from(spki));
  }
  const err = new Error("Cannot derive public key for this key type");
  err.code = "ERR_NOT_SUPPORTED";
  throw err;
}
// 同步头检（sync/async 入口共用，真机口径均为同步抛）：
// type 非串 → ARG_TYPE（`invalidArgTypeHelper` 形：null/undefined 裸收，
// 其余 `type X (inspect)`）；options 仅容 undefined/编码串/对象，
// 余者 → ARG_TYPE（`The "options" argument must be of type object. …`）。
function __checkKeyPairHead(type, options) {
  if (typeof type !== "string") {
    const recv = type === null || type === undefined ? `Received ${String(type)}`
      : `Received type ${typeof type} (${Array.isArray(type) ? "[]" : String(type)})`;
    const err = new TypeError(`The "type" argument must be of type string. ${recv}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (options !== undefined && typeof options !== "string" &&
      (typeof options !== "object" || options === null || Array.isArray(options))) {
    const recv = options === null ? "null"
      : Array.isArray(options) ? "an instance of Array"
      : `type ${typeof options} (${String(options)})`;
    const err = new TypeError(`The "options" argument must be of type object. Received ${recv}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
}
// `inspect` 近似（真机 util.inspect 口径：串单引号、空数组 `[]`、
// 空对象 `{}`；非空复合体的空格排版差异另案记档）。
function __inspectRecv(v) {
  if (typeof v === "string") return `'${v}'`;
  if (Array.isArray(v)) return v.length === 0 ? "[]" : `[ ${v.map(__inspectRecv).join(", ")} ]`;
  if (v !== null && typeof v === "object") {
    try { const j = JSON.stringify(v); if (j !== undefined) return j; } catch {}
  }
  return String(v);
}
// `common.invalidArgTypeHelper` 形（真机 26.8.2 实测）。
function __argTypeHelper(v) {
  if (v === null || v === undefined) return ` Received ${String(v)}`;
  if (Array.isArray(v)) return " Received an instance of Array";
  if (typeof v === "object") return ` Received ${__inspectRecv(v)}`;
  return ` Received type ${typeof v} (${__inspectRecv(v)})`;
}
function __checkKeyEncoding(prop, enc, typeSet) {
  if (enc === undefined) return;
  if (typeof enc !== "object" || enc === null || Array.isArray(enc)) {
    if (Array.isArray(enc)) {
      const err = new TypeError('The "options" argument must be of type object. Received an instance of Array');
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    const err = new TypeError(`The property '${prop}' is invalid. Received ${__inspectRecv(enc)}`);
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
  if (typeof enc.type !== "string" || (typeSet && !typeSet.has(enc.type))) {
    const err = new TypeError(`The property '${prop}.type' is invalid. Received ${__inspectRecv(enc.type)}`);
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
  if (typeof enc.format !== "string" || !__KEY_FORMATS.has(enc.format)) {
    const err = new TypeError(`The property '${prop}.format' is invalid. Received ${__inspectRecv(enc.format)}`);
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
}
// keypair 编码取值表（真机 26.8.2 实测；表外 key 类型只检形状，值交 export 判定）。
const __KEY_FORMATS = new Set(["pem", "der", "jwk"]);
const __KEY_ENC_TYPES = {
  rsa: { pub: new Set(["spki", "pkcs1"]), priv: new Set(["pkcs1", "pkcs8"]) },
  "rsa-pss": { pub: new Set(["spki"]), priv: new Set(["pkcs8"]) },
  ec: { pub: new Set(["spki"]), priv: new Set(["sec1", "pkcs8"]) },
};
function __checkKeyPairEncs(keyType, options, publicEncoding, privateEncoding) {
  const sets = __KEY_ENC_TYPES[keyType];
  __checkKeyEncoding("options.publicKeyEncoding", options.publicKeyEncoding, sets && sets.pub);
  __checkKeyEncoding("options.privateKeyEncoding", options.privateKeyEncoding, sets && sets.priv);
  __checkKeyEncoding("options.publicKeyEncoding", publicEncoding, sets && sets.pub);
  __checkKeyEncoding("options.privateKeyEncoding", privateEncoding, sets && sets.priv);
  // 私钥 cipher/passphrase（rsa 系；真机口径：cipher 非串即 invalid，
  // 值错误交 export 报 UNKNOWN_CIPHER；有 cipher 时 passphrase 须为串/字节视图）。
  const priv = options.privateKeyEncoding;
  if (priv !== undefined && typeof priv === "object" && priv !== null && !Array.isArray(priv)) {
    if (priv.cipher !== undefined && typeof priv.cipher !== "string") {
      const err = new TypeError(
        `The property 'options.privateKeyEncoding.cipher' is invalid. Received ${__inspectRecv(priv.cipher)}`);
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    if (priv.cipher !== undefined) {
      const pp = priv.passphrase;
      const okPp = typeof pp === "string" || pp instanceof ArrayBuffer ||
        (typeof ArrayBuffer !== "undefined" && ArrayBuffer.isView(pp));
      if (!okPp) {
        const err = new TypeError(
          `The property 'options.privateKeyEncoding.passphrase' is invalid. Received ${__inspectRecv(pp)}`);
        err.code = "ERR_INVALID_ARG_VALUE";
        throw err;
      }
    }
  }
}
const __KEYPAIR_TYPES = new Set(["rsa", "rsa-pss", "ec", "ed25519", "x25519", "x448", "ed448",
  "dsa", "ml-kem-512", "ml-kem-768", "ml-kem-1024", "ml-dsa-44", "ml-dsa-65", "ml-dsa-87"]);
function __checkKeyPairTypeKnown(type) {
  if (!__KEYPAIR_TYPES.has(type)) {
    const err = new TypeError(`The argument 'type' must be a supported key type. Received '${type}'`);
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
}
// RSA 参数同步校验（真机 26.8.2 口径；sync/async 入口共用，返回归一化值）。
// 非数 → ARG_TYPE + helper；非整数 → OUT_OF_RANGE；
// 越界（<0 或 >2^32-1）→ OUT_OF_RANGE；<512 → OSSL（旧行为保留）。
function __checkRsaKeyOptions(options) {
  // 真机口径：modulusLength 必给（{} 亦抛，无 2048 缺省）。
  const bits = options.modulusLength;
  if (typeof bits !== "number") {
    const err = new TypeError(
      `The "options.modulusLength" property must be of type number.${__argTypeHelper(bits)}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (!Number.isInteger(bits)) {
    const err = new RangeError(
      `The value of "options.modulusLength" is out of range. It must be an integer. Received ${__inspectRecv(bits)}`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  if (bits < 0 || bits > 4294967295) {
    const err = new RangeError(
      `The value of "options.modulusLength" is out of range. It must be >= 0 && <= 4294967295. Received ${bits}`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  if (bits < 512) {
    const err = new Error("error:1C8000AB:Provider routines::key size too small");
    err.code = "ERR_OSSL_KEY_SIZE_TOO_SMALL";
    throw err;
  }
  let e = options.publicExponent ?? 65537;
  // 旧 accommodation 保留：Uint8Array 指数先折叠为数（历史行为），再走真机校验。
  if (e instanceof Uint8Array) {
    let n = 0;
    for (const b of e) n = n * 256 + b;
    e = n;
  }
  if (typeof e !== "number") {
    const err = new TypeError(
      `The "options.publicExponent" property must be of type number.${__argTypeHelper(e)}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (!Number.isInteger(e) || e < 0 || e > 4294967295) {
    const what = !Number.isInteger(e) ? "It must be an integer. " : "It must be >= 0 && <= 4294967295. ";
    const err = new RangeError(
      `The value of "options.publicExponent" is out of range. ${what}Received ${__inspectRecv(e)}`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  return { bits, e: Number(e) };
}
function __genPairSync(type, options) {
  options = options ?? {};
  __checkKeyPairHead(type, options);
  if (type === "rsa" || type === "rsa-pss") {
    const { bits, e } = __checkRsaKeyOptions(options);
    const privDer = __cryptCall(() => __wjs_rsa_generate(bits, e));
    const pubDer = __cryptCall(() => __wjs_rsa_public(privDer));
    const kt = type === "rsa-pss" ? "rsa-pss" : "rsa";
    const priv = new PrivateKeyObject("private", kt, Buffer.from(privDer));
    const pub = new PublicKeyObject("public", kt, Buffer.from(pubDer));
    // 10f crypto二轮：asymmetricKeyDetails 口径（modulusLength + publicExponent）。
    priv.__detail = { modulusLength: bits, publicExponent: e };
    pub.__detail = { modulusLength: bits, publicExponent: e };
    if (type === "rsa-pss") {
      priv.__detail.hash = options.hash ?? "sha256";
      priv.__detail.saltLength = options.saltLength;
      pub.__detail.hash = priv.__detail.hash;
      pub.__detail.saltLength = priv.__detail.saltLength;
    }
    return { privateKey: priv, publicKey: pub };
  }
  if (type === "ec") {
    // 真机口径：paramEncoding 仅收 der/pem（缺省 der；其余同步抛）。
    const pe = options.paramEncoding ?? "der";
    if (pe !== "der" && pe !== "pem") {
      const err = new TypeError(
        `The property 'options.paramEncoding' is invalid. Received '${pe}'`);
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    const curve = __normCurve(options.namedCurve);
    if (curve === "Ed25519" || curve === "X25519") {
      // 10f crypto六轮：真机口径（`generateKeyPair('ec',{namedCurve:'ed25519'})`
      // 即 INVALID_CURVE；旧指引文案退役）。
      __badEcCurve();
    }
    const privDer = __cryptCall(() => __wjs_ec_generate(curve));
    const pubDer = __cryptCall(() => __wjs_ec_public(curve, privDer));
    const priv = new PrivateKeyObject("private", "ec", Buffer.from(privDer));
    priv.__detail = { namedCurve: curve };
    const pub = new PublicKeyObject("public", "ec", Buffer.from(pubDer));
    pub.__detail = { namedCurve: curve };
    return { privateKey: priv, publicKey: pub };
  }
  if (type === "ed25519" || type === "x25519" || type === "x448" || type === "ed448") {
    const isEd = type === "ed25519", is448 = type === "ed448", isX448 = type === "x448";
    const seed = __cryptCall(() => isEd ? __wjs_ed_generate() : is448 ? __wjs_ed448_generate() : isX448 ? __wjs_x448_generate() : __wjs_x_generate());
    const pubB = __cryptCall(() => isEd ? __wjs_ed_public(seed) : is448 ? __wjs_ed448_public(seed) : isX448 ? __wjs_x448_public(seed) : __wjs_x_public(seed));
    return {
      privateKey: new PrivateKeyObject("private", type, Buffer.from(seed)),
      publicKey: new PublicKeyObject("public", type, Buffer.from(pubB)),
    };
  }
  if (type === "dsa") {
    // Node 缺省：divisorLength 按 modulus 取（1024→160，其余→256；2048/224 须显式）。
    let modulusLength = options.modulusLength ?? 2048;
    let divisorLength = options.divisorLength;
    if (divisorLength === undefined) divisorLength = modulusLength === 1024 ? 160 : 256;
    const env = JSON.parse(__cryptCall(() => __wjs_dsa_generate(modulusLength, divisorLength)));
    const priv = new PrivateKeyObject("private", "dsa", Buffer.from(JSON.stringify(env)));
    priv.__detail = { modulusLength, divisorLength };
    const pubEnv = { p: env.p, q: env.q, g: env.g, y: env.y };
    const pub = new PublicKeyObject("public", "dsa", Buffer.from(JSON.stringify(pubEnv)));
    pub.__detail = priv.__detail;
    return { privateKey: priv, publicKey: pub };
  }
  if (type === "ml-kem-512" || type === "ml-kem-768" || type === "ml-kem-1024") {
    // 9i-4：FIPS 203 PQ KEM（真机 generateKey 不收 ml-kem，此处 generateKeyPair 专属）。
    const parts = JSON.parse(__cryptCall(() => __wjs_mlkem_gen(type)));
    return {
      privateKey: new PrivateKeyObject("private", type, Buffer.from(__b64dec(parts.pkcs8))),
      publicKey: new PublicKeyObject("public", type, Buffer.from(__b64dec(parts.spki))),
    };
  }
  if (type === "ml-dsa-44" || type === "ml-dsa-65" || type === "ml-dsa-87") {
    // 9i-6：FIPS 204 PQ 签名（纯签名，hash=null）。
    const parts = JSON.parse(__cryptCall(() => __wjs_mldsa_gen(type)));
    return {
      privateKey: new PrivateKeyObject("private", type, Buffer.from(__b64dec(parts.pkcs8))),
      publicKey: new PublicKeyObject("public", type, Buffer.from(__b64dec(parts.spki))),
    };
  }
  const err = new TypeError(`The argument 'type' must be a supported key type. Received '${type}'`);
  err.code = "ERR_INVALID_ARG_VALUE";
  throw err;
}
function __applyEncoding(pair, publicEncoding, privateEncoding) {
  const out = {};
  if (publicEncoding !== undefined) {
    out.publicKey = pair.publicKey.export(publicEncoding);
  } else out.publicKey = pair.publicKey;
  if (privateEncoding !== undefined) {
    out.privateKey = pair.privateKey.export(privateEncoding);
  } else out.privateKey = pair.privateKey;
  return out;
}
export function generateKeyPairSync(type, options, publicEncoding, privateEncoding) {
  // 原始 options 先过头检（null 即抛；string/undefined 才归一），再检编码形。
  __checkKeyPairHead(type, options);
  if (typeof options === "string" || options === undefined) options = {};
  __checkKeyPairEncs(type, options, publicEncoding, privateEncoding);
  // 10f crypto二轮：encoding 可放 options 内（真机双形态）。
  if (publicEncoding === undefined) publicEncoding = options.publicKeyEncoding;
  if (privateEncoding === undefined) privateEncoding = options.privateKeyEncoding;
  return __applyEncoding(__genPairSync(type, options), publicEncoding, privateEncoding);
}
