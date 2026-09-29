//! tests/node/crypto/cipher.rs — 对称/CCM/GCM/round1（对齐 src/builtins/node/crypto.rs）。

use crate::helpers::*;

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
fn p2_crypto_cipherinfo_nid_options() {
    // P2 crypto三件簇：getCipherInfo nid 形态 + options 校验/过滤 + ocb 元数据
    //（正常 + 报错 + 边界；node internal/crypto/cipher.js 口径）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { getCipherInfo } from "node:crypto";
const eq = (a, b) => JSON.stringify(a) === JSON.stringify(b);
// 正常：nid 往返与名查同构
const info = getCipherInfo("aes-128-cbc");
console.log("nid-rt", eq(info, getCipherInfo(419)));
console.log("nid-192", getCipherInfo(899).keyLength === 24);
console.log("nounk", getCipherInfo("nope") === undefined && getCipherInfo(-1) === undefined && getCipherInfo("") === undefined);
// 边界：长短错配回 undefined（ccm/ocb 可变窗）
console.log("kl-ok", !!getCipherInfo("aes-128-cbc", { keyLength: 16 }));
console.log("kl-bad", getCipherInfo("aes-128-cbc", { keyLength: 12 }) === undefined);
console.log("iv-ok", !!getCipherInfo("aes-128-cbc", { ivLength: 16 }));
console.log("iv-bad", getCipherInfo("aes-128-cbc", { ivLength: 12 }) === undefined);
console.log("ccm-win", !!getCipherInfo("aes-128-ccm", { ivLength: 7 }) && !!getCipherInfo("aes-128-ccm", { ivLength: 13 }));
console.log("ccm-out", getCipherInfo("aes-128-ccm", { ivLength: 1 }) === undefined);
console.log("ocb", getCipherInfo("aes-128-ocb").nid === 958 && !!getCipherInfo("aes-128-ocb", { ivLength: 15 }) && getCipherInfo("aes-128-ocb", { ivLength: 16 }) === undefined);
console.log("ecb-noiv", getCipherInfo("aes-128-ecb").ivLength === undefined);
// 报错：非串非数 / 非对象 options / 非 uint32 长
for (const bad of [null, undefined, [], {}]) {
  try { getCipherInfo(bad); console.log("noname FAIL", JSON.stringify(bad)); }
  catch (e) { console.log("noname", e.code === "ERR_INVALID_ARG_TYPE"); }
}
for (const opt of [null, "", 1, true]) {
  try { getCipherInfo("aes-192-cbc", opt); console.log("opt FAIL"); }
  catch (e) { console.log("opt", e.code === "ERR_INVALID_ARG_TYPE"); }
}
for (const len of [null, "", {}, [], true]) {
  try { getCipherInfo("aes-192-cbc", { keyLength: len }); console.log("len FAIL"); }
  catch (e) { console.log("len", e.code === "ERR_INVALID_ARG_TYPE"); }
}
"#,
    );
    assert!(out.contains("nid-rt true"), "out: {out}");
    assert!(out.contains("nid-192 true"), "out: {out}");
    assert!(out.contains("nounk true"), "out: {out}");
    assert!(out.contains("kl-ok true"), "out: {out}");
    assert!(out.contains("kl-bad true"), "out: {out}");
    assert!(out.contains("iv-ok true"), "out: {out}");
    assert!(out.contains("iv-bad true"), "out: {out}");
    assert!(out.contains("ccm-win true"), "out: {out}");
    assert!(out.contains("ccm-out true"), "out: {out}");
    assert!(out.contains("ocb true"), "out: {out}");
    assert!(out.contains("ecb-noiv true"), "out: {out}");
    assert!(!out.contains("FAIL"), "out: {out}");
    assert_eq!(out.matches("noname true").count(), 4, "out: {out}");
    assert_eq!(out.matches("opt true").count(), 4, "out: {out}");
    assert_eq!(out.matches("len true").count(), 5, "out: {out}");
    dir.close().unwrap();
}

