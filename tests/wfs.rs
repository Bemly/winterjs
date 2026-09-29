//! 本体 FS 黑盒（对齐 src/builtins/wfs.rs + prelude/wfs.rs，与 node:fs 分离）。
//! 正常 + 报错 + 边界三件；UNSAFE-BOUNDARY panic 路径用例。

mod common;

use assert_fs::prelude::*;
use common::*;

/// 正常：write/read/stat/mkdir/readdir/rename/copy/remove/exists 全链。
#[test]
fn wfs_read_write_stat() {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("p.mjs")
        .write_str(
            r#"
console.log("wfs", typeof fs, typeof WinterJS2.fs, fs === WinterJS2.fs);
await fs.writeFile("a.txt", "hello");
await fs.writeFile("b.bin", new Uint8Array([1, 2, 255]));
console.log("text", await fs.readTextFile("a.txt"));
console.log("bin", Array.from(await fs.readFile("b.bin")).join(","));
console.log("stat", JSON.stringify(await fs.stat("a.txt")));
console.log("exists", await fs.exists("a.txt"), await fs.exists("nope"));
await fs.rename("a.txt", "c.txt");
console.log("ren", await fs.exists("a.txt"), await fs.exists("c.txt"));
await fs.copyFile("c.txt", "d.txt");
console.log("cp", await fs.readTextFile("d.txt"));
await fs.remove("b.bin", {});
console.log("rm", await fs.exists("b.bin"));
"#,
        )
        .unwrap();
    let out = winterjs2()
        .args(["--run", "p.mjs"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("wfs object object true"), "out: {stdout}");
    assert!(stdout.contains("text hello"), "out: {stdout}");
    assert!(stdout.contains("bin 1,2,255"), "out: {stdout}");
    assert!(stdout.contains("\"isFile\":true"), "out: {stdout}");
    assert!(stdout.contains("exists true false"), "out: {stdout}");
    assert!(stdout.contains("ren false true"), "out: {stdout}");
    assert!(stdout.contains("cp hello"), "out: {stdout}");
    assert!(stdout.contains("rm false"), "out: {stdout}");
    dir.close().unwrap();
}

/// 正常：mkdir/readdir 链。
#[test]
fn wfs_mkdir_readdir_remove() {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("p.mjs")
        .write_str(
            r#"
await fs.mkdir("sub/nest", { recursive: true });
await fs.writeFile("sub/nest/f.txt", "x");
await fs.writeFile("sub/a.txt", "y");
console.log("dir", JSON.stringify(await fs.readdir("sub")));
console.log("nest", JSON.stringify(await fs.readdir("sub/nest")));
await fs.remove("sub", { recursive: true });
console.log("gone", await fs.exists("sub"));
"#,
        )
        .unwrap();
    let out = winterjs2()
        .args(["--run", "p.mjs"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("\"name\":\"a.txt\""), "out: {stdout}");
    assert!(stdout.contains("\"name\":\"nest\""), "out: {stdout}");
    assert!(stdout.contains("\"name\":\"f.txt\""), "out: {stdout}");
    assert!(stdout.contains("gone false"), "out: {stdout}");
    dir.close().unwrap();
}

/// 报错 + 边界：空路径/缺文件/非递归删非空目录/坏 base64 经 native 侧。
#[test]
fn wfs_errors_boundary() {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("p.mjs")
        .write_str(
            r#"
const t = async (name, f) => {
  try { await f(); console.log(name, "NO-THROW"); }
  catch (e) { console.log(name, "THROW", e.constructor.name); }
};
await t("empty", () => fs.readFile(""));
await t("missing", () => fs.readFile("nope.txt"));
await t("stat-missing", () => fs.stat("nope.txt"));
await t("baddata", () => fs.writeFile("", "x"));
await fs.mkdir("d", {});
await fs.writeFile("d/f.txt", "x");
await t("rmdir-nonempty", () => fs.remove("d", {}));
console.log("exists-empty-throw-check", await fs.exists("d/f.txt"));
try { __wjs2_wfs_read("ok-but-native"); console.log("native-no-throw"); }
catch (e) { console.log("native-throw", e.constructor.name); }
"#,
        )
        .unwrap();
    let out = winterjs2()
        .args(["--run", "p.mjs"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("empty THROW TypeError"), "out: {stdout}");
    assert!(stdout.contains("missing THROW"), "out: {stdout}");
    assert!(stdout.contains("stat-missing THROW"), "out: {stdout}");
    assert!(stdout.contains("baddata THROW TypeError"), "out: {stdout}");
    assert!(stdout.contains("rmdir-nonempty THROW"), "out: {stdout}");
    assert!(stdout.contains("exists-empty-throw-check true"), "out: {stdout}");
    dir.close().unwrap();
}
