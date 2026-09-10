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
    assert_eq!(out.matches(".TH").count(), 7, "main + 6 subcommand pages");
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

#[test]
fn phase3_rsa_pkcs1v15_sign_verify() {
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"const {publicKey, privateKey} = await crypto.subtle.generateKey({ name: "RSASSA-PKCS1-v1_5", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" }, true, ["sign", "verify"]); console.log(publicKey.type, privateKey.type, privateKey.algorithm.modulusLength); const sig = await crypto.subtle.sign("RSASSA-PKCS1-v1_5", privateKey, new TextEncoder().encode("m")); console.log(new Uint8Array(sig).length, await crypto.subtle.verify("RSASSA-PKCS1-v1_5", publicKey, sig, new TextEncoder().encode("m")), await crypto.subtle.verify("RSASSA-PKCS1-v1_5", publicKey, sig, new TextEncoder().encode("x")));"#]));
    assert_eq!(out, "public private 2048\n256 true false\n", "rsa-pkcs1v15: {out}");
}

#[test]
fn phase3_rsa_oaep_roundtrip() {
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"const {publicKey, privateKey} = await crypto.subtle.generateKey({ name: "RSA-OAEP", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" }, true, ["encrypt", "decrypt"]); const ct = await crypto.subtle.encrypt({ name: "RSA-OAEP" }, publicKey, new TextEncoder().encode("secret")); const pt = await crypto.subtle.decrypt({ name: "RSA-OAEP" }, privateKey, ct); console.log(ct.byteLength, new TextDecoder().decode(pt)); const spki = await crypto.subtle.exportKey("spki", publicKey); console.log(new Uint8Array(spki).length);"#]));
    assert_eq!(out, "256 secret\n294\n", "rsa-oaep: {out}");
}

#[test]
fn phase3_rsa_jwk_roundtrip() {
    // 私钥 JWK 来回（n/e/d 进，p/q 恢复）+ 公钥 JWK 进；签名跨导入验证。
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"const {publicKey, privateKey} = await crypto.subtle.generateKey({ name: "RSASSA-PKCS1-v1_5", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" }, true, ["sign", "verify"]); const jwk = await crypto.subtle.exportKey("jwk", privateKey); console.log(jwk.kty, typeof jwk.dp, typeof jwk.qi); const priv2 = await crypto.subtle.importKey("jwk", jwk, { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" }, true, ["sign"]); console.log(priv2.type); const pubJwk = await crypto.subtle.exportKey("jwk", publicKey); const pub2 = await crypto.subtle.importKey("jwk", pubJwk, { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" }, true, ["verify"]); const sig = await crypto.subtle.sign("RSASSA-PKCS1-v1_5", priv2, new TextEncoder().encode("m")); console.log(await crypto.subtle.verify("RSASSA-PKCS1-v1_5", pub2, sig, new TextEncoder().encode("m")));"#]));
    assert_eq!(out, "RSA string string\nprivate\ntrue\n", "rsa-jwk: {out}");
}

#[test]
fn phase3_ecdsa_p256_roundtrip() {
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"const {publicKey, privateKey} = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, ["sign", "verify"]); console.log(publicKey.type, privateKey.algorithm.namedCurve); const sig = await crypto.subtle.sign({ name: "ECDSA", hash: "SHA-256" }, privateKey, new TextEncoder().encode("hello")); console.log(new Uint8Array(sig).length, await crypto.subtle.verify({ name: "ECDSA", hash: "SHA-256" }, publicKey, sig, new TextEncoder().encode("hello")), await crypto.subtle.verify({ name: "ECDSA", hash: "SHA-256" }, publicKey, sig, new TextEncoder().encode("bye")));"#]));
    assert_eq!(out, "public P-256\n64 true false\n", "ecdsa: {out}");
}

