//! loader/模块系统黑盒测试(对齐 src/loader/ + src/modules.rs:resolve/转译/循环/http 导入/入口类型)。

mod common;

use common::*;

use assert_fs::prelude::*;

#[test]
fn phase2_relative_import() {
    let (_dir, entry) = mod_dir(
        &[
            ("lib.js", "export const x = 40 + 2;\n"),
            (
                "app.js",
                "import { x } from \"./lib.js\";\nconsole.log(x);\n",
            ),
        ],
        "app.js",
    );
    assert_eq!(stdout_of(&mut winterjs2().arg("--run").arg(&entry)), "42\n");
}

#[test]
fn phase2_typescript_transpile() {
    let (_dir, entry) = mod_dir(
        &[
            (
                "math.ts",
                "export function add(a: number, b: number): number { return a + b; }\n",
            ),
            (
                "app.ts",
                "import { add } from \"./math\";\nconsole.log(add(40, 2));\n",
            ),
        ],
        "app.ts",
    );
    assert_eq!(stdout_of(&mut winterjs2().arg("--run").arg(&entry)), "42\n");
}

#[test]
fn phase2_circular_import_no_deadlock() {
    let (_dir, entry) = mod_dir(
        &[
            ("a.js", "import \"./b.js\";\nconsole.log(\"a\");\n"),
            ("b.js", "import \"./a.js\";\nconsole.log(\"b\");\n"),
        ],
        "a.js",
    );
    // spec 求值序：b 先于 a，不死锁
    assert_eq!(
        stdout_of(&mut winterjs2().arg("--run").arg(&entry)),
        "b\na\n"
    );
}

#[test]
fn phase2_import_meta_url() {
    let (_dir, entry) = mod_dir(&[("meta.js", "console.log(import.meta.url);\n")], "meta.js");
    let out = stdout_of(&mut winterjs2().arg("--run").arg(&entry));
    assert!(
        out.starts_with("file://") && out.trim_end().ends_with("/meta.js"),
        "meta url: {out}"
    );
}

#[test]
fn phase2_bare_specifier_missing_friendly_error() {
    let (_dir, entry) = mod_dir(
        &[("bare.js", "import \"left-pad-xyz-absent\";\n")],
        "bare.js",
    );
    let out = winterjs2().arg("--run").arg(&entry).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("cannot resolve 'left-pad-xyz-absent'"),
        "stderr: {stderr}"
    );
}

#[test]
fn phase2_bare_specifier_node_modules() {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("node_modules/left-pad/package.json")
        .write_str("{\"name\":\"left-pad\",\"version\":\"1.0.0\",\"main\":\"index.js\"}")
        .unwrap();
    dir.child("node_modules/left-pad/index.js")
        .write_str("export default \"pad!\";\n")
        .unwrap();
    dir.child("nm.js")
        .write_str("import pad from \"left-pad\";\nconsole.log(pad);\n")
        .unwrap();
    let entry = dir.child("nm.js").path().to_path_buf();
    assert_eq!(
        stdout_of(&mut winterjs2().arg("--run").arg(&entry)),
        "pad!\n"
    );
    dir.close().unwrap();
}

#[test]
fn phase2_tsconfig_paths_alias() {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("tsconfig.json")
        .write_str("{\"compilerOptions\":{\"baseUrl\":\".\",\"paths\":{\"@lib/*\":[\"src/*\"]}}}")
        .unwrap();
    dir.child("src/add.ts")
        .write_str("export const add = (a: number, b: number): number => a + b;\n")
        .unwrap();
    dir.child("app.ts")
        .write_str("import { add } from \"@lib/add\";\nconsole.log(add(1, 2));\n")
        .unwrap();
    let entry = dir.child("app.ts").path().to_path_buf();
    assert_eq!(stdout_of(&mut winterjs2().arg("--run").arg(&entry)), "3\n");
    dir.close().unwrap();
}

#[test]
fn phase2_ts_js_extension_alias() {
    // TS 约定：`./foo.js` 指向 `./foo.ts` 源码
    let (_dir, entry) = mod_dir(
        &[
            ("foo.ts", "export const v: number = 7;\n"),
            (
                "app.ts",
                "import { v } from \"./foo.js\";\nconsole.log(v);\n",
            ),
        ],
        "app.ts",
    );
    assert_eq!(stdout_of(&mut winterjs2().arg("--run").arg(&entry)), "7\n");
}

#[test]
fn phase2_dynamic_import() {
    let (_dir, entry) = mod_dir(
        &[
            ("lib.js", "export const x = 40 + 2;\n"),
            (
                "dyn.js",
                "const m = await import(\"./lib.js\");\nconsole.log(m.x);\n",
            ),
        ],
        "dyn.js",
    );
    assert_eq!(stdout_of(&mut winterjs2().arg("--run").arg(&entry)), "42\n");
}

