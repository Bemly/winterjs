//! WinterCG 存储黑盒测试（对齐 src/builtins/storage.rs + prelude/storage.rs + CLI --db/--storage-path，S1）。
//! 默认库落在 cwd（`run_fs_file` 的 workdir = TempDir，每测天然隔离）。

mod common;

use assert_fs::prelude::*;
use common::*;

#[test]
fn storage_crud_and_persist() {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("p.mjs")
        .write_str(
            r#"
await storage.set("user:1", { name: "alice", n: 7, tags: ["a"] });
console.log("get", JSON.stringify(await storage.get("user:1")));
console.log("has", await storage.has("user:1"), await storage.has("nope"));
await storage.set("user:2", "bob");
console.log("keys", JSON.stringify(await storage.keys("user:")));
console.log("size", await storage.size());
await storage.set("bin", new Uint8Array([1, 2, 255]));
console.log("u8", Array.from(await storage.get("bin")).join(","));
console.log("del", await storage.delete("user:2"), await storage.get("user:2"));
console.log("deldel", await storage.delete("user:2"));
"#,
        )
        .unwrap();
    let out = winterjs2()
        .args(["--run", "p.mjs", "--storage-path", "t.db"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains(r#"get {"name":"alice","n":7,"tags":["a"]}"#), "out: {stdout}");
    assert!(stdout.contains("has true false"), "out: {stdout}");
    assert!(stdout.contains(r#"keys ["user:1","user:2"]"#), "out: {stdout}");
    assert!(stdout.contains("size 2"), "out: {stdout}");
    assert!(stdout.contains("u8 1,2,255"), "out: {stdout}");
    assert!(stdout.contains("del true null"), "out: {stdout}");
    assert!(stdout.contains("deldel false"), "out: {stdout}");
    // 换进程同文件重读（持久化）+ --db 透传可见。
    let (ok2, stdout2, _) = wjs(
        &["--db", "t.db", "--exec", "SELECT k FROM wjs_kv ORDER BY k"],
        &dir,
    );
    assert!(ok2, "db exec failed");
    assert!(stdout2.contains("\"user:1\"") && stdout2.contains("\"bin\""), "out: {stdout2}");
    assert!(!stdout2.contains("user:2"), "deleted key leaked: {stdout2}");
    dir.close().unwrap();
}

#[test]
fn storage_localstorage_faces() {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("p.mjs")
        .write_str(
            r#"
localStorage.setItem("theme", "dark");
localStorage.setItem("n", 42);
console.log("get", localStorage.getItem("theme"), localStorage.getItem("n"), localStorage.getItem("nope"));
console.log("len", localStorage.length);
console.log("key0", localStorage.key(0));
console.log("keybad", localStorage.key(99), localStorage.key(-1));
localStorage.removeItem("n");
console.log("after", localStorage.getItem("n"), localStorage.length);
console.log("skeys", JSON.stringify(await storage.keys("")));
localStorage.clear();
console.log("cleared", localStorage.length, JSON.stringify(await storage.keys("")));
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
    assert!(stdout.contains("get dark 42 null"), "out: {stdout}");
    assert!(stdout.contains("len 2"), "out: {stdout}");
    assert!(stdout.contains("key0 n"), "out: {stdout}");
    assert!(stdout.contains("keybad null null"), "out: {stdout}");
    assert!(stdout.contains("after null 1"), "out: {stdout}");
    // storage 域与 localStorage 域互不可见。
    assert!(stdout.contains("skeys []"), "out: {stdout}");
    assert!(stdout.contains("cleared 0 []"), "out: {stdout}");
    dir.close().unwrap();
}

#[test]
fn storage_errors_boundary() {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("p.mjs")
        .write_str(
            r#"
const t = async (name, f) => { try { await f(); console.log(name, "NO-THROW"); } catch (e) { console.log(name, e instanceof TypeError); } };
await t("empty", () => storage.get(""));
await t("long", () => storage.get("k".repeat(1025)));
await t("reserved", () => storage.set("__localStorage__:x", 1));
await t("noserial", () => storage.set("k", undefined));
await t("blob", () => storage.set("k", new Blob(["x"])));
await t("badprefix", () => storage.keys(42));
await t("lskey", () => localStorage.getItem(42));
await t("lskey2", () => localStorage.setItem(42, "x"));
console.log("done");
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
    for tag in ["empty", "long", "reserved", "noserial", "blob", "badprefix", "lskey", "lskey2"] {
        assert!(stdout.contains(&format!("{tag} true")), "out: {stdout}");
    }
    assert!(stdout.contains("done"), "out: {stdout}");
    // --db 坏 SQL exit=1；--exec 裸给（无 --db）归属错；--storage-path 裸给归属错。
    let bad = winterjs2()
        .args(["--db", "t.db", "--exec", "NOPE SYNTAX @@"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("db execute failed"), "stderr: {}", String::from_utf8_lossy(&bad.stderr));
    let scope = winterjs2()
        .args(["--run", "p.mjs", "--exec", "SELECT 1"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(scope.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&scope.stderr).contains("--exec only works with --db"), "stderr: {}", String::from_utf8_lossy(&scope.stderr));
    let scope2 = winterjs2()
        .args(["--config", "--storage-path", "x.db"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(scope2.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&scope2.stderr).contains("--storage-path only works with"), "stderr: {}", String::from_utf8_lossy(&scope2.stderr));
    dir.close().unwrap();
}

#[test]
fn storage_default_path_and_isolation() {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("p.mjs")
        .write_str(r#"await storage.set("k", "v"); console.log("ok", await storage.get("k"));"#)
        .unwrap();
    // 缺省路径：cwd 下 winterjs2-storage.db。
    let out = winterjs2()
        .args(["--run", "p.mjs"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(dir.child("winterjs2-storage.db").path().exists(), "default db not created");
    // 两文件隔离（每文件写各自的值）。
    for (f, v) in [("a.db", "va"), ("b.db", "vb")] {
        dir.child("q.mjs")
            .write_str(&format!(r#"await storage.set("k", "{v}");"#))
            .unwrap();
        let o = winterjs2()
            .args(["--run", "q.mjs", "--storage-path", f])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(o.status.success(), "stderr: {}", String::from_utf8_lossy(&o.stderr));
    }
    let (ok, stdout_a, _) = wjs(&["--db", "a.db", "--exec", "SELECT v FROM wjs_kv WHERE k='k'"], &dir);
    assert!(ok);
    assert!(stdout_a.contains("va"), "out: {stdout_a}");
    let (ok, stdout_b, _) = wjs(&["--db", "b.db", "--exec", "SELECT v FROM wjs_kv WHERE k='k'"], &dir);
    assert!(ok);
    assert!(stdout_b.contains("vb"), "out: {stdout_b}");
    // --db 缺省列出表。
    let (ok, stdout_t, _) = wjs(&["--db", "a.db"], &dir);
    assert!(ok);
    assert!(stdout_t.contains("wjs_kv"), "out: {stdout_t}");
    dir.close().unwrap();
}
