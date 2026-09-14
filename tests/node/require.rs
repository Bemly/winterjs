//! tests/node/require.rs — 对齐 src/builtins/node/require.rs（require/CJS 互操作/extensions）。

use crate::common::*;
use assert_fs::prelude::*;

#[test]
fn phase4_require_cjs_builtin_relative_json() {
    // CJS 文件 + 内建 + JSON + 相对路径 + require.main（经 .cjs 入口）。
    let dir = assert_fs::TempDir::new().unwrap();
    let lib = dir.child("lib/util.cjs");
    lib.write_str("const path = require(\"node:path\");\nmodule.exports = { joined: path.join(\"a\", \"b\") };\n").unwrap();
    let data = dir.child("lib/data.json");
    data.write_str("{\"answer\": 42}").unwrap();
    let main = dir.child("main.cjs");
    main.write_str("const u = require(\"./lib/util.cjs\");\nconst d = require(\"./lib/data.json\");\nconsole.log(\"main:\", u.joined, d.answer, __filename.endsWith(\"main.cjs\"), require.main.filename.endsWith(\"main.cjs\"));\n").unwrap();
    let out = winterjs().arg("--run").arg(main.path()).output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout, "main: a/b 42 true true\n", "require: {stdout}");
    dir.close().unwrap();
}

#[test]
fn phase4_require_cycle_partial_exports() {
    // 循环引用见半成品（Node 语义）。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("b.cjs")
        .write_str(
            "const a = require(\"./a.cjs\");\nmodule.exports = { b: 2, aVal: (a.a || 0) + 10 };\n",
        )
        .unwrap();
    dir.child("a.cjs")
        .write_str(
            "const b = require(\"./b.cjs\");\nmodule.exports = { a: 1, bVal: (b.b || 0) + 100 };\n",
        )
        .unwrap();
    let main = dir.child("main.cjs");
    main.write_str("const a = require(\"./a.cjs\");\nconsole.log(\"cycle:\", a.a, a.bVal);\n")
        .unwrap();
    let out = winterjs().arg("--run").arg(main.path()).output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "cycle: 1 102\n");
    dir.close().unwrap();
}

