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
    // PQC 系（pqc-keygen-ml-dsa 套件 deepStrictEqual 口径）：ml/slh 一律 {}。
    if (s.keyType.startsWith("ml-kem-") || s.keyType.startsWith("ml-dsa-") ||
        s.keyType.startsWith("slh-dsa-")) {
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
