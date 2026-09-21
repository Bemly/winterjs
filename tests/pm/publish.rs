//! tests/pm/publish.rs — 发布（对齐 src/pm/publish.rs）。

use crate::common::*;
use assert_fs::prelude::*;

#[test]
fn phase5_publish_dry_run_ok() {
    // 正常：`publish --dry-run` 打印名@版/registry/files，不碰网络。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("package.json")
        .write_str(r#"{"name":"pub-pkg","version":"1.2.3","license":"MIT"}"#)
        .unwrap();
    dir.child("index.js").write_str("exports.v = 1;\n").unwrap();
    let out = stdout_of(
        winterjs()
            .args([
                "--publish",
                "--dry-run",
                "--registry",
                "http://127.0.0.1:9/",
            ])
            .current_dir(dir.path()),
    );
    assert!(out.contains("pub-pkg@1.2.3"), "summary: {out}");
    assert!(
        out.contains("registry: http://127.0.0.1:9/"),
        "summary: {out}"
    );
    assert!(out.contains("files:"), "summary: {out}");
    dir.close().unwrap();
}

#[test]
fn phase5_publish_manifest_errors() {
    // 报错：缺名 / 坏 license，皆 exit=1 且可读。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("package.json")
        .write_str(r#"{"version":"1.0.0"}"#)
        .unwrap();
    let out = winterjs()
        .args(["--publish", "--dry-run"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("no name"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    dir.child("package.json")
        .write_str(r#"{"name":"p","version":"1.0.0","license":"Not-A-License!!"}"#)
        .unwrap();
    let out = winterjs()
        .args(["--publish", "--dry-run"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("license"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    dir.close().unwrap();
}

#[test]
fn pm_publish_put_end_to_end() {
    // 真发布闭环：打包→PUT→200；stub 验方法/路径/Bearer/包体形状；无 token 指路 login。
    use std::sync::{Arc, Mutex};
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen2 = seen.clone();
    let port = serve_http(1, move |head, body| {
        let mut log = seen2.lock().unwrap();
        let line = head.lines().next().unwrap_or("").to_owned();
        log.push(line.clone());
        let auth = head
            .lines()
            .find_map(|l| {
                l.strip_prefix("authorization:")
                    .or_else(|| l.strip_prefix("Authorization:"))
            })
            .unwrap_or("")
            .trim()
            .to_owned();
        log.push(format!("auth:{auth}"));
        // 包体形状：versions + dist-tags + _attachments 带 b64
        let v: serde_json::Value =
            serde_json::from_str(&String::from_utf8_lossy(&body)).unwrap_or_default();
        let ok = v.get("versions").and_then(|x| x.get("1.0.0")).is_some()
            && v.get("dist-tags")
                .and_then(|x| x.get("latest"))
                .and_then(|x| x.as_str())
                == Some("1.0.0")
            && v.get("_attachments")
                .and_then(|a| a.as_object())
                .is_some_and(|m| m.len() == 1);
        log.push(format!("body-ok:{ok}"));
        (
            200,
            vec![("content-type", "application/json".into())],
            b"{}".to_vec(),
        )
    });
    let reg = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("package.json")
        .write_str(r#"{"name":"my-pkg","version":"1.0.0","license":"MIT"}"#)
        .unwrap();
    dir.child("index.js").write_str("exports.v = 1;\n").unwrap();
    dir.child(".npmrc")
        .write_str(&format!("registry={reg}\n//127.0.0.1/:_authToken=sekret\n"))
        .unwrap();
    let out = winterjs()
        .args(["--publish", "--registry", &reg])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("published my-pkg@1.0.0"),
        "stdout: {stdout}"
    );
    let log = seen.lock().unwrap();
    assert!(
        log.iter().any(|l| l.starts_with("PUT /my-pkg ")),
        "method/path: {log:?}"
    );
    assert!(
        log.iter().any(|l| l == "auth:Bearer sekret"),
        "auth: {log:?}"
    );
    assert!(log.iter().any(|l| l == "body-ok:true"), "body: {log:?}");
    dir.close().unwrap();
}

#[test]
fn pm_publish_errors() {
    // 报错：无 token 指路 login（exit=1）；dry-run 不碰网络。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("package.json")
        .write_str(r#"{"name":"p","version":"1.0.0","license":"MIT"}"#)
        .unwrap();
    dir.child("index.js").write_str("1").unwrap();
    // 无 token（HOME 隔离防污染真机 npmrc）
    let home = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .args(["--publish", "--registry", "http://127.0.0.1:9/"])
        .env("HOME", home.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--login"), "stderr:\n{err}");
    // dry-run 不碰网络（坏 registry 也过）
    let out = winterjs()
        .args([
            "--publish",
            "--dry-run",
            "--registry",
            "http://127.0.0.1:9/",
        ])
        .env("HOME", home.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8(out.stdout).unwrap().contains("p@1.0.0"));
    dir.close().unwrap();
    home.close().unwrap();
}

// ── ACME 自动证书（dry-run 只校验打印，不碰网络；真签发需公网 :80 + DNS）─────
