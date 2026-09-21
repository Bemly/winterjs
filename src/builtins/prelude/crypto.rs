//! WebCrypto（CryptoKey/crypto 对象/派生）（prelude 分域；拼接顺序见 mod.rs）。
pub const CRYPTO_JS: &str = r#"
globalThis.CryptoKey = class CryptoKey {
  constructor() { throw new TypeError("Illegal constructor"); }
  get algorithm() { return { ...__wjs_keyState.get(this)?.alg }; }
  get extractable() { return !!__wjs_keyState.get(this)?.extractable; }
  get type() { return __wjs_keyState.get(this)?.kind ?? "secret"; }
  get usages() { return [...(__wjs_keyState.get(this)?.usages ?? [])]; }
};
function __wjs_normCurve(c) {
  const s = String(c ?? "").trim().toUpperCase().replace("_", "-");
  const map = { "P-256": "P-256", "P256": "P-256", "P-384": "P-384", "P384": "P-384", "P-521": "P-521", "P521": "P-521" };
  if (!map[s]) throw new Error(`NotSupportedError: unsupported curve '${c}' (P-256/384/521)`);
  return map[s];
}
function __wjs_rsaPubExp(v) {
  if (v === undefined) return 65537;
  if (v instanceof Uint8Array) {
    let n = 0;
    for (const b of v) n = n * 256 + b;
    return n;
  }
  return Number(v);
}
function __wjs_x_bits(algorithm, st, length) {
  const pubKey = algorithm?.public;
  const pst = __wjs_keyState.get(pubKey);
  if (!pst || pst.alg.name !== "X25519" || pst.kind === "private") {
    throw new TypeError("deriveBits: algorithm.public must be an X25519 public key");
  }
  const secret = __wjs_x_derive(st.material, pst.material);
  if (length === undefined || length === null) return secret.buffer;
  const bits = Number(length);
  if (!Number.isInteger(bits) || bits < 0 || bits > secret.length * 8 || bits % 8 !== 0) {
    throw new Error("OperationError: bad X25519 deriveBits length");
  }
  return secret.slice(0, bits / 8).buffer;
}
function __wjs_ecdh_bits(algorithm, st, length) {
  const pubKey = algorithm?.public;
  const pst = __wjs_keyState.get(pubKey);
  if (!pst || pst.alg.name !== "ECDH" || pst.kind === "private") {
    throw new TypeError("deriveBits: algorithm.public must be an ECDH public key");
  }
  if (pst.alg.namedCurve !== st.alg.namedCurve) throw new Error("InvalidAccessError: ECDH curves differ");
  const secret = __wjs_ecdh_derive(st.alg.namedCurve, st.material, pst.material);
  if (length === undefined || length === null) return secret.buffer;
  const bits = Number(length);
  if (!Number.isInteger(bits) || bits < 0 || bits > secret.length * 8 || bits % 8 !== 0) {
    throw new Error("OperationError: bad ECDH deriveBits length");
  }
  return secret.slice(0, bits / 8).buffer;
}
globalThis.crypto = {
  getRandomValues(view) { __wjs_fill_random(view); return view; },
  randomUUID() { return __wjs_random_uuid(); },
  subtle: {
    async digest(algorithm, data) {
      const name = typeof algorithm === "string" ? algorithm : String(algorithm?.name ?? algorithm);
      let view = data;
      if (view instanceof ArrayBuffer) view = new Uint8Array(view);
      else if (ArrayBuffer.isView(view) && !(view instanceof Uint8Array)) {
        view = new Uint8Array(view.buffer, view.byteOffset, view.byteLength);
      }
      const out = __wjs_subtle_digest(name, view);
      return out.buffer;
    },
    async generateKey(alg, extractable, usages) {
      const name = typeof alg === "string" ? alg.toUpperCase() : String(alg?.name ?? "").toUpperCase();
      usages = [...(usages ?? [])].map(String);
      if (name === "AES-GCM") {
        const length = Number(alg?.length ?? 256);
        if (![128, 192, 256].includes(length)) throw new Error("NotSupportedError: AES-GCM length must be 128/192/256");
        const bytes = new Uint8Array(length / 8);
        crypto.getRandomValues(bytes);
        return __wjs_makeKey({ name: "AES-GCM", length }, bytes, usages, !!extractable);
      }
      if (name === "HMAC") {
        const hash = __wjs_normHash(alg?.hash);
        let length = alg?.length === undefined ? null : Number(alg.length);
        const outLen = { "SHA-1": 160, "SHA-256": 256, "SHA-384": 384, "SHA-512": 512 }[hash];
        if (length === null) length = outLen;
        if (!Number.isInteger(length) || length <= 0 || length > 1024 * 1024) {
          throw new Error("NotSupportedError: bad HMAC length");
        }
        const bytes = new Uint8Array(Math.ceil(length / 8));
        crypto.getRandomValues(bytes);
        return __wjs_makeKey({ name: "HMAC", hash, length }, bytes, usages, !!extractable);
      }
      if (name === "RSASSA-PKCS1-V1_5" || name === "RSA-OAEP" || name === "RSA-PSS") {
        const length = Number(alg?.modulusLength ?? 2048);
        if (![2048, 3072, 4096].includes(length)) throw new Error("NotSupportedError: RSA modulusLength must be 2048/3072/4096");
        const e = __wjs_rsaPubExp(alg?.publicExponent);
        if (!Number.isInteger(e) || e < 2 || e > 2 ** 33 - 1) throw new Error("DataError: bad RSA publicExponent");
        const hash = __wjs_normHash(alg?.hash ?? "SHA-256");
        const privDer = __wjs_rsa_generate(length, e);
        const pubDer = __wjs_rsa_public(privDer);
        const expBytes = (() => { const out = []; let n = e; do { out.unshift(n & 255); n = Math.floor(n / 256); } while (n > 0); return new Uint8Array(out); })();
        const keyAlg = { name, modulusLength: length, publicExponent: expBytes, hash };
        const mkPub = __wjs_makeKey(keyAlg, pubDer, usages, !!extractable, "public");
        const mkPriv = __wjs_makeKey(keyAlg, privDer, usages, !!extractable, "private");
        return { publicKey: mkPub, privateKey: mkPriv };
      }
      if (name === "ECDSA" || name === "ECDH") {
        const curve = __wjs_normCurve(alg?.namedCurve);
        const privDer = __wjs_ec_generate(curve);
        const pubDer = __wjs_ec_public(curve, privDer);
        const mkPub = __wjs_makeKey({ name, namedCurve: curve }, pubDer, usages, !!extractable, "public");
        const mkPriv = __wjs_makeKey({ name, namedCurve: curve }, privDer, usages, !!extractable, "private");
        return { publicKey: mkPub, privateKey: mkPriv };
      }
      if (name === "ED25519") {
        const seed = __wjs_ed_generate();
        const pub = __wjs_ed_public(seed);
        const mkPub = __wjs_makeKey({ name, namedCurve: "Ed25519" }, pub, usages, !!extractable, "public");
        const mkPriv = __wjs_makeKey({ name, namedCurve: "Ed25519" }, seed, usages, !!extractable, "private");
        return { publicKey: mkPub, privateKey: mkPriv };
      }
      if (name === "X25519") {
        const priv = __wjs_x_generate();
        const pub = __wjs_x_public(priv);
        const mkPub = __wjs_makeKey({ name, namedCurve: "X25519" }, pub, usages, !!extractable, "public");
        const mkPriv = __wjs_makeKey({ name, namedCurve: "X25519" }, priv, usages, !!extractable, "private");
        return { publicKey: mkPub, privateKey: mkPriv };
      }
      if (name === "ED448") {
        throw new Error(`NotSupportedError: generateKey ${name} needs follow-up`);
      }
      throw new Error(`NotSupportedError: generateKey ${name} needs Phase 3 c-4`);
    },
    async importKey(format, keyData, alg, extractable, usages) {
      const name = typeof alg === "string" ? alg.toUpperCase() : String(alg?.name ?? "").toUpperCase();
      usages = [...(usages ?? [])].map(String);
      const needHash = name === "HMAC" ? __wjs_normHash(alg?.hash) : undefined;
      if (name === "RSASSA-PKCS1-V1_5" || name === "RSA-OAEP" || name === "RSA-PSS") {
        const hash = __wjs_normHash(alg?.hash ?? "SHA-256");
        if (format === "jwk") {
          if (!keyData || keyData.kty !== "RSA" || typeof keyData.n !== "string" || typeof keyData.e !== "string") {
            throw new Error("DataError: bad RSA JWK (n/e)");
          }
          const n = __wjs_b64urlDecode(keyData.n), e = __wjs_b64urlDecode(keyData.e);
          const expBytes = e.slice();
          if (typeof keyData.d === "string") {
            const d = __wjs_b64urlDecode(keyData.d);
            const privDer = __wjs_rsa_import_priv(n, e, d);
            const keyAlg = { name, modulusLength: n.length * 8, publicExponent: expBytes, hash };
            return __wjs_makeKey(keyAlg, privDer, usages, !!extractable, "private");
          }
          const pubDer = __wjs_rsa_import_pub(n, e);
          const keyAlg = { name, modulusLength: n.length * 8, publicExponent: expBytes, hash };
          return __wjs_makeKey(keyAlg, pubDer, usages, !!extractable, "public");
        }
        if (format === "pkcs8") {
          const privDer = __wjs_keyBytes(keyData);
          // PKCS#8 自验证（解析失败即 DataError；公钥顺带导出供 algorithm）。
          const pubDer = __wjs_rsa_public(privDer);
          const parts = JSON.parse(__wjs_rsa_jwk(privDer, pubDer));
          const n = __wjs_b64urlDecode(parts.n), e = __wjs_b64urlDecode(parts.e);
          const keyAlg = { name, modulusLength: n.length * 8, publicExponent: e.slice(), hash };
          return __wjs_makeKey(keyAlg, privDer, usages, !!extractable, "private");
        }
        if (format === "spki") {
          const pubDer = __wjs_keyBytes(keyData);
          const parts = JSON.parse(__wjs_rsa_jwk_pub(pubDer));
          const n = __wjs_b64urlDecode(parts.n), e = __wjs_b64urlDecode(parts.e);
          const keyAlg = { name, modulusLength: n.length * 8, publicExponent: e.slice(), hash };
          return __wjs_makeKey(keyAlg, pubDer, usages, !!extractable, "public");
        }
        throw new Error(`NotSupportedError: importKey ${format} for RSA needs pkcs8/spki/jwk`);
      }
      if (name === "ECDSA" || name === "ECDH") {
        const curve = __wjs_normCurve(alg?.namedCurve);
        const keyAlg = { name, namedCurve: curve };
        if (format === "jwk") {
          if (!keyData || keyData.kty !== "EC" || keyData.crv !== curve
            || typeof keyData.x !== "string" || typeof keyData.y !== "string") {
            throw new Error("DataError: bad EC JWK (x/y/crv)");
          }
          const x = __wjs_b64urlDecode(keyData.x), y = __wjs_b64urlDecode(keyData.y);
          if (typeof keyData.d === "string") {
            const privDer = __wjs_ec_import_priv(curve, __wjs_b64urlDecode(keyData.d));
            // 公钥一致性：JWK 的 x/y 须与 d 对应（防混入）。
            const expect = __wjs_ec_public(curve, privDer);
            const got = __wjs_ec_import_pub(curve, x, y);
            const same = expect.length === got.length && expect.every((b, i) => b === got[i]);
            if (!same) throw new Error("DataError: EC JWK x/y does not match d");
            return __wjs_makeKey(keyAlg, privDer, usages, !!extractable, "private");
          }
          return __wjs_makeKey(keyAlg, __wjs_ec_import_pub(curve, x, y), usages, !!extractable, "public");
        }
        if (format === "pkcs8") {
          const privDer = __wjs_keyBytes(keyData);
          // PKCS#8 自验证（曲线错/损坏即 DataError）。
          __wjs_ec_public(curve, privDer);
          return __wjs_makeKey(keyAlg, privDer, usages, !!extractable, "private");
        }
        if (format === "spki") {
          const pubDer = __wjs_keyBytes(keyData);
          // SPKI 自验证（曲线错即 DataError）。
          __wjs_ec_jwk_pub(curve, pubDer);
          return __wjs_makeKey(keyAlg, pubDer, usages, !!extractable, "public");
        }
        if (format === "raw") {
          const v = __wjs_keyBytes(keyData);
          if (v.length < 1 || v[0] !== 0x04) throw new Error("DataError: EC raw public must be uncompressed (0x04‖x‖y)");
          const size = (v.length - 1) / 2;
          if (![32, 48, 66].includes(size) || 1 + 2 * size !== v.length) throw new Error("DataError: bad EC raw length");
          const x = v.slice(1, 1 + size), y = v.slice(1 + size);
          const c2 = size === 32 ? "P-256" : size === 48 ? "P-384" : "P-521";
          if (c2 !== curve) throw new Error("DataError: EC raw length does not match namedCurve");
          return __wjs_makeKey(keyAlg, __wjs_ec_import_pub(curve, x, y), usages, !!extractable, "public");
        }
        throw new Error(`NotSupportedError: importKey ${format} for EC needs jwk/pkcs8/spki/raw`);
      }
      if (name === "ED25519" || name === "X25519") {
        // JWK crv 用混合大小写（RFC 8037；内部 name 全大写，不外泄）。
        const crv = name === "ED25519" ? "Ed25519" : "X25519";
        const keyAlg = { name, namedCurve: crv };
        if (format === "jwk") {
          if (!keyData || keyData.kty !== "OKP" || keyData.crv !== crv
            || typeof keyData.x !== "string") {
            throw new Error("DataError: bad OKP JWK (kty/crv/x)");
          }
          const x = __wjs_b64urlDecode(keyData.x);
          if (x.length !== 32) throw new Error("DataError: bad OKP JWK (x length)");
          if (typeof keyData.d === "string") {
            const seed = __wjs_b64urlDecode(keyData.d);
            if (seed.length !== 32) throw new Error("DataError: bad OKP JWK (d length)");
            // 私钥一致性：JWK 的 x 须与 d 对应（防混入）。
            const expect = name === "ED25519" ? __wjs_ed_public(seed) : __wjs_x_public(seed);
            const same = expect.length === 32 && expect.every((b, i) => b === x[i]);
            if (!same) throw new Error("DataError: OKP JWK x does not match d");
            return __wjs_makeKey(keyAlg, seed, usages, !!extractable, "private");
          }
          return __wjs_makeKey(keyAlg, x, usages, !!extractable, "public");
        }
        if (format === "raw") {
          const v = __wjs_keyBytes(keyData);
          if (v.length !== 32) throw new Error("DataError: OKP raw key must be 32 bytes");
          return __wjs_makeKey(keyAlg, v, usages, !!extractable, "public");
        }
        if (format === "pkcs8") {
          const seed = __wjs_okp_seed_from_pkcs8(name, __wjs_keyBytes(keyData));
          return __wjs_makeKey(keyAlg, seed, usages, !!extractable, "private");
        }
        if (format === "spki") {
          const publ = __wjs_okp_pub_from_spki(name, __wjs_keyBytes(keyData));
          return __wjs_makeKey(keyAlg, publ, usages, !!extractable, "public");
        }
        throw new Error(`NotSupportedError: importKey ${format} for OKP needs jwk/raw/pkcs8/spki`);
      }
      let bytes;
      if (format === "raw") {
        bytes = __wjs_keyBytes(keyData);
      } else if (format === "jwk") {
        if (!keyData || keyData.kty !== "oct" || typeof keyData.k !== "string") {
          throw new Error("NotSupportedError: only oct JWK keys for now");
        }
        bytes = __wjs_b64urlDecode(keyData.k);
      } else throw new Error(`NotSupportedError: importKey ${format} needs Phase 3 c-4`);
      if (name === "AES-GCM") {
        if (![16, 24, 32].includes(bytes.length)) throw new TypeError("AES-GCM raw key must be 16/24/32 bytes");
        return __wjs_makeKey({ name, length: bytes.length * 8 }, bytes, usages, !!extractable);
      }
      if (name === "HMAC") {
        return __wjs_makeKey({ name, hash: needHash, length: bytes.length * 8 }, bytes, usages, !!extractable);
      }
      throw new Error(`NotSupportedError: importKey ${name} needs Phase 3 c-4`);
    },
    async exportKey(format, key) {
      const st = __wjs_keyState.get(key);
      if (!st) throw new TypeError("exportKey: not a CryptoKey");
      if (!st.extractable) throw new Error("InvalidAccessError: key is not extractable");
      const aname = st.alg.name;
      if (aname === "RSASSA-PKCS1-V1_5" || aname === "RSA-OAEP" || aname === "RSA-PSS") {
        const hash = st.alg.hash ?? "SHA-256";
        const isPriv = st.kind === "private";
        const privDer = isPriv ? st.material : null;
        const pubDer = isPriv ? __wjs_rsa_public(st.material) : st.material;
        if (format === "pkcs8") {
          if (!isPriv) throw new Error("InvalidAccessError: not a private key");
          return st.material.slice().buffer;
        }
        if (format === "spki") {
          return pubDer.slice().buffer;
        }
        if (format === "jwk") {
          const parts = isPriv
            ? JSON.parse(__wjs_rsa_jwk(privDer, pubDer))
            : JSON.parse(__wjs_rsa_jwk_pub(pubDer));
          const jwk = { kty: "RSA", n: parts.n, e: parts.e };
          if (isPriv) { jwk.d = parts.d; jwk.p = parts.p; jwk.q = parts.q; jwk.dp = parts.dp; jwk.dq = parts.dq; jwk.qi = parts.qi; }
          jwk.alg = aname === "RSASSA-PKCS1-V1_5"
            ? { "SHA-256": "RS256", "SHA-384": "RS384", "SHA-512": "RS512" }[hash] ?? "RS256"
            : aname === "RSA-PSS"
              ? { "SHA-256": "PS256", "SHA-384": "PS384", "SHA-512": "PS512" }[hash] ?? "PS256"
              : { "SHA-256": "RSA-OAEP", "SHA-384": "RSA-OAEP-384", "SHA-512": "RSA-OAEP-512" }[hash] ?? "RSA-OAEP";
          jwk.ext = true;
          return jwk;
        }
        throw new Error(`NotSupportedError: exportKey ${format} for RSA needs pkcs8/spki/jwk`);
      }
      if (aname === "ECDSA" || aname === "ECDH") {
        const curve = st.alg.namedCurve;
        const isPriv = st.kind === "private";
        const pubDer = isPriv ? __wjs_ec_public(curve, st.material) : st.material;
        if (format === "pkcs8") {
          if (!isPriv) throw new Error("InvalidAccessError: not a private key");
          return st.material.slice().buffer;
        }
        if (format === "spki") {
          return pubDer.slice().buffer;
        }
        if (format === "jwk") {
          const parts = isPriv
            ? JSON.parse(__wjs_ec_jwk(curve, st.material, pubDer))
            : JSON.parse(__wjs_ec_jwk_pub(curve, pubDer));
          const jwk = { kty: "EC", crv: curve, x: parts.x, y: parts.y };
          if (isPriv) jwk.d = parts.d;
          jwk.ext = true;
          return jwk;
        }
        if (format === "raw") {
          if (isPriv) throw new Error("InvalidAccessError: raw export needs a public key");
          const parts = JSON.parse(__wjs_ec_jwk_pub(curve, pubDer));
          const x = __wjs_b64urlDecode(parts.x), y = __wjs_b64urlDecode(parts.y);
          const out = new Uint8Array(1 + x.length + y.length);
          out[0] = 0x04; out.set(x, 1); out.set(y, 1 + x.length);
          return out.buffer;
        }
        throw new Error(`NotSupportedError: exportKey ${format} for EC needs pkcs8/spki/jwk/raw`);
      }
      if (aname === "ED25519" || aname === "X25519") {
        const isPriv = st.kind === "private";
        const pubBytes = isPriv
          ? (aname === "ED25519" ? __wjs_ed_public(st.material) : __wjs_x_public(st.material))
          : st.material;
        if (format === "pkcs8") {
          if (!isPriv) throw new Error("InvalidAccessError: not a private key");
          return __wjs_okp_pkcs8_from_seed(aname, st.material).buffer;
        }
        if (format === "spki") {
          return __wjs_okp_spki_from_pub(aname, pubBytes).buffer;
        }
        if (format === "jwk") {
          const jwk = { kty: "OKP", crv: aname === "ED25519" ? "Ed25519" : "X25519", x: __wjs_b64urlEncode(pubBytes) };
          if (isPriv) jwk.d = __wjs_b64urlEncode(st.material);
          // JWA 只给 Ed25519 定义了 "EdDSA"；X25519 无 alg（与 Node 一致，省略）。
          if (aname === "ED25519") jwk.alg = "EdDSA";
          jwk.ext = true;
          return jwk;
        }
        if (format === "raw") {
          if (isPriv) throw new Error("InvalidAccessError: raw export needs a public key");
          return pubBytes.slice().buffer;
        }
        throw new Error(`NotSupportedError: exportKey ${format} for OKP needs pkcs8/spki/jwk/raw`);
      }
      if (format === "raw") return st.material.slice().buffer;
      if (format === "jwk") {
        return { kty: "oct", k: __wjs_b64urlEncode(st.material), alg: st.alg.name === "AES-GCM" ? `A${st.alg.length}GCM` : `HS${st.alg.hash.split("-")[1]}`, ext: true };
      }
      throw new Error(`NotSupportedError: exportKey ${format} needs Phase 3 c-4`);
    },
    async encrypt(algorithm, key, data) {
      const st = __wjs_keyState.get(key);
      if (!st) throw new TypeError("encrypt: not a CryptoKey");
      __wjs_needUsage(st, "encrypt");
      if (st.alg.name === "AES-GCM") {
        const p = __wjs_aesParams(algorithm);
        const out = __wjs_aesgcm_encrypt(st.material, p.iv, p.aad, __wjs_dataBytes(data));
        return out.buffer;
      }
      if (st.alg.name === "RSA-OAEP") {
        if (st.kind !== "public") throw new TypeError("encrypt: not an RSA public key");
        const hash = __wjs_normHash(algorithm?.hash ?? st.alg.hash ?? "SHA-256");
        const label = algorithm?.label === undefined ? undefined : __wjs_dataBytes(algorithm.label);
        const out = __wjs_rsa_encrypt(hash, st.material, __wjs_dataBytes(data), label);
        return out.buffer;
      }
      throw new TypeError("encrypt: unsupported key");
    },
    async decrypt(algorithm, key, data) {
      const st = __wjs_keyState.get(key);
      if (!st) throw new TypeError("decrypt: not a CryptoKey");
      __wjs_needUsage(st, "decrypt");
      if (st.alg.name === "AES-GCM") {
        const p = __wjs_aesParams(algorithm);
        const out = __wjs_aesgcm_decrypt(st.material, p.iv, p.aad, __wjs_dataBytes(data));
        return out.buffer;
      }
      if (st.alg.name === "RSA-OAEP") {
        if (st.kind !== "private") throw new TypeError("decrypt: not an RSA private key");
        const hash = __wjs_normHash(algorithm?.hash ?? st.alg.hash ?? "SHA-256");
        const label = algorithm?.label === undefined ? undefined : __wjs_dataBytes(algorithm.label);
        const out = __wjs_rsa_decrypt(hash, st.material, __wjs_dataBytes(data), label);
        return out.buffer;
      }
      throw new TypeError("decrypt: unsupported key");
    },
    async sign(algorithm, key, data) {
      const st = __wjs_keyState.get(key);
      if (!st) throw new TypeError("sign: not a CryptoKey");
      __wjs_needUsage(st, "sign");
      if (st.alg.name === "HMAC") {
        const out = __wjs_hmac_sign(st.alg.hash, st.material, __wjs_dataBytes(data));
        return out.buffer;
      }
      if (st.alg.name === "RSASSA-PKCS1-V1_5") {
        if (st.kind !== "private") throw new TypeError("sign: not an RSA private key");
        const hash = __wjs_normHash(algorithm?.hash ?? st.alg.hash ?? "SHA-256");
        const out = __wjs_rsa_sign(hash, st.material, __wjs_dataBytes(data));
        return out.buffer;
      }
      if (st.alg.name === "RSA-PSS") {
        if (st.kind !== "private") throw new TypeError("sign: not an RSA private key");
        const hash = __wjs_normHash(algorithm?.hash ?? st.alg.hash ?? "SHA-256");
        // 缺省 saltLength = digest 长度（WebCrypto 口径）。
        const defSalt = { "SHA-256": 32, "SHA-384": 48, "SHA-512": 64 }[hash];
        const salt = algorithm?.saltLength === undefined ? defSalt : Number(algorithm.saltLength);
        if (!Number.isInteger(salt) || salt < 0) throw new Error("OperationError: bad RSA-PSS saltLength");
        const out = __wjs_pss_sign(hash, salt, st.material, __wjs_dataBytes(data));
        return out.buffer;
      }
      if (st.alg.name === "ED25519") {
        if (st.kind !== "private") throw new TypeError("sign: not an Ed25519 private key");
        const out = __wjs_ed_sign(st.material, __wjs_dataBytes(data));
        return out.buffer;
      }
      if (st.alg.name === "ECDSA") {
        if (st.kind !== "private") throw new TypeError("sign: not an EC private key");
        const hash = __wjs_normHash(algorithm?.hash ?? "SHA-256");
        const out = __wjs_ecdsa_sign(st.alg.namedCurve, hash, st.material, __wjs_dataBytes(data));
        return out.buffer;
      }
      throw new TypeError("sign: unsupported key");
    },
    async verify(algorithm, key, signature, data) {
      const st = __wjs_keyState.get(key);
      if (!st) throw new TypeError("verify: not a CryptoKey");
      __wjs_needUsage(st, "verify");
      if (st.alg.name === "HMAC") {
        return __wjs_hmac_verify(st.alg.hash, st.material, __wjs_dataBytes(signature), __wjs_dataBytes(data));
      }
      if (st.alg.name === "RSASSA-PKCS1-V1_5") {
        if (st.kind === "private") throw new TypeError("verify: not an RSA public key");
        const hash = __wjs_normHash(algorithm?.hash ?? st.alg.hash ?? "SHA-256");
        return __wjs_rsa_verify(hash, st.material, __wjs_dataBytes(signature), __wjs_dataBytes(data));
      }
      if (st.alg.name === "RSA-PSS") {
        if (st.kind === "private") throw new TypeError("verify: not an RSA public key");
        const hash = __wjs_normHash(algorithm?.hash ?? st.alg.hash ?? "SHA-256");
        const defSalt = { "SHA-256": 32, "SHA-384": 48, "SHA-512": 64 }[hash];
        const salt = algorithm?.saltLength === undefined ? defSalt : Number(algorithm.saltLength);
        if (!Number.isInteger(salt) || salt < 0) throw new Error("OperationError: bad RSA-PSS saltLength");
        return __wjs_pss_verify(hash, salt, st.material, __wjs_dataBytes(signature), __wjs_dataBytes(data));
      }
      if (st.alg.name === "ED25519") {
        if (st.kind === "private") throw new TypeError("verify: not an Ed25519 public key");
        return __wjs_ed_verify(st.material, __wjs_dataBytes(signature), __wjs_dataBytes(data));
      }
      if (st.alg.name === "ECDSA") {
        if (st.kind === "private") throw new TypeError("verify: not an EC public key");
        const hash = __wjs_normHash(algorithm?.hash ?? "SHA-256");
        return __wjs_ecdsa_verify(st.alg.namedCurve, hash, st.material, __wjs_dataBytes(signature), __wjs_dataBytes(data));
      }
      throw new TypeError("verify: unsupported key");
    },
    async deriveBits(algorithm, baseKey, length) {
      const st = __wjs_keyState.get(baseKey);
      if (!st || (st.alg.name !== "ECDH" && st.alg.name !== "X25519")) {
        throw new TypeError("deriveBits: not an ECDH/X25519 key");
      }
      if (st.kind !== "private") throw new TypeError("deriveBits: needs a private key");
      __wjs_needUsage(st, "deriveBits");
      if (st.alg.name === "X25519") return __wjs_x_bits(algorithm, st, length);
      return __wjs_ecdh_bits(algorithm, st, length);
    },
    async deriveKey(algorithm, baseKey, derivedKeyAlg, extractable, usages) {
      const st = __wjs_keyState.get(baseKey);
      if (!st || (st.alg.name !== "ECDH" && st.alg.name !== "X25519")) {
        throw new TypeError("deriveKey: not an ECDH/X25519 key");
      }
      if (st.kind !== "private") throw new TypeError("deriveKey: needs a private key");
      __wjs_needUsage(st, "deriveKey");
      const bitsOf = (length) => st.alg.name === "X25519"
        ? __wjs_x_bits(algorithm, st, length)
        : __wjs_ecdh_bits(algorithm, st, length);
      const dname = String(derivedKeyAlg?.name ?? "").toUpperCase();
      let bytes;
      if (dname === "AES-GCM") {
        const length = Number(derivedKeyAlg?.length ?? 256);
        if (![128, 192, 256].includes(length)) throw new Error("NotSupportedError: derived AES-GCM length must be 128/192/256");
        bytes = new Uint8Array(bitsOf(length));
        return __wjs_makeKey({ name: "AES-GCM", length }, bytes, [...(usages ?? [])].map(String), !!extractable);
      }
      if (dname === "HMAC") {
        const hash = __wjs_normHash(derivedKeyAlg?.hash);
        let length = derivedKeyAlg?.length === undefined ? null : Number(derivedKeyAlg.length);
        bytes = new Uint8Array(bitsOf(length));
        if (length === null) length = bytes.length * 8;
        return __wjs_makeKey({ name: "HMAC", hash, length }, bytes, [...(usages ?? [])].map(String), !!extractable);
      }
      throw new Error(`NotSupportedError: deriveKey to ${dname || "?"} needs follow-up`);
    },
  },
};
"#;
