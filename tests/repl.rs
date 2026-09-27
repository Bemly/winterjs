//! winterjs repl 黑盒测试(对齐 src/repl.rs)。

#[test]
fn phase7_repl_persistent_ctx() {
    // 正常：跨行持久上下文（`const` 次行可用）+ banner + exit 0。
    let (stdout, _, code) = repl_session("const x = 21\nx * 2\n.exit\n");
    assert_eq!(code, 0);
    assert!(stdout.starts_with("winterjs repl"), "banner:\n{stdout}");
    assert!(stdout.contains("42\n"), "stdout:\n{stdout}");
}

#[test]
fn phase7_repl_error_recovery() {
    // 正常：报错行打印后继续，会话不死。
    let (stdout, stderr, code) = repl_session("undefinedVar\n40 + 2\n.exit\n");
    assert_eq!(code, 0);
    assert!(
        stderr.contains("undefinedVar is not defined"),
        "stderr:\n{stderr}"
    );
    assert!(stdout.contains("42\n"), "stdout:\n{stdout}");
}

#[test]
fn phase7_repl_help_no_ansi() {
    // 正常 + 边界：`.help` 列命令；非 TTY 输出无 ANSI 转义。
    let (stdout, stderr, code) = repl_session(".help\n\n40+2\n.quit\n");
    assert_eq!(code, 0);
    assert!(
        stdout.contains(".exit") && stdout.contains(".help"),
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains("42\n"), "stdout:\n{stdout}");
    assert!(
        !stdout.contains('\u{1b}'),
        "stdout must not contain ANSI:\n{stdout:?}"
    );
    assert!(
        !stderr.contains('\u{1b}'),
        "stderr must not contain ANSI:\n{stderr:?}"
    );
}

#[test]
fn phase7_repl_syntax_continues() {
    // 边界：语法错误行（非 TTY 无续行）报错后继续。
    let (stdout, stderr, code) = repl_session("1 +\n40 + 2\n.exit\n");
    assert_eq!(code, 0);
    assert!(!stderr.is_empty(), "expected a syntax error on stderr");
    assert!(stdout.contains("42\n"), "stdout:\n{stdout}");
}

#[test]
fn repl_bare_invocation_enters_repl() {
    // 正常：裸启动（无任何参数）直接进 REPL（node/python 同款）。
    let (stdout, _, code) = repl_session_with_args("40 + 2\n.exit\n", &[]);
    assert_eq!(code, 0);
    assert!(stdout.starts_with("winterjs repl"), "banner:\n{stdout}");
    assert!(stdout.contains("42\n"), "stdout:\n{stdout}");
}

#[test]
fn repl_exit_functions() {
    // 正常：exit()/quit()/q() 干净退出（code 0，无 ReferenceError）。
    for (input, tag) in [("exit()\n", "exit"), ("quit()\n", "quit"), ("q()\n", "q")] {
        let (stdout, stderr, code) = repl_session(input);
        assert_eq!(code, 0, "{tag}");
        assert!(stdout.starts_with("winterjs repl"), "{tag} banner:\n{stdout}");
        assert!(!stderr.contains("not defined"), "{tag} stderr:\n{stderr}");
    }
}

#[test]
fn repl_exit_functions_not_in_scripts() {
    // 边界：退出函数是 REPL 专属，脚本里不可见（不污染用户全局）。
    for name in ["exit", "quit", "q"] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_winterjs"))
            .args(["--eval", &format!("typeof {name}")])
            .output()
            .expect("eval runs");
        assert!(out.status.success());
        assert_eq!(String::from_utf8(out.stdout).unwrap(), "undefined\n", "{name}");
    }
}

// ── Phase 7-e4: bun:sqlite（turso 之上的 Bun 兼容层）─────────────────────────

/// REPL 会话（stdin 全量喂入后关管；返回 stdout/stderr/exit）。
/// `HOME` 隔离到临时目录（历史文件不污染真 home）。
fn repl_session(input: &str) -> (String, String, i32) {
    repl_session_with_args(input, &["--repl"])
}

/// REPL 会话（`args` 自定；空即裸启动不断言 `--repl`）。
fn repl_session_with_args(input: &str, args: &[&str]) -> (String, String, i32) {
    use std::io::Write;
    let home = assert_fs::TempDir::new().unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_winterjs"))
        .args(args)
        .env("HOME", home.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("repl spawns");
    {
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(input.as_bytes()).unwrap();
    }
    let out = child.wait_with_output().expect("repl runs");
    // home 取不到 path？TempDir 活到此处，drop 即清理。
    let _ = home.close();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}
