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
console.log("curves", getCurves().includes("prime256v1") && !getCurves().includes("ed25519"));
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
// 10f crypto二轮翻转（真机口径）：modp1 已支持（768B 素数），旧拒绝系伪语义。
console.log("modp1", createDiffieHellmanGroup("modp1").getPrime("buffer").length === 96);
try { createDiffieHellmanGroup("modp99"); } catch (e) { console.log("modp99", e.code === "ERR_NOT_SUPPORTED"); }
// 10f crypto首轮翻转（真机口径）：数值位长形同步生成素数（旧 ERR_NOT_SUPPORTED 系伪语义）。
console.log("dhsize", createDiffieHellman(512).getPrime("buffer").length === 64);
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
// 10f 四轮翻转（真机口径）：DSA 无 JWK 面（RFC 7518 无 DSA kty）——导出
// JWK_UNSUPPORTED_KEY_TYPE、导入 INVALID_JWK（§4.65 宽松 API 严格化翻旧断言）。
try { publicKey.export({ format: "jwk" }); console.log("d-jwk", "NO-THROW"); }
catch (e) { console.log("d-jwk", e.code === "ERR_CRYPTO_JWK_UNSUPPORTED_KEY_TYPE"); }
try { createPublicKey({ key: { kty: "DSA", p: "AA", q: "AA", g: "AA", y: "AA" }, format: "jwk" }); console.log("d-jwkim", "NO-THROW"); }
catch (e) { console.log("d-jwkim", e.code === "ERR_CRYPTO_INVALID_JWK"); }
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
// 10f crypto首轮翻转（真机口径）：无参 copy 回默认长（32B），非保留源长 16B。
console.log("x-copy", h.digest("hex").length === 32 && c2.digest("hex").length === 64);
try { createHash("shake256", { outputLength: -1 }); console.log("x-badlen-never", false); }
// 10f crypto首轮翻转（真机口径）：负 outputLength 报 OUT_OF_RANGE（旧 ARG_VALUE 系伪语义）。
catch (e) { console.log("x-badlen", e.code === "ERR_OUT_OF_RANGE"); }
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
// 10f 四轮翻转（真机口径）：非对称导出必须显式 type（typeless → ARG_VALUE
// 'options.type' is invalid；§4.65/§4.82 翻旧断言）。
try { privateKey.export({ format: "pem" }); console.log("mk-pem", "NO-THROW"); }
catch (e) { console.log("mk-pem", e.code === "ERR_INVALID_ARG_VALUE"); }
console.log("mk-pem-typed", privateKey.export({ format: "pem", type: "pkcs8" }).startsWith("-----BEGIN PRIVATE KEY-----"),
  publicKey.export({ format: "pem", type: "spki" }).startsWith("-----BEGIN PUBLIC KEY-----"));
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
        "mk-pem true",
        "mk-pem-typed true true",
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
// OKP 导出即标准 DER（10e 起直吐 PKCS#8/SPKI，不再经 raw 手工包）
const edpair = crypto.generateKeyPairSync("ed25519");
console.log("xi-okp-shape",
  edpair.privateKey.export({ format: "der", type: "pkcs8" }).length === 48,
  edpair.publicKey.export({ format: "der", type: "spki" }).length === 44);
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

