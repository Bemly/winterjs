//! crypto 回归向量（c4x 系；按属主分散前集中一处，见 §0.9 拆分）。

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::context::JSContext;
use mozjs::jsapi::JSObject;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;
use mozjs::typedarray::{CreateWith, TypedArray, Uint8};

use crate::jsapi_glue::{report_error, value_to_string, view_bytes, wrap_cx, Frame};


#[cfg(test)]
mod c4x_tests {
    use super::super::{
        der_ecdsa_sig_to_raw, der_tlv, ec_curve_name, okp_unwrap_pkcs8, okp_unwrap_spki,
        okp_wrap_pkcs8, okp_wrap_spki, rsa_pss_verify_manual, rsa_v15_verify_manual,
        x509_ecdsa_sig_hash, x509_hash_oid_name, x509_pss_params, x509_rsa_sig_hash,
        x509_tbs_bytes, x448_dh,
    };
    use super::super::common::{mgf1_with, SystemRng};

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    // openssl 生成的独立向量（ED_SEED/ED_PUB/XA_*，见 plan c-4x）。
    const ED_SEED: &str = "b12d94858bb317baa5d40f669a784aa878bb17ad25e149e89594d7d9855b58a0";
    const ED_PUB: &str = "a9a53ddffd0e9b2d2b83eb442fac6a95391d07160fe1926f51386d31e786c869";
    const ED_PKCS8: &str = "302e020100300506032b657004220420b12d94858bb317baa5d40f669a784aa878bb17ad25e149e89594d7d9855b58a0";
    const ED_SPKI: &str = "302a300506032b6570032100a9a53ddffd0e9b2d2b83eb442fac6a95391d07160fe1926f51386d31e786c869";
    const XA_PRIV: &str = "a0e63ac582ee05d53337ba21c948389dc4e3bc0825fd506e2fa0719e038cc84d";
    const XA_PUB: &str = "8751eff746df600cb3f29b4e608b76d4c7cf6f08311f294bc9d0160af4354936";
    const XA_PKCS8: &str = "302e020100300506032b656e04220420a0e63ac582ee05d53337ba21c948389dc4e3bc0825fd506e2fa0719e038cc84d";
    const XA_SPKI: &str = "302a300506032b656e0321008751eff746df600cb3f29b4e608b76d4c7cf6f08311f294bc9d0160af4354936";
    // 真机取证向量（node 26.8.2 fixture x448_*.pem 导出；SELF_DH 为真机
    // diffieHellman 产物同值）。
    const X4_SEED: &str = "b4c36dacefeaf1d958e98e0d0de493c4d4390800a41e23ef9ae92274f9cd22e5ff12472bc8b117b7fee2ea3e9864c2acad6c92d239526099";
    const X4_PKCS8: &str = "3046020100300506032b656f043a0438b4c36dacefeaf1d958e98e0d0de493c4d4390800a41e23ef9ae92274f9cd22e5ff12472bc8b117b7fee2ea3e9864c2acad6c92d239526099";
    const X4_PUB: &str = "8a81d21d5a53b3a84cbe0868b04243211edc785884dfe5dc7316ad8bae2839527b3568dfa3313b69edf53d721416ae9c55882f48ab0314f0";
    const X4_SPKI: &str = "3042300506032b656f0339008a81d21d5a53b3a84cbe0868b04243211edc785884dfe5dc7316ad8bae2839527b3568dfa3313b69edf53d721416ae9c55882f48ab0314f0";
    const X4_SELF_DH: &str = "a1a343b2b66655d0b5ebc67950baf8c892f9bf951b8f7da2a2b0d1e10ccb1f338cdbea38a74140ff74e3bf69f8860d5ca566b55de1091a09";

