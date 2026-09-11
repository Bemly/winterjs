//! winterjs test 黑盒测试(对齐 src/testrun.rs:多文件/过滤/watch)。

mod common;

use common::*;

use assert_fs::prelude::*;

#[test]
fn phase7_test_mixed_files() {
    // 正常：子测试 TAP 行透出 + runner 行 + 汇总，有挂则 exit=1。
    let dir = test_fixture();
    let out = winterjs()
        .args(["--test", "."])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("not ok - fails"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("not ok - a.test.js (exit 1)"),
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains("# pass 0, fail 1"), "stdout:\n{stdout}");
    assert!(
        !stdout.contains("helper"),
        "non-test file must not run:\n{stdout}"
    );
    dir.close().unwrap();
}

#[test]
fn phase7_test_all_pass() {
    // 正常：全过则 exit=0 + `ok -` 行；两个文件同进程连跑（§4.24 引擎单例回归，
    // 修前第二个文件起全部 `failed to init JS engine`）。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("o.test.js")
        .write_str("import { test } from \"node:test\";\ntest(\"ok\", () => {});\n")
        .unwrap();
    dir.child("o2.test.js")
        .write_str("console.log(\"two\");\n")
        .unwrap();
    let out = stdout_of(winterjs().args(["--test", "."]).current_dir(dir.path()));
    assert!(out.contains("ok - o.test.js"), "stdout:\n{out}");
    assert!(out.contains("ok - o2.test.js"), "stdout:\n{out}");
    assert!(out.contains("# pass 2, fail 0"), "stdout:\n{out}");
    dir.close().unwrap();
}

#[test]
fn phase7_test_filter() {
    // 边界：--filter 只跑命中文件（此处零命中 → exit 0 提示行）。
    let dir = test_fixture();
    let out = stdout_of(
        winterjs()
            .args(["--test", ".", "--filter", "zzz*"])
            .current_dir(dir.path()),
    );
    assert!(out.contains("no test files found"), "stdout:\n{out}");
    dir.close().unwrap();
}

#[test]
fn phase7_test_bad_path() {
    // 报错：不存在的路径 exit=1 且可读。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .args(["--test", "no-such-dir"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no such test path"), "stderr: {stderr}");
    dir.close().unwrap();
}

#[test]
fn phase7_test_watch_reruns_on_change() {
    // e5：初始跑一轮 → 改文件防抖重跑（含新输出）→ SIGINT 优雅退出 exit=0。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("a.test.js")
        .write_str("console.log(\"v1\");\n")
        .unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_winterjs"))
        .args(["--test", "--watch"])
        .current_dir(dir.path())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let sink = lines.clone();
    let reader = std::io::BufReader::new(child.stdout.take().unwrap());
    std::thread::spawn(move || {
        let mut r = reader;
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match std::io::BufRead::read_until(&mut r, b'\n', &mut buf) {
                Ok(0) | Err(_) => return,
                Ok(_) => sink
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buf).into_owned()),
            }
        }
    });
    let count_needle = |needle: &str| -> usize {
        lines
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.contains(needle))
            .count()
    };
    let wait_needle = |needle: &str, n: usize| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while count_needle(needle) < n {
            assert!(
                std::time::Instant::now() < deadline,
                "timeout waiting for {n}x '{needle}'; lines: {:?}",
                lines.lock().unwrap()
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    };
    wait_needle("# pass 1, fail 0", 1);
    dir.child("a.test.js")
        .write_str("console.log(\"v2\");\n")
        .unwrap();
    wait_needle("# pass 1, fail 0", 2);
    assert!(count_needle("v2") > 0, "expected v2 output after rerun");
    let _ = std::process::Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status();
    let status = child.wait().unwrap();
    assert!(
        status.success(),
        "watch must exit 0 on SIGINT, got {status}"
    );
}

// ── Phase 7-e6: bun:ffi（libloading 之上的纯 Rust 动态调用引擎）──────────────

/// 测试 fixture：一个过、一个挂、一个非测试文件（不应被跑）。
fn test_fixture() -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("a.test.js")
        .write_str("import { test } from \"node:test\";\ntest(\"adds\", () => { if (1 + 1 !== 2) throw new Error(\"math\"); });\ntest(\"fails\", () => { throw new Error(\"boom\"); });\n")
        .unwrap();
    dir.child("helper.js")
        .write_str("console.log(\"helper\");\n")
        .unwrap();
    dir
}