#[test]
fn p2_crypto_cipher_setautopadding() {
    // P2 crypto MISSING-EXCEPTION 轮：setAutoPadding 透传 + CbcEnc autopad +
    // OSSL 错误 reason/码形 + GCM tag 长校验 + generateKey/keypair 头检
    //（正常 + 报错 + 边界；node 真机 26.8.2 口径逐项实测）。
    // UNSAFE-BOUNDARY 覆盖：`__wjs2_cipher_set_autopad`（前置见定义注释）——
    // panic 路径经公开 API 触发（double-final → ERR_CRYPTO_INVALID_STATE）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { createCipheriv, createDecipheriv, generateKeySync, generateKeyPairSync } from "node:crypto";
const key = Buffer.alloc(32, 1), iv16 = Buffer.alloc(16, 2), iv12 = Buffer.alloc(12, 3);
// 正常：默认填充回环
{
  const e = createCipheriv("aes-256-cbc", key, iv16);
  const ct = Buffer.concat([e.update("hello world"), e.final()]);
  const d = createDecipheriv("aes-256-cbc", key, iv16);
  console.log("pad-rt", d.update(ct).toString() + d.final("utf8") === "hello world");
}
// 正常：构造后 setAutoPadding(false) + 整块回环（本轮根因：旧实现空转）
{
  const e = createCipheriv("aes-256-cbc", key, iv16);
  e.setAutoPadding(false);
  const pt = Buffer.alloc(32, 7);
  const ct = Buffer.concat([e.update(pt), e.final()]);
  const d = createDecipheriv("aes-256-cbc", key, iv16);
  d.setAutoPadding(false);
  console.log("nopad-rt", Buffer.concat([d.update(ct), d.final()]).equals(pt));
}
// 报错：enc 无填充非整块 → WRONG_FINAL_BLOCK_LENGTH（三件：message/code/reason）
{
  const e = createCipheriv("aes-256-cbc", key, iv16);
  e.setAutoPadding(false);
  e.update(Buffer.alloc(10));
  try { e.final(); console.log("enc-nopad FAIL"); }
  catch (err) { console.log("enc-nopad", err.code === "ERR_OSSL_WRONG_FINAL_BLOCK_LENGTH" && /wrong final block length/i.test(err.message) && /wrong final block length/i.test(err.reason)); }
}
// 报错：dec 坏填充 → BAD_DECRYPT（与长度错区分）
{
  const d = createDecipheriv("aes-256-cbc", key, iv16);
  d.update(Buffer.alloc(32, 9));
  try { d.final(); console.log("dec-badpad FAIL"); }
  catch (err) { console.log("dec-badpad", err.code === "ERR_OSSL_BAD_DECRYPT" && /bad decrypt/i.test(err.message)); }
}
// 报错：GCM 短 tag 无选项即 set → INVALID_AUTH_TAG；非法 tagLen 选项 → 同码
{
  const d = createDecipheriv("aes-256-gcm", key, iv12);
  try { d.setAuthTag(Buffer.alloc(12)); console.log("gcm-tag FAIL"); }
  catch (err) { console.log("gcm-tag", err.code === "ERR_CRYPTO_INVALID_AUTH_TAG"); }
  try { createDecipheriv("aes-256-gcm", key, iv12, { authTagLength: 17 }); console.log("gcm-len FAIL"); }
  catch (err) { console.log("gcm-len", err.code === "ERR_CRYPTO_INVALID_AUTH_TAG"); }
  try { createCipheriv("aes-256-gcm", key, iv12, { authTagLength: 11 }); console.log("gcm-enc-len FAIL"); }
  catch (err) { console.log("gcm-enc-len", err.code === "ERR_CRYPTO_INVALID_AUTH_TAG"); }
}
// 报错：generateKey 头检（type 非串 / options 非对象）
{
  try { generateKeySync(1, 1); console.log("gk-type FAIL"); }
  catch (err) { console.log("gk-type", err.code === "ERR_INVALID_ARG_TYPE"); }
  try { generateKeySync("aes", []); console.log("gk-opt FAIL"); }
  catch (err) { console.log("gk-opt", err.code === "ERR_INVALID_ARG_TYPE"); }
  const k = generateKeySync("hmac", { length: 123 });
  console.log("gk-hmac", k.export().byteLength === 15);
}
// 报错：keypair 头检（未知串 / 非串 type）
{
  try { generateKeyPairSync("rsa2", {}); console.log("kp-type FAIL"); }
  catch (err) { console.log("kp-type", err.code === "ERR_INVALID_ARG_VALUE"); }
  try { generateKeyPairSync(0, {}); console.log("kp-nonstr FAIL"); }
  catch (err) { console.log("kp-nonstr", err.code === "ERR_INVALID_ARG_TYPE"); }
}
// 边界：final 后 setAutoPadding → INVALID_STATE；double-final → INVALID_STATE（panic 路径）
{
  const e = createCipheriv("aes-256-cbc", key, iv16);
  e.final();
  try { e.setAutoPadding(true); console.log("late-autopad FAIL"); }
  catch (err) { console.log("late-autopad", err.code === "ERR_CRYPTO_INVALID_STATE"); }
  try { e.final(); console.log("dbl-final FAIL"); }
  catch (err) { console.log("dbl-final", err.code === "ERR_CRYPTO_INVALID_STATE"); }
}
"#,
    );
    assert!(out.contains("pad-rt true"), "out: {out}");
    assert!(out.contains("nopad-rt true"), "out: {out}");
    assert!(out.contains("enc-nopad true"), "out: {out}");
    assert!(out.contains("dec-badpad true"), "out: {out}");
    assert!(out.contains("gcm-tag true"), "out: {out}");
    assert!(out.contains("gcm-len true"), "out: {out}");
    assert!(out.contains("gcm-enc-len true"), "out: {out}");
    assert!(out.contains("gk-type true"), "out: {out}");
    assert!(out.contains("gk-opt true"), "out: {out}");
    assert!(out.contains("gk-hmac true"), "out: {out}");
    assert!(out.contains("kp-type true"), "out: {out}");
    assert!(out.contains("kp-nonstr true"), "out: {out}");
    assert!(out.contains("late-autopad true"), "out: {out}");
    assert!(out.contains("dbl-final true"), "out: {out}");
    assert!(!out.contains("FAIL"), "out: {out}");
    dir.close().unwrap();
}

