//! tests/node/crypto/pq.rs — ML-KEM/ML-DSA（对齐 src/builtins/node/crypto.rs）。

use crate::common::*;
use crate::helpers::*;
use assert_fs::prelude::*;

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
    let fixture = include_str!("../../fixtures/mldsa-node-fixture.b64");
    let mut lines = fixture.lines();
    let node_pkcs8_b64 = lines.next().unwrap().trim();
    let node_sig_b64 = lines.next().unwrap().trim();
    let node_cert_pem = include_str!("../../fixtures/mldsa-cert.pem");
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
        winterjs2()
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

