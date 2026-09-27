//! `winterjs --remove/--uninstall` 黑盒（正常 + 报错 + 边界；纯本地方案，无网络）。

use crate::common::*;
use assert_fs::prelude::*;

/// 假工程：node_modules/{left-pad,@s/p} + .bin 溯源链接 + lockfile 三条目。
fn fake_project(dir: &assert_fs::TempDir) {
    let nm = dir.child("node_modules");
    nm.child("left-pad").create_dir_all().unwrap();
    nm.child("left-pad/package.json")
        .write_str(r#"{"name":"left-pad","version":"1.3.0","bin":{"lp":"cli.js"}}"#)
        .unwrap();
    nm.child("@s/p").create_dir_all().unwrap();
    let bin = nm.child(".bin");
    bin.create_dir_all().unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("../left-pad", bin.path().join("lp")).unwrap();
        std::os::unix::fs::symlink("../@s/p", bin.path().join("sp")).unwrap();
    }
    #[cfg(not(unix))]
    {
        bin.child("lp").write_str("x").unwrap();
        bin.child("sp").write_str("x").unwrap();
    }
    dir.child("winterjs-lock.json")
        .write_str(r#"{"version":1,"packages":{"left-pad":{"version":"1.3.0"},"@s/p":{"version":"2.0.0"}},"manifest":null}"#)
        .unwrap();
}

#[test]
fn remove_prunes_dir_bins_and_lockfile() {
    // 正常：目录 + lockfile 条目没了，stdout 报 removed。
    let dir = assert_fs::TempDir::new().unwrap();
    fake_project(&dir);
    let out = winterjs()
        .args(["--remove", "left-pad"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("removed left-pad"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(!dir.path().join("node_modules/left-pad").exists());
    let lock = std::fs::read_to_string(dir.path().join("winterjs-lock.json")).unwrap();
    assert!(!lock.contains("left-pad"), "lock: {lock}");
    assert!(lock.contains("@s/p"), "lock: {lock}");
    dir.close().unwrap();
}

#[test]
fn remove_missing_is_error_and_changes_nothing() {
    // 报错：缺失整单报错不动盘（all-or-nothing）。
    let dir = assert_fs::TempDir::new().unwrap();
    fake_project(&dir);
    let out = winterjs()
        .args(["--remove", "left-pad", "ghost"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("not installed: ghost"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(dir.path().join("node_modules/left-pad").exists());
    dir.close().unwrap();
}

#[test]
fn remove_dry_run_and_scope_and_uninstall() {
    // 边界：--dry-run 不动盘；scope 包删完父空即收；--uninstall 走全局根。
    let dir = assert_fs::TempDir::new().unwrap();
    fake_project(&dir);
    let out = winterjs()
        .args(["--remove", "left-pad", "--dry-run"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("would remove left-pad@1.3.0"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(dir.path().join("node_modules/left-pad").exists());

    let out = winterjs()
        .args(["--remove", "@s/p"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(!dir.path().join("node_modules/@s").exists(), "空 scope 父应顺手删");

    // --uninstall 经 WINTERJS_GLOBAL_ROOT 隔离（与 --install 同根，见 pm::global_root）。
    let global = assert_fs::TempDir::new().unwrap();
    std::fs::create_dir_all(global.path().join("node_modules/gpkg")).unwrap();
    let out = winterjs()
        .args(["--uninstall", "gpkg"])
        .env("WINTERJS_GLOBAL_ROOT", global.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(!global.path().join("node_modules/gpkg").exists());
    // 本地 untouched（全局动作不碰 cwd）。
    assert!(dir.path().join("node_modules/left-pad").exists());
    dir.close().unwrap();
    global.close().unwrap();
}

#[test]
fn remove_rejects_registry_and_bad_names() {
    // 边界：--registry 非 remove 归属（错配即错）；`..` 包名即错。
    let dir = assert_fs::TempDir::new().unwrap();
    fake_project(&dir);
    let out = winterjs()
        .args(["--remove", "left-pad", "--registry", "https://x.invalid"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("--registry only works with"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = winterjs()
        .args(["--remove", "../esc"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("bad package name"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    dir.close().unwrap();
}
