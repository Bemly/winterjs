//! tests/pm/npmrc.rs — npmrc 读写（对齐 src/pm/npmrc.rs）。

use crate::common::*;
use assert_fs::prelude::*;
use super::helpers::*;

#[test]
fn phase5_npmrc_registry_mirror() {
    // 正常：项目 `.npmrc` 的 registry 生效（不传 --registry 也命中 stub）。
    let port = serve_registry();
    let reg = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    let home = assert_fs::TempDir::new().unwrap();
    dir.child(".npmrc")
        .write_str(&format!("registry={reg}/\n"))
        .unwrap();
    let out = stdout_of(
        winterjs()
            .args(["--add", "left-pad@^1.0.0", "--dry-run"])
            .env("HOME", home.path())
            .env_remove("NPM_CONFIG_REGISTRY")
            .env_remove("npm_config_registry")
            .current_dir(dir.path()),
    );
    assert_eq!(
        out,
        format!("left-pad@1.3.0 http://127.0.0.1:{port}/left-pad/-/left-pad-1.3.0.tgz\n"),
        "npmrc mirror: {out}"
    );
    dir.close().unwrap();
    home.close().unwrap();
}

#[test]
fn phase5_npmrc_bad_registry_errors() {
    // 报错：`.npmrc` 指向连不上的 registry，exit=1 且可读（不碰外网，9 端口必拒）。
    let dir = assert_fs::TempDir::new().unwrap();
    let home = assert_fs::TempDir::new().unwrap();
    dir.child(".npmrc")
        .write_str("registry=http://127.0.0.1:9/\n")
        .unwrap();
    let out = winterjs()
        .args(["--add", "left-pad@^1.0.0", "--dry-run"])
        .env("HOME", home.path())
        .env_remove("NPM_CONFIG_REGISTRY")
        .env_remove("npm_config_registry")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("registry"), "stderr: {stderr}");
    dir.close().unwrap();
    home.close().unwrap();
}
