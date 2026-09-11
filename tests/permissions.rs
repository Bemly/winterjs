//! 权限沙箱黑盒测试(对齐 src/permissions.rs:--allow-*)。

mod common;

use common::*;

use assert_fs::prelude::*;

#[test]
fn phase8_permissions_fs() {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("in/sub").create_dir_all().unwrap();
    dir.child("in/a.txt").write_str("hi").unwrap();

    // 默认（无旗标）= 全开放，行为不变
    let file = dir.child("d.mjs");
    file.write_str("console.log(require(\"node:fs\").readFileSync(\"in/a.txt\", \"utf8\"));\n")
        .unwrap();
    let (ok, out, err) = wjs(&["--run", file.path().to_str().unwrap()], &dir);
    assert!(ok, "default open: {err}");
    assert_eq!(out, "hi\n");

    // --allow-read 裸旗标：读放行、写拒绝
    file.write_str(r#"
const fs = require("node:fs");
console.log(fs.readFileSync("in/a.txt", "utf8"));
try { fs.writeFileSync("out.txt", "x"); console.log("WRITE-OK"); } catch (e) { console.log(e.name); }
"#).unwrap();
    let (ok, out, err) = wjs(
        &["--run", file.path().to_str().unwrap(), "--allow-read"],
        &dir,
    );
    assert!(ok, "allow-read: {err}");
    assert_eq!(out, "hi\nPermissionError\n");

    // 路径清单：目录内放行、目录外拒绝（可读错误）
    file.write_str(r#"
const fs = require("node:fs");
try { fs.readFileSync("in/a.txt"); console.log("IN-OK"); } catch (e) { console.log("IN-FAIL", e.name); }
try { fs.readFileSync("/etc/hosts"); console.log("OUT-OK"); } catch (e) { console.log("OUT-FAIL", e.name); }
try { fs.readFileSync("/nonexistent-perm-xyz/f"); } catch (e) { console.log("MISS:", String(e.message).includes("--allow-read")); }
"#).unwrap();
    let (ok, out, err) = wjs(
        &["--run", file.path().to_str().unwrap(), "--allow-read=in"],
        &dir,
    );
    assert!(ok, "allow-list: {err}");
    assert_eq!(out, "IN-OK\nOUT-FAIL PermissionError\nMISS: true\n");

    // --allow-all 全开
    file.write_str(
        r#"
const fs = require("node:fs");
fs.writeFileSync("out2.txt", "z");
console.log(fs.readFileSync("out2.txt", "utf8"));
"#,
    )
    .unwrap();
    let (ok, out, err) = wjs(
        &["--run", file.path().to_str().unwrap(), "--allow-all"],
        &dir,
    );
    assert!(ok, "allow-all: {err}");
    assert_eq!(out, "z\n");
    dir.close().unwrap();
}

#[test]
fn phase8_permissions_env_run() {
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("e.mjs");

    // env：清单授权放行指定键、枚举拒绝；未授权键拒绝
    file.write_str(r#"
console.log(process.env.WJS_TEST_VAR === undefined);
try { console.log(typeof process.env.HOME); } catch (e) { console.log("HOME:", e.name); }
try { Object.keys(process.env); console.log("KEYS-OK"); } catch (e) { console.log("KEYS:", String(e.message).includes("PermissionError")); }
"#).unwrap();
    let (ok, out, err) = wjs(
        &[
            "--run",
            file.path().to_str().unwrap(),
            "--allow-env=WJS_TEST_VAR,HOME",
        ],
        &dir,
    );
    assert!(ok, "env: {err}");
    assert!(out.contains("true"), "out: {out}");
    assert!(out.contains("string"), "out: {out}");
    assert!(out.contains("KEYS: true"), "out: {out}");

    // run：execSync 授权清单按首词匹配
    file.write_str(r#"
const { execSync } = require("node:child_process");
try { console.log(execSync("echo run-ok").toString().trim()); } catch (e) { console.log("ECHO:", e.name); }
try { execSync("ls ."); } catch (e) { console.log("LS:", String(e.message).includes("allow-run")); }
"#).unwrap();
    let (ok, out, err) = wjs(
        &["--run", file.path().to_str().unwrap(), "--allow-run=echo"],
        &dir,
    );
    assert!(ok, "run: {err}");
    assert!(out.contains("run-ok"), "out: {out}");
    assert!(out.contains("LS: true"), "out: {out}");
    dir.close().unwrap();
}

#[cfg(unix)]
#[test]
fn phase8_permissions_sqlite_ffi() {
    let dir = assert_fs::TempDir::new().unwrap();
    let libname = build_ffi_dylib(&dir);
    let file = dir.child("p.mjs");

    // sqlite：未授权读写被拒；--allow-read+--allow-write 放行
    file.write_str(
        r#"
const { Database } = await import("bun:sqlite");
try { new Database("kv.db"); console.log("DB-OK"); } catch (e) { console.log("DB:", e.name); }
"#,
    )
    .unwrap();
    let (ok, out, err) = wjs(
        &["--run", file.path().to_str().unwrap(), "--allow-env"],
        &dir,
    );
    assert!(ok, "sandbox via env: {err}");
    assert!(out.contains("DB: PermissionError"), "out: {out}");
    let (ok, out, err) = wjs(
        &[
            "--run",
            file.path().to_str().unwrap(),
            "--allow-read",
            "--allow-write",
        ],
        &dir,
    );
    assert!(ok, "sqlite allowed: {err}");
    assert!(out.contains("DB-OK"), "out: {out}");

    // ffi：--allow-ffi 才能加载
    file.write_str(&format!(
        r#"
const {{ dlopen, FFIType: T }} = await import("bun:ffi");
try {{ dlopen("./{libname}", {{ ffi_add: {{ args: [T.i32, T.i32], returns: T.i32 }} }}); console.log("FFI-OK"); }} catch (e) {{ console.log("FFI:", e.name, String(e.message).includes("allow-ffi")); }}
"#,
        libname = libname,
    )).unwrap();
    let (ok, out, err) = wjs(
        &["--run", file.path().to_str().unwrap(), "--allow-read"],
        &dir,
    );
    assert!(ok, "ffi denied run: {err}");
    assert!(out.contains("FFI: Error true"), "out: {out}");
    let (ok, out, err) = wjs(
        &["--run", file.path().to_str().unwrap(), "--allow-ffi"],
        &dir,
    );
    assert!(ok, "ffi allowed: {err}");
    assert!(out.contains("FFI-OK"), "out: {out}");
    dir.close().unwrap();
}

// ── Phase 8-a: winterjs lint/fmt（oxlint/oxfmt 命令穿透）────────────────────