#[test]
fn phase3_ecdh_derive_and_key() {
    // 共享秘密对称 + deriveKey 出 AES-GCM 可加解密；P-384 JWK/spki/raw 来回。
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"const a = await crypto.subtle.generateKey({ name: "ECDH", namedCurve: "P-256" }, true, ["deriveBits", "deriveKey"]); const b = await crypto.subtle.generateKey({ name: "ECDH", namedCurve: "P-256" }, true, ["deriveBits"]); const s1 = new Uint8Array(await crypto.subtle.deriveBits({ name: "ECDH", public: b.publicKey }, a.privateKey, 256)); const s2 = new Uint8Array(await crypto.subtle.deriveBits({ name: "ECDH", public: a.publicKey }, b.privateKey, 256)); console.log(s1.length, s1.join(",") === s2.join(",")); const dk = await crypto.subtle.deriveKey({ name: "ECDH", public: b.publicKey }, a.privateKey, { name: "AES-GCM", length: 128 }, false, ["encrypt"]); console.log(dk.type, dk.algorithm.length, dk.extractable); const kp = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-384" }, true, ["sign", "verify"]); const raw = new Uint8Array(await crypto.subtle.exportKey("raw", kp.publicKey)); console.log(raw.length, raw[0]); const imp = await crypto.subtle.importKey("spki", await crypto.subtle.exportKey("spki", kp.publicKey), { name: "ECDSA", namedCurve: "P-384" }, true, ["verify"]); console.log(imp.type);"#]));
    assert_eq!(out, "32 true\nsecret 128 false\n97 4\npublic\n", "ecdh: {out}");
}

#[test]
fn phase3_asymmetric_errors() {
    // RSA-PSS/Ed25519 明确顺延；坏曲线/错用途/非私钥 derive 进报错面。
    let out = winterjs()
        .args(["eval", r#"await crypto.subtle.generateKey({ name: "RSA-PSS", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" }, true, ["sign"])"#])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("NotSupportedError"), "stderr: {stderr}");
    let out = winterjs()
        .args(["eval", r#"await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-192" }, true, ["sign"])"#])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("NotSupportedError"), "stderr: {stderr}");
    let ok = stdout_of(&mut winterjs().args(["eval",
        r#"const k = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, ["sign", "verify"]); try { await crypto.subtle.sign("ECDSA", k.publicKey, new Uint8Array(1)); console.log("no-throw"); } catch (e) { console.log(String(e).includes("private key") ? "sign-needs-private" : e); }"#]));
    assert_eq!(ok, "sign-needs-private\n", "usage: {ok}");
}

#[test]
fn phase3_websocket_wss_self_signed() {
    // rcgen 自签 127.0.0.1 → tokio-rustls wss 回显服务；客户端经
    // WINTERJS_TEST_CA_PEMFILE 接缝信任（生产默认链不变，见 src/builtins/ws.rs）。
    use base64::Engine as _;
    let certified = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()]).unwrap();
    let cert_der = certified.cert.der().to_vec();
    let key_der = certified.signing_key.serialize_der();
    let pem = format!(
        "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
        base64::engine::general_purpose::STANDARD.encode(&cert_der)
    );
    let dir = assert_fs::TempDir::new().unwrap();
    let ca_file = dir.child("test-ca.pem");
    ca_file.write_str(&pem).unwrap();
    let ca_path = ca_file.path().to_owned();

    let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std_listener.set_nonblocking(true).unwrap();
    let port = std_listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let server_config = rustls::ServerConfig::builder()
                    .with_no_client_auth()
                    .with_single_cert(
                        vec![rustls::pki_types::CertificateDer::from(cert_der)],
                        rustls::pki_types::PrivateKeyDer::Pkcs8(
                            rustls::pki_types::PrivatePkcs8KeyDer::from(key_der),
                        ),
                    )
                    .unwrap();
                let acceptor =
                    tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(server_config));
                let listener = tokio::net::TcpListener::from_std(std_listener).unwrap();
                let (stream, _) = listener.accept().await.unwrap();
                let tls = acceptor.accept(stream).await.unwrap();
                let mut ws = tokio_tungstenite::accept_async(tls).await.unwrap();
                use futures::{SinkExt as _, StreamExt as _};
                while let Some(msg) = ws.next().await {
                    let Ok(msg) = msg else { break };
                    if msg.is_text() || msg.is_binary() {
                        if ws.send(msg).await.is_err() {
                            break;
                        }
                    } else if msg.is_close() {
                        let _ = ws.send(tokio_tungstenite::tungstenite::Message::Close(None)).await;
                        while ws.next().await.is_some() {}
                        break;
                    }
                }
            });
    });
    let code = format!(
        r#"const log = []; const ws = new WebSocket("wss://127.0.0.1:{port}/c"); ws.onopen = () => ws.send("secure-ping"); ws.onmessage = (e) => {{ log.push(e.data); ws.close(1000, "bye"); }}; ws.onclose = (e) => console.log(log.join("|") + "|close:" + e.code + ":" + e.wasClean); undefined;"#
    );
    let out = winterjs()
        .env("WINTERJS_TEST_CA_PEMFILE", &ca_path)
        .args(["eval", &code])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "secure-ping|close:1000:true\n"
    );
}

