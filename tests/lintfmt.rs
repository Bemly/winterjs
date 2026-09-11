//! lint/fmt 穿透黑盒测试(对齐 src/lintfmt.rs)。

mod common;

use common::*;

use assert_fs::prelude::*;

#[cfg(unix)]
#[test]
fn phase8_lintfmt_passthrough() {
    // 正常：参数原样转发（首参 flag 也在内）/stderr 直出/monorepo 向上查找/fmt --check。
    let dir = assert_fs::TempDir::new().unwrap();
    make_tool_repo(
        &dir,
        "#!/bin/sh\necho \"fake-oxlint args: $@\"\necho \"lint-stderr\" >&2\nexit 0\n",
    );
    let (ok, out, err) = wjs(&["--lint", "src", "--write"], &dir);
    assert!(ok, "lint run: {err}");
    assert!(out.contains("fake-oxlint args: src --write"), "out: {out}");
    assert!(
        err.contains("lint-stderr"),
        "stderr must pass through: {err}"
    );
    // monorepo：子目录里跑，向上命中根安装的工具
    let (ok, out, err) = wjs(&["--lint", "."], &dir);
    assert!(ok, "subdir: {err}");
    assert!(out.contains("fake-oxlint args: ."), "out: {out}");
    // fmt 完全透传（oxfmt 默认写回、--check 为 CI 检查，均上游语义）
    let (ok, out, err) = wjs(&["--fmt", "--check", "src"], &dir);
    assert!(ok, "fmt: {err}");
    assert!(out.contains("fake-oxfmt got: --check src"), "out: {out}");
    dir.close().unwrap();
}

#[cfg(unix)]
#[test]
fn phase8_lintfmt_exit_and_notfound() {
    // 退出码透传（非零静默映射 exit code）；本地+PATH 双落空给可读指引。
    let dir = assert_fs::TempDir::new().unwrap();
    make_tool_repo(&dir, "#!/bin/sh\nexit 3\n");
    let file = dir.child("l.mjs");
    let _ = file;
    let out = winterjs()
        .args(["--lint", "src"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3), "exit code must forward");
    assert!(out.stdout.is_empty(), "Error::Exit is silent");

    // 未找到：清 PATH（env 清空 + 本地无工具），报两种安装指引
    let empty = assert_fs::TempDir::new().unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_winterjs"))
        .args(["--lint"])
        .current_dir(empty.path())
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("oxlint was not found"), "{err}");
    assert!(err.contains("release:github/oxc-project/oxc"), "{err}");
    assert!(!err.contains("npx"), "{err}");
    dir.close().unwrap();
    empty.close().unwrap();
}

// ── Phase 8-c: sentry 崩溃上报（opt-in）─────────────────────────────────────

#[cfg(unix)]
fn make_tool_repo(dir: &assert_fs::TempDir, script: &str) {
    use std::os::unix::fs::PermissionsExt as _;
    let bin = dir.child("node_modules").child(".bin");
    bin.create_dir_all().unwrap();
    let tool = bin.child("oxlint");
    tool.write_str(script).unwrap();
    std::fs::set_permissions(tool.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let fmt = bin.child("oxfmt");
    fmt.write_str("#!/bin/sh\necho \"fake-oxfmt got: $@\"\n")
        .unwrap();
    std::fs::set_permissions(fmt.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    dir.child("packages/foo").create_dir_all().unwrap();
}
