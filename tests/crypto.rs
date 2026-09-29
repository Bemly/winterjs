//! crypto 黑盒测试(对齐 src/builtins/crypto.rs:WebCrypto SubtleCrypto 全家)。

mod common;

use common::*;

#[test]
fn phase3_crypto_random() {
    let out = stdout_of(&mut winterjs2().args(["--eval",
        r#"const v = new Uint8Array(16); console.log(crypto.getRandomValues(v) === v, v.length); const a = crypto.randomUUID(), b = crypto.randomUUID(); console.log(a.length, a !== b, /^[0-9a-f-]{36}$/.test(a))"#]));
    assert_eq!(
        out,
        "true 16
36 true true
",
        "crypto: {out}"
    );
    let out = winterjs2()
        .args(["--eval", "crypto.getRandomValues(new Uint8Array(70000))"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
}

// ── Phase 3b：fetch / Headers / Request / Response ─────────────────────────

#[test]
fn phase3_subtle_digest_vectors() {
    let out = stdout_of(&mut winterjs2().args(["--eval",
        r#"const hex = async (a, d) => [...new Uint8Array(await crypto.subtle.digest(a, new TextEncoder().encode(d)))].map((b) => b.toString(16).padStart(2, "0")).join(""); console.log(await hex("SHA-256", "abc")); console.log(await hex("SHA-1", "abc"));"#]));
    assert_eq!(
        out,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\na9993e364706816aba3e25717850c26c9cd0d89d\n",
        "digest: {out}"
    );
}

#[test]
fn phase3_subtle_digest_unsupported() {
    let out = winterjs2()
        .args([
            "--eval",
            r#"await crypto.subtle.digest("MD5", new Uint8Array(1))"#,
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("NotSupportedError"), "stderr: {stderr}");
}

#[test]
fn phase3_aes_gcm_roundtrip() {
    let out = stdout_of(&mut winterjs2().args(["--eval",
        r#"const iv = new Uint8Array(12); const key = await crypto.subtle.generateKey({ name: "AES-GCM", length: 256 }, true, ["encrypt", "decrypt"]); const ct = await crypto.subtle.encrypt({ name: "AES-GCM", iv }, key, new TextEncoder().encode("secret")); const pt = await crypto.subtle.decrypt({ name: "AES-GCM", iv }, key, ct); console.log(ct.byteLength, new TextDecoder().decode(pt));"#]));
    assert_eq!(out, "22 secret\n", "aes: {out}");
}

#[test]
fn phase3_hmac_sign_verify() {
    let out = stdout_of(&mut winterjs2().args(["--eval",
        r#"const hk = await crypto.subtle.importKey("raw", new TextEncoder().encode("k"), { name: "HMAC", hash: "SHA-256" }, true, ["sign", "verify"]); const sig = await crypto.subtle.sign("HMAC", hk, new TextEncoder().encode("m")); console.log(new Uint8Array(sig).length, await crypto.subtle.verify("HMAC", hk, sig, new TextEncoder().encode("m")), await crypto.subtle.verify("HMAC", hk, sig, new TextEncoder().encode("x")), (await crypto.subtle.exportKey("jwk", hk)).kty);"#]));
    assert_eq!(out, "32 true false oct\n", "hmac: {out}");
}

#[test]
fn phase3_rsa_pkcs1v15_sign_verify() {
    let out = stdout_of(&mut winterjs2().args(["--eval",
        r#"const {publicKey, privateKey} = await crypto.subtle.generateKey({ name: "RSASSA-PKCS1-v1_5", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" }, true, ["sign", "verify"]); console.log(publicKey.type, privateKey.type, privateKey.algorithm.modulusLength); const sig = await crypto.subtle.sign("RSASSA-PKCS1-v1_5", privateKey, new TextEncoder().encode("m")); console.log(new Uint8Array(sig).length, await crypto.subtle.verify("RSASSA-PKCS1-v1_5", publicKey, sig, new TextEncoder().encode("m")), await crypto.subtle.verify("RSASSA-PKCS1-v1_5", publicKey, sig, new TextEncoder().encode("x")));"#]));
    assert_eq!(
        out, "public private 2048\n256 true false\n",
        "rsa-pkcs1v15: {out}"
    );
}

#[test]
fn phase3_rsa_oaep_roundtrip() {
    let out = stdout_of(&mut winterjs2().args(["--eval",
        r#"const {publicKey, privateKey} = await crypto.subtle.generateKey({ name: "RSA-OAEP", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" }, true, ["encrypt", "decrypt"]); const ct = await crypto.subtle.encrypt({ name: "RSA-OAEP" }, publicKey, new TextEncoder().encode("secret")); const pt = await crypto.subtle.decrypt({ name: "RSA-OAEP" }, privateKey, ct); console.log(ct.byteLength, new TextDecoder().decode(pt)); const spki = await crypto.subtle.exportKey("spki", publicKey); console.log(new Uint8Array(spki).length);"#]));
    assert_eq!(out, "256 secret\n294\n", "rsa-oaep: {out}");
}

#[test]
fn phase3_rsa_jwk_roundtrip() {
    // 私钥 JWK 来回（n/e/d 进，p/q 恢复）+ 公钥 JWK 进；签名跨导入验证。
    let out = stdout_of(&mut winterjs2().args(["--eval",
        r#"const {publicKey, privateKey} = await crypto.subtle.generateKey({ name: "RSASSA-PKCS1-v1_5", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" }, true, ["sign", "verify"]); const jwk = await crypto.subtle.exportKey("jwk", privateKey); console.log(jwk.kty, typeof jwk.dp, typeof jwk.qi); const priv2 = await crypto.subtle.importKey("jwk", jwk, { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" }, true, ["sign"]); console.log(priv2.type); const pubJwk = await crypto.subtle.exportKey("jwk", publicKey); const pub2 = await crypto.subtle.importKey("jwk", pubJwk, { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" }, true, ["verify"]); const sig = await crypto.subtle.sign("RSASSA-PKCS1-v1_5", priv2, new TextEncoder().encode("m")); console.log(await crypto.subtle.verify("RSASSA-PKCS1-v1_5", pub2, sig, new TextEncoder().encode("m")));"#]));
    assert_eq!(out, "RSA string string\nprivate\ntrue\n", "rsa-jwk: {out}");
}

#[test]
fn phase3_ecdsa_p256_roundtrip() {
    let out = stdout_of(&mut winterjs2().args(["--eval",
        r#"const {publicKey, privateKey} = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, ["sign", "verify"]); console.log(publicKey.type, privateKey.algorithm.namedCurve); const sig = await crypto.subtle.sign({ name: "ECDSA", hash: "SHA-256" }, privateKey, new TextEncoder().encode("hello")); console.log(new Uint8Array(sig).length, await crypto.subtle.verify({ name: "ECDSA", hash: "SHA-256" }, publicKey, sig, new TextEncoder().encode("hello")), await crypto.subtle.verify({ name: "ECDSA", hash: "SHA-256" }, publicKey, sig, new TextEncoder().encode("bye")));"#]));
    assert_eq!(out, "public P-256\n64 true false\n", "ecdsa: {out}");
}

#[test]
fn phase3_ecdh_derive_and_key() {
    // 共享秘密对称 + deriveKey 出 AES-GCM 可加解密；P-384 JWK/spki/raw 来回。
    let out = stdout_of(&mut winterjs2().args(["--eval",
        r#"const a = await crypto.subtle.generateKey({ name: "ECDH", namedCurve: "P-256" }, true, ["deriveBits", "deriveKey"]); const b = await crypto.subtle.generateKey({ name: "ECDH", namedCurve: "P-256" }, true, ["deriveBits"]); const s1 = new Uint8Array(await crypto.subtle.deriveBits({ name: "ECDH", public: b.publicKey }, a.privateKey, 256)); const s2 = new Uint8Array(await crypto.subtle.deriveBits({ name: "ECDH", public: a.publicKey }, b.privateKey, 256)); console.log(s1.length, s1.join(",") === s2.join(",")); const dk = await crypto.subtle.deriveKey({ name: "ECDH", public: b.publicKey }, a.privateKey, { name: "AES-GCM", length: 128 }, false, ["encrypt"]); console.log(dk.type, dk.algorithm.length, dk.extractable); const kp = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-384" }, true, ["sign", "verify"]); const raw = new Uint8Array(await crypto.subtle.exportKey("raw", kp.publicKey)); console.log(raw.length, raw[0]); const imp = await crypto.subtle.importKey("spki", await crypto.subtle.exportKey("spki", kp.publicKey), { name: "ECDSA", namedCurve: "P-384" }, true, ["verify"]); console.log(imp.type);"#]));
    assert_eq!(
        out, "32 true\nsecret 128 false\n97 4\npublic\n",
        "ecdh: {out}"
    );
}

#[test]
fn phase3_asymmetric_errors() {
    // c-4x 已收官：RSA-PSS 可生成（详见 subtle_c4x_*）；坏曲线/错用途/非私钥 derive 进报错面。
    let out = winterjs2()
        .args(["--eval", r#"const k = await crypto.subtle.generateKey({ name: "RSA-PSS", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" }, true, ["sign"]); console.log(k.publicKey.algorithm.name);"#])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "RSA-PSS\n");
    let out = winterjs2()
        .args(["--eval", r#"await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-192" }, true, ["sign"])"#])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("NotSupportedError"), "stderr: {stderr}");
    let ok = stdout_of(&mut winterjs2().args(["--eval",
        r#"const k = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, ["sign", "verify"]); try { await crypto.subtle.sign("ECDSA", k.publicKey, new Uint8Array(1)); console.log("no-throw"); } catch (e) { console.log(String(e).includes("private key") ? "sign-needs-private" : e); }"#]));
    assert_eq!(ok, "sign-needs-private\n", "usage: {ok}");
}

#[test]
fn subtle_c4x_ed25519_vectors() {
    // 正常：openssl 向量验签 + 签名回环 + pkcs8/spki/jwk 往返；报错：坏长度/坏签；
    // 边界：64B 全零签（合法长度，验签 false 不抛）。
    let code = format!(
        r#"{C4X_HEXJS}
const MSG = new TextEncoder().encode("winterjs2-vector");
const SEED = "b12d94858bb317baa5d40f669a784aa878bb17ad25e149e89594d7d9855b58a0";
const PUB = "a9a53ddffd0e9b2d2b83eb442fac6a95391d07160fe1926f51386d31e786c869";
const SIG = "541366402670fd6d20dbecfb6932e2cb5efe69dc92521d577ba70e355112b6ae40d0cb55a0f839d9b97b2841548edd4bf85da2665da35005bc7f6619463f800e";
const pub1 = await crypto.subtle.importKey("raw", bx(PUB), {{ name: "Ed25519" }}, true, ["verify"]);
const ok = await crypto.subtle.verify("Ed25519", pub1, bx(SIG), MSG);
if (ok !== true) throw new Error("openssl vector verify failed");
// 回环
const kp = await crypto.subtle.generateKey({{ name: "Ed25519" }}, true, ["sign", "verify"]);
const s2 = await crypto.subtle.sign("Ed25519", kp.privateKey, MSG);
if (await crypto.subtle.verify("Ed25519", kp.publicKey, s2, MSG) !== true) throw new Error("roundtrip failed");
const bad = new Uint8Array(s2); bad[0] ^= 1;
if (await crypto.subtle.verify("Ed25519", kp.publicKey, bad.buffer, MSG) !== false) throw new Error("tamper must be false");
const zero = await crypto.subtle.verify("Ed25519", kp.publicKey, new Uint8Array(64).buffer, MSG);
if (zero !== false) throw new Error("zero sig must be false");
// pkcs8/spki/jwk 往返
const skcs8 = await crypto.subtle.exportKey("pkcs8", kp.privateKey);
const k2 = await crypto.subtle.importKey("pkcs8", skcs8, {{ name: "Ed25519" }}, true, ["sign"]);
const s3 = await crypto.subtle.sign("Ed25519", k2, MSG);
if (await crypto.subtle.verify("Ed25519", kp.publicKey, s3, MSG) !== true) throw new Error("pkcs8 roundtrip failed");
const spki = await crypto.subtle.exportKey("spki", kp.publicKey);
const k3 = await crypto.subtle.importKey("spki", spki, {{ name: "Ed25519" }}, true, ["verify"]);
if (await crypto.subtle.verify("Ed25519", k3, s3, MSG) !== true) throw new Error("spki roundtrip failed");
const jwk = await crypto.subtle.exportKey("jwk", kp.privateKey);
if (jwk.kty !== "OKP" || jwk.crv !== "Ed25519" || jwk.alg !== "EdDSA" || typeof jwk.d !== "string") throw new Error("bad Ed JWK: " + JSON.stringify(jwk));
const k4 = await crypto.subtle.importKey("jwk", jwk, {{ name: "Ed25519" }}, true, ["sign"]);
if (await crypto.subtle.verify("Ed25519", kp.publicKey, await crypto.subtle.sign("Ed25519", k4, MSG), MSG) !== true) throw new Error("jwk roundtrip failed");
// 报错：raw 非 32B
try {{ await crypto.subtle.importKey("raw", new Uint8Array(31).buffer, {{ name: "Ed25519" }}, true, ["verify"]); throw new Error("must throw"); }}
catch (e) {{ if (!String(e.message).includes("32 bytes")) throw e; }}
// 报错：公钥验签用私钥对象
try {{ await crypto.subtle.verify("Ed25519", kp.privateKey, s2, MSG); throw new Error("must throw"); }}
catch (e) {{ if (!String(e.message).includes("public key")) throw e; }}
console.log("ed-ok");
"#
    );
    assert_eq!(
        stdout_of(&mut winterjs2().args(["--eval", &code])),
        "ed-ok\n"
    );
}

#[test]
fn subtle_c4x_x25519_vectors() {
    // 正常：openssl 向量 derive（双方一致）+ deriveKey 落 AES-GCM；报错：错对端类型；
    // 边界：deriveBits 长度越界。
    let code = format!(
        r#"{C4X_HEXJS}
const A_PRIV = "302e020100300506032b656e04220420a0e63ac582ee05d53337ba21c948389dc4e3bc0825fd506e2fa0719e038cc84d";
const B_PUB = "302a300506032b656e0321002c1c3ea839b4fb38c52c098df2af755e34cce1d2f657d8d58e3ec58529b56f73";
const EXPECT = "31526c245be4719dee9b1d1efe980c8ac796a6a2c6179a4ffe4ae018a3bc8763";
const hex = (b) => [...new Uint8Array(b)].map(x => x.toString(16).padStart(2, "0")).join("");
const pa = await crypto.subtle.importKey("pkcs8", bx(A_PRIV), {{ name: "X25519" }}, true, ["deriveBits"]);
const pb = await crypto.subtle.importKey("spki", bx(B_PUB), {{ name: "X25519" }}, true, []);
const bits = await crypto.subtle.deriveBits({{ name: "X25519", public: pb }}, pa, 256);
if (hex(bits) !== EXPECT) throw new Error("openssl vector derive failed: " + hex(bits));
// 自生成交换一致
const ka = await crypto.subtle.generateKey({{ name: "X25519" }}, true, ["deriveBits", "deriveKey"]);
const kb = await crypto.subtle.generateKey({{ name: "X25519" }}, true, ["deriveBits", "deriveKey"]);
const sab = hex(await crypto.subtle.deriveBits({{ name: "X25519", public: kb.publicKey }}, ka.privateKey, 256));
const sba = hex(await crypto.subtle.deriveBits({{ name: "X25519", public: ka.publicKey }}, kb.privateKey, 256));
if (sab !== sba) throw new Error("DH commutativity failed");
// deriveKey 落 AES-GCM 加解密
const aes = await crypto.subtle.deriveKey({{ name: "X25519", public: kb.publicKey }}, ka.privateKey, {{ name: "AES-GCM", length: 256 }}, false, ["encrypt", "decrypt"]);
const ct = await crypto.subtle.encrypt({{ name: "AES-GCM", iv: new Uint8Array(12) }}, aes, new TextEncoder().encode("x-secret"));
const pt = await crypto.subtle.decrypt({{ name: "AES-GCM", iv: new Uint8Array(12) }}, aes, ct);
if (new TextDecoder().decode(pt) !== "x-secret") throw new Error("derived AES failed");
// 报错：对端非 X25519 公钥
try {{ await crypto.subtle.deriveBits({{ name: "X25519", public: ka.privateKey }}, kb.privateKey, 256); throw new Error("must throw"); }}
catch (e) {{ if (!String(e.message).includes("public key")) throw e; }}
// 边界：长度越界
try {{ await crypto.subtle.deriveBits({{ name: "X25519", public: kb.publicKey }}, ka.privateKey, 257); throw new Error("must throw"); }}
catch (e) {{ if (!String(e.message).includes("length")) throw e; }}
// jwk 往返（X25519 无 alg，與 Node 一致省略）
const jwk = await crypto.subtle.exportKey("jwk", ka.publicKey);
if (jwk.kty !== "OKP" || jwk.crv !== "X25519" || "alg" in jwk) throw new Error("bad X JWK: " + JSON.stringify(jwk));
console.log("x-ok");
"#
    );
    assert_eq!(stdout_of(&mut winterjs2().args(["--eval", &code])), "x-ok\n");
}

#[test]
fn subtle_c4x_pss_roundtrip() {
    // 正常：生成→签名→验签 + 篡改/错 salt 为 false + jwk PS256；
    // 报错：公钥签名、私钥验签；边界：saltLength 缺省 = digest 长。
    let code = r#"const MSG = new TextEncoder().encode("pss-hello");
const kp = await crypto.subtle.generateKey({ name: "RSA-PSS", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" }, true, ["sign", "verify"]);
const sig = await crypto.subtle.sign({ name: "RSA-PSS", saltLength: 32 }, kp.privateKey, MSG);
if (await crypto.subtle.verify({ name: "RSA-PSS", saltLength: 32 }, kp.publicKey, sig, MSG) !== true) throw new Error("roundtrip failed");
// 缺省 salt（=32）与显式一致口径：交叉验签
const sig2 = await crypto.subtle.sign("RSA-PSS", kp.privateKey, MSG);
if (await crypto.subtle.verify("RSA-PSS", kp.publicKey, sig2, MSG) !== true) throw new Error("default salt failed");
const bad = new Uint8Array(sig); bad[bad.length - 1] ^= 1;
if (await crypto.subtle.verify({ name: "RSA-PSS", saltLength: 32 }, kp.publicKey, bad.buffer, MSG) !== false) throw new Error("tamper must be false");
if (await crypto.subtle.verify({ name: "RSA-PSS", saltLength: 20 }, kp.publicKey, sig, MSG) !== false) throw new Error("wrong salt must be false");
const jwk = await crypto.subtle.exportKey("jwk", kp.privateKey);
if (jwk.kty !== "RSA" || jwk.alg !== "PS256" || typeof jwk.d !== "string") throw new Error("bad PSS JWK");
const k2 = await crypto.subtle.importKey("jwk", jwk, { name: "RSA-PSS", hash: "SHA-256" }, true, ["sign"]);
if (await crypto.subtle.verify({ name: "RSA-PSS", saltLength: 32 }, kp.publicKey, await crypto.subtle.sign({ name: "RSA-PSS", saltLength: 32 }, k2, MSG), MSG) !== true) throw new Error("jwk roundtrip failed");
try { await crypto.subtle.sign({ name: "RSA-PSS", saltLength: 32 }, kp.publicKey, MSG); throw new Error("must throw"); }
catch (e) { if (!String(e.message).includes("private key")) throw e; }
console.log("pss-ok");
"#;
    assert_eq!(
        stdout_of(&mut winterjs2().args(["--eval", code])),
        "pss-ok\n"
    );
}

#[test]
fn subtle_c4x_aes192() {
    // 正常：192 回环 + raw 24B 导入；报错：20B；边界：192 派生（deriveKey 落 192）。
    let code = r#"const k192 = await crypto.subtle.generateKey({ name: "AES-GCM", length: 192 }, true, ["encrypt", "decrypt"]);
if (k192.algorithm.length !== 192) throw new Error("bad length");
const ct = await crypto.subtle.encrypt({ name: "AES-GCM", iv: new Uint8Array(12) }, k192, new TextEncoder().encode("topsecret"));
if (new TextDecoder().decode(await crypto.subtle.decrypt({ name: "AES-GCM", iv: new Uint8Array(12) }, k192, ct)) !== "topsecret") throw new Error("roundtrip failed");
const raw = await crypto.subtle.exportKey("raw", k192);
if (raw.byteLength !== 24) throw new Error("raw must be 24B");
const k2 = await crypto.subtle.importKey("raw", raw, "AES-GCM", true, ["decrypt"]);
if (new TextDecoder().decode(await crypto.subtle.decrypt({ name: "AES-GCM", iv: new Uint8Array(12) }, k2, ct)) !== "topsecret") throw new Error("raw import failed");
try { await crypto.subtle.importKey("raw", new Uint8Array(20).buffer, "AES-GCM", true, ["decrypt"]); throw new Error("must throw"); }
catch (e) { if (!String(e.message).includes("16/24/32")) throw e; }
try { await crypto.subtle.generateKey({ name: "AES-GCM", length: 100 }, true, ["encrypt"]); throw new Error("must throw"); }
catch (e) { if (!String(e.message).includes("128/192/256")) throw e; }
console.log("aes192-ok");
"#;
    assert_eq!(
        stdout_of(&mut winterjs2().args(["--eval", code])),
        "aes192-ok\n"
    );
}

// ── Web 流收官：TextDecoder 流式 / Abort 事件 / BYOB ──────────────────────────

/// c-4x 通用：hex 串转 ArrayBuffer（各用例内联，避免 helper 依赖）。
const C4X_HEXJS: &str =
    r#"const bx = (s) => new Uint8Array(s.match(/../g).map(h => parseInt(h, 16))).buffer;"#;