#[test]
fn phase3_fetch_in_flight_abort() {
    // 5s 才回的服务，50ms abort：拒绝带 AbortError，进程不等 5s（超时即挂）。
    let port = serve_http(1, |_head, _body| {
        std::thread::sleep(std::time::Duration::from_secs(5));
        (200, vec![], b"too-late".to_vec())
    });
    let code = format!(
        r#"const c = new AbortController(); const p = fetch("http://127.0.0.1:{port}/slow", {{ signal: c.signal }}); setTimeout(() => c.abort(), 50); try {{ await p; console.log("no-throw"); }} catch (e) {{ console.log("aborted:" + String(e && e.message || e).includes("AbortError")); }}"#
    );
    assert_eq!(
        stdout_of(&mut winterjs().args(["eval", &code])),
        "aborted:true\n"
    );
}

#[test]
fn phase3_fetch_abort_reason_and_late_abort_noop() {
    // 自定义 reason 原样透出；已决议后 abort 不翻转结果。
    let port = serve_http(1, |_head, _body| {
        std::thread::sleep(std::time::Duration::from_secs(5));
        (200, vec![], b"too-late".to_vec())
    });
    let code = format!(
        r#"const c = new AbortController(); const p = fetch("http://127.0.0.1:{port}/slow", {{ signal: c.signal }}); setTimeout(() => c.abort(new Error("custom-stop")), 50); try {{ await p; console.log("no-throw"); }} catch (e) {{ console.log(e.message); }}"#
    );
    assert_eq!(
        stdout_of(&mut winterjs().args(["eval", &code])),
        "custom-stop\n"
    );
    let ok = stdout_of(&mut winterjs().args(["eval",
        r#"const c = new AbortController(); const r = await fetch("data:text/plain,settled", { signal: c.signal }); c.abort(); console.log(await r.text());"#]));
    assert_eq!(ok, "settled\n", "late abort: {ok}");
}

/// 分半写的 hang 服务器（先吐 "abc"，200ms 后吐 "def"，content-length 6）。
fn serve_split() -> u16 {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for _ in 0..8 {
            let Ok((mut s, _)) = listener.accept() else { return };
            let mut head = Vec::new();
            let mut buf = [0u8; 1024];
            loop {
                let Ok(k) = s.read(&mut buf) else { break };
                if k == 0 { break; }
                head.extend_from_slice(&buf[..k]);
                if head.windows(4).any(|w| w == b"\r\n\r\n") { break; }
            }
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 6\r\nconnection: close\r\n\r\nabc");
            let _ = s.flush();
            std::thread::sleep(std::time::Duration::from_millis(200));
            let _ = s.write_all(b"def");
        }
    });
    port
}

#[test]
fn phase3_fetch_body_streams_chunks() {
    // 首个 read 在第二个半包到达前即返回 "abc"（整包缓冲实现会给出 "abcdef"）。
    let port = serve_split();
    let code = format!(
        r#"const r = await fetch("http://127.0.0.1:{port}/split"); const rd = r.body.getReader(); const a = await rd.read(); const b = await rd.read(); const c = await rd.read(); console.log(new TextDecoder().decode(a.value), new TextDecoder().decode(b.value), c.done);"#
    );
    assert_eq!(
        stdout_of(&mut winterjs().args(["eval", &code])),
        "abc def true\n"
    );
}

#[test]
fn phase3_fetch_body_stream_text_and_cancel() {
    // text() 照常拼装流式 body；读一半 cancel 照常退出。
    let port = serve_split();
    let code = format!(
        r#"const r = await fetch("http://127.0.0.1:{port}/split"); console.log(await r.text());"#
    );
    assert_eq!(stdout_of(&mut winterjs().args(["eval", &code])), "abcdef\n");
    let port = serve_split();
    let code = format!(
        r#"const r = await fetch("http://127.0.0.1:{port}/split"); const rd = r.body.getReader(); const a = await rd.read(); console.log(new TextDecoder().decode(a.value)); await rd.cancel(); console.log("cancelled");"#
    );
    assert_eq!(
        stdout_of(&mut winterjs().args(["eval", &code])),
        "abc\ncancelled\n"
    );
}

#[test]
fn phase3_fetch_body_mid_stream_abort() {
    // 流中 abort：已读 chunk 保留，后继 read 以 AbortError 拒绝（非静默 done）。
    let port = serve_split();
    let code = format!(
        r#"const c = new AbortController(); const r = await fetch("http://127.0.0.1:{port}/split", {{ signal: c.signal }}); const rd = r.body.getReader(); const a = await rd.read(); console.log(new TextDecoder().decode(a.value)); c.abort(); try {{ await rd.read(); console.log("no-throw"); }} catch (e) {{ console.log("stream-aborted:" + String(e.message || e).includes("Abort")); }}"#
    );
    assert_eq!(
        stdout_of(&mut winterjs().args(["eval", &code])),
        "abc\nstream-aborted:true\n"
    );
}

/// node: 测试脚手架（tempdir 单文件模块；`run` 执行）。
fn run_node_file(dir: &assert_fs::TempDir, name: &str, source: &str) -> std::process::Output {
    let file = dir.child(name);
    file.write_str(source).unwrap();
    winterjs().arg("run").arg(file.path()).output().unwrap()
}

#[test]
fn phase4_node_path_basic() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(&dir, "p.mjs", r#"
import path, { join, basename, extname, dirname, normalize, relative, isAbsolute, sep, parse } from "node:path";
import { win32, posix } from "node:path";
console.log(join("a", "b", "..", "c"));
console.log(basename("/x/y.ts"), extname("a.d.ts"), extname(".gitignore"), dirname("/x/y/z"));
console.log(normalize("a//b/./c/"), isAbsolute("/x"), isAbsolute("x"), sep);
console.log(relative("/a/b/c", "/a/d"), JSON.stringify(parse("/x/y.ts")).length > 0);
console.log(path.sep === (globalThis.process.platform === "win32" ? win32.sep : posix.sep) ? "ns-ok" : "ns-bad");
console.log(win32.join("C:\\a", "b"), win32.basename("C:\\x\\y.txt"), win32.sep);
console.log(posix.join("a", "b"));
"#);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "a/c\ny.ts .ts  /x/y\n" .to_string()
            + "a/b/c/ true false /\n"
            + "../../d true\n"
            + "ns-ok\n"
            + "C:\\a\\b y.txt \\\n"
            + "a/b\n"
    );
    dir.close().unwrap();
}

