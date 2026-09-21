//! 内建注册与 JS prelude。
//! prelude 用 JS 实现需要 Promise/参数打包语义的薄壳（queueMicrotask、timers 包装、
//! `__wjs_call`/`__wjs_entries` 辅助），native 只做 Rust 侧的活。

pub mod clone;
pub mod console;
pub mod crypto;
pub mod encoding;
pub mod fetch;
pub mod node;
pub mod prelude;
pub mod timers;
pub mod url;
pub mod ws;
pub mod bun;

use std::ffi::CString;

use mozjs::context::JSContext;
use mozjs::jsapi::{JSObject, JSNative, JSPROP_ENUMERATE};
use mozjs::jsval::ObjectValue;
use mozjs::rooted;
use crate::jsapi_glue::raw_handle;

use crate::error::Error;
use crate::jsapi_glue::report_error;


pub use prelude::PRELUDE;

/// 在 global 上定义全部 native（prelude 求值之前）。
pub fn define_all(cx: &mut JSContext, global: *mut JSObject) -> Result<(), Error> {
    // SAFETY: cx 处于 global 所属 realm（调用方持 AutoRealm）；raw 调用不触发 GC。
    unsafe {
        let rcx = cx.raw_cx();
        let timers: &[(&str, JSNative, u32)] = &[
            ("__wjs_setTimeout", Some(timers::set_timeout), 3),
            ("__wjs_setInterval", Some(timers::set_interval), 3),
            ("__wjs_clearTimeout", Some(timers::clear_timeout), 1),
            ("__wjs_timer_ref", Some(timers::timer_ref), 2),
            ("__wjs_timer_refresh", Some(timers::timer_refresh), 1),
        ];
        for (name, native, nargs) in timers {
            let cname = CString::new(*name).expect("no NUL");
            if mozjs::jsapi::JS_DefineFunction(rcx, raw_handle(&global), cname.as_ptr(), *native, *nargs, 0)
                .is_null()
            {
                report_error(cx, "failed to define builtin");
                return Err(Error::Other(format!("failed to define builtin {name}")));
            }
        }

        // Phase 3a: URL 解析 / form 编解码 / base64 / 编码器 / 随机数
        // （prelude 真类 + 薄壳；复杂值走 JSON 桥；见各模块文档）
        let web: &[(&str, JSNative, u32)] = &[
            ("__wjs_url_parse", Some(url::url_parse), 2),
            ("__wjs_url_get", Some(url::url_get), 2),
            ("__wjs_url_set", Some(url::url_set), 3),
            ("__wjs_usp_parse", Some(url::usp_parse), 1),
            ("__wjs_usp_serialize", Some(url::usp_serialize), 1),
            ("__wjs_urlpattern_parse", Some(url::urlpattern_parse), 3),
            ("__wjs_urlpattern_test", Some(url::urlpattern_test), 3),
            ("__wjs_urlpattern_exec", Some(url::urlpattern_exec), 3),
            ("__wjs_btoa", Some(encoding::btoa_encode), 1),
            ("__wjs_atob", Some(encoding::atob_decode), 1),
            ("__wjs_te_encode", Some(encoding::te_encode), 1),
            ("__wjs_te_encode_into", Some(encoding::te_encode_into), 2),
            ("__wjs_td_canonical", Some(encoding::td_canonical), 1),
            ("__wjs_td_decode", Some(encoding::td_decode), 4),
            ("__wjs_td_stream_open", Some(encoding::td_stream_open), 3),
            ("__wjs_td_stream_feed", Some(encoding::td_stream_feed), 3),
            ("__wjs_fill_random", Some(crypto::fill_random), 1),
            ("__wjs_random_uuid", Some(crypto::random_uuid), 0),
            ("__wjs_subtle_digest", Some(crypto::subtle_digest), 2),
            ("__wjs_aesgcm_encrypt", Some(crypto::aesgcm_encrypt), 4),
            ("__wjs_aesgcm_decrypt", Some(crypto::aesgcm_decrypt), 4),
            ("__wjs_hmac_sign", Some(crypto::hmac_sign), 3),
            ("__wjs_hmac_verify", Some(crypto::hmac_verify), 4),
            ("__wjs_rsa_generate", Some(crypto::rsa_generate), 2),
            ("__wjs_rsa_public", Some(crypto::rsa_public), 1),
            ("__wjs_rsa_sign", Some(crypto::rsa_sign), 3),
            ("__wjs_rsa_verify", Some(crypto::rsa_verify), 4),
            ("__wjs_rsa_encrypt", Some(crypto::rsa_encrypt), 4),
            ("__wjs_rsa_decrypt", Some(crypto::rsa_decrypt), 4),
            ("__wjs_rsa_jwk", Some(crypto::rsa_jwk), 2),
            ("__wjs_rsa_jwk_pub", Some(crypto::rsa_jwk_pub), 1),
            ("__wjs_rsa_import_priv", Some(crypto::rsa_import_priv), 3),
            ("__wjs_rsa_import_pub", Some(crypto::rsa_import_pub), 2),
            ("__wjs_ec_generate", Some(crypto::ec_generate), 1),
            ("__wjs_ec_public", Some(crypto::ec_public), 2),
            ("__wjs_ecdsa_sign", Some(crypto::ecdsa_sign), 4),
            ("__wjs_ecdsa_verify", Some(crypto::ecdsa_verify), 5),
            ("__wjs_ecdh_derive", Some(crypto::ecdh_derive), 3),
            ("__wjs_ec_jwk", Some(crypto::ec_jwk), 3),
            ("__wjs_ec_jwk_pub", Some(crypto::ec_jwk_pub), 2),
            ("__wjs_ec_import_priv", Some(crypto::ec_import_priv), 2),
            ("__wjs_ec_import_pub", Some(crypto::ec_import_pub), 3),
            // 10f crypto五轮：压缩/混合 SEC1 点导入（轮子内解压+上曲线校验）
            ("__wjs_ec_import_compressed", Some(crypto::ec_import_compressed), 2),
            // 9h-1：SPKI/PKCS#8 算法 OID 直判曲线（试解误判 secp256k1→P-256）
            ("__wjs_ec_guess_curve", Some(crypto::ec_guess_curve), 1),
            // 9i-3：X.509 证书验签（TBS 裸段 + 签名算法 OID 分发，复用验签底座）
            ("__wjs_x509_verify", Some(crypto::x509_verify), 3),
            // Phase c-4x：RSA-PSS / Ed25519 / X25519
            ("__wjs_pss_sign", Some(crypto::pss_sign), 4),
            ("__wjs_pss_verify", Some(crypto::pss_verify), 5),
            ("__wjs_ed_generate", Some(crypto::ed_generate), 0),
            ("__wjs_ed_public", Some(crypto::ed_public), 1),
            ("__wjs_ed_sign", Some(crypto::ed_sign), 2),
            ("__wjs_ed_verify", Some(crypto::ed_verify), 3),
            // 10e：Ed448（ed448-goldilocks 特批钉版；RFC 8032 纯签名）
            ("__wjs_ed448_generate", Some(crypto::ed448_generate), 0),
            ("__wjs_ed448_public", Some(crypto::ed448_public), 1),
            ("__wjs_ed448_sign", Some(crypto::ed448_sign), 2),
            ("__wjs_ed448_verify", Some(crypto::ed448_verify), 3),
            ("__wjs_x_generate", Some(crypto::x_generate), 0),
            ("__wjs_x_public", Some(crypto::x_public), 1),
            ("__wjs_x_derive", Some(crypto::x_derive), 2),
            ("__wjs_x448_generate", Some(crypto::x448_generate), 0),
            ("__wjs_x448_public", Some(crypto::x448_public), 1),
            ("__wjs_x448_derive", Some(crypto::x448_derive), 2),
            ("__wjs_okp_pkcs8_from_seed", Some(crypto::okp_pkcs8_from_seed), 2),
            ("__wjs_okp_spki_from_pub", Some(crypto::okp_spki_from_pub), 2),
            ("__wjs_okp_seed_from_pkcs8", Some(crypto::okp_seed_from_pkcs8), 2),
            ("__wjs_okp_pub_from_spki", Some(crypto::okp_pub_from_spki), 2),
            ("__wjs_fetch_start", Some(fetch::fetch_start), 6),
            ("__wjs_fetch_abort", Some(fetch::fetch_abort), 1),
            ("__wjs_fetch_pull", Some(fetch::fetch_pull), 3),
            // plan4 T1：独立 serve 响应面（head/push/fail；见 serve_bridge）。
            ("__wjs_serve_head", Some(crate::serve_bridge::serve_head), 2),
            ("__wjs_serve_push", Some(crate::serve_bridge::serve_push), 2),
            ("__wjs_serve_fail", Some(crate::serve_bridge::serve_fail), 2),
            // plan4 T4：服务端 WS 决策面（create/accept/decline；同上）。
            ("__wjs_serve_ws_create", Some(crate::serve_bridge::serve_ws_create), 1),
            ("__wjs_serve_ws_accept", Some(crate::serve_bridge::serve_ws_accept), 1),
            ("__wjs_serve_ws_decline", Some(crate::serve_bridge::serve_ws_decline), 1),
            // Phase 4a: node:os / process（path 纯 JS，见 node/）
            ("__wjs_os_platform", Some(node::os::os_platform), 0),
            ("__wjs_os_arch", Some(node::os::os_arch), 0),
            ("__wjs_os_info", Some(node::os::os_info), 0),
            ("__wjs_os_cpus", Some(node::os::os_cpus), 0),
            ("__wjs_os_mem", Some(node::os::os_mem), 0),
            ("__wjs_os_net", Some(node::os::os_net), 0),
            ("__wjs_os_user", Some(node::os::os_user), 0),
            ("__wjs_os_uptime", Some(node::os::os_uptime), 0),
            ("__wjs_os_load", Some(node::os::os_load), 0),
            ("__wjs_os_locale", Some(node::os::os_locale), 0),
            // 10f os 对拍：machine/uname/priority
            ("__wjs_os_machine", Some(node::os::os_machine), 0),
            ("__wjs_os_uname", Some(node::os::os_uname), 0),
            ("__wjs_os_prio_get", Some(node::os::os_prio_get), 1),
            ("__wjs_os_prio_set", Some(node::os::os_prio_set), 2),
            ("__wjs_argv_json", Some(node::process_::argv_json), 0),
            ("__wjs_next_tick", Some(node::process_::next_tick_queue), 2),
            ("__wjs_process_getuid", Some(node::process_::getuid), 0),
            ("__wjs_process_getgid", Some(node::process_::getgid), 0),
            ("__wjs_process_geteuid", Some(node::process_::geteuid), 0),
            ("__wjs_process_getegid", Some(node::process_::getegid), 0),
            ("__wjs_process_getgroups", Some(node::process_::getgroups), 0),
            ("__wjs_env_get", Some(node::process_::env_get), 1),
            ("__wjs_env_set", Some(node::process_::env_set), 2),
            ("__wjs_env_del", Some(node::process_::env_del), 1),
            ("__wjs_env_keys", Some(node::process_::env_keys), 0),
            ("__wjs_cwd", Some(node::process_::cwd), 0),
            ("__wjs_chdir", Some(node::process_::chdir), 1),
            ("__wjs_process_exit", Some(node::process_::process_exit), 1),
            ("__wjs_exit_code_get", Some(node::process_::exit_code_get), 0),
            ("__wjs_exit_code_set", Some(node::process_::exit_code_set), 1),
            ("__wjs_exec_path", Some(node::process_::exec_path), 0),
            ("__wjs_pid", Some(node::process_::pid), 0),
            // 10f：process.umask（unix 真改，test/common 前置）
            ("__wjs_umask", Some(node::process_::umask), 1),
            ("__wjs_uptime", Some(node::process_::uptime), 0),
            ("__wjs_hrtime_ns", Some(node::process_::hrtime_ns), 0),
            ("__wjs_memory_usage", Some(node::process_::memory_usage), 0),
            ("__wjs_stdout_write", Some(node::process_::stdout_write), 1),
            ("__wjs_stderr_write", Some(node::process_::stderr_write), 1),
            ("__wjs_stdio_istty", Some(node::process_::stdio_istty), 1),
            ("__wjs_stdin_poll", Some(node::process_::stdin_poll), 0),
            // 10c-1: node:tty（winsize/setRawMode；unix-only 实现，其余平台回落）
            ("__wjs_tty_winsize", Some(node::tty::tty_winsize), 1),
            ("__wjs_tty_set_raw_mode", Some(node::tty::tty_set_raw_mode), 2),
            // Phase 4b: node:fs
            ("__wjs_fs_read_file", Some(node::fs::fs_read_file), 1),
            ("__wjs_fs_write_file", Some(node::fs::fs_write_file), 3),
            ("__wjs_fs_append_file", Some(node::fs::fs_append_file), 2),
            ("__wjs_fs_stat", Some(node::fs::fs_stat), 2),
            // M5 vitest 牵引：statfs（unix 经 nix statvfs，既有直引轮子）
            ("__wjs_fs_statfs", Some(node::fs::fs_statfs), 1),
            ("__wjs_fs_mkdir", Some(node::fs::fs_mkdir), 2),
            ("__wjs_fs_rm", Some(node::fs::fs_rm), 3),
            ("__wjs_fs_readdir", Some(node::fs::fs_readdir), 2),
            ("__wjs_fs_rename", Some(node::fs::fs_rename), 2),
            ("__wjs_fs_copy_file", Some(node::fs::fs_copy_file), 2),
            ("__wjs_fs_exists", Some(node::fs::fs_exists), 1),
            ("__wjs_fs_unlink", Some(node::fs::fs_unlink), 1),
            ("__wjs_fs_rmdir", Some(node::fs::fs_rmdir), 2),
            ("__wjs_fs_realpath", Some(node::fs::fs_realpath), 1),
            ("__wjs_fs_mkdtemp", Some(node::fs::fs_mkdtemp), 1),
            // Phase 9c: fs 同步面增补（fd 系/link 系/时间戳/权限/access）
            ("__wjs_fs_read_link", Some(node::fs::fs_read_link), 1),
            ("__wjs_fs_link", Some(node::fs::fs_link), 2),
            ("__wjs_fs_symlink", Some(node::fs::fs_symlink), 2),
            ("__wjs_fs_truncate", Some(node::fs::fs_truncate), 2),
            ("__wjs_fs_utimes", Some(node::fs::fs_utimes), 3),
            #[cfg(unix)]
            ("__wjs_fs_lutimes", Some(node::fs::fs_lutimes), 3),
            ("__wjs_fs_chmod", Some(node::fs::fs_chmod), 2),
            ("__wjs_fs_access", Some(node::fs::fs_access), 2),
            ("__wjs_fs_open", Some(node::fs::fs_open), 2),
            ("__wjs_fs_close", Some(node::fs::fs_close), 1),
            ("__wjs_fs_read_fd", Some(node::fs::fs_read_fd), 3),
            ("__wjs_fs_write_fd", Some(node::fs::fs_write_fd), 3),
            ("__wjs_fs_ftruncate", Some(node::fs::fs_ftruncate), 2),
            ("__wjs_fs_fstat", Some(node::fs::fs_fstat), 1),
            ("__wjs_fs_fchmod", Some(node::fs::fs_fchmod), 2),
            ("__wjs_fs_chown", Some(node::fs::fs_chown), 3),
            ("__wjs_fs_lchown", Some(node::fs::fs_lchown), 3),
            #[cfg(target_os = "macos")]
            ("__wjs_fs_lchmod", Some(node::fs::fs_lchmod), 2),
            ("__wjs_fs_fchown", Some(node::fs::fs_fchown), 3),
            ("__wjs_fs_futimes", Some(node::fs::fs_futimes), 3),
            ("__wjs_fs_fsync", Some(node::fs::fs_fsync), 2),
            ("__wjs_fs_stream_ref", Some(node::fs::fs_stream_ref), 0),
            ("__wjs_fs_stream_unref", Some(node::fs::fs_stream_unref), 0),
            ("__wjs_watch_start", Some(node::fs::watch_start), 4),
            ("__wjs_watch_close", Some(node::fs::watch_close), 1),
            ("__wjs_watch_persistent", Some(node::fs::watch_persistent), 2),
            ("__wjs_glob_match", Some(node::fs::glob_match), 4),
            // Phase 4c: child_process
            // Phase 9d: node:net + node:dns
            ("__wjs_net_connect", Some(node::net::net_connect), 5),
            ("__wjs_net_bind", Some(node::net::net_bind), 3),
            ("__wjs_net_unhold", Some(node::net::net_unhold), 1),
            ("__wjs_net_fd", Some(node::net::net_fd), 1),
            ("__wjs_net_isip", Some(node::net::net_isip), 1),
            ("__wjs_net_listen", Some(node::net::net_listen), 3),
            ("__wjs_net_attach", Some(node::net::net_attach), 2),
            ("__wjs_net_write", Some(node::net::net_write), 2),
            ("__wjs_net_end", Some(node::net::net_end), 1),
            ("__wjs_net_destroy", Some(node::net::net_destroy), 1),
            // 10a：ref 真计数（net/dgram 共用）
            ("__wjs_net_ref", Some(node::net::net_ref), 1),
            ("__wjs_net_unref", Some(node::net::net_unref), 1),
            ("__wjs_dns_lookup", Some(node::dns::dns_lookup), 1),
            // 10d：dns 深件（hickory 全套；lookup 维持 std）
            ("__wjs_dns_query", Some(node::dns::dns_query), 2),
            // 10f：Resolver 定制查询（投递/轮询/遗忘，见 dns.rs job 表）
            ("__wjs_dns_job_start", Some(node::dns::dns_job_start), 6),
            ("__wjs_dns_job_poll", Some(node::dns::dns_job_poll), 1),
            ("__wjs_dns_job_forget", Some(node::dns::dns_job_forget), 1),
            ("__wjs_dns_servers_get", Some(node::dns::dns_servers_get), 0),
            ("__wjs_dns_servers_set", Some(node::dns::dns_servers_set), 1),
            ("__wjs_dns_order_get", Some(node::dns::dns_order_get), 0),
            ("__wjs_dns_order_set", Some(node::dns::dns_order_set), 1),
            // Phase 9d-6: node:tls（握手底座；读写复用 net_* natives）
            ("__wjs_tls_connect", Some(node::tls::tls_connect), 4),
            ("__wjs_tls_listen", Some(node::tls::tls_listen), 4),
            // Phase 9d-7: node:http2（hyper 直引；关闭复用 __wjs_net_destroy）
            // 10f 流式化：头/体/收尾/RST 分离（ChanBody 增量应答）
            ("__wjs_h2_listen", Some(node::http2::h2_listen), 4),
            ("__wjs_h2_connect", Some(node::http2::h2_connect), 4),
            ("__wjs_h2_open", Some(node::http2::h2_open), 4),
            ("__wjs_h2_open_trailers", Some(node::http2::h2_open_trailers), 3),
            ("__wjs_h2_respond", Some(node::http2::h2_respond), 4),
            ("__wjs_h2_data", Some(node::http2::h2_data), 3),
            ("__wjs_h2_end", Some(node::http2::h2_end), 3),
            ("__wjs_h2_reset", Some(node::http2::h2_reset), 3),
            // Phase 9e-1a: node:crypto 增量 Hash（oneshot 复用全局 __wjs_*）
            ("__wjs_crypto_hash_new", Some(node::crypto::crypto_hash_new), 1),
            ("__wjs_crypto_hash_update", Some(node::crypto::crypto_hash_update), 2),
            ("__wjs_crypto_hash_digest", Some(node::crypto::crypto_hash_digest), 1),
            ("__wjs_crypto_hash_copy", Some(node::crypto::crypto_hash_copy), 1),
            ("__wjs_crypto_hash_set_len", Some(node::crypto::crypto_hash_set_len), 2),
            // Phase 9e-1b: node:crypto 对称密码（CBC/CTR 流式 + ChaCha oneshot）
            ("__wjs_cipher_new", Some(node::crypto::cipher_new), 5),
            ("__wjs_cipher_update", Some(node::crypto::cipher_update), 2),
            ("__wjs_cipher_final", Some(node::crypto::cipher_final), 1),
            ("__wjs_cipher_chacha", Some(node::crypto::cipher_chacha), 6),
            // 10e: AES-CCM oneshot（ccm 0.6 直引）
            ("__wjs_ccm_crypt", Some(node::crypto::ccm_crypt_native), 7),
            // 10e: GCM 任意 iv（12B 走 crate，其余 J0 手工；WebCrypto 共用面不动）
            ("__wjs_gcm_anyiv", Some(node::crypto::gcm_anyiv), 5),
            // Phase 9e-1c: RSA v1.5 + DH/素性（签名/派生复用既有 natives）
            ("__wjs_rsa_encrypt_v15", Some(node::crypto::rsa_encrypt_v15), 2),
            ("__wjs_rsa_decrypt_v15", Some(node::crypto::rsa_decrypt_v15), 2),
            ("__wjs_dh_genkey", Some(node::crypto::dh_genkey), 3),
            ("__wjs_dh_secret", Some(node::crypto::dh_secret), 3),
            ("__wjs_prime_check", Some(node::crypto::prime_check), 2),
            ("__wjs_prime_gen", Some(node::crypto::prime_gen), 3),
            // Phase 9h-1: DSA（dsa 0.7 + hazmat；信封 JSON 桥）
            ("__wjs_dsa_generate", Some(crypto::dsa_generate), 2),
            ("__wjs_dsa_sign", Some(crypto::dsa_sign), 3),
            ("__wjs_dsa_verify", Some(crypto::dsa_verify), 4),
            ("__wjs_dsa_export", Some(crypto::dsa_export), 1),
            // Phase 9e-1c: RSA-SHA1 手工件（digest 0.10 版本面，§0.5 未批新行）
            ("__wjs_node_rsa_oaep", Some(node::crypto::node_rsa_oaep), 4),
            ("__wjs_node_rsa_oaep_flip", Some(node::crypto::node_rsa_oaep_flip), 4),
            ("__wjs_rsa_v15_flip", Some(node::crypto::rsa_v15_flip), 3),
            ("__wjs_rsa_raw", Some(node::crypto::rsa_raw), 3),
            ("__wjs_node_rsa_v15_sign", Some(node::crypto::node_rsa_v15_sign), 3),
            ("__wjs_node_rsa_v15_verify", Some(node::crypto::node_rsa_v15_verify), 4),
            // Phase 9e-1d: KDF + X509（全员树内轮子）
            ("__wjs_kdf_pbkdf2", Some(node::crypto::kdf_pbkdf2), 5),
            ("__wjs_kdf_scrypt", Some(node::crypto::kdf_scrypt), 7),
            ("__wjs_kdf_hkdf", Some(node::crypto::kdf_hkdf), 5),
            ("__wjs_kdf_argon2", Some(node::crypto::kdf_argon2), 9),
            ("__wjs_x509_parse", Some(node::crypto::x509_parse), 1),
            // Phase 9i-7: X509 checkIssued（名字 DER + AKID/SKID + keyUsage）
            ("__wjs_x509_check_issued", Some(node::crypto::x509_check_issued), 2),
            // Phase 9i-4: ml-kem（FIPS 203；ml-kem crate，种子形 PKCS#8/SPKI/封装面）
            ("__wjs_mlkem_gen", Some(node::crypto::mlkem_gen), 1),
            ("__wjs_mlkem_seed_from_pkcs8", Some(node::crypto::mlkem_seed_from_pkcs8), 1),
            ("__wjs_mlkem_kind_from_spki", Some(node::crypto::mlkem_kind_from_spki), 1),
            ("__wjs_mlkem_encaps", Some(node::crypto::mlkem_encaps), 2),
            ("__wjs_mlkem_decaps", Some(node::crypto::mlkem_decaps), 2),
            // Phase 9i-6: ml-dsa（FIPS 204；纯签名，种子形 PKCS#8/SPKI/Sign-Verify）
            ("__wjs_mldsa_gen", Some(node::crypto::mldsa_gen), 1),
            ("__wjs_mldsa_seed_from_pkcs8", Some(node::crypto::mldsa_seed_from_pkcs8), 1),
            ("__wjs_mldsa_kind_from_spki", Some(node::crypto::mldsa_kind_from_spki), 1),
            ("__wjs_mldsa_public", Some(node::crypto::mldsa_public), 1),
            ("__wjs_mldsa_sign", Some(node::crypto::mldsa_sign), 2),
            ("__wjs_mldsa_verify", Some(node::crypto::mldsa_verify), 3),
            // Phase 9e-4: inspector 会话求值（同线程嵌套 evaluate_script）
            ("__wjs_inspector_eval", Some(node::inspector::inspector_eval), 1),
            // Phase 9f-1: node:vm（同 Runtime 多 global；id 字符串形态）
            ("__wjs_vm_create", Some(node::vm::vm_create), 0),
            ("__wjs_vm_compile", Some(node::vm::vm_compile), 2),
            ("__wjs_vm_global", Some(node::vm::vm_global), 1),
            ("__wjs_vm_run", Some(node::vm::vm_run), 3),
            ("__wjs_vm_run_this", Some(node::vm::vm_run_this), 2),
            ("__wjs_vm_compile_fn", Some(node::vm::vm_compile_fn), 4),
            ("__wjs_vm_set", Some(node::vm::vm_set), 3),
            ("__wjs_vm_get", Some(node::vm::vm_get), 2),
            ("__wjs_vm_keys", Some(node::vm::vm_keys), 1),
            ("__wjs_vm_keys_all", Some(node::vm::vm_keys_all), 1),
            ("__wjs_vm_keys_count", Some(node::vm::vm_keys_count), 1),
            ("__wjs_vm_same", Some(node::vm::vm_same), 2),
            ("__wjs_vm_release", Some(node::vm::vm_release), 1),
            ("__wjs_vm_take_error", Some(node::vm::vm_take_error), 0),
            // Phase 9i-1: vm 模块系（SourceText；Synthetic 纯 JS）
            ("__wjs_vm_compile_mod", Some(node::vm::vm_mod_compile), 3),
            ("__wjs_vm_link", Some(node::vm::vm_mod_link), 1),
            ("__wjs_vm_evaluate", Some(node::vm::vm_mod_evaluate), 1),
            ("__wjs_vm_mod_ns", Some(node::vm::vm_mod_ns), 1),
            ("__wjs_vm_mod_release", Some(node::vm::vm_mod_release), 1),
            ("__wjs_vm_mod_settled", Some(node::vm::vm_mod_settled), 1),
            ("__wjs_vm_mod_deps", Some(node::vm::vm_mod_deps), 1),
            // Phase 9f-2: worker 消息通道（端口对/投递/线程身份/环境数据）
            ("__wjs_port_pair", Some(node::worker::port_pair), 0),
            ("__wjs_port_attach", Some(node::worker::port_attach), 2),
            ("__wjs_port_post", Some(node::worker::port_post), 2),
            ("__wjs_port_try_recv", Some(node::worker::port_try_recv), 1),
            ("__wjs_port_close", Some(node::worker::port_close), 1),
            ("__wjs_port_unref", Some(node::worker::port_unref), 1),
            ("__wjs_port_ref", Some(node::worker::port_ref), 1),
            // Phase 9i-2: 端口迁移（offer/accept 经邀约槽 + 转发器）+ BroadcastChannel
            ("__wjs_port_offer", Some(node::worker::port_offer), 1),
            ("__wjs_port_accept", Some(node::worker::port_accept), 1),
            ("__wjs_port_withdraw", Some(node::worker::port_withdraw), 1),
            ("__wjs_port_detach", Some(node::worker::port_detach), 1),
            ("__wjs_bc_sub", Some(node::worker::bc_sub), 1),
            ("__wjs_bc_unsub", Some(node::worker::bc_unsub), 1),
            ("__wjs_bc_pub", Some(node::worker::bc_pub), 3),
            ("__wjs_bc_try_recv", Some(node::worker::bc_try_recv), 1),
            ("__wjs_bc_flags", Some(node::worker::bc_flags), 2),
            ("__wjs_bc_attach", Some(node::worker::bc_attach), 2),
            ("__wjs_worker_is_main", Some(node::worker::worker_is_main), 0),
            ("__wjs_worker_thread_id", Some(node::worker::worker_thread_id), 0),
            ("__wjs_worker_name", Some(node::worker::worker_name), 0),
            ("__wjs_worker_is_fork", Some(node::worker::worker_is_fork), 0),
            ("__wjs_worker_parent", Some(node::worker::worker_parent), 0),
            ("__wjs_worker_data", Some(node::worker::worker_data), 0),
            ("__wjs_worker_env_snapshot", Some(node::worker::env_snapshot), 0),
            ("__wjs_worker_env_set", Some(node::worker::env_set), 2),
            ("__wjs_worker_env_get", Some(node::worker::env_get), 1),
            // Phase 9f-3: Worker（spawn/投递/终止/监听计数）
            ("__wjs_worker_spawn", Some(node::worker::worker_spawn), 3),
            ("__wjs_worker_attach", Some(node::worker::worker_attach), 2),
            ("__wjs_worker_post", Some(node::worker::worker_post), 2),
            ("__wjs_worker_terminate", Some(node::worker::worker_terminate), 1),
            ("__wjs_worker_set_ref", Some(node::worker::worker_set_ref), 2),
            ("__wjs_worker_tid", Some(node::worker::worker_tid), 1),
            ("__wjs_port_listen", Some(node::worker::port_listen), 1),
            ("__wjs_port_unlisten", Some(node::worker::port_unlisten), 1),
            ("__wjs_port_has_ref", Some(node::worker::port_has_ref), 1),
            // Phase 9g-1: node:quic（Endpoint/会话；流/数据报 9g-2）
            ("__wjs_quic_listen", Some(node::quic::quic_listen), 1),
            ("__wjs_quic_ep_addr", Some(node::quic::quic_ep_addr), 1),
            ("__wjs_quic_ep_close", Some(node::quic::quic_ep_close), 1),
            ("__wjs_quic_ep_attach", Some(node::quic::quic_ep_attach), 2),
            ("__wjs_quic_connect", Some(node::quic::quic_connect), 1),
            ("__wjs_quic_sess_attach", Some(node::quic::quic_sess_attach), 2),
            ("__wjs_quic_sess_info", Some(node::quic::quic_sess_info), 1),
            ("__wjs_quic_sess_stats", Some(node::quic::quic_sess_stats), 1),
            ("__wjs_quic_sess_close", Some(node::quic::quic_sess_close), 2),
            // Phase 9g-2: QUIC 流/数据报
            ("__wjs_quic_sess_open", Some(node::quic::quic_sess_open), 2),
            ("__wjs_quic_stream_attach", Some(node::quic::quic_stream_attach), 2),
            // Phase 9i-9: H3 分支（服务端 respond / 客户端 request）
            ("__wjs_quic_h3_respond", Some(node::quic::quic_h3_respond), 3),
            ("__wjs_quic_h3_request", Some(node::quic::quic_h3_request), 2),
            ("__wjs_quic_stream_write", Some(node::quic::quic_stream_write), 2),
            ("__wjs_quic_stream_finish", Some(node::quic::quic_stream_finish), 1),
            ("__wjs_quic_stream_reset", Some(node::quic::quic_stream_reset), 2),
            ("__wjs_quic_stream_stop", Some(node::quic::quic_stream_stop), 2),
            ("__wjs_quic_sess_send_dgram", Some(node::quic::quic_sess_send_dgram), 2),
            ("__wjs_quic_sess_max_dgram", Some(node::quic::quic_sess_max_dgram), 1),
            ("__wjs_dgram_bind", Some(node::dgram::dgram_bind), 4),
            ("__wjs_dgram_bind_sync", Some(node::dgram::dgram_bind_sync), 5),
            ("__wjs_dgram_send", Some(node::dgram::dgram_send), 4),
            // 10a：组播/广播/TTL/connect（JSON 单 native；id+op 包）
            ("__wjs_dgram_sockopt", Some(node::dgram::dgram_sockopt), 2),
            // buffer size 同步 get/setsockopt（fd 表登记于 bind 任务）
            ("__wjs_dgram_bufsize", Some(node::dgram::dgram_bufsize), 3),
            // Phase 9d-5: node:zlib（convenience 压缩面；流式类顺延）
            ("__wjs_zlib_deflate_lv", Some(node::zlib::zlib_deflate_lv), 2),
            ("__wjs_zlib_inflate", Some(node::zlib::zlib_inflate), 1),
            ("__wjs_zlib_deflate_raw", Some(node::zlib::zlib_deflate_raw), 2),
            ("__wjs_zlib_inflate_raw", Some(node::zlib::zlib_inflate_raw), 1),
            ("__wjs_zlib_gzip", Some(node::zlib::zlib_gzip), 2),
            ("__wjs_zlib_gunzip", Some(node::zlib::zlib_gunzip), 1),
            ("__wjs_zlib_unzip", Some(node::zlib::zlib_unzip), 1),
            // 10a：crc32（ISO-HDLC 自实现；flate2::Crc 不收 seed）
            ("__wjs_zlib_crc32", Some(node::zlib::zlib_crc32), 2),
            // 10f 欠账轮：增量流式编解码状态机（flush 档位/premature-end/truncated）
            ("__wjs_zlib_stream_new", Some(node::zlib::zlib_stream_new), 5),
            ("__wjs_zlib_stream_feed", Some(node::zlib::zlib_stream_feed), 3),
            ("__wjs_zlib_stream_out", Some(node::zlib::zlib_stream_out), 1),
            ("__wjs_zlib_stream_free", Some(node::zlib::zlib_stream_free), 1),
            ("__wjs_zlib_stream_reset", Some(node::zlib::zlib_stream_reset), 1),
            ("__wjs_cp_exec", Some(node::child::cp_exec), 2),
            ("__wjs_cp_spawn", Some(node::child::cp_spawn), 3),
            // Phase 4d: 异步 spawn（c-4x 加 pipe：stdin 写/关 natives）
            ("__wjs_spawn_start", Some(node::child::spawn_start), 5),
            ("__wjs_child_kill", Some(node::child::child_kill), 2),
            ("__wjs_child_pid", Some(node::child::child_pid), 1),
            ("__wjs_child_stdin_write", Some(node::child::child_stdin_write), 2),
            ("__wjs_child_stdin_close", Some(node::child::child_stdin_close), 1),
            // Phase 4d: require（裸 native，直调保调用方定位；附属见 NODE_PRELUDE）
            ("require", Some(node::require::require_native), 1),
            ("__wjs_require_resolve", Some(node::require::require_resolve), 1),
            ("__wjs_require_main_url", Some(node::require::require_main_url), 0),
            // Phase 9j: node:module（createRequire 显式 base 底座 + 内建列表）
            ("__wjs_require_from", Some(node::require::require_from), 2),
            ("__wjs_require_resolve_from", Some(node::require::require_resolve_from), 2),
            ("__wjs_cjs_compile", Some(node::require::cjs_compile), 3),
            ("__wjs_builtin_modules", Some(node::require::builtin_modules_json), 0),
            // Phase 9j: CJS 互操作垫片（import 命中 CJS → export default）
            ("__wjs_require_cjs_by_url", Some(node::require::require_cjs_by_url), 1),
            ("__wjs_ws_connect", Some(ws::ws_connect), 3),
            ("__wjs_ws_send", Some(ws::ws_send), 3),
            ("__wjs_ws_close", Some(ws::ws_close), 3),
            // Phase 7-e4: bun:sqlite（同步语义，worker 线程见 bun/sqlite.rs）
            ("__wjs_sqlite_open", Some(bun::sqlite::sqlite_open), 1),
            ("__wjs_sqlite_exec", Some(bun::sqlite::sqlite_exec), 2),
            ("__wjs_sqlite_run", Some(bun::sqlite::sqlite_run), 4),
            ("__wjs_sqlite_rows", Some(bun::sqlite::sqlite_rows), 4),
            ("__wjs_sqlite_txn", Some(bun::sqlite::sqlite_txn), 1),
            ("__wjs_sqlite_close", Some(bun::sqlite::sqlite_close), 1),
            // 10d：node:sqlite（turso 底座；DatabaseSync/StatementSync）
            ("__wjs_nsqlite_open", Some(node::sqlite::nsqlite_open), 1),
            ("__wjs_nsqlite_exec", Some(node::sqlite::nsqlite_exec), 2),
            ("__wjs_nsqlite_run", Some(node::sqlite::nsqlite_run), 4),
            ("__wjs_nsqlite_rows", Some(node::sqlite::nsqlite_rows), 4),
            ("__wjs_nsqlite_cols", Some(node::sqlite::nsqlite_cols), 2),
            ("__wjs_nsqlite_close", Some(node::sqlite::nsqlite_close), 1),
            // Phase 7-e6: bun:ffi（动态调用引擎见 ffi.rs 头注；UNSAFE-BOUNDARY 密集区）
            ("__wjs_ffi_dlopen", Some(bun::ffi::ffi_dlopen), 2),
            ("__wjs_ffi_ptr_str", Some(bun::ffi::ffi_ptr_str), 1),
            ("__wjs_ffi_ptr_view", Some(bun::ffi::ffi_ptr_view), 1),
            ("__wjs_ffi_call", Some(bun::ffi::ffi_call), 2),
            ("__wjs_ffi_cstring", Some(bun::ffi::ffi_cstring), 1),
            ("__wjs_ffi_bytes", Some(bun::ffi::ffi_bytes), 2),
        ];
        // 重名 native 会静默覆盖（如 __wjs_env_* 曾被 worker 环境数据顶掉，
        // process.env 全坏——debug 期即炸，见 9f-2。局部表：每会话 define_all
        // 都跑一次，判重集必须局部（static 跨会话误报）。
        #[cfg(debug_assertions)]
        let mut seen_native: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for (name, native, nargs) in web {
            let cname = CString::new(*name).expect("no NUL");
            #[cfg(debug_assertions)]
            debug_assert!(
                seen_native.insert(*name),
                "duplicate builtin native name: {name}"
            );
            if mozjs::jsapi::JS_DefineFunction(rcx, raw_handle(&global), cname.as_ptr(), *native, *nargs, 0)
                .is_null()
            {
                report_error(cx, "failed to define builtin");
                return Err(Error::Other(format!("failed to define builtin {name}")));
            }
        }

        // console 对象 + 方法
        let console = mozjs::jsapi::JS_NewPlainObject(rcx);
        if console.is_null() {
            return Err(Error::Other("failed to create console object".into()));
        }
        rooted!(in(rcx) let console_root: *mut JSObject = console);
        let methods: &[(&str, JSNative, u32)] = &[
            ("log", Some(console::log), 0),
            ("info", Some(console::info), 0),
            ("warn", Some(console::warn), 0),
            ("error", Some(console::error), 0),
            ("debug", Some(console::debug), 0),
            ("trace", Some(console::trace), 0),
            ("dir", Some(console::dir), 0),
            ("assert", Some(console::assert), 0),
            ("count", Some(console::count), 1),
            ("countReset", Some(console::count_reset), 1),
            ("time", Some(console::time), 1),
            ("timeLog", Some(console::time_log), 1),
            ("timeEnd", Some(console::time_end), 1),
            ("group", Some(console::group), 0),
            ("groupEnd", Some(console::group_end), 0),
            ("clear", Some(console::clear), 0),
        ];
        for (name, native, nargs) in methods {
            let cname = CString::new(*name).expect("no NUL");
            if mozjs::jsapi::JS_DefineFunction(
                rcx,
                raw_handle(console_root.as_ptr()),
                cname.as_ptr(),
                *native,
                *nargs,
                0,
            )
            .is_null()
            {
                return Err(Error::Other(format!("failed to define console.{name}")));
            }
        }
        rooted!(in(rcx) let console_val = ObjectValue(console));
        // SAFETY: 定义 console 属性（5 参简化形态）
        let ok = mozjs::jsapi::JS_DefineProperty(
            rcx,
            raw_handle(&global),
            c"console".as_ptr(),
            raw_handle(console_val.as_ptr()),
            JSPROP_ENUMERATE as u32,
        );
        if !ok {
            return Err(Error::Other("failed to define global console".into()));
        }

        // structuredClone
        let cname = c"structuredClone";
        let clone_native: JSNative = Some(clone::structured_clone);
        if mozjs::jsapi::JS_DefineFunction(
            rcx,
            raw_handle(&global),
            cname.as_ptr(),
            clone_native,
            1,
            0,
        )
        .is_null()
        {
            return Err(Error::Other("failed to define structuredClone".into()));
        }
    }
    Ok(())
}
