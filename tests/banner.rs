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
    assert!(!stderr.contains("w i n t e r j s"), "stderr: {stderr}");
}

#[test]
fn hide_banner_forms() {
    // 破例单横杠与双横杠形都接受（重写不断言位置，只断言无 banner 且动作照跑）。
    for flag in ["-hide_banner", "--hide_banner"] {
        let out = winterjs().args([flag, "--eval", "1"]).output().unwrap();
        assert!(out.status.success(), "flag: {flag}");
        assert_eq!(String::from_utf8(out.stdout).unwrap(), "1\n");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!stderr.contains("w i n t e r j s"), "flag: {flag}, stderr: {stderr}");
    }
}

#[test]
fn help_lists_banner_switches() {
    // help 只展单横杠 token（DisplayHelp 后处理；补全/man 原样双横杠）。
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
    // 强制 ASCII 形：单横杠与双横杠都接受（管道下与 hide 同样无 banner，TTY 面由单测覆盖），
    // 此处断言可解析且动作照跑。
    for flag in ["-ascii_banner", "--ascii_banner"] {
        let out = winterjs().args([flag, "--eval", "1"]).output().unwrap();
        assert!(out.status.success(), "flag: {flag}");
        assert_eq!(String::from_utf8(out.stdout).unwrap(), "1\n");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!stderr.contains("w i n t e r j s"), "flag: {flag}, stderr: {stderr}");
    }
}
