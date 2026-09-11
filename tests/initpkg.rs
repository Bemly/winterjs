//! winterjs init 黑盒测试(对齐 src/initpkg.rs)。

mod common;

use common::*;

#[test]
fn phase7_init_closed_loop() {
    // 正常：init 三件 + 内容含名 + 紧接着 `test` 即绿（闭环）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = stdout_of(
        winterjs()
            .args(["--init", "my-pkg", "--yes"])
            .current_dir(dir.path()),
    );
    assert!(
        out.contains("created package.json") && out.contains("created hello.test.js"),
        "init:\n{out}"
    );
    let pkg = std::fs::read_to_string(dir.path().join("package.json")).unwrap();
    assert!(pkg.contains("\"my-pkg\""), "package.json:\n{pkg}");
    let out = winterjs()
        .arg("--test")
        .arg(".")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("ok - hello.test.js"));
    dir.close().unwrap();
}

#[test]
fn phase7_init_bad_name() {
    // 报错：非法名 exit=1 且可读。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .args(["--init", "Bad Name!", "--yes"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("bad package name"), "stderr: {stderr}");
    dir.close().unwrap();
}

#[test]
fn phase7_init_conflict() {
    // 边界：已存在文件不覆盖，第二次 init exit=1 且一个不写。
    let dir = assert_fs::TempDir::new().unwrap();
    assert!(
        winterjs()
            .args(["--init", "p", "--yes"])
            .current_dir(dir.path())
            .output()
            .unwrap()
            .status
            .success()
    );
    std::fs::write(dir.path().join("index.js"), b"mine\n").unwrap();
    let out = winterjs()
        .args(["--init", "p", "--yes"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("refusing to overwrite"));
    assert_eq!(
        std::fs::read(dir.path().join("index.js")).unwrap(),
        b"mine\n"
    );
    dir.close().unwrap();
}

#[test]
fn phase7_init_needs_yes_without_tty() {
    // 边界：非 TTY 缺 --yes 即报可读错（不挂起等输入）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .args(["--init", "p"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--yes"));
    assert!(
        !dir.path().join("package.json").exists(),
        "nothing must be written"
    );
    dir.close().unwrap();
}
