//! tests/pm/lifecycle.rs — 生命周期脚本（对齐 src/pm/lifecycle.rs）。

use crate::common::*;

#[test]
fn phase5_lifecycle_runs_in_order() {
    // preinstall → install → postinstall 按序跑，cwd 即包目录。
    use base64::Engine as _;
    use sha2::Digest as _;
    let tgz = make_tgz(&[
        (
            "package.json",
            br#"{"name":"life-pkg","version":"1.0.0","scripts":{"preinstall":"printf '%s' pre >> order.txt","install":"printf '%s' \"$npm_lifecycle_event\" >> order.txt","postinstall":"printf '%s' post >> order.txt"}}"#,
        ),
        ("index.js", b"exports.v = 1;\n"),
    ]);
    let integrity = format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&tgz))
    );
    let tgz_holder = std::sync::Arc::new(tgz);
    let int_holder = std::sync::Arc::new(integrity);
    let port = serve_http(2, move |head, _body| {
        let line = head.lines().next().unwrap_or("").to_owned();
        let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
        let port = head
            .lines()
            .find_map(|l| l.strip_prefix("Host:").or_else(|| l.strip_prefix("host:")))
            .and_then(|v| v.trim().split(':').nth(1))
            .unwrap_or("")
            .to_owned();
        if path == "/life-pkg" {
            let body = serde_json::json!({
                "name": "life-pkg",
                "dist-tags": { "latest": "1.0.0" },
                "versions": { "1.0.0": {
                    "dist": {
                        "tarball": format!("http://127.0.0.1:{port}/life-pkg/-/life-pkg-1.0.0.tgz"),
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
        if path == "/life-pkg/-/life-pkg-1.0.0.tgz" {
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
    let out = winterjs2()
        .arg("--add")
        .arg("life-pkg")
        .arg("--registry")
        .arg(&reg)
        .env("WINTERJS2_CACHE", cache.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let order =
        std::fs::read_to_string(dir.path().join("node_modules/life-pkg/order.txt")).unwrap();
    assert_eq!(order, "preinstallpost", "lifecycle order: {order}");
    dir.close().unwrap();
    cache.close().unwrap();
}

#[test]
fn phase5_lifecycle_failure_breaks_install() {
    // lifecycle 非零退出即安装失败（可读错误）。
    use base64::Engine as _;
    use sha2::Digest as _;
    let tgz = make_tgz(&[
        (
            "package.json",
            br#"{"name":"badlife","version":"1.0.0","scripts":{"postinstall":"exit 3"}}"#,
        ),
        ("index.js", b"exports.v = 1;\n"),
    ]);
    let integrity = format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&tgz))
    );
    let tgz_holder = std::sync::Arc::new(tgz);
    let int_holder = std::sync::Arc::new(integrity);
    let port = serve_http(2, move |head, _body| {
        let line = head.lines().next().unwrap_or("").to_owned();
        let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
        let port = head
            .lines()
            .find_map(|l| l.strip_prefix("Host:").or_else(|| l.strip_prefix("host:")))
            .and_then(|v| v.trim().split(':').nth(1))
            .unwrap_or("")
            .to_owned();
        if path == "/badlife" {
            let body = serde_json::json!({
                "name": "badlife",
                "dist-tags": { "latest": "1.0.0" },
                "versions": { "1.0.0": {
                    "dist": {
                        "tarball": format!("http://127.0.0.1:{port}/badlife/-/badlife-1.0.0.tgz"),
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
        if path == "/badlife/-/badlife-1.0.0.tgz" {
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
    let out = winterjs2()
        .arg("--add")
        .arg("badlife")
        .arg("--registry")
        .arg(&reg)
        .env("WINTERJS2_CACHE", cache.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("postinstall"), "stderr: {stderr}");
    dir.close().unwrap();
    cache.close().unwrap();
}