    #[test]
    fn okp_der_matches_openssl_fixtures() {
        assert_eq!(okp_wrap_pkcs8("Ed25519", &hex(ED_SEED)).unwrap(), hex(ED_PKCS8));
        assert_eq!(okp_wrap_spki("Ed25519", &hex(ED_PUB)).unwrap(), hex(ED_SPKI));
        assert_eq!(okp_unwrap_pkcs8("Ed25519", &hex(ED_PKCS8)).unwrap(), hex(ED_SEED).as_slice());
        assert_eq!(okp_unwrap_spki("Ed25519", &hex(ED_SPKI)).unwrap(), hex(ED_PUB).as_slice());
        assert_eq!(okp_wrap_pkcs8("X25519", &hex(XA_PRIV)).unwrap(), hex(XA_PKCS8));
        assert_eq!(okp_wrap_spki("X25519", &hex(XA_PUB)).unwrap(), hex(XA_SPKI));
        assert_eq!(okp_unwrap_pkcs8("X25519", &hex(XA_PKCS8)).unwrap(), hex(XA_PRIV).as_slice());
        assert_eq!(okp_unwrap_spki("X25519", &hex(XA_SPKI)).unwrap(), hex(XA_PUB).as_slice());
        // 10f X448（node 26.8.2 fixture x448_*.pem 导出的 DER，56B 档）。
        assert_eq!(okp_wrap_pkcs8("X448", &hex(X4_SEED)).unwrap(), hex(X4_PKCS8));
        assert_eq!(okp_wrap_spki("X448", &hex(X4_PUB)).unwrap(), hex(X4_SPKI));
        assert_eq!(okp_unwrap_pkcs8("X448", &hex(X4_PKCS8)).unwrap(), hex(X4_SEED).as_slice());
        assert_eq!(okp_unwrap_spki("X448", &hex(X4_SPKI)).unwrap(), hex(X4_PUB).as_slice());
    }

    #[test]
    fn x448_dh_cross_checks() {
        // RFC 7748 §5.2 双 X448 DH（真机 26.8.2 diffieHellman 产物同值）。
        let a = x448_dh(&hex(X4_SEED), &hex(X4_PUB)).unwrap();
        assert_eq!(a, hex(X4_SELF_DH).as_slice());
        // 低阶点（全零 u）→ None（RFC 7748 拒收；node 口径 FAILED_DURING_DERIVATION）
        assert!(x448_dh(&hex(X4_SEED), &[0u8; 56]).is_none());
    }

    #[test]
    fn okp_der_rejects_wrong_oid_and_truncation() {
        // Ed 的 DER 喂给 X 解析：OID 对不上即 DataError
        assert!(okp_unwrap_pkcs8("X25519", &hex(ED_PKCS8)).is_err());
        assert!(okp_unwrap_spki("X25519", &hex(ED_SPKI)).is_err());
        // 截断/未知 kind
        assert!(okp_unwrap_pkcs8("Ed25519", &hex(ED_PKCS8)[..40]).is_err());
        assert!(okp_unwrap_spki("Ed25519", &hex(ED_SPKI)[..40]).is_err());
        assert!(okp_wrap_pkcs8("ED448", &[0u8; 32]).is_err());
        assert!(okp_wrap_pkcs8("Ed25519", &[0u8; 31]).is_err());
    }

    // 真机取证向量（Ed448 seed=01‖42×56；SPKI 69B/PKCS#8 73B 逐字节对；
    // SIG 为 "determinism-check" 的确定性签名，真机同值）。
    const E448_SEED: &str = "014242424242424242424242424242424242424242424242424242424242424242424242424242424242424242424242424242424242424242";
    const E448_PUB: &str = "75cb897328ba2e61dcba2a4e0d9c496594d55978478e3d7d865b8834b02b8230f9aae29cddd7f4f01ad6cedead3bdd0708c0f8297686d0c000";
    const E448_SPKI: &str = "3043300506032b6571033a0075cb897328ba2e61dcba2a4e0d9c496594d55978478e3d7d865b8834b02b8230f9aae29cddd7f4f01ad6cedead3bdd0708c0f8297686d0c000";
    const E448_PKCS8: &str = "3047020100300506032b6571043b0439014242424242424242424242424242424242424242424242424242424242424242424242424242424242424242424242424242424242424242";
    const E448_SIG: &str = "390f63c4e8ccaa3e9dce99084c5a8716caf1be49eeb40e452cec29a576f4dcec6fef3a39fd44da6d561277738e75acc162ef69e846230571802e93bcb4966519c15a1c1417adfb1bb70a8c87a1e873843cc1afbdbcf87442839b190f5a45ac21600122c2fd6ddb81b5f1bc3b36843c410400";

