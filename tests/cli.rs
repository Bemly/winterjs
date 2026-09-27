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
    // AGENTS.md §3 验收格式（D4，node 形，非 TTY）：`eval.js:1` + 源行 + `^` + 空行 +
    // `Error: boom` + `    at eval.js:1:7`，exit=1。
    let out = winterjs()
        .args(["--eval", "throw new Error(\"boom\")"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.starts_with("eval.js:1\nthrow new Error(\"boom\")\n      ^\n\nError: boom\n    at eval.js:1:7\n"),
        "stderr: {stderr}"
    );
}

#[test]
fn uncaught_error_node_shape_kinds_and_values() {
    // D4：类名头行（TypeError/SyntaxError）、非对象抛出打印值本身、宿主管线帧不进栈。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("t.js").write_str("\nfunction f() { throw new TypeError(\"tt\"); }\nf();\n").unwrap();
    let (ok, _, err) = wjs(&["--run", "t.js"], &dir);
    assert!(!ok);
    assert!(err.starts_with("t.js:2\n"), "stderr: {err}");
    assert!(err.contains("\nTypeError: tt\n    at f ("), "stderr: {err}");
    assert!(!err.contains("__wjs_"), "stderr: {err}");
    // 报错：语法错误无栈，头行 SyntaxError。
    dir.child("s.js").write_str("let = = ;\n").unwrap();
    let (ok, _, err) = wjs(&["--run", "s.js"], &dir);
    assert!(!ok);
    assert!(err.contains("\nSyntaxError: "), "stderr: {err}");
    // 边界：`throw 42` 打印值本身；ESM 入口同形（file: URL 头）。
    let (ok, _, err) = wjs(&["--eval", "throw 42"], &dir);
    assert!(!ok);
    assert!(err.ends_with("^\n\n42\n"), "stderr: {err}");
    dir.child("m.mjs").write_str("throw new RangeError(\"rr\");\n").unwrap();
    let (ok, _, err) = wjs(&["--run", "m.mjs"], &dir);
    assert!(!ok);
    assert!(err.starts_with("file://") && err.contains("\nRangeError: rr\n"), "stderr: {err}");
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
    // node 口径（真机 26.8.2 实测 `node app.js` 无完成值回显）：typeless .js
    // 入口走 CJS require 主模块（b701cf6）后无 rval——静默退出即对等行为，
    // 执行效应断言改走 console.log（§4.72 旧断言对真机翻转）。
    script.write_str("console.log(1 + 41)").unwrap();

    assert_eq!(
        stdout_of(&mut winterjs().arg("--run").arg(script.path())),
        "42\n"
    );
    dir.close().unwrap();
}

