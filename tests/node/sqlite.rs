//! tests/node/sqlite.rs — 对齐 src/builtins/node/sqlite.rs（node:sqlite，10d）。

use crate::helpers::*;

#[test]
fn phase10d_sqlite_crud() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { DatabaseSync } from "node:sqlite";
const db = new DatabaseSync(":memory:");
console.log("open", db.isOpen, db.location());
db.exec("CREATE TABLE t(x INT, name TEXT)");
console.log("run", JSON.stringify(db.prepare("INSERT INTO t VALUES (?,?)").run(1, "a")));
console.log("get", JSON.stringify(db.prepare("SELECT * FROM t").get()));
console.log("all", JSON.stringify(db.prepare("SELECT * FROM t").all()));
console.log("iter", JSON.stringify([...db.prepare("SELECT * FROM t").iterate()]));
console.log("cols", JSON.stringify(db.prepare("SELECT x AS v FROM t").columns()).slice(0, 60));
console.log("named", JSON.stringify(db.prepare("SELECT * FROM t WHERE x=:v").get({ v: 1 })));
console.log("miss-get", String(db.prepare("SELECT * FROM t WHERE 0").get()));
console.log("miss-all", JSON.stringify(db.prepare("SELECT * FROM t WHERE 0").all()));
const s = db.prepare("SELECT 1 AS one, 2 AS two");
s.setReturnArrays(true);
console.log("arr", JSON.stringify(s.all()));
db.close();
console.log("closed", db.isOpen);
const f = new DatabaseSync("f.db");
f.exec("CREATE TABLE k(v TEXT)");
f.prepare("INSERT INTO k VALUES (?)").run("hello");
f.close();
const f2 = new DatabaseSync("f.db");
console.log("persist", JSON.stringify(f2.prepare("SELECT * FROM k").get()));
console.log("floc", f2.location().endsWith("f.db"), f2.isOpen);
f2.close();
"#,
    );
    assert!(out.contains("open true null"), "out: {out}");
    assert!(out.contains(r#"run {"changes":1,"lastInsertRowid":1}"#), "out: {out}");
    assert!(out.contains(r#"get {"x":1,"name":"a"}"#), "out: {out}");
    assert!(out.contains(r#"all [{"x":1,"name":"a"}]"#), "out: {out}");
    assert!(out.contains(r#"iter [{"x":1,"name":"a"}]"#), "out: {out}");
    assert!(out.contains("cols ["), "out: {out}");
    assert!(out.contains(r#"named {"x":1,"name":"a"}"#), "out: {out}");
    assert!(out.contains("miss-get undefined"), "out: {out}");
    assert!(out.contains("miss-all []"), "out: {out}");
    assert!(out.contains("arr [[1,2]]"), "out: {out}");
    assert!(out.contains("closed false"), "out: {out}");
    assert!(out.contains(r#"persist {"v":"hello"}"#), "out: {out}");
    assert!(out.contains("floc true true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase10d_sqlite_errors_boundary() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { DatabaseSync } from "node:sqlite";
const db = new DatabaseSync(":memory:");
db.exec("CREATE TABLE t(x)");
try { db.prepare("NOPE SYNTAX @@"); } catch (e) { console.log("prep-err", e.code); }
try { db.prepare(""); } catch (e) { console.log("empty", e.code); }
try { db.prepare("SELECT :a").get({ a: 1, b: 2 }); } catch (e) { console.log("unknown", e.code); }
const s = db.prepare("SELECT :a AS v");
s.setAllowUnknownNamedParameters(true);
console.log("unknown-allow", JSON.stringify(s.get({ a: 7, b: 2 })));
db.exec("CREATE TABLE bi(v BIGINT)");
db.prepare("INSERT INTO bi VALUES (?)").run(9007199254740993n);
try { db.prepare("SELECT v FROM bi").get(); } catch (e) { console.log("big-err", e.code); }
const sb = db.prepare("SELECT v FROM bi");
sb.setReadBigInts(true);
console.log("big-ok", typeof sb.get().v, String(sb.get().v));
db.close();
try { db.prepare("SELECT 1"); } catch (e) { console.log("closed", e.code); }
try { db.exec("SELECT 1"); } catch (e) { console.log("exec-closed", e.code); }
try { new DatabaseSync("/no/such/dir/x.db"); } catch (e) { console.log("open-err", e.code); }
console.log("end-ok");
"#,
    );
    assert!(out.contains("prep-err ERR_SQLITE_ERROR"), "out: {out}");
    assert!(out.contains("empty ERR_INVALID_ARG_VALUE"), "out: {out}");
    assert!(out.contains("unknown ERR_INVALID_STATE"), "out: {out}");
    assert!(out.contains(r#"unknown-allow {"v":7}"#), "out: {out}");
    assert!(out.contains("big-err ERR_OUT_OF_RANGE"), "out: {out}");
    assert!(out.contains("big-ok bigint 9007199254740993"), "out: {out}");
    assert!(out.contains("closed ERR_INVALID_STATE"), "out: {out}");
    assert!(out.contains("exec-closed ERR_INVALID_STATE"), "out: {out}");
    assert!(out.contains("open-err ERR_SQLITE_ERROR"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}