#[test]
fn phase10e_crypto_gcm_anyiv() {
    // 10e-2 GCM 任意 iv：真 Node 交叉取证逐字节向量 + 往返 + 报错/边界三件
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { createCipheriv, createDecipheriv } from "node:crypto";
const k = Buffer.from("000102030405060708090a0b0c0d0e0f", "hex");
// 已知向量（与真机逐字节一致；12B 回归 crate 路径）
for (const [ivh, ct, tag] of [
  ["01", "138fb39fea1878f98e", "4e8c59b0daa7aca4d3da0bc3058f77a5"],
  ["0102030405060708", "0abdd127a6e4463bbe", "5fee9590bf890f933f19030aa67fad71"],
  ["000102030405060708090a0b", "fb09cba2093bb01706", "ce6f0f4faa84b1d687a70f3fd41cbf67"],
  ["000102030405060708090a0b0c0d0e0f10", "2300d58d728165e659", "457e809f0765819935663e0ad8afe3e7"],
]) {
  const iv = Buffer.from(ivh, "hex");
  const c = createCipheriv("aes-128-gcm", k, iv);
  c.setAAD(Buffer.from("aad"));
  const out = Buffer.concat([c.update("hello GCM", "utf8"), c.final()]);
  console.log("vec-" + iv.length, out.toString("hex") === ct, c.getAuthTag().toString("hex") === tag);
  const d = createDecipheriv("aes-128-gcm", k, iv);
  d.setAAD(Buffer.from("aad")); d.setAuthTag(c.getAuthTag());
  console.log("rt-" + iv.length, Buffer.concat([d.update(Buffer.from(ct, "hex")), d.final()]).toString() === "hello GCM");
}
// 192/256 档非 12B 往返
for (const [alg, kl] of [["aes-192-gcm", 24], ["aes-256-gcm", 32]]) {
  const key = Buffer.alloc(kl, 5), nonce = Buffer.alloc(8, 6);
  const c = createCipheriv(alg, key, nonce);
  const ct = Buffer.concat([c.update("data", "utf8"), c.final()]);
  const d = createDecipheriv(alg, key, nonce);
  d.setAuthTag(c.getAuthTag());
  console.log("wide-" + alg, Buffer.concat([d.update(ct), d.final()]).toString() === "data");
}
// 报错：空 iv；错 tag 无码错
try { createCipheriv("aes-128-gcm", k, Buffer.alloc(0)); } catch (e) { console.log("emptyiv", e.code); }
try {
  const iv = Buffer.alloc(8, 1);
  const c = createCipheriv("aes-128-gcm", k, iv);
  const ct = Buffer.concat([c.update("x", "utf8"), c.final()]);
  const d = createDecipheriv("aes-128-gcm", k, iv);
  d.setAuthTag(Buffer.alloc(16, 2));
  d.update(ct); d.final();
} catch (e) { console.log("badtag8", e.code === undefined, e.message === "Unsupported state or unable to authenticate data"); }
"#,
    );
    assert!(out.contains("vec-1 true true"), "out: {out}");
    assert!(out.contains("vec-8 true true"), "out: {out}");
    assert!(out.contains("vec-12 true true"), "out: {out}");
    assert!(out.contains("vec-17 true true"), "out: {out}");
    assert!(out.contains("rt-1 true"), "out: {out}");
    assert!(out.contains("rt-8 true"), "out: {out}");
    assert!(out.contains("rt-12 true"), "out: {out}");
    assert!(out.contains("rt-17 true"), "out: {out}");
    assert!(out.contains("wide-aes-192-gcm true"), "out: {out}");
    assert!(out.contains("wide-aes-256-gcm true"), "out: {out}");
    assert!(out.contains("emptyiv ERR_CRYPTO_INVALID_IV"), "out: {out}");
    assert!(out.contains("badtag8 true true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase10e_crypto_ed448() {
    // 10e Ed448：真机取证向量（确定性签名逐字节）+ 全链 + 报错/边界三件 + 证书
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("ed448-cert.pem")
        .write_str(include_str!("../fixtures/ed448-cert.pem"))
        .unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import crypto, { generateKeyPairSync, generateKeyPair, sign, verify, createPrivateKey, createPublicKey } from "node:crypto";
import fs from "node:fs";
const { publicKey, privateKey } = generateKeyPairSync("ed448");
console.log("types", publicKey.asymmetricKeyType, privateKey.type, publicKey.type);
const sig = sign(null, Buffer.from("hello"), privateKey);
console.log("sig", sig.length === 114, verify(null, Buffer.from("hello"), publicKey, sig));
// 已知向量（定种子的确定性签名，真机同值）
const seed = Buffer.concat([Buffer.from([1]), Buffer.alloc(56, 0x42)]);
const fixPriv = createPrivateKey({ key: Buffer.concat([Buffer.from("3047020100300506032b6571043b0439", "hex"), seed]), format: "der", type: "pkcs8" });
const fixSig = sign(null, Buffer.from("determinism-check"), fixPriv);
console.log("vec", fixSig.toString("hex") === "390f63c4e8ccaa3e9dce99084c5a8716caf1be49eeb40e452cec29a576f4dcec6fef3a39fd44da6d561277738e75acc162ef69e846230571802e93bcb4966519c15a1c1417adfb1bb70a8c87a1e873843cc1afbdbcf87442839b190f5a45ac21600122c2fd6ddb81b5f1bc3b36843c410400");
// JWK 进出 + DER 形态（73/69B）+ 重导入
const jwk = publicKey.export({ format: "jwk" });
console.log("jwk", jwk.kty === "OKP" && jwk.crv === "Ed448" && typeof jwk.x === "string");
const pub2 = createPublicKey({ key: publicKey.export({ format: "der", type: "spki" }), format: "der", type: "spki" });
console.log("der-pub", pub2.asymmetricKeyType === "ed448", publicKey.export({ format: "der", type: "spki" }).length === 69);
const privDer = privateKey.export({ format: "der", type: "pkcs8" });
console.log("der-priv", privDer.length === 73);
const priv2 = createPrivateKey({ key: privDer, format: "der", type: "pkcs8" });
console.log("reimport", verify(null, Buffer.from("hello"), createPublicKey(priv2), sig));
const jwkPriv = privateKey.export({ format: "jwk" });
const priv3 = createPrivateKey({ key: jwkPriv, format: "jwk" });
console.log("jwk-priv", verify(null, Buffer.from("hello"), createPublicKey(priv3), sig));
// async 形态
generateKeyPair("ed448", (e, pub, priv) => {
  console.log("async", e === null, pub.asymmetricKeyType === "ed448", priv.type === "private");
});
// 报错三件
try { sign("sha256", Buffer.from("m"), privateKey); } catch (e) { console.log("sign-alg", e.code); }
try { verify("sha256", Buffer.from("m"), publicKey, sig); } catch (e) { console.log("verify-alg", e.code); }
try { sign(null, Buffer.from("m"), publicKey); } catch (e) { console.log("sign-pub", e.code); }
// 10f crypto二轮翻转（真机口径）：私钥验签合法（派生公钥），错消息回 false。
console.log("verify-priv", verify(null, Buffer.from("m"), privateKey, sig) === false);
try { createPrivateKey({ key: Buffer.alloc(10), format: "der", type: "pkcs8" }); } catch (e) { console.log("bad-der", e.code); }
try { publicKey.export({ format: "der", type: "spki" }).length; console.log("exp-ok", true); } catch (e) { console.log("exp-ok", false); }
try { publicKey.export({ format: "der", type: "pkcs8" }); } catch (e) { console.log("exp-pub-pkcs8", e.code); }
// 边界：错签/错钥回 false；空消息往返
const bad = Buffer.from(sig); bad[0] ^= 0xff;
console.log("tamper", verify(null, Buffer.from("hello"), publicKey, bad) === false);
const { publicKey: other } = generateKeyPairSync("ed448");
console.log("wrongkey", verify(null, Buffer.from("hello"), other, sig) === false);
console.log("empty", verify(null, Buffer.alloc(0), publicKey, sign(null, Buffer.alloc(0), privateKey)));
// 证书（openssl ed448 自签固件）
const x = new crypto.X509Certificate(fs.readFileSync("ed448-cert.pem", "utf8"));
console.log("cert", x.verify(x.publicKey), x.publicKey.asymmetricKeyType === "ed448", x.verify(other) === false);
"#,
    );
    assert!(out.contains("types ed448 private public"), "out: {out}");
    assert!(out.contains("sig true true"), "out: {out}");
    assert!(out.contains("vec true"), "out: {out}");
    assert!(out.contains("jwk true"), "out: {out}");
    assert!(out.contains("der-pub true true"), "out: {out}");
    assert!(out.contains("der-priv true"), "out: {out}");
    assert!(out.contains("reimport true"), "out: {out}");
    assert!(out.contains("jwk-priv true"), "out: {out}");
    assert!(out.contains("async true true true"), "out: {out}");
    assert!(out.contains("sign-alg ERR_OSSL_INVALID_DIGEST"), "out: {out}");
    assert!(out.contains("verify-alg ERR_OSSL_INVALID_DIGEST"), "out: {out}");
    assert!(out.contains("sign-pub ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("verify-priv true"), "out: {out}");
    assert!(out.contains("bad-der ERR_INVALID_ARG_VALUE"), "out: {out}");
    assert!(out.contains("exp-ok true"), "out: {out}");
    assert!(out.contains("exp-pub-pkcs8 ERR_INVALID_ARG_VALUE"), "out: {out}");
    assert!(out.contains("tamper true"), "out: {out}");
    assert!(out.contains("wrongkey true"), "out: {out}");
    assert!(out.contains("empty true"), "out: {out}");
    assert!(out.contains("cert true true true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase10f_crypto_round1_parity() {
    // 10f crypto首轮：call-without-new + DEP0179/DEP0181 + uuid 校验 + 摘要别名 +
    // outputLength 全套 + 流式鸭子面 + ECB + DH 数值形（正常/报错/边界三件）
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import crypto, { createHash, createHmac, createCipheriv, createDecipheriv, createSecretKey,
  createDiffieHellman, randomUUID, randomUUIDv7 } from "node:crypto";
// 无 new 调用（真机口径；Hash/Hmac 附 DEP0179/DEP0181 一次性警告）
const warns = [];
process.on("warning", (w) => warns.push(w.code));
const h0 = crypto.Hash("sha256");
console.log("cw-hash", h0 instanceof crypto.Hash);
const m0 = crypto.Hmac("sha256", "Node");
console.log("cw-hmac", m0 instanceof crypto.Hmac);
const c0 = crypto.Cipheriv("aes-128-cbc", "1234567890123456", "1234567890123456");
console.log("cw-civ", c0 instanceof crypto.Cipheriv);
const d0 = crypto.Decipheriv("aes-128-cbc", "1234567890123456", "1234567890123456");
console.log("cw-dcv", d0 instanceof crypto.Decipheriv);
const e0 = crypto.ECDH("prime256v1");
console.log("cw-ecdh", e0 instanceof crypto.ECDH);
const g0 = crypto.DiffieHellmanGroup("modp14");
console.log("cw-dhg", g0 instanceof crypto.DiffieHellmanGroup, g0 instanceof crypto.DiffieHellman);
// uuid 选项校验（真机文案逐字对码）
console.log("uuid-opt", typeof randomUUID({ disableEntropyCache: true }) === "string");
console.log("uuid7-opt", typeof randomUUIDv7({ disableEntropyCache: true }) === "string");
for (const [tag, fn] of [["u-bad-num", () => randomUUID(1)],
    ["u-bad-prop", () => randomUUID({ disableEntropyCache: "" })],
    ["u-bad-null", () => randomUUID(null)],
    ["u7-bad-num", () => randomUUIDv7(1)],
    ["u7-bad-prop", () => randomUUIDv7({ disableEntropyCache: "" })]]) {
  try { fn(); console.log(tag, "NO-THROW"); } catch (e) { console.log(tag, e.code); }
}
// 摘要别名（逐字节对真机）
console.log("dgst-alias224", createHash("sha224").update("abc").digest("hex") === "23097d223405d8228642a477bda255b32aadbce4bda0b3f7e36c9da7");
console.log("dgst-aliasrip", createHash("ripemd").update("abc").digest("hex") === "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc");
console.log("dgst-aliasdss1", createHmac("dss1", "key").update("The quick brown fox jumps over the lazy dog").digest("hex") === "de7c9b85b8b78aa6bc8a7a36f70a90701c9db4d9");
// outputLength 全套
console.log("olen-ok224", createHash("sha224", { outputLength: 28 }).update("abc").digest("hex").slice(0, 8) === "23097d22");
try { createHash("sha256", { outputLength: 28 }); console.log("olen-notxof", "NO-THROW"); }
catch (e) { console.log("olen-notxof", e.code); }
try { createHash("sha256", { outputLength: null }); console.log("olen-argtype", "NO-THROW"); }
catch (e) { console.log("olen-argtype", e.code); }
try { createHash("sha256", { outputLength: -1 }); console.log("olen-range", "NO-THROW"); }
catch (e) { console.log("olen-range", e.code); }
console.log("copy-ovr", createHash("shake128", { outputLength: 5 }).copy({ outputLength: 0 }).digest("hex") === "");
console.log("copy-dflt", createHash("shake256", { outputLength: 0 }).copy().digest("hex").length === 64);
// 流式鸭子面
let s1 = createHash("sha512"); s1.end("Test123");
console.log("stm-hash", s1.read().toString("hex").slice(0, 16) === createHash("sha512").update("Test123").digest("hex").slice(0, 16));
const s2 = createHmac("sha256", "key"); s2.end("The quick brown fox jumps over the lazy dog");
console.log("stm-hmac", s2.read().toString("hex") === createHmac("sha256", "key").update("The quick brown fox jumps over the lazy dog").digest("hex"));
const s3 = createCipheriv("des-ede3-cbc", "0123456789abcd0123456789", "12345678");
s3.end("Test123Test123");
const s3ct = s3.read();
console.log("stm-rlen", s3ct.length === 16);
const s4 = createDecipheriv("des-ede3-cbc", "0123456789abcd0123456789", "12345678");
s4.end(s3ct);
console.log("stm-ciph", s4.read().toString("utf8") === "Test123Test123");
// ECB（真机向量前缀 + 往返 + iv 规则 + nid）
const ek = Buffer.from("000102030405060708090a0b0c0d0e0f", "hex");
const ept = Buffer.from("00112233445566778899aabbccddeeff", "hex");
const ee = createCipheriv("aes-128-ecb", ek, null);
console.log("ecb-rt", ee.update(ept).toString("hex").slice(0, 16) === "69c4e0d86a7b0430");
const ecbCt = (() => { const x = createCipheriv("aes-128-ecb", ek, null); return Buffer.concat([x.update(ept), x.final()]); })();
const ed = createDecipheriv("aes-128-ecb", ek, Buffer.alloc(0));
console.log("ecb-rt2", Buffer.concat([ed.update(ecbCt), ed.final()]).equals(ept));
console.log("ecb-nid", crypto.getCipherInfo("aes-128-ecb").nid === 418 && crypto.getCipherInfo("aes-128-ecb").ivLength === undefined);
try { createCipheriv("aes-128-ecb", ek, Buffer.alloc(1)); console.log("ecb-ivbad", "NO-THROW"); }
catch (e) { console.log("ecb-ivbad", e.code); }
try { createCipheriv("aes-128-ecb", ek); console.log("ecb-ivundef", "NO-THROW"); }
catch (e) { console.log("ecb-ivundef", e.code); }
try { createCipheriv("aes-128-ecb", Buffer.alloc(17), null); console.log("ecb-keylen", "NO-THROW"); }
catch (e) { console.log("ecb-keylen", e.code); }
// DH 数值形 + prime buffer 形
const dh1 = createDiffieHellman(256);
console.log("dh-num", dh1.getPrime("buffer").length === 32);
const dh2 = crypto.DiffieHellman(dh1.getPrime("buffer"), "buffer");
console.log("cw-dh", dh2 instanceof crypto.DiffieHellman);
// 'buffer' 编码与二次 digest 形态
console.log("bufenc", Buffer.isBuffer(createHmac("sha1", "k").update("d").digest("buffer")));
const hz = createHmac("sha1", "k"); hz.update("d"); hz.digest();
console.log("bufenc2", Buffer.isBuffer(hz.digest("buffer")) && hz.digest("buffer").length === 0 && hz.digest("hex") === "");
// KeyObject 作 HMAC key + 参数名文案
console.log("hmac-keyobj", createHmac("sha256", createSecretKey(Buffer.from("key"))).update("msg").digest("hex") === createHmac("sha256", "key").update("msg").digest("hex"));
try { createHmac(null); } catch (e) { console.log("needstr-hmac", e.code, JSON.stringify(e.message)); }
try { createHash(); } catch (e) { console.log("needstr-undef", e.code, JSON.stringify(e.message)); }
try { createCipheriv(null); } catch (e) { console.log("ciph-null", e.code, JSON.stringify(e.message)); }
setTimeout(() => console.log("dep-warn", warns.includes("DEP0179"), warns.includes("DEP0181")), 20);
"#,
    );
    assert!(out.contains("cw-hash true"), "out: {out}");
    assert!(out.contains("cw-hmac true"), "out: {out}");
    assert!(out.contains("cw-civ true"), "out: {out}");
    assert!(out.contains("cw-dcv true"), "out: {out}");
    assert!(out.contains("cw-ecdh true"), "out: {out}");
    assert!(out.contains("cw-dhg true true"), "out: {out}");
    assert!(out.contains("cw-dh true"), "out: {out}");
    assert!(out.contains("uuid-opt true"), "out: {out}");
    assert!(out.contains("uuid7-opt true"), "out: {out}");
    assert!(out.contains("u-bad-num ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("u-bad-prop ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("u-bad-null ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("u7-bad-num ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("u7-bad-prop ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("dgst-alias224 true"), "out: {out}");
    assert!(out.contains("dgst-aliasrip true"), "out: {out}");
    assert!(out.contains("dgst-aliasdss1 true"), "out: {out}");
    assert!(out.contains("olen-ok224 true"), "out: {out}");
    assert!(out.contains("olen-notxof ERR_OSSL_EVP_NOT_XOF_OR_INVALID_LENGTH"), "out: {out}");
    assert!(out.contains("olen-argtype ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("olen-range ERR_OUT_OF_RANGE"), "out: {out}");
    assert!(out.contains("copy-ovr true"), "out: {out}");
    assert!(out.contains("copy-dflt true"), "out: {out}");
    assert!(out.contains("stm-hash true"), "out: {out}");
    assert!(out.contains("stm-hmac true"), "out: {out}");
    assert!(out.contains("stm-rlen true"), "out: {out}");
    assert!(out.contains("stm-ciph true"), "out: {out}");
    assert!(out.contains("ecb-rt true"), "out: {out}");
    assert!(out.contains("ecb-rt2 true"), "out: {out}");
    assert!(out.contains("ecb-nid true"), "out: {out}");
    assert!(out.contains("ecb-ivbad ERR_CRYPTO_INVALID_IV"), "out: {out}");
    assert!(out.contains("ecb-ivundef ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("ecb-keylen ERR_CRYPTO_INVALID_KEYLEN"), "out: {out}");
    assert!(out.contains("dh-num true"), "out: {out}");
    assert!(out.contains("bufenc true"), "out: {out}");
    assert!(out.contains("bufenc2 true"), "out: {out}");
    assert!(out.contains("hmac-keyobj true"), "out: {out}");
    assert!(out.contains(r#"needstr-hmac ERR_INVALID_ARG_TYPE "The \"hmac\" argument must be of type string. Received null""#), "out: {out}");
    assert!(out.contains(r#"needstr-undef ERR_INVALID_ARG_TYPE "The \"algorithm\" argument must be of type string. Received undefined""#), "out: {out}");
    assert!(out.contains(r#"ciph-null ERR_INVALID_ARG_TYPE "The \"cipher\" argument must be of type string. Received null""#), "out: {out}");
    assert!(out.contains("dep-warn true true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase10f_crypto_round2_parity() {
    // 10f crypto二轮：DH 组/KeyObject 品牌/RSA 位长/pkcs1/加密 PEM/混合 OAEP
    //（正常/报错/边界三件；慢操作一律小参数）
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import crypto, { KeyObject, createDiffieHellman, createDiffieHellmanGroup,
  getDiffieHellman, generateKeyPairSync, createSecretKey, createPublicKey,
  createPrivateKey, publicEncrypt, privateDecrypt, privateEncrypt, publicDecrypt,
  randomBytes } from "node:crypto";
import { types } from "node:util";
// DH 组与 flavor
console.log("r2-modp", getDiffieHellman("modp1").getPrime("hex").length === 192,
  getDiffieHellman("modp2").getPrime("hex").length === 256);
const r2g = getDiffieHellman("modp2");
console.log("r2-flav", r2g.constructor === crypto.DiffieHellmanGroup,
  r2g.setPrivateKey === undefined, r2g.setPublicKey === undefined);
console.log("r2-gen", createDiffieHellman(getDiffieHellman("modp14").getPrime(), Buffer.from([2])).getGenerator("hex") === "02");
// RSA 位长与 details
const r2rsa = generateKeyPairSync("rsa", { modulusLength: 512 });
console.log("r2-rsa512", r2rsa.publicKey.asymmetricKeyDetails.modulusLength === 512,
  typeof r2rsa.publicKey.asymmetricKeyDetails.publicExponent === "bigint");
try { generateKeyPairSync("rsa", { modulusLength: 511 }); console.log("r2-small", "NO-THROW"); }
catch (e) { console.log("r2-small", e.code); }
// KeyObject 品牌面
const r2sec = createSecretKey(Buffer.alloc(16));
console.log("r2-noown", Object.getOwnPropertyNames(r2sec).length === 0,
  Object.getOwnPropertySymbols(r2sec).length === 0);
console.log("r2-tag", String(r2sec) === "[object KeyObject]");
console.log("r2-isKO", types.isKeyObject(r2sec) === true, types.isKeyObject({}) === false);
try { crypto.KeyObject.prototype.type.call({}); console.log("r2-brand", "NO-THROW"); }
catch (e) { console.log("r2-brand", e.code); }
const r2asymGet = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Object.getPrototypeOf(r2rsa.publicKey)), "asymmetricKeyType").get;
try { r2asymGet.call(r2sec); console.log("r2-secasym", "NO-THROW"); }
catch (e) { console.log("r2-secasym", e.code); }
console.log("r2-eq", r2sec.equals(r2sec) === true);
try { r2sec.equals({}); console.log("r2-eqbad", "NO-THROW"); }
catch (e) { console.log("r2-eqbad", e.code); }
try { KeyObject.from("x"); console.log("r2-from", "NO-THROW"); }
catch (e) { console.log("r2-from", e.code); }
try { new KeyObject("nope"); console.log("r2-ctor", "NO-THROW"); }
catch (e) { console.log("r2-ctor", e.code); }
// ESM 具名导出 + 回调异步形
console.log("r2-esm", typeof KeyObject === "function");
crypto.sign("sha256", Buffer.from("m"), r2rsa.privateKey, (e, s) =>
  console.log("r2-async", e === null, s.length === 64));
// pkcs1 与派生规则
const r2pkcs1 = r2rsa.publicKey.export({ type: "pkcs1", format: "pem" });
console.log("r2-pkcs1pem", r2pkcs1.split("\n")[0] === "-----BEGIN RSA PUBLIC KEY-----");
console.log("r2-derive", createPublicKey(r2rsa.privateKey).type === "public");
try { createPublicKey(r2rsa.publicKey); console.log("r2-pubpub", "NO-THROW"); }
catch (e) { console.log("r2-pubpub", e.code); }
try { createPrivateKey(r2rsa.privateKey); console.log("r2-privpriv", "NO-THROW"); }
catch (e) { console.log("r2-privpriv", e.code); }
// 加密 PEM 往返 + 缺/错口令
const r2enc = r2rsa.privateKey.export({ type: "pkcs1", format: "pem", cipher: "aes-128-cbc", passphrase: "pw" });
console.log("r2-enchdr", r2enc.split("\n")[1] === "Proc-Type: 4,ENCRYPTED");
const r2back = createPrivateKey({ key: r2enc, passphrase: "pw" });
console.log("r2-encrt", r2back.type === "private");
try { createPrivateKey({ key: r2enc }); console.log("r2-nopass", "NO-THROW"); }
catch (e) { console.log("r2-nopass", e.code); }
try { createPrivateKey({ key: r2enc, passphrase: "bad" }); console.log("r2-badpass", "NO-THROW"); }
catch (e) { console.log("r2-badpass", e.code); }
// 混合 OAEP + 反向操作 + NO_PADDING
const r2msg = Buffer.from("hello-mgf1");
const r2rsa1k = generateKeyPairSync("rsa", { modulusLength: 1024 });
const r2ct = publicEncrypt({ key: r2rsa1k.publicKey, padding: 4, oaepHash: "sha256", mgf1Hash: "sha1" }, r2msg);
console.log("r2-mgf1", privateDecrypt({ key: r2rsa1k.privateKey, padding: 4, oaepHash: "sha256", mgf1Hash: "sha1" }, r2ct).toString() === "hello-mgf1");
try { publicEncrypt({ key: r2rsa1k.publicKey, padding: 4, oaepHash: "sha256", mgf1Hash: 1 }, r2msg); console.log("r2-mgf1bad", "NO-THROW"); }
catch (e) { console.log("r2-mgf1bad", e.code); }
const r2pe = privateEncrypt(r2rsa.privateKey, r2msg);
console.log("r2-privenc", publicDecrypt(r2rsa.publicKey, r2pe).toString() === "hello-mgf1");
const r2raw = publicEncrypt({ key: r2rsa.publicKey, padding: 3 }, Buffer.alloc(64, 7));
console.log("r2-nopad", privateDecrypt({ key: r2rsa.privateKey, padding: 3 }, r2raw).equals(Buffer.alloc(64, 7)));
// 验签形态错回 false + 输出编码形
const r2sig = crypto.sign("sha256", r2msg, r2rsa.privateKey);
console.log("r2-verifyfalse", crypto.verify("sha256", r2msg, r2rsa.publicKey, Buffer.alloc(0)) === false);
const r2s = crypto.createSign("SHA256"); r2s.update(r2msg);
console.log("r2-signenc", typeof r2s.sign(r2rsa.privateKey, "hex") === "string");
// export 门与 JWK 非法形
try { r2rsa.publicKey.export(undefined); console.log("r2-expopt", "NO-THROW"); }
catch (e) { console.log("r2-expopt", e.code); }
try { r2rsa.publicKey.export({ format: "der", type: "pkcs8" }); console.log("r2-expmat", "NO-THROW"); }
catch (e) { console.log("r2-expmat", e.code); }
try { createPrivateKey({ key: { kty: "RSA", n: "AQAB", e: "AQAB" }, format: "jwk" }); console.log("r2-jwkbad", "NO-THROW"); }
catch (e) { console.log("r2-jwkbad", e.code); }
setTimeout(() => console.log("r2-done"), 20);
"#,
    );
    assert!(out.contains("r2-modp true true"), "out: {out}");
    assert!(out.contains("r2-flav true true true"), "out: {out}");
    assert!(out.contains("r2-gen true"), "out: {out}");
    assert!(out.contains("r2-rsa512 true true"), "out: {out}");
    assert!(out.contains("r2-small ERR_OSSL_KEY_SIZE_TOO_SMALL"), "out: {out}");
    assert!(out.contains("r2-noown true true"), "out: {out}");
    assert!(out.contains("r2-tag true"), "out: {out}");
    assert!(out.contains("r2-isKO true true"), "out: {out}");
    assert!(out.contains("r2-brand ERR_INVALID_THIS"), "out: {out}");
    assert!(out.contains("r2-secasym ERR_INVALID_THIS"), "out: {out}");
    assert!(out.contains("r2-eq true"), "out: {out}");
    assert!(out.contains("r2-eqbad ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("r2-from ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("r2-ctor ERR_INVALID_ARG_VALUE"), "out: {out}");
    assert!(out.contains("r2-esm true"), "out: {out}");
    assert!(out.contains("r2-async true true"), "out: {out}");
    assert!(out.contains("r2-pkcs1pem true"), "out: {out}");
    assert!(out.contains("r2-derive true"), "out: {out}");
    assert!(out.contains("r2-pubpub ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE"), "out: {out}");
    assert!(out.contains("r2-privpriv ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("r2-enchdr true"), "out: {out}");
    assert!(out.contains("r2-encrt true"), "out: {out}");
    assert!(out.contains("r2-nopass ERR_MISSING_PASSPHRASE"), "out: {out}");
    assert!(out.contains("r2-badpass ERR_OSSL_BAD_DECRYPT"), "out: {out}");
    assert!(out.contains("r2-mgf1 true"), "out: {out}");
    assert!(out.contains("r2-mgf1bad ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("r2-privenc true"), "out: {out}");
    assert!(out.contains("r2-nopad true"), "out: {out}");
    assert!(out.contains("r2-verifyfalse true"), "out: {out}");
    assert!(out.contains("r2-signenc true"), "out: {out}");
    assert!(out.contains("r2-expopt ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("r2-expmat ERR_INVALID_ARG_VALUE"), "out: {out}");
    assert!(out.contains("r2-jwkbad ERR_CRYPTO_INVALID_JWK"), "out: {out}");
    assert!(out.contains("r2-done"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase10f_crypto_x448_parity() {
    // 10f crypto三轮：X448 全链（用户拍板引 x448 =0.14.0-pre.12，见
    // docs/dependencies3.md §5）——生成/导入/DER/JWK/raw/DH/低阶点，
    // 正常/报错/边界三件；每项真机 26.8.2 对拍。
    let dir = assert_fs::TempDir::new().unwrap();
    // node 套件 fixture（test/fixtures/keys/x448_*.pem，MIT）落盘（§4.44）。
    let priv_pem = "-----BEGIN PRIVATE KEY-----\nMEYCAQAwBQYDK2VvBDoEOLTDbazv6vHZWOmODQ3kk8TUOQgApB4j75rpInT5zSLl\n/xJHK8ixF7f+4uo+mGTCrK1sktI5UmCZ\n-----END PRIVATE KEY-----\n";
    let pub_pem = "-----BEGIN PUBLIC KEY-----\nMEIwBQYDK2VvAzkAioHSHVpTs6hMvghosEJDIR7ceFiE3+Xccxati64oOVJ7NWjf\nozE7ae31PXIUFq6cVYgvSKsDFPA=\n-----END PUBLIC KEY-----\n";
    dir.child("x448_priv.pem").write_str(priv_pem).unwrap();
    dir.child("x448_pub.pem").write_str(pub_pem).unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import crypto, { generateKeyPairSync, createPrivateKey, createPublicKey,
  diffieHellman } from "node:crypto";
import { readFileSync } from "node:fs";
import { deepStrictEqual } from "node:assert";
const log = (...a) => console.log(...a);
const privPem = readFileSync("x448_priv.pem", "ascii");
const pubPem = readFileSync("x448_pub.pem", "ascii");
// 生成 + 类型面
const { publicKey, privateKey } = generateKeyPairSync("x448");
log("x448-gen", privateKey.asymmetricKeyType, publicKey.asymmetricKeyType,
  privateKey.type, publicKey.type, privateKey.symmetricKeySize);
// fixture 导入 → PEM 逐字导出（套件 x448 行断言）
const fk = createPrivateKey(privPem);
log("x448-fixture", fk.asymmetricKeyType, fk.export({ type: "pkcs8", format: "pem" }) === privPem);
const fpk = createPublicKey(pubPem);
log("x448-fixturepub", fpk.asymmetricKeyType, fpk.export({ type: "spki", format: "pem" }) === pubPem);
// JWK 双向（deepStrictEqual 不看键序，与真机一致）
const jwk = fk.export({ format: "jwk" });
deepStrictEqual(jwk, { crv: "X448",
  x: "ioHSHVpTs6hMvghosEJDIR7ceFiE3-Xccxati64oOVJ7NWjfozE7ae31PXIUFq6cVYgvSKsDFPA",
  d: "tMNtrO_q8dlY6Y4NDeSTxNQ5CACkHiPvmukidPnNIuX_EkcryLEXt_7i6j6YZMKsrWyS0jlSYJk",
  kty: "OKP" });
log("x448-jwk", true);
const jk = createPrivateKey({ key: jwk, format: "jwk" });
log("x448-jwkimp", jk.asymmetricKeyType, jk.export({ type: "pkcs8", format: "pem" }) === privPem);
// DH 双侧一致（56B）
const { publicKey: pb2, privateKey: pv2 } = generateKeyPairSync("x448");
const s1 = diffieHellman({ privateKey, publicKey: pb2 });
const s2 = diffieHellman({ privateKey: pv2, publicKey });
log("x448-dh", s1.length, s2.length, Buffer.compare(s1, s2) === 0);
// raw 往返 + 私钥建公钥
const rp = privateKey.export({ format: "raw-private" });
const ru = publicKey.export({ format: "raw-public" });
const k3 = createPrivateKey({ key: rp, format: "raw-private", asymmetricKeyType: "x448" });
const pu3 = createPublicKey({ key: rp, format: "raw-private", asymmetricKeyType: "x448" });
log("x448-raw", Buffer.isBuffer(rp), rp.length, ru.length,
  k3.asymmetricKeyType, Buffer.compare(k3.export({ format: "raw-private" }), rp) === 0,
  pu3.asymmetricKeyType, Buffer.compare(pu3.export({ format: "raw-public" }), ru) === 0);
// 低阶点（全零 u）→ 真机同码
try {
  const kz = createPublicKey({ key: Buffer.alloc(56), format: "raw-public", asymmetricKeyType: "x448" });
  diffieHellman({ privateKey, publicKey: kz });
  log("x448-loworder", "NO-THROW");
} catch (e) { log("x448-loworder", e.code, e.message.startsWith("error:1C8000A4")); }
// sign/verify 无原语
try { crypto.sign(null, Buffer.alloc(8), privateKey); log("x448-sign", "NO-THROW"); }
catch (e) { log("x448-sign", e.code); }
// 报错矩阵（真机逐项）
const t = (f) => { try { f(); return "NO-THROW"; } catch (e) { return e.code; } };
log("x448-raw-noakt", t(() => createPrivateKey({ key: rp, format: "raw-private" })));
log("x448-raw-wrongtype", t(() => createPrivateKey({ key: rp, format: "raw-private", asymmetricKeyType: "x25519" })));
log("x448-raw-badlen", t(() => createPublicKey({ key: Buffer.alloc(32), format: "raw-public", asymmetricKeyType: "x448" })));
log("x448-rawpub-priv", t(() => createPrivateKey({ key: ru, format: "raw-public", asymmetricKeyType: "x448" })));
log("x448-priv-rawpub", t(() => privateKey.export({ format: "raw-public" })));
const rsa = generateKeyPairSync("rsa", { modulusLength: 512 });
log("x448-rsa-raw", t(() => rsa.privateKey.export({ format: "raw-private" })));
log("x448-gen-nope", t(() => generateKeyPairSync("nope")));
// 边界：x25519/ed448 无回归 + 编码参数生成
const x = generateKeyPairSync("x25519");
log("x448-x25519-ok", diffieHellman({ privateKey: x.privateKey, publicKey: generateKeyPairSync("x25519").publicKey }).length);
const { publicKey: encPub } = generateKeyPairSync("x448", { publicKeyEncoding: { type: "spki", format: "pem" } });
log("x448-enc", typeof encPub === "string" && encPub.startsWith("-----BEGIN PUBLIC KEY-----"), encPub ? encPub.length > 40 : false);
log("x448-done");
"#,
    );
    assert!(out.contains("x448-gen x448 x448 private public undefined"), "out: {out}");
    assert!(out.contains("x448-fixture x448 true"), "out: {out}");
    assert!(out.contains("x448-fixturepub x448 true"), "out: {out}");
    assert!(out.contains("x448-jwk true"), "out: {out}");
    assert!(out.contains("x448-jwkimp x448 true"), "out: {out}");
    assert!(out.contains("x448-dh 56 56 true"), "out: {out}");
    assert!(out.contains("x448-raw true 56 56 x448 true x448 true"), "out: {out}");
    assert!(out.contains("x448-loworder ERR_OSSL_FAILED_DURING_DERIVATION true"), "out: {out}");
    assert!(out.contains("x448-sign ERR_OSSL_EVP_OPERATION_NOT_SUPPORTED_FOR_THIS_KEYTYPE"), "out: {out}");
    assert!(out.contains("x448-raw-noakt ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("x448-raw-wrongtype ERR_INVALID_ARG_VALUE"), "out: {out}");
    assert!(out.contains("x448-raw-badlen ERR_INVALID_ARG_VALUE"), "out: {out}");
    assert!(out.contains("x448-rawpub-priv ERR_INVALID_ARG_VALUE"), "out: {out}");
    assert!(out.contains("x448-priv-rawpub ERR_INVALID_ARG_VALUE"), "out: {out}");
    assert!(out.contains("x448-rsa-raw ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS"), "out: {out}");
    assert!(out.contains("x448-gen-nope ERR_INVALID_ARG_VALUE"), "out: {out}");
    assert!(out.contains("x448-x25519-ok 32"), "out: {out}");
    assert!(out.contains("x448-enc true true"), "out: {out}");
    assert!(out.contains("x448-done"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase10f_crypto_round4_parity() {
    // 10f crypto四轮：key-objects 剩余阻塞簇——非对称导出 type/format 门矩阵、
    // EC raw 导入导出往返、EC sec1 导出、asymmetricKeyDetails（EC/OKP/DSA）、
    // OKP/EC JWK 校验矩阵、DSA JWK 面（无）。每项真机 26.8.2 对拍
    //（/tmp/wjs-agentA-ko/probe-node*.js 逐项）。
    let dir = assert_fs::TempDir::new().unwrap();
    // node 套件 fixture（test/fixtures/keys，MIT）落盘（§4.44）。
    dir.child("ec_priv.pem").write_str(
        "-----BEGIN PRIVATE KEY-----\nMIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgDxBsPQPIgMuMyQbx\nzbb9toew6Ev6e9O6ZhpxLNgmAEqhRANCAARfSYxhH+6V5lIg+M3O0iQBLf+53kuE\n2luIgWnp81/Ya1Gybj8tl4tJVu1GEwcTyt8hoA7vRACmCHnI5B1+bNpS\n-----END PRIVATE KEY-----\n",
    ).unwrap();
    dir.child("ec_pub.pem").write_str(
        "-----BEGIN PUBLIC KEY-----\nMFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEX0mMYR/uleZSIPjNztIkAS3/ud5L\nhNpbiIFp6fNf2GtRsm4/LZeLSVbtRhMHE8rfIaAO70QApgh5yOQdfmzaUg==\n-----END PUBLIC KEY-----\n",
    ).unwrap();
    dir.child("ed_priv.pem").write_str(
        "-----BEGIN PRIVATE KEY-----\nMC4CAQAwBQYDK2VwBCIEIMFSujN0jIUIdzSvuxka0lfgVVkMdRTuaVvIYUHrvzXQ\n-----END PRIVATE KEY-----\n",
    ).unwrap();
    dir.child("ed_pub.pem").write_str(
        "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEAK1wIouqnuiA04b3WrMa+xKIKIpfHetNZRv3h9fBf768=\n-----END PUBLIC KEY-----\n",
    ).unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { createPrivateKey, createPublicKey, createSecretKey, generateKeyPairSync } from "node:crypto";
import { readFileSync } from "node:fs";
import { deepStrictEqual } from "node:assert";
const log = (...a) => console.log(...a);
const throws = (fn) => { try { fn(); return "NO-THROW"; } catch (e) { return e.code ?? "no-code"; } };
const ecPriv = createPrivateKey(readFileSync("ec_priv.pem", "ascii"));
const ecPub = createPublicKey(readFileSync("ec_pub.pem", "ascii"));
const edPriv = createPrivateKey(readFileSync("ed_priv.pem", "ascii"));
const edPub = createPublicKey(readFileSync("ed_pub.pem", "ascii"));
const { privateKey: rsaPriv, publicKey: rsaPub } = generateKeyPairSync("rsa", { modulusLength: 512 });

// ── 正常件：EC raw 往返（raw-private=定长标量 32B；raw-public=04||X||Y 65B）──
const rawPriv = ecPriv.export({ format: "raw-private" });
const rawPub = ecPub.export({ format: "raw-public" });
log("r4-raw-len", rawPriv.length, rawPub.length, rawPub[0] === 4);
const impPriv = createPrivateKey({ key: rawPriv, format: "raw-private", asymmetricKeyType: "ec", namedCurve: "prime256v1" });
const impPub = createPublicKey({ key: rawPub, format: "raw-public", asymmetricKeyType: "ec", namedCurve: "prime256v1" });
log("r4-raw-rt", impPriv.type, impPriv.equals(ecPriv), impPub.type, impPub.equals(ecPub));
// raw-private 建公钥 → 派生；P-256 别名 'P-256' 同收
const impPub2 = createPublicKey({ key: rawPriv, format: "raw-private", asymmetricKeyType: "ec", namedCurve: "P-256" });
log("r4-raw-derive", impPub2.equals(ecPub), impPub2.asymmetricKeyDetails.namedCurve);
// EC sec1 导出（真机逐字节头：307702010104200f；PEM 标签 EC PRIVATE KEY）
const sec1 = ecPriv.export({ format: "der", type: "sec1" });
log("r4-sec1", sec1.length > 100, sec1.subarray(0, 8).toString("hex"), ecPriv.export({ format: "pem", type: "sec1" }).startsWith("-----BEGIN EC PRIVATE KEY-----"));
const sec1Back = createPrivateKey({ key: sec1, format: "der", type: "sec1" });
log("r4-sec1-rt", sec1Back.equals(ecPriv), sec1Back.asymmetricKeyDetails.namedCurve);

// ── 正常件：asymmetricKeyDetails（EC=OpenSSL 名；OKP={}；DSA 现状）──
log("r4-details-ec", ecPriv.asymmetricKeyDetails.namedCurve, ecPub.asymmetricKeyDetails.namedCurve,
  createPublicKey(ecPriv).asymmetricKeyDetails.namedCurve);
log("r4-details-okp", typeof edPriv.asymmetricKeyDetails === "object",
  Object.keys(edPriv.asymmetricKeyDetails).length, edPub.asymmetricKeyDetails !== undefined);

// ── 报错件：导出 type 门矩阵（真机 26 逐项：ARG_VALUE 'options.type' / INCOMPATIBLE）──
log("r4-gate-typeless", throws(() => rsaPriv.export({ format: "pem" })));
log("r4-gate-banana", throws(() => ecPub.export({ format: "pem", type: "banana" })));
log("r4-gate-pub-pkcs8", throws(() => ecPub.export({ format: "pem", type: "pkcs8" })));
log("r4-gate-pub-sec1", throws(() => rsaPub.export({ format: "pem", type: "sec1" })));
log("r4-gate-priv-spki", throws(() => ecPriv.export({ format: "der", type: "spki" })));
log("r4-gate-ec-pkcs1", throws(() => ecPriv.export({ format: "pem", type: "pkcs1" })));
log("r4-gate-ec-pub-pkcs1", throws(() => ecPub.export({ format: "pem", type: "pkcs1" })));
log("r4-gate-rsa-sec1", throws(() => rsaPriv.export({ format: "pem", type: "sec1" })));
log("r4-gate-pss-pkcs1", throws(() => {
  generateKeyPairSync("rsa-pss", { modulusLength: 512 }).publicKey.export({ format: "pem", type: "pkcs1" });
}));
// format 门：非对称 'buffer'/缺 format → ARG_VALUE；secret 'pem' → must-be-one-of
log("r4-gate-fmt-buffer", throws(() => ecPriv.export({ format: "buffer" })));
log("r4-gate-fmt-undef", throws(() => ecPriv.export({ format: undefined })));
log("r4-gate-sec-pem", throws(() => createSecretKey(Buffer.alloc(8)).export({ format: "pem" })));
log("r4-gate-sec-ok", createSecretKey(Buffer.alloc(8)).export({ format: undefined }).length === 8);

// ── 报错件：raw 门（kind 错位/非对称 raw 不支持/EC raw 坏形）──
log("r4-raw-kind", throws(() => ecPub.export({ format: "raw-private" })));
log("r4-raw-rsa", throws(() => rsaPriv.export({ format: "raw-private" })));
log("r4-raw-noCurve", throws(() => createPrivateKey({ key: rawPriv, format: "raw-private", asymmetricKeyType: "ec" })));
log("r4-raw-badCurve", throws(() => createPrivateKey({ key: rawPriv, format: "raw-private", asymmetricKeyType: "ec", namedCurve: "banana" })));
log("r4-raw-secp256r1", throws(() => createPrivateKey({ key: rawPriv, format: "raw-private", asymmetricKeyType: "ec", namedCurve: "secp256r1" })));
log("r4-raw-aktBanana", throws(() => createPrivateKey({ key: rawPriv, format: "raw-private", asymmetricKeyType: "banana", namedCurve: "prime256v1" })));
log("r4-raw-aktRsa", throws(() => createPrivateKey({ key: rawPriv, format: "raw-private", asymmetricKeyType: "rsa", namedCurve: "prime256v1" })));
log("r4-raw-badPoint", throws(() => createPublicKey({ key: Buffer.concat([Buffer.from([4]), Buffer.alloc(64, 7)]), format: "raw-public", asymmetricKeyType: "ec", namedCurve: "prime256v1" })));
log("r4-raw-compressed", throws(() => createPublicKey({ key: Buffer.concat([Buffer.from([2]), rawPub.subarray(1)]), format: "raw-public", asymmetricKeyType: "ec", namedCurve: "prime256v1" })));
log("r4-raw-wrongSize", throws(() => createPublicKey({ key: rawPub, format: "raw-public", asymmetricKeyType: "ec", namedCurve: "secp384r1" })));

// ── 报错件：OKP JWK 校验矩阵（真机 26 逐项）──
const edJwk = edPriv.export({ format: "jwk" });
log("r4-okp-badx", throws(() => createPrivateKey({ key: { ...edJwk, x: "A" + edJwk.x.slice(1) }, format: "jwk" })));
log("r4-okp-noD", throws(() => createPrivateKey({ key: { kty: edJwk.kty, crv: edJwk.crv, x: edJwk.x }, format: "jwk" })));
log("r4-okp-noCrv", throws(() => createPublicKey({ key: { kty: edJwk.kty, x: edJwk.x }, format: "jwk" })));
log("r4-okp-badCrv", throws(() => createPublicKey({ key: { ...edJwk, crv: "invalid" }, format: "jwk" })));
log("r4-okp-badD", throws(() => createPublicKey({ key: { ...edJwk, d: "AAAA" }, format: "jwk" })));
// 带 d 的 JWK 建公钥：x 必带（真机：d 无 x → INVALID_JWK）；x,d 齐即由 d 派生
log("r4-okp-fromD", throws(() => createPublicKey({ key: { kty: "OKP", crv: "Ed25519", d: edJwk.d }, format: "jwk" })));
log("r4-okp-fromDxd", createPublicKey({ key: { kty: "OKP", crv: "Ed25519", x: edJwk.x, d: edJwk.d }, format: "jwk" }).equals(edPub));
log("r4-okp-privD", createPrivateKey({ key: { kty: "OKP", crv: "Ed25519", x: edJwk.x, d: edJwk.d }, format: "jwk" }).equals(edPriv));

// ── 报错件：EC JWK 校验矩阵（真机 26 逐项；注意 crv 缺失=INVALID_JWK、非法=INVALID_CURVE）──
const ecJwk = ecPriv.export({ format: "jwk" });
deepStrictEqual(ecJwk, { kty: "EC", crv: "P-256",
  x: "X0mMYR_uleZSIPjNztIkAS3_ud5LhNpbiIFp6fNf2Gs", y: "UbJuPy2Xi0lW7UYTBxPK3yGgDu9EAKYIecjkHX5s2lI",
  d: "DxBsPQPIgMuMyQbxzbb9toew6Ev6e9O6ZhpxLNgmAEo" });
log("r4-ec-jwkexp", true);
log("r4-ec-badx", throws(() => createPrivateKey({ key: { ...ecJwk, x: "A" + ecJwk.x.slice(1) }, format: "jwk" })));
log("r4-ec-bady", throws(() => createPrivateKey({ key: { ...ecJwk, y: "A" + ecJwk.y.slice(1) }, format: "jwk" })));
log("r4-ec-noD", throws(() => createPrivateKey({ key: { kty: "EC", crv: ecJwk.crv, x: ecJwk.x, y: ecJwk.y }, format: "jwk" })));
log("r4-ec-noCrv", throws(() => createPublicKey({ key: { kty: "EC", x: ecJwk.x, y: ecJwk.y }, format: "jwk" })));
log("r4-ec-badCrv", throws(() => createPublicKey({ key: { ...ecJwk, crv: "invalid" }, format: "jwk" })));
log("r4-ec-badD", throws(() => createPublicKey({ key: { ...ecJwk, d: "AAAA" }, format: "jwk" })));
log("r4-ec-badPoint", throws(() => createPublicKey({ key: { kty: "EC", crv: "P-256", x: Buffer.alloc(32, 9).toString("base64url"), y: Buffer.alloc(32, 9).toString("base64url") }, format: "jwk" })));
log("r4-ec-priv-rt", createPrivateKey({ key: ecJwk, format: "jwk" }).equals(ecPriv));
log("r4-ec-pub-fromD", createPublicKey({ key: ecJwk, format: "jwk" }).equals(ecPub));

// ── 报错件：DSA JWK 面（无）──
const { publicKey: dsaPub } = generateKeyPairSync("dsa", { modulusLength: 1024, divisorLength: 160 });
log("r4-dsa-jwkexp", throws(() => dsaPub.export({ format: "jwk" })));
log("r4-dsa-jwkimp", throws(() => createPublicKey({ key: { kty: "DSA", p: "AA", q: "AA", g: "AA", y: "AA" }, format: "jwk" })));
// DSA details（真机 {modulusLength, divisorLength}）
log("r4-dsa-details", typeof dsaPub.asymmetricKeyDetails === "object",
  typeof dsaPub.asymmetricKeyDetails.modulusLength === "number",
  typeof dsaPub.asymmetricKeyDetails.divisorLength === "number",
  dsaPub.asymmetricKeyDetails.publicExponent === undefined);
log("r4-done");
"#,
    );
    for line in [
        "r4-raw-len 32 65 true",
        "r4-raw-rt private true public true",
        "r4-raw-derive true prime256v1",
        "r4-sec1 true 307702010104200f true",
        "r4-sec1-rt true prime256v1",
        "r4-details-ec prime256v1 prime256v1 prime256v1",
        "r4-details-okp true 0 true",
        "r4-gate-typeless ERR_INVALID_ARG_VALUE",
        "r4-gate-banana ERR_INVALID_ARG_VALUE",
        "r4-gate-pub-pkcs8 ERR_INVALID_ARG_VALUE",
        "r4-gate-pub-sec1 ERR_INVALID_ARG_VALUE",
        "r4-gate-priv-spki ERR_INVALID_ARG_VALUE",
        "r4-gate-ec-pkcs1 ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS",
        "r4-gate-ec-pub-pkcs1 ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS",
        "r4-gate-rsa-sec1 ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS",
        "r4-gate-pss-pkcs1 ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS",
        "r4-gate-fmt-buffer ERR_INVALID_ARG_VALUE",
        "r4-gate-fmt-undef ERR_INVALID_ARG_VALUE",
        "r4-gate-sec-pem ERR_INVALID_ARG_VALUE",
        "r4-gate-sec-ok true",
        "r4-raw-kind ERR_INVALID_ARG_VALUE",
        "r4-raw-rsa ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS",
        "r4-raw-noCurve ERR_INVALID_ARG_TYPE",
        "r4-raw-badCurve ERR_CRYPTO_INVALID_CURVE",
        "r4-raw-secp256r1 ERR_CRYPTO_INVALID_CURVE",
        "r4-raw-aktBanana ERR_INVALID_ARG_VALUE",
        "r4-raw-aktRsa ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS",
        "r4-raw-badPoint ERR_INVALID_ARG_VALUE",
        "r4-raw-compressed ERR_INVALID_ARG_VALUE",
        "r4-raw-wrongSize ERR_INVALID_ARG_VALUE",
        "r4-okp-badx ERR_CRYPTO_INVALID_JWK",
        "r4-okp-noD ERR_CRYPTO_INVALID_JWK",
        "r4-okp-noCrv ERR_CRYPTO_INVALID_JWK",
        "r4-okp-badCrv ERR_CRYPTO_INVALID_JWK",
        "r4-okp-badD ERR_CRYPTO_INVALID_JWK",
        "r4-okp-fromD ERR_CRYPTO_INVALID_JWK",
        "r4-okp-fromDxd true",
        "r4-okp-privD true",
        "r4-ec-jwkexp true",
        "r4-ec-badx ERR_CRYPTO_INVALID_JWK",
        "r4-ec-bady ERR_CRYPTO_INVALID_JWK",
        "r4-ec-noD ERR_CRYPTO_INVALID_JWK",
        "r4-ec-noCrv ERR_CRYPTO_INVALID_JWK",
        "r4-ec-badCrv ERR_CRYPTO_INVALID_CURVE",
        "r4-ec-badD ERR_CRYPTO_INVALID_JWK",
        "r4-ec-badPoint ERR_CRYPTO_INVALID_JWK",
        "r4-ec-priv-rt true",
        "r4-ec-pub-fromD true",
        "r4-dsa-jwkexp ERR_CRYPTO_JWK_UNSUPPORTED_KEY_TYPE",
        "r4-dsa-jwkimp ERR_CRYPTO_INVALID_JWK",
        "r4-dsa-details true true true true",
        "r4-done",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_crypto_raw_seed_parity() {
    // 10f crypto五轮：raw 加密门 + raw-seed（真机 26.8.2 对拍，
    // /tmp/wjs-raw-probe*.mjs 逐项）——导出 passphrase 门最前（仅 pem/der
    // 放行）、raw-seed 导入导出 INCOMPATIBLE、cipher 单给忽略。
    let dir = assert_fs::TempDir::new().unwrap();
    // slh 套件 fixture（test/fixtures/keys，MIT）落盘（§4.44）。
    dir.child("slh_pub.pem").write_str(
        "-----BEGIN PUBLIC KEY-----\nMDAwCwYJYIZIAWUDBAMVAyEApMCPV24BQ+l/NSWx/R3ybiV8fL2NYUVVGb+XNahP\ndco=\n-----END PUBLIC KEY-----\n",
    ).unwrap();
    dir.child("slh_priv.pem").write_str(
        "-----BEGIN PRIVATE KEY-----\nMFICAQAwCwYJYIZIAWUDBAMVBECSzBw9GGOCapA9uSDmWwzK5By75k4dJZt9GEv7\naWL4AaTAj1duAUPpfzUlsf0d8m4lfHy9jWFFVRm/lzWoT3XK\n-----END PRIVATE KEY-----\n",
    ).unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { generateKeyPairSync, createPrivateKey, createPublicKey, createSecretKey } from "node:crypto";
import { readFileSync } from "node:fs";
const log = (...a) => console.log(...a);
const throws = (fn) => { try { fn(); return "NO-THROW"; } catch (e) { return `${e.code}|${e.message}`; } };
const { privateKey: edPriv, publicKey: edPub } = generateKeyPairSync("ed25519");
const { privateKey: ecPriv } = generateKeyPairSync("ec", { namedCurve: "prime256v1" });
const rawPriv = edPriv.export({ format: "raw-private" });

// ── 正常件：raw 往返不受新门影响 ──
log("r5-raw-rt", Buffer.isBuffer(rawPriv) && rawPriv.length === 32,
  createPrivateKey({ key: rawPriv, format: "raw-private", asymmetricKeyType: "ed25519" }).equals(edPriv));

// ── 报错件：导出 passphrase 门（先于 kind/format 门）──
log("r5-raw-priv-pp", throws(() => edPriv.export({ format: "raw-private", passphrase: "test" })));
log("r5-raw-pub-pp", throws(() => edPriv.export({ format: "raw-public", passphrase: "test" })));
log("r5-raw-seed-pp", throws(() => edPriv.export({ format: "raw-seed", passphrase: "test" })));
log("r5-raw-seed", throws(() => edPriv.export({ format: "raw-seed" })));
log("r5-ec-raw-seed", throws(() => ecPriv.export({ format: "raw-seed" })));
log("r5-ec-raw-seed-type", throws(() => ecPriv.export({ format: "raw-seed", type: "banana" })));
log("r5-banana-pp", throws(() => edPriv.export({ format: "banana", passphrase: "x" })));
log("r5-undef-pp", throws(() => edPriv.export({ passphrase: "x" })));

// ── 报错件：导入 raw-seed（走 akt 链后 INCOMPATIBLE）──
log("r5-imp-seed", throws(() => createPrivateKey({ key: rawPriv, format: "raw-seed", asymmetricKeyType: "ed25519" })));
log("r5-imp-seed-ec", throws(() => createPrivateKey({
  key: ecPriv.export({ format: "raw-private" }), format: "raw-seed",
  asymmetricKeyType: "ec", namedCurve: "prime256v1" })));
log("r5-imp-seed-noakt", throws(() => createPrivateKey({ key: rawPriv, format: "raw-seed" })));
log("r5-imp-seed-badakt", throws(() => createPrivateKey({ key: rawPriv, format: "raw-seed", asymmetricKeyType: "banana" })));

// ── 边界件：cipher 单给忽略、公钥/secret 侧忽略 passphrase ──
log("r5-cipher-only", Buffer.isBuffer(edPriv.export({ format: "raw-private", cipher: "aes-256-cbc" })));
log("r5-jwk-cipher-only", typeof edPriv.export({ format: "jwk", cipher: "aes-256-cbc" }) === "object");
log("r5-sec-pp", Buffer.isBuffer(createSecretKey(Buffer.alloc(8)).export({ passphrase: "x" })));
log("r5-pub-pp", edPub.export({ format: "pem", type: "spki", passphrase: "x" }).startsWith("-----BEGIN PUBLIC KEY-----"));

// ── 报错件：raw 导入不收字符串（超 28 码点截前 25 + '...'；料随机，动态验回显）──
const hexAll = rawPriv.toString("hex");
const hex10 = hexAll.slice(0, 10);
const strPub = (key) => {
  try { createPublicKey({ key, encoding: "hex", format: "raw-public", asymmetricKeyType: "ed25519" }); return "NO-THROW"; }
  catch (e) { return e.code; }
};
const recvOk = (key, want) => {
  try { createPublicKey({ key, encoding: "hex", format: "raw-public", asymmetricKeyType: "ed25519" }); return "NO-THROW"; }
  catch (e) { return e.message.includes(`('${want}')`) ? "recv-ok" : "recv-bad"; }
};
log("r5-str-short", strPub(hex10), recvOk(hex10, hex10));
log("r5-str-long", strPub(hexAll), recvOk(hexAll, `${hexAll.slice(0, 25)}...`));
log("r5-str-seed", (() => {
  try { createPrivateKey({ key: hexAll, encoding: "hex", format: "raw-seed", asymmetricKeyType: "ed25519" }); return "NO-THROW"; }
  catch (e) { return e.code + " " + (e.message.includes(`('${hexAll.slice(0, 25)}...')`) ? "recv-ok" : "recv-bad"); }
})());

// ── 正常件：ml raw-public/seed 导出（尺寸即口径）──
const { publicKey: kemPub, privateKey: kemPriv } = generateKeyPairSync("ml-kem-768");
const { publicKey: dsaPub, privateKey: dsaPriv } = generateKeyPairSync("ml-dsa-44");
log("r5-ml-pub", kemPub.export({ format: "raw-public" }).length, dsaPub.export({ format: "raw-public" }).length);
log("r5-ml-seed", kemPriv.export({ format: "raw-seed" }).length, dsaPriv.export({ format: "raw-seed" }).length);
log("r5-ml-priv-noraw", throws(() => kemPriv.export({ format: "raw-private" })));
log("r5-ml-pub-kind", throws(() => kemPriv.export({ format: "raw-public" })));
// ml raw 导入：对尺寸过、错尺寸 ARG_VALUE、raw-private 不兼容、seed 往返
const kemRawPub = kemPub.export({ format: "raw-public" });
const kemSeed = kemPriv.export({ format: "raw-seed" });
log("r5-ml-imp-ok", createPublicKey({ key: kemRawPub, format: "raw-public", asymmetricKeyType: "ml-kem-768" }).type);
log("r5-ml-imp-badlen", throws(() => createPublicKey({ key: Buffer.alloc(800), format: "raw-public", asymmetricKeyType: "ml-kem-768" })));
log("r5-ml-imp-norawpriv", throws(() => createPrivateKey({ key: kemSeed, format: "raw-private", asymmetricKeyType: "ml-kem-768" })));
const kemSeedBack = createPrivateKey({ key: kemSeed, format: "raw-seed", asymmetricKeyType: "ml-kem-768" });
log("r5-ml-seed-rt", kemSeedBack.type, kemSeedBack.export({ format: "raw-seed" }).equals(kemSeed));
log("r5-ml-derive", createPublicKey({ key: kemSeed, format: "raw-seed", asymmetricKeyType: "ml-kem-768" }).type);

// ── 正常件：EC 压缩点（导出 33B + 导入解压往返，真机逐字节对拍）──
const { publicKey: p256Pub } = generateKeyPairSync("ec", { namedCurve: "prime256v1" });
const comp = p256Pub.export({ format: "raw-public", type: "compressed" });
log("r5-ec-comp-len", comp.length, comp[0] === 2 || comp[0] === 3);
log("r5-ec-comp-rt", createPublicKey({ key: comp, format: "raw-public", asymmetricKeyType: "ec", namedCurve: "P-256" }).equals(p256Pub));
log("r5-ec-uncomp", p256Pub.export({ format: "raw-public", type: "uncompressed" }).length);
log("r5-ec-comp-hybrid", throws(() => p256Pub.export({ format: "raw-public", type: "hybrid" })));
log("r5-ec-comp-badpre", throws(() => createPublicKey({
  key: Buffer.concat([Buffer.from([5]), comp.subarray(1)]), format: "raw-public",
  asymmetricKeyType: "ec", namedCurve: "P-256" })));
log("r5-ec-comp-wrongcurve", throws(() => createPublicKey({
  key: comp, format: "raw-public", asymmetricKeyType: "ec", namedCurve: "P-384" })));

// ── 正常件：slh 装载 + raw 尺寸（套件 fixture 内嵌落盘，§4.44）──
const slhPub = createPublicKey(readFileSync("slh_pub.pem", "ascii"));
const slhPriv = createPrivateKey(readFileSync("slh_priv.pem", "ascii"));
log("r5-slh-load", slhPub.type, slhPub.asymmetricKeyType, slhPub.export({ format: "raw-public" }).length);
log("r5-slh-priv", slhPriv.type, slhPriv.asymmetricKeyType, slhPriv.export({ format: "raw-private" }).length);
log("r5-slh-seed", throws(() => slhPriv.export({ format: "raw-seed" })));
log("r5-slh-rt", createPublicKey({
  key: slhPub.export({ format: "raw-public" }), format: "raw-public",
  asymmetricKeyType: "slh-dsa-sha2-128f" }).equals(slhPub));
log("r5-done");
"#,
    );
    for line in [
        "r5-raw-rt true true",
        "r5-raw-priv-pp ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS|The selected key encoding raw-private does not support encryption.",
        "r5-raw-pub-pp ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS|The selected key encoding raw-public does not support encryption.",
        "r5-raw-seed-pp ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS|The selected key encoding raw-seed does not support encryption.",
        "r5-raw-seed ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS|The selected key encoding is incompatible with the key type",
        "r5-ec-raw-seed ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS|The selected key encoding is incompatible with the key type",
        "r5-ec-raw-seed-type ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS|The selected key encoding is incompatible with the key type",
        "r5-banana-pp ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS|The selected key encoding banana does not support encryption.",
        "r5-undef-pp ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS|The selected key encoding undefined does not support encryption.",
        "r5-imp-seed ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS|The selected key encoding is incompatible with the key type",
        "r5-imp-seed-ec ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS|The selected key encoding is incompatible with the key type",
        "r5-imp-seed-noakt ERR_INVALID_ARG_TYPE|The \"key.asymmetricKeyType\" property must be of type string. Received undefined",
        "r5-imp-seed-badakt ERR_INVALID_ARG_VALUE|Invalid asymmetricKeyType: banana",
        "r5-cipher-only true",
        "r5-jwk-cipher-only true",
        "r5-sec-pp true",
        "r5-pub-pp true",
        "r5-str-short ERR_INVALID_ARG_TYPE recv-ok",
        "r5-str-long ERR_INVALID_ARG_TYPE recv-ok",
        "r5-str-seed ERR_INVALID_ARG_TYPE recv-ok",
        "r5-ml-pub 1184 1312",
        "r5-ml-seed 64 32",
        "r5-ml-priv-noraw ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS|The selected key encoding is incompatible with the key type",
        "r5-ml-pub-kind ERR_INVALID_ARG_VALUE|The property 'options.format' is invalid. Received 'raw-public'",
        "r5-ml-imp-ok public",
        "r5-ml-imp-badlen ERR_INVALID_ARG_VALUE|Invalid key data",
        "r5-ml-imp-norawpriv ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS|The selected key encoding is incompatible with the key type",
        "r5-ml-seed-rt private true",
        "r5-ml-derive public",
        "r5-ec-comp-len 33 true",
        "r5-ec-comp-rt true",
        "r5-ec-uncomp 65",
        "r5-ec-comp-hybrid ERR_INVALID_ARG_VALUE|The property 'options.type' must be one of: 'compressed', 'uncompressed'. Received 'hybrid'",
        "r5-ec-comp-badpre ERR_INVALID_ARG_VALUE|Invalid key data",
        "r5-ec-comp-wrongcurve ERR_INVALID_ARG_VALUE|Invalid key data",
        "r5-slh-load public slh-dsa-sha2-128f 32",
        "r5-slh-priv private slh-dsa-sha2-128f 64",
        "r5-slh-seed ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS|The selected key encoding is incompatible with the key type",
        "r5-slh-rt true",
        "r5-done",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_crypto_pss_gates() {
    // 10f crypto六轮：RSA-PSS 装载/约束/入口门（真机 26.8.2 对拍）。
    // 约束键（一次性 params）无 repo fixture，以生成键覆盖无约束面；
    // 约束执行/MGF 切换由 key-objects.js 套件 trace 覆盖（见 bun-parity）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { generateKeyPairSync, createPublicKey, createPrivateKey, createSign, createVerify, getCurves } from "node:crypto";
const log = (...a) => console.log(...a);
const throws = (fn) => { try { fn(); return "NO-THROW"; } catch (e) { return `${e.code}|${e.message}`; } };

// ── 装载面：生成 PSS 键无约束（details 精确形）+ 导出往返 ──
const { publicKey: pssPub, privateKey: pssPriv } = generateKeyPairSync("rsa-pss", { modulusLength: 1024 });
log("r6-pss-type", pssPub.asymmetricKeyType, pssPriv.asymmetricKeyType);
log("r6-pss-det", JSON.stringify(pssPub.asymmetricKeyDetails, (k, v) => typeof v === "bigint" ? "BIG" : v));
const spki = pssPub.export({ format: "der", type: "spki" });
const back = createPublicKey({ key: spki, format: "der", type: "spki" });
log("r6-pss-rt", back.asymmetricKeyType, back.equals(pssPub));
log("r6-pss-jwk", throws(() => pssPub.export({ format: "jwk" })));
log("r6-pss-pkcs1", throws(() => pssPub.export({ format: "pem", type: "pkcs1" })));
// PSS SHA-1 自签自验（sha1_010 底座）
const sig1 = createSign("sha1").update("foo").sign({ key: pssPriv, saltLength: 8 });
log("r6-pss-sha1", createVerify("sha1").update("foo").verify({ key: pssPub, saltLength: 8 }, sig1));

// ── 约束面：生成键 saltLength 选项即下限 ──
const { privateKey: lim } = generateKeyPairSync("rsa-pss", { modulusLength: 1024, saltLength: 20 });
log("r6-lim-small", throws(() => createSign("sha256").update("x").sign({ key: lim, saltLength: 8 })));
const sigD = createSign("sha256").update("x").sign(lim);
log("r6-lim-def", createVerify("sha256").update("x").verify(lim, sigD));

// ── 入口门：key.format/key.type/JWK key 形态 ──
log("r6-fmt", throws(() => createPrivateKey({ key: Buffer.alloc(0), format: "banana", type: "pkcs8" })));
log("r6-typ", throws(() => createPublicKey({ key: Buffer.alloc(0), format: "der", type: "banana" })));
log("r6-jwk-str", throws(() => createPublicKey({ key: "", format: "jwk" })));
log("r6-jwk-null", throws(() => createPrivateKey({ key: null, format: "jwk" })));
log("r6-curves", getCurves().join(","));
log("r6-gen-ec-okp", throws(() => generateKeyPairSync("ec", { namedCurve: "ed25519" })));

// ── 加密导出缺 cipher 门 ──
const { privateKey: rsa } = generateKeyPairSync("rsa", { modulusLength: 1024 });
log("r6-nocipher", throws(() => rsa.export({ format: "pem", type: "pkcs8", passphrase: "s" })));
log("r6-done");
"#,
    );
    for line in [
        "r6-pss-type rsa-pss rsa-pss",
        "r6-pss-det {\"modulusLength\":1024,\"publicExponent\":\"BIG\"}",
        "r6-pss-rt rsa-pss true",
        "r6-pss-jwk ERR_CRYPTO_JWK_UNSUPPORTED_KEY_TYPE|Unsupported JWK Key Type.",
        "r6-pss-pkcs1 ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS|The selected key encoding pkcs1 can only be used for RSA keys.",
        "r6-pss-sha1 true",
        "r6-lim-small ERR_OSSL_PSS_SALTLEN_TOO_SMALL|error:1C8000AC:Provider routines::pss saltlen too small",
        "r6-lim-def true",
        "r6-fmt ERR_INVALID_ARG_VALUE|The property 'key.format' is invalid. Received 'banana'",
        "r6-typ ERR_INVALID_ARG_VALUE|The property 'key.type' is invalid. Received 'banana'",
        "r6-jwk-str ERR_INVALID_ARG_TYPE|The \"key.key\" property must be of type object. Received type string ('')",
        "r6-jwk-null ERR_INVALID_ARG_TYPE|The \"key.key\" property must be of type object. Received null",
        "r6-curves prime256v1,secp384r1,secp521r1,secp256k1",
        "r6-gen-ec-okp ERR_CRYPTO_INVALID_CURVE|Invalid EC curve name",
        "r6-nocipher ERR_INVALID_ARG_VALUE|The property 'options.cipher' is required when a passphrase is specified. Received undefined",
        "r6-done",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}
