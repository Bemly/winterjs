//! tests/pm/cache.rs — 缓存（对齐 src/pm/cache.rs）。

use crate::common::*;

#[test]
fn phase5_cache_second_install_hits_cache() {
    // 二次安装全命中缓存：tarball 只下一次，第二次删 node_modules 重装仍成功，
    // 此时 stub 的 tarball 端点已翻为 404（若回源必败），证明走缓存。
    use base64::Engine as _;
    use sha2::Digest as _;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let tgz = make_tgz(&[
        (
            "package.json",
            br#"{"name":"cached-pkg","version":"1.0.0","main":"index.js"}"#,
        ),
        ("index.js", b"exports.v = 1;\n"),
    ]);
    let integrity = format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&tgz))
    );
    let tgz_holder = std::sync::Arc::new(tgz);
    let int_holder = std::sync::Arc::new(integrity);
    let tarball_hits = std::sync::Arc::new(AtomicUsize::new(0));
    let hits = tarball_hits.clone();
    // 首次 2 请求（packument+tarball），二次 1 请求（packument，tarball 必须零回源）。
    let port = serve_http(3, move |head, _body| {
        let line = head.lines().next().unwrap_or("").to_owned();
        let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
        let port = head
            .lines()
            .find_map(|l| l.strip_prefix("Host:").or_else(|| l.strip_prefix("host:")))
            .and_then(|v| v.trim().split(':').nth(1))
            .unwrap_or("")
            .to_owned();
        if path == "/cached-pkg" {
            let body = serde_json::json!({
                "name": "cached-pkg",
                "dist-tags": { "latest": "1.0.0" },
                "versions": { "1.0.0": {
                    "dist": {
                        "tarball": format!("http://127.0.0.1:{port}/cached-pkg/-/cached-pkg-1.0.0.tgz"),
                        "integrity": *int_holder,
                    },
                    "dependencies": {},
                } },
            })
            .to_string();
            return (
                200,
                vec![("content-type", "application/json".into())],
                body.into_bytes(),
            );
        }
        if path == "/cached-pkg/-/cached-pkg-1.0.0.tgz" {
            let n = hits.fetch_add(1, Ordering::SeqCst);
            if n >= 1 {
                return (404, vec![], b"gone".to_vec());
            }
            return (
                200,
                vec![("content-type", "application/octet-stream".into())],
                (*tgz_holder).clone(),
            );
        }
        (404, vec![], b"nope".to_vec())
    });
    let reg = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    let cache = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .arg("--add")
        .arg("cached-pkg")
        .arg("--registry")
        .arg(&reg)
        .env("WINTERJS_CACHE", cache.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "first: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(tarball_hits.load(Ordering::SeqCst), 1);
    // 缓存文件已落（pkgs/*.tgz）。
    let cached: Vec<_> = std::fs::read_dir(cache.path().join("pkgs"))
        .unwrap()
        .collect();
    assert_eq!(cached.len(), 1, "cache dir should hold one tgz");
    // 删 node_modules 模拟二次安装（缓存保留）。
    std::fs::remove_dir_all(dir.path().join("node_modules")).unwrap();
    let out = winterjs()
        .arg("--add")
        .arg("cached-pkg")
        .arg("--registry")
        .arg(&reg)
        .env("WINTERJS_CACHE", cache.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "second (cache hit): {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        tarball_hits.load(Ordering::SeqCst),
        1,
        "tarball must not be re-downloaded"
    );
    assert!(
        dir.path()
            .join("node_modules/cached-pkg/package.json")
            .is_file()
    );
    dir.close().unwrap();
    cache.close().unwrap();
}