#[test]
fn phase4_node_os_basic() {
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"const os = await import("node:os"); console.log([os.platform(), os.arch()].join(",")); console.log(os.EOL.length, os.hostname().length > 0, os.tmpdir().length > 0, os.totalmem() > 0, os.freemem() >= 0, os.cpus().length > 0, typeof os.cpus()[0].model, Object.keys(os.networkInterfaces()).length > 0, os.userInfo().username.length >= 0, os.uptime() >= 0, os.loadavg().length, os.release().length >= 0);"#]));
    let mut lines = out.lines();
    let pa = lines.next().unwrap_or("");
    assert!(
        ["darwin", "linux", "win32", "android"].contains(&pa.split(',').next().unwrap_or("")),
        "platform: {pa}"
    );
    assert!(
        ["arm64", "x64", "arm"].contains(&pa.split(',').nth(1).unwrap_or("")),
        "arch: {pa}"
    );
    assert_eq!(
        lines.next().unwrap_or(""),
        "1 true true true true true string true true true 3 true",
        "os: {out}"
    );
}

#[test]
fn phase4_node_process_argv_env() {
    // argv 透传 + env 读写删查（Proxy 活视图）。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("argv.mjs");
    file.write_str(r#"console.log(process.argv.length, process.argv[2], process.execPath.length > 0, process.pid > 0);"#).unwrap();
    let out = winterjs().arg("run").arg(file.path()).arg("hello").arg("--flag").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8(out.stdout).unwrap().starts_with("4 hello true true\n"), "argv");
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"process.env.WINTERJS_T4 = "v1"; console.log(process.env.WINTERJS_T4, "WINTERJS_T4" in process.env, Object.keys(process.env).includes("WINTERJS_T4")); delete process.env.WINTERJS_T4; console.log(process.env.WINTERJS_T4, "WINTERJS_T4" in process.env);"#]));
    assert_eq!(out, "v1 true true\nundefined false\n", "env: {out}");
    dir.close().unwrap();
}

