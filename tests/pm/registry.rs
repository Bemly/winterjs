//! tests/pm/registry.rs — registry/login（对齐 src/pm/registry.rs）。

use crate::common::*;
use assert_fs::prelude::*;
use super::helpers::*;

#[test]
fn phase5_registry_flag_overrides_npmrc() {
    // 边界：`--registry` flag 覆盖坏掉的 `.npmrc`（优先级 flag > npmrc）。
    let port = serve_registry();
    let reg = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    let home = assert_fs::TempDir::new().unwrap();
    dir.child(".npmrc")
        .write_str("registry=http://127.0.0.1:9/\n")
        .unwrap();
    let out = stdout_of(
        winterjs2()
            .args(["--add", "left-pad@^1.0.0", "--dry-run", "--registry"])
            .arg(&reg)
            .env("HOME", home.path())
            .env_remove("NPM_CONFIG_REGISTRY")
            .env_remove("npm_config_registry")
            .current_dir(dir.path()),
    );
    assert_eq!(
        out,
        format!("left-pad@1.3.0 http://127.0.0.1:{port}/left-pad/-/left-pad-1.3.0.tgz\n"),
        "flag override: {out}"
    );
    dir.close().unwrap();
    home.close().unwrap();
}

#[test]
fn phase5_npm_config_registry_env_overrides_npmrc() {
    // 边界：`NPM_CONFIG_REGISTRY` env 覆盖坏掉的 `.npmrc`（优先级 env > npmrc）。
    let port = serve_registry();
    let reg = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    let home = assert_fs::TempDir::new().unwrap();
    dir.child(".npmrc")
        .write_str("registry=http://127.0.0.1:9/\n")
        .unwrap();
    let out = stdout_of(
        winterjs2()
            .args(["--add", "left-pad@^1.0.0", "--dry-run"])
            .env("HOME", home.path())
            .env("NPM_CONFIG_REGISTRY", &reg)
            .env_remove("npm_config_registry")
            .current_dir(dir.path()),
    );
    assert_eq!(
        out,
        format!("left-pad@1.3.0 http://127.0.0.1:{port}/left-pad/-/left-pad-1.3.0.tgz\n"),
        "env override: {out}"
    );
    dir.close().unwrap();
    home.close().unwrap();
}

#[test]
fn phase5_login_token_writes_npmrc() {
    // 正常：`login --token` 把 token 行写进 `$HOME/.npmrc`（其他行保留）。
    let home = assert_fs::TempDir::new().unwrap();
    home.child(".npmrc")
        .write_str("registry=http://127.0.0.1:4873/\n")
        .unwrap();
    let dir = assert_fs::TempDir::new().unwrap();
    let out = winterjs2()
        .args([
            "--login",
            "--token",
            "sekret",
            "--registry",
            "http://127.0.0.1:4873/",
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
    let npmrc = std::fs::read_to_string(home.path().join(".npmrc")).unwrap();
    assert!(
        npmrc.contains("//127.0.0.1/:_authToken=sekret"),
        "npmrc: {npmrc}"
    );
    assert!(
        npmrc.contains("registry=http://127.0.0.1:4873/"),
        "npmrc: {npmrc}"
    );
    dir.close().unwrap();
    home.close().unwrap();
}

#[test]
fn phase5_login_oauth_prints_url() {
    // 边界：`login --oauth` 打印授权 URL（headless 下浏览器打不开也不失败）。
    let home = assert_fs::TempDir::new().unwrap();
    let dir = assert_fs::TempDir::new().unwrap();
    let out = stdout_of(
        winterjs2()
            .args(["--login", "--oauth", "--registry", "http://127.0.0.1:4873/"])
            .env("HOME", home.path())
            .current_dir(dir.path()),
    );
    assert!(
        out.contains("http://127.0.0.1:4873/oauth/authorize?"),
        "url: {out}"
    );
    assert!(out.contains("--token"), "hint: {out}");
    dir.close().unwrap();
    home.close().unwrap();
}
