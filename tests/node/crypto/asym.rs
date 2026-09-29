//! tests/node/crypto/asym.rs — 非对称/DH/EC/Ed448（对齐 src/builtins/node/crypto.rs）。

use crate::helpers::*;
use assert_fs::prelude::*;

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
try { generateKeyPairSync("dsa", {}); console.log("dsa", false); } catch (e) { console.log("dsa", e.code === "ERR_INVALID_ARG_TYPE"); }
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
fn phase10e_crypto_ed448() {
    // 10e Ed448：真机取证向量（确定性签名逐字节）+ 全链 + 报错/边界三件 + 证书
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("ed448-cert.pem")
        .write_str(include_str!("../../fixtures/ed448-cert.pem"))
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
// async 形态（真机口径：options 必给，二参省略即抛，见 keygen 77 行）
generateKeyPair("ed448", {}, (e, pub, priv) => {
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
fn p2_crypto_sign_verify_nonew() {
    // P2 crypto三件簇：Sign/Verify 无 new 调用形（legacy 函数口径；正常 + 报错）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import crypto from "node:crypto";
const S = crypto.Sign("SHA256");
console.log("sign-nonew", S instanceof crypto.Sign);
const V = crypto.Verify("SHA256");
console.log("verify-nonew", V instanceof crypto.Verify);
console.log("exported", typeof crypto.Sign === "function" && typeof crypto.Verify === "function");
try { crypto.Sign("nope-digest"); console.log("sig-alg FAIL"); }
catch (e) { console.log("sig-alg", e.code === "ERR_CRYPTO_INVALID_DIGEST"); }
"#,
    );
    assert!(out.contains("sign-nonew true"), "out: {out}");
    assert!(out.contains("verify-nonew true"), "out: {out}");
    assert!(out.contains("exported true"), "out: {out}");
    assert!(out.contains("sig-alg true"), "out: {out}");
    dir.close().unwrap();
}