    #[test]
    fn okp_der_ed448_matches_node_fixtures() {
        assert_eq!(okp_wrap_pkcs8("ED448", &hex(E448_SEED)).unwrap(), hex(E448_PKCS8));
        assert_eq!(okp_wrap_spki("ED448", &hex(E448_PUB)).unwrap(), hex(E448_SPKI));
        assert_eq!(okp_unwrap_pkcs8("ED448", &hex(E448_PKCS8)).unwrap(), hex(E448_SEED).as_slice());
        assert_eq!(okp_unwrap_spki("ED448", &hex(E448_SPKI)).unwrap(), hex(E448_PUB).as_slice());
        // 异族互斥（48/73B 长度即分水岭）
        assert!(okp_unwrap_pkcs8("Ed25519", &hex(E448_PKCS8)).is_err());
        assert!(okp_unwrap_spki("ED448", &hex(ED_SPKI)).is_err());
        // 签名确定性（同种子同消息，真机同值）+ 验签自洽
        let sk = ed448_goldilocks::SigningKey::try_from(hex(E448_SEED).as_slice()).unwrap();
        let sig = sk.sign_raw(b"determinism-check");
        assert_eq!(const_hex::encode(sig.to_bytes()), E448_SIG);
        let vk = sk.verifying_key();
        assert!(vk.verify_raw(&sig, b"determinism-check").is_ok());
        assert!(vk.verify_raw(&sig, b"tampered").is_err());
    }

    // openssl 实测 DER（P-256 SPKI/PKCS8 + secp256k1 SPKI；9h-1 OID 直判回归）。
    const P256_SPKI: &str = "3059301306072a8648ce3d020106082a8648ce3d03010703420004934652ada5371695be1ebf30c3cb0d895f08d56bacf65704d30fa0d57c2df7d92c5abb658d19cf89cf70458b35649b30e1178c4e3e5991b1d5cea92aca090b25";
    const P256_PKCS8: &str = "308187020100301306072a8648ce3d020106082a8648ce3d030107046d306b0201010420bc6225513217d5896a52275273c3f96641e446fb8abe6d98f260dd719910712ba14403420004dd0884d23cdff883f5ce6cf98a51dbb48c578868b9daf1f67e79740279be14ccbc39ea21b3352be24d5c14b29c0523bb1e489e21abf0993286f8314471ce1c18";
    const K256_SPKI: &str = "3056301006072a8648ce3d020106052b8104000a034200047919bb26319bdf32b776c2b622c1cf49c14131d658aea027a5ebe069f6bf955cba294bd12bc5f30123a9e91598435cafae0952237601335f0f3d33f8fce4baf9";

    #[test]
    fn ec_curve_name_reads_oid_not_coords() {
        assert_eq!(ec_curve_name(&hex(P256_SPKI)), "P-256");
        assert_eq!(ec_curve_name(&hex(P256_PKCS8)), "P-256");
        // 同 32 字节坐标：试解会误判 P-256，OID 直判必须给 secp256k1。
        assert_eq!(ec_curve_name(&hex(K256_SPKI)), "secp256k1");
        assert_eq!(ec_curve_name(&hex(ED_SPKI)), "");
        assert_eq!(ec_curve_name(&[0u8; 10]), "");
    }

    // ── 9i-3 X.509 验签底座 ────────────────────────────────────────────────

    #[test]
    fn x509_der_tlv_shapes() {
        // 短形：04 05 <5B>
        assert_eq!(der_tlv(&[0x04, 0x05, 0, 0, 0, 0, 0]), Some((0x04, 2, 5)));
        // 长形：30 82 01 00 → 256B
        let mut long = vec![0x30, 0x82, 0x01, 0x00];
        long.extend(std::iter::repeat_n(0u8, 256));
        assert_eq!(der_tlv(&long), Some((0x30, 4, 256)));
        // 截断 / indefinite / 超长字段数
        assert_eq!(der_tlv(&[0x30, 0x05, 0]), None);
        assert_eq!(der_tlv(&[0x30, 0x80]), None);
        assert_eq!(der_tlv(&[0x30]), None);
        assert_eq!(der_tlv(&[]), None);
    }

    #[test]
    fn x509_tbs_span_exact() {
        // 手搭证书：SEQ{ SEQ{INT 1}, SEQ{OID}, BITSTRING }——TBS 裸段必须逐字节还原
        let tbs_body = [0x02u8, 0x01, 0x01, 0x0c, 0x03, b'a', b'b', b'c'];
        let mut tbs = vec![0x30, tbs_body.len() as u8];
        tbs.extend_from_slice(&tbs_body);
        let alg = [0x30u8, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70];
        let sig = [0x03u8, 0x02, 0x00, 0xAA];
        let mut body = tbs.clone();
        body.extend_from_slice(&alg);
        body.extend_from_slice(&sig);
        let mut der = vec![0x30, body.len() as u8];
        der.extend_from_slice(&body);
        assert_eq!(x509_tbs_bytes(&der), Some(tbs.as_slice()));
        // 非 SEQ 首件 / 截断
        assert_eq!(x509_tbs_bytes(&[0x04, 0x00]), None);
        assert_eq!(x509_tbs_bytes(&der[..der.len() - 1]), None);
    }