#[test]
fn phase4_require_errors() {
    // 缺失模块 / ESM 拒绝 / resolve 直给。
    let out = winterjs().args(["--eval", "try { require(\"node:nope-xyz\"); } catch (e) { console.log(e.message.slice(0, 30)); }"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Cannot find module"), "missing: {stdout}");
    let dir = assert_fs::TempDir::new().unwrap();
    let mod_ = dir.child("m.mjs");
    mod_.write_str("export const x = 1;\n").unwrap();
    let code = format!(
        "try {{ require({:?}); }} catch (e) {{ console.log(e.message.slice(0, 30)); }}",
        mod_.path().to_string_lossy()
    );
    let out = winterjs().args(["--eval", &code]).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("require() of ES Module"), "esm: {stdout}");
    let out =
        stdout_of(&mut winterjs().args(["--eval", "console.log(require.resolve(\"node:path\"));"]));
    assert_eq!(out, "node:path\n", "resolve: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9k_module_extensions_hook() {
    // 正常：createRequire 实例的 extensions 钩子 + module._compile 内存求值
    // （vite loadConfigFromBundledFile 形态：内存码优先于磁盘，filename 走
    // realpath 口径）；exports 重赋值终态；cache 命中（Node 口径 cache 先于
    // extensions，vite delete cache[resolve] 即为绕过）；.js 兜底（loaderExt）。
    // 报错：无钩子回落 native（磁盘 ESM 经 require 报经典 SyntaxError 文案）。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("esm-target.js").write_str("export default 1;\n").unwrap();
    dir.child("fresh-esm.js").write_str("export default 2;\n").unwrap();
    dir.child("reassign.js").write_str("module.exports = { disk: true };\n").unwrap();
    dir.child("noext.cfg").write_str("anything\n").unwrap();
    let file = dir.child("m.mjs");
    file.write_str(
        r#"
import { createRequire } from "node:module";
const req = createRequire(import.meta.url);

req.extensions[".js"] = (mod, filename) => {
  mod._compile("module.exports = { v: 42, who: __filename };", filename);
};
const m = req("./esm-target.js");
console.log("hooked", m.v, m.who.endsWith("esm-target.js"));

req.extensions[".js"] = (mod, filename) => {
  mod._compile("module.exports = { reassigned: true };", filename);
};
console.log("reassigned", req("./reassign.js").reassigned);

let calls = 0;
req.extensions[".js"] = (mod, fn) => { calls++; mod._compile("module.exports = { n: " + calls + " };", fn); };
delete req.cache[req.resolve("./esm-target.js")];
const a = req("./esm-target.js");
const b = req("./esm-target.js");
console.log("cache", a.n === b.n, calls);

delete req.extensions[".js"];
try { req("./fresh-esm.js"); console.log("NO-ERR"); }
catch (e) { console.log("native-err", e.constructor.name, String(e.message).slice(0, 60)); }

req.extensions[".js"] = (mod, fn) => { mod._compile("module.exports = { via: 'fallback' };", fn); };
console.log("fallback", req("./noext.cfg").via);
console.log("ext-ok");
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
        "hooked 42 true",
        "reassigned true",
        "cache true 1",
        "native-err Error export declarations may only appear at top level of a module",
        "fallback fallback",
        "ext-ok",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase9j_cjs_interop_default() {
    // CJS 互操作（Node detect-module 口径）：import 命中 .cjs/无语法 .js 即 default；
    // require() 同一文件值同一；副作用 import 照跑；命名导入直取（M5 具名导出，
    // 见 phase9j_cjs_interop_named；旧"缺导出"断言随功能上线退役）。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("dep.cjs").write_str("module.exports = { v: 41 };\n").unwrap();
    dir.child("plain.js").write_str("module.exports = { w: 7 };\n").unwrap();
    dir.child("side.cjs").write_str("globalThis.__wjs_side = 1;\n").unwrap();
    let file = dir.child("m.mjs");
    file.write_str(
        r#"
import pkg from "./dep.cjs";
import plain from "./plain.js";
import "./side.cjs";
console.log("cjs-def", pkg.v, plain.w, globalThis.__wjs_side);
console.log("cjs-same", globalThis.require("./dep.cjs") === pkg);
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
    for line in ["cjs-def 41 7 1", "cjs-same true"] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    // 边界：CJS 垫片具名直取成功（M5 具名导出；与 Node 同为 link 期解析）。
    let named = dir.child("n.mjs");
    named.write_str("import { v } from \"./dep.cjs\";\nconsole.log(\"cjs-named\", v);\n").unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(named.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "cjs-named 41\n",
        "named from CJS: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    dir.close().unwrap();
}

#[test]
fn phase9j_native_node_rejected() {
    // napi（.node，plan-napi M0）：垃圾 .node → dlopen 可读错（9j 的"不支持"
    // 拒错随 napi 落地退役；非 Mach-O 文件进 dlopen 即可读失败）。
    let dir = assert_fs::TempDir::new().unwrap();
    let fake = dir.child("fake.node");
    fake.write_str("not a real binary").unwrap();
    let out = winterjs()
        .args(["--eval", &format!("try {{ require({:?}); }} catch (e) {{ console.log(String(e.message).slice(0, 200)); }}", fake.path().to_string_lossy())])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("cannot load native module") || stdout.contains("dlopen"),
        "stdout: {stdout}"
    );
    dir.close().unwrap();
}

#[test]
fn phase9j_tla_dep_stays_esm() {
    // 回归（CJS 互操作曾吞掉它）：TLA 专属 .js 被 import 时仍走 ESM，不进垫片。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("tla-dep.js")
        .write_str("const v = await Promise.resolve(6);\nexport default v * 7;\n")
        .unwrap();
    let file = dir.child("m.mjs");
    file.write_str("import v from \"./tla-dep.js\";\nconsole.log(\"tla-dep\", v);\n").unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(file.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "tla-dep 42\n");
    dir.close().unwrap();
}

#[test]
fn phase9j_cjs_interop_named() {
    // 正常：CJS 命名导出（exports 赋值 + module.exports 对象 + TS __exportStar
    // 形）经 import 具名直取，default 照旧是整包；
    // 报错：不存在的命名报 link 错误；边界：`exports.default` 不合成具名
    // default（default 整包 `.default` 直通），kebab 键引号形往返。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("named.cjs")
        .write_str("exports.a = 1;\nexports['kebab-key'] = 2;\nexports.default = 5;\n")
        .unwrap();
    dir.child("obj.cjs")
        .write_str("module.exports = { add: (x, y) => x + y, nested: { v: 3 } };\n")
        .unwrap();
    dir.child("star-a.cjs")
        .write_str("exports.x = 10;\n")
        .unwrap();
    dir.child("star.cjs")
        .write_str("function __es(r) { for (const k in r) { if (k !== 'default') exports[k] = r[k]; } }\n__es(require('./star-a.cjs'));\nexports.y = 20;\n")
        .unwrap();
    let file = dir.child("m.mjs");
    file.write_str("import \"./mix.mjs\";\n").unwrap();
    dir.child("mix.mjs")
        .write_str(
            r#"
import { a, default as d } from "./named.cjs";
import defObj, { add, nested } from "./obj.cjs";
import { x, y } from "./star.cjs";
import { default as stard } from "./star.cjs";
import { default as namedd } from "./named.cjs";
import { "kebab-key" as kebab } from "./named.cjs";
console.log("named", a, add(19, 23), nested.v, x, y);
console.log("defaults", typeof d, d.a, typeof defObj, namedd.default, stard.y);
console.log("kebab", kebab);
"#,
        )
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
    let out = String::from_utf8(out.stdout).unwrap();
    for line in ["named 1 42 3 10 20", "defaults object 1 object 5 20", "kebab 2"] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    // 报错：不存在的命名 link 期即错（非运行时 undefined）。
    let bad = dir.child("bad.mjs");
    bad.write_str("import { nope_missing_xyz } from \"./named.cjs\";\nconsole.log(nope_missing_xyz);\n")
        .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(bad.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "missing-name link should fail"
    );
    dir.close().unwrap();
}
