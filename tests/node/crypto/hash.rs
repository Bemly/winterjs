//! tests/node/crypto/hash.rs — 哈希/HMAC/随机/KDF/XOF（对齐 src/builtins/node/crypto.rs）。

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
    let out = winterjs2().arg("--run").arg(file.path()).current_dir(dir.path()).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    for tag in ["x-ripemd", "x-s128", "x-s256", "x-hmacri", "x-hmacshake", "x-copy", "x-badlen", "x-hashes", "x-dflt", "x-dflt128"] {
        assert!(stdout.contains(&format!("{tag} true")), "out: {stdout}");
    }
    // DEP0198 缺省警告走 stderr（真机同款）。
    assert!(String::from_utf8_lossy(&out.stderr).contains("DEP0198"), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    dir.close().unwrap();
}

