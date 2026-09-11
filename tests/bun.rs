//! bun: 内建黑盒测试(对齐 src/builtins/bun/:sqlite/ffi)。

mod common;

use common::*;

use assert_fs::prelude::*;

#[test]
fn phase7_sqlite_memory_roundtrip() {
    // 正常：建表/参数绑定（positional + named）/get/all/values/as/缓存/事务/回滚/iterate/finalize/close。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_sqlite_file(
        &dir,
        "mem.mjs",
        r#"
import { Database, SqliteError } from "bun:sqlite";
const db = new Database(":memory:");
db.exec("CREATE TABLE t (x INTEGER, s TEXT, b BLOB)");
console.log(db.exec("INSERT INTO t VALUES (1, 'one', NULL), (2, 'two', x'00FA07')") === db);
const ins = db.prepare("INSERT INTO t VALUES (?, ?, ?)");
console.log(JSON.stringify(ins.run(3, "three", new Uint8Array([1, 2, 3]))));
console.log(db.run("INSERT INTO t VALUES (4, 'four', ?)", new Uint8Array([9])) === db);
const q = db.query("SELECT x, s, b FROM t ORDER BY x");
console.log(q === db.query("SELECT x, s, b FROM t ORDER BY x"));
const r3 = db.query("SELECT x, s, b FROM t WHERE x = ?").get(3);
console.log(r3.s, r3.b instanceof Uint8Array, r3.b.length);
console.log(JSON.stringify(q.all().map((r) => r.x)), JSON.stringify(q.values().map((r) => r[0])));
console.log(q.as("array").all()[1][2].length, q.as("raw") === q);
console.log(db.query("SELECT x FROM t WHERE s = :s").get({ ":s": "two" }).x);
console.log(db.query("SELECT x FROM t WHERE x = -1").get());
const add = db.transaction((a, b) => {
  if (!db.inTransaction) throw new Error("expected in-transaction");
  db.run("INSERT INTO t (x) VALUES (?)", a);
  db.run("INSERT INTO t (x) VALUES (?)", b);
  return a + b;
});
console.log(add(10, 20));
const bad = db.transaction(() => { db.run("INSERT INTO t (x) VALUES (99)"); throw new Error("boom"); });
try { bad(); } catch (e) { console.log("caught", e.message); }
console.log(JSON.stringify(db.query("SELECT x FROM t WHERE x >= 10 ORDER BY x").values()), db.inTransaction);
let sum = 0;
for (const row of db.query("SELECT x FROM t").iterate()) sum += row.x;
console.log(sum);
const f = db.prepare("SELECT 1 AS one");
f.finalize();
console.log(f.isFinalized);
try { f.get(); } catch (e) { console.log(e.message); }
db.close();
db.close();
console.log(db.isClosed);
try { db.query("SELECT 1"); } catch (e) { console.log(e.name, "|", e.message); }
console.log(typeof SqliteError);
"#,
    );
    assert_eq!(
        out,
        "true\n{\"changes\":1,\"lastInsertRowid\":3}\ntrue\ntrue\nthree true 3\n[1,2,3,4] [1,2,3,4]\n3 false\n2\nnull\n30\ncaught boom\n[[10],[20]] false\n40\ntrue\nstatement is finalized\ntrue\nSqliteError | database is not open\nfunction\n",
        "sqlite mem: {out}"
    );
    dir.close().unwrap();
}

