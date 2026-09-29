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
        const privDer = __cryptCall(() => __wjs2_ec_import_priv(c, scalar.body));
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