#[test]
fn phase2_top_level_await_entry() {
    let (_dir, entry) = mod_dir(
        &[(
            "tla.js",
            "await new Promise(r=>setTimeout(()=>r(7),5)).then(v=>console.log(\"tla\",v));\n",
        )],
        "tla.js",
    );
    assert_eq!(
        stdout_of(&mut winterjs2().arg("--run").arg(&entry)),
        "tla 7\n"
    );
}

#[test]
fn phase2_data_url_import() {
    let (_dir, entry) = mod_dir(
        &[(
            "data.js",
            "import x from \"data:text/javascript,export default 99\";\nconsole.log(x);\n",
        )],
        "data.js",
    );
    assert_eq!(stdout_of(&mut winterjs2().arg("--run").arg(&entry)), "99\n");
}

#[test]
fn phase2_ts_runtime_error_location() {
    // TS 报错行号经 sourcemap 回映射到原文（转译行会漂移，断言原文行）
    let (_dir, entry) = mod_dir(
        &[(
            "e.ts",
            "interface Big {\n  a: string;\n}\nconst o: Big = { a: \"x\" };\nconsole.log(o.a);\nboom_ts();\n",
        )],
        "e.ts",
    );
    let out = winterjs2().arg("--run").arg(&entry).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("e.ts:6:1"), "stderr: {stderr}");
    assert!(
        stderr.contains("boom_ts is not defined"),
        "stderr: {stderr}"
    );
}

// ── Phase 3a：URL / 编码 / crypto ──────────────────────────────────────────

#[test]
fn loader_http_import_end_to_end() {
    // 正常：绝对 http 导入 + 远端相对导入（URL join）；TF：同 URL 去重（模块单例）。
    let port = serve_http(3, move |head, _body| {
        let line = head.lines().next().unwrap_or("").to_owned();
        let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
        let body = if path == "/main.mjs" {
            "import { answer } from \"./lib.mjs\";\nexport const double = answer * 2;\n"
        } else if path == "/lib.mjs" {
            "export const answer = 42;\n"
        } else {
            return (404, vec![], b"nope".to_vec());
        };
        (
            200,
            vec![("content-type", "text/javascript".into())],
            body.as_bytes().to_vec(),
        )
    });
    let code = format!(
        "const m = await import(\"http://127.0.0.1:{port}/main.mjs\"); \
         const m2 = await import(\"http://127.0.0.1:{port}/main.mjs\"); \
         console.log(m.double, m === m2);"
    );
    assert_eq!(
        stdout_of(&mut winterjs2().args(["--eval", &code])),
        "84 true\n"
    );
}

#[test]
fn loader_http_errors() {
    // 报错：404 可读错（exit=1）；边界：超大/非 UTF-8 由单元口径覆盖，此处只钉 404。
    let port = serve_http(1, move |_head, _body| (404, vec![], b"nope".to_vec()));
    let code = format!("await import(\"http://127.0.0.1:{port}/missing.mjs\")");
    let out = winterjs2().args(["--eval", &code]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("404"), "stderr:\n{err}");
}

#[test]
fn run_entry_respects_package_json_type() {
    // 入口与 require() 同口径：`type: module` 包的 extensionless bin 走模块；
    // 无 type 即经典（Node 口径）；`type: commonjs` 的 ESM 语法自然报 SyntaxError。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("package.json")
        .write_str(r#"{"type":"module"}"#)
        .unwrap();
    dir.child("lib.mjs")
        .write_str("export const x = 1;\n")
        .unwrap();
    dir.child("bin-noext")
        .write_str("import \"./lib.mjs\";\nconsole.log(\"esm-entry-ok\");\n")
        .unwrap();
    let out = winterjs2()
        .arg("--run")
        .arg(dir.path().join("bin-noext"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "esm-entry-ok\n");
    // 无 type：同内容走经典，import 即 SyntaxError（exit=1，可读）。
    let dir2 = assert_fs::TempDir::new().unwrap();
    dir2.child("bin-noext")
        .write_str("import \"./lib.mjs\";\n")
        .unwrap();
    let out = winterjs2()
        .arg("--run")
        .arg(dir2.path().join("bin-noext"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    dir.close().unwrap();
    dir2.close().unwrap();
}

/// 搭一个临时模块目录：files 为 (name, content)，返回 dir（调用方持有）+ 入口路径。
fn mod_dir(files: &[(&str, &str)], entry: &str) -> (assert_fs::TempDir, std::path::PathBuf) {
    let dir = assert_fs::TempDir::new().unwrap();
    for (name, content) in files {
        dir.child(name).write_str(content).unwrap();
    }
    let path = dir.child(entry).path().to_path_buf();
    (dir, path)
}
