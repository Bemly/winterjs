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
    // require(esm)（Node ≥22.12/26 无条件，真机 26.8.2 实测同款成功）：
    // .mjs 直接 require 返回 namespace（旧"ESM 拒绝"断言随语义升级退役）。
    let code = format!(
        "try {{ const m = require({:?}); console.log(\"esm\", m.x); }} catch (e) {{ console.log(\"esm-err\", e.constructor.name); }}",
        mod_.path().to_string_lossy()
    );
    let out = winterjs().args(["--eval", &code]).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("esm 1"), "esm: {stdout}");
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
    // 报错改成功：无钩子回落 native 后 require(typeless ESM .js) 经
    // detect-module + require(esm) 成功（真机 26.8.2 实测同款；旧"经典
    // SyntaxError"断言随语义升级退役）。
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
        "NO-ERR",
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
    // 转出跟随用 TS __exportStar 形（真机 cjs-module-lexer 同款静态识别；
    // 自定义 `__es(...)` 包装真机也不识别——link 期 x 缺失同款失败）。
    dir.child("star.cjs")
        .write_str("function __exportStar(r, e) { for (const k in r) { if (k !== 'default') e[k] = r[k]; } }\n__exportStar(require('./star-a.cjs'), exports);\nexports.y = 20;\n")
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