    #[test]
    fn x509_digestinfo_prefix_known_bytes() {
        // RFC 8017 §9.2 注记值（标准 DigestInfo 前缀）
        assert_eq!(
            x509_rsa_sig_hash("1.2.840.113549.1.1.4").unwrap().1,
            &const_hex::decode("3020300c06082a864886f70d020505000410").unwrap()[..]
        );
        assert_eq!(
            x509_rsa_sig_hash("1.2.840.113549.1.1.5").unwrap().1,
            &const_hex::decode("3021300906052b0e03021a05000414").unwrap()[..]
        );
        assert_eq!(
            x509_rsa_sig_hash("1.2.840.113549.1.1.11").unwrap().1,
            &const_hex::decode("3031300d060960864801650304020105000420").unwrap()[..]
        );
        // PSS/未知 → None
        assert!(x509_rsa_sig_hash("1.2.840.113549.1.1.10").is_none());
        assert!(x509_rsa_sig_hash("nope").is_none());
        assert_eq!(x509_ecdsa_sig_hash("1.2.840.10045.4.3.2"), Some("SHA-256"));
        assert_eq!(x509_ecdsa_sig_hash("1.2.840.10045.4.3.9"), None);
    }

    #[test]
    fn x509_rsa_v15_manual_matches_rsa_crate() {
        // rsa crate 自签（sha2_010, digest 0.10）⇄ 手工 EMSA 验签（sha2 0.11 直算）交叉
        use rsa::signature::Signer as _;
        use sha2::Digest as _;
        let mut key_bytes = [0u8; 32];
        getrandom::fill(&mut key_bytes).unwrap();
        let key = rsa::RsaPrivateKey::new(&mut SystemRng, 2048).expect("keygen");
        let data = b"tbs-bytes-for-manual-verify";
        let sig = Box::<[u8]>::from(
            rsa::pkcs1v15::SigningKey::<sha2_010::Sha256>::new(key.clone()).sign(data),
        )
        .into_vec();
        let digest = sha2::Sha256::digest(data).to_vec();
        let (_, prefix) = x509_rsa_sig_hash("1.2.840.113549.1.1.11").unwrap();
        let pub_key = key.to_public_key();
        assert!(rsa_v15_verify_manual(&pub_key, prefix, &digest, &sig));
        // 篡改签名 / 篡改摘要 / 短签名 → false
        let mut bad = sig.clone();
        bad[10] ^= 0xFF;
        assert!(!rsa_v15_verify_manual(&pub_key, prefix, &digest, &bad));
        assert!(!rsa_v15_verify_manual(&pub_key, prefix, &vec![0u8; 32], &sig));
        assert!(!rsa_v15_verify_manual(&pub_key, prefix, &digest, &sig[..sig.len() - 1]));
    }

    #[test]
    fn x509_mgf1_and_hash_oid_table() {
        // MGF1-SHA1("test", 20) = SHA1("test" ‖ 0x00000000)（node/crypto.rs 同向量）
        assert_eq!(
            mgf1_with("SHA-1", b"test", 20).unwrap(),
            const_hex::decode("b67344dc7dea343795faaba3bc4d4508bf6766b1").unwrap()
        );
        assert_eq!(mgf1_with("SHA-256", b"x", 52).unwrap().len(), 52);
        assert!(mgf1_with("nope", b"x", 4).is_err());
        assert_eq!(x509_hash_oid_name(&[0x2b, 0x0e, 0x03, 0x02, 0x1a]), Some("SHA-1"));
        assert_eq!(x509_hash_oid_name(&[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01]), Some("SHA-256"));
        assert_eq!(x509_hash_oid_name(&[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x08]), None);
    }

