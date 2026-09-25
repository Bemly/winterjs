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
function __genPairSync(type, options) {
  options = options ?? {};
  if (type === "rsa" || type === "rsa-pss") {
    const bits = options.modulusLength ?? 2048;
    // 10f crypto二轮：位长下限 512（真机口径；旧 2048/3072/4096 白名单记档修）。
    if (typeof bits !== "number") {
      const recv = bits === null ? "null"
        : typeof bits === "string" ? `type string ('${bits}')`
        : `type ${typeof bits} (${String(bits)})`;
      const err = new TypeError(
        `The "options.modulusLength" property must be of type number. Received ${recv}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    if (bits < 512) {
      const err = new Error("error:1C8000AB:Provider routines::key size too small");
      err.code = "ERR_OSSL_KEY_SIZE_TOO_SMALL";
      throw err;
    }
    let e = options.publicExponent ?? 65537;
    if (e instanceof Uint8Array) {
      let n = 0;
      for (const b of e) n = n * 256 + b;
      e = n;
    }
    const privDer = __cryptCall(() => __wjs_rsa_generate(bits, Number(e)));
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
  if (typeof options === "string" || options === undefined) options = {};
  // 10f crypto二轮：encoding 可放 options 内（真机双形态）。
  if (publicEncoding === undefined) publicEncoding = options.publicKeyEncoding;
  if (privateEncoding === undefined) privateEncoding = options.privateKeyEncoding;
  return __applyEncoding(__genPairSync(type, options), publicEncoding, privateEncoding);
}