#[test]
fn phase9m_require_exports_conditions() {
    // require 条件族（真机 26.8.2 对拍）：require 走 require 条件（双条件包
    // 命中 CJS 入口，非 ESM namespace）；imports-only 包 require 即解析失败
    // （真机同款 ERR_PACKAGE_PATH_NOT_EXPORTED，无 import 回落）。
    let dir = assert_fs::TempDir::new().unwrap();
    let mixed = dir.child("node_modules/mixed");
    mixed.create_dir_all().unwrap();
    mixed
        .child("package.json")
        .write_str(r#"{"name":"mixed","version":"1.0.0","exports":{".":{"import":"./esm.mjs","require":"./cjs.cjs"}}}"#)
        .unwrap();
    mixed
        .child("esm.mjs")
        .write_str("export const side = \"esm\";\nexport default {};\n")
        .unwrap();
    mixed
        .child("cjs.cjs")
        .write_str("module.exports = { side: \"cjs\" };\n")
        .unwrap();
    let only = dir.child("node_modules/onlyesm");
    only.create_dir_all().unwrap();
    only
        .child("package.json")
        .write_str(r#"{"name":"onlyesm","version":"1.0.0","exports":{".":{"import":"./esm.mjs"}}}"#)
        .unwrap();
    only.child("esm.mjs").write_str("export default {};\n").unwrap();
    let main = dir.child("main.cjs");
    main.write_str(
        r#"const mixed = require("mixed");
console.log("cond", mixed.side);
try { require("onlyesm"); console.log("NO-ERR"); }
catch (e) { console.log("onlyerr", String(e.message).includes("Cannot find module 'onlyesm'")); }
"#,
    )
    .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(main.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    for line in ["cond cjs", "onlyerr true"] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase9m_require_resolve_caller_relative() {
    // require.resolve 相对说明符按**调用方文件**定 base：直挂原生后
    // describe_scripted_caller 的最内层帧 = 调用方（prelude 闭包帧不再盖住；
    // jsdom api.js 实测同款）。
    let dir = assert_fs::TempDir::new().unwrap();
    let a = dir.child("a");
    a.create_dir_all().unwrap();
    a.child("two.cjs").write_str("module.exports = 1;\n").unwrap();
    a.child("one.cjs")
        .write_str("console.log(\"res\", require.resolve(\"./two.cjs\").endsWith(\"two.cjs\"));\n")
        .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(a.child("one.cjs").path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "res true\n");
    dir.close().unwrap();
}

#[test]
fn phase10f_require_cjs_entry_relative() {
    // 10f：CJS 入口（typeless .js）相对 require 以入口文件为 base
    //（caller_base 裸路径回落；修前 "eval has no file base URL"）。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("b.js").write_str("module.exports = 42;\n").unwrap();
    dir.child("a.js")
        .write_str("const x = require(\"./b.js\");\nconsole.log(\"entry\", x);\n")
        .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(dir.child("a.js").path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "entry 42\n");
    dir.close().unwrap();
}

#[test]
fn phase10f_cjs_top_level_return_entry_and_dep() {
    // CJS 函数包装语义（node 口径）：顶层 return 合法——入口 typeless .js
    // 与 require 依赖双形。export 文件不被误判 CJS（cjs_goal_probe 的模块
    // 信号拒绝；phase9k require(esm) 同源）。
    let dir = assert_fs::TempDir::new().unwrap();
    let entry = dir.child("early.js");
    entry.write_str("if (1 === 1) {\n  return;\n}\nconsole.log(\"unreachable\");\n").unwrap();
    let out = winterjs().arg("--run").arg(entry.path()).output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout, "", "early return 应静默退出: {stdout}");

    let main = dir.child("dep-main.cjs");
    main.write_str(
        "const r = require(\"./retdep.js\");\nconsole.log(\"dep-ret\", r === \"early\");\nconst esm = require(\"./esmdep.js\");\nconsole.log(\"esm-ok\", esm.default === 1);\n",
    )
    .unwrap();
    // node：wrapper 的 return 值被丢弃，exports 面由 module.exports 决定——
    // return 用于提前终止，exports 先挂再 return。
    dir.child("retdep.js")
        .write_str("module.exports = \"early\";\nif (true) return;\nmodule.exports = \"late\";\n")
        .unwrap();
    dir.child("esmdep.js")
        .write_str("export default 1;\n")
        .unwrap();
    let out2 = winterjs().arg("--run").arg(main.path()).output().unwrap();
    assert!(
        out2.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out2.stderr)
    );
    let stdout2 = String::from_utf8(out2.stdout).unwrap();
    assert_eq!(
        stdout2,
        "dep-ret true\nesm-ok true\n",
        "return 提前终止 + require(esm) 成功: {stdout2}"
    );
    dir.close().unwrap();
}

#[test]
fn phase11_require_rethrows_original_exception() {
    // 2026-09-25：require 透传用户代码原异常（身份/类/code/stack，node 同），
    // 入口报错取真实抛点行号（CJS 包装头编在第 0 行，行号 = 物理行），
    // NodeError（super() 后 defineProperty message）文案不再为空。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("dep.js")
        .write_str("class E extends Error {}\nmodule.exports = E;\nthrow Object.assign(new E(\"orig\"), { code: \"XC\" });\n")
        .unwrap();
    dir.child("main.js")
        .write_str(
            "let first;\n\
             try { require(\"./dep.js\"); } catch (e) { first = e; console.log(\"cls\", e.constructor.name, e.code, e.message, /dep\\.js:3:/.test(e.stack)); }\n\
             try { require(\"./dep.js\"); } catch (e) { console.log(\"again\", e !== first, e.code); }\n\
             try { require(\"./nope-missing.js\"); } catch (e) { console.log(\"missing\", /Cannot find module/.test(e.message)); }\n\
             console.log(\"line\", new Error().stack.split(\"\\n\")[0].includes(\"main.js:5:\"));\n",
        )
        .unwrap();
    // 正常：原对象透传 + 行号物理对齐；二次 require 重新求值（失败清场，非缓存半成品）。
    let (ok, out, err) = wjs(&["--run", "main.js"], &dir);
    assert!(ok, "stderr: {err}");
    for line in ["cls E XC orig true", "again true XC", "missing true", "line true"] {
        assert!(out.lines().any(|l| l == line), "missing {line}\nout: {out}");
    }
    // 报错：入口未捕获——位置是真实抛点（第 3 行），非 prelude 行号。
    dir.child("boom.js").write_str("\n\nthrow new TypeError(\"deep\");\n").unwrap();
    let (ok, _, err) = wjs(&["--run", "boom.js"], &dir);
    assert!(!ok);
    assert!(err.contains("boom.js:3:7: deep"), "stderr: {err}");
    // 边界：NodeError（message 为属性而非引擎槽）文案非空、位置如实报内部文件。
    dir.child("ne.js")
        .write_str("require(\"stream\").pipeline(process.stdin, {}, () => {});\n")
        .unwrap();
    let (ok, _, err) = wjs(&["--run", "ne.js"], &dir);
    assert!(!ok);
    assert!(err.contains("argument must be"), "stderr: {err}");
}
