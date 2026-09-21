//! tests/node/crypto/parity.rs — 10f 对拍轮（对齐 src/builtins/node/crypto.rs）。

use crate::helpers::*;
use assert_fs::prelude::*;

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
