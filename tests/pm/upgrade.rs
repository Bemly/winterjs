//! tests/pm/upgrade.rs — 升级（对齐 src/pm/upgrade.rs）。

use crate::common::*;

#[test]
fn phase5_upgrade_dry_run_reports_version() {
    // 正常：`upgrade --dry-run` 打印当前版 + 渠道，不碰网络。
    let out = stdout_of(
        winterjs()
            .args(["--upgrade", "--dry-run"])
            .env_remove("WINTERJS_UPDATE_GITHUB"),
    );
    assert!(out.contains(env!("CARGO_PKG_VERSION")), "version: {out}");
    assert!(out.contains("channel:"), "channel: {out}");
}

#[test]
fn phase5_upgrade_no_channel_errors() {
    // 报错：无渠道真升，exit=1 且指路（不碰网络）。
    let out = winterjs()
        .arg("--upgrade")
        .env_remove("WINTERJS_UPDATE_GITHUB")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("WINTERJS_UPDATE_GITHUB"),
        "stderr: {stderr}"
    );
}

#[test]
fn phase5_upgrade_dry_run_shows_channel() {
    // 边界：设了渠道时 dry-run 回显渠道，仍不碰网络。
    let out = stdout_of(
        winterjs()
            .args(["--upgrade", "--dry-run"])
            .env("WINTERJS_UPDATE_GITHUB", "someowner/somerepo"),
    );
    assert!(out.contains("github:someowner/somerepo"), "channel: {out}");
}
