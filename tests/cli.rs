//! CLI 黑盒测试（docs/plan.md Phase 0「CLI 黑盒测试」）。
//! 注意：assert_cmd 跑子进程、stderr 非 TTY → 错误走稳定 plain 格式。

use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;
use rstest::rstest;

fn winterjs() -> Command {
    Command::cargo_bin("winterjs").expect("binary builds")
}

fn stdout_of(cmd: &mut Command) -> String {
    let out = cmd.output().expect("binary runs");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).expect("utf8 stdout")
}

#[rstest]
#[case("40 + 2", "42\n")]
#[case("'a' + 'b'", "ab\n")]
#[case("Math.max(3, 9)", "9\n")]
#[case("undefined", "")]
fn eval_completion_value(#[case] code: &str, #[case] expected: &str) {
    winterjs()
        .args(["eval", code])
        .assert()
        .success()
        .stdout(expected.to_string());
}

#[test]
fn eval_uncaught_exception_exit_1_with_plain_format() {
    // AGENTS.md §3 验收格式：Error: eval.js:1:7: boom，exit=1（非 TTY）
    winterjs()
        .args(["eval", "throw new Error(\"boom\")"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicates::str::contains("Error: eval.js:1:7: boom"));
}

#[test]
fn run_missing_file_reports_chain() {
    winterjs()
        .args(["run", "/nope/such.js"])
        .assert()
        .failure()
        .code(1)
        .stderr(
            predicates::str::contains("Error: failed to read /nope/such.js")
                .and(predicates::str::contains("Caused by:")),
        );
}

#[test]
fn run_file_from_tempdir() {
    let dir = assert_fs::TempDir::new().unwrap();
    let script = dir.child("app.js");
    script.write_str("1 + 41").unwrap();

    winterjs()
        .arg("run")
        .arg(script.path())
        .assert()
        .success()
        .stdout("42\n");
    dir.close().unwrap();
}

#[test]
fn run_script_in_tempdir_workdir() {
    // tempfile 直用：cwd 下的相对脚本
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("rel.js");
    std::fs::write(&path, "'ok'").unwrap();

    winterjs()
        .arg("run")
        .arg("rel.js")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout("ok\n");
}

#[test]
fn version_contains_pkg_version() {
    winterjs()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicates::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn config_outputs_resolved_settings_json() {
    let out = stdout_of(&mut winterjs().args(["config"]));
    let value: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert!(value["log"].is_object(), "log section present");

    // 环境变量覆盖（WINTERJS_LOG__COLOR）优先于缺省
    let out = stdout_of(&mut winterjs().env("WINTERJS_LOG__COLOR", "always").args(["config"]));
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value["log"]["color"], "always");
}

#[test]
fn config_schema_is_valid_json_schema() {
    let out = stdout_of(&mut winterjs().args(["config", "--schema"]));
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        value["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
}

#[test]
fn completions_bash_script() {
    winterjs()
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicates::str::starts_with("_winterjs()"));
}

#[test]
fn man_pages_render_roff() {
    let out = stdout_of(&mut winterjs().arg("man"));
    assert_eq!(out.matches(".TH").count(), 6, "main + 5 subcommand pages");
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
    let out = stdout_of(&mut winterjs().arg("config").current_dir(tmp.path()));
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    // pretty_assertions：比对失败时输出可读 diff
    pretty_assertions::assert_eq!(
        value["log"]["color"].as_str().unwrap(),
        "never",
        "settings file should override default"
    );
}
