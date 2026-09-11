//! ACME 黑盒测试(对齐 src/acme.rs)。

mod common;

use common::*;

#[test]
fn serve_acme_dry_run_plan() {
    // 正常：dry-run 打印 domain/email/directory/cache，不碰网络不绑端口。
    let dir = assert_fs::TempDir::new().unwrap();
    let cache = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .args([
            "--serve",
            ".",
            "--acme-domain",
            "winterjs.bemly.moe",
            "--acme-email",
            "a@b.c",
            "--acme-cache",
            cache.path().to_str().unwrap(),
            "--dry-run",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("winterjs.bemly.moe"), "stdout: {stdout}");
    assert!(stdout.contains("staging"), "default staging: {stdout}");
    // 生产旗标切换目录
    let out = winterjs()
        .args([
            "--serve",
            ".",
            "--acme-email",
            "a@b.c",
            "--acme-production",
            "--dry-run",
        ])
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
        !stdout.contains("staging") && stdout.contains("acme-v02"),
        "production: {stdout}"
    );
    dir.close().unwrap();
    cache.close().unwrap();
}

#[test]
fn serve_acme_cert_conflict() {
    // 报错：--acme-* 与 --cert/--key 互斥（exit=1，可读）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .args(["--serve", ".", "--acme-email", "a@b.c", "--cert", "c.pem"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--acme-"), "stderr:\n{err}");
    dir.close().unwrap();
}

// ── loader http(s)（远端导入；stub 回环，不碰外网）────────────────────────────
