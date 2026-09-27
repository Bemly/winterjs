//! 启动 banner 黑盒测试（对齐 src/banner.rs + CLI `-hide_banner`，2026-09-28）。
//! harness 下 stderr 恒管道（非 TTY）→ 断言"无 banner"面；有 TTY 的图形/ASCII
//! 面由 `cargo test --bin winterjs banner` 单测覆盖（纯函数 + 真素材）。

mod common;

use common::*;

#[test]
fn banner_hidden_when_piped() {
    // stderr 非 TTY 自动跳过；stdout 数据通道逐字节干净。
    let out = winterjs().args(["--eval", "40 + 2"]).output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "42\n");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("❄"), "stderr: {stderr}");
}

#[test]
fn hide_banner_forms() {
    // 单横杠是唯一形：可解析且动作照跑（管道下本就无 banner）。
    let out = winterjs().args(["-hide_banner", "--eval", "1"]).output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "1\n");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("❄"), "stderr: {stderr}");
    // 双横杠形不存在：走未知 flag 统一通道（exit 1 + 指路 --help，与 --bogus 同口径）。
    let out = winterjs().args(["--hide_banner", "--eval", "1"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unexpected argument '--hide_banner'"), "stderr: {stderr}");
}

#[test]
fn help_lists_banner_switches() {
    // help 只认单横杠形（after_help 双语段）。
    let out = winterjs().arg("--help").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("-hide_banner"), "help: {stdout}");
    assert!(stdout.contains("-ascii_banner"), "help: {stdout}");
    assert!(!stdout.contains("--hide_banner"), "help: {stdout}");
    assert!(!stdout.contains("--ascii_banner"), "help: {stdout}");
}

#[test]
fn ascii_banner_forms() {
    // 强制 ASCII 形：单横杠唯一（管道下与 hide 同样无 banner，TTY 面由单测覆盖），
    // 此处断言可解析且动作照跑。
    let out = winterjs().args(["-ascii_banner", "--eval", "1"]).output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "1\n");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("❄"), "stderr: {stderr}");
}
