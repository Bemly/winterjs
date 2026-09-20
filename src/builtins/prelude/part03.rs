//! prelude part 03 (byte-exact slice; order matters, see prelude/mod.rs).
pub const PART_03: &str = r#"
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
// ---- M5: 全局 Event / EventTarget / CustomEvent（Node 平坦派发口径）----
// Node 的 EventTarget 不实现捕获/冒泡 propagation path（官方文档明言）：
// capture 选项仅为 removeEventListener 匹配保留；listener 收函数或 {handleEvent}。
// 事件状态走共享 WeakMap（Event 与 EventTarget 跨类要读写字段，# 私有够不着；
// 与既有 __wjs_abortState 同风格，前缀避免污染全局面）。
const __wjs_eventState = new WeakMap();
const __wjs_etState = new WeakMap();
globalThis.Event = class Event {
  constructor(type, options = {}) {
    if (arguments.length === 0) throw new TypeError("Event requires at least 1 argument, but only 0 were passed");
    const o = options ?? {};
    __wjs_eventState.set(this, {
      type: String(type),
      bubbles: !!o.bubbles,
      cancelable: !!o.cancelable,
      composed: !!o.composed,
      defaultPrevented: false,
      stopped: false,
      immediate: false,
      dispatching: false,
      timeStamp: Date.now(),
      target: null,
      currentTarget: null,
    });
  }
  get type() { return __wjs_eventState.get(this).type; }
  get bubbles() { return __wjs_eventState.get(this).bubbles; }
  get cancelable() { return __wjs_eventState.get(this).cancelable; }
  get composed() { return __wjs_eventState.get(this).composed; }
  get timeStamp() { return __wjs_eventState.get(this).timeStamp; }
  get defaultPrevented() { return __wjs_eventState.get(this).defaultPrevented; }
  get target() { return __wjs_eventState.get(this).target; }
  get currentTarget() { return __wjs_eventState.get(this).currentTarget; }
  get srcElement() { return __wjs_eventState.get(this).target; }
  get isTrusted() { return false; }
  preventDefault() {
    const s = __wjs_eventState.get(this);
    if (s.cancelable) s.defaultPrevented = true;
  }
  stopPropagation() { __wjs_eventState.get(this).stopped = true; }
  stopImmediatePropagation() {
    const s = __wjs_eventState.get(this);
    s.stopped = true;
    s.immediate = true;
  }
};
globalThis.CustomEvent = class CustomEvent extends Event {
  #detail;
  constructor(type, options = {}) {
    super(type, options);
    this.#detail = (options ?? {}).detail ?? null;
  }
  get detail() { return this.#detail; }
};
// undici webidl 口径的值回显（MessageEvent 校验文案；真机逐形实测）：
// instanceOf 消息 = `"` + inspect(v, {quotes:'double'}) + `"`（"str" 形串自带
// 双引号故现 `""str""`；数字/容器仅外包一对）；not-iterable 用裸 inspect。
// 覆盖套件点名的形状（标量/空容器/数组/类实例），完整 inspect 面在 util。
const __wjs_insp = (v) => {
  if (v === null) return "null";
  if (v === undefined) return "undefined";
  const t = typeof v;
  if (t === "string") {
    const body = v.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
    return `"${body}"`;
  }
  if (t === "number" || t === "boolean" || t === "bigint") return String(v);
  if (t === "symbol") return v.toString();
  if (t === "function") return `[Function: ${v.name || "(anonymous)"}]`;
  if (Array.isArray(v)) return `[ ${v.map((x) => __wjs_insp(x)).join(", ")} ]`;
  const n = v.constructor && v.constructor.name && v.constructor.name !== "Object" ? v.constructor.name : null;
  const keys = Object.keys(v);
  const body = keys.length === 0 ? "" : ` ${keys.map((k) => `${k}: ${__wjs_insp(v[k])}`).join(", ")} `;
  return n ? `${n} {${body}}` : `{${body}}`;
};
const __wjs_inspQuoted = (v) => `"${__wjs_insp(v)}"`;
// Rust 侧取 symbol 描述（ToString 对 symbol 抛 TypeError；JS 侧 toString 合法）。
globalThis.__wjs_symToString = (v) => (typeof v === "symbol") ? v.toString() : null;
// 10f：全局 MessageEvent（node 26 主/worker 线程均全局；message-port/
// message-event 套件逐项对拍）。source/ports 须 MessagePort 实例——品牌经
// worker 模块求值期登记的 `__wjs_MessagePort` 隐藏槽判定（主线程无全局
// MessagePort；求值前无从有端口，非 null source 即 TypeError 正确）。
globalThis.MessageEvent = class MessageEvent extends Event {
  #data; #origin; #lastEventId; #source; #ports;
  constructor(type, init = {}) {
    if (arguments.length === 0) throw new TypeError("MessageEvent requires at least 1 argument, but only 0 were passed");
    super(type, init);
    const o = init ?? {};
    this.#data = o.data ?? null;
    this.#origin = String(o.origin ?? "");
    this.#lastEventId = String(o.lastEventId ?? "");
    const src = o.source ?? null;
    if (src !== null) {
      const M = globalThis.__wjs_MessagePort;
      if (!M || !(src instanceof M)) {
        throw new TypeError(`MessageEvent constructor: Expected eventInitDict.source (${__wjs_inspQuoted(src)}) to be an instance of MessagePort.`);
      }
    }
    this.#source = src;
    let ports = o.ports;
    if (ports !== undefined && ports !== null) {
      if (typeof ports[Symbol.iterator] !== "function") {
        throw new TypeError(`MessageEvent constructor: eventInitDict.ports (${__wjs_insp(ports)}) is not iterable.`);
      }
      const list = [...ports];
      for (let i = 0; i < list.length; i++) {
        const M2 = globalThis.__wjs_MessagePort;
        if (!M2 || !(list[i] instanceof M2)) {
          throw new TypeError(`MessageEvent constructor: Expected eventInitDict.ports[${i}] (${__wjs_inspQuoted(list[i])}) to be an instance of MessagePort.`);
        }
      }
      this.#ports = list;
    } else {
      this.#ports = [];
    }
    // 内部派发目标（`__wjsTarget` 不属 WebIDL 字典面，仅宿主 port 桥使用）。
    const tgt = o.__wjsTarget;
    if (tgt) __wjs_eventState.get(this).target = tgt;
  }
  get data() { return this.#data; }
  get origin() { return this.#origin; }
  get lastEventId() { return this.#lastEventId; }
  get source() { return this.#source; }
  get ports() { return this.#ports; }
};
globalThis.EventTarget = class EventTarget {
  constructor() {
    __wjs_etState.set(this, new Map());
  }
  addEventListener(type, listener, options = {}) {
    if (arguments.length < 2) throw new TypeError("addEventListener requires at least 2 arguments");
    if (typeof listener !== "function" && (typeof listener !== "object" || listener === null || typeof listener.handleEvent !== "function")) {
      throw new TypeError("addEventListener: listener must be a function or an object with handleEvent");
    }
    const o = typeof options === "boolean" ? { capture: options } : (options ?? {});
    if (o.signal?.aborted) return;
    // Proxy 目标无表决不抛（mustNotMutate 包裹的 signal 形 addEventListener；
    // 监听记代理身份下，触发侧 miss 即 benign——abort 竞速由 aborted 轮询门覆盖）。
    let st = __wjs_etState.get(this);
    if (!st) { st = new Map(); __wjs_etState.set(this, st); }
    const key = String(type);
    const list = st.get(key) ?? [];
    if (list.some((e) => e.listener === listener && e.capture === !!o.capture)) return;
    const entry = { listener, once: !!o.once, capture: !!o.capture, signal: o.signal ?? null, removed: false };
    list.push(entry);
    st.set(key, list);
    if (o.signal) o.signal.addEventListener("abort", () => this.removeEventListener(key, listener, options), { once: true });
  }
  removeEventListener(type, listener, options = {}) {
    const o = typeof options === "boolean" ? { capture: options } : (options ?? {});
    const st = __wjs_etState.get(this);
    if (!st) return;
    const list = st.get(String(type));
    if (!list) return;
    const i = list.findIndex((e) => e.listener === listener && e.capture === !!o.capture && !e.removed);
    if (i >= 0) {
      list[i].removed = true;
      list.splice(i, 1);
    }
  }
  dispatchEvent(event) {
    if (!(event instanceof Event)) throw new TypeError("dispatchEvent requires an Event instance");
    const es = __wjs_eventState.get(event);
    if (es.dispatching) throw new Error("InvalidStateError: event is already being dispatched");
    const st = __wjs_etState.get(this);
    if (!st) throw new TypeError("dispatchEvent called on non-EventTarget");
    es.target = this;
    es.dispatching = true;
    const list = (st.get(es.type) ?? []).slice();
    try {
      for (const entry of list) {
        if (es.immediate || entry.removed) continue;
        if (entry.signal?.aborted) continue;
        if (entry.once) this.removeEventListener(es.type, entry.listener, { capture: entry.capture });
        es.currentTarget = this;
        if (typeof entry.listener === "function") {
          entry.listener.call(this, event);
        } else {
          entry.listener.handleEvent(event);
        }
      }
    } finally {
      es.dispatching = false;
      es.currentTarget = null;
    }
    return !(es.cancelable && es.defaultPrevented);
  }
};

// ---- Phase 3b: Headers / Request / Response / fetch ----
// AbortSignal 重构到全局 EventTarget 基类（Node 同构：signal 即 EventTarget，
// abort 走 dispatchEvent；监听登记/移除/once/signal 选项全由基类承载）。
const __wjs_abortState = new WeakMap();
// Proxy 穿透键（mustNotMutateObjectDeep 包信号形）：Node 套件把 { signal }
//  deep-proxy 后再传入，WeakMap 的精确身份键即断裂（get→undefined→
//  TypeError）。状态同时挂 symbol 自有属性——读经 Proxy 转发仍命中目标本体；
//  写入只发生在构造/触发期（真实对象），永不穿过 Proxy 的 set 陷阱。
const __wjs_abortSym = Symbol("winterjs.abortState");
function __wjs_abortStateOf(signal) {
  return __wjs_abortState.get(signal) ?? signal[__wjs_abortSym];
}
function __wjs_abortFire(signal, reason) {
  const st = __wjs_abortStateOf(signal);
  if (!st || st.aborted) return;
  st.aborted = true;
  st.reason = reason === undefined
    ? new DOMException("This operation was aborted", "AbortError")
    : reason;
  const event = new Event("abort");
  // onabort 独立属性路径（Node 同为 getter/setter 而非 EventTarget on* 表）；
  // 沿既有口径吞错（abort 链失败不该炸用户回调）。
  if (typeof st.onabort === "function") {
    try { st.onabort.call(signal, event); } catch {}
  }
  signal.dispatchEvent(event);
}
globalThis.AbortSignal = class AbortSignal extends EventTarget {
  constructor() {
    super();
    const st = { aborted: false, reason: undefined, onabort: null };
    __wjs_abortState.set(this, st);
    this[__wjs_abortSym] = st;
  }
  get aborted() { return __wjs_abortStateOf(this).aborted; }
  get reason() { return __wjs_abortStateOf(this).reason; }
  get onabort() { return __wjs_abortStateOf(this).onabort; }
  set onabort(cb) { __wjs_abortStateOf(this).onabort = typeof cb === "function" ? cb : null; }
  throwIfAborted() {
    const st = __wjs_abortStateOf(this);
    if (st.aborted) throw st.reason;
  }
  static abort(reason) {
    const s = new AbortSignal();
    __wjs_abortFire(s, reason);
    return s;
  }
  static timeout(ms) {
    const c = new AbortController();
    const t = Number(ms);
    if (!Number.isFinite(t) || t < 0) throw new TypeError("AbortSignal.timeout needs a non-negative delay");
    setTimeout(() => c.abort(new DOMException("The operation was aborted due to timeout", "TimeoutError")), t);
    return c.signal;
  }
  static any(signals) {
    const list = [...(signals ?? [])];
    const c = new AbortController();
    for (const s of list) {
      if (!(s instanceof AbortSignal)) throw new TypeError("AbortSignal.any needs AbortSignals");
      if (s.aborted) { c.abort(s.reason); break; }
      s.addEventListener("abort", () => c.abort(s.reason), { once: true });
    }
    return c.signal;
  }
};
globalThis.AbortController = class AbortController {
  #signal;
  constructor() { this.#signal = new AbortSignal(); }
  get signal() { return this.#signal; }
  abort(reason) { __wjs_abortFire(this.#signal, reason); }
};
globalThis.Headers = class Headers {
  #pairs;
  constructor(init) {
    this.#pairs = [];
    if (init === undefined) return;
    if (init instanceof Headers) { for (const [k, v] of init) this.append(k, v); }
    else if (Array.isArray(init)) { for (const [k, v] of init) this.append(String(k), String(v)); }
    else if (typeof init === "object" && init !== null) {
      for (const [k, v] of Object.entries(init)) this.append(k, String(v));
    } else throw new TypeError("Headers: unsupported init");
  }
  static #norm(n) { return String(n).trim().toLowerCase(); }
  append(n, v) { this.#pairs.push([Headers.#norm(n), String(v).trim()]); }
  delete(n) { n = Headers.#norm(n); this.#pairs = this.#pairs.filter((p) => p[0] !== n); }
  get(n) {
    n = Headers.#norm(n);
    const vs = this.#pairs.filter((p) => p[0] === n).map((p) => p[1]);
    return vs.length ? vs.join(", ") : null;
  }
  getSetCookie() {
    return this.#pairs.filter((p) => p[0] === "set-cookie").map((p) => p[1]);
  }
  has(n) { n = Headers.#norm(n); return this.#pairs.some((p) => p[0] === n); }
  set(n, v) {
    n = Headers.#norm(n); v = String(v).trim();
    let found = false;
    this.#pairs = this.#pairs.filter((p) => {
      if (p[0] !== n) return true;
      if (!found) { p[1] = v; found = true; return true; }
      return false;
    });
    if (!found) this.#pairs.push([n, v]);
  }
  *keys() { for (const [k] of this.#sorted()) yield k; }
  *values() { for (const [, v] of this.#sorted()) yield v; }
  *entries() { for (const p of this.#sorted()) yield p; }
  [Symbol.iterator]() { return this.entries(); }
  forEach(cb, thisArg) { for (const [k, v] of this.#sorted()) cb.call(thisArg, v, k, this); }
  #sorted() { return [...this.#pairs].sort((a, b) => a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0); }
};
const __wjs_respState = new WeakMap();
function __wjs_respInit(resp, s) {
  __wjs_respState.set(resp, {
    status: s.status, statusText: s.statusText ?? "", headers: s.headers,
    url: s.url ?? "", bodyU8: s.bodyU8 ?? null, streamId: s.streamId ?? null, bodyUsed: false,
  });
}
function __wjs_takeBody(resp, what) {
  const st = __wjs_respState.get(resp);
  if (st.bodyUsed) throw new TypeError(`${what}: body already used`);
  st.bodyUsed = true;
  return st.bodyU8;
}
// 流式/快照统一建流（body getter 与 text 系共用；bodyUsed 由调用方维护）。
function __wjs_respStream(resp) {
  const st = __wjs_respState.get(resp);
  if (!st.bodyStream) {
    if (st.streamId !== null && st.streamId !== undefined) {
      const sid = st.streamId;
      st.bodyStream = new ReadableStream({
        pull(c) {
          return new Promise((resolve, reject) => {
            // 中止后 pull 直接拒绝（Rust 状态已摘，不再进 native）。
            if (__wjs_abortedFetch.has(sid)) {
              reject(new Error("AbortError: fetch aborted"));
              return;
            }
            __wjs_fetch_pull(sid,
              (chunk) => {
                if (chunk === null || chunk === undefined) {
                  try { c.close(); } catch {}
                  __wjs_fetchCleanup(sid);
                  resolve();
                  return;
                }
                try { c.enqueue(chunk); } catch (e) { reject(e); return; }
                resolve();
              },
              (e) => reject(e));
          });
        },
        cancel() { __wjs_fetchCleanup(sid); __wjs_fetch_abort(sid); },
      });
    } else {
      const bytes = st.bodyU8 ? st.bodyU8.slice() : new Uint8Array(0);
      st.bodyStream = new ReadableStream({
        start(c) { if (bytes.length) c.enqueue(bytes); c.close(); },
      });
    }
  }
  return st.bodyStream;
}
// 取全量字节（流式即读完流；快照即原路径；读即标记 disturbed）。
async function __wjs_respStreamBytes(resp, what) {
  const st = __wjs_respState.get(resp);
  if (st.streamId !== null && st.streamId !== undefined) {
    if (st.bodyUsed) throw new TypeError(`${what}: body already used`);
    st.bodyUsed = true;
    const chunks = [];
    let total = 0;
    for await (const c of __wjs_respStream(resp)) {
      const u8 = c instanceof Uint8Array ? c : new Uint8Array(c);
      chunks.push(u8);
      total += u8.length;
    }
    const out = new Uint8Array(total);
    let off = 0;
    for (const u8 of chunks) { out.set(u8, off); off += u8.length; }
    return out;
  }
  const b = __wjs_takeBody(resp, what);
  return b ? b.slice() : new Uint8Array(0);
}
function __wjs_normBody(body, what) {
  if (body === undefined || body === null) return null;
  if (typeof body === "string") return new TextEncoder().encode(body);
  if (body instanceof URLSearchParams) return new TextEncoder().encode(body.toString());
  if (body instanceof Uint8Array) return body.slice();
  if (body instanceof ArrayBuffer) return new Uint8Array(body.slice(0));
  throw new TypeError(`${what}: unsupported body type`);
}
function __wjs_fillHeaders(headers, init) {
  if (init === undefined) return;
  if (init instanceof Headers) { for (const [k, v] of init) headers.append(k, v); }
  else if (Array.isArray(init)) { for (const [k, v] of init) headers.append(String(k), String(v)); }
  else if (typeof init === "object" && init !== null) {
    for (const [k, v] of Object.entries(init)) headers.append(k, String(v));
  } else throw new TypeError("Headers: unsupported init");
}
globalThis.Response = class Response {
  constructor(body, init = {}) {
    const bytes = __wjs_normBody(body, "Response");
    const status = init.status === undefined ? 200 : Number(init.status);
    if (!Number.isInteger(status) || status < 200 || status > 599) {
      throw new RangeError("Response status must be 200-599");
    }
    const headers = new Headers();
    __wjs_fillHeaders(headers, init.headers);
    __wjs_respInit(this, {
      status, headers, url: "",
      statusText: init.statusText === undefined ? "" : String(init.statusText),
      bodyU8: bytes,
    });
  }
  get status() { return __wjs_respState.get(this).status; }
  get statusText() { return __wjs_respState.get(this).statusText; }
  get headers() { return __wjs_respState.get(this).headers; }
  get url() { return __wjs_respState.get(this).url; }
  get ok() { const s = this.status; return s >= 200 && s < 300; }
  get bodyUsed() { return __wjs_respState.get(this).bodyUsed; }
  get body() {
    const st = __wjs_respState.get(this);
    if (st.bodyUsed) return null;
    return __wjs_respStream(this);
  }
  async text() { return new TextDecoder().decode(await __wjs_respStreamBytes(this, "Response.text")); }
  async json() { return JSON.parse(await this.text()); }
  async arrayBuffer() { const b = await __wjs_respStreamBytes(this, "Response.arrayBuffer"); return b.slice().buffer; }
  async bytes() { return __wjs_respStreamBytes(this, "Response.bytes"); }
  static error() {
    const r = new Response(null);
    __wjs_respInit(r, { status: 0, statusText: "", headers: new Headers(), url: "", bodyU8: null });
    return r;
  }
  static redirect(url, status = 302) {
    if (![301, 302, 303, 307, 308].includes(status)) throw new RangeError("redirect status must be 301/302/303/307/308");
    const h = new Headers();
    h.set("location", String(url));
    return new Response(null, { status, headers: h });
  }
};
const __wjs_reqState = new WeakMap();
function __wjs_takeReqBody(req) {
  const st = __wjs_reqState.get(req);
  if (st.bodyUsed) throw new TypeError("Request body already used");
  st.bodyUsed = true;
  return st.bodyU8;
}
// serve 请求体流（plan4 T1）：streamId 由 Rust Head 分发置入，chunk 经既有
// `__wjs_fetch_pull` 拉取（fetch 流机制复用，零新 native）；`__wjs_respStream`
// 同构（快照分支/流分支/读完标记三件照抄，`Response.body` 语义对等）。
function __wjs_reqStream(req) {
  const st = __wjs_reqState.get(req);
  if (!st.reqStream) {
    if (st.streamId !== null && st.streamId !== undefined) {
      const sid = st.streamId;
      st.reqStream = new ReadableStream({
        pull(c) {
          return new Promise((resolve, reject) => {
            __wjs_fetch_pull(sid,
              (chunk) => {
                if (chunk === null || chunk === undefined) {
                  try { c.close(); } catch {}
"#;