    #[test]
    fn x509_pss_params_parse() {
        let tlv = |tag: u8, body: &[u8]| -> Vec<u8> {
            let mut out = vec![tag, body.len() as u8];
            out.extend_from_slice(body);
            out
        };
        let sha256_oid = [0x60u8, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01];
        let mgf1_oid = [0x2au8, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x08];
        let hash_alg = tlv(0x30, &[tlv(0x06, &sha256_oid), tlv(0x05, &[])].concat());
        // openssl 形：[0] hash、[1] mgf1(sha256)、[2] salt（EXPLICIT INTEGER）
        let params_body = [
            tlv(0xa0, &hash_alg),
            tlv(0xa1, &tlv(0x30, &[tlv(0x06, &mgf1_oid), hash_alg.clone()].concat())),
            tlv(0xa2, &tlv(0x02, &[32])),
        ]
        .concat();
        use der::Decode as _;
        let any = der::Any::from_der(&tlv(0x30, &params_body)).unwrap();
        assert_eq!(x509_pss_params(&any).unwrap(), ("SHA-256", "SHA-256", 32));
        // IMPLICIT 原生 salt 形（0x82）也收
        let params_body2 = [tlv(0xa0, &hash_alg.clone()), tlv(0x82, &[20])].concat();
        let any2 = der::Any::from_der(&tlv(0x30, &params_body2)).unwrap();
        assert_eq!(x509_pss_params(&any2).unwrap(), ("SHA-256", "SHA-1", 20));
        // 空参 → 全缺省（sha1/mgf1-sha1/20）
        let any3 = der::Any::from_der(&tlv(0x30, &[])).unwrap();
        assert_eq!(x509_pss_params(&any3).unwrap(), ("SHA-1", "SHA-1", 20));
    }

    #[test]
    fn x509_pss_manual_vs_rsa_crate() {
        // rsa crate PSS 自签（sha2_010, salt=hLen）⇄ 手工 EMSA-PSS 验签（sha2 0.11 直算）
        use rsa::signature::{RandomizedSigner as _, SignatureEncoding as _};
        use sha2::Digest as _;
        let key = rsa::RsaPrivateKey::new(&mut SystemRng, 2048).expect("keygen");
        let data = b"pss-manual-verify";
        let sk = rsa::pss::SigningKey::<sha2_010::Sha256>::new(key.clone());
        let sig = sk
            .try_sign_with_rng(&mut SystemRng, data)
            .expect("pss sign")
            .to_vec();
        let m_hash = sha2::Sha256::digest(data).to_vec();
        let pub_key = key.to_public_key();
        assert!(rsa_pss_verify_manual(&pub_key, "SHA-256", "SHA-256", 32, &m_hash, &sig));
        let mut bad = sig.clone();
        bad[7] ^= 0xff;
        assert!(!rsa_pss_verify_manual(&pub_key, "SHA-256", "SHA-256", 32, &m_hash, &bad));
        assert!(!rsa_pss_verify_manual(&pub_key, "SHA-256", "SHA-256", 31, &m_hash, &sig));
        assert!(!rsa_pss_verify_manual(&pub_key, "SHA-256", "SHA-1", 32, &m_hash, &sig));
    }

    #[test]
    fn x509_der_ecdsa_sig_to_raw_shapes() {
        // SEQ{INT 1, INT 0xdeadbeef(高字节非零)} → 32B 定长左补零拼接
        let der = [0x30u8, 0x0a, 0x02, 0x01, 0x01, 0x02, 0x05, 0x00, 0xde, 0xad, 0xbe, 0xef];
        let raw = der_ecdsa_sig_to_raw(&der, 32).unwrap();
        assert_eq!(raw.len(), 64);
        assert!(raw[..31].iter().all(|&b| b == 0));
        assert_eq!(raw[31], 1);
        assert_eq!(&raw[60..], &[0xde, 0xad, 0xbe, 0xef]);
        // 前导零剥除 / 非法形 / 空整数
        let der0 = [0x30u8, 0x07, 0x02, 0x02, 0x00, 0x01, 0x02, 0x01, 0x02];
        let raw0 = der_ecdsa_sig_to_raw(&der0, 2).unwrap();
        assert_eq!(raw0, vec![0, 1, 0, 2]);
        assert!(der_ecdsa_sig_to_raw(&[0x04, 0x00], 32).is_none());
        assert!(der_ecdsa_sig_to_raw(&[0x30, 0x02, 0x04, 0x00], 32).is_none());
        assert!(der_ecdsa_sig_to_raw(&[0x30, 0x00], 32).is_none());
    }
}
