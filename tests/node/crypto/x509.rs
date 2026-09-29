//! tests/node/crypto/x509.rs — X509（对齐 src/builtins/node/crypto.rs）。

use crate::common::*;
use crate::helpers::*;
use assert_fs::prelude::*;

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
fn phase9i_x509_issued_privkey() {
    let dir = assert_fs::TempDir::new().unwrap();
    // openssl 3.6 链固件：CA（SKI/AKID/keyCertSign 齐）+ leaf + 同名不同钥 CA。
    dir.child("chain-leaf.pem").write_str(include_str!("../../fixtures/chain-leaf.pem")).unwrap();
    dir.child("chain-ca.pem").write_str(include_str!("../../fixtures/chain-ca.pem")).unwrap();
    dir.child("unrelated-ca.pem").write_str(include_str!("../../fixtures/unrelated-ca.pem")).unwrap();
    dir.child("chain-leaf.key").write_str(include_str!("../../fixtures/chain-leaf.key")).unwrap();
    dir.child("mldsa-cert.pem").write_str(include_str!("../../fixtures/mldsa-cert.pem")).unwrap();
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
    dir.child("pss.pem").write_str(include_str!("../../fixtures/pss.pem")).unwrap();
    dir.child("chain-leaf.pem").write_str(include_str!("../../fixtures/chain-leaf.pem")).unwrap();
    dir.child("chain-ca.pem").write_str(include_str!("../../fixtures/chain-ca.pem")).unwrap();
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
        "xp-self true true true",
        "xp-ec-still true",
        "xp-wrong true",
        "xp-cross true true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

