//! CLI 底座黑盒测试(对齐 src/cli.rs + main/settings/logging/i18n/error)。
//! 注意:assert_cmd 跑子进程、stderr 非 TTY → 错误走稳定 plain 格式。

mod common;

use common::*;

use assert_fs::prelude::*;
use rstest::rstest;

#[rstest]
#[case("40 + 2", "42\n")]
#[case("'a' + 'b'", "ab\n")]
#[case("Math.max(3, 9)", "9\n")]
#[case("undefined", "")]
fn eval_completion_value(#[case] code: &str, #[case] expected: &str) {
    assert_eq!(stdout_of(&mut winterjs().args(["--eval", code])), expected);
}

#[test]
fn eval_uncaught_exception_exit_1_with_plain_format() {
    // AGENTS.md §3 验收格式：Error: eval.js:1:7: boom，exit=1（非 TTY）
    let out = winterjs()
        .args(["--eval", "throw new Error(\"boom\")"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Error: eval.js:1:7: boom"),
        "stderr: {stderr}"
    );
}

#[test]
fn run_missing_file_reports_chain() {
    let out = winterjs()
        .args(["--run", "/nope/such.js"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Error: failed to read /nope/such.js"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("Caused by:"), "stderr: {stderr}");
}

#[test]
fn run_file_from_tempdir() {
    let dir = assert_fs::TempDir::new().unwrap();
    let script = dir.child("app.js");
    script.write_str("1 + 41").unwrap();

    assert_eq!(
        stdout_of(&mut winterjs().arg("--run").arg(script.path())),
        "42\n"
    );
    dir.close().unwrap();
}

#[test]
fn run_script_in_tempdir_workdir() {
    // tempfile 直用：cwd 下的相对脚本
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("rel.js");
    std::fs::write(&path, "'ok'").unwrap();

    assert_eq!(
        stdout_of(
            &mut winterjs()
                .arg("--run")
                .arg("rel.js")
                .current_dir(tmp.path())
        ),
        "ok\n"
    );
}

#[test]
fn version_contains_pkg_version() {
    let out = stdout_of(&mut winterjs().arg("--version"));
    assert!(out.contains(env!("CARGO_PKG_VERSION")), "version: {out}");
    // vergen gitcl 元数据也应嵌进来（git 仓库内构建时）
    assert!(out.contains("built "), "version: {out}");
}

#[test]
fn config_outputs_resolved_settings_json() {
    let out = stdout_of(&mut winterjs().args(["--config"]));
    let value: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert!(value["log"].is_object(), "log section present: {out}");

    // 环境变量覆盖（WINTERJS_LOG__COLOR）优先于缺省
    let out = stdout_of(
        &mut winterjs()
            .env("WINTERJS_LOG__COLOR", "always")
            .args(["--config"]),
    );
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value["log"]["color"], "always");
}

#[test]
fn config_schema_is_valid_json_schema() {
    let out = stdout_of(&mut winterjs().args(["--config", "--schema"]));
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        value["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
}

#[test]
fn winterjs_log_filter_does_not_break_config() {
    // AGENTS §4.10 回归：WINTERJS_LOG=<EnvFilter> 是日志直读变量，不得被 config 误收
    let out = winterjs()
        .env("WINTERJS_LOG", "winterjs=debug")
        .args(["--config"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(value["log"].is_object(), "log section present: {stdout}");
}

#[test]
fn completions_bash_script() {
    let out = stdout_of(&mut winterjs().args(["--completions", "bash"]));
    assert!(out.starts_with("_winterjs()"), "completions: {out}");
}

#[test]
fn man_pages_render_roff() {
    let out = stdout_of(&mut winterjs().arg("--man"));
    assert_eq!(
        out.matches(".TH").count(),
        1,
        "single man page (flag CLI has no subcommands)"
    );
}

#[test]
fn settings_toml_is_loaded() {
    // config crate 从 cwd 读 winterjs.toml
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("winterjs.toml"),
        "[log]\ncolor = \"never\"\n",
    )
    .unwrap();
    let out = stdout_of(&mut winterjs().arg("--config").current_dir(tmp.path()));
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    // pretty_assertions：比对失败时输出可读 diff
    pretty_assertions::assert_eq!(
        value["log"]["color"].as_str().unwrap(),
        "never",
        "settings file should override default"
    );
}

#[test]
fn settings_yaml_is_loaded() {
    // config 的 yaml 特性（yaml-rust2 链，2026-09-10 纯度审计后启用）
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("winterjs.yaml"),
        "log:\n  color: \"never\"\n",
    )
    .unwrap();
    let out = stdout_of(&mut winterjs().arg("--config").current_dir(tmp.path()));
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    pretty_assertions::assert_eq!(
        value["log"]["color"].as_str().unwrap(),
        "never",
        "yaml settings file should override default"
    );
}

// ── Phase 1：console / timers / microtask / structuredClone ─────────────

#[test]
fn i18n_help_zh() {
    // 正常：`-l zh` 顶层 help 全中；flag 放子命令后也认（预扫全 argv）
    let out = stdout_of(&mut winterjs().args(["-l", "zh", "--help"]));
    assert!(out.contains("运行 JS 文件"), "zh top help:\n{out}");
    assert!(out.contains("帮助文本语言"), "zh lang flag:\n{out}");
    // flag 世界：动作的值必须紧贴（--run 后直接跟别的 flag 会被当缺值）；
    // 跨 flag 写法是 -l 前置 + 动作给值（--help 短路只展示不执行；扁平 CLI 无 per-action 页）
    let out = stdout_of(&mut winterjs().args(["-l", "zh", "--run", "dummy.js", "--help"]));
    assert!(
        out.contains("--run") && out.contains("运行 JS 文件"),
        "zh run flag:\n{out}"
    );
    assert!(
        out.contains("允许文件系统读取"),
        "zh flattened perms:\n{out}"
    );
    // `--lang=` 连写
    let out = stdout_of(&mut winterjs().args(["--lang=zh", "--help"]));
    assert!(out.contains("求值内联 JS 代码"), "zh eval flag:\n{out}");
}

#[test]
fn i18n_help_en_pinned() {
    // 正常：`-l en` 在中文系统上也强制英文
    let out = stdout_of(&mut winterjs().args(["-l", "en", "--help"]));
    assert!(out.contains("Run a JS file"), "en top help:\n{out}");
    assert!(!out.contains("运行 JS 文件"), "must not leak zh:\n{out}");
}

#[test]
fn i18n_lang_env_and_precedence() {
    // 正常：无 flag 时 WINTERJS_LANG 生效；有 flag 时 flag 赢
    let out = stdout_of(&mut winterjs().args(["--help"]).env("WINTERJS_LANG", "zh"));
    assert!(out.contains("运行 JS 文件"), "env zh:\n{out}");
    let out = stdout_of(
        &mut winterjs()
            .args(["-l", "en", "--help"])
            .env("WINTERJS_LANG", "zh"),
    );
    assert!(out.contains("Run a JS file"), "flag beats env:\n{out}");
}

#[test]
fn i18n_invalid_lang_rejected() {
    // 报错：非法值走 clap 报错（exit=2，英文，clap 自带词不汉化）
    let out = winterjs().args(["-l", "fr", "--help"]).output().unwrap();
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid value"), "clap error:\n{err}");
}

#[test]
fn i18n_unknown_env_falls_back_to_en() {
    // 边界：未知 env 值回退英文（不炸）；`--` 后的 `-l` 不认（脚本参数）
    let out = stdout_of(&mut winterjs().args(["--help"]).env("WINTERJS_LANG", "fr"));
    assert!(out.contains("Run a JS file"), "fallback en:\n{out}");
}

// ── add（工程本地）/ install（全局）拆分 ─────────────────────────────────────

#[test]
fn cli_flag_spec_single_action() {
    // 正常：短 flag 全套（-r/-e/-a/-i）与子命令等价
    assert_eq!(stdout_of(&mut winterjs().args(["-e", "40 + 2"])), "42\n");
    // 报错：多动作互斥（exit=1，可读）
    let out = winterjs()
        .args(["--run", "a.js", "--eval", "1"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("exactly one action"), "stderr:\n{err}");
    // 报错：无动作裸奔指路 --help
    let out = winterjs().args(["--dry-run"]).output().unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--help"), "stderr:\n{err}");
}

// ── SubtleCrypto c-4x（RSA-PSS/Ed25519/X25519/AES-192；向量经 openssl 独立生成）──
