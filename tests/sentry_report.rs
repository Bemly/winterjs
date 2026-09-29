//! 崩溃上报黑盒测试(对齐 src/sentry_report.rs)。

mod common;

use common::*;

#[test]
fn phase8_sentry_optin_never_breaks_cli() {
    // 上报是旁路：坏 DSN 告警后继续；不可达端点不影响 CLI 行为与退出码。
    let dir = assert_fs::TempDir::new().unwrap();

    // 坏 DSN：stderr 告警 + 继续正常执行
    let out = winterjs2()
        .args(["--eval", "40 + 2"])
        .env("WINTERJS2_SENTRY_DSN", "not-a-valid-dsn")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "42\n");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("not a valid Sentry DSN"), "stderr: {err}");

    // 不可达端点：CLI 照常（transport 后台线程吞错，主流程无感）
    let out = winterjs2()
        .args(["--eval", "40 + 2"])
        .env("WINTERJS2_SENTRY_DSN", "http://key@127.0.0.1:9/42")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "unreachable dsn must not break cli");
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "42\n");

    // 未设置：无任何告警（默认关闭零成本）
    let out = winterjs2()
        .args(["--eval", "40 + 2"])
        .env_remove("WINTERJS2_SENTRY_DSN")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "42\n");
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("Sentry"),
        "no sentry noise"
    );
    dir.close().unwrap();
}

// ── CLI 双语（-l/--lang > WINTERJS2_LANG > 系统 > en）────────────────────────────
// 本机系统语言可能是中文，所有用例显式定语言，保证任何机器上确定性。