#[test]
fn phase7_sqlite_file_persist_and_errors() {
    // 正常：文件库写盘→关→重开读回（blob 原样）+ filename。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_sqlite_file(
        &dir,
        "a.mjs",
        r#"
import { Database } from "bun:sqlite";
const db = new Database("kv.db");
db.run("CREATE TABLE kv (k TEXT PRIMARY KEY, v BLOB)");
db.run("INSERT INTO kv VALUES ('bin', ?)", new Uint8Array([0, 255, 7]));
db.close();
const db2 = new Database("kv.db");
const back = db2.query("SELECT v FROM kv WHERE k = 'bin'").get();
console.log(back.v instanceof Uint8Array, back.v.length, back.v[1], db2.filename.endsWith("kv.db"));
db2.close();
"#,
    );
    assert_eq!(out, "true 3 255 true\n", "persist: {out}");

    // 报错三件：坏路径构造即抛 / 坏 SQL / 类型拒绝（bool/NaN/无名前缀）/finalize 后使用。
    let file = dir.child("b.mjs");
    file.write_str(r#"
import { Database } from "bun:sqlite";
try { new Database("/nonexistent-wjs-dir/x.db"); } catch (e) { console.log("open:", e.name); }
const db = new Database(":memory:");
try { db.query("SELECT * FROM").all(); } catch (e) { console.log("sql:", e.name); }
try { db.run("INSERT INTO t VALUES (?)", true); } catch (e) { console.log("bool:", e.name, "|", e.message); }
try { db.run("INSERT INTO t VALUES (?)", NaN); } catch (e) { console.log("nan:", e.name); }
try { db.run("INSERT INTO t VALUES (:x)", { no_prefix: 1 }); } catch (e) { console.log("named:", e.name); }
const s = db.prepare("SELECT 1");
s.finalize();
try { s.get(); } catch (e) { console.log("fin:", e.name); }
console.log("done");
"#).unwrap();
    let out3 = winterjs()
        .arg("--run")
        .arg(file.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out3.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out3.stderr)
    );
    let so = String::from_utf8(out3.stdout).unwrap();
    assert_eq!(
        so,
        "open: SqliteError\nsql: SqliteError\nbool: TypeError | Unsupported parameter type: boolean\nnan: SqliteError\nnamed: SqliteError\nfin: SqliteError\ndone\n",
        "errors: {so}"
    );
    dir.close().unwrap();
}

#[test]
fn phase7_sqlite_unknown_spec() {
    // 边界：未知 bun: 内建整跑失败，报错含可用列表（与裸导入报错同路径）；
    // 可用项 `bun:sqlite` 动态导入正常。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("c.mjs");
    file.write_str("import \"bun:nosuch\";\n").unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(file.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!out.status.success(), "expected failure");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("bun:sqlite"), "stderr: {err}");
    let ok = dir.child("d.mjs");
    ok.write_str(
        "const { Database } = await import(\"bun:sqlite\");\nconsole.log(typeof Database);\n",
    )
    .unwrap();
    let out2 = winterjs()
        .arg("--run")
        .arg(ok.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out2.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out2.stderr)
    );
    assert_eq!(String::from_utf8(out2.stdout).unwrap(), "function\n");
    dir.close().unwrap();
}

#[cfg(unix)]
#[test]
fn phase7_ffi_dylib() {
    // 正常：整数/混合类别/void+指针写回（零拷贝语义）/u8/f32 返回/CString/toBuffer。
    let dir = assert_fs::TempDir::new().unwrap();
    let libname = build_ffi_dylib(&dir);
    let file = dir.child("ffi.mjs");
    file.write_str(&format!(
        r#"
import {{ dlopen, FFIType as T, suffix, ptr, CString, toBuffer }} from "bun:ffi";
if (suffix !== "{suffix}") throw new Error("bad suffix: " + suffix);
const lib = dlopen("./{libname}", {{
  ffi_add: {{ args: [T.i32, T.i32], returns: T.i32 }},
  ffi_mul64: {{ args: [T.i64, T.i64], returns: T.i64 }},
  ffi_mix: {{ args: [T.i32, T.f64], returns: T.f64 }},
  ffi_sum3: {{ args: [T.f64, T.f64, T.f64], returns: T.f64 }},
  ffi_is_even: {{ args: [T.u32], returns: T.u8 }},
  ffi_fill: {{ args: [T.ptr, T.i32, T.u8], returns: T.void }},
  ffi_count_zeros: {{ args: [T.ptr, T.i32], returns: T.i32 }},
  ffi_hello: {{ returns: T.ptr }},
  ffi_f32ret: {{ args: [T.f64], returns: T.f32 }},
}});
console.log(lib.symbols.ffi_add(2, 3), lib.symbols.ffi_mul64(3, 4));
console.log(lib.symbols.ffi_mix(1, 0.5), lib.symbols.ffi_sum3(1, 2, 3.5));
console.log(lib.symbols.ffi_is_even(10), lib.symbols.ffi_is_even(7));
const buf = new Uint8Array(4);
console.log(lib.symbols.ffi_fill(ptr(buf), 4, 0xab) === undefined, buf[0] === 0xab && buf[3] === 0xab);
console.log(lib.symbols.ffi_count_zeros(ptr(new Uint8Array([1, 0, 2, 0, 0])), 5));
console.log(lib.symbols.ffi_count_zeros(ptr("abc"), 4));
const cs = new CString(lib.symbols.ffi_hello());
console.log(cs.toString(), cs.ptr !== 0, cs.length);
console.log(lib.symbols.ffi_f32ret(21));
const tb = toBuffer(ptr(new Uint8Array([7, 8])), 2);
console.log(tb instanceof Uint8Array, tb[0], tb.length);
console.log(typeof lib.symbols.ffi_add);
"#,
        suffix = if cfg!(target_os = "macos") { ".dylib" } else { ".so" },
        libname = libname,
    ))
    .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(file.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let so = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        so, "5 12\n1.5 6.5\n1 0\ntrue true\n3\n1\nhi from c true 9\n42\ntrue 7 2\nfunction\n",
        "ffi: {so}"
    );
    dir.close().unwrap();
}

