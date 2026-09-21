//! tests/pm/git.rs — git 依赖（对齐 src/pm/git.rs）。

use crate::common::*;
use assert_fs::prelude::*;
use super::helpers::*;

#[test]
fn phase5_git_dry_run_local() {
    // 正常：`git+file://` dry-run 解析出 commit（40 hex），不落地。
    let repo = make_git_repo("git-pkg", true);
    let url = format!("file://{}", repo.path().display());
    let home = assert_fs::TempDir::new().unwrap();
    let dir = assert_fs::TempDir::new().unwrap();
    let out = stdout_of(
        winterjs()
            .args(["--add", &format!("git-pkg@git+{url}#v1.0.0"), "--dry-run"])
            .env("HOME", home.path())
            .env_remove("NPM_CONFIG_REGISTRY")
            .env_remove("npm_config_registry")
            .current_dir(dir.path()),
    );
    assert!(
        out.starts_with(&format!("git-pkg@git+{url}#")),
        "dry-run: {out}"
    );
    let commit = out.trim().rsplit('#').next().unwrap();
    assert_eq!(commit.len(), 40, "commit hex: {out}");
    dir.close().unwrap();
    home.close().unwrap();
}

#[test]
fn phase5_git_unknown_rev_errors() {
    // 报错：未知 rev，exit=1 且可读。
    let repo = make_git_repo("git-pkg", false);
    let url = format!("file://{}", repo.path().display());
    let home = assert_fs::TempDir::new().unwrap();
    let dir = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .args([
            "--add",
            &format!("git-pkg@git+{url}#no-such-ref"),
            "--dry-run",
        ])
        .env("HOME", home.path())
        .env_remove("NPM_CONFIG_REGISTRY")
        .env_remove("npm_config_registry")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no-such-ref"), "stderr: {stderr}");
    dir.close().unwrap();
    home.close().unwrap();
}

#[test]
fn phase5_git_bare_spec_reads_name() {
    // 边界：裸 `git+…` 无显式名，从源 package.json 读名。
    let repo = make_git_repo("bare-pkg", false);
    let url = format!("file://{}", repo.path().display());
    let home = assert_fs::TempDir::new().unwrap();
    let dir = assert_fs::TempDir::new().unwrap();
    let out = stdout_of(
        winterjs()
            .args(["--add", &format!("git+{url}"), "--dry-run"])
            .env("HOME", home.path())
            .env_remove("NPM_CONFIG_REGISTRY")
            .env_remove("npm_config_registry")
            .current_dir(dir.path()),
    );
    assert!(
        out.starts_with(&format!("bare-pkg@git+{url}#")),
        "bare name: {out}"
    );
    dir.close().unwrap();
    home.close().unwrap();
}

#[test]
fn phase5_git_end_to_end_local() {
    // 真装闭环：本地 git 装完 `require` 可跑 + lockfile 记 `git+…#commit`。
    let repo = make_git_repo("git-e2e", false);
    let url = format!("file://{}", repo.path().display());
    let home = assert_fs::TempDir::new().unwrap();
    let cache = assert_fs::TempDir::new().unwrap();
    let dir = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .arg("--add")
        .arg(format!("git+{url}"))
        .env("HOME", home.path())
        .env("WINTERJS_CACHE", cache.path())
        .env_remove("NPM_CONFIG_REGISTRY")
        .env_remove("npm_config_registry")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        dir.path()
            .join("node_modules/git-e2e/package.json")
            .is_file()
    );
    assert!(
        !dir.path().join("node_modules/git-e2e/.git").exists(),
        ".git must not land"
    );
    let lock = std::fs::read_to_string(dir.path().join("winterjs-lock.json")).unwrap();
    assert!(
        lock.contains("\"git-e2e\"") && lock.contains(&format!("git+{url}#")),
        "lock: {lock}"
    );
    let app = dir.child("app.cjs");
    app.write_str("const t = require(\"git-e2e\");\nconsole.log(t.add(19, 23));\n")
        .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(app.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "42\n");
    dir.close().unwrap();
    home.close().unwrap();
    cache.close().unwrap();
}
