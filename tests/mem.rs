//! 本体内存黑盒（WinterJS2.memory/alloc/unsafe*；unsafe 面需 --allow-ffi）。

mod common;

use assert_fs::prelude::*;
use common::*;

fn eval_ok(code: &str) -> String {
    let out = winterjs2().args(["--eval", code]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn mem_info_and_alloc() {
    let stdout = eval_ok(
        r#"console.log("mem", JSON.stringify(WinterJS2.memory())); const a = WinterJS2.alloc(4); console.log("alloc", a.length, Array.from(a).join(",")); a[0] = 7; console.log("w", a[0]);"#,
    );
    assert!(stdout.contains("\"allocator\":\"smmalloc\"") || stdout.contains("\"allocator\":\"talc\""), "out: {stdout}");
    assert!(stdout.contains("alloc 4 0,0,0,0"), "out: {stdout}");
    assert!(stdout.contains("w 7"), "out: {stdout}");
}

#[test]
fn mem_unsafe_heap() {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("p.mjs")
        .write_str(
            r#"
const id = WinterJS2.unsafeAlloc(8);
console.log("size", WinterJS2.unsafeSize(id));
WinterJS2.unsafeWrite(id, 0, new Uint8Array([1, 2, 3]));
WinterJS2.unsafeWrite(id, 3, "hi");
console.log("read", Array.from(WinterJS2.unsafeRead(id, 0, 5)).join(","));
console.log("list", JSON.stringify(WinterJS2.unsafeList()).includes(String(id)));
WinterJS2.unsafeFree(id);
console.log("freed", JSON.stringify(WinterJS2.unsafeList()).includes(String(id)));
"#,
        )
        .unwrap();
    let out = winterjs2()
        .args(["--run", "p.mjs", "--allow-ffi"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("size 8"), "out: {stdout}");
    assert!(stdout.contains("read 1,2,3,104,105"), "out: {stdout}");
    assert!(stdout.contains("list true"), "out: {stdout}");
    assert!(stdout.contains("freed false"), "out: {stdout}");
    dir.close().unwrap();
}

#[test]
fn mem_errors_boundary() {
    let stdout = eval_ok(
        r#"const t = (n, f) => { try { f(); console.log(n, "NO-THROW"); } catch (e) { console.log(n, "THROW", e.constructor.name); } }; t("neg", () => WinterJS2.alloc(-1)); t("frac", () => WinterJS2.alloc(1.5)); t("huge", () => WinterJS2.alloc(256 * 1024 * 1024));"#,
    );
    assert!(stdout.contains("neg THROW TypeError"), "out: {stdout}");
    assert!(stdout.contains("frac THROW TypeError"), "out: {stdout}");
    assert!(stdout.contains("huge THROW TypeError"), "out: {stdout}");
    // 沙箱内未给 --allow-ffi：unsafe 面拒（无 flag 时全开放，故此处带 --allow-read 进沙箱）。
    let out = winterjs2()
        .args(["--eval", r#"try { WinterJS2.unsafeAlloc(8); console.log("noffi NO-THROW"); } catch (e) { console.log("noffi THROW"); }"#, "--allow-read"])
        .output()
        .unwrap();
    let stdout2 = String::from_utf8(out.stdout).unwrap();
    assert!(stdout2.contains("noffi THROW"), "out: {stdout2}");
}