#[cfg(unix)]
#[test]
fn phase7_ffi_errors() {
    // 报错三件：坏路径/缺符号/arity 不匹配/f32 参数/未知类型/null CString。
    let dir = assert_fs::TempDir::new().unwrap();
    let libname = build_ffi_dylib(&dir);
    let file = dir.child("err.mjs");
    file.write_str(&format!(
        r#"
import {{ dlopen, FFIType as T, suffix, ptr, CString }} from "bun:ffi";
try {{ dlopen("/nonexistent-ffi-xyz/libnope" + suffix, {{}}); }} catch (e) {{ console.log("load:", String(e.message).includes("nonexistent-ffi-xyz")); }}
try {{ dlopen("./{libname}", {{ nope: T.i32 }}); }} catch (e) {{ console.log("symbol:", String(e.message).includes("nope")); }}
const lib = dlopen("./{libname}", {{ ffi_add: {{ args: [T.i32, T.i32], returns: T.i32 }} }});
try {{ lib.symbols.ffi_add(1); }} catch (e) {{ console.log("arity:", e.message.startsWith("FFI call 'ffi_add'")); }}
try {{ lib.symbols.ffi_add(1, "x"); }} catch (e) {{ console.log("argtype:", String(e.message).includes("must be a number")); }}
try {{ dlopen("./{libname}", {{ bad: {{ args: [T.f32], returns: T.void }} }}); }} catch (e) {{ console.log("f32arg:", e.name, String(e.message).includes("f32")); }}
try {{ dlopen("./{libname}", {{ bad: T.nope }}); }} catch (e) {{ console.log("type:", e.name); }}
try {{ dlopen("./{libname}", {{ bad: {{ args: "nope", returns: T.void }} }}); }} catch (e) {{ console.log("argsfmt:", e.name); }}
try {{ ptr({{}}); }} catch (e) {{ console.log("ptrtype:", e.name); }}
try {{ new CString(0); }} catch (e) {{ console.log("nullptr:", String(e.message)); }}
console.log("done");
"#,
        libname = libname,
    ))
    .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(file.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let so = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        so,
        "load: true\nsymbol: true\narity: true\nargtype: true\nf32arg: TypeError true\ntype: TypeError\nargsfmt: TypeError\nptrtype: TypeError\nnullptr: RangeError: CString: null pointer\ndone\n",
        "ffi errors: {so}"
    );
    dir.close().unwrap();
}

// ── Phase 8-b: --allow-* 权限开关（opt-in 沙箱）─────────────────────────────

fn run_sqlite_file(dir: &assert_fs::TempDir, name: &str, source: &str) -> String {
    let file = dir.child(name);
    file.write_str(source).unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(file.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}
