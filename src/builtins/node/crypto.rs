//! `node:crypto` 9e-1a/9e-1b/9e-1c：Hash/Hmac/随机 + 对称密码 + 非对称。
//! 复用全局 `__wjs_*`（随机/UUID/AES-GCM；零新 `UNSAFE-BOUNDARY`）；HMAC 经通用构造
//! 自架流式 Hash natives（sha3 与 hmac 0.13 的 block-API 不兼容，见修法记）；
//! 新增仅增量 Hash/Cipher 注册表（`encoding.rs` `STREAM_DECODERS` 同款线程本地表）。
//! 偏差记档（9e-1a）：
//! - 摘要集合 = RustCrypto 已接线：sha1/sha256/sha384/sha512/md5/sha3-256/384/512/
//!   blake2b512/blake2s256 + `ripemd160`（9h-2，`ripemd` crate）+ XOF
//!   `shake128/256`（9h-2，`tiny-keccak`；缺省输出长 shake128→16/shake256→32
//!   + DEP0198 警告，非法即 `ERR_INVALID_ARG_VALUE`）；其余 XOF（cshake/
//!   turboshake/kangaroo）不支持 → `createHash` 报原文
//!   `Digest method not supported`（无 code，Node 同款）；`getHashes` 只列已支持。
//! - 别名：大小写不敏感、`-`/`_` 可选、`RSA-` 前缀可剥（如 `RSA-SHA256`，Node 同款）。
//! - `Hmac` 更新攒 JS 侧（`digest` 时 oneshot），二次 `digest` 回空（Node 同款）；
//!   `digest` 后 `update`/`copy` 报 `ERR_CRYPTO_HASH_FINALIZED`。
//! - 未知输出编码的 `digest(enc)` 回 Buffer（Node 同款宽容）；未知输入编码按 utf8。
//! - 异步形态（`randomBytes(cb)`/`randomInt(cb)`/`randomFill`）经 `queueMicrotask`
//!   派发同步底层（`node:fs` 同款口径）；`getMacs`/`createMac` 真 Node 26 运行时
//!   不存在（仅 `lib/crypto.js` 残留导出），不做；`setEngine/getFips` 等随 9e-1c。
//! 偏差记档（9i-4 ml-kem）：
//! - 密钥 DER 用 LAMPS 种子形 PKCS#8（`[0]` 64B 种子，真机 26.8.2 同款逐字节同构，
//!   双向交叉互解）；`generateKey`（单面）不收 ml-kem（本仓 generateKey 仅为
//!   secret 面，既有口径）；`encapsulate` 异步回调形不做（给了第二参即
//!   ERR_INVALID_ARG_TYPE，与真机该路径报错同码）；`generateKeyPair` 未知类型
//!   仍报既有 ERR_NOT_SUPPORTED（真机为 ERR_INVALID_ARG_VALUE，pre-existing）。
//! 偏差记档（9i-6 ml-dsa）：
//! - 种子形 PKCS#8（`[0]` 32B 种子，真机同款）；顶层 `sign/verify` 收 ml-dsa
//!   （hash 必须 null，非 null 即真机码 ERR_OSSL_INVALID_DIGEST）；`Sign`/`Verify`
//!   流式类不收 ml-dsa（真机同款走顶层）；crate 的 Signer 为**确定性**签名档
//!   （真机 hedged，双方互验不受影响，双向交叉已验）；X.509 验签收 ml-dsa 证书
//!   （签名 OID 与密钥 OID 同族，openssl 3.6 实签证书真机/本仓同验）。
//! 偏差记档（9e-1b）：
//! - 对称集合：aes-128/192/256-cbc/ctr/gcm + chacha20-poly1305 + des-ede3-cbc
//!   + aes-128/192/256-ccm（10e；`ccm` 0.6 直引，全档分发）。
//!   GCM/ChaCha/CCM 系 AEAD 无流式（buffered，`final` 时 oneshot；http 体整收同款口径）。
//! - GCM 任意 iv（10e-2：12B 内走 crate，其余 J0 手工 `__wjs_gcm_anyiv`；
//!   WebCrypto 共用面维持 12B，spec 口径）。
//! - PKCS#7 填充校验非恒定时间实现（功能等价，侧信道记档）；`bf-cbc` 真机 26
//!   `getCiphers()` 已无 bf 系（10e 删项，不做）；`ocb/wrap` 系不做（ocb 非 Node 面）。

