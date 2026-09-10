//! CLI 黑盒测试（docs/plan.md Phase 0「CLI 黑盒测试」）。
//! 注意：assert_cmd 跑子进程、stderr 非 TTY → 错误走稳定 plain 格式。

use assert_cmd::Command;
use assert_fs::prelude::*;
use rstest::rstest;

fn winterjs() -> Command {
    Command::cargo_bin("winterjs").expect("binary builds")
}

fn stdout_of(cmd: &mut Command) -> String {
    let out = cmd.output().expect("binary runs");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf8 stdout")
}

#[rstest]
#[case("40 + 2", "42\n")]
#[case("'a' + 'b'", "ab\n")]
#[case("Math.max(3, 9)", "9\n")]
#[case("undefined", "")]
fn eval_completion_value(#[case] code: &str, #[case] expected: &str) {
    assert_eq!(stdout_of(&mut winterjs().args(["eval", code])), expected);
}

#[test]
fn eval_uncaught_exception_exit_1_with_plain_format() {
    // AGENTS.md §3 验收格式：Error: eval.js:1:7: boom，exit=1（非 TTY）
    let out = winterjs()
        .args(["eval", "throw new Error(\"boom\")"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Error: eval.js:1:7: boom"), "stderr: {stderr}");
}

#[test]
fn run_missing_file_reports_chain() {
    let out = winterjs().args(["run", "/nope/such.js"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Error: failed to read /nope/such.js"), "stderr: {stderr}");
    assert!(stderr.contains("Caused by:"), "stderr: {stderr}");
}

#[test]
fn run_file_from_tempdir() {
    let dir = assert_fs::TempDir::new().unwrap();
    let script = dir.child("app.js");
    script.write_str("1 + 41").unwrap();

    assert_eq!(
        stdout_of(&mut winterjs().arg("run").arg(script.path())),
        "42\n"
    );
    dir.close().unwrap();
}

#[test]
fn run_script_in_tempdir_workdir() {
    // tempfile 直用：cwd 下的相对脚本
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("rel.js");
    std::fs::write(&path, "'ok'").unwrap();

    assert_eq!(
        stdout_of(&mut winterjs().arg("run").arg("rel.js").current_dir(tmp.path())),
        "ok\n"
    );
}

#[test]
fn version_contains_pkg_version() {
    let out = stdout_of(&mut winterjs().arg("--version"));
    assert!(out.contains(env!("CARGO_PKG_VERSION")), "version: {out}");
    // vergen gitcl 元数据也应嵌进来（git 仓库内构建时）
    assert!(out.contains("built "), "version: {out}");
}

#[test]
fn config_outputs_resolved_settings_json() {
    let out = stdout_of(&mut winterjs().args(["config"]));
    let value: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert!(value["log"].is_object(), "log section present: {out}");

    // 环境变量覆盖（WINTERJS_LOG__COLOR）优先于缺省
    let out = stdout_of(&mut winterjs().env("WINTERJS_LOG__COLOR", "always").args(["config"]));
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value["log"]["color"], "always");
}

#[test]
fn config_schema_is_valid_json_schema() {
    let out = stdout_of(&mut winterjs().args(["config", "--schema"]));
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        value["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
}

#[test]
fn winterjs_log_filter_does_not_break_config() {
    // AGENTS §4.10 回归：WINTERJS_LOG=<EnvFilter> 是日志直读变量，不得被 config 误收
    let out = winterjs()
        .env("WINTERJS_LOG", "winterjs=debug")
        .args(["config"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(value["log"].is_object(), "log section present: {stdout}");
}

#[test]
fn completions_bash_script() {
    let out = stdout_of(&mut winterjs().args(["completions", "bash"]));
    assert!(out.starts_with("_winterjs()"), "completions: {out}");
}

#[test]
fn man_pages_render_roff() {
    let out = stdout_of(&mut winterjs().arg("man"));
    assert_eq!(out.matches(".TH").count(), 6, "main + 5 subcommand pages");
}

#[test]
fn settings_toml_is_loaded() {
    // config crate 从 cwd 读 winterjs.toml
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("winterjs.toml"),
        "[log]\ncolor = \"never\"\n",
    )
    .unwrap();
    let out = stdout_of(&mut winterjs().arg("config").current_dir(tmp.path()));
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    // pretty_assertions：比对失败时输出可读 diff
    pretty_assertions::assert_eq!(
        value["log"]["color"].as_str().unwrap(),
        "never",
        "settings file should override default"
    );
}

#[test]
fn settings_yaml_is_loaded() {
    // config 的 yaml 特性（yaml-rust2 链，2026-09-10 纯度审计后启用）
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("winterjs.yaml"),
        "log:\n  color: \"never\"\n",
    )
    .unwrap();
    let out = stdout_of(&mut winterjs().arg("config").current_dir(tmp.path()));
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    pretty_assertions::assert_eq!(
        value["log"]["color"].as_str().unwrap(),
        "never",
        "yaml settings file should override default"
    );
}

// ── Phase 1：console / timers / microtask / structuredClone ─────────────

#[test]
fn phase1_microtask_order_before_timer() {
    // 规范顺序：同步 → 微任务（FIFO）→ 宏任务
    let out = stdout_of(&mut winterjs().args(["eval",
        "console.log('1'); setTimeout(()=>console.log('4'),0); Promise.resolve().then(()=>console.log('3')); queueMicrotask(()=>console.log('2'))"]));
    assert_eq!(out, "1\n3\n2\n4\n", "microtask/timer ordering: {out}");
}

#[test]
fn phase1_promise_chain_three_hops() {
    let out = stdout_of(&mut winterjs().args(["eval",
        "Promise.resolve(1).then(v=>v+1).then(v=>v+1).then(v=>console.log('chain:',v))"]));
    assert!(out.contains("chain: 3"), "chain: {out}");
}

#[test]
fn phase1_top_level_await_acceptance() {
    // docs/plan.md Phase 1 验收样例
    assert_eq!(
        stdout_of(&mut winterjs().args(["eval",
            "await new Promise(r=>setTimeout(()=>r(1),10))"])),
        "1\n"
    );
}

#[test]
fn phase1_interval_until_cleared() {
    let out = stdout_of(&mut winterjs().args(["eval",
        "let n=0; const id=setInterval(()=>{n++; console.log('tick',n); if(n>=3) clearInterval(id)},5)"]));
    assert_eq!(out, "tick 1\ntick 2\ntick 3\n", "interval: {out}");
}

#[test]
fn phase1_nested_microtasks() {
    let out = stdout_of(&mut winterjs().args(["eval",
        "async function f(){ for(let i=0;i<3;i++){ await Promise.resolve(); console.log('micro',i);} } f()"]));
    assert_eq!(out, "micro 0\nmicro 1\nmicro 2\n[object Promise]\n", "nested: {out}");
}

#[test]
fn phase1_structured_clone_json_values() {
    let out = stdout_of(&mut winterjs().args(["eval",
        "const a={x:1,y:[1,2,{z:'s'}]}; const b=structuredClone(a); console.log(JSON.stringify(b), b===a)"]));
    assert_eq!(
        out,
        "{\"x\":1,\"y\":[1,2,{\"z\":\"s\"}]} false\n",
        "clone object: {out}"
    );
    let out = stdout_of(&mut winterjs().args(["eval",
        "console.log(JSON.stringify([structuredClone(42), structuredClone('s'), structuredClone(null)]))"]));
    assert_eq!(out, "[42,\"s\",null]\n");
}

#[test]
fn phase1_unhandled_rejection_is_fatal() {
    let out = winterjs().args(["eval", "Promise.reject(new Error('nope'))"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1), "unhandled rejection must be fatal");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unhandled rejection"), "stderr: {stderr}");
}

#[test]
fn phase1_console_count_and_time() {
    let out = stdout_of(&mut winterjs().args(["eval",
        "console.count('a'); console.count('a'); console.time('t'); console.timeLog('t'); console.timeEnd('t')"]));
    assert!(out.contains("a: 1") && out.contains("a: 2"), "count: {out}");
    assert!(out.contains("t: ") && out.matches("t: ").count() == 2, "time: {out}");
}

// ── Phase 2 切片 a：ESM loader ────────────────────────────────────────────

/// 搭一个临时模块目录：files 为 (name, content)，返回 dir（调用方持有）+ 入口路径。
fn mod_dir(files: &[(&str, &str)], entry: &str) -> (assert_fs::TempDir, std::path::PathBuf) {
    let dir = assert_fs::TempDir::new().unwrap();
    for (name, content) in files {
        dir.child(name).write_str(content).unwrap();
    }
    let path = dir.child(entry).path().to_path_buf();
    (dir, path)
}

#[test]
fn phase2_relative_import() {
    let (_dir, entry) = mod_dir(
        &[
            ("lib.js", "export const x = 40 + 2;\n"),
            ("app.js", "import { x } from \"./lib.js\";\nconsole.log(x);\n"),
        ],
        "app.js",
    );
    assert_eq!(stdout_of(&mut winterjs().arg("run").arg(&entry)), "42\n");
}

#[test]
fn phase2_typescript_transpile() {
    let (_dir, entry) = mod_dir(
        &[
            ("math.ts", "export function add(a: number, b: number): number { return a + b; }\n"),
            ("app.ts", "import { add } from \"./math\";\nconsole.log(add(40, 2));\n"),
        ],
        "app.ts",
    );
    assert_eq!(stdout_of(&mut winterjs().arg("run").arg(&entry)), "42\n");
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
    assert_eq!(stdout_of(&mut winterjs().arg("run").arg(&entry)), "b\na\n");
}

#[test]
fn phase2_import_meta_url() {
    let (_dir, entry) = mod_dir(&[("meta.js", "console.log(import.meta.url);\n")], "meta.js");
    let out = stdout_of(&mut winterjs().arg("run").arg(&entry));
    assert!(out.starts_with("file://") && out.trim_end().ends_with("/meta.js"), "meta url: {out}");
}

#[test]
fn phase2_bare_specifier_missing_friendly_error() {
    let (_dir, entry) = mod_dir(&[("bare.js", "import \"left-pad-xyz-absent\";\n")], "bare.js");
    let out = winterjs().arg("run").arg(&entry).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("cannot resolve 'left-pad-xyz-absent'"), "stderr: {stderr}");
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
    assert_eq!(stdout_of(&mut winterjs().arg("run").arg(&entry)), "pad!\n");
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
    assert_eq!(stdout_of(&mut winterjs().arg("run").arg(&entry)), "3\n");
    dir.close().unwrap();
}

#[test]
fn phase2_ts_js_extension_alias() {
    // TS 约定：`./foo.js` 指向 `./foo.ts` 源码
    let (_dir, entry) = mod_dir(
        &[
            ("foo.ts", "export const v: number = 7;\n"),
            ("app.ts", "import { v } from \"./foo.js\";\nconsole.log(v);\n"),
        ],
        "app.ts",
    );
    assert_eq!(stdout_of(&mut winterjs().arg("run").arg(&entry)), "7\n");
}

#[test]
fn phase2_dynamic_import() {
    let (_dir, entry) = mod_dir(
        &[
            ("lib.js", "export const x = 40 + 2;\n"),
            ("dyn.js", "const m = await import(\"./lib.js\");\nconsole.log(m.x);\n"),
        ],
        "dyn.js",
    );
    assert_eq!(stdout_of(&mut winterjs().arg("run").arg(&entry)), "42\n");
}

#[test]
fn phase2_top_level_await_entry() {
    let (_dir, entry) = mod_dir(
        &[("tla.js", "await new Promise(r=>setTimeout(()=>r(7),5)).then(v=>console.log(\"tla\",v));\n")],
        "tla.js",
    );
    assert_eq!(stdout_of(&mut winterjs().arg("run").arg(&entry)), "tla 7\n");
}

#[test]
fn phase2_data_url_import() {
    let (_dir, entry) = mod_dir(
        &[("data.js", "import x from \"data:text/javascript,export default 99\";\nconsole.log(x);\n")],
        "data.js",
    );
    assert_eq!(stdout_of(&mut winterjs().arg("run").arg(&entry)), "99\n");
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
    let out = winterjs().arg("run").arg(&entry).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("e.ts:6:1"), "stderr: {stderr}");
    assert!(stderr.contains("boom_ts is not defined"), "stderr: {stderr}");
}

// ── Phase 3a：URL / 编码 / crypto ──────────────────────────────────────────

#[test]
fn phase3_url_components() {
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"const u = new URL("https://user:pass@example.com:8080/p?q=1#h"); console.log([u.href, u.protocol, u.host, u.hostname, u.port, u.pathname, u.search, u.hash, u.origin].join("|"))"#]));
    assert_eq!(
        out,
        "https://user:pass@example.com:8080/p?q=1#h|https:|example.com:8080|example.com|8080|/p|?q=1|#h|https://example.com:8080
",
        "url: {out}"
    );
}

#[test]
fn phase3_url_relative_and_can_parse() {
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"console.log(new URL("/p", "https://h.org/x").href, URL.canParse(':::'), URL.canParse('https://a.b'))"#]));
    assert_eq!(out, "https://h.org/p false true\n", "url base: {out}");
}

#[test]
fn phase3_url_invalid_throws() {
    let out = winterjs().args(["eval", "new URL(':::')"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Invalid URL"), "stderr: {stderr}");
}

#[test]
fn phase3_usp_live_view() {
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"const u = new URL("https://ex.com/?b=2"); const sp = u.searchParams; sp.append("c", "3"); console.log(u.search, sp === u.searchParams); u.search = "?x=9"; console.log(sp.toString())"#]));
    assert_eq!(out, "?b=2&c=3 true
x=9
", "live view: {out}");
}

#[test]
fn phase3_usp_ops() {
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"const s = new URLSearchParams("z=1&a=2&a=3"); s.sort(); console.log(s.toString(), s.get("a"), s.getAll("a").length, s.size)"#]));
    assert_eq!(out, "a=2&a=3&z=1 2 2 3
", "usp: {out}");
}

#[test]
fn phase3_text_encoder_decoder() {
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"const e = new TextEncoder(); console.log(e.encoding, e.encode("hi").length, JSON.stringify(new TextEncoder().encodeInto("hello", new Uint8Array(3)))); console.log(new TextDecoder().decode(new Uint8Array([104, 105])), new TextDecoder("utf-16le").decode(new Uint8Array([104, 0, 105, 0])));"#]));
    assert_eq!(out, "utf-8 2 {\"read\":3,\"written\":3}\nhi hi\n", "codec: {out}");
}

#[test]
fn phase3_text_decoder_fatal() {
    let out = winterjs()
        .args(["eval", "new TextDecoder('utf-8', {fatal:true}).decode(new Uint8Array([0xff]))"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let out = stdout_of(&mut winterjs().args(["eval",
        "console.log(new TextDecoder('utf-8').decode(new Uint8Array([0xff])).length)"]));
    assert_eq!(out, "1
");
}

#[test]
fn phase3_base64_roundtrip() {
    let out = stdout_of(&mut winterjs().args(["eval",
        "console.log(btoa('hello'), atob('aGVsbG8='))"]));
    assert_eq!(out, "aGVsbG8= hello
", "base64: {out}");
    let out = winterjs().args(["eval", "btoa('€')"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn phase3_crypto_random() {
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"const v = new Uint8Array(16); console.log(crypto.getRandomValues(v) === v, v.length); const a = crypto.randomUUID(), b = crypto.randomUUID(); console.log(a.length, a !== b, /^[0-9a-f-]{36}$/.test(a))"#]));
    assert_eq!(out, "true 16
36 true true
", "crypto: {out}");
    let out = winterjs()
        .args(["eval", "crypto.getRandomValues(new Uint8Array(70000))"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
}

// ── Phase 3b：fetch / Headers / Request / Response ─────────────────────────

/// 起一个只 serving N 个请求的本机 HTTP 服务器（ephemeral 端口，hermetic）。
/// handler 收完整请求头（+ POST body），回包由闭包定。
fn serve_http(n: usize, handler: impl Fn(String, Vec<u8>) -> (u16, Vec<(&'static str, String)>, Vec<u8>) + Send + 'static) -> u16 {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for _ in 0..n {
            let Ok((mut s, _)) = listener.accept() else { return };
            let mut head = Vec::new();
            let mut buf = [0u8; 1024];
            loop {
                let Ok(k) = s.read(&mut buf) else { break };
                if k == 0 { break; }
                head.extend_from_slice(&buf[..k]);
                if head.windows(4).any(|w| w == b"\r\n\r\n") { break; }
            }
            let head_end = head.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4).unwrap_or(head.len());
            let head_str = String::from_utf8_lossy(&head[..head_end]).into_owned();
            let body_len = head_str
                .lines()
                .find_map(|l| l.strip_prefix("content-length:").or_else(|| l.strip_prefix("Content-Length:")))
                .and_then(|v| v.trim().parse::<usize>().ok())
                .unwrap_or(0);
            let mut body = head[head_end..].to_vec();
            while body.len() < body_len {
                let Ok(k) = s.read(&mut buf) else { break };
                if k == 0 { break; }
                body.extend_from_slice(&buf[..k]);
            }
            body.truncate(body_len);
            let (status, headers, resp_body) = handler(head_str, body);
            let reason = if status == 200 { "OK" } else { "Error" };
            let mut resp = format!("HTTP/1.1 {status} {reason}\r\ncontent-length: {}\r\nconnection: close\r\n", resp_body.len());
            for (k, v) in headers {
                resp.push_str(&format!("{k}: {v}\r\n"));
            }
            resp.push_str("\r\n");
            let _ = s.write_all(resp.as_bytes());
            let _ = s.write_all(&resp_body);
        }
    });
    port
}

#[test]
fn phase3_fetch_http_get() {
    let port = serve_http(1, |_head, _body| {
        (200, vec![("x-echo", "yes".into())], b"hello-http".to_vec())
    });
    let code = format!(
        r#"const r = await fetch("http://127.0.0.1:{port}/p?q=1"); console.log(r.status, r.ok, r.url, await r.text(), r.headers.get("x-echo"));"#
    );
    assert_eq!(
        stdout_of(&mut winterjs().args(["eval", &code])),
        format!("200 true http://127.0.0.1:{port}/p?q=1 hello-http yes\n")
    );
}

#[test]
fn phase3_fetch_http_post_echo() {
    let port = serve_http(1, |head, body| {
        let ct = head.lines().find(|l| l.to_lowercase().starts_with("content-type:")).unwrap_or("").to_owned();
        let mut echo = b"got:".to_vec();
        echo.extend_from_slice(&body);
        (200, vec![("x-ct", ct)], echo)
    });
    let code = format!(
        r#"const r = await fetch("http://127.0.0.1:{port}/echo", {{method: "POST", body: "a=1&b=2", headers: {{"content-type": "text/plain"}}}}); console.log(r.status, await r.text(), r.headers.get("x-ct"));"#
    );
    let out = stdout_of(&mut winterjs().args(["eval", &code]));
    assert!(out.starts_with("200 got:a=1&b=2 content-type: text/plain"), "post: {out}");
}

#[test]
fn phase3_fetch_data_and_file() {
    assert_eq!(
        stdout_of(&mut winterjs().args(["eval",
            r#"const r = await fetch("data:text/plain,hello-fetch"); console.log(r.status, r.ok, await r.text());"#])),
        "200 true hello-fetch\n"
    );
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("f.txt").write_str("hello-file").unwrap();
    let url = format!("file://{}", dir.child("f.txt").path().display());
    let code = format!(r#"console.log(await (await fetch("{url}")).text());"#);
    assert_eq!(stdout_of(&mut winterjs().args(["eval", &code])), "hello-file\n");
    dir.close().unwrap();
}

#[test]
fn phase3_fetch_errors_are_rejections() {
    // 不支持的 scheme 与连不上的地址都以 rejection 呈现（catch 可接住）
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"console.log(await fetch("blob:xyz").then(() => "no", () => "blob-err"))"#]));
    assert_eq!(out, "blob-err\n", "blob: {out}");
    // 保证关闭的端口：bind 后立刻 drop
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let code = format!(
        r#"console.log(await fetch("http://127.0.0.1:{port}/").then(() => "no", (e) => String(e).includes("fetch failed") ? "net-err" : "other:" + e))"#
    );
    assert_eq!(stdout_of(&mut winterjs().args(["eval", &code])), "net-err\n");
}

#[test]
fn phase3_headers_request_response_classes() {
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"const h = new Headers([["X-A", "1"], ["x-a", "2"]]); console.log(h.get("x-a"), [...h.keys()].join(",")); const r = new Response("hi", { status: 201 }); console.log(r.status, r.ok, await r.text()); const q = new Request("https://ex.com/a", { method: "post", body: "x" }); console.log(q.method, q.url, await q.text());"#]));
    assert_eq!(out, "1, 2 x-a,x-a
201 true hi
POST https://ex.com/a x
", "classes: {out}");
}

#[test]
fn phase3_abort_signal_pre_abort() {
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"const c = new AbortController(); c.abort(); console.log(await fetch("http://127.0.0.1:9/x", { signal: c.signal }).then(() => 'no', () => 'abort-ok'));"#]));
    assert_eq!(out, "abort-ok
", "abort: {out}");
}

#[test]
fn phase3_subtle_digest_vectors() {
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"const hex = async (a, d) => [...new Uint8Array(await crypto.subtle.digest(a, new TextEncoder().encode(d)))].map((b) => b.toString(16).padStart(2, "0")).join(""); console.log(await hex("SHA-256", "abc")); console.log(await hex("SHA-1", "abc"));"#]));
    assert_eq!(
        out,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\na9993e364706816aba3e25717850c26c9cd0d89d\n",
        "digest: {out}"
    );
}

#[test]
fn phase3_subtle_digest_unsupported() {
    let out = winterjs()
        .args(["eval", r#"await crypto.subtle.digest("MD5", new Uint8Array(1))"#])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("NotSupportedError"), "stderr: {stderr}");
}

#[test]
fn phase3_streams_basic() {
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"const rs = new ReadableStream({ start(c) { c.enqueue("a"); c.enqueue("b"); c.close(); } }); const out = []; for await (const x of rs) out.push(x); console.log(out.join(",")); const t = new TransformStream({ transform(c, ctl) { ctl.enqueue(String(c).toUpperCase()); } }); const w = t.writable.getWriter(); w.write("hi"); w.close(); const r = t.readable.getReader(); console.log((await r.read()).value, (await r.read()).done);"#]));
    assert_eq!(out, "a,b\nHI true\n", "streams: {out}");
}

#[test]
fn phase3_streams_pipe_tee_body() {
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"const rs = new ReadableStream({ start(c) { c.enqueue("x"); c.close(); } }); const ts = new TransformStream({ transform(c, ctl) { ctl.enqueue(c + "!"); } }); const out = []; await rs.pipeThrough(ts).pipeTo(new WritableStream({ write(c) { out.push(c); } })); console.log(out.join(",")); const [a, b] = new ReadableStream({ start(c) { c.enqueue(1); c.close(); } }).tee(); console.log(await a.getReader().read().then((x) => x.value), await b.getReader().read().then((x) => x.value)); const r = new Response("stream-me"); console.log(r.body === r.body, (await r.body.getReader().read()).value.length);"#]));
    assert_eq!(out, "x!\n1 1\ntrue 9\n", "pipe: {out}");
}

#[test]
fn phase3_aes_gcm_roundtrip() {
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"const iv = new Uint8Array(12); const key = await crypto.subtle.generateKey({ name: "AES-GCM", length: 256 }, true, ["encrypt", "decrypt"]); const ct = await crypto.subtle.encrypt({ name: "AES-GCM", iv }, key, new TextEncoder().encode("secret")); const pt = await crypto.subtle.decrypt({ name: "AES-GCM", iv }, key, ct); console.log(ct.byteLength, new TextDecoder().decode(pt));"#]));
    assert_eq!(out, "22 secret\n", "aes: {out}");
}

#[test]
fn phase3_hmac_sign_verify() {
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"const hk = await crypto.subtle.importKey("raw", new TextEncoder().encode("k"), { name: "HMAC", hash: "SHA-256" }, true, ["sign", "verify"]); const sig = await crypto.subtle.sign("HMAC", hk, new TextEncoder().encode("m")); console.log(new Uint8Array(sig).length, await crypto.subtle.verify("HMAC", hk, sig, new TextEncoder().encode("m")), await crypto.subtle.verify("HMAC", hk, sig, new TextEncoder().encode("x")), (await crypto.subtle.exportKey("jwk", hk)).kty);"#]));
    assert_eq!(out, "32 true false oct\n", "hmac: {out}");
}

#[test]
fn phase3_websocket_echo_and_close() {
    // 本机回显服务器（tokio，ephemeral 端口）：文本/二进制原样返回。
    // std listener 主线程建好后移交线程——backlog 接住先到的 SYN，无需轮询等待。
    let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std_listener.set_nonblocking(true).unwrap();
    let port = std_listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let listener = tokio::net::TcpListener::from_std(std_listener).unwrap();
                let (stream, _) = listener.accept().await.unwrap();
                let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
                use futures::{SinkExt as _, StreamExt as _};
                while let Some(msg) = ws.next().await {
                    let Ok(msg) = msg else { break };
                    if msg.is_text() || msg.is_binary() {
                        if ws.send(msg).await.is_err() {
                            break;
                        }
                    } else if msg.is_close() {
                        // 回 close 帧完成握手，再排空到对端 FIN（避免 RST 竞态）
                        let _ = ws.send(tokio_tungstenite::tungstenite::Message::Close(None)).await;
                        while ws.next().await.is_some() {}
                        break;
                    }
                }
            });
    });
    let code = format!(
        r#"const log = []; const ws = new WebSocket("ws://127.0.0.1:{port}/c"); ws.onopen = () => ws.send("ping"); ws.onmessage = (e) => {{ if (typeof e.data === "string") {{ log.push(e.data); ws.send(new Uint8Array([7, 8])); }} else {{ log.push("bin:" + new Uint8Array(e.data).join(",")); ws.close(1000, "bye"); }} }}; ws.onclose = (e) => console.log(log.join("|") + "|close:" + e.code + ":" + e.wasClean); undefined;"#
    );
    assert_eq!(
        stdout_of(&mut winterjs().args(["eval", &code])),
        "ping|bin:7,8|close:1000:true\n"
    );
}

#[test]
fn phase3_websocket_bad_url_and_send_while_connecting() {
    // 非 ws scheme 直接抛；CONNECTING 时 send 抛（连不上的端口测 readyState 报错面）
    let out = winterjs().args(["eval", r#"new WebSocket("http://x/")"#]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let code = format!(
        r#"const ws = new WebSocket("ws://127.0.0.1:{port}/"); try {{ ws.send("early"); console.log("no-throw"); }} catch (e) {{ console.log("send-while-connecting-throws"); }}"#
    );
    assert_eq!(
        stdout_of(&mut winterjs().args(["eval", &code])),
        "send-while-connecting-throws\n"
    );
}