#[test]
fn phase4_process_exit_codes() {
    // 正常/显式/默认/模块顶层/异步后设码，全走静默退出（无 stderr）。
    let dir = assert_fs::TempDir::new().unwrap();
    let run = |name: &str, src: &str| {
        let f = dir.child(name);
        f.write_str(src).unwrap();
        winterjs().arg("run").arg(f.path()).output().unwrap()
    };
    let out = run("e3.mjs", "process.exit(3);");
    assert_eq!(out.status.code(), Some(3));
    assert!(out.stderr.is_empty(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = run("e0.mjs", "process.exit();");
    assert_eq!(out.status.code(), Some(0));
    let out = run("c7.mjs", "process.exitCode = 7;");
    assert_eq!(out.status.code(), Some(7));
    assert!(out.stderr.is_empty());
    let out = run("t.mjs", "setTimeout(() => { process.exitCode = 5; }, 10);");
    assert_eq!(out.status.code(), Some(5));
    // exit 被 catch 也照退（Node 同 outcome；此处验证退出码，不断言抛）。
    let out = run("caught.mjs", "try { process.exit(4); } catch (e) {}\n");
    assert_eq!(out.status.code(), Some(4));
    assert!(out.stderr.is_empty());
    dir.close().unwrap();
}

#[test]
fn phase4_process_stdio_nexttick_cwd() {
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"process.stdout.write("out-direct"); const order = []; process.nextTick(() => order.push("tick")); Promise.resolve().then(() => order.push("promise")); await new Promise((r) => setTimeout(r, 20)); console.log("|" + order.join(","), process.cwd().length > 0, typeof process.uptime(), typeof process.hrtime.bigint(), process.memoryUsage().rss > 0, process.versions.winterjs.length > 0);"#]));
    assert!(out.starts_with("out-direct|"), "stdio: {out}");
    assert!(out.contains("tick,promise true number bigint true true\n"), "order: {out}");
}

#[test]
fn phase4_node_errors() {
    // 未知内建（静态/动态）给可用列表；exitCode 非整数 TypeError。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(&dir, "bad.mjs", "import x from \"node:nope\";\nconsole.log(x);\n");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("node:nope") && stderr.contains("node:path"), "stderr: {stderr}");
    let out = winterjs().args(["eval", "await import(\"node:nope\")"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let out = winterjs().args(["eval", "process.exitCode = 1.5;"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("integer"), "stderr: {stderr}");
    dir.close().unwrap();
}

/// node:fs 脚手架（workdir 内跑模块；返回 stdout）。
fn run_fs_file(dir: &assert_fs::TempDir, name: &str, source: &str) -> String {
    let file = dir.child(name);
    file.write_str(source).unwrap();
    let out = winterjs().arg("run").arg(file.path()).current_dir(dir.path()).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn phase4_fs_read_write_roundtrip() {
    // 文本/二进制/追加 + stat 字段 + exists。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(&dir, "rw.mjs", r#"
import fs from "node:fs";
fs.writeFileSync("a.txt", "hello");
fs.appendFileSync("a.txt", " world");
console.log(fs.readFileSync("a.txt", "utf8"));
const bin = new Uint8Array([0, 1, 2, 250]);
fs.writeFileSync("b.bin", bin);
const back = fs.readFileSync("b.bin");
console.log(back.length, back[3], back instanceof Uint8Array);
const st = fs.statSync("a.txt");
console.log(st.size, st.isFile(), st.isDirectory(), st.mtime instanceof Date, st.mtimeMs > 0);
console.log(fs.existsSync("a.txt"), fs.existsSync("missing-xyz"), fs.existsSync(123));
"#);
    assert_eq!(out, "hello world\n4 250 true\n11 true false true true\ntrue false false\n", "fs rw: {out}");
    dir.close().unwrap();
}

#[test]
fn phase4_fs_dirs_and_moves() {
    // mkdir -p + readdir(+types) + rename + copy + rm -rf + realpath + mkdtemp.
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(&dir, "dirs.mjs", r#"
import fs from "node:fs";
import path from "node:path";
fs.mkdirSync("d/sub/deep", { recursive: true });
fs.writeFileSync("d/sub/deep/f.txt", "x");
fs.writeFileSync("d/top.txt", "y");
console.log(fs.readdirSync("d").join(","), fs.readdirSync("d/sub").join(","));
const typed = fs.readdirSync("d", { withFileTypes: true });
console.log(typed.map((e) => e.name + ":" + e.isDirectory() + ":" + e.isFile()).join(","));
fs.renameSync("d/top.txt", "d/renamed.txt");
fs.copyFileSync("d/renamed.txt", "d/copied.txt");
console.log(fs.readdirSync("d").join(","));
console.log(fs.realpathSync("d").endsWith("d"));
const tmp = fs.mkdtempSync(path.join(fs.realpathSync("."), "pre-"));
console.log(tmp.includes("pre-"), fs.statSync(tmp).isDirectory());
fs.rmSync("d", { recursive: true, force: true });
console.log(fs.existsSync("d"));
fs.rmSync("missing-xyz", { force: true });
console.log("force-ok");
"#);
    assert_eq!(
        out,
        "sub,top.txt deep\nsub:true:false,top.txt:false:true\ncopied.txt,renamed.txt,sub\ntrue\ntrue true\nfalse\nforce-ok\n",
        "fs dirs: {out}"
    );
    dir.close().unwrap();
}

#[test]
fn phase4_fs_promises_and_errors() {
    // promises 对等 + ENOENT 三件（code/syscall/path）+ lstat 链接 + file: URL 路径。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(&dir, "p.mjs", r#"
import fsp from "node:fs/promises";
import fs from "node:fs";
await fsp.writeFile("p.txt", "via-promises");
console.log(await fsp.readFile("p.txt", "utf8"), (await fsp.stat("p.txt")).size);
try {
  fs.readFileSync("definitely-missing-xyz");
  console.log("no-throw");
} catch (e) {
  console.log(e.code, e.syscall, e.path, e instanceof Error);
}
try {
  await fsp.readFile("definitely-missing-xyz");
  console.log("no-throw");
} catch (e) {
  console.log("async-" + e.code);
}
console.log(fs.readFileSync(new URL("file://" + process.cwd() + "/p.txt"), "utf8"));
"#);
    assert_eq!(
        out,
        "via-promises 12\nENOENT open definitely-missing-xyz true\nasync-ENOENT\nvia-promises\n",
        "fs promises: {out}"
    );
    dir.close().unwrap();
}

#[test]
fn phase4_cp_exec_spawn_sync() {
    // 回显/管道输入/env/cwd + 非零抛错形状 + spawn 缺失命令。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(&dir, "cp.mjs", r#"
import { execSync, spawnSync } from "node:child_process";
console.log(execSync("echo hi").trim());
console.log(execSync("cat", { input: "piped" }).trim());
const r = spawnSync("echo", ["a", "b"], { env: { PATH: process.env.PATH } });
console.log(r.status, r.signal, r.stdout.trim(), r.pid > 0, r.error);
const e = spawnSync("definitely-missing-binary-xyz", []);
console.log(e.status, e.error.code);
try {
  execSync("exit 3");
  console.log("no-throw");
} catch (err) {
  console.log("code:", err.status, err.signal);
}
"#);
    assert_eq!(
        out,
        "hi\npiped\n0 null a b true undefined\nnull ENOENT\ncode: 3 null\n",
        "child_process: {out}"
    );
    dir.close().unwrap();
}

#[test]
fn phase4_cp_timeout_and_shell() {
    // 超时杀直系（SIGKILL 形）+ shell:false 直跑。
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"const { spawnSync, execSync } = await import("node:child_process"); const r = spawnSync("sleep", ["5"], { timeout: 200 }); console.log(r.signal, !!r.error); console.log(execSync("echo noshell", { shell: false }).trim());"#]));
    assert_eq!(out, "SIGKILL true\nnoshell\n", "timeout: {out}");
}

#[test]
fn phase4_node_assert_subset() {
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"const assert = (await import("node:assert")).default; assert.ok(1); assert.strictEqual(1, 1); assert.notStrictEqual(1, "1"); assert.deepStrictEqual({ a: [1, 2] }, { a: [1, 2] }); assert.equal(1, "1"); assert.throws(() => { throw new TypeError("x"); }, TypeError); assert.throws(() => { throw new Error("boom"); }, /boom/); await assert.rejects(async () => { throw new Error("r"); }); assert.match("foobar", /^foo/); assert.ifError(null); console.log("assert-ok"); try { assert.strictEqual(1, 2); } catch (e) { console.log(e.code, e.operator, e.actual, e.expected); }"#]));
    assert_eq!(out, "assert-ok\nERR_ASSERTION strictEqual 1 2\n", "assert: {out}");
}

#[test]
fn phase4_node_test_runner() {
    // 通过/失败/跳过计数 + 小结 + 失败 exitCode=1。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("t.mjs");
    file.write_str("import { test, describe } from \"node:test\";\nimport assert from \"node:assert\";\ndescribe(\"math\", () => {\n  test(\"adds\", () => assert.strictEqual(1 + 1, 2));\n  test(\"fails\", () => assert.strictEqual(1, 2));\n  test.skip(\"skipped\", () => {});\n});\n").unwrap();
    let out = winterjs().arg("run").arg(file.path()).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("not ok - math > fails"), "runner: {stdout}");
    assert!(stdout.contains("# pass 1, fail 1, skip 1, todo 0"), "summary: {stdout}");
    dir.close().unwrap();
}

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
    let out = winterjs().arg("run").arg(main.path()).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout, "main: a/b 42 true true\n", "require: {stdout}");
    dir.close().unwrap();
}

#[test]
fn phase4_require_cycle_partial_exports() {
    // 循环引用见半成品（Node 语义）。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("b.cjs").write_str("const a = require(\"./a.cjs\");\nmodule.exports = { b: 2, aVal: (a.a || 0) + 10 };\n").unwrap();
    dir.child("a.cjs").write_str("const b = require(\"./b.cjs\");\nmodule.exports = { a: 1, bVal: (b.b || 0) + 100 };\n").unwrap();
    let main = dir.child("main.cjs");
    main.write_str("const a = require(\"./a.cjs\");\nconsole.log(\"cycle:\", a.a, a.bVal);\n").unwrap();
    let out = winterjs().arg("run").arg(main.path()).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "cycle: 1 102\n");
    dir.close().unwrap();
}

#[test]
fn phase4_require_errors() {
    // 缺失模块 / ESM 拒绝 / resolve 直给。
    let out = winterjs().args(["eval", "try { require(\"node:nope-xyz\"); } catch (e) { console.log(e.message.slice(0, 30)); }"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Cannot find module"), "missing: {stdout}");
    let dir = assert_fs::TempDir::new().unwrap();
    let mod_ = dir.child("m.mjs");
    mod_.write_str("export const x = 1;\n").unwrap();
    let code = format!("try {{ require({:?}); }} catch (e) {{ console.log(e.message.slice(0, 30)); }}", mod_.path().to_string_lossy());
    let out = winterjs().args(["eval", &code]).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("require() of ES Module"), "esm: {stdout}");
    let out = stdout_of(&mut winterjs().args(["eval", "console.log(require.resolve(\"node:path\"));"]));
    assert_eq!(out, "node:path\n", "resolve: {out}");
    dir.close().unwrap();
}

#[test]
fn phase4_fs_watch_fires_and_closes() {
    // 写文件触发 rename 事件；close 后进程即退（persistent 续命验证）。
    let dir = assert_fs::TempDir::new().unwrap();
    let watchdir = dir.child("watched");
    std::fs::create_dir(watchdir.path()).unwrap();
    let file = dir.child("watch.mjs");
    file.write_str("import fs from \"node:fs\";\nconst w = fs.watch(\"watched\", (ev, file) => { console.log(\"ev:\", ev, file); w.close(); });\nsetTimeout(() => fs.writeFileSync(\"watched/n.txt\", \"x\"), 100);\n").unwrap();
    let out = winterjs().arg("run").arg(file.path()).current_dir(dir.path()).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "ev: rename n.txt\n");
    dir.close().unwrap();
}

#[test]
fn phase4_spawn_async_exit_close_kill() {
    // exit+close 双调 + kill 中断（SIGTERM 形）。
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"const { spawn } = await import("node:child_process"); const log = []; const c = spawn("echo", ["async-hi"], { stdio: "ignore" }); console.log("pid:", c.pid > 0, "killed:", c.killed); c.on("exit", (e) => log.push("exit:" + e.status)); c.on("close", () => { log.push("close"); console.log(log.join("|")); });"#]));
    assert_eq!(out, "pid: true killed: false\nexit:0|close\n", "spawn: {out}");
    let out = stdout_of(&mut winterjs().args(["eval",
        r#"const { spawn } = await import("node:child_process"); const log = []; const c = spawn("sleep", ["30"]); c.on("exit", (e) => log.push("exit:" + e.signal)); c.on("close", () => { log.push("close"); console.log(log.join("|")); }); setTimeout(() => console.log("killed:", c.kill()), 100);"#]));
    assert_eq!(out, "killed: true\nexit:SIGTERM|close\n", "kill: {out}");
}

/// 本地 stub registry（packument JSON；tarball URL 指回本端口，5b 用）。
fn serve_registry() -> u16 {
    let holder = std::sync::Arc::new(std::sync::Mutex::new(None));
    let held = holder.clone();
    let port = serve_http(8, move |head, _body| {
        let line = head.lines().next().unwrap_or("").to_owned();
        let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
        let port = held.lock().unwrap().unwrap_or(0);
        let pack = |name: &str, versions: serde_json::Value, tags: serde_json::Value| {
            serde_json::json!({ "name": name, "dist-tags": tags, "versions": versions }).to_string()
        };
        let ver = |tarball: String, deps: serde_json::Value| {
            serde_json::json!({ "dist": { "tarball": tarball, "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==" }, "dependencies": deps })
        };
        let body = if path == "/left-pad" {
            pack(
                "left-pad",
                serde_json::json!({
                    "1.2.0": ver(format!("http://127.0.0.1:{port}/left-pad/-/left-pad-1.2.0.tgz"), serde_json::json!({})),
                    "1.3.0": ver(format!("http://127.0.0.1:{port}/left-pad/-/left-pad-1.3.0.tgz"), serde_json::json!({})),
                }),
                serde_json::json!({ "latest": "1.3.0" }),
            )
        } else if path == "/app" {
            pack(
                "app",
                serde_json::json!({
                    "1.0.0": ver(format!("http://127.0.0.1:{port}/app/-/app-1.0.0.tgz"), serde_json::json!({ "lib": "^2.0.0" })),
                }),
                serde_json::json!({ "latest": "1.0.0" }),
            )
        } else if path == "/lib" {
            pack(
                "lib",
                serde_json::json!({
                    "2.0.0": ver(format!("http://127.0.0.1:{port}/lib/-/lib-2.0.0.tgz"), serde_json::json!({})),
                    "2.1.0": ver(format!("http://127.0.0.1:{port}/lib/-/lib-2.1.0.tgz"), serde_json::json!({})),
                }),
                serde_json::json!({ "latest": "2.1.0" }),
            )
        } else {
            return (404, vec![], b"nope".to_vec());
        };
        (200, vec![("content-type", "application/json".into())], body.into_bytes())
    });
    *holder.lock().unwrap() = Some(port);
    port
}

#[test]
fn phase5_install_dry_run_stub_registry() {
    // 单包精确解 + 传递解（app→lib^2 取最大 2.1.0）；只打印不落地。
    let port = serve_registry();
    let reg = format!("http://127.0.0.1:{port}");
    let out = stdout_of(
        winterjs()
            .args(["install", "left-pad@^1.0.0", "--dry-run", "--registry"])
            .arg(&reg),
    );
    assert_eq!(
        out,
        format!("left-pad@1.3.0 http://127.0.0.1:{port}/left-pad/-/left-pad-1.3.0.tgz\n"),
        "dry-run single: {out}"
    );
    let out = stdout_of(
        winterjs().args(["install", "app", "--dry-run", "--registry"]).arg(&reg),
    );
    assert_eq!(
        out,
        format!(
            "app@1.0.0 http://127.0.0.1:{port}/app/-/app-1.0.0.tgz\nlib@2.1.0 http://127.0.0.1:{port}/lib/-/lib-2.1.0.tgz\n"
        ),
        "dry-run tree: {out}"
    );
}

#[test]
fn phase5_install_errors() {
    // 空包列表 / 未知包 / 无满足版本，皆 exit=1 且可读。
    let out = winterjs().args(["install", "--dry-run"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let port = serve_registry();
    let reg = format!("http://127.0.0.1:{port}");
    let out = winterjs().args(["install", "no-such-pkg-xyz", "--dry-run", "--registry"]).arg(&reg).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not found"), "stderr: {stderr}");
    let out = winterjs().args(["install", "left-pad@^9.0.0", "--dry-run", "--registry"]).arg(&reg).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no version"), "stderr: {stderr}");
}