use mozjs::context::JSContext;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{report_error, value_to_string, view_bytes, Frame};

pub(crate) fn set_rval_str(cx: &mut JSContext, frame: &Frame, s: &str) {
    rooted!(&in(cx) let mut v = UndefinedValue());
    {
        use mozjs::conversions::ToJSValConvertible as _;
        s.to_jsval(cx, v.handle_mut());
    }
    frame.set_rval(v.get());
}

/// Uint8Array 返回值（`node:fs` 同款小 helper，不跨模块引）。
pub(crate) fn set_rval_bytes(cx: &mut JSContext, frame: &Frame, out: &[u8]) -> bool {
    rooted!(&in(cx) let mut obj: *mut mozjs::jsapi::JSObject = std::ptr::null_mut());
    // SAFETY: realm 内创建；obj 为 rooted 出参；out 存活到调用返回
    let ok = unsafe {
        mozjs::typedarray::TypedArray::<mozjs::typedarray::Uint8, *mut mozjs::jsapi::JSObject>::create(
            cx,
            mozjs::typedarray::CreateWith::Slice(out),
            obj.handle_mut(),
        )
    };
    if ok.is_err() || obj.is_null() {
        report_error(cx, "RangeError: cannot allocate output");
        return false;
    }
    frame.set_rval(mozjs::jsval::ObjectValue(obj.get()));
    true
}

/// id 实参（native 数值统一走字符串，JS 侧 `Number()` 包装，见 §4.33）。
pub(crate) fn arg_id(frame: &Frame, i: u32, what: &str, cx: &mut JSContext) -> Option<u64> {
    if frame.argc() <= i {
        report_error(cx, &format!("TypeError: {what} needs a hash handle"));
        return None;
    }
    value_to_string(cx, frame.arg(i)).parse::<u64>().ok().or_else(|| {
        report_error(cx, &format!("TypeError: {what} needs a hash handle"));
        None
    })
}

/// nullable 视图实参（`aad`/`tag` 传 null 即缺省）。
pub(crate) fn opt_view(cx: &mut JSContext, v: JSVal, what: &str) -> Option<Option<Vec<u8>>> {
    if v.is_null_or_undefined() {
        return Some(None);
    }
    view_bytes(cx, v, what).map(Some)
}
/// 左补零到定长（DH 密钥/密钥交换的定长口径，Node 回环长度断言即此）。
pub(crate) fn pad_be(bytes: &[u8], len: usize) -> Vec<u8> {
    if bytes.len() >= len {
        return bytes[bytes.len() - len..].to_vec();
    }
    let mut out = vec![0u8; len - bytes.len()];
    out.extend_from_slice(bytes);
    out
}

// ── RSA-SHA1/MD5 手工件（rsa 0.9 的 OAEP/签名接口绑 digest 0.10，
// sha1/sha2_010 0.10 系无直引行（§0.5 未批），故 OAEP-SHA1 与 v1.5-SHA1/MD5
// 签名在此手写：MGF1-SHA1 + BigUint 模幂 + 定长编码，全经真 Node 交叉验证。
// OAEP-SHA256/384/512 与 v1.5-SHA256/384/512 继续走既有 natives。）─────────

/// MGF1-SHA1（OAEP 编解码用）。
pub(crate) fn mgf1_sha1(seed: &[u8], len: usize) -> Vec<u8> {
    use sha1::Digest as _;
    let mut out = Vec::with_capacity(len);
    let mut ctr = 0u32;
    while out.len() < len {
        let mut h = sha1::Sha1::new();
        h.update(seed);
        h.update(ctr.to_be_bytes());
        out.extend_from_slice(&h.finalize());
        ctr += 1;
    }
    out.truncate(len);
    out
}

pub(crate) fn sha1_bytes(data: &[u8]) -> Vec<u8> {
    use sha1::Digest as _;
    sha1::Sha1::digest(data).to_vec()
}

