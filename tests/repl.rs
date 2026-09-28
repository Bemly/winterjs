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
fn repl_error_prints_stack() {
    // 报错：有栈错误打 node 形多行（定位行 + `at` 帧），会话继续。
    let (stdout, stderr, code) =
        repl_session("function f() { throw new Error(\"boom\") }\nf()\n40 + 2\n.exit\n");
    assert_eq!(code, 0);
    assert!(stderr.contains("Error: boom"), "stderr:\n{stderr}");
    assert!(
        stderr.contains("    at f (repl.js:"),
        "missing stack frame:\n{stderr}"
    );
    assert!(stdout.contains("42\n"), "stdout:\n{stdout}");
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

#[test]
fn repl_tla_await_resolves() {
    // R6：REPL 顶层 await（试错包装 + 挂起状态机；node defaultEval 同构语义）。
    let (stdout, _, code) =
        repl_session("await 41\nawait new Promise(r=>setTimeout(()=>r(\"tick\"),10))\n2+2\n.exit\n");
    assert_eq!(code, 0);
    assert!(stdout.lines().any(|l| l == "41"), "stdout:\n{stdout}");
    assert!(stdout.lines().any(|l| l == "tick"), "stdout:\n{stdout}");
    assert!(stdout.lines().any(|l| l == "4"), "stdout:\n{stdout}");
}

#[test]
fn repl_tla_await_rejects() {
    // 报错件：rejected 走 uncaught 渲染（D4 形），会话继续。
    let (stdout, stderr, code) =
        repl_session("await Promise.reject(new Error(\"boom\"))\n2+2\n.exit\n");
    assert_eq!(code, 0);
    assert!(stderr.contains("Error: boom"), "stderr:\n{stderr}");
    assert!(stdout.lines().any(|l| l == "4"), "stdout:\n{stdout}");
}

#[test]
fn repl_tla_in_async_fn_untouched() {
    // 边界：await 在 async 函数内——原码编译即过，不进包装路径。
    let (stdout, _, code) = repl_session(
        "const f = async () => await 7\nf().then(v => console.log(v))\n.exit\n",
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("7\n"), "stdout:\n{stdout}");
}

#[test]
fn repl_tla_let_hoisting() {
    // R6b：声明提升（node await.js VariableDeclaration 重写）——`let a = await x`
    // 跨行存活（acorn vendored 8.18.0，node 26.8.2 内建同款）。
    let (stdout, _, code) = repl_session("let a = await 41\na + 1\n.exit\n");
    assert_eq!(code, 0);
    // 声明完成值 undefined（node 同形：无 return 改写时不打印值）。
    assert!(stdout.lines().any(|l| l == "42"), "stdout:\n{stdout}");
}

#[test]
fn repl_tla_multi_decl_and_fn() {
    // 边界：多声明解构式提升 + function 声明提升（var 提升语义）。
    let (stdout, _, code) = repl_session(
        "let a = await 1, b = await 2\nfunction g() { return a + b }\ng()\n.exit\n",
    );
    assert_eq!(code, 0);
    assert!(stdout.lines().any(|l| l == "3"), "stdout:\n{stdout}");
}

#[test]
fn repl_doc_mdn_page() {
    // `.doc` 整篇文档（irb show_doc 方向；CLI 本体面）：正常出官方首句+Syntax
    // 节；未知条目走 stderr 提示；空参打用法；非 TTY 纯文本无 ANSI。
    let (stdout, stderr, code) =
        repl_session(".doc console.log\n.doc fetch\n.doc Array.from\n.doc encodeURI\n.doc o.assign\n.doc\n40 + 2\n.exit\n");
    assert_eq!(code, 0);
    assert!(
        stdout.contains("The console.log() static method outputs a message to the console."),
        "stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("starts the process of fetching a resource from the network"),
        "stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("creates a new, shallow-copied Array instance"),
        "stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("function encodes a URI by replacing"),
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains("Syntax"), "stdout:\n{stdout}");
    assert!(stdout.contains("42\n"), "stdout:\n{stdout}");
    assert!(
        stderr.contains("no documentation for 'o.assign'"),
        "stderr:\n{stderr}"
    );
    assert!(stderr.contains(".doc <topic>"), "stderr:\n{stderr}");
    assert!(!stdout.contains('\u{1b}'), "stdout must not contain ANSI");
    assert!(!stderr.contains('\u{1b}'), "stderr must not contain ANSI");
}

#[test]
fn repl_empty_line_lists_globals() {
    // 空行 Tab：全局全枚举（node 真机同形），不再 NO RECORDS。
    // 正常：含 console/fetch/globalThis 且有序；边界：无 `__wjs_` 内部面、
    // completeOn 为空；会话继续。
    let (stdout, _, code) = repl_session(
        "const r = globalThis.__wjs_cli_complete(\"\");\n\
         const names = r[0].map((p) => p[0]);\n\
         console.log(\"n\", names.length > 50);\n\
         console.log(\"has\", names.includes(\"console\") && names.includes(\"fetch\") && names.includes(\"globalThis\"));\n\
         console.log(\"sorted\", JSON.stringify(names) === JSON.stringify([...names].sort()));\n\
         console.log(\"no-internal\", names.every((n) => !n.startsWith(\"__wjs_\")));\n\
         console.log(\"on\", JSON.stringify(r[1]));\n\
         .exit\n",
    );
    assert_eq!(code, 0);
    for line in ["n true", "has true", "sorted true", "no-internal true", "on \"\""] {
        assert!(stdout.lines().any(|l| l == line), "missing {line:?}; stdout:\n{stdout}");
    }
}
