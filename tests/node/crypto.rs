//! tests/node/crypto.rs — 对齐 src/builtins/node/crypto.rs（node:crypto + 9h/9i x509/PQ）。

use crate::common::*;
use crate::helpers::*;
use assert_fs::prelude::*;

#[test]
fn phase9e_crypto_hash_hmac() {
    // 真 Node 取证向量（HMAC-SHA256/MD5/SHA3-256 + BLAKE2b/SHA3-512，逐字节对）
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import c, { createHash, createHmac, hash, getHashes, getCurves } from "node:crypto";
console.log("sha", createHash("sha256").update("a").update("b").digest("hex") === "fb8e20fc2e4c3f248c60c39bd652f3c1347298bb977b8b4d5903b85055620603");
console.log("hmac", createHmac("sha256", "key").update("The quick brown fox jumps over the lazy dog").digest("hex") === "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8");
console.log("hmac-md5", createHmac("md5", "key").update("msg").digest("hex") === "18e3548c59ad40dd03907b7aeee71d67");
console.log("hmac-s3", createHmac("sha3-256", "key").update("msg").digest("hex") === "56b616feab81d996beb8cf47719b253cfe6d1da9be562c63520fef130a6d935e");
console.log("blake", createHash("blake2b512").update("abc").digest("hex").slice(0, 32) === "ba80a53f981c4d0d6a2797b69f12f6e9");
console.log("md5vec", createHash("md5").update("abc").digest("hex") === "900150983cd24fb0d6963f7d28e17f72");
const h = createHash("sha256"); h.update("a"); const h2 = h.copy();
console.log("copy", h2.update("b").digest("hex") === createHash("sha256").update("ab").digest("hex"));
console.log("buf", Buffer.isBuffer(createHash("sha256").update("x").digest()), createHash("sha256").update("x").digest("hex").length === 64);
console.log("oneshot", hash("sha256", "abc", "hex") === "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
console.log("alias", createHash("RSA-SHA256").update("x").digest("hex").slice(0, 8) === createHash("sha256").update("x").digest("hex").slice(0, 8));
console.log("hashes", getHashes().includes("sha256") && getHashes().includes("blake2s256") && getHashes().includes("ripemd160") && getHashes().includes("shake256"));
console.log("curves", getCurves().includes("prime256v1") && getCurves().includes("ed25519"));
console.log("ns", typeof c.createHash === "function", c.webcrypto === globalThis.crypto);
"#,
    );
    assert!(out.contains("sha true"), "out: {out}");
    assert!(out.contains("hmac true"), "out: {out}");
    assert!(out.contains("hmac-md5 true"), "out: {out}");
    assert!(out.contains("hmac-s3 true"), "out: {out}");
    assert!(out.contains("blake true"), "out: {out}");
    assert!(out.contains("md5vec true"), "out: {out}");
    assert!(out.contains("copy true"), "out: {out}");
    assert!(out.contains("buf true true"), "out: {out}");
    assert!(out.contains("oneshot true"), "out: {out}");
    assert!(out.contains("alias true"), "out: {out}");
    assert!(out.contains("hashes true"), "out: {out}");
    assert!(out.contains("curves true"), "out: {out}");
    assert!(out.contains("ns true true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9e_crypto_random() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { randomBytes, randomFill, randomFillSync, randomInt, randomUUID, randomUUIDv7, timingSafeEqual } from "node:crypto";
console.log("rb", randomBytes(16).length === 16, Buffer.isBuffer(randomBytes(4)));
randomBytes(8, (e, b) => {
  console.log("rbcb", e === null, b.length === 8);
  randomInt(1, 7, (e2, v) => {
    console.log("ricb", e2 === null, v >= 1 && v < 7);
    const buf = Buffer.alloc(8);
    randomFill(buf, 2, 4, (e3, out) => {
      console.log("rfcb", e3 === null, out === buf);
      console.log("done");
    });
  });
});
console.log("ri", randomInt(5) >= 0 && randomInt(5) < 5, randomInt(3, 4) === 3);
const u = randomUUID();
console.log("uuid", u.length === 36 && u[14] === "4");
const v7 = randomUUIDv7();
console.log("uuid7", v7.length === 36 && v7[14] === "7" && v7 !== randomUUIDv7());
const f = Buffer.alloc(4); randomFillSync(f);
console.log("rfsync", f.length === 4, randomFillSync(new Uint8Array(3)).length === 3);
console.log("tse", timingSafeEqual(Buffer.from([1, 2]), Buffer.from([1, 2])) === true,
  timingSafeEqual(Buffer.from([1, 2]), Buffer.from([1, 3])) === false);
"#,
    );
    assert!(out.contains("rb true true"), "out: {out}");
    assert!(out.contains("rbcb true true"), "out: {out}");
    assert!(out.contains("ricb true true"), "out: {out}");
    assert!(out.contains("rfcb true true"), "out: {out}");
    assert!(out.contains("done"), "out: {out}");
    assert!(out.contains("ri true true"), "out: {out}");
    assert!(out.contains("uuid true"), "out: {out}");
    assert!(out.contains("uuid7 true"), "out: {out}");
    assert!(out.contains("rfsync true true"), "out: {out}");
    assert!(out.contains("tse true true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9e_crypto_errors_boundary() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { createHash, createHmac, randomBytes, randomInt, randomFillSync, timingSafeEqual, hash } from "node:crypto";
// 报错：未知算法（Hash 无码原文 / Hmac 有码，俱为真 Node 口径）
try { createHash("nope"); } catch (e) { console.log("halg", e.code === undefined, e.message); }
try { createHmac("nope", "k"); } catch (e) { console.log("malg", e.code === "ERR_CRYPTO_INVALID_DIGEST"); }
try { createHash(123); } catch (e) { console.log("halgtype", e.code === "ERR_INVALID_ARG_TYPE"); }
// 报错：finalized 后 update/copy（真 Node 同码）
try { const h = createHash("sha256"); h.digest(); h.update("x"); } catch (e) { console.log("fin", e.code === "ERR_CRYPTO_HASH_FINALIZED"); }
try { const h = createHash("sha256"); h.digest(); h.copy(); } catch (e) { console.log("fincopy", e.code === "ERR_CRYPTO_HASH_FINALIZED"); }
// 报错：随机数形状
try { randomBytes(-1); } catch (e) { console.log("rneg", e.code === "ERR_OUT_OF_RANGE"); }
try { randomInt(5, 5); } catch (e) { console.log("rrange", e.code === "ERR_OUT_OF_RANGE"); }
try { randomInt(); } catch (e) { console.log("rinttype", e.code === "ERR_INVALID_ARG_TYPE"); }
try { randomFillSync("no"); } catch (e) { console.log("rftype", e.code === "ERR_INVALID_ARG_TYPE"); }
try { timingSafeEqual(Buffer.from([1]), Buffer.from([1, 2])); } catch (e) { console.log("tse", e.code === "ERR_CRYPTO_TIMING_SAFE_EQUAL_LENGTH"); }
try { hash("sha256", "x", "nope"); } catch (e) { console.log("henc", e.code === "ERR_INVALID_ARG_VALUE"); }
// 边界：未知输出编码回 Buffer（真 Node 宽容口径）；空输入；大块 1MB 往返一致
const enc = createHash("sha256").update("x").digest("nope");
console.log("badenc", Buffer.isBuffer(enc));
console.log("empty", createHash("sha256").update("").digest("hex") === "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
const big = "ab".repeat(524288);
console.log("big", createHash("sha256").update(big).digest("hex") === createHash("sha256").update(big).digest("hex"));
"#,
    );
    assert!(out.contains("halg true Digest method not supported"), "out: {out}");
    assert!(out.contains("malg true"), "out: {out}");
    assert!(out.contains("halgtype true"), "out: {out}");
    assert!(out.contains("fin true"), "out: {out}");
    assert!(out.contains("fincopy true"), "out: {out}");
    assert!(out.contains("rneg true"), "out: {out}");
    assert!(out.contains("rrange true"), "out: {out}");
    assert!(out.contains("rinttype true"), "out: {out}");
    assert!(out.contains("rftype true"), "out: {out}");
    assert!(out.contains("tse true"), "out: {out}");
    assert!(out.contains("henc true"), "out: {out}");
    assert!(out.contains("badenc true"), "out: {out}");
    assert!(out.contains("empty true"), "out: {out}");
    assert!(out.contains("big true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9e_crypto_cipher_roundtrip() {
    // 真 Node 取证向量（逐字节对；gcm/chacha tag 另断长度）
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { createCipheriv, createDecipheriv, getCiphers, getCipherInfo } from "node:crypto";
const key = Buffer.alloc(32, 1), iv16 = Buffer.alloc(16, 2), iv12 = Buffer.alloc(12, 3);
const x = createCipheriv("aes-256-cbc", key, iv16);
console.log("cbc", x.update("hello world", "utf8", "hex") + x.final("hex"));
const e = createCipheriv("aes-256-cbc", key, iv16);
const ct = Buffer.concat([e.update("hi"), e.final()]);
const d = createDecipheriv("aes-256-cbc", key, iv16);
console.log("dec", d.update(ct).toString() + d.final("utf8"));
// 流式多 update 与 oneshot 等价
const a = createCipheriv("aes-256-cbc", key, iv16);
const p1 = a.update("hel", "utf8", "hex") + a.update("lo world", "utf8", "hex") + a.final("hex");
console.log("stream", p1 === "f563737a376afbed282274255a7fcabd");
const g = createCipheriv("aes-256-gcm", key, iv12);
g.setAAD(Buffer.from("aad"));
console.log("gcm", g.update("secret", "utf8", "hex") + g.final("hex"), g.getAuthTag().length);
const gd = createDecipheriv("aes-256-gcm", key, iv12);
gd.setAAD(Buffer.from("aad")); gd.setAuthTag(g.getAuthTag());
const gct = Buffer.from("8b0477e89af0", "hex");
console.log("gdec", gd.update(gct).toString() + gd.final("utf8"));
const ch = createCipheriv("chacha20-poly1305", key, iv12);
console.log("chacha", ch.update("hello", "utf8", "hex") + ch.final("hex"), ch.getAuthTag().length);
const chd = createDecipheriv("chacha20-poly1305", key, iv12);
chd.setAuthTag(ch.getAuthTag());
console.log("chdec", chd.update(Buffer.from("e66dea2709", "hex")).toString() + chd.final("utf8"));
const t = createCipheriv("aes-128-ctr", Buffer.alloc(16, 7), iv16);
console.log("ctr", t.update("0123456789abcdef", "utf8", "hex") + t.final("hex"));
console.log("list", getCiphers().includes("aes-256-gcm") && getCiphers().includes("des-ede3-cbc"));
const info = getCipherInfo("aes-256-cbc");
console.log("info", info.mode === "cbc" && info.keyLength === 32 && info.ivLength === 16 && info.nid === 427);
console.log("nounk", getCipherInfo("nope") === undefined);
"#,
    );
    assert!(out.contains("cbc f563737a376afbed282274255a7fcabd"), "out: {out}");
    assert!(out.contains("dec hi"), "out: {out}");
    assert!(out.contains("stream true"), "out: {out}");
    assert!(out.contains("gcm 8b0477e89af0 16"), "out: {out}");
    assert!(out.contains("gdec secret"), "out: {out}");
    assert!(out.contains("chacha e66dea2709 16"), "out: {out}");
    assert!(out.contains("chdec hello"), "out: {out}");
    assert!(out.contains("ctr 60d4f4ceae18fbef892ccaa49d8b32a6"), "out: {out}");
    assert!(out.contains("list true"), "out: {out}");
    assert!(out.contains("info true"), "out: {out}");
    assert!(out.contains("nounk true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9e_crypto_cipher_errors() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { createCipheriv, createDecipheriv } from "node:crypto";
const key = Buffer.alloc(32, 1), iv16 = Buffer.alloc(16, 2), iv12 = Buffer.alloc(12, 3);
try { createCipheriv("aes-999-cbc", key, iv16); } catch (e) { console.log("alg", e.code === "ERR_CRYPTO_UNKNOWN_CIPHER"); }
try { createCipheriv("aes-256-cbc", Buffer.alloc(5), iv16); } catch (e) { console.log("key", e.code === "ERR_CRYPTO_INVALID_KEYLEN"); }
try { createCipheriv("aes-256-cbc", key, Buffer.alloc(4)); } catch (e) { console.log("iv", e.code === "ERR_CRYPTO_INVALID_IV"); }
try { const d = createDecipheriv("aes-256-cbc", key, iv16); d.update(Buffer.from("00112233", "hex")); d.final(); } catch (e) { console.log("pad", e.code === "ERR_OSSL_WRONG_FINAL_BLOCK_LENGTH"); }
try { const x = createCipheriv("aes-256-cbc", key, iv16); x.final(); x.final("hex"); } catch (e) { console.log("fin2", e.code === "ERR_CRYPTO_INVALID_STATE"); }
try { const x = createCipheriv("aes-256-cbc", key, iv16); x.final(); x.update("x", "utf8", "hex"); } catch (e) { console.log("updfin", e.code === undefined); }
try {
  const x = createCipheriv("aes-256-gcm", key, iv12);
  const ct = Buffer.concat([x.update("s"), x.final()]);
  const tag = x.getAuthTag(); tag[0] ^= 1;
  const dd = createDecipheriv("aes-256-gcm", key, iv12);
  dd.setAuthTag(tag); dd.update(ct); dd.final("utf8");
} catch (e) { console.log("tag", e.code === undefined && /authenticate/.test(e.message)); }
try {
  const dd = createDecipheriv("aes-256-gcm", key, iv12);
  dd.setAuthTag(Buffer.alloc(16)); dd.update(Buffer.from("00", "hex")); dd.final("utf8");
} catch (e) { console.log("noaad", e.code === undefined); }
"#,
    );
    assert!(out.contains("alg true"), "out: {out}");
    assert!(out.contains("key true"), "out: {out}");
    assert!(out.contains("iv true"), "out: {out}");
    assert!(out.contains("pad true"), "out: {out}");
    assert!(out.contains("fin2 true"), "out: {out}");
    assert!(out.contains("updfin true"), "out: {out}");
    assert!(out.contains("tag true"), "out: {out}");
    assert!(out.contains("noaad true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9e_crypto_keys_sign() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { generateKeyPairSync, createSign, createVerify, sign, verify, createPrivateKey, createPublicKey, constants } from "node:crypto";
const { publicKey, privateKey } = generateKeyPairSync("rsa", { modulusLength: 2048 });
console.log("rsa", privateKey.type === "private" && privateKey.asymmetricKeyType === "rsa" && publicKey.type === "public");
const sig = sign("sha256", Buffer.from("msg"), privateKey);
console.log("sign", verify("sha256", Buffer.from("msg"), publicKey, sig) === true);
console.log("neg", verify("sha256", Buffer.from("msg!"), publicKey, sig) === false);
const s = createSign("RSA-SHA256"); s.update("he"); s.update("llo");
const v = createVerify("RSA-SHA256"); v.update("hello");
console.log("sv", v.verify(publicKey, s.sign(privateKey)) === true);
const s1 = sign("RSA-SHA1", Buffer.from("m"), privateKey);
console.log("sha1", verify("RSA-SHA1", Buffer.from("m"), publicKey, s1) === true);
const pem = privateKey.export({ format: "pem", type: "pkcs8" });
const back = createPrivateKey(pem);
console.log("pem", back.type === "private" && back.asymmetricKeyType === "rsa");
console.log("pubfrompriv", createPublicKey(privateKey).type === "public");
const jwk = publicKey.export({ format: "jwk" });
console.log("jwk", jwk.kty === "RSA" && jwk.e === "AQAB");
const { publicKey: ep, privateKey: es } = generateKeyPairSync("ec", { namedCurve: "prime256v1" });
const esig = sign("sha256", Buffer.from("m"), es);
console.log("ec", verify("sha256", Buffer.from("m"), ep, esig) === true);
const { publicKey: dp, privateKey: ds } = generateKeyPairSync("ed25519");
const dsg = sign(null, Buffer.from("m"), ds);
console.log("ed", verify(null, Buffer.from("m"), dp, dsg) === true);
console.log("const", constants.RSA_PKCS1_PADDING === 1 && constants.RSA_PKCS1_OAEP_PADDING === 4 && constants.RSA_PSS_SALTLEN_DIGEST === -1);
"#,
    );
    assert!(out.contains("rsa true"), "out: {out}");
    assert!(out.contains("sign true"), "out: {out}");
    assert!(out.contains("neg true"), "out: {out}");
    assert!(out.contains("sv true"), "out: {out}");
    assert!(out.contains("sha1 true"), "out: {out}");
    assert!(out.contains("pem true"), "out: {out}");
    assert!(out.contains("pubfrompriv true"), "out: {out}");
    assert!(out.contains("jwk true"), "out: {out}");
    assert!(out.contains("ec true"), "out: {out}");
    assert!(out.contains("ed true"), "out: {out}");
    assert!(out.contains("const true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9e_crypto_enc_dh_ecdh() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { generateKeyPairSync, publicEncrypt, privateDecrypt, createECDH, createDiffieHellmanGroup, diffieHellman } from "node:crypto";
const { publicKey, privateKey } = generateKeyPairSync("rsa", { modulusLength: 2048 });
const enc = publicEncrypt(publicKey, Buffer.from("hi"));
console.log("oaep", privateDecrypt(privateKey, enc).toString() === "hi");
const enc1 = publicEncrypt({ key: publicKey, padding: 1 }, Buffer.from("v15"));
console.log("v15", privateDecrypt({ key: privateKey, padding: 1 }, enc1).toString() === "v15");
const a = createECDH("prime256v1"); a.generateKeys();
const b = createECDH("prime256v1"); b.generateKeys();
console.log("ecdh", a.computeSecret(b.getPublicKey()).equals(b.computeSecret(a.getPublicKey())));
console.log("ecdhraw", a.getPublicKey()[0] === 4 && a.getPrivateKey().length === 32);
const x = createDiffieHellmanGroup("modp14"); x.generateKeys();
const y = createDiffieHellmanGroup("modp14"); y.generateKeys();
const sx = x.computeSecret(y.getPublicKey());
console.log("dh", sx.equals(y.computeSecret(x.getPublicKey())) && sx.length === 256);
console.log("dhprime", x.getPrime().length === 256 && x.verifyError() === 0);
"#,
    );
    assert!(out.contains("oaep true"), "out: {out}");
    assert!(out.contains("v15 true"), "out: {out}");
    assert!(out.contains("ecdh true"), "out: {out}");
    assert!(out.contains("ecdhraw true"), "out: {out}");
    assert!(out.contains("dh true"), "out: {out}");
    assert!(out.contains("dhprime true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9e_crypto_asym_errors() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { generateKeyPairSync, checkPrimeSync, generatePrimeSync, createECDH, createDiffieHellman, createDiffieHellmanGroup, sign } from "node:crypto";
console.log("prime", checkPrimeSync(13n) === true && checkPrimeSync(15n) === false);
console.log("primebuf", checkPrimeSync(Buffer.from([13])) === true);
try { generateKeyPairSync("dsa", {}); console.log("dsa", true); } catch (e) { console.log("dsa", false); }
try { createECDH("secp256k1"); console.log("k1", true); } catch (e) { console.log("k1", false); }
try { createDiffieHellmanGroup("modp1"); } catch (e) { console.log("modp1", e.code === "ERR_NOT_SUPPORTED"); }
try { createDiffieHellmanGroup("modp99"); } catch (e) { console.log("modp99", e.code === "ERR_NOT_SUPPORTED"); }
try { createDiffieHellman(2048); } catch (e) { console.log("dhsize", e.code === "ERR_NOT_SUPPORTED"); }
const p = generatePrimeSync(64, { checks: 3 });
console.log("gen", p.length === 8 && checkPrimeSync(p, { checks: 3 }) === true);
try { console.log("bigint", typeof generatePrimeSync(64, { bigint: true }) === "bigint"); } catch (e) { console.log("bigint", false); }
try { sign("nope", Buffer.from("m"), generateKeyPairSync("ed25519").privateKey); } catch (e) { console.log("edalg", e.code === "ERR_CRYPTO_INVALID_DIGEST"); }
"#,
    );
    assert!(out.contains("prime true"), "out: {out}");
    assert!(out.contains("primebuf true"), "out: {out}");
    assert!(out.contains("dsa true"), "out: {out}");
    assert!(out.contains("k1 true"), "out: {out}");
    assert!(out.contains("modp1 true"), "out: {out}");
    assert!(out.contains("modp99 true"), "out: {out}");
    assert!(out.contains("dhsize true"), "out: {out}");
    assert!(out.contains("gen true"), "out: {out}");
    assert!(out.contains("bigint true"), "out: {out}");
    assert!(out.contains("edalg true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9e_crypto_kdf() {
    // 真 Node 取证向量（pbkdf2/scrypt/hkdf/argon2，逐字节对）
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { pbkdf2Sync, pbkdf2, scryptSync, scrypt, hkdfSync, hkdf, argon2Sync, argon2 } from "node:crypto";
console.log("pbkdf2", pbkdf2Sync("password", "salt", 1, 32, "sha256").toString("hex") === "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b");
console.log("pbkdf2b", pbkdf2Sync("password", "salt", 2, 20, "sha1").toString("hex") === "ea6c014dc72d6f8ccd1ed92ace1d41f0d8de8957");
pbkdf2("password", "salt", 1, 32, "sha256", (e, dk) => {
  console.log("pbkdf2a", e === null && dk.toString("hex") === "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b");
  console.log("done");
});
console.log("scrypt", scryptSync("password", "salt", 64, { N: 1024, r: 8, p: 1 }).toString("hex").slice(0, 64) === "16dbc8906763c7f048977a68f9d305f7710e068ca2cd95dab372125bb3f19608");
scrypt("password", "salt", 32, { N: 1024 }, (e, dk) => {
  console.log("scrypta", e === null && dk.length === 32);
});
console.log("hkdf", Buffer.from(hkdfSync("sha256", "ikm", "salt", "info", 42)).toString("hex") === "fe8f9615d2374c0d17f77d1aeaf408c2e75fe0466073d0def23c733e2f862dfd6814c9254418fa112fe8");
hkdf("sha256", "ikm", "salt", "info", 42, (e, okm) => {
  console.log("hkdfa", e === null && okm instanceof ArrayBuffer && okm.byteLength === 42);
});
console.log("argon2", argon2Sync("argon2id", { message: "password", nonce: "somesalt", parallelism: 4, tagLength: 32, memory: 32, passes: 1 }).toString("hex") === "299d5e50f0022a4eef2d510ade9b1743bd1f568feefc042c3dff926a271e7fb2");
argon2("argon2id", { message: "password", nonce: "somesalt", parallelism: 4, tagLength: 16, memory: 32, passes: 1 }, (e, tag) => {
  console.log("argon2a", e === null && tag.length === 16);
});
try { pbkdf2Sync("p", "s", 0, 32, "sha256"); } catch (e) { console.log("it0", e.code === "ERR_OUT_OF_RANGE"); }
try { scryptSync("p", "s", 32, { N: 1048576, r: 8, p: 1 }); } catch (e) { console.log("mem", e.code === "ERR_CRYPTO_INVALID_SCRYPT_PARAMS"); }
console.log("ad", argon2Sync("argon2id", { message: "secret", nonce: "somesalt12345678", parallelism: 1, tagLength: 32, memory: 8, passes: 1, associatedData: Buffer.from("ad-data") }).toString("hex") === "81454faa04011e9d56a85f66352875d91e04fb8edf2458d44c18c4d9bcef4762");
"#,
    );
    assert!(out.contains("pbkdf2 true"), "out: {out}");
    assert!(out.contains("pbkdf2b true"), "out: {out}");
    assert!(out.contains("pbkdf2a true"), "out: {out}");
    assert!(out.contains("done"), "out: {out}");
    assert!(out.contains("scrypt true"), "out: {out}");
    assert!(out.contains("scrypta true"), "out: {out}");
    assert!(out.contains("hkdf true"), "out: {out}");
    assert!(out.contains("hkdfa true"), "out: {out}");
    assert!(out.contains("argon2 true"), "out: {out}");
    assert!(out.contains("argon2a true"), "out: {out}");
    assert!(out.contains("it0 true"), "out: {out}");
    assert!(out.contains("mem true"), "out: {out}");
    assert!(out.contains("ad true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9e_crypto_x509() {
    let dir = assert_fs::TempDir::new().unwrap();
    let pem = "-----BEGIN CERTIFICATE-----\nMIIDizCCAnOgAwIBAgIUXHVjPqV6YzyPBqRQYPc0ZcZRzkswDQYJKoZIhvcNAQEL\nBQAwNjELMAkGA1UEBhMCVVMxDTALBgNVBAoMBEFjbWUxGDAWBgNVBAMMD3d3dy5l\neGFtcGxlLmNvbTAeFw0yNjA5MTIwNzAzMTZaFw0yNjA5MTQwNzAzMTZaMDYxCzAJ\nBgNVBAYTAlVTMQ0wCwYDVQQKDARBY21lMRgwFgYDVQQDDA93d3cuZXhhbXBsZS5j\nb20wggEiMA0GCSqGSIb3DQEBAQUAA4IBDwAwggEKAoIBAQChD22N6LlqRJlVEyGj\nE+zohSE50NYazdABnbAcECBTT9d0NAsLPfASUbVWzDoyDDiMGGbApwORUiACMwZq\nNI7KQ1OeEe6wDkVUWXb+07bNV2pUZzDZXnZJzgkZMjy7kNT6uu+36n4KdApSt9jO\nwG89qdYQ/5wIMo8LCA0vV1Px2jiYvgXQTACy4BXa6QbzZR2iUFlGA4wYfzv7dEk1\nY5Bz4vwo/5PfxdrtUkirg/kdxJqqBypV+ptW8YZRPZLwTh05dAOHx2Mh/vx1QKi+\nnvj7PQUihHogt64+i0Q6hqQNX2U/FI05dvUonRAnHl+o0YJCVhOOBNPqB5ZBNXEc\n9kAVAgMBAAGjgZAwgY0wHQYDVR0OBBYEFFl6516N9nn1EPzjIobzQcYtEV0wMB8G\nA1UdIwQYMBaAFFl6516N9nn1EPzjIobzQcYtEV0wMA8GA1UdEwEB/wQFMAMBAf8w\nLQYDVR0RBCYwJIIPd3d3LmV4YW1wbGUuY29tggtleGFtcGxlLmNvbYcEfwAAATAL\nBgNVHQ8EBAMCBaAwDQYJKoZIhvcNAQELBQADggEBAAd7FdDiGjuGBBtw5GTn+zD6\n+qTq2YoJIZzkKJ/TaPpPk67jyEVpKghI+aJ6o7ZBDiAytOGPCZsEmX7j+26oj1c6\nsukEQn3jF9h9eKw+ih/FUsFUsU7JGuywO7lbk9GbHxKtfF1na0tYDSpQnN9WldXz\n5/btna3Nzj+53wdkO0BkkXefVZfFu0dIH7o6hvxhW40RLfhkwW0DWSJ9vgHYta0d\nfDlfTxiy6M+f1YxM49MDmzL37FopkuFj0xmbRXUdIjHTKq+rIZYuMW9x510uVD4y\ndh6vOBNEHn8gVd1JIJLjrBvY55ecfA/UieRe8TCJ380CqZ9bYHJoIm1JZiaGSdg=\n-----END CERTIFICATE-----\n";
    dir.child("c.pem").write_str(pem).unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { X509Certificate, Certificate } from "node:crypto";
import fs from "node:fs";
const pem = fs.readFileSync("c.pem", "utf8");
const x = new X509Certificate(pem);
console.log("subj", x.subject === "C=US\nO=Acme\nCN=www.example.com");
console.log("san", x.subjectAltName === "DNS:www.example.com, DNS:example.com, IP Address:127.0.0.1");
console.log("host", x.checkHost("www.example.com") === "www.example.com", x.checkHost("other.com") === undefined, x.checkHost("127.0.0.1") === "127.0.0.1");
console.log("sn", x.serialNumber.length > 4, x.validFrom.endsWith("GMT"), x.validTo.endsWith("GMT"));
console.log("fp", x.fingerprint.split(":").length === 20, x.fingerprint256.split(":").length === 32, x.fingerprint512.split(":").length === 64);
console.log("pem", x.toString().startsWith("-----BEGIN CERTIFICATE-----"), x.raw.length > 100);
console.log("legacy", x.toLegacyObject().subject.CN === "www.example.com");
console.log("ku", JSON.stringify(x.keyUsage) === JSON.stringify(["Digital Signature", "Key Encipherment"]));
try { new X509Certificate("nope"); } catch (e) { console.log("bad", e.code === "ERR_INVALID_ARG_VALUE"); }
try { x.verify(); } catch (e) { console.log("verify", e.code === "ERR_INVALID_ARG_TYPE"); }
console.log("verifyself", x.verify(x.publicKey) === true);
try { new Certificate(); } catch (e) { console.log("legacy-cert", e.code === "ERR_NOT_SUPPORTED"); }
"#,
    );
    assert!(out.contains("subj true"), "out: {out}");
    assert!(out.contains("san true"), "out: {out}");
    assert!(out.contains("host true true true"), "out: {out}");
    assert!(out.contains("sn true true true"), "out: {out}");
    assert!(out.contains("fp true true true"), "out: {out}");
    assert!(out.contains("pem true true"), "out: {out}");
    assert!(out.contains("legacy true"), "out: {out}");
    assert!(out.contains("ku true"), "out: {out}");
    assert!(out.contains("bad true"), "out: {out}");
    assert!(out.contains("verify true"), "out: {out}");
    assert!(out.contains("verifyself true"), "out: {out}");
    assert!(out.contains("legacy-cert true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9h_crypto_k256() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { createECDH, generateKeyPairSync, createSign, createVerify, createPrivateKey, createPublicKey, getCurves } from "node:crypto";
console.log("k-curves", getCurves().includes("secp256k1"));
// 真机固定向量（node v26.8.2 实测）：priv/pub/peer/secret 逐字节对
const FIX = {
  priv: "ab5ece87dd1089783678deadcac0283eff35dddd6a32081dce7f2c1d3630de74",
  pub: "04a72a7632bbef9c8b9a9a58224afba9ce6ba199b5d0d8dddf906e6de486ae34aa43be1ee6eab356aab327347da80fac8c09a38183a1ece15d37d570184912fd19",
  peer: "0421d3b023a66019230f034f7fb38b575a613d1b4465d530ab96829b7d047687e34680bb8cbf806cfdcaf8216881aaa2a4f3f0e8f48de7a49f7d858a497ed1ab2d",
  secret: "c9fe11e3f27bac5fb3692fac8787c0a07566ba02e47a32c45136234d1b080364",
};
const a = createECDH("secp256k1");
a.setPrivateKey(Buffer.from(FIX.priv, "hex"));
console.log("k-ecdh-vec", a.computeSecret(Buffer.from(FIX.peer, "hex")).toString("hex") === FIX.secret);
const e1 = createECDH("secp256k1"); e1.generateKeys();
const e2 = createECDH("secp256k1"); e2.generateKeys();
console.log("k-ecdh-self", e1.computeSecret(e2.getPublicKey()).equals(e2.computeSecret(e1.getPublicKey())));
// 签名往返 + 内容错验不过
const { privateKey, publicKey } = generateKeyPairSync("ec", { namedCurve: "secp256k1" });
const data = Buffer.from("hello-k256");
const sig = createSign("sha256").update(data).sign(privateKey);
console.log("k-sign", createVerify("sha256").update(data).verify(publicKey, sig) === true);
console.log("k-tamper", createVerify("sha256").update(Buffer.from("hello-k257")).verify(publicKey, sig) === false);
// ieeep1363 形态往返
const raw = createSign("sha256").update(data).sign({ key: privateKey, dsaEncoding: "ieee-p1363" });
console.log("k-rawlen", raw.length === 64);
console.log("k-rawvec", createVerify("sha256").update(data).verify({ key: publicKey, dsaEncoding: "ieee-p1363" }, raw) === true);
// 导出导入往返（der/pem/jwk/sec1）
const spki = publicKey.export({ format: "der", type: "spki" });
const pkcs8 = privateKey.export({ format: "der", type: "pkcs8" });
const pub2 = createPublicKey({ key: spki, format: "der", type: "spki" });
console.log("k-spki", createVerify("sha256").update(data).verify(pub2, sig) === true);
const priv2 = createPrivateKey({ key: pkcs8, format: "der", type: "pkcs8" });
console.log("k-pkcs8", createSign("sha256").update(data).sign(priv2).length > 64);
const jwk = publicKey.export({ format: "jwk" });
console.log("k-jwk", jwk.kty === "EC" && jwk.crv === "secp256k1" && typeof jwk.x === "string");
const pem = publicKey.export({ format: "pem", type: "spki" });
console.log("k-pem", pem.startsWith("-----BEGIN PUBLIC KEY-----"));
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("k-curves true"), "out: {out}");
    assert!(out.contains("k-ecdh-vec true"), "out: {out}");
    assert!(out.contains("k-ecdh-self true"), "out: {out}");
    assert!(out.contains("k-sign true"), "out: {out}");
    assert!(out.contains("k-tamper true"), "out: {out}");
    assert!(out.contains("k-rawlen true"), "out: {out}");
    assert!(out.contains("k-rawvec true"), "out: {out}");
    assert!(out.contains("k-spki true"), "out: {out}");
    assert!(out.contains("k-pkcs8 true"), "out: {out}");
    assert!(out.contains("k-jwk true"), "out: {out}");
    assert!(out.contains("k-pem true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9h_crypto_dsa_prime() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { generateKeyPairSync, generateKeyPair, createSign, createVerify, createPrivateKey, createPublicKey, generatePrimeSync, checkPrimeSync } from "node:crypto";
// DSA 快档（1024/160，Sign/Verify 全链；慢档只验形状不断言向量）
const { privateKey, publicKey } = generateKeyPairSync("dsa", { modulusLength: 1024, divisorLength: 160 });
console.log("d-gen", privateKey.type === "private", publicKey.type === "public", privateKey.asymmetricKeyType === "dsa");
const data = Buffer.from("hello-dsa");
const sig = createSign("sha256").update(data).sign(privateKey);
console.log("d-sign", createVerify("sha256").update(data).verify(publicKey, sig) === true);
console.log("d-tamper", createVerify("sha256").update(Buffer.from("hello-dsb")).verify(publicKey, sig) === false);
// sha384 档（prehash 全哈希）
const sig384 = createSign("sha384").update(data).sign(privateKey);
console.log("d-384", createVerify("sha384").update(data).verify(publicKey, sig384) === true);
// 导出导入往返（der/pem/jwk）
const spki = publicKey.export({ format: "der", type: "spki" });
const pkcs8 = privateKey.export({ format: "der", type: "pkcs8" });
console.log("d-der", spki.length > 100, pkcs8.length > 100);
const pub2 = createPublicKey({ key: spki, format: "der", type: "spki" });
console.log("d-spki", createVerify("sha256").update(data).verify(pub2, sig) === true);
const priv2 = createPrivateKey({ key: pkcs8, format: "der", type: "pkcs8" });
console.log("d-pkcs8", createSign("sha256").update(data).sign(priv2).length > 40);
console.log("d-pem", publicKey.export({ format: "pem", type: "spki" }).startsWith("-----BEGIN PUBLIC KEY-----"));
const jwk = publicKey.export({ format: "jwk" });
console.log("d-jwk", jwk.kty === "DSA" && typeof jwk.p === "string" && typeof jwk.y === "string" && jwk.x === undefined);
const pub3 = createPublicKey({ key: jwk, format: "jwk" });
console.log("d-jwkim", createVerify("sha256").update(data).verify(pub3, sig) === true);
// 异步形态
generateKeyPair("dsa", { modulusLength: 1024, divisorLength: 160 }, (e, pub, priv) => {
  console.log("d-async", e === null && pub.type === "public" && priv.type === "private");
});
// bigint 素数（16 进制桥）
const p = generatePrimeSync(256, { bigint: true });
console.log("d-bigint", typeof p === "bigint" && checkPrimeSync(p) === true);
const ps = generatePrimeSync(256, { bigint: true, safe: true });
console.log("d-safe", typeof ps === "bigint" && checkPrimeSync(ps) === true && checkPrimeSync((ps - 1n) / 2n) === true);
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("d-gen true true true"), "out: {out}");
    assert!(out.contains("d-sign true"), "out: {out}");
    assert!(out.contains("d-tamper true"), "out: {out}");
    assert!(out.contains("d-384 true"), "out: {out}");
    assert!(out.contains("d-der true true"), "out: {out}");
    assert!(out.contains("d-spki true"), "out: {out}");
    assert!(out.contains("d-pkcs8 true"), "out: {out}");
    assert!(out.contains("d-pem true"), "out: {out}");
    assert!(out.contains("d-jwk true"), "out: {out}");
    assert!(out.contains("d-jwkim true"), "out: {out}");
    assert!(out.contains("d-async true"), "out: {out}");
    assert!(out.contains("d-bigint true"), "out: {out}");
    assert!(out.contains("d-safe true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9h_crypto_xof_ripemd() {
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("p.mjs");
    file.write_str(
        r#"
import { createHash, createHmac, getHashes } from "node:crypto";
console.log("x-ripemd", createHash("ripemd160").update("abc").digest("hex") === "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc");
console.log("x-s128", createHash("shake128", { outputLength: 32 }).update("abc").digest("hex") === "5881092dd818bf5cf8a3ddb793fbcba74097d5c526a6d35f97b83351940f2cc8");
console.log("x-s256", createHash("shake256", { outputLength: 32 }).update("abc").digest("hex") === "483366601360a8771c6863080cc4114d8db44530f8f1e1ee4f94ea37e78b5739");
console.log("x-hmacri", createHmac("ripemd160", "key").update("msg").digest("hex") === "af9f1041c7727ee3161fdbda8821364fb888a0e2");
try { createHmac("shake256", "key"); console.log("x-hmacshake-never", false); }
catch (e) { console.log("x-hmacshake", e.code === undefined); }
const h = createHash("shake256", { outputLength: 16 });
h.update("a");
const c2 = h.copy();
h.update("bc"); c2.update("bc");
console.log("x-copy", h.digest("hex") === c2.digest("hex") && h.digest === c2.digest);
try { createHash("shake256", { outputLength: -1 }); console.log("x-badlen-never", false); }
catch (e) { console.log("x-badlen", e.code === "ERR_INVALID_ARG_VALUE"); }
console.log("x-hashes", getHashes().includes("ripemd160") && getHashes().includes("shake128") && getHashes().includes("shake256"));
console.log("x-dflt", createHash("shake256").update("abc").digest("hex").length === 64);
console.log("x-dflt128", createHash("shake128").update("abc").digest("hex").length === 32);
"#,
    ).unwrap();
    let out = winterjs().arg("--run").arg(file.path()).current_dir(dir.path()).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    for tag in ["x-ripemd", "x-s128", "x-s256", "x-hmacri", "x-hmacshake", "x-copy", "x-badlen", "x-hashes", "x-dflt", "x-dflt128"] {
        assert!(stdout.contains(&format!("{tag} true")), "out: {stdout}");
    }
    // DEP0198 缺省警告走 stderr（真机同款）。
    assert!(String::from_utf8_lossy(&out.stderr).contains("DEP0198"), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    dir.close().unwrap();
}

#[test]
fn phase9i_x509_verify() {
    let dir = assert_fs::TempDir::new().unwrap();
    let key = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    dir.child("c.pem").write_str(&key.cert.pem()).unwrap();
    // openssl 烤入固件：sha256WithRSAEncryption（CA:TRUE）与 Ed25519（SKI/AKID 齐）。
    let out = {
        let file = dir.child("x.mjs");
        file.write_str(
            r#"
import { X509Certificate, createPublicKey, generateKeyPairSync } from "node:crypto";
import fs from "node:fs";
const x = new X509Certificate(fs.readFileSync("c.pem", "utf8"));
console.log("xv-self", x.verify(x.publicKey));
console.log("xv-ca", x.ca === false, typeof x.publicKey === "object");
console.log("xv-pemrt", new X509Certificate(x.toString()).verify(x.publicKey));
const pk2 = createPublicKey(x.publicKey.export({ type: "spki", format: "pem" }));
console.log("xv-pubrt", x.verify(pk2));
const RSA_PEM = `-----BEGIN CERTIFICATE-----
MIIDBzCCAe+gAwIBAgIUKMqG5DU1vRAPDN73tipxwpNdnyYwDQYJKoZIhvcNAQEL
BQAwEzERMA8GA1UEAwwIcnNhcHJvYmUwHhcNMjYwOTEyMTUzNzEwWhcNMjYxMDEy
MTUzNzEwWjATMREwDwYDVQQDDAhyc2Fwcm9iZTCCASIwDQYJKoZIhvcNAQEBBQAD
ggEPADCCAQoCggEBAKNUnvi0BslHzHg4FsLJVRAGGnJLau1qpKYsUpl9o54Gi37o
mDISUL+m+2sHk4GfdHmQtZMv/2ehbsZlWeXF+KN7R8Y3gsjXjws772d2H2KSKeBs
rgXuW0aK7dZ298VJWiOkwn3Yxw3/VIrCnf22OvFD6hIEnJINsed7pWXcO6ENs0sn
ZjSoTrdUARO4bP4UUaRHRC7OvvmJOhk7YP27GxeZYlpGZAVc9bApeqNXFMORbw7h
56F5OaWRd3+hLS2Qzb1WG8hkHNP2p6IjrM3sJv9/MxDG7oqUyGfHUU+L7WwsKVSX
IvH1KVjSgCpQIe0xNK6hDVeEzCQ1K+1NerUXZb0CAwEAAaNTMFEwHQYDVR0OBBYE
FCYP4DCcqmSzcUh9l5twIQZ+vfTnMB8GA1UdIwQYMBaAFCYP4DCcqmSzcUh9l5tw
IQZ+vfTnMA8GA1UdEwEB/wQFMAMBAf8wDQYJKoZIhvcNAQELBQADggEBAELdfAXs
hThVFUilcN1IqvCgnpJQaWt9F9hrK/kFU4xYuufULn8/ON1XSEsq9zFdaieh3yUS
gfK5TzaqYWdmReZQmQwu3tWQP3i2N/+yhlKtyG8HVYGE8XnWzc1vqVCvo4Qgbo8A
vprQmAByt1d73hYRAVqy6352VyaODUzv88o+bhA4lEVoFT+ywq/NTKMBItzX2nrf
mbYHk5BuaD60LrgoHhagfZN0AtyjUsrSpZzhZibvkF/dd8JECjgMfdW2TzQt7oGu
XlWJbDY2dLREtjr9Gy9xL9VYEqVwBl0LnOn+6NVXu1SWpdvb7JJkiTl/E5T7jJHM
JYXWO6JD6QVhZA4=
-----END CERTIFICATE-----
`;
const ED_PEM = `-----BEGIN CERTIFICATE-----
MIIBODCB66ADAgECAhQKqo7DNn6Wqr9sY8nb/FFQHmFWrzAFBgMrZXAwEjEQMA4G
A1UEAwwHZWRwcm9iZTAeFw0yNjA5MTIxNTM3MTBaFw0yNjEwMTIxNTM3MTBaMBIx
EDAOBgNVBAMMB2VkcHJvYmUwKjAFBgMrZXADIQDZBIIBibh/Hk5+4U8s3/NQ1YLC
kRPopcUuaL2ubsrCyaNTMFEwHwYDVR0jBBgwFoAUx3a9XMqYMQnv2WV1xfhH8yx4
lbIwDwYDVR0TAQH/BAUwAwEB/zAdBgNVHQ4EFgQUx3a9XMqYMQnv2WV1xfhH8yx4
lbIwBQYDK2VwA0EADkN9ATKhMQdKm8vmdTP4+kV0BczvogHkDyXLYf+If4nw4CYs
BngogF7qMQ7NdKgX1SlKGef1y1Oqc6T0zFQAAg==
-----END CERTIFICATE-----
`;
const rsa = new X509Certificate(RSA_PEM);
console.log("xv-rsa", rsa.verify(rsa.publicKey), rsa.ca === true);
const ed = new X509Certificate(ED_PEM);
console.log("xv-ed", ed.verify(ed.publicKey));
const { publicKey: other } = generateKeyPairSync("ec", { namedCurve: "P-256" });
console.log("xv-wrong", x.verify(other));
console.log("xv-cross", rsa.verify(x.publicKey), ed.verify(rsa.publicKey));
const { publicKey: okp } = generateKeyPairSync("x25519");
console.log("xv-okp", x.verify(okp));
const der = Buffer.from(x.raw);
der[der.length - 1] ^= 0xff;
const tampered = new X509Certificate(der);
console.log("xv-tamper", tampered.verify(tampered.publicKey) === false);
const t = (n, f) => { try { f(); console.log(n, "NO-THROW"); } catch (e) { console.log(n, e.code); } };
t("xv-noarg", () => x.verify());
t("xv-strarg", () => x.verify("nope"));
t("xv-priv", () => x.verify(generateKeyPairSync("ec", { namedCurve: "P-256" }).privateKey));
"#,
        )
        .unwrap();
        winterjs()
            .arg("--run")
            .arg(file.path())
            .current_dir(dir.path())
            .output()
            .unwrap()
    };
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    for line in [
        "xv-self true",
        "xv-ca true true",
        "xv-pemrt true",
        "xv-pubrt true",
        "xv-rsa true true",
        "xv-ed true",
        "xv-wrong false",
        "xv-cross false false",
        "xv-okp false",
        "xv-tamper true",
        "xv-noarg ERR_INVALID_ARG_TYPE",
        "xv-strarg ERR_INVALID_ARG_TYPE",
        "xv-priv ERR_INVALID_ARG_VALUE",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase9i_mlkem() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "mk.mjs",
        r#"
import crypto from "node:crypto";
const { generateKeyPairSync, encapsulate, decapsulate, createPrivateKey, createPublicKey } = crypto;
for (const kind of ["ml-kem-512", "ml-kem-768", "ml-kem-1024"]) {
  const { publicKey, privateKey } = generateKeyPairSync(kind);
  const spki = publicKey.export({ format: "der", type: "spki" });
  const pkcs8 = privateKey.export({ format: "der", type: "pkcs8" });
  const r = encapsulate(publicKey);
  const sk2 = decapsulate(privateKey, r.ciphertext);
  // 尺寸与真机逐字节同构：SPKI 822/1206/1590，PKCS#8 恒 86（64B 种子形），ct 768/1088/1568，ss 恒 32。
  const sizes = `${spki.length} ${pkcs8.length} ${r.ciphertext.length} ${r.sharedKey.length}`;
  const expect = { "ml-kem-512": "822 86 768 32", "ml-kem-768": "1206 86 1088 32", "ml-kem-1024": "1590 86 1568 32" }[kind];
  console.log(`mk-${kind.split("-")[2]}-sizes`, sizes === expect);
  console.log(`mk-${kind.split("-")[2]}-roundtrip`, Buffer.compare(Buffer.from(r.sharedKey), Buffer.from(sk2)) === 0);
  const r2 = encapsulate(privateKey);
  console.log(`mk-${kind.split("-")[2]}-encap-priv`, r2.ciphertext.length === r.ciphertext.length, decapsulate(privateKey, r2.ciphertext).length === 32);
  const k2 = createPrivateKey({ key: pkcs8, format: "der", type: "pkcs8" });
  const p2 = createPublicKey({ key: spki, format: "der", type: "spki" });
  console.log(`mk-${kind.split("-")[2]}-import`, k2.asymmetricKeyType === kind, p2.asymmetricKeyType === kind,
    Buffer.compare(Buffer.from(decapsulate(k2, r.ciphertext)), Buffer.from(r.sharedKey)) === 0);
  const j = publicKey.export({ format: "jwk" });
  const jp = privateKey.export({ format: "jwk" });
  console.log(`mk-${kind.split("-")[2]}-jwk`, j.kty === "AKP", j.alg === "ML-KEM-" + kind.split("-")[2], jp.kty === "AKP", typeof jp.priv === "string", jp.priv.length === 86);
}
// 报错/边界（真机口径）
const { publicKey, privateKey } = generateKeyPairSync("ml-kem-768");
const r = encapsulate(publicKey);
const t = (n, f) => { try { f(); console.log(n, "NO-THROW"); } catch (e) { console.log(n, e.code ?? "no-code"); } };
t("mk-err-decap-pub", () => decapsulate(publicKey, r.ciphertext));
t("mk-err-decap-ec", () => decapsulate(generateKeyPairSync("ec", { namedCurve: "P-256" }).privateKey, r.ciphertext));
t("mk-err-decap-str", () => decapsulate("str", r.ciphertext));
t("mk-err-decap-short", () => decapsulate(privateKey, r.ciphertext.subarray(0, 100)));
t("mk-err-encap-str", () => encapsulate("nope"));
t("mk-err-encap-2arg", () => encapsulate(publicKey, {}));
// 等长坏文：FIPS 203 隐式拒绝（不抛，回 32B 伪随机且不等于原共享密钥）。
const bad = Buffer.from(r.ciphertext);
bad[10] ^= 0xff;
console.log("mk-err-implicit", decapsulate(privateKey, bad).length === 32,
  Buffer.compare(decapsulate(privateKey, bad), Buffer.from(r.sharedKey)) !== 0);
// PEM 导出（material 直通）与 X509 公钥链复用同一 try 表。
console.log("mk-pem", privateKey.export({ format: "pem" }).startsWith("-----BEGIN PRIVATE KEY-----"),
  publicKey.export({ format: "pem" }).startsWith("-----BEGIN PUBLIC KEY-----"));
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    for line in [
        "mk-512-sizes true",
        "mk-512-roundtrip true",
        "mk-512-encap-priv true true",
        "mk-512-import true true true",
        "mk-512-jwk true true true true true",
        "mk-768-sizes true",
        "mk-768-roundtrip true",
        "mk-768-encap-priv true true",
        "mk-768-import true true true",
        "mk-768-jwk true true true true true",
        "mk-1024-sizes true",
        "mk-1024-roundtrip true",
        "mk-1024-encap-priv true true",
        "mk-1024-import true true true",
        "mk-1024-jwk true true true true true",
        "mk-err-decap-pub ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE",
        "mk-err-decap-ec no-code",
        "mk-err-decap-str ERR_OSSL_UNSUPPORTED",
        "mk-err-decap-short ERR_CRYPTO_OPERATION_FAILED",
        "mk-err-encap-str ERR_OSSL_UNSUPPORTED",
        "mk-err-encap-2arg ERR_INVALID_ARG_TYPE",
        "mk-err-implicit true true",
        "mk-pem true true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase9i_mldsa() {
    let dir = assert_fs::TempDir::new().unwrap();
    // 真机固件：node 26.8.2 签发（PKCS#8 种子 + "from-node-fixture" 的 hedged 签名）
    // 与 openssl 3.6 ML-DSA-65 自签证书（X509 verify ml-dsa 臂）。
    let fixture = include_str!("../fixtures/mldsa-node-fixture.b64");
    let mut lines = fixture.lines();
    let node_pkcs8_b64 = lines.next().unwrap().trim();
    let node_sig_b64 = lines.next().unwrap().trim();
    let node_cert_pem = include_str!("../fixtures/mldsa-cert.pem");
    dir.child("node-cert.pem").write_str(node_cert_pem).unwrap();
    let out = {
        let file = dir.child("md.mjs");
        file.write_str(&format!(
            r#"
import crypto from "node:crypto";
import fs from "node:fs";
const {{ generateKeyPairSync, sign, verify, createPrivateKey, createPublicKey, X509Certificate }} = crypto;
for (const kind of ["ml-dsa-44", "ml-dsa-65", "ml-dsa-87"]) {{
  const {{ publicKey, privateKey }} = generateKeyPairSync(kind);
  const spki = publicKey.export({{ format: "der", type: "spki" }});
  const pkcs8 = privateKey.export({{ format: "der", type: "pkcs8" }});
  const sig = sign(null, Buffer.from("hello"), privateKey);
  const tag = kind.split("-")[2];
  // 尺寸与真机同构：SPKI 22+pk（1312/1952/2592）、PKCS#8 恒 54（32B 种子形）、sig 2420/3309/4627。
  const sizes = `${{spki.length}} ${{pkcs8.length}} ${{sig.length}}`;
  const expect = {{ "ml-dsa-44": "1334 54 2420", "ml-dsa-65": "1974 54 3309", "ml-dsa-87": "2614 54 4627" }}[kind];
  console.log(`md-${{tag}}-sizes`, sizes === expect);
  console.log(`md-${{tag}}-roundtrip`, verify(null, Buffer.from("hello"), publicKey, sig) === true);
  const pub2 = createPublicKey(privateKey);
  const k2 = createPrivateKey({{ key: pkcs8, format: "der", type: "pkcs8" }});
  const p2 = createPublicKey({{ key: spki, format: "der", type: "spki" }});
  console.log(`md-${{tag}}-import`, pub2.asymmetricKeyType === kind, k2.asymmetricKeyType === kind, p2.asymmetricKeyType === kind,
    verify(null, Buffer.from("hello"), pub2, sig));
  const j = publicKey.export({{ format: "jwk" }});
  const jp = privateKey.export({{ format: "jwk" }});
  console.log(`md-${{tag}}-jwk`, j.kty === "AKP", j.alg === "ML-DSA-" + tag, jp.priv.length === 43);
  try {{ sign("sha256", Buffer.from("x"), privateKey); console.log(`md-${{tag}}-hash`, "NO-THROW"); }}
  catch (e) {{ console.log(`md-${{tag}}-hash`, e.code === "ERR_OSSL_INVALID_DIGEST"); }}
  const bad = Buffer.from(sig);
  bad[100] ^= 0xff;
  console.log(`md-${{tag}}-tamper`, verify(null, Buffer.from("hello"), publicKey, bad) === false);
}}
// 真机交叉：node 26.8.2 的 hedged 签名本仓可验（PKCS#8 种子形逐字节互通）。
const nodePriv = createPrivateKey({{ key: Buffer.from("{node_pkcs8_b64}", "base64"), format: "der", type: "pkcs8" }});
const nodePub = createPublicKey(nodePriv);
const nodeSig = Buffer.from("{node_sig_b64}", "base64");
console.log("md-node-cross", nodePub.asymmetricKeyType === "ml-dsa-65",
  verify(null, Buffer.from("from-node-fixture"), nodePub, nodeSig) === true);
// openssl ML-DSA-65 自签证书走 X509 verify（签名 OID 与密钥 OID 同族）。
const cert = new X509Certificate(fs.readFileSync("node-cert.pem", "utf8"));
console.log("md-cert", cert.verify(cert.publicKey) === true, cert.ca === true, cert.publicKey.asymmetricKeyType === "ml-dsa-65");
"#
        ))
        .unwrap();
        winterjs()
            .arg("--run")
            .arg(file.path())
            .current_dir(dir.path())
            .output()
            .unwrap()
    };
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    for line in [
        "md-44-sizes true",
        "md-44-roundtrip true",
        "md-44-import true true true true",
        "md-44-jwk true true true",
        "md-44-hash true",
        "md-44-tamper true",
        "md-65-sizes true",
        "md-65-roundtrip true",
        "md-65-import true true true true",
        "md-65-jwk true true true",
        "md-65-hash true",
        "md-65-tamper true",
        "md-87-sizes true",
        "md-87-roundtrip true",
        "md-87-import true true true true",
        "md-87-jwk true true true",
        "md-87-hash true",
        "md-87-tamper true",
        "md-node-cross true true",
        "md-cert true true true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase9i_x509_issued_privkey() {
    let dir = assert_fs::TempDir::new().unwrap();
    // openssl 3.6 链固件：CA（SKI/AKID/keyCertSign 齐）+ leaf + 同名不同钥 CA。
    dir.child("chain-leaf.pem").write_str(include_str!("../fixtures/chain-leaf.pem")).unwrap();
    dir.child("chain-ca.pem").write_str(include_str!("../fixtures/chain-ca.pem")).unwrap();
    dir.child("unrelated-ca.pem").write_str(include_str!("../fixtures/unrelated-ca.pem")).unwrap();
    dir.child("chain-leaf.key").write_str(include_str!("../fixtures/chain-leaf.key")).unwrap();
    dir.child("mldsa-cert.pem").write_str(include_str!("../fixtures/mldsa-cert.pem")).unwrap();
    let out = {
        let file = dir.child("i.mjs");
        file.write_str(
            r#"
import crypto from "node:crypto";
import fs from "node:fs";
const X = (p) => new crypto.X509Certificate(fs.readFileSync(p, "utf8"));
const leaf = X("chain-leaf.pem");
const ca = X("chain-ca.pem");
const unrelated = X("unrelated-ca.pem"); // 同 subject 名（CN=Test CA），AKID/SKID 对不上
console.log("xi-issued", leaf.checkIssued(ca) === true, leaf.checkIssued(unrelated) === false, leaf.checkIssued(leaf) === false);
const pk = crypto.createPrivateKey(fs.readFileSync("chain-leaf.key", "utf8"));
const wrong = crypto.generateKeyPairSync("ec", { namedCurve: "P-256" }).privateKey;
console.log("xi-priv", leaf.checkPrivateKey(pk) === true, leaf.checkPrivateKey(wrong) === false);
const t = (n, f) => { try { f(); console.log(n, "NO-THROW"); } catch (e) { console.log(n, e.code ?? "no-code"); } };
t("xi-issued-noarg", () => leaf.checkIssued());
t("xi-issued-str", () => leaf.checkIssued("x"));
t("xi-priv-noarg", () => leaf.checkPrivateKey());
t("xi-priv-pub", () => leaf.checkPrivateKey(goodPub()));
function goodPub() { return crypto.createPublicKey(crypto.generateKeyPairSync("ec", { namedCurve: "P-256" }).privateKey); }
// OKP 私钥匹配（material 裸 32B → 手工 SPKI 包装比较）
const edpair = crypto.generateKeyPairSync("ed25519");
const edSelf = (() => {
  const spki = Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), edpair.publicKey.export({ format: "der", type: "raw" })]);
  return spki;
})();
console.log("xi-okp-shape", edSelf.length === 44, edSelf[0] === 0x30);
// PQ 私钥在证书上不匹配即 false（derive 链走 PQ 分支）
const pqcert = new crypto.X509Certificate(fs.readFileSync("mldsa-cert.pem", "utf8"));
const pqpair = crypto.generateKeyPairSync("ml-dsa-65");
console.log("xi-pq-priv", pqcert.checkPrivateKey(pqpair.privateKey) === false, pqcert.checkIssued(pqcert) === true);
"#,
        )
        .unwrap();
        winterjs()
            .arg("--run")
            .arg(file.path())
            .current_dir(dir.path())
            .output()
            .unwrap()
    };
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    for line in [
        "xi-issued true true true",
        "xi-priv true true",
        "xi-issued-noarg ERR_INVALID_ARG_TYPE",
        "xi-issued-str ERR_INVALID_ARG_TYPE",
        "xi-priv-noarg ERR_INVALID_ARG_TYPE",
        "xi-priv-pub ERR_INVALID_ARG_VALUE",
        "xi-okp-shape true true",
        "xi-pq-priv true true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase9i_x509_pss() {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("pss.pem").write_str(include_str!("../fixtures/pss.pem")).unwrap();
    dir.child("chain-leaf.pem").write_str(include_str!("../fixtures/chain-leaf.pem")).unwrap();
    dir.child("chain-ca.pem").write_str(include_str!("../fixtures/chain-ca.pem")).unwrap();
    // pss.pem：openssl 3.6 rsassaPss（sha256 + mgf1-sha256）实签，真机 node 26.8.2 验过。
    let out = {
        let file = dir.child("ps.mjs");
        file.write_str(
            r#"
import crypto from "node:crypto";
import fs from "node:fs";
const pss = new crypto.X509Certificate(fs.readFileSync("pss.pem", "utf8"));
console.log("xp-self", pss.verify(pss.publicKey) === true, pss.ca === true, pss.publicKey.asymmetricKeyType === "rsa");
const leaf = new crypto.X509Certificate(fs.readFileSync("chain-leaf.pem", "utf8"));
const ca = new crypto.X509Certificate(fs.readFileSync("chain-ca.pem", "utf8"));
console.log("xp-ec-still", leaf.verify(ca.publicKey) === true);
const { publicKey: other } = crypto.generateKeyPairSync("rsa", { modulusLength: 2048 });
console.log("xp-wrong", pss.verify(other) === false);
console.log("xp-cross", pss.verify(ca.publicKey) === false, leaf.checkIssued(pss) === false);
"#,
        )
        .unwrap();
        winterjs()
            .arg("--run")
            .arg(file.path())
            .current_dir(dir.path())
            .output()
            .unwrap()
    };
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    for line in [
        "xp-self true true true",
        "xp-ec-still true",
        "xp-wrong true",
        "xp-cross true true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10e_crypto_ccm() {
    // 10e AES-CCM 三档：真 Node 交叉取证逐字节向量 + 全档往返 + 报错/边界三件
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { createCipheriv, createDecipheriv, getCiphers, getCipherInfo } from "node:crypto";
// 已知向量（与真机逐字节一致）
const k = Buffer.from("000102030405060708090a0b0c0d0e0f", "hex");
const iv = Buffer.from("101112131415161718191a1b", "hex");
const c0 = createCipheriv("aes-128-ccm", k, iv, { authTagLength: 12 });
const ct0 = Buffer.concat([c0.update("hello CCM", "utf8"), c0.final()]);
console.log("vec", ct0.toString("hex") === "4bd0d5cc2dd46ef147", c0.getAuthTag().toString("hex") === "e3e2af5555de4dea4caafca2");
// 三档 × AAD 往返
for (const [alg, kl] of [["aes-128-ccm", 16], ["aes-192-ccm", 24], ["aes-256-ccm", 32]]) {
  const key = Buffer.alloc(kl, 9), nonce = Buffer.alloc(12, 4);
  const c = createCipheriv(alg, key, nonce, { authTagLength: 8 });
  c.setAAD(Buffer.from("hd"), { plaintextLength: 5 });
  const ct = Buffer.concat([c.update("world", "utf8"), c.final()]);
  const d = createDecipheriv(alg, key, nonce, { authTagLength: 8 });
  d.setAAD(Buffer.from("hd"), { plaintextLength: 5 });
  d.setAuthTag(c.getAuthTag());
  console.log("rt-" + alg, Buffer.concat([d.update(ct), d.final()]).toString() === "world", c.getAuthTag().length);
}
// 报错三件
try { createCipheriv("aes-128-ccm", k, Buffer.alloc(6), { authTagLength: 8 }); } catch (e) { console.log("badiv", e.code); }
try { createCipheriv("aes-128-ccm", k, iv); } catch (e) { console.log("notaglen", e.code); }
try { createCipheriv("aes-128-ccm", k, iv, { authTagLength: 5 }); } catch (e) { console.log("badtaglen", e.code); }
try {
  const c = createCipheriv("aes-128-ccm", k, iv, { authTagLength: 8 });
  c.setAAD(Buffer.from("x"));
} catch (e) { console.log("aad-noopt", e.code); }
try {
  const c = createCipheriv("aes-128-ccm", k, iv, { authTagLength: 8 });
  const ct = Buffer.concat([c.update("hi", "utf8"), c.final()]);
  const d = createDecipheriv("aes-128-ccm", k, iv, { authTagLength: 8 });
  d.setAuthTag(Buffer.alloc(8, 1));
  d.update(ct); d.final();
} catch (e) { console.log("badtag", e.code === undefined, e.message === "Unsupported state or unable to authenticate data"); }
try {
  const c = createCipheriv("aes-128-ccm", k, iv, { authTagLength: 8 });
  const ct = Buffer.concat([c.update("hi", "utf8"), c.final()]);
  const d = createDecipheriv("aes-128-ccm", k, iv, { authTagLength: 8 });
  d.update(ct); d.final();
} catch (e) { console.log("notag", e.code === undefined); }
try {
  const d = createDecipheriv("aes-128-ccm", k, iv, { authTagLength: 12 });
  d.setAuthTag(Buffer.alloc(8));
} catch (e) { console.log("taglen-mismatch", e.code); }
// 边界：nonce 7/13、tag 4/16、空明文
for (const nl of [7, 13]) {
  for (const tl of [4, 16]) {
    const key = Buffer.alloc(16, 2), nonce = Buffer.alloc(nl, 3);
    const c = createCipheriv("aes-128-ccm", key, nonce, { authTagLength: tl });
    const ct = Buffer.concat([c.update("", "utf8"), c.final()]);
    const d = createDecipheriv("aes-128-ccm", key, nonce, { authTagLength: tl });
    d.setAuthTag(c.getAuthTag());
    console.log("edge", nl, tl, c.getAuthTag().length, d.final().length);
  }
}
console.log("list", getCiphers().includes("aes-128-ccm") && getCiphers().includes("aes-256-ccm"));
console.log("info", getCipherInfo("aes-128-ccm").nid === 896 && getCipherInfo("aes-256-ccm").keyLength === 32);
"#,
    );
    assert!(out.contains("vec true true"), "out: {out}");
    assert!(out.contains("rt-aes-128-ccm true 8"), "out: {out}");
    assert!(out.contains("rt-aes-192-ccm true 8"), "out: {out}");
    assert!(out.contains("rt-aes-256-ccm true 8"), "out: {out}");
    assert!(out.contains("badiv ERR_CRYPTO_INVALID_IV"), "out: {out}");
    assert!(out.contains("notaglen ERR_CRYPTO_INVALID_AUTH_TAG"), "out: {out}");
    assert!(out.contains("badtaglen ERR_CRYPTO_INVALID_AUTH_TAG"), "out: {out}");
    assert!(out.contains("aad-noopt ERR_MISSING_ARGS"), "out: {out}");
    assert!(out.contains("badtag true true"), "out: {out}");
    assert!(out.contains("notag true"), "out: {out}");
    assert!(out.contains("taglen-mismatch ERR_CRYPTO_INVALID_AUTH_TAG"), "out: {out}");
    assert!(out.contains("edge 7 4 4 0"), "out: {out}");
    assert!(out.contains("edge 7 16 16 0"), "out: {out}");
    assert!(out.contains("edge 13 4 4 0"), "out: {out}");
    assert!(out.contains("edge 13 16 16 0"), "out: {out}");
    assert!(out.contains("list true"), "out: {out}");
    assert!(out.contains("info true"), "out: {out}");
    dir.close().unwrap();
}
