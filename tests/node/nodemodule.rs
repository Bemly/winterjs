//! tests/node/nodemodule.rs — 对齐 src/builtins/node/nodemodule.rs（node:module createRequire）。

use crate::common::*;
use assert_fs::prelude::*;

#[test]
fn phase9j_module_create_require() {
    // 正常：createRequire(file URL) 读 CJS/JSON/内建 + resolve；
    // Module.createRequire 同口径；builtinModules 双形/isBuiltin/sync 无操作。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("helper.cjs").write_str("module.exports = { v: 41 };\n").unwrap();
    dir.child("data.json").write_str("{\"n\": 7}\n").unwrap();
    let file = dir.child("m.mjs");
    file.write_str(
        r#"
import { createRequire, builtinModules, isBuiltin, Module } from "node:module";
const req = createRequire(import.meta.url);
console.log("cr-cjs", req("./helper.cjs").v);
console.log("cr-json", req("./data.json").n);
console.log("cr-builtin", typeof req("node:path").join);
console.log("cr-resolve", req.resolve("./helper.cjs").endsWith("helper.cjs"));
const req2 = Module.createRequire(import.meta.url);
console.log("mod-cr", req2("./helper.cjs").v);
console.log("bl", builtinModules.includes("node:module") && builtinModules.includes("module") && isBuiltin("fs") && isBuiltin("node:fs") && !isBuiltin("node:nope"));
console.log("sync", Module.syncBuiltinESMExports() === undefined);
try { req("./nope-missing-xyz.cjs"); } catch (e) { console.log("miss", String(e.message).includes("Cannot find module")); }
try { createRequire(42); } catch (e) { console.log("badbase", e.code); }
try { Module.register(); } catch (e) { console.log("reg", e.code); }
"#,
    )
    .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(file.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    for line in [
        "cr-cjs 41",
        "cr-json 7",
        "cr-builtin function",
        "cr-resolve true",
        "mod-cr 41",
        "bl true",
        "sync true",
        "miss true",
        "badbase ERR_INVALID_ARG_TYPE",
        "reg ERR_METHOD_NOT_IMPLEMENTED",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}