pub use super::crypto_cipher::{
    ccm_crypt_native, cipher_chacha, cipher_final, cipher_new, cipher_set_autopad,
    cipher_update, gcm_anyiv,
};
pub use super::crypto_dh::{dh_genkey, dh_secret, prime_check, prime_gen};
pub use super::crypto_hash::{
    crypto_hash_copy, crypto_hash_digest, crypto_hash_new, crypto_hash_set_len,
    crypto_hash_update,
};
pub use super::crypto_kdf::{kdf_argon2, kdf_hkdf, kdf_pbkdf2, kdf_scrypt};
pub use super::crypto_pq::{
    mldsa_gen, mldsa_kind_from_spki, mldsa_public, mldsa_seed_from_pkcs8, mldsa_sign,
    mldsa_verify, mlkem_decaps, mlkem_encaps, mlkem_gen, mlkem_kind_from_spki,
    mlkem_seed_from_pkcs8,
};
pub(crate) use super::crypto_pq::{mldsa_kind_by_oid_str, mldsa_spki_pk, mldsa_verify_core};
pub use super::crypto_rsa::{
    node_rsa_oaep, node_rsa_oaep_flip, node_rsa_v15_sign, node_rsa_v15_verify,
    rsa_decrypt_v15, rsa_encrypt_v15, rsa_raw, rsa_v15_flip,
};
pub use super::crypto_x509::{x509_check_issued, x509_parse};