#[test]
fn run_script_in_tempdir_workdir() {
    // tempfile 直用：cwd 下的相对脚本（完成值回显同上，改执行效应断言）
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("rel.js");
    std::fs::write(&path, "console.log('ok')").unwrap();

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

// ── 9i-10 --run 脚本解释（带后缀→文件；裸名→package.json scripts 优先）──────

/// 造一个带 scripts 的 package.json + 可选 .bin 工具，返回目录。
fn script_project(dir: &assert_fs::TempDir, package_json: &str) {
    dir.child("package.json").write_str(package_json).unwrap();
}

#[test]
fn run_script_shell_command() {
    let dir = assert_fs::TempDir::new().unwrap();
    script_project(&dir, r#"{"scripts":{"dev":"echo shell-ok"}}"#);
    let out = winterjs()
        .arg("--run")
        .arg("dev")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("shell-ok"), "stdout: {stdout}");
    dir.close().unwrap();
}

#[test]
fn run_script_js_bin_runs_with_self() {
    // 零 node 快路径：.bin 里 shebang node 的 JS bin → 递归调自身 --run 执行。
    let dir = assert_fs::TempDir::new().unwrap();
    script_project(
        &dir,
        r#"{"scripts":{"dev":"mybin --a 1","other":"mybin"}}"#,
    );
    let bin = dir.child("node_modules/.bin/mybin");
    bin.write_str("#!/usr/bin/env node\nconsole.log('bin-ok', process.argv.slice(2).join(','));\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(bin.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let out = winterjs()
        .arg("--run")
        .arg("dev")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("bin-ok --a,1"), "stdout: {stdout}");
    dir.close().unwrap();
}

#[test]
fn run_script_js_bin_known_flag_passthrough() {
    // 子递归经 `--` 收尾：rest 里的本仓已知 flag（--version）必须落到 bin 的
    // argv，不能被子进程 clap 吃掉（修前打出 winterjs 版本横幅，bin 没跑）。
    let dir = assert_fs::TempDir::new().unwrap();
    script_project(&dir, r#"{"scripts":{"v":"mybin --version --help"}}"#);
    let bin = dir.child("node_modules/.bin/mybin");
    bin.write_str("#!/usr/bin/env node\nconsole.log('bin-ok', process.argv.slice(2).join(','));\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(bin.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let out = winterjs()
        .arg("--run")
        .arg("v")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("bin-ok --version,--help"), "stdout: {stdout}");
    assert!(!stdout.contains("winterjs"), "clap banner leaked: {stdout}");
    dir.close().unwrap();
}

#[test]
fn run_script_native_bin_direct_argv() {
    // .bin 里的原生/非 JS bin → 直接 argv 派发（不经 shell、不经 node）。
    let dir = assert_fs::TempDir::new().unwrap();
    script_project(&dir, r#"{"scripts":{"dev":"nativebin"}}"#);
    let bin = dir.child("node_modules/.bin/nativebin");
    bin.write_str("#!/bin/sh\necho native-ok\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(bin.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let out = winterjs()
        .arg("--run")
        .arg("dev")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("native-ok"), "stdout: {stdout}");
    dir.close().unwrap();
}

#[test]
fn run_script_missing_lists_available() {
    let dir = assert_fs::TempDir::new().unwrap();
    script_project(&dir, r#"{"scripts":{"build":"x","start":"y"}}"#);
    let out = winterjs()
        .arg("--run")
        .arg("nope")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Missing script: nope"), "stderr: {stderr}");
    assert!(stderr.contains("build") && stderr.contains("start"), "stderr: {stderr}");
    dir.close().unwrap();
}

#[test]
fn run_script_exit_code_propagates() {
    // exit 是 shell 内建 → shell 路径；退出码透传（Error::Exit 静默）。
    let dir = assert_fs::TempDir::new().unwrap();
    script_project(&dir, r#"{"scripts":{"fail":"echo before-fail && exit 3"}}"#);
    let out = winterjs()
        .arg("--run")
        .arg("fail")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    dir.close().unwrap();
}

#[test]
fn run_script_args_passthrough_with_dashdash() {
    // npm 口径：`--` 分隔符剥一个，其余拼到脚本串后。
    let dir = assert_fs::TempDir::new().unwrap();
    script_project(&dir, r#"{"scripts":{"dev":"echo args:"}}"#);
    let out = winterjs()
        .args(["--run", "dev", "--", "x", "y"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("args: x y"), "stdout: {stdout}");
    dir.close().unwrap();
}

#[test]
fn run_bare_name_falls_back_to_file() {
    // 无对应 script 但 cwd 有同名文件 → 回落按文件跑。
    let dir = assert_fs::TempDir::new().unwrap();
    script_project(&dir, r#"{"scripts":{"build":"x"}}"#);
    dir.child("dev").write_str("console.log('fallback-file');\n").unwrap();
    let out = winterjs()
        .arg("--run")
        .arg("dev")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("fallback-file"), "stdout: {stdout}");
    dir.close().unwrap();
}

#[test]
fn run_script_finds_package_json_up_the_tree() {
    // monorepo：在 packages/foo 里跑，package.json 命中仓库根。
    let dir = assert_fs::TempDir::new().unwrap();
    let root = dir.child("repo");
    root.child("package.json").write_str(r#"{"scripts":{"dev":"echo root-script"}}"#).unwrap();
    root.child("packages/foo").create_dir_all().unwrap();
    let out = winterjs()
        .arg("--run")
        .arg("dev")
        .current_dir(root.join("packages/foo").to_path_buf())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("root-script"), "stdout: {stdout}");
    dir.close().unwrap();
}

#[test]
fn run_script_vue_dev_shape_through_node_module() {
    // vue-project 案：scripts.dev → .bin JS bin（自递归）→ import node:module →
    // createRequire 读 CJS 配置，全链 exit=0（缺 node:module 时到此即炸）。
    let dir = assert_fs::TempDir::new().unwrap();
    script_project(
        &dir,
        r#"{"name":"vue-probe","type":"module","scripts":{"dev":"tool --watch-mode"}}"#,
    );
    dir.child("cfg.cjs").write_str("module.exports = { ok: true };\n").unwrap();
    let bin = dir.child("node_modules/.bin/tool");
    bin.write_str(
        "#!/usr/bin/env node\nimport { createRequire } from \"node:module\";\nconst req = createRequire(import.meta.url);\nconst cfg = req(\"../../cfg.cjs\");\nconsole.log(\"vue-dev\", cfg.ok, typeof req.resolve, process.argv.slice(2).join(\",\"));\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(bin.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let out = winterjs()
        .arg("--run")
        .arg("dev")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("vue-dev true function --watch-mode"), "stdout: {stdout}");
    dir.close().unwrap();
}

#[test]
fn serve_handler_without_serve_errors() {
    // 报错：`--handler` 是 `--serve` 的修饰 flag（§0.8），无 serve 即错。
    let out = winterjs()
        .args(["--handler", "h.js"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--handler only works with --serve"),
        "stderr: {stderr}"
    );
}

#[test]
fn serve_help_lists_handler() {
    // 正常：help/补全/man 同源生成（localized_command），`--handler` 可见。
    let stdout = stdout_of(&mut winterjs().arg("--help"));
    assert!(stdout.contains("--handler"), "help: {stdout}");
}

#[test]
fn node_runtime_flags_self_spawn_forms() {
    // D1（2026-09-25）：node 运行时旗按规则剥除（精确名单 + `--experimental-*` 等前缀族），
    // `node --flag file` / `node --flag -e` 自举形可跑，旗值经 execArgv/getOptionValue 读回。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("a.js")
        .write_str(
            "const { getOptionValue } = require('internal/options');\n\
             console.log('argv', JSON.stringify(process.argv.slice(2)));\n\
             console.log('exec', JSON.stringify(process.execArgv));\n\
             console.log('opt', getOptionValue('--pending-deprecation'), getOptionValue('--stack-trace-limit'), getOptionValue('--no-warnings'));\n",
        )
        .unwrap();
    // 正常：前缀族 + 精确名单 + `=值` 形，裸文件补 --run，位置参数透传。
    let (ok, out, err) = wjs(
        &["--pending-deprecation", "--stack-trace-limit=3", "--no-warnings", "a.js", "child"],
        &dir,
    );
    assert!(ok, "stderr: {err}");
    assert!(out.contains("argv [\"child\"]"), "out: {out}");
    assert!(
        out.contains("exec [\"--pending-deprecation\",\"--stack-trace-limit=3\",\"--no-warnings\"]"),
        "out: {out}"
    );
    assert!(out.contains("opt true 3 true"), "out: {out}");
    // 正常：`--experimental-x -e`（-e → --eval）。
    let (ok, out, err) = wjs(&["--experimental-vm-modules", "-e", "console.log('ev', 6 * 7)"], &dir);
    assert!(ok, "stderr: {err}");
    assert!(out.contains("ev 42"), "out: {out}");
    // 报错：非 node 旗（未知 `--bogus`、未登记的 `--experimental-foo-bar`）不被吞；
    // 非法旗值 node 同款 exit 9（4.209：剥除后重跑即自 spawn 无限递归）。
    let (ok, _, _) = wjs(&["--bogus", "a.js"], &dir);
    assert!(!ok);
    let (ok, _, _) = wjs(&["--experimental-foo-bar", "a.js"], &dir);
    assert!(!ok);
    let out = winterjs().args(["--unhandled-rejections=foobar", "a.js"]).current_dir(dir.path()).output().unwrap();
    assert_eq!(out.status.code(), Some(9));
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid value for --unhandled-rejections"));
    // 边界：无兼容旗的裸文件照旧报错（§0.8），`--` 后的脚本旗原样透传。
    let (ok, _, _) = wjs(&["a.js"], &dir);
    assert!(!ok);
    let (ok, out, err) = wjs(&["--run", "a.js", "--", "--no-warnings"], &dir);
    assert!(ok, "stderr: {err}");
    assert!(out.contains("argv [\"--no-warnings\"]") && out.contains("exec []"), "out: {out}");
}

#[test]
fn self_spawn_depth_guard_stops_recursion() {
    // 4.209 防线二：自递归起自身的脚本止于有限深度（上限 32），而不是吃光系统。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("rec.js")
        .write_str(
            "const { spawnSync } = require('child_process');\n\
             const r = spawnSync(process.execPath, [__filename], { encoding: 'utf8' });\n\
             const d = Number((r.stdout.match(/depth=(\\d+)/) || [])[1] || 0);\n\
             console.log('depth=' + (d + 1), 'visible=' + ('WINTERJS_SPAWN_DEPTH' in process.env));\n",
        )
        .unwrap();
    let (ok, out, err) = wjs(&["--run", "rec.js"], &dir);
    assert!(ok, "stderr: {err}");
    // 链深 = 上限 + 1 层（最深一层被闸拒绝，stdout 无 depth）；变量对 JS 不可见。
    assert!(out.contains("depth=33 visible=false"), "out: {out}");
}

#[test]
fn modifier_flags_only_work_with_their_action() {
    // §0.8：修饰 flag 错配即错（exit=1 + 指路 --help），不静默吞掉。
    for (args, expect) in [
        (vec!["--eval", "1", "--tag", "next"], "--tag only works with --publish"),
        (vec!["--eval", "1", "--filter", "*.js"], "--filter only works with --test"),
        (vec!["--eval", "1", "--port", "8080"], "--port only works with --serve"),
        (vec!["--eval", "1", "--schema"], "--schema only works with --config"),
        (vec!["--config", "--allow-read"], "--allow-* only works with --run/--eval/--test/--repl"),
        (vec!["--eval", "1", "--registry", "https://x.invalid"], "--registry only works with"),
        (vec!["--eval", "1", "--token", "abc"], "--token only works with --login"),
        (vec!["--eval", "1", "--watch"], "--watch only works with --test"),
        (vec!["--eval", "1", "--yes"], "--yes only works with --init"),
    ] {
        let out = winterjs().args(&args).output().unwrap();
        assert_eq!(out.status.code(), Some(1), "args: {args:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(expect) && stderr.contains("--help"),
            "args: {args:?} stderr: {stderr}"
        );
    }
    // 正常：归属正确不报错（--config --schema 既有行为；--eval + --allow-all 放行）。
    assert_eq!(stdout_of(&mut winterjs().args(["--eval", "40 + 2", "--allow-all"])), "42\n");
}

#[test]
fn modifier_explicit_defaults_rejected_and_trailing_not_swallowed() {
    // 显式给默认值也算显式（按解析来源判，不按值比；修前 `--port 3000` 漏判）。
    for (args, expect) in [
        (vec!["--eval", "1", "--port", "3000"], "--port only works with --serve"),
        (vec!["--eval", "1", "--host", "127.0.0.1"], "--host only works with --serve"),
        (vec!["--eval", "1", "--dir", "."], "--dir only works with --serve"),
        (vec!["--eval", "1", "--limit-rps", "0"], "--limit-rps only works with --serve"),
        (vec!["--tag", "latest"], "--tag only works with --publish"),
    ] {
        let out = winterjs().args(&args).output().unwrap();
        assert_eq!(out.status.code(), Some(1), "args: {args:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(expect) && stderr.contains("--help"),
            "args: {args:?} stderr: {stderr}"
        );
    }
    // 报错：trailing 透传只归 --run；未知旗形落进 args 不再静默吞掉。
    for args in [
        vec!["--eval", "1", "--env-file=x"],
        vec!["--eval", "1", "--unknown-flag"],
        vec!["--eval", "1", "--", "--env-file=x"],
        vec!["--config", "stray-positional"],
    ] {
        let out = winterjs().args(&args).output().unwrap();
        assert_eq!(out.status.code(), Some(1), "args: {args:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("only works with --run") && stderr.contains("--help"),
            "args: {args:?} stderr: {stderr}"
        );
    }
    // 报错：--test 的 `--` 打头位置值是 flag 误写，不报"无此路径"。
    let out = winterjs().args(["--test", "--cov"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown flag '--cov'"), "stderr: {stderr}");
    // 正常：--run 的 `--` 后旗形照旧透传给脚本（§4.61/§4.63）。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("a.js")
        .write_str("console.log('argv ' + JSON.stringify(process.argv.slice(2)))")
        .unwrap();
    let (ok, out, err) = wjs(&["--run", "a.js", "--", "--port", "3000"], &dir);
    assert!(ok, "stderr: {err}");
    assert!(out.contains("argv [\"--port\",\"3000\"]"), "out: {out}");
    // 边界：裸位置脚本参数（无 `--`）照旧是脚本参数，不误判。
    let (ok, out, err) = wjs(&["--run", "a.js", "child"], &dir);
    assert!(ok, "stderr: {err}");
    assert!(out.contains("argv [\"child\"]"), "out: {out}");
    dir.close().unwrap();
}