/// 内嵌 ESM 源（`node:crypto`；§0.9 按域分块：hash/cipher/keys/ec/sign/kdf/pqx509，
/// concat 字节恒等，首行无前导换行与既有拆分同口径）。
pub const SOURCE: &str = concat!(
    include_str!("crypto_hash.js"),
    include_str!("crypto_cipher.js"),
    include_str!("crypto_keys.js"),
    include_str!("crypto_keys_asymmetrickeyobject.js"),
    include_str!("crypto_ec.js"),
    include_str!("crypto_ec_derivepublic.js"),
    include_str!("crypto_sign.js"),
    include_str!("crypto_sign_diffiehellmanimpl.js"),
    include_str!("crypto_kdf.js"),
    include_str!("crypto_pqx509.js"),
);

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::crypto_cipher::{
        ccm_crypt, cipher_params, from_blocks, gcm_block_enc, gcm_j0, gcm_manual,
        pkcs7_pad, pkcs7_unpad, to_blocks,
    };
    use super::super::crypto_dh::is_prime;
    use super::super::crypto_hash::norm_hash;
    use super::super::crypto_pq::{
        mldsa_expand, mldsa_kind_by_oid, mldsa_params, mlkem_expand, mlkem_kind_by_oid,
        mlkem_params, mlkem_pkcs8, mlkem_pkcs8_seed, mlkem_spki, mlkem_spki_ek, mlkem_tlv,
    };
    use super::super::crypto_x509::{atv_short, fmt_asn1_time, x509_names_raw};

    #[test]
    fn crypto_hash_norm_table() {
        assert!(norm_hash("sha256", 0).is_some());
        assert!(norm_hash("SHA256", 0).is_some());
        assert!(norm_hash("sha-256", 0).is_some());
        assert!(norm_hash("RSA-SHA256", 0).is_some());
        assert!(norm_hash("sha3-256", 0).is_some());
        assert!(norm_hash("blake2b512", 0).is_some());
        assert!(norm_hash("blake2s256", 0).is_some());
        assert!(norm_hash("md5", 0).is_some());
        // 9h-2 落地：ripemd160 + SHAKE（长度注册时带）。
        assert!(norm_hash("ripemd160", 0).is_some());
        // 10f crypto首轮：sha224 + 别名 dss1/ripemd。
        assert!(norm_hash("sha224", 0).is_some());
        assert!(norm_hash("dss1", 0).is_some());
        assert!(norm_hash("DSS1", 0).is_some());
        assert!(norm_hash("ripemd", 0).is_some());
        assert!(norm_hash("shake128", 16).is_some());
        assert!(norm_hash("shake256", 32).is_some());
        assert!(norm_hash("nope", 0).is_none());
        assert!(norm_hash("", 0).is_none());
    }

    #[test]
    fn crypto_hash_known_vectors() {        use sha2::Digest as _; // 全员 digest 0.11 系（sha1/sha2/md5/sha3/blake2 同 trait，无需直引 digest，见 §0.5 零新增）
        assert_eq!(
            const_hex::encode(sha2::Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            const_hex::encode(md5::Md5::digest(b"abc")),
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            const_hex::encode(sha3::Sha3_256::digest(b"abc")),
            "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532"
        );
        // 9h-2：ripemd160 + SHAKE（真机向量，见 9h-2 黑盒）。
        assert_eq!(
            const_hex::encode(ripemd::Ripemd160::digest(b"abc")),
            "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc"
        );
        {
            use tiny_keccak::Hasher as _;
            let mut h = tiny_keccak::Shake::v128();
            h.update(b"abc");
            let mut out = [0u8; 32];
            h.finalize(&mut out);
            assert_eq!(
                const_hex::encode(out),
                "5881092dd818bf5cf8a3ddb793fbcba74097d5c526a6d35f97b83351940f2cc8"
            );
            let mut h = tiny_keccak::Shake::v256();
            h.update(b"abc");
            h.finalize(&mut out);
            assert_eq!(
                const_hex::encode(out),
                "483366601360a8771c6863080cc4114d8db44530f8f1e1ee4f94ea37e78b5739"
            );
        }
    }

    #[test]
    fn crypto_cipher_params_table() {
        assert_eq!(cipher_params("aes-256-cbc"), Some(("cbc-aes256", 32, 16, 16)));
        assert_eq!(cipher_params("AES-256-GCM"), None); // AEAD 不走流式注册表
        assert_eq!(cipher_params("aes-128-ctr"), Some(("ctr-aes128", 16, 16, 16)));
        assert_eq!(cipher_params("des-ede3-cbc"), Some(("cbc-des3", 24, 8, 8)));
        // 10f crypto首轮：ECB 三档（无 iv，iv 长记 0）。
        assert_eq!(cipher_params("aes-128-ecb"), Some(("ecb-aes128", 16, 0, 16)));
        assert_eq!(cipher_params("aes-192-ecb"), Some(("ecb-aes192", 24, 0, 16)));
        assert_eq!(cipher_params("aes-256-ecb"), Some(("ecb-aes256", 32, 0, 16)));
        assert!(cipher_params("aes-999-cbc").is_none());
        assert!(cipher_params("").is_none());
    }

        #[test]
    fn crypto_ccm_known_answer() {        // 真 Node 交叉取证：aes-128-ccm(key=0001..0f, nonce=1011..1b, tag 12, "hello CCM")
        // 逐字节一致（ct 4bd0d5cc2dd46ef147 / tag e3e2af5555de4dea4caafca2）。
        let key = const_hex::decode("000102030405060708090a0b0c0d0e0f").unwrap();
        let nonce = const_hex::decode("101112131415161718191a1b").unwrap();
        let out = ccm_crypt(&key, &nonce, b"", b"hello CCM", 12, true).unwrap();
        assert_eq!(const_hex::encode(&out[..9]), "4bd0d5cc2dd46ef147");
        assert_eq!(const_hex::encode(&out[9..]), "e3e2af5555de4dea4caafca2");
        // 解密往返 + 错 tag 拒收（原文无码错，Node 同款）
        let pt = ccm_crypt(&key, &nonce, b"", &out, 12, false).unwrap();
        assert_eq!(pt, b"hello CCM");
        let mut bad = out.clone();
        bad[10] ^= 0xFF;
        assert_eq!(
            ccm_crypt(&key, &nonce, b"", &bad, 12, false),
            Err("Unsupported state or unable to authenticate data".to_string())
        );
        // 边界：nonce 6/14 拒、tag 5 拒、key 10B 拒
        assert!(ccm_crypt(&key, &[0u8; 6], b"", b"x", 8, true).is_err());
        assert!(ccm_crypt(&key, &[0u8; 14], b"", b"x", 8, true).is_err());
        assert!(ccm_crypt(&key, &nonce, b"", b"x", 5, true).is_err());
        assert!(ccm_crypt(&[0u8; 10], &nonce, b"", b"x", 8, true).is_err());
        // 全档冒烟：三密钥 × nonce 7/13 × tag 4/16 往返
        for kl in [16usize, 24, 32] {
            for nl in [7usize, 13] {
                for tl in [4usize, 16] {
                    let k = vec![0xA5u8; kl];
                    let n = vec![0x3Cu8; nl];
                    let ct = ccm_crypt(&k, &n, b"ad", b"data", tl, true).unwrap();
                    assert_eq!(ct.len(), 4 + tl);
                    let back = ccm_crypt(&k, &n, b"ad", &ct, tl, false).unwrap();
                    assert_eq!(back, b"data");
                }
            }
        }
    }

    #[test]
    fn crypto_gcm_j0_known_answer() {
        // 真 Node 交叉取证：aes-128-gcm(key=0001..0f, iv8=0102..08, aad="aad",
        // "hello GCM") → ct 0abdd127a6e4463bbe / tag 5fee9590bf890f933f19030aa67fad71。
        let key = const_hex::decode("000102030405060708090a0b0c0d0e0f").unwrap();
        let iv = const_hex::decode("0102030405060708").unwrap();
        let out = gcm_manual(&key, &iv, b"aad", b"hello GCM", true).unwrap();
        assert_eq!(const_hex::encode(&out[..9]), "0abdd127a6e4463bbe");
        assert_eq!(const_hex::encode(&out[9..]), "5fee9590bf890f933f19030aa67fad71");
        let back = gcm_manual(&key, &iv, b"aad", &out, false).unwrap();
        assert_eq!(back, b"hello GCM");
        // J0 形状：12B 后缀 00..01；8B 走 GHASH（与 12B 结果不同即分路生效）
        let h = gcm_block_enc(&key, &[0u8; 16]).unwrap();
        let j12 = gcm_j0(&h, &[0xABu8; 12]).unwrap();
        assert_eq!(&j12[..12], &[0xABu8; 12]);
        assert_eq!(&j12[12..], &[0, 0, 0, 1]);
        // 错 tag 拒收（原文无码错）+ 空 iv 拒收
        let mut bad = out.clone();
        bad[10] ^= 0xFF;
        assert_eq!(
            gcm_manual(&key, &iv, b"aad", &bad, false),
            Err("Unsupported state or unable to authenticate data".to_string())
        );
        assert!(gcm_manual(&key, &[], b"", b"x", true).is_err());
        // 192/256 档冒烟（与 crate 12B 路径同口径：12B 手工 == crate 输出）
        for kl in [16usize, 24, 32] {
            let k = vec![0x11u8; kl];
            let iv12 = vec![0x22u8; 12];
            let a = gcm_manual(&k, &iv12, b"a", b"data", true).unwrap();
            let b = crate::builtins::crypto::gcm_encrypt_raw(&k, &iv12, b"a", b"data").unwrap();
            assert_eq!(a, b);
        }
    }

    #[test]
    fn crypto_pkcs7_roundtrip() {
        assert_eq!(pkcs7_pad(16, vec![]), vec![16u8; 16]);
        assert_eq!(pkcs7_pad(16, vec![1, 2, 3]).len(), 16);
        assert_eq!(pkcs7_unpad(16, &pkcs7_pad(16, b"hello".to_vec())), Some(b"hello".to_vec()));
        assert!(pkcs7_unpad(16, &[1, 2, 3]).is_none());
        assert!(pkcs7_unpad(16, &[]).is_none());
        assert!(pkcs7_unpad(16, &[16u8; 15]).is_none());
    }

    #[test]
    fn crypto_cbc_known_vector() {        // 真 Node 取证：aes-256-cbc(key=01×32, iv=02×16, "hello world")
        use aes::cipher::KeyIvInit as _;
        use aes::cipher::block::BlockModeEncrypt as _;
        let key: aes::cipher::Key<aes::Aes256> = [1u8; 32].into();
        let iv: aes::cipher::Block<aes::Aes256> = [2u8; 16].into();
        let mut enc = cbc::Encryptor::<aes::Aes256>::new(&key, &iv);
        let mut blocks = to_blocks::<aes::Aes256>(&pkcs7_pad(16, b"hello world".to_vec()));
        enc.encrypt_blocks(&mut blocks);
        assert_eq!(
            const_hex::encode(from_blocks::<aes::Aes256>(&blocks)),
            "f563737a376afbed282274255a7fcabd"
        );
    }

    #[test]
    fn crypto_miller_rabin_small() {
        for p in [2u32, 3, 5, 7, 11, 13, 7919, 104729] {
            assert!(is_prime(&rsa::BigUint::from(p), 8), "{p} should be prime");
        }
        for n in [0u32, 1, 4, 9, 15, 21, 25, 27, 561, 2047] {
            assert!(!is_prime(&rsa::BigUint::from(n), 8), "{n} should be composite");
        }
    }

    #[test]
    fn crypto_mgf1_sha1_vector() {
        // MGF1-SHA1("test", 20) = SHA1("test" ‖ 0x00000000)
        assert_eq!(
            const_hex::encode(mgf1_sha1(b"test", 20)),
            "b67344dc7dea343795faaba3bc4d4508bf6766b1"
        );
        assert_eq!(mgf1_sha1(b"test", 24).len(), 24);
        assert_eq!(
            const_hex::encode(mgf1_sha1(b"test", 24)[20..].to_vec()),
            "b45ef443"
        );
    }

    #[test]
    fn crypto_pad_be_shapes() {
        assert_eq!(pad_be(&[1, 2], 4), vec![0, 0, 1, 2]);
        assert_eq!(pad_be(&[1, 2, 3, 4, 5], 3), vec![3, 4, 5]);
    }

    #[test]
    fn crypto_asn1_time_shapes() {
        assert_eq!(fmt_asn1_time(0), "Jan  1 00:00:00 1970 GMT");
        // 2026-09-12T07:03:16Z（真机证书有效期同形）
        assert_eq!(fmt_asn1_time(1789196596), "Sep 12 07:03:16 2026 GMT");
    }

    #[test]
    fn crypto_atv_short_table() {
        assert_eq!(atv_short("2.5.4.3"), "CN");
        assert_eq!(atv_short("2.5.4.10"), "O");
        assert_eq!(atv_short("1.2.840.113549.1.9.1"), "emailAddress");
        assert_eq!(atv_short("1.2.3.4"), "1.2.3.4");
    }

    // ── 9i-4 ml-kem ────────────────────────────────────────────────────────

    #[test]
    fn mlkem_tables_and_tlv() {
        assert_eq!(mlkem_params("ml-kem-512").map(|p| (p.1, p.2)), Some((800, 768)));
        assert_eq!(mlkem_params("ml-kem-768").map(|p| (p.1, p.2)), Some((1184, 1088)));
        assert_eq!(mlkem_params("ml-kem-1024").map(|p| (p.1, p.2)), Some((1568, 1568)));
        assert!(mlkem_params("ml-kem").is_none());
        assert!(mlkem_params("nope").is_none());
        for kind in ["ml-kem-512", "ml-kem-768", "ml-kem-1024"] {
            let (oid, _, _) = mlkem_params(kind).unwrap();
            assert_eq!(mlkem_kind_by_oid(oid), Some(kind));
        }
        assert!(mlkem_kind_by_oid(&[0u8; 9]).is_none());
        // 长形长度（800 = 0x0320 → 0x82 双字节形）
        let t = mlkem_tlv(0x04, &vec![0u8; 800]);
        assert_eq!(&t[..4], &[0x04, 0x82, 0x03, 0x20]);
        assert_eq!(t.len(), 804);
        let s = mlkem_tlv(0x04, &[1, 2, 3]);
        assert_eq!(s, vec![0x04, 0x03, 1, 2, 3]);
    }

    #[test]
    fn mlkem_wrap_parse_roundtrip() {
        let seed = [7u8; 64];
        let (oid, _, _) = mlkem_params("ml-kem-768").unwrap();
        // PKCS#8：总长 86（真机同款），种子逐字节还原；[0] 标签破坏即 None。
        let pkcs8 = mlkem_pkcs8(oid, &seed);
        assert_eq!(pkcs8.len(), 86);
        let (oid2, parsed) = mlkem_pkcs8_seed(&pkcs8, 64).unwrap();
        assert_eq!(oid2, oid);
        assert_eq!(parsed, &seed[..]);
        let mut bad = pkcs8.clone();
        bad[20] = 0x81; // 内层 [0] → 0x81（上下文构造形），结构不符
        assert!(mlkem_pkcs8_seed(&bad, 64).is_none());
        assert!(mlkem_pkcs8_seed(&pkcs8[..40], 64).is_none());
        // 10f crypto五轮：展开形（套件 ml_dsa_44_private.pem 指纹
        // OCTET{ SEQ{ OCTET(seed), OCTET(rest) } }）——首子 OCTET 即种子；
        // 首子错长/非 OCTET 即 None。
        let seed44 = [9u8; 32];
        let (doid, _, _) = mldsa_params("ml-dsa-44").unwrap();
        let alg = mlkem_tlv(0x30, &mlkem_tlv(0x06, doid));
        let mut exp_body = mlkem_tlv(0x02, &[0]);
        exp_body.extend_from_slice(&alg);
        let mut inner_seq = mlkem_tlv(0x04, &seed44);
        inner_seq.extend_from_slice(&mlkem_tlv(0x04, &[1u8; 16]));
        exp_body.extend_from_slice(&mlkem_tlv(0x04, &mlkem_tlv(0x30, &inner_seq)));
        let exp = mlkem_tlv(0x30, &exp_body);
        let (oid4, got) = mlkem_pkcs8_seed(&exp, 32).unwrap();
        assert_eq!(oid4, doid);
        assert_eq!(got, &seed44[..]);
        // 首子 OCTET 错长（31B）即 None；首子换 INTEGER 即 None。
        let mut short_seq = mlkem_tlv(0x04, &[9u8; 31]);
        short_seq.extend_from_slice(&mlkem_tlv(0x04, &[1u8; 16]));
        let mut short_body = mlkem_tlv(0x02, &[0]);
        short_body.extend_from_slice(&alg);
        short_body.extend_from_slice(&mlkem_tlv(0x04, &mlkem_tlv(0x30, &short_seq)));
        assert!(mlkem_pkcs8_seed(&mlkem_tlv(0x30, &short_body), 32).is_none());
        let mut int_seq = mlkem_tlv(0x02, &[9u8; 32]);
        int_seq.extend_from_slice(&mlkem_tlv(0x04, &[1u8; 16]));
        let mut int_body = mlkem_tlv(0x02, &[0]);
        int_body.extend_from_slice(&alg);
        int_body.extend_from_slice(&mlkem_tlv(0x04, &mlkem_tlv(0x30, &int_seq)));
        assert!(mlkem_pkcs8_seed(&mlkem_tlv(0x30, &int_body), 32).is_none());
        // SPKI：768 档总长 1206（真机同款），ek 原样还原；未用位非零即 None。
        let ek = vec![3u8; 1184];
        let spki = mlkem_spki(oid, &ek);
        assert_eq!(spki.len(), 1206);
        let (oid3, ek2) = mlkem_spki_ek(&spki).unwrap();
        assert_eq!(oid3, oid);
        assert_eq!(ek2, &ek[..]);
        let mut bad2 = spki.clone();
        bad2[21] = 1; // BIT STRING 未用位
        assert!(mlkem_spki_ek(&bad2).is_none());
        assert!(mlkem_spki_ek(&[0x04, 0x00]).is_none());
    }

    #[test]
    fn x509_names_raw_walk() {
        // 手搭 TBS：[0]版本 + serial + sig + issuer(A) + validity + subject(B)，
        // 名字裸 TLV 必须逐字节还原（issuer 取 A、subject 取 B）。
        let name_a: &[u8] = &[0x30, 0x05, 0x0c, 0x03, b'a', b'b', b'c'];
        let name_b: &[u8] = &[0x30, 0x05, 0x0c, 0x03, b'x', b'y', b'z'];
        let utc = |s: &[u8]| -> Vec<u8> {
            let mut t = vec![0x17u8, 0x0d];
            t.extend_from_slice(s);
            t
        };
        let v1 = utc(b"260912000000Z");
        let v2 = utc(b"261012000000Z");
        let mut validity = vec![0x30u8, (v1.len() + v2.len()) as u8];
        validity.extend_from_slice(&v1);
        validity.extend_from_slice(&v2);
        let mut t: Vec<u8> = vec![0xa0, 0x03, 0x02, 0x01, 0x02, 0x02, 0x01, 0x03, 0x30, 0x00];
        t.extend_from_slice(name_a);
        t.extend_from_slice(&validity);
        t.extend_from_slice(name_b);
        let wrap = |content: &[u8]| -> Vec<u8> {
            let mut tbs = vec![0x30u8, 0x81, content.len() as u8];
            tbs.extend_from_slice(content);
            let mut cert = vec![0x30u8, 0x81, tbs.len() as u8];
            cert.extend_from_slice(&tbs);
            cert
        };
        let der = wrap(&t);
        let (issuer, subject) = x509_names_raw(&der).unwrap();
        assert_eq!(issuer, name_a);
        assert_eq!(subject, name_b);
        // 无 [0] 版本头也走通（v1 证书形态）
        let v1_der = wrap(&t[5..]);
        let (issuer2, subject2) = x509_names_raw(&v1_der).unwrap();
        assert_eq!(issuer2, name_a);
        assert_eq!(subject2, name_b);
        // 截断
        assert!(x509_names_raw(&der[..der.len() - 1]).is_none());
    }

    #[test]
    fn mldsa_tables_and_wrap() {
        assert_eq!(mldsa_params("ml-dsa-44").map(|p| (p.1, p.2)), Some((1312, 2420)));
        assert_eq!(mldsa_params("ml-dsa-65").map(|p| (p.1, p.2)), Some((1952, 3309)));
        assert_eq!(mldsa_params("ml-dsa-87").map(|p| (p.1, p.2)), Some((2592, 4627)));
        assert!(mldsa_params("ml-dsa").is_none());
        for kind in ["ml-dsa-44", "ml-dsa-65", "ml-dsa-87"] {
            let (oid, _, _) = mldsa_params(kind).unwrap();
            assert_eq!(mldsa_kind_by_oid(oid), Some(kind));
        }
        // dotted 串往返（X.509 证书签名算法 OID）
        assert_eq!(mldsa_kind_by_oid_str("2.16.840.1.101.3.4.3.17"), Some("ml-dsa-44"));
        assert_eq!(mldsa_kind_by_oid_str("2.16.840.1.101.3.4.3.18"), Some("ml-dsa-65"));
        assert_eq!(mldsa_kind_by_oid_str("2.16.840.1.101.3.4.3.19"), Some("ml-dsa-87"));
        assert_eq!(mldsa_kind_by_oid_str("2.16.840.1.101.3.4.3.99"), None);
        // PKCS#8 54B / SPKI 1974B（65 档，真机同款）；种子逐字节还原
        let seed = [9u8; 32];
        let (pkcs8, spki) = mldsa_expand("ml-dsa-65", &seed).unwrap();
        assert_eq!(pkcs8.len(), 54);
        assert_eq!(spki.len(), 1974);
        let (oid, parsed) = mlkem_pkcs8_seed(&pkcs8, 32).unwrap();
        assert_eq!(mldsa_kind_by_oid(oid), Some("ml-dsa-65"));
        assert_eq!(parsed, &seed[..]);
        // 结构破坏（内层 [0] 标签改写）即 None
        let mut bad = pkcs8.clone();
        bad[20] = 0x81;
        assert!(mlkem_pkcs8_seed(&bad, 32).is_none());
        // SPKI pk 原样还原；未用位非零即 None
        let (_, pk) = mlkem_spki_ek(&spki).unwrap();
        assert_eq!(pk.len(), 1952);
        let mut bad2 = spki.clone();
        bad2[21] = 1;
        assert!(mlkem_spki_ek(&bad2).is_none());
    }

    #[test]
    fn mldsa_sign_verify_core_roundtrip() {
        // 真 crate：from_seed → sign（确定性档）→ verify_core 过；篡改/换消息不过。
        let seed = [0x42u8; 32];
        let (pkcs8, spki) = mldsa_expand("ml-dsa-65", &seed).unwrap();
        let (_, seed_b) = mlkem_pkcs8_seed(&pkcs8, 32).unwrap();
        let mut s = [0u8; 32];
        s.copy_from_slice(seed_b);
        let sig = {
            use ml_dsa::Signer as _;
            ml_dsa::SigningKey::<ml_dsa::MlDsa65>::from_seed(&s.into())
                .sign(b"unit-msg")
                .encode()
                .as_slice()
                .to_vec()
        };
        assert_eq!(sig.len(), 3309);
        let (_, pk) = mlkem_spki_ek(&spki).unwrap();
        assert!(mldsa_verify_core("ml-dsa-65", pk, &sig, b"unit-msg"));
        assert!(!mldsa_verify_core("ml-dsa-65", pk, &sig, b"other-msg"));
        let mut bad = sig.clone();
        bad[500] ^= 0xff;
        assert!(!mldsa_verify_core("ml-dsa-65", pk, &bad, b"unit-msg"));
        // 档位错配（44 公钥验 65 签名）→ false
        let (_, pk44_spki) = mldsa_expand("ml-dsa-44", &seed).unwrap();
        let (_, pk44_raw) = mlkem_spki_ek(&pk44_spki).unwrap();
        assert!(!mldsa_verify_core("ml-dsa-44", pk44_raw, &sig, b"unit-msg"));
    }

    #[test]
    fn mlkem_expand_real_crate() {
        let mut seed = [0u8; 64];
        getrandom::fill(&mut seed).unwrap();
        let (pkcs8, spki) = mlkem_expand("ml-kem-768", &seed).unwrap();
        assert_eq!(pkcs8.len(), 86);
        assert_eq!(spki.len(), 1206);
        let (_, parsed) = mlkem_pkcs8_seed(&pkcs8, 64).unwrap();
        assert_eq!(parsed, &seed[..]);
        let (_, ek) = mlkem_spki_ek(&spki).unwrap();
        assert_eq!(ek.len(), 1184);
        // 三档尺寸表（真机对齐：SPKI 822/1206/1590）
        for (kind, spki_len) in [("ml-kem-512", 822), ("ml-kem-1024", 1590)] {
            let (_, sp) = mlkem_expand(kind, &seed).unwrap();
            assert_eq!(sp.len(), spki_len);
        }
    }
}
