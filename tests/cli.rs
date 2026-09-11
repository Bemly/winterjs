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
    assert_eq!(stdout_of(&mut winterjs().args(["--eval", code])), expected);
}

#[test]
fn eval_uncaught_exception_exit_1_with_plain_format() {
    // AGENTS.md §3 验收格式：Error: eval.js:1:7: boom，exit=1（非 TTY）
    let out = winterjs()
        .args(["--eval", "throw new Error(\"boom\")"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Error: eval.js:1:7: boom"), "stderr: {stderr}");
}

#[test]
fn run_missing_file_reports_chain() {
    let out = winterjs().args(["--run", "/nope/such.js"]).output().unwrap();
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
        stdout_of(&mut winterjs().arg("--run").arg(script.path())),
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
        stdout_of(&mut winterjs().arg("--run").arg("rel.js").current_dir(tmp.path())),
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
    let out = stdout_of(&mut winterjs().args(["--config"]));
    let value: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert!(value["log"].is_object(), "log section present: {out}");

    // 环境变量覆盖（WINTERJS_LOG__COLOR）优先于缺省
    let out = stdout_of(&mut winterjs().env("WINTERJS_LOG__COLOR", "always").args(["--config"]));
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value["log"]["color"], "always");
}

#[test]
fn config_schema_is_valid_json_schema() {
    let out = stdout_of(&mut winterjs().args(["--config", "--schema"]));
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
        .args(["--config"])
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
    let out = stdout_of(&mut winterjs().args(["--completions", "bash"]));
    assert!(out.starts_with("_winterjs()"), "completions: {out}");
}

#[test]
fn man_pages_render_roff() {
    let out = stdout_of(&mut winterjs().arg("--man"));
    assert_eq!(out.matches(".TH").count(), 1, "single man page (flag CLI has no subcommands)");
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
    let out = stdout_of(&mut winterjs().arg("--config").current_dir(tmp.path()));
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
    let out = stdout_of(&mut winterjs().arg("--config").current_dir(tmp.path()));
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
    let out = stdout_of(&mut winterjs().args(["--eval",
        "console.log('1'); setTimeout(()=>console.log('4'),0); Promise.resolve().then(()=>console.log('3')); queueMicrotask(()=>console.log('2'))"]));
    assert_eq!(out, "1\n3\n2\n4\n", "microtask/timer ordering: {out}");
}

#[test]
fn phase1_promise_chain_three_hops() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        "Promise.resolve(1).then(v=>v+1).then(v=>v+1).then(v=>console.log('chain:',v))"]));
    assert!(out.contains("chain: 3"), "chain: {out}");
}

#[test]
fn phase1_top_level_await_acceptance() {
    // docs/plan.md Phase 1 验收样例
    assert_eq!(
        stdout_of(&mut winterjs().args(["--eval",
            "await new Promise(r=>setTimeout(()=>r(1),10))"])),
        "1\n"
    );
}

#[test]
fn phase1_interval_until_cleared() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        "let n=0; const id=setInterval(()=>{n++; console.log('tick',n); if(n>=3) clearInterval(id)},5)"]));
    assert_eq!(out, "tick 1\ntick 2\ntick 3\n", "interval: {out}");
}

#[test]
fn phase1_nested_microtasks() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        "async function f(){ for(let i=0;i<3;i++){ await Promise.resolve(); console.log('micro',i);} } f()"]));
    assert_eq!(out, "micro 0\nmicro 1\nmicro 2\n[object Promise]\n", "nested: {out}");
}

#[test]
fn phase1_structured_clone_json_values() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        "const a={x:1,y:[1,2,{z:'s'}]}; const b=structuredClone(a); console.log(JSON.stringify(b), b===a)"]));
    assert_eq!(
        out,
        "{\"x\":1,\"y\":[1,2,{\"z\":\"s\"}]} false\n",
        "clone object: {out}"
    );
    let out = stdout_of(&mut winterjs().args(["--eval",
        "console.log(JSON.stringify([structuredClone(42), structuredClone('s'), structuredClone(null)]))"]));
    assert_eq!(out, "[42,\"s\",null]\n");
}

#[test]
fn phase1_unhandled_rejection_is_fatal() {
    let out = winterjs().args(["--eval", "Promise.reject(new Error('nope'))"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1), "unhandled rejection must be fatal");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unhandled rejection"), "stderr: {stderr}");
}

#[test]
fn phase1_console_count_and_time() {
    let out = stdout_of(&mut winterjs().args(["--eval",
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
    assert_eq!(stdout_of(&mut winterjs().arg("--run").arg(&entry)), "42\n");
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
    assert_eq!(stdout_of(&mut winterjs().arg("--run").arg(&entry)), "42\n");
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
    assert_eq!(stdout_of(&mut winterjs().arg("--run").arg(&entry)), "b\na\n");
}

#[test]
fn phase2_import_meta_url() {
    let (_dir, entry) = mod_dir(&[("meta.js", "console.log(import.meta.url);\n")], "meta.js");
    let out = stdout_of(&mut winterjs().arg("--run").arg(&entry));
    assert!(out.starts_with("file://") && out.trim_end().ends_with("/meta.js"), "meta url: {out}");
}

#[test]
fn phase2_bare_specifier_missing_friendly_error() {
    let (_dir, entry) = mod_dir(&[("bare.js", "import \"left-pad-xyz-absent\";\n")], "bare.js");
    let out = winterjs().arg("--run").arg(&entry).output().unwrap();
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
    assert_eq!(stdout_of(&mut winterjs().arg("--run").arg(&entry)), "pad!\n");
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
    assert_eq!(stdout_of(&mut winterjs().arg("--run").arg(&entry)), "3\n");
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
    assert_eq!(stdout_of(&mut winterjs().arg("--run").arg(&entry)), "7\n");
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
    assert_eq!(stdout_of(&mut winterjs().arg("--run").arg(&entry)), "42\n");
}

#[test]
fn phase2_top_level_await_entry() {
    let (_dir, entry) = mod_dir(
        &[("tla.js", "await new Promise(r=>setTimeout(()=>r(7),5)).then(v=>console.log(\"tla\",v));\n")],
        "tla.js",
    );
    assert_eq!(stdout_of(&mut winterjs().arg("--run").arg(&entry)), "tla 7\n");
}

#[test]
fn phase2_data_url_import() {
    let (_dir, entry) = mod_dir(
        &[("data.js", "import x from \"data:text/javascript,export default 99\";\nconsole.log(x);\n")],
        "data.js",
    );
    assert_eq!(stdout_of(&mut winterjs().arg("--run").arg(&entry)), "99\n");
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
    let out = winterjs().arg("--run").arg(&entry).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("e.ts:6:1"), "stderr: {stderr}");
    assert!(stderr.contains("boom_ts is not defined"), "stderr: {stderr}");
}

// ── Phase 3a：URL / 编码 / crypto ──────────────────────────────────────────

#[test]
fn phase3_url_components() {
    let out = stdout_of(&mut winterjs().args(["--eval",
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
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"console.log(new URL("/p", "https://h.org/x").href, URL.canParse(':::'), URL.canParse('https://a.b'))"#]));
    assert_eq!(out, "https://h.org/p false true\n", "url base: {out}");
}

#[test]
fn phase3_url_invalid_throws() {
    let out = winterjs().args(["--eval", "new URL(':::')"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Invalid URL"), "stderr: {stderr}");
}

#[test]
fn phase3_usp_live_view() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const u = new URL("https://ex.com/?b=2"); const sp = u.searchParams; sp.append("c", "3"); console.log(u.search, sp === u.searchParams); u.search = "?x=9"; console.log(sp.toString())"#]));
    assert_eq!(out, "?b=2&c=3 true
x=9
", "live view: {out}");
}

#[test]
fn phase3_usp_ops() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const s = new URLSearchParams("z=1&a=2&a=3"); s.sort(); console.log(s.toString(), s.get("a"), s.getAll("a").length, s.size)"#]));
    assert_eq!(out, "a=2&a=3&z=1 2 2 3
", "usp: {out}");
}

#[test]
fn phase3_text_encoder_decoder() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const e = new TextEncoder(); console.log(e.encoding, e.encode("hi").length, JSON.stringify(new TextEncoder().encodeInto("hello", new Uint8Array(3)))); console.log(new TextDecoder().decode(new Uint8Array([104, 105])), new TextDecoder("utf-16le").decode(new Uint8Array([104, 0, 105, 0])));"#]));
    assert_eq!(out, "utf-8 2 {\"read\":3,\"written\":3}\nhi hi\n", "codec: {out}");
}

#[test]
fn phase3_text_decoder_fatal() {
    let out = winterjs()
        .args(["--eval", "new TextDecoder('utf-8', {fatal:true}).decode(new Uint8Array([0xff]))"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let out = stdout_of(&mut winterjs().args(["--eval",
        "console.log(new TextDecoder('utf-8').decode(new Uint8Array([0xff])).length)"]));
    assert_eq!(out, "1
");
}

#[test]
fn phase3_base64_roundtrip() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        "console.log(btoa('hello'), atob('aGVsbG8='))"]));
    assert_eq!(out, "aGVsbG8= hello
", "base64: {out}");
    let out = winterjs().args(["--eval", "btoa('€')"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn phase3_crypto_random() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const v = new Uint8Array(16); console.log(crypto.getRandomValues(v) === v, v.length); const a = crypto.randomUUID(), b = crypto.randomUUID(); console.log(a.length, a !== b, /^[0-9a-f-]{36}$/.test(a))"#]));
    assert_eq!(out, "true 16
36 true true
", "crypto: {out}");
    let out = winterjs()
        .args(["--eval", "crypto.getRandomValues(new Uint8Array(70000))"])
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
        stdout_of(&mut winterjs().args(["--eval", &code])),
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
    let out = stdout_of(&mut winterjs().args(["--eval", &code]));
    assert!(out.starts_with("200 got:a=1&b=2 content-type: text/plain"), "post: {out}");
}

#[test]
fn phase3_fetch_data_and_file() {
    assert_eq!(
        stdout_of(&mut winterjs().args(["--eval",
            r#"const r = await fetch("data:text/plain,hello-fetch"); console.log(r.status, r.ok, await r.text());"#])),
        "200 true hello-fetch\n"
    );
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("f.txt").write_str("hello-file").unwrap();
    let url = format!("file://{}", dir.child("f.txt").path().display());
    let code = format!(r#"console.log(await (await fetch("{url}")).text());"#);
    assert_eq!(stdout_of(&mut winterjs().args(["--eval", &code])), "hello-file\n");
    dir.close().unwrap();
}

#[test]
fn phase3_fetch_errors_are_rejections() {
    // 不支持的 scheme 与连不上的地址都以 rejection 呈现（catch 可接住）
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"console.log(await fetch("blob:xyz").then(() => "no", () => "blob-err"))"#]));
    assert_eq!(out, "blob-err\n", "blob: {out}");
    // 保证关闭的端口：bind 后立刻 drop
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let code = format!(
        r#"console.log(await fetch("http://127.0.0.1:{port}/").then(() => "no", (e) => String(e).includes("fetch failed") ? "net-err" : "other:" + e))"#
    );
    assert_eq!(stdout_of(&mut winterjs().args(["--eval", &code])), "net-err\n");
}

#[test]
fn phase3_headers_request_response_classes() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const h = new Headers([["X-A", "1"], ["x-a", "2"]]); console.log(h.get("x-a"), [...h.keys()].join(",")); const r = new Response("hi", { status: 201 }); console.log(r.status, r.ok, await r.text()); const q = new Request("https://ex.com/a", { method: "post", body: "x" }); console.log(q.method, q.url, await q.text());"#]));
    assert_eq!(out, "1, 2 x-a,x-a
201 true hi
POST https://ex.com/a x
", "classes: {out}");
}

#[test]
fn phase3_abort_signal_pre_abort() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const c = new AbortController(); c.abort(); console.log(await fetch("http://127.0.0.1:9/x", { signal: c.signal }).then(() => 'no', () => 'abort-ok'));"#]));
    assert_eq!(out, "abort-ok
", "abort: {out}");
}

#[test]
fn phase3_subtle_digest_vectors() {
    let out = stdout_of(&mut winterjs().args(["--eval",
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
        .args(["--eval", r#"await crypto.subtle.digest("MD5", new Uint8Array(1))"#])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("NotSupportedError"), "stderr: {stderr}");
}

#[test]
fn phase3_streams_basic() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const rs = new ReadableStream({ start(c) { c.enqueue("a"); c.enqueue("b"); c.close(); } }); const out = []; for await (const x of rs) out.push(x); console.log(out.join(",")); const t = new TransformStream({ transform(c, ctl) { ctl.enqueue(String(c).toUpperCase()); } }); const w = t.writable.getWriter(); w.write("hi"); w.close(); const r = t.readable.getReader(); console.log((await r.read()).value, (await r.read()).done);"#]));
    assert_eq!(out, "a,b\nHI true\n", "streams: {out}");
}

#[test]
fn phase3_streams_pipe_tee_body() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const rs = new ReadableStream({ start(c) { c.enqueue("x"); c.close(); } }); const ts = new TransformStream({ transform(c, ctl) { ctl.enqueue(c + "!"); } }); const out = []; await rs.pipeThrough(ts).pipeTo(new WritableStream({ write(c) { out.push(c); } })); console.log(out.join(",")); const [a, b] = new ReadableStream({ start(c) { c.enqueue(1); c.close(); } }).tee(); console.log(await a.getReader().read().then((x) => x.value), await b.getReader().read().then((x) => x.value)); const r = new Response("stream-me"); console.log(r.body === r.body, (await r.body.getReader().read()).value.length);"#]));
    assert_eq!(out, "x!\n1 1\ntrue 9\n", "pipe: {out}");
}

#[test]
fn phase3_aes_gcm_roundtrip() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const iv = new Uint8Array(12); const key = await crypto.subtle.generateKey({ name: "AES-GCM", length: 256 }, true, ["encrypt", "decrypt"]); const ct = await crypto.subtle.encrypt({ name: "AES-GCM", iv }, key, new TextEncoder().encode("secret")); const pt = await crypto.subtle.decrypt({ name: "AES-GCM", iv }, key, ct); console.log(ct.byteLength, new TextDecoder().decode(pt));"#]));
    assert_eq!(out, "22 secret\n", "aes: {out}");
}

#[test]
fn phase3_hmac_sign_verify() {
    let out = stdout_of(&mut winterjs().args(["--eval",
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
        stdout_of(&mut winterjs().args(["--eval", &code])),
        "ping|bin:7,8|close:1000:true\n"
    );
}

#[test]
fn phase3_websocket_bad_url_and_send_while_connecting() {
    // 非 ws scheme 直接抛；CONNECTING 时 send 抛（连不上的端口测 readyState 报错面）
    let out = winterjs().args(["--eval", r#"new WebSocket("http://x/")"#]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let code = format!(
        r#"const ws = new WebSocket("ws://127.0.0.1:{port}/"); try {{ ws.send("early"); console.log("no-throw"); }} catch (e) {{ console.log("send-while-connecting-throws"); }}"#
    );
    assert_eq!(
        stdout_of(&mut winterjs().args(["--eval", &code])),
        "send-while-connecting-throws\n"
    );
}

#[test]
fn phase3_rsa_pkcs1v15_sign_verify() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const {publicKey, privateKey} = await crypto.subtle.generateKey({ name: "RSASSA-PKCS1-v1_5", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" }, true, ["sign", "verify"]); console.log(publicKey.type, privateKey.type, privateKey.algorithm.modulusLength); const sig = await crypto.subtle.sign("RSASSA-PKCS1-v1_5", privateKey, new TextEncoder().encode("m")); console.log(new Uint8Array(sig).length, await crypto.subtle.verify("RSASSA-PKCS1-v1_5", publicKey, sig, new TextEncoder().encode("m")), await crypto.subtle.verify("RSASSA-PKCS1-v1_5", publicKey, sig, new TextEncoder().encode("x")));"#]));
    assert_eq!(out, "public private 2048\n256 true false\n", "rsa-pkcs1v15: {out}");
}

#[test]
fn phase3_rsa_oaep_roundtrip() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const {publicKey, privateKey} = await crypto.subtle.generateKey({ name: "RSA-OAEP", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" }, true, ["encrypt", "decrypt"]); const ct = await crypto.subtle.encrypt({ name: "RSA-OAEP" }, publicKey, new TextEncoder().encode("secret")); const pt = await crypto.subtle.decrypt({ name: "RSA-OAEP" }, privateKey, ct); console.log(ct.byteLength, new TextDecoder().decode(pt)); const spki = await crypto.subtle.exportKey("spki", publicKey); console.log(new Uint8Array(spki).length);"#]));
    assert_eq!(out, "256 secret\n294\n", "rsa-oaep: {out}");
}

#[test]
fn phase3_rsa_jwk_roundtrip() {
    // 私钥 JWK 来回（n/e/d 进，p/q 恢复）+ 公钥 JWK 进；签名跨导入验证。
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const {publicKey, privateKey} = await crypto.subtle.generateKey({ name: "RSASSA-PKCS1-v1_5", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" }, true, ["sign", "verify"]); const jwk = await crypto.subtle.exportKey("jwk", privateKey); console.log(jwk.kty, typeof jwk.dp, typeof jwk.qi); const priv2 = await crypto.subtle.importKey("jwk", jwk, { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" }, true, ["sign"]); console.log(priv2.type); const pubJwk = await crypto.subtle.exportKey("jwk", publicKey); const pub2 = await crypto.subtle.importKey("jwk", pubJwk, { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" }, true, ["verify"]); const sig = await crypto.subtle.sign("RSASSA-PKCS1-v1_5", priv2, new TextEncoder().encode("m")); console.log(await crypto.subtle.verify("RSASSA-PKCS1-v1_5", pub2, sig, new TextEncoder().encode("m")));"#]));
    assert_eq!(out, "RSA string string\nprivate\ntrue\n", "rsa-jwk: {out}");
}

#[test]
fn phase3_ecdsa_p256_roundtrip() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const {publicKey, privateKey} = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, ["sign", "verify"]); console.log(publicKey.type, privateKey.algorithm.namedCurve); const sig = await crypto.subtle.sign({ name: "ECDSA", hash: "SHA-256" }, privateKey, new TextEncoder().encode("hello")); console.log(new Uint8Array(sig).length, await crypto.subtle.verify({ name: "ECDSA", hash: "SHA-256" }, publicKey, sig, new TextEncoder().encode("hello")), await crypto.subtle.verify({ name: "ECDSA", hash: "SHA-256" }, publicKey, sig, new TextEncoder().encode("bye")));"#]));
    assert_eq!(out, "public P-256\n64 true false\n", "ecdsa: {out}");
}

#[test]
fn phase3_ecdh_derive_and_key() {
    // 共享秘密对称 + deriveKey 出 AES-GCM 可加解密；P-384 JWK/spki/raw 来回。
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const a = await crypto.subtle.generateKey({ name: "ECDH", namedCurve: "P-256" }, true, ["deriveBits", "deriveKey"]); const b = await crypto.subtle.generateKey({ name: "ECDH", namedCurve: "P-256" }, true, ["deriveBits"]); const s1 = new Uint8Array(await crypto.subtle.deriveBits({ name: "ECDH", public: b.publicKey }, a.privateKey, 256)); const s2 = new Uint8Array(await crypto.subtle.deriveBits({ name: "ECDH", public: a.publicKey }, b.privateKey, 256)); console.log(s1.length, s1.join(",") === s2.join(",")); const dk = await crypto.subtle.deriveKey({ name: "ECDH", public: b.publicKey }, a.privateKey, { name: "AES-GCM", length: 128 }, false, ["encrypt"]); console.log(dk.type, dk.algorithm.length, dk.extractable); const kp = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-384" }, true, ["sign", "verify"]); const raw = new Uint8Array(await crypto.subtle.exportKey("raw", kp.publicKey)); console.log(raw.length, raw[0]); const imp = await crypto.subtle.importKey("spki", await crypto.subtle.exportKey("spki", kp.publicKey), { name: "ECDSA", namedCurve: "P-384" }, true, ["verify"]); console.log(imp.type);"#]));
    assert_eq!(out, "32 true\nsecret 128 false\n97 4\npublic\n", "ecdh: {out}");
}

#[test]
fn phase3_asymmetric_errors() {
    // c-4x 已收官：RSA-PSS 可生成（详见 subtle_c4x_*）；坏曲线/错用途/非私钥 derive 进报错面。
    let out = winterjs()
        .args(["--eval", r#"const k = await crypto.subtle.generateKey({ name: "RSA-PSS", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" }, true, ["sign"]); console.log(k.publicKey.algorithm.name);"#])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "RSA-PSS\n");
    let out = winterjs()
        .args(["--eval", r#"await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-192" }, true, ["sign"])"#])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("NotSupportedError"), "stderr: {stderr}");
    let ok = stdout_of(&mut winterjs().args(["--eval",
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
        .args(["--eval", &code])
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
        stdout_of(&mut winterjs().args(["--eval", &code])),
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
        stdout_of(&mut winterjs().args(["--eval", &code])),
        "custom-stop\n"
    );
    let ok = stdout_of(&mut winterjs().args(["--eval",
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
        stdout_of(&mut winterjs().args(["--eval", &code])),
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
    assert_eq!(stdout_of(&mut winterjs().args(["--eval", &code])), "abcdef\n");
    let port = serve_split();
    let code = format!(
        r#"const r = await fetch("http://127.0.0.1:{port}/split"); const rd = r.body.getReader(); const a = await rd.read(); console.log(new TextDecoder().decode(a.value)); await rd.cancel(); console.log("cancelled");"#
    );
    assert_eq!(
        stdout_of(&mut winterjs().args(["--eval", &code])),
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
        stdout_of(&mut winterjs().args(["--eval", &code])),
        "abc\nstream-aborted:true\n"
    );
}

/// node: 测试脚手架（tempdir 单文件模块；`run` 执行）。
fn run_node_file(dir: &assert_fs::TempDir, name: &str, source: &str) -> std::process::Output {
    let file = dir.child(name);
    file.write_str(source).unwrap();
    winterjs().arg("--run").arg(file.path()).output().unwrap()
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
    let out = stdout_of(&mut winterjs().args(["--eval",
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
    let out = winterjs().arg("--run").arg(file.path()).arg("hello").arg("--flag").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8(out.stdout).unwrap().starts_with("4 hello true true\n"), "argv");
    let out = stdout_of(&mut winterjs().args(["--eval",
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
        winterjs().arg("--run").arg(f.path()).output().unwrap()
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
    let out = stdout_of(&mut winterjs().args(["--eval",
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
    let out = winterjs().args(["--eval", "await import(\"node:nope\")"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let out = winterjs().args(["--eval", "process.exitCode = 1.5;"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("integer"), "stderr: {stderr}");
    dir.close().unwrap();
}

/// node:fs 脚手架（workdir 内跑模块；返回 stdout）。
fn run_fs_file(dir: &assert_fs::TempDir, name: &str, source: &str) -> String {
    let file = dir.child(name);
    file.write_str(source).unwrap();
    let out = winterjs().arg("--run").arg(file.path()).current_dir(dir.path()).output().unwrap();
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
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const { spawnSync, execSync } = await import("node:child_process"); const r = spawnSync("sleep", ["5"], { timeout: 200 }); console.log(r.signal, !!r.error); console.log(execSync("echo noshell", { shell: false }).trim());"#]));
    assert_eq!(out, "SIGKILL true\nnoshell\n", "timeout: {out}");
}

#[test]
fn phase4_node_assert_subset() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const assert = (await import("node:assert")).default; assert.ok(1); assert.strictEqual(1, 1); assert.notStrictEqual(1, "1"); assert.deepStrictEqual({ a: [1, 2] }, { a: [1, 2] }); assert.equal(1, "1"); assert.throws(() => { throw new TypeError("x"); }, TypeError); assert.throws(() => { throw new Error("boom"); }, /boom/); await assert.rejects(async () => { throw new Error("r"); }); assert.match("foobar", /^foo/); assert.ifError(null); console.log("assert-ok"); try { assert.strictEqual(1, 2); } catch (e) { console.log(e.code, e.operator, e.actual, e.expected); }"#]));
    assert_eq!(out, "assert-ok\nERR_ASSERTION strictEqual 1 2\n", "assert: {out}");
}

#[test]
fn phase4_node_test_runner() {
    // 通过/失败/跳过计数 + 小结 + 失败 exitCode=1。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("t.mjs");
    file.write_str("import { test, describe } from \"node:test\";\nimport assert from \"node:assert\";\ndescribe(\"math\", () => {\n  test(\"adds\", () => assert.strictEqual(1 + 1, 2));\n  test(\"fails\", () => assert.strictEqual(1, 2));\n  test.skip(\"skipped\", () => {});\n});\n").unwrap();
    let out = winterjs().arg("--run").arg(file.path()).output().unwrap();
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
    let out = winterjs().arg("--run").arg(main.path()).output().unwrap();
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
    let out = winterjs().arg("--run").arg(main.path()).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
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
    let code = format!("try {{ require({:?}); }} catch (e) {{ console.log(e.message.slice(0, 30)); }}", mod_.path().to_string_lossy());
    let out = winterjs().args(["--eval", &code]).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("require() of ES Module"), "esm: {stdout}");
    let out = stdout_of(&mut winterjs().args(["--eval", "console.log(require.resolve(\"node:path\"));"]));
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
    let out = winterjs().arg("--run").arg(file.path()).current_dir(dir.path()).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "ev: rename n.txt\n");
    dir.close().unwrap();
}

#[test]
fn phase4_spawn_async_exit_close_kill() {
    // exit+close 双调 + kill 中断（SIGTERM 形）。
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const { spawn } = await import("node:child_process"); const log = []; const c = spawn("echo", ["async-hi"], { stdio: "ignore" }); console.log("pid:", c.pid > 0, "killed:", c.killed); c.on("exit", (e) => log.push("exit:" + e.status)); c.on("close", () => { log.push("close"); console.log(log.join("|")); });"#]));
    assert_eq!(out, "pid: true killed: false\nexit:0|close\n", "spawn: {out}");
    let out = stdout_of(&mut winterjs().args(["--eval",
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
            .args(["--add", "left-pad@^1.0.0", "--dry-run", "--registry"])
            .arg(&reg),
    );
    assert_eq!(
        out,
        format!("left-pad@1.3.0 http://127.0.0.1:{port}/left-pad/-/left-pad-1.3.0.tgz\n"),
        "dry-run single: {out}"
    );
    let out = stdout_of(
        winterjs().args(["--add", "app", "--dry-run", "--registry"]).arg(&reg),
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
    // 空包列表（clap 拦：--add 至少 1 值）/ 未知包 / 无满足版本，皆非零且可读。
    let out = winterjs().args(["--add", "--dry-run"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--add"), "stderr:\n{err}");
    let port = serve_registry();
    let reg = format!("http://127.0.0.1:{port}");
    let out = winterjs().args(["--add", "no-such-pkg-xyz", "--dry-run", "--registry"]).arg(&reg).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not found"), "stderr: {stderr}");
    let out = winterjs().args(["--add", "left-pad@^9.0.0", "--dry-run", "--registry"]).arg(&reg).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no version"), "stderr: {stderr}");
}

#[test]
fn phase5_npmrc_registry_mirror() {
    // 正常：项目 `.npmrc` 的 registry 生效（不传 --registry 也命中 stub）。
    let port = serve_registry();
    let reg = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    let home = assert_fs::TempDir::new().unwrap();
    dir.child(".npmrc").write_str(&format!("registry={reg}/\n")).unwrap();
    let out = stdout_of(
        winterjs()
            .args(["--add", "left-pad@^1.0.0", "--dry-run"])
            .env("HOME", home.path())
            .env_remove("NPM_CONFIG_REGISTRY")
            .env_remove("npm_config_registry")
            .current_dir(dir.path()),
    );
    assert_eq!(
        out,
        format!("left-pad@1.3.0 http://127.0.0.1:{port}/left-pad/-/left-pad-1.3.0.tgz\n"),
        "npmrc mirror: {out}"
    );
    dir.close().unwrap();
    home.close().unwrap();
}

#[test]
fn phase5_npmrc_bad_registry_errors() {
    // 报错：`.npmrc` 指向连不上的 registry，exit=1 且可读（不碰外网，9 端口必拒）。
    let dir = assert_fs::TempDir::new().unwrap();
    let home = assert_fs::TempDir::new().unwrap();
    dir.child(".npmrc").write_str("registry=http://127.0.0.1:9/\n").unwrap();
    let out = winterjs()
        .args(["--add", "left-pad@^1.0.0", "--dry-run"])
        .env("HOME", home.path())
        .env_remove("NPM_CONFIG_REGISTRY")
        .env_remove("npm_config_registry")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("registry"), "stderr: {stderr}");
    dir.close().unwrap();
    home.close().unwrap();
}

#[test]
fn phase5_registry_flag_overrides_npmrc() {
    // 边界：`--registry` flag 覆盖坏掉的 `.npmrc`（优先级 flag > npmrc）。
    let port = serve_registry();
    let reg = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    let home = assert_fs::TempDir::new().unwrap();
    dir.child(".npmrc").write_str("registry=http://127.0.0.1:9/\n").unwrap();
    let out = stdout_of(
        winterjs()
            .args(["--add", "left-pad@^1.0.0", "--dry-run", "--registry"])
            .arg(&reg)
            .env("HOME", home.path())
            .env_remove("NPM_CONFIG_REGISTRY")
            .env_remove("npm_config_registry")
            .current_dir(dir.path()),
    );
    assert_eq!(
        out,
        format!("left-pad@1.3.0 http://127.0.0.1:{port}/left-pad/-/left-pad-1.3.0.tgz\n"),
        "flag override: {out}"
    );
    dir.close().unwrap();
    home.close().unwrap();
}

#[test]
fn phase5_npm_config_registry_env_overrides_npmrc() {
    // 边界：`NPM_CONFIG_REGISTRY` env 覆盖坏掉的 `.npmrc`（优先级 env > npmrc）。
    let port = serve_registry();
    let reg = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    let home = assert_fs::TempDir::new().unwrap();
    dir.child(".npmrc").write_str("registry=http://127.0.0.1:9/\n").unwrap();
    let out = stdout_of(
        winterjs()
            .args(["--add", "left-pad@^1.0.0", "--dry-run"])
            .env("HOME", home.path())
            .env("NPM_CONFIG_REGISTRY", &reg)
            .env_remove("npm_config_registry")
            .current_dir(dir.path()),
    );
    assert_eq!(
        out,
        format!("left-pad@1.3.0 http://127.0.0.1:{port}/left-pad/-/left-pad-1.3.0.tgz\n"),
        "env override: {out}"
    );
    dir.close().unwrap();
    home.close().unwrap();
}

/// 现场建 git 仓（`git` CLI；`user.*` 经 `-c` 注入，不碰全局配置；返回仓目录）。
/// 含 `package.json(name/index.js)` + 一个 commit + 可选 tag。
fn make_git_repo(name: &str, tagged: bool) -> assert_fs::TempDir {
    fn git(dir: &std::path::Path, args: &[&str]) {
        let mut c = std::process::Command::new("git");
        c.args(args).current_dir(dir).env("GIT_CONFIG_NOSYSTEM", "1");
        let out = c.output().expect("git runs");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }
    let dir = assert_fs::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        format!(r#"{{"name":"{name}","version":"0.1.0","main":"index.js"}}"#),
    )
    .unwrap();
    std::fs::write(dir.path().join("index.js"), b"exports.add = (a, b) => a + b;\n").unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "init"]);
    if tagged {
        git(dir.path(), &["tag", "v1.0.0"]);
    }
    dir
}

#[test]
fn phase5_git_dry_run_local() {
    // 正常：`git+file://` dry-run 解析出 commit（40 hex），不落地。
    let repo = make_git_repo("git-pkg", true);
    let url = format!("file://{}", repo.path().display());
    let home = assert_fs::TempDir::new().unwrap();
    let dir = assert_fs::TempDir::new().unwrap();
    let out = stdout_of(
        winterjs()
            .args(["--add", &format!("git-pkg@git+{url}#v1.0.0"), "--dry-run"])
            .env("HOME", home.path())
            .env_remove("NPM_CONFIG_REGISTRY")
            .env_remove("npm_config_registry")
            .current_dir(dir.path()),
    );
    assert!(out.starts_with(&format!("git-pkg@git+{url}#")), "dry-run: {out}");
    let commit = out.trim().rsplit('#').next().unwrap();
    assert_eq!(commit.len(), 40, "commit hex: {out}");
    dir.close().unwrap();
    home.close().unwrap();
}

#[test]
fn phase5_git_unknown_rev_errors() {
    // 报错：未知 rev，exit=1 且可读。
    let repo = make_git_repo("git-pkg", false);
    let url = format!("file://{}", repo.path().display());
    let home = assert_fs::TempDir::new().unwrap();
    let dir = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .args(["--add", &format!("git-pkg@git+{url}#no-such-ref"), "--dry-run"])
        .env("HOME", home.path())
        .env_remove("NPM_CONFIG_REGISTRY")
        .env_remove("npm_config_registry")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no-such-ref"), "stderr: {stderr}");
    dir.close().unwrap();
    home.close().unwrap();
}

#[test]
fn phase5_git_bare_spec_reads_name() {
    // 边界：裸 `git+…` 无显式名，从源 package.json 读名。
    let repo = make_git_repo("bare-pkg", false);
    let url = format!("file://{}", repo.path().display());
    let home = assert_fs::TempDir::new().unwrap();
    let dir = assert_fs::TempDir::new().unwrap();
    let out = stdout_of(
        winterjs()
            .args(["--add", &format!("git+{url}"), "--dry-run"])
            .env("HOME", home.path())
            .env_remove("NPM_CONFIG_REGISTRY")
            .env_remove("npm_config_registry")
            .current_dir(dir.path()),
    );
    assert!(out.starts_with(&format!("bare-pkg@git+{url}#")), "bare name: {out}");
    dir.close().unwrap();
    home.close().unwrap();
}

#[test]
fn phase5_git_end_to_end_local() {
    // 真装闭环：本地 git 装完 `require` 可跑 + lockfile 记 `git+…#commit`。
    let repo = make_git_repo("git-e2e", false);
    let url = format!("file://{}", repo.path().display());
    let home = assert_fs::TempDir::new().unwrap();
    let cache = assert_fs::TempDir::new().unwrap();
    let dir = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .arg("--add")
        .arg(format!("git+{url}"))
        .env("HOME", home.path())
        .env("WINTERJS_CACHE", cache.path())
        .env_remove("NPM_CONFIG_REGISTRY")
        .env_remove("npm_config_registry")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(dir.path().join("node_modules/git-e2e/package.json").is_file());
    assert!(!dir.path().join("node_modules/git-e2e/.git").exists(), ".git must not land");
    let lock = std::fs::read_to_string(dir.path().join("winterjs-lock.json")).unwrap();
    assert!(lock.contains("\"git-e2e\"") && lock.contains(&format!("git+{url}#")), "lock: {lock}");
    let app = dir.child("app.cjs");
    app.write_str("const t = require(\"git-e2e\");\nconsole.log(t.add(19, 23));\n").unwrap();
    let out = winterjs().arg("--run").arg(app.path()).current_dir(dir.path()).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "42\n");
    dir.close().unwrap();
    home.close().unwrap();
    cache.close().unwrap();
}

#[test]
fn phase5_publish_dry_run_ok() {
    // 正常：`publish --dry-run` 打印名@版/registry/files，不碰网络。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("package.json")
        .write_str(r#"{"name":"pub-pkg","version":"1.2.3","license":"MIT"}"#)
        .unwrap();
    dir.child("index.js").write_str("exports.v = 1;\n").unwrap();
    let out = stdout_of(
        winterjs()
            .args(["--publish", "--dry-run", "--registry", "http://127.0.0.1:9/"])
            .current_dir(dir.path()),
    );
    assert!(out.contains("pub-pkg@1.2.3"), "summary: {out}");
    assert!(out.contains("registry: http://127.0.0.1:9/"), "summary: {out}");
    assert!(out.contains("files:"), "summary: {out}");
    dir.close().unwrap();
}

#[test]
fn phase5_publish_manifest_errors() {
    // 报错：缺名 / 坏 license，皆 exit=1 且可读。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("package.json").write_str(r#"{"version":"1.0.0"}"#).unwrap();
    let out = winterjs().args(["--publish", "--dry-run"]).current_dir(dir.path()).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no name"), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    dir.child("package.json")
        .write_str(r#"{"name":"p","version":"1.0.0","license":"Not-A-License!!"}"#)
        .unwrap();
    let out = winterjs().args(["--publish", "--dry-run"]).current_dir(dir.path()).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("license"), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    dir.close().unwrap();
}

#[test]
fn phase5_login_token_writes_npmrc() {
    // 正常：`login --token` 把 token 行写进 `$HOME/.npmrc`（其他行保留）。
    let home = assert_fs::TempDir::new().unwrap();
    home.child(".npmrc").write_str("registry=http://127.0.0.1:4873/\n").unwrap();
    let dir = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .args(["--login", "--token", "sekret", "--registry", "http://127.0.0.1:4873/"])
        .env("HOME", home.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let npmrc = std::fs::read_to_string(home.path().join(".npmrc")).unwrap();
    assert!(npmrc.contains("//127.0.0.1/:_authToken=sekret"), "npmrc: {npmrc}");
    assert!(npmrc.contains("registry=http://127.0.0.1:4873/"), "npmrc: {npmrc}");
    dir.close().unwrap();
    home.close().unwrap();
}

#[test]
fn phase5_upgrade_dry_run_reports_version() {
    // 正常：`upgrade --dry-run` 打印当前版 + 渠道，不碰网络。
    let out = stdout_of(
        winterjs()
            .args(["--upgrade", "--dry-run"])
            .env_remove("WINTERJS_UPDATE_GITHUB"),
    );
    assert!(out.contains(env!("CARGO_PKG_VERSION")), "version: {out}");
    assert!(out.contains("channel:"), "channel: {out}");
}

#[test]
fn phase5_upgrade_no_channel_errors() {
    // 报错：无渠道真升，exit=1 且指路（不碰网络）。
    let out = winterjs()
        .arg("--upgrade")
        .env_remove("WINTERJS_UPDATE_GITHUB")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("WINTERJS_UPDATE_GITHUB"), "stderr: {stderr}");
}

#[test]
fn phase5_upgrade_dry_run_shows_channel() {
    // 边界：设了渠道时 dry-run 回显渠道，仍不碰网络。
    let out = stdout_of(
        winterjs()
            .args(["--upgrade", "--dry-run"])
            .env("WINTERJS_UPDATE_GITHUB", "someowner/somerepo"),
    );
    assert!(out.contains("github:someowner/somerepo"), "channel: {out}");
}

#[test]
fn phase5_login_oauth_prints_url() {
    // 边界：`login --oauth` 打印授权 URL（headless 下浏览器打不开也不失败）。
    let home = assert_fs::TempDir::new().unwrap();
    let dir = assert_fs::TempDir::new().unwrap();
    let out = stdout_of(
        winterjs()
            .args(["--login", "--oauth", "--registry", "http://127.0.0.1:4873/"])
            .env("HOME", home.path())
            .current_dir(dir.path()),
    );
    assert!(out.contains("http://127.0.0.1:4873/oauth/authorize?"), "url: {out}");
    assert!(out.contains("--token"), "hint: {out}");
    dir.close().unwrap();
    home.close().unwrap();
}

/// 现场造 tgz（`package/` 包裹；`files` 为包内路径→内容）。
fn make_tgz(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut tar_data = Vec::new();
    {
        let mut ar = tar::Builder::new(&mut tar_data);
        for (name, data) in files {
            let mut header = tar::Header::new_gnu();
            header.set_path(format!("package/{name}")).unwrap();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            ar.append(&header, *data).unwrap();
        }
        ar.finish().unwrap();
    }
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write as _;
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(&tar_data).unwrap();
    enc.finish().unwrap()
}

#[test]
fn phase5_install_end_to_end_stub() {
    // 造包→装包→require 可跑→lockfile：真装闭环（tarball 经同一 stub 下发）。
    use base64::Engine as _;
    use sha2::Digest as _;
    let tgz = make_tgz(&[
        ("package.json", br#"{"name":"tiny-pkg","version":"1.0.0","main":"index.js","bin":{"tiny-bin":"cli.js"}}"#),
        ("index.js", b"exports.add = (a, b) => a + b;\n"),
        ("cli.js", b"console.log(\"bin-ok\");\n"),
    ]);
    let integrity = format!("sha512-{}", base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&tgz)));
    let tgz_holder = std::sync::Arc::new(tgz);
    let int_holder = std::sync::Arc::new(integrity);
    let port = serve_http(2, move |head, _body| {
        let line = head.lines().next().unwrap_or("").to_owned();
        let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
        let port = head
            .lines()
            .find_map(|l| l.strip_prefix("Host:").or_else(|| l.strip_prefix("host:")))
            .and_then(|v| v.trim().split(':').nth(1))
            .unwrap_or("")
            .to_owned();
        if path == "/tiny-pkg" {
            let body = serde_json::json!({
                "name": "tiny-pkg",
                "dist-tags": { "latest": "1.0.0" },
                "versions": {
                    "1.0.0": {
                        "dist": {
                            "tarball": format!("http://127.0.0.1:{port}/tiny-pkg/-/tiny-pkg-1.0.0.tgz"),
                            "integrity": *int_holder,
                        },
                        "dependencies": {},
                    },
                },
            })
            .to_string();
            return (200, vec![("content-type", "application/json".into())], body.into_bytes());
        }
        if path == "/tiny-pkg/-/tiny-pkg-1.0.0.tgz" {
            return (200, vec![("content-type", "application/octet-stream".into())], (*tgz_holder).clone());
        }
        (404, vec![], b"nope".to_vec())
    });
    let reg = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    let out = winterjs().arg("--add").arg("tiny-pkg").arg("--registry").arg(&reg).current_dir(dir.path()).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("added tiny-pkg@1.0.0"));
    // 落地断言：包文件 + bin 链接 + lockfile。
    assert!(dir.path().join("node_modules/tiny-pkg/package.json").is_file());
    assert!(dir.path().join("node_modules/.bin/tiny-bin").exists());
    let lock = std::fs::read_to_string(dir.path().join("winterjs-lock.json")).unwrap();
    assert!(lock.contains("\"tiny-pkg\"") && lock.contains("1.0.0") && lock.contains("sha512-"), "lock: {lock}");
    // 装完即跑（裸导入走 node_modules 解析）。
    let app = dir.child("app.cjs");
    app.write_str("const t = require(\"tiny-pkg\");\nconsole.log(t.add(19, 23));\n").unwrap();
    let out = winterjs().arg("--run").arg(app.path()).current_dir(dir.path()).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "42\n");
    dir.close().unwrap();
}

#[test]
fn phase5_cache_second_install_hits_cache() {
    // 二次安装全命中缓存：tarball 只下一次，第二次删 node_modules 重装仍成功，
    // 此时 stub 的 tarball 端点已翻为 404（若回源必败），证明走缓存。
    use base64::Engine as _;
    use sha2::Digest as _;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let tgz = make_tgz(&[
        ("package.json", br#"{"name":"cached-pkg","version":"1.0.0","main":"index.js"}"#),
        ("index.js", b"exports.v = 1;\n"),
    ]);
    let integrity = format!("sha512-{}", base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&tgz)));
    let tgz_holder = std::sync::Arc::new(tgz);
    let int_holder = std::sync::Arc::new(integrity);
    let tarball_hits = std::sync::Arc::new(AtomicUsize::new(0));
    let hits = tarball_hits.clone();
    // 首次 2 请求（packument+tarball），二次 1 请求（packument，tarball 必须零回源）。
    let port = serve_http(3, move |head, _body| {
        let line = head.lines().next().unwrap_or("").to_owned();
        let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
        let port = head
            .lines()
            .find_map(|l| l.strip_prefix("Host:").or_else(|| l.strip_prefix("host:")))
            .and_then(|v| v.trim().split(':').nth(1))
            .unwrap_or("")
            .to_owned();
        if path == "/cached-pkg" {
            let body = serde_json::json!({
                "name": "cached-pkg",
                "dist-tags": { "latest": "1.0.0" },
                "versions": { "1.0.0": {
                    "dist": {
                        "tarball": format!("http://127.0.0.1:{port}/cached-pkg/-/cached-pkg-1.0.0.tgz"),
                        "integrity": *int_holder,
                    },
                    "dependencies": {},
                } },
            })
            .to_string();
            return (200, vec![("content-type", "application/json".into())], body.into_bytes());
        }
        if path == "/cached-pkg/-/cached-pkg-1.0.0.tgz" {
            let n = hits.fetch_add(1, Ordering::SeqCst);
            if n >= 1 {
                return (404, vec![], b"gone".to_vec());
            }
            return (200, vec![("content-type", "application/octet-stream".into())], (*tgz_holder).clone());
        }
        (404, vec![], b"nope".to_vec())
    });
    let reg = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    let cache = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .arg("--add")
        .arg("cached-pkg")
        .arg("--registry")
        .arg(&reg)
        .env("WINTERJS_CACHE", cache.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "first: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(tarball_hits.load(Ordering::SeqCst), 1);
    // 缓存文件已落（pkgs/*.tgz）。
    let cached: Vec<_> = std::fs::read_dir(cache.path().join("pkgs")).unwrap().collect();
    assert_eq!(cached.len(), 1, "cache dir should hold one tgz");
    // 删 node_modules 模拟二次安装（缓存保留）。
    std::fs::remove_dir_all(dir.path().join("node_modules")).unwrap();
    let out = winterjs()
        .arg("--add")
        .arg("cached-pkg")
        .arg("--registry")
        .arg(&reg)
        .env("WINTERJS_CACHE", cache.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "second (cache hit): {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(tarball_hits.load(Ordering::SeqCst), 1, "tarball must not be re-downloaded");
    assert!(dir.path().join("node_modules/cached-pkg/package.json").is_file());
    dir.close().unwrap();
    cache.close().unwrap();
}

#[test]
fn phase5_lifecycle_runs_in_order() {
    // preinstall → install → postinstall 按序跑，cwd 即包目录。
    use base64::Engine as _;
    use sha2::Digest as _;
    let tgz = make_tgz(&[
        (
            "package.json",
            br#"{"name":"life-pkg","version":"1.0.0","scripts":{"preinstall":"printf '%s' pre >> order.txt","install":"printf '%s' \"$npm_lifecycle_event\" >> order.txt","postinstall":"printf '%s' post >> order.txt"}}"#,
        ),
        ("index.js", b"exports.v = 1;\n"),
    ]);
    let integrity = format!("sha512-{}", base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&tgz)));
    let tgz_holder = std::sync::Arc::new(tgz);
    let int_holder = std::sync::Arc::new(integrity);
    let port = serve_http(2, move |head, _body| {
        let line = head.lines().next().unwrap_or("").to_owned();
        let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
        let port = head
            .lines()
            .find_map(|l| l.strip_prefix("Host:").or_else(|| l.strip_prefix("host:")))
            .and_then(|v| v.trim().split(':').nth(1))
            .unwrap_or("")
            .to_owned();
        if path == "/life-pkg" {
            let body = serde_json::json!({
                "name": "life-pkg",
                "dist-tags": { "latest": "1.0.0" },
                "versions": { "1.0.0": {
                    "dist": {
                        "tarball": format!("http://127.0.0.1:{port}/life-pkg/-/life-pkg-1.0.0.tgz"),
                        "integrity": *int_holder,
                    },
                    "dependencies": {},
                } },
            })
            .to_string();
            return (200, vec![("content-type", "application/json".into())], body.into_bytes());
        }
        if path == "/life-pkg/-/life-pkg-1.0.0.tgz" {
            return (200, vec![("content-type", "application/octet-stream".into())], (*tgz_holder).clone());
        }
        (404, vec![], b"nope".to_vec())
    });
    let reg = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    let cache = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .arg("--add")
        .arg("life-pkg")
        .arg("--registry")
        .arg(&reg)
        .env("WINTERJS_CACHE", cache.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let order = std::fs::read_to_string(dir.path().join("node_modules/life-pkg/order.txt")).unwrap();
    assert_eq!(order, "preinstallpost", "lifecycle order: {order}");
    dir.close().unwrap();
    cache.close().unwrap();
}

#[test]
fn phase5_lifecycle_failure_breaks_install() {
    // lifecycle 非零退出即安装失败（可读错误）。
    use base64::Engine as _;
    use sha2::Digest as _;
    let tgz = make_tgz(&[
        ("package.json", br#"{"name":"badlife","version":"1.0.0","scripts":{"postinstall":"exit 3"}}"#),
        ("index.js", b"exports.v = 1;\n"),
    ]);
    let integrity = format!("sha512-{}", base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&tgz)));
    let tgz_holder = std::sync::Arc::new(tgz);
    let int_holder = std::sync::Arc::new(integrity);
    let port = serve_http(2, move |head, _body| {
        let line = head.lines().next().unwrap_or("").to_owned();
        let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
        let port = head
            .lines()
            .find_map(|l| l.strip_prefix("Host:").or_else(|| l.strip_prefix("host:")))
            .and_then(|v| v.trim().split(':').nth(1))
            .unwrap_or("")
            .to_owned();
        if path == "/badlife" {
            let body = serde_json::json!({
                "name": "badlife",
                "dist-tags": { "latest": "1.0.0" },
                "versions": { "1.0.0": {
                    "dist": {
                        "tarball": format!("http://127.0.0.1:{port}/badlife/-/badlife-1.0.0.tgz"),
                        "integrity": *int_holder,
                    },
                    "dependencies": {},
                } },
            })
            .to_string();
            return (200, vec![("content-type", "application/json".into())], body.into_bytes());
        }
        if path == "/badlife/-/badlife-1.0.0.tgz" {
            return (200, vec![("content-type", "application/octet-stream".into())], (*tgz_holder).clone());
        }
        (404, vec![], b"nope".to_vec())
    });
    let reg = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    let cache = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .arg("--add")
        .arg("badlife")
        .arg("--registry")
        .arg(&reg)
        .env("WINTERJS_CACHE", cache.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("postinstall"), "stderr: {stderr}");
    dir.close().unwrap();
    cache.close().unwrap();
}

#[test]
fn phase5_stale_staging_recovered() {
    // kill -9 模拟：孤儿 `.staging-*` + 半写 tmp 残留，下次安装自愈且不 corrupt。
    use base64::Engine as _;
    use sha2::Digest as _;
    let tgz = make_tgz(&[
        ("package.json", br#"{"name":"stale-pkg","version":"1.0.0","main":"index.js"}"#),
        ("index.js", b"exports.v = 1;\n"),
    ]);
    let integrity = format!("sha512-{}", base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&tgz)));
    let tgz_holder = std::sync::Arc::new(tgz);
    let int_holder = std::sync::Arc::new(integrity);
    let port = serve_http(2, move |head, _body| {
        let line = head.lines().next().unwrap_or("").to_owned();
        let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
        let port = head
            .lines()
            .find_map(|l| l.strip_prefix("Host:").or_else(|| l.strip_prefix("host:")))
            .and_then(|v| v.trim().split(':').nth(1))
            .unwrap_or("")
            .to_owned();
        if path == "/stale-pkg" {
            let body = serde_json::json!({
                "name": "stale-pkg",
                "dist-tags": { "latest": "1.0.0" },
                "versions": { "1.0.0": {
                    "dist": {
                        "tarball": format!("http://127.0.0.1:{port}/stale-pkg/-/stale-pkg-1.0.0.tgz"),
                        "integrity": *int_holder,
                    },
                    "dependencies": {},
                } },
            })
            .to_string();
            return (200, vec![("content-type", "application/json".into())], body.into_bytes());
        }
        if path == "/stale-pkg/-/stale-pkg-1.0.0.tgz" {
            return (200, vec![("content-type", "application/octet-stream".into())], (*tgz_holder).clone());
        }
        (404, vec![], b"nope".to_vec())
    });
    let reg = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    let cache = assert_fs::TempDir::new().unwrap();
    // 预埋孤儿暂存（模拟上次中断）。
    let nm = dir.path().join("node_modules");
    std::fs::create_dir_all(nm.join(".staging-999-deadbeef/package")).unwrap();
    std::fs::write(nm.join(".staging-999-deadbeef/package/junk.txt"), b"half").unwrap();
    let out = winterjs()
        .arg("--add")
        .arg("stale-pkg")
        .arg("--registry")
        .arg(&reg)
        .env("WINTERJS_CACHE", cache.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(!nm.join(".staging-999-deadbeef").exists(), "stale staging must be cleaned");
    assert!(nm.join("stale-pkg/package.json").is_file(), "real package must land");
    assert!(!nm.join("stale-pkg/junk.txt").exists(), "orphan junk must not leak into package");
    dir.close().unwrap();
    cache.close().unwrap();
}

// ── Phase 6-d1：serve 静态文件 ─────────────────────────────────────────────

/// 空闲端口（bind :0 取号即放；被抢概率极低，抢了则 connect 轮询超时即红）。
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// 存活 serve 子进程（Drop 即 kill + wait，不泄漏）。
struct ServeGuard {
    child: std::process::Child,
    port: u16,
}

impl Drop for ServeGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 起 `winterjs serve . --port <free> [extra]`，轮询到 connect 成功（5s 超时）。
fn spawn_serve(root: &std::path::Path) -> ServeGuard {
    spawn_serve_args(root, &[])
}

fn spawn_serve_args(root: &std::path::Path, extra: &[&str]) -> ServeGuard {
    let port = free_port();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_winterjs"))
        .args(["--serve", ".", "--port"])
        .arg(port.to_string())
        .args(extra)
        .current_dir(root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("serve spawns");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return ServeGuard { child, port };
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            panic!("serve on :{port} never came up");
        }
        // 子进程早退（如 bind 失败）直接把 stderr 捞出来当失败信息。
        if let Ok(Some(st)) = child.try_wait() {
            panic!("serve exited early: {st}");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// 裸 socket GET（hermetic，不依赖外部 client；`extra` 为附加请求头）。
fn http_get(port: u16, path: &str, extra: &[(&str, &str)]) -> (u16, std::collections::HashMap<String, String>, Vec<u8>) {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    let mut req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n");
    for (k, v) in extra {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes()).unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).unwrap();
    parse_response(&raw)
}

/// 原始 HTTP 响应解析（明文/TLS 共用）。
fn parse_response(raw: &[u8]) -> (u16, std::collections::HashMap<String, String>, Vec<u8>) {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("http response has head");
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let body = raw[split + 4..].to_vec();
    let mut lines = head.lines();
    let status: u16 = lines
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let mut headers = std::collections::HashMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_lowercase(), v.trim().to_owned());
        }
    }
    // 压缩响应走 chunked（tower-http 默认），此处解帧再返回。
    let body = if headers
        .get("transfer-encoding")
        .is_some_and(|v| v.contains("chunked"))
    {
        dechunk(&body)
    } else {
        body
    };
    (status, headers, body)
}

/// 解 HTTP chunked 帧（测试 helper；非法帧即 panic，属测试失败）。
fn dechunk(mut body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let end = body
            .windows(2)
            .position(|w| w == b"\r\n")
            .expect("chunk size line");
        let size_line = std::str::from_utf8(&body[..end]).expect("chunk size utf8");
        let size = usize::from_str_radix(size_line.split(';').next().unwrap().trim(), 16)
            .expect("chunk size hex");
        body = &body[end + 2..];
        if size == 0 {
            break;
        }
        out.extend_from_slice(&body[..size]);
        body = &body[size + 2..];
    }
    out
}

fn serve_fixture() -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("index.html").write_str("<h1>hi</h1>").unwrap();
    dir.child("app.js").write_str("console.log(1);\n").unwrap();
    std::fs::write(dir.path().join("big.bin"), b"0123456789abcdef").unwrap();
    dir
}

#[test]
fn phase6_serve_static_file() {
    // 正常：`/` 落到 index.html（content-type + etag），子路径按 mime，缺失 404。
    let dir = serve_fixture();
    let srv = spawn_serve(dir.path());
    let (st, h, body) = http_get(srv.port, "/", &[]);
    assert_eq!(st, 200);
    assert_eq!(body, b"<h1>hi</h1>");
    assert!(h.get("content-type").is_some_and(|v| v.contains("text/html")), "headers: {h:?}");
    assert!(h.contains_key("etag"), "etag missing: {h:?}");
    let (st, h, body) = http_get(srv.port, "/app.js", &[]);
    assert_eq!(st, 200);
    assert_eq!(body, b"console.log(1);\n");
    assert!(h.get("content-type").is_some_and(|v| v.contains("javascript")), "headers: {h:?}");
    let (st, _, _) = http_get(srv.port, "/nope.txt", &[]);
    assert_eq!(st, 404);
    dir.close().unwrap();
}

#[test]
fn phase6_serve_ts_mime_as_javascript() {
    // 正常：`.ts` 等 TS 家族按 JS MIME（Vite 对等），否则浏览器拒载模块；
    // 边界：不存在的 `.ts` 路径仍 404（重写只动成功响应）。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("main.ts").write_str("export const x: number = 1;\n").unwrap();
    let srv = spawn_serve(dir.path());
    let (st, h, body) = http_get(srv.port, "/main.ts", &[]);
    assert_eq!(st, 200);
    assert_eq!(body, b"export const x: number = 1;\n");
    assert!(h.get("content-type").is_some_and(|v| v.contains("javascript")), "headers: {h:?}");
    assert!(!h.get("content-type").is_some_and(|v| v.contains("video")), "headers: {h:?}");
    let (st, _, _) = http_get(srv.port, "/nope.ts", &[]);
    assert_eq!(st, 404);
    dir.close().unwrap();
}

#[test]
fn phase6_serve_range() {
    // 正常：Range → 206 + Content-Range + 切片 body。
    let dir = serve_fixture();
    let srv = spawn_serve(dir.path());
    let (st, h, body) = http_get(srv.port, "/big.bin", &[("Range", "bytes=0-3")]);
    assert_eq!(st, 206);
    assert_eq!(body, b"0123");
    assert_eq!(h.get("content-range").map(String::as_str), Some("bytes 0-3/16"), "headers: {h:?}");
    dir.close().unwrap();
}

#[test]
fn phase6_serve_bad_dir_errors() {
    // 报错：不存在的目录 exit=1 且可读。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .args(["--serve", "no-such-dir", "--port", "18099"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no-such-dir"), "stderr: {stderr}");
    dir.close().unwrap();
}

#[test]
fn phase6_serve_traversal_blocked() {
    // 边界：`/../` 越界读不到 root 之外的文件（非 200 且不泄露内容）。
    let dir = serve_fixture();
    let secret_name = format!("wjs-outside-secret-{}.txt", std::process::id());
    let secret = dir.path().join("..").join(&secret_name);
    std::fs::write(&secret, b"topsecret").unwrap();
    let srv = spawn_serve(dir.path());
    let (st, _, body) = http_get(srv.port, &format!("/../{secret_name}"), &[]);
    assert_ne!(st, 200, "traversal must not succeed");
    assert!(!body.windows(9).any(|w| w == b"topsecret"), "secret leaked");
    let _ = std::fs::remove_file(&secret);
    dir.close().unwrap();
}

#[test]
fn phase6_serve_gzip() {
    // 正常：大文件 + Accept-Encoding: gzip → content-encoding: gzip，解压一致。
    // （小 body 被轮子默认 predicate 跳过，见 §4.19，故用 5KB。）
    let dir = assert_fs::TempDir::new().unwrap();
    let payload: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(dir.path().join("data.bin"), &payload).unwrap();
    let srv = spawn_serve(dir.path());
    let (st, h, body) = http_get(srv.port, "/data.bin", &[("Accept-Encoding", "gzip")]);
    assert_eq!(st, 200);
    assert_eq!(h.get("content-encoding").map(String::as_str), Some("gzip"), "headers: {h:?}");
    let decoded = {
        use std::io::Read;
        let mut d = flate2::read::GzDecoder::new(&body[..]);
        let mut out = Vec::new();
        d.read_to_end(&mut out).unwrap();
        out
    };
    assert_eq!(decoded, payload);
    dir.close().unwrap();
}

#[test]
fn phase6_serve_cors() {
    // 正常：带 Origin 请求 → access-control-allow-origin: *。
    let dir = serve_fixture();
    let srv = spawn_serve(dir.path());
    let (st, h, _) = http_get(srv.port, "/app.js", &[("Origin", "http://example.com")]);
    assert_eq!(st, 200);
    assert_eq!(h.get("access-control-allow-origin").map(String::as_str), Some("*"), "headers: {h:?}");
    dir.close().unwrap();
}

#[test]
fn phase6_serve_request_trace() {
    // 正常：WINTERJS_LOG=winterjs=debug 下 stderr 有逐请求 method/uri/status 行。
    use std::io::Read;
    let dir = serve_fixture();
    let port = free_port();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_winterjs"))
        .args(["--serve", ".", "--port"])
        .arg(port.to_string())
        .env("WINTERJS_LOG", "winterjs=debug")
        .current_dir(dir.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("serve spawns");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "serve never came up");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let (st, _, _) = http_get(port, "/app.js", &[]);
    assert_eq!(st, 200);
    let _ = child.kill();
    let _ = child.wait();
    let mut stderr = String::new();
    child.stderr.take().unwrap().read_to_string(&mut stderr).unwrap();
    assert!(stderr.contains("method=GET") && stderr.contains("uri=/app.js"), "stderr:\n{stderr}");
    assert!(stderr.contains("status=200"), "stderr:\n{stderr}");
    dir.close().unwrap();
}

#[test]
fn phase6_serve_metrics() {
    // 正常：打 2 个请求后 /metrics 含三指标，且计数行精确递增。
    let dir = serve_fixture();
    let srv = spawn_serve(dir.path());
    let (st, _, _) = http_get(srv.port, "/app.js", &[]);
    assert_eq!(st, 200);
    let (st, _, _) = http_get(srv.port, "/app.js", &[]);
    assert_eq!(st, 200);
    let (st, h, body) = http_get(srv.port, "/metrics", &[]);
    assert_eq!(st, 200);
    assert!(h.get("content-type").is_some_and(|v| v.contains("text/plain")), "headers: {h:?}");
    let text = String::from_utf8_lossy(&body).into_owned();
    assert!(text.contains("winterjs_serve_request_duration_seconds"), "metrics:\n{text}");
    assert!(text.contains("winterjs_serve_in_flight"), "metrics:\n{text}");
    let line = text
        .lines()
        .find(|l| l.starts_with("winterjs_serve_requests_total{method=\"GET\",path=\"/app.js\",status=\"200\"}"))
        .expect("counter line present");
    let count: f64 = line.split_whitespace().nth(1).unwrap().parse().unwrap();
    assert!(count >= 2.0, "counter line: {line}");
    dir.close().unwrap();
}

#[test]
fn phase6_serve_rate_limit() {
    // 边界：`--limit-rps 1` 下连打两请求，第二个 429 + Retry-After。
    // （burst=1，第一发必过、第二发必限，时序确定；/metrics 本身也耗配额故不用它断言。）
    let dir = serve_fixture();
    let srv = spawn_serve_args(dir.path(), &["--limit-rps", "1"]);
    let (st1, _, _) = http_get(srv.port, "/app.js", &[]);
    let (st2, h2, body2) = http_get(srv.port, "/app.js", &[]);
    assert_eq!((st1, st2), (200, 429), "burst then limit");
    assert!(h2.contains_key("retry-after"), "headers: {h2:?}");
    assert_eq!(body2, b"rate limited\n");
    dir.close().unwrap();
}

/// rcgen 自签证书（SAN 127.0.0.1；返回 cert/key 路径 + 信任用 DER）。
fn make_self_signed(dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf, rustls::pki_types::CertificateDer<'static>) {
    let key = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()]).unwrap();
    let cert_pem = key.cert.pem();
    let key_pem = key.signing_key.serialize_pem();
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    std::fs::write(&cert_path, &cert_pem).unwrap();
    std::fs::write(&key_path, &key_pem).unwrap();
    (cert_path, key_path, key.cert.der().clone())
}

/// TLS GET（rustls client 信任自签根； noble negotiates http/1.1 by default）。
fn https_get(
    port: u16,
    path: &str,
    trust: &rustls::pki_types::CertificateDer<'static>,
) -> (u16, std::collections::HashMap<String, String>, Vec<u8>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let mut roots = rustls::RootCertStore::empty();
        roots.add(trust.clone()).unwrap();
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let connector =
            tokio_rustls::TlsConnector::from(std::sync::Arc::new(config));
        let tcp = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let name = rustls::pki_types::ServerName::try_from("127.0.0.1").unwrap();
        let mut tls = connector.connect(name, tcp).await.unwrap();
        tls.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut raw = Vec::new();
        tls.read_to_end(&mut raw).await.unwrap();
        parse_response(&raw)
    })
}

#[test]
fn phase6_serve_tls() {
    // 正常：自签 PEM 起 https，真握手后静态 + /metrics 皆 200。
    let dir = serve_fixture();
    let (cert, key, trust) = make_self_signed(dir.path());
    let srv = spawn_serve_args(
        dir.path(),
        &["--cert", cert.to_str().unwrap(), "--key", key.to_str().unwrap()],
    );
    let (st, _, body) = https_get(srv.port, "/", &trust);
    assert_eq!(st, 200);
    assert_eq!(body, b"<h1>hi</h1>");
    let (st, _, _) = https_get(srv.port, "/metrics", &trust);
    assert_eq!(st, 200);
    dir.close().unwrap();
}

#[test]
fn phase6_serve_tls_half_args() {
    // 报错：只给 --cert 不给 --key，exit=1 且指路（不静默降级明文）。
    let dir = serve_fixture();
    let (cert, _, _) = make_self_signed(dir.path());
    let out = winterjs()
        .args(["--serve", ".", "--port", "18098", "--cert"])
        .arg(&cert)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--cert and --key"), "stderr: {stderr}");
    dir.close().unwrap();
}

#[test]
fn phase6_serve_tls_bad_pem() {
    // 报错：坏 PEM exit=1 且可读（cert/key 双给但内容非法）。
    let dir = serve_fixture();
    let cert = dir.path().join("c.pem");
    let key = dir.path().join("k.pem");
    std::fs::write(&cert, b"not a pem\n").unwrap();
    std::fs::write(&key, b"not a pem\n").unwrap();
    let out = winterjs()
        .args(["--serve", ".", "--port", "18097", "--cert"])
        .arg(&cert)
        .args(["--key"])
        .arg(&key)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("bad --cert"), "stderr: {stderr}");
    dir.close().unwrap();
}

/// 测试 fixture：一个过、一个挂、一个非测试文件（不应被跑）。
fn test_fixture() -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("a.test.js")
        .write_str("import { test } from \"node:test\";\ntest(\"adds\", () => { if (1 + 1 !== 2) throw new Error(\"math\"); });\ntest(\"fails\", () => { throw new Error(\"boom\"); });\n")
        .unwrap();
    dir.child("helper.js").write_str("console.log(\"helper\");\n").unwrap();
    dir
}

#[test]
fn phase7_test_mixed_files() {
    // 正常：子测试 TAP 行透出 + runner 行 + 汇总，有挂则 exit=1。
    let dir = test_fixture();
    let out = winterjs()
        .args(["--test", "."])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("not ok - fails"), "stdout:\n{stdout}");
    assert!(stdout.contains("not ok - a.test.js (exit 1)"), "stdout:\n{stdout}");
    assert!(stdout.contains("# pass 0, fail 1"), "stdout:\n{stdout}");
    assert!(!stdout.contains("helper"), "non-test file must not run:\n{stdout}");
    dir.close().unwrap();
}

#[test]
fn phase7_test_all_pass() {
    // 正常：全过则 exit=0 + `ok -` 行；两个文件同进程连跑（§4.24 引擎单例回归，
    // 修前第二个文件起全部 `failed to init JS engine`）。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("o.test.js")
        .write_str("import { test } from \"node:test\";\ntest(\"ok\", () => {});\n")
        .unwrap();
    dir.child("o2.test.js").write_str("console.log(\"two\");\n").unwrap();
    let out = stdout_of(winterjs().args(["--test", "."]).current_dir(dir.path()));
    assert!(out.contains("ok - o.test.js"), "stdout:\n{out}");
    assert!(out.contains("ok - o2.test.js"), "stdout:\n{out}");
    assert!(out.contains("# pass 2, fail 0"), "stdout:\n{out}");
    dir.close().unwrap();
}

#[test]
fn phase7_test_filter() {
    // 边界：--filter 只跑命中文件（此处零命中 → exit 0 提示行）。
    let dir = test_fixture();
    let out = stdout_of(
        winterjs().args(["--test", ".", "--filter", "zzz*"]).current_dir(dir.path()),
    );
    assert!(out.contains("no test files found"), "stdout:\n{out}");
    dir.close().unwrap();
}

#[test]
fn phase7_test_bad_path() {
    // 报错：不存在的路径 exit=1 且可读。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = winterjs().args(["--test", "no-such-dir"]).current_dir(dir.path()).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no such test path"), "stderr: {stderr}");
    dir.close().unwrap();
}

#[test]
fn phase7_init_closed_loop() {
    // 正常：init 三件 + 内容含名 + 紧接着 `test` 即绿（闭环）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = stdout_of(winterjs().args(["--init", "my-pkg", "--yes"]).current_dir(dir.path()));
    assert!(out.contains("created package.json") && out.contains("created hello.test.js"), "init:\n{out}");
    let pkg = std::fs::read_to_string(dir.path().join("package.json")).unwrap();
    assert!(pkg.contains("\"my-pkg\""), "package.json:\n{pkg}");
    let out = winterjs().arg("--test").arg(".").current_dir(dir.path()).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("ok - hello.test.js"));
    dir.close().unwrap();
}

#[test]
fn phase7_init_bad_name() {
    // 报错：非法名 exit=1 且可读。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = winterjs().args(["--init", "Bad Name!", "--yes"]).current_dir(dir.path()).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("bad package name"), "stderr: {stderr}");
    dir.close().unwrap();
}

#[test]
fn phase7_init_conflict() {
    // 边界：已存在文件不覆盖，第二次 init exit=1 且一个不写。
    let dir = assert_fs::TempDir::new().unwrap();
    assert!(winterjs().args(["--init", "p", "--yes"]).current_dir(dir.path()).output().unwrap().status.success());
    std::fs::write(dir.path().join("index.js"), b"mine\n").unwrap();
    let out = winterjs().args(["--init", "p", "--yes"]).current_dir(dir.path()).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("refusing to overwrite"));
    assert_eq!(std::fs::read(dir.path().join("index.js")).unwrap(), b"mine\n");
    dir.close().unwrap();
}

#[test]
fn phase7_init_needs_yes_without_tty() {
    // 边界：非 TTY 缺 --yes 即报可读错（不挂起等输入）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = winterjs().args(["--init", "p"]).current_dir(dir.path()).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--yes"));
    assert!(!dir.path().join("package.json").exists(), "nothing must be written");
    dir.close().unwrap();
}

/// REPL 会话（stdin 全量喂入后关管；返回 stdout/stderr/exit）。
/// `HOME` 隔离到临时目录（历史文件不污染真 home）。
fn repl_session(input: &str) -> (String, String, i32) {
    use std::io::Write;
    let home = assert_fs::TempDir::new().unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_winterjs"))
        .arg("--repl")
        .env("HOME", home.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("repl spawns");
    {
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(input.as_bytes()).unwrap();
    }
    let out = child.wait_with_output().expect("repl runs");
    // home 取不到 path？TempDir 活到此处，drop 即清理。
    let _ = home.close();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

#[test]
fn phase7_repl_persistent_ctx() {
    // 正常：跨行持久上下文（`const` 次行可用）+ banner + exit 0。
    let (stdout, _, code) = repl_session("const x = 21\nx * 2\n.exit\n");
    assert_eq!(code, 0);
    assert!(stdout.starts_with("winterjs repl"), "banner:\n{stdout}");
    assert!(stdout.contains("42\n"), "stdout:\n{stdout}");
}

#[test]
fn phase7_repl_error_recovery() {
    // 正常：报错行打印后继续，会话不死。
    let (stdout, stderr, code) = repl_session("undefinedVar\n40 + 2\n.exit\n");
    assert_eq!(code, 0);
    assert!(stderr.contains("undefinedVar is not defined"), "stderr:\n{stderr}");
    assert!(stdout.contains("42\n"), "stdout:\n{stdout}");
}

#[test]
fn phase7_repl_help_no_ansi() {
    // 正常 + 边界：`.help` 列命令；非 TTY 输出无 ANSI 转义。
    let (stdout, stderr, code) = repl_session(".help\n\n40+2\n.quit\n");
    assert_eq!(code, 0);
    assert!(stdout.contains(".exit") && stdout.contains(".help"), "stdout:\n{stdout}");
    assert!(stdout.contains("42\n"), "stdout:\n{stdout}");
    assert!(!stdout.contains('\u{1b}'), "stdout must not contain ANSI:\n{stdout:?}");
    assert!(!stderr.contains('\u{1b}'), "stderr must not contain ANSI:\n{stderr:?}");
}

#[test]
fn phase7_repl_syntax_continues() {
    // 边界：语法错误行（非 TTY 无续行）报错后继续。
    let (stdout, stderr, code) = repl_session("1 +\n40 + 2\n.exit\n");
    assert_eq!(code, 0);
    assert!(!stderr.is_empty(), "expected a syntax error on stderr");
    assert!(stdout.contains("42\n"), "stdout:\n{stdout}");
}

// ── Phase 7-e4: bun:sqlite（turso 之上的 Bun 兼容层）─────────────────────────

fn run_sqlite_file(dir: &assert_fs::TempDir, name: &str, source: &str) -> String {
    let file = dir.child(name);
    file.write_str(source).unwrap();
    let out = winterjs().arg("--run").arg(file.path()).current_dir(dir.path()).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn phase7_sqlite_memory_roundtrip() {
    // 正常：建表/参数绑定（positional + named）/get/all/values/as/缓存/事务/回滚/iterate/finalize/close。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_sqlite_file(&dir, "mem.mjs", r#"
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
"#);
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
    let out = run_sqlite_file(&dir, "a.mjs", r#"
import { Database } from "bun:sqlite";
const db = new Database("kv.db");
db.run("CREATE TABLE kv (k TEXT PRIMARY KEY, v BLOB)");
db.run("INSERT INTO kv VALUES ('bin', ?)", new Uint8Array([0, 255, 7]));
db.close();
const db2 = new Database("kv.db");
const back = db2.query("SELECT v FROM kv WHERE k = 'bin'").get();
console.log(back.v instanceof Uint8Array, back.v.length, back.v[1], db2.filename.endsWith("kv.db"));
db2.close();
"#);
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
    let out3 = winterjs().arg("--run").arg(file.path()).current_dir(dir.path()).output().unwrap();
    assert!(out3.status.success(), "stderr: {}", String::from_utf8_lossy(&out3.stderr));
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
    let out = winterjs().arg("--run").arg(file.path()).current_dir(dir.path()).output().unwrap();
    assert!(!out.status.success(), "expected failure");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("bun:sqlite"), "stderr: {err}");
    let ok = dir.child("d.mjs");
    ok.write_str("const { Database } = await import(\"bun:sqlite\");\nconsole.log(typeof Database);\n").unwrap();
    let out2 = winterjs().arg("--run").arg(ok.path()).current_dir(dir.path()).output().unwrap();
    assert!(out2.status.success(), "stderr: {}", String::from_utf8_lossy(&out2.stderr));
    assert_eq!(String::from_utf8(out2.stdout).unwrap(), "function\n");
    dir.close().unwrap();
}

#[test]
fn phase7_test_watch_reruns_on_change() {
    // e5：初始跑一轮 → 改文件防抖重跑（含新输出）→ SIGINT 优雅退出 exit=0。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("a.test.js").write_str("console.log(\"v1\");\n").unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_winterjs"))
        .args(["--test", "--watch"])
        .current_dir(dir.path())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let sink = lines.clone();
    let reader = std::io::BufReader::new(child.stdout.take().unwrap());
    std::thread::spawn(move || {
        let mut r = reader;
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match std::io::BufRead::read_until(&mut r, b'\n', &mut buf) {
                Ok(0) | Err(_) => return,
                Ok(_) => sink
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buf).into_owned()),
            }
        }
    });
    let count_needle = |needle: &str| -> usize {
        lines.lock().unwrap().iter().filter(|l| l.contains(needle)).count()
    };
    let wait_needle = |needle: &str, n: usize| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while count_needle(needle) < n {
            assert!(
                std::time::Instant::now() < deadline,
                "timeout waiting for {n}x '{needle}'; lines: {:?}",
                lines.lock().unwrap()
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    };
    wait_needle("# pass 1, fail 0", 1);
    dir.child("a.test.js").write_str("console.log(\"v2\");\n").unwrap();
    wait_needle("# pass 1, fail 0", 2);
    assert!(count_needle("v2") > 0, "expected v2 output after rerun");
    let _ = std::process::Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status();
    let status = child.wait().unwrap();
    assert!(status.success(), "watch must exit 0 on SIGINT, got {status}");
}

// ── Phase 7-e6: bun:ffi（libloading 之上的纯 Rust 动态调用引擎）──────────────

#[cfg(unix)]
fn build_ffi_dylib(dir: &assert_fs::TempDir) -> String {
    let c = dir.child("ffitest.c");
    c.write_str(
        r#"
#include <stdint.h>
int32_t ffi_add(int32_t a, int32_t b) { return a + b; }
int64_t ffi_mul64(int64_t a, int64_t b) { return a * b; }
double ffi_mix(int32_t a, double b) { return a + b; }
double ffi_sum3(double a, double b, double c) { return a + b + c; }
uint8_t ffi_is_even(uint32_t n) { return (n % 2) == 0; }
void ffi_fill(uint8_t *buf, int32_t len, uint8_t v) { for (int32_t i = 0; i < len; i++) buf[i] = v; }
int32_t ffi_count_zeros(const uint8_t *buf, int32_t len) { int32_t n = 0; for (int32_t i = 0; i < len; i++) if (buf[i] == 0) n++; return n; }
const char *ffi_hello(void) { return "hi from c"; }
float ffi_f32ret(double x) { return (float)(x * 2.0); }
"#,
    )
    .unwrap();
    let name = if cfg!(target_os = "macos") { "libffitest.dylib" } else { "libffitest.so" };
    let out = dir.child(name);
    let mut cmd = std::process::Command::new("cc");
    if cfg!(target_os = "macos") {
        cmd.arg("-dynamiclib");
    } else {
        cmd.args(["-shared", "-fPIC"]);
    }
    let status = cmd.arg("-o").arg(out.path()).arg(c.path()).status().expect("cc must be available");
    assert!(status.success(), "cc failed");
    name.to_string()
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
    let out = winterjs().arg("--run").arg(file.path()).current_dir(dir.path()).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let so = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        so,
        "5 12\n1.5 6.5\n1 0\ntrue true\n3\n1\nhi from c true 9\n42\ntrue 7 2\nfunction\n",
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
    let out = winterjs().arg("--run").arg(file.path()).current_dir(dir.path()).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let so = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        so,
        "load: true\nsymbol: true\narity: true\nargtype: true\nf32arg: TypeError true\ntype: TypeError\nargsfmt: TypeError\nptrtype: TypeError\nnullptr: RangeError: CString: null pointer\ndone\n",
        "ffi errors: {so}"
    );
    dir.close().unwrap();
}

// ── Phase 8-b: --allow-* 权限开关（opt-in 沙箱）─────────────────────────────

fn wjs(args: &[&str], dir: &assert_fs::TempDir) -> (bool, String, String) {
    let out = winterjs().args(args).current_dir(dir.path()).output().unwrap();
    (
        out.status.success(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn phase8_permissions_fs() {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("in/sub").create_dir_all().unwrap();
    dir.child("in/a.txt").write_str("hi").unwrap();

    // 默认（无旗标）= 全开放，行为不变
    let file = dir.child("d.mjs");
    file.write_str("console.log(require(\"node:fs\").readFileSync(\"in/a.txt\", \"utf8\"));\n").unwrap();
    let (ok, out, err) = wjs(&["--run", file.path().to_str().unwrap()], &dir);
    assert!(ok, "default open: {err}");
    assert_eq!(out, "hi\n");

    // --allow-read 裸旗标：读放行、写拒绝
    file.write_str(r#"
const fs = require("node:fs");
console.log(fs.readFileSync("in/a.txt", "utf8"));
try { fs.writeFileSync("out.txt", "x"); console.log("WRITE-OK"); } catch (e) { console.log(e.name); }
"#).unwrap();
    let (ok, out, err) = wjs(&["--run", file.path().to_str().unwrap(), "--allow-read"], &dir);
    assert!(ok, "allow-read: {err}");
    assert_eq!(out, "hi\nPermissionError\n");

    // 路径清单：目录内放行、目录外拒绝（可读错误）
    file.write_str(r#"
const fs = require("node:fs");
try { fs.readFileSync("in/a.txt"); console.log("IN-OK"); } catch (e) { console.log("IN-FAIL", e.name); }
try { fs.readFileSync("/etc/hosts"); console.log("OUT-OK"); } catch (e) { console.log("OUT-FAIL", e.name); }
try { fs.readFileSync("/nonexistent-perm-xyz/f"); } catch (e) { console.log("MISS:", String(e.message).includes("--allow-read")); }
"#).unwrap();
    let (ok, out, err) = wjs(&["--run", file.path().to_str().unwrap(), "--allow-read=in"], &dir);
    assert!(ok, "allow-list: {err}");
    assert_eq!(out, "IN-OK\nOUT-FAIL PermissionError\nMISS: true\n");

    // --allow-all 全开
    file.write_str(r#"
const fs = require("node:fs");
fs.writeFileSync("out2.txt", "z");
console.log(fs.readFileSync("out2.txt", "utf8"));
"#).unwrap();
    let (ok, out, err) = wjs(&["--run", file.path().to_str().unwrap(), "--allow-all"], &dir);
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
    let (ok, out, err) = wjs(&["--run", file.path().to_str().unwrap(), "--allow-env=WJS_TEST_VAR,HOME"], &dir);
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
    let (ok, out, err) = wjs(&["--run", file.path().to_str().unwrap(), "--allow-run=echo"], &dir);
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
    file.write_str(r#"
const { Database } = await import("bun:sqlite");
try { new Database("kv.db"); console.log("DB-OK"); } catch (e) { console.log("DB:", e.name); }
"#).unwrap();
    let (ok, out, err) = wjs(&["--run", file.path().to_str().unwrap(), "--allow-env"], &dir);
    assert!(ok, "sandbox via env: {err}");
    assert!(out.contains("DB: PermissionError"), "out: {out}");
    let (ok, out, err) = wjs(&["--run", file.path().to_str().unwrap(), "--allow-read", "--allow-write"], &dir);
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
    let (ok, out, err) = wjs(&["--run", file.path().to_str().unwrap(), "--allow-read"], &dir);
    assert!(ok, "ffi denied run: {err}");
    assert!(out.contains("FFI: Error true"), "out: {out}");
    let (ok, out, err) = wjs(&["--run", file.path().to_str().unwrap(), "--allow-ffi"], &dir);
    assert!(ok, "ffi allowed: {err}");
    assert!(out.contains("FFI-OK"), "out: {out}");
    dir.close().unwrap();
}

// ── Phase 8-a: winterjs lint/fmt（oxlint/oxfmt 命令穿透）────────────────────

#[cfg(unix)]
fn make_tool_repo(dir: &assert_fs::TempDir, script: &str) {
    use std::os::unix::fs::PermissionsExt as _;
    let bin = dir.child("node_modules").child(".bin");
    bin.create_dir_all().unwrap();
    let tool = bin.child("oxlint");
    tool.write_str(script).unwrap();
    std::fs::set_permissions(tool.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let fmt = bin.child("oxfmt");
    fmt.write_str("#!/bin/sh\necho \"fake-oxfmt got: $@\"\n").unwrap();
    std::fs::set_permissions(fmt.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    dir.child("packages/foo").create_dir_all().unwrap();
}

#[cfg(unix)]
#[test]
fn phase8_lintfmt_passthrough() {
    // 正常：参数原样转发（首参 flag 也在内）/stderr 直出/monorepo 向上查找/fmt --check。
    let dir = assert_fs::TempDir::new().unwrap();
    make_tool_repo(
        &dir,
        "#!/bin/sh\necho \"fake-oxlint args: $@\"\necho \"lint-stderr\" >&2\nexit 0\n",
    );
    let (ok, out, err) = wjs(&["--lint", "src", "--write"], &dir);
    assert!(ok, "lint run: {err}");
    assert!(out.contains("fake-oxlint args: src --write"), "out: {out}");
    assert!(err.contains("lint-stderr"), "stderr must pass through: {err}");
    // monorepo：子目录里跑，向上命中根安装的工具
    let (ok, out, err) = wjs(&["--lint", "."], &dir);
    assert!(ok, "subdir: {err}");
    assert!(out.contains("fake-oxlint args: ."), "out: {out}");
    // fmt 完全透传（oxfmt 默认写回、--check 为 CI 检查，均上游语义）
    let (ok, out, err) = wjs(&["--fmt", "--check", "src"], &dir);
    assert!(ok, "fmt: {err}");
    assert!(out.contains("fake-oxfmt got: --check src"), "out: {out}");
    dir.close().unwrap();
}

#[cfg(unix)]
#[test]
fn phase8_lintfmt_exit_and_notfound() {
    // 退出码透传（非零静默映射 exit code）；本地+PATH 双落空给可读指引。
    let dir = assert_fs::TempDir::new().unwrap();
    make_tool_repo(&dir, "#!/bin/sh\nexit 3\n");
    let file = dir.child("l.mjs");
    let _ = file;
    let out = winterjs().args(["--lint", "src"]).current_dir(dir.path()).output().unwrap();
    assert_eq!(out.status.code(), Some(3), "exit code must forward");
    assert!(out.stdout.is_empty(), "Error::Exit is silent");

    // 未找到：清 PATH（env 清空 + 本地无工具），报两种安装指引
    let empty = assert_fs::TempDir::new().unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_winterjs"))
        .args(["--lint"])
        .current_dir(empty.path())
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("oxlint was not found"), "{err}");
    assert!(err.contains("release:github/oxc-project/oxc"), "{err}");
    assert!(!err.contains("npx"), "{err}");
    dir.close().unwrap();
    empty.close().unwrap();
}


// ── Phase 8-c: sentry 崩溃上报（opt-in）─────────────────────────────────────

#[test]
fn phase8_sentry_optin_never_breaks_cli() {
    // 上报是旁路：坏 DSN 告警后继续；不可达端点不影响 CLI 行为与退出码。
    let dir = assert_fs::TempDir::new().unwrap();

    // 坏 DSN：stderr 告警 + 继续正常执行
    let out = winterjs()
        .args(["--eval", "40 + 2"])
        .env("WINTERJS_SENTRY_DSN", "not-a-valid-dsn")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "42\n");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("not a valid Sentry DSN"), "stderr: {err}");

    // 不可达端点：CLI 照常（transport 后台线程吞错，主流程无感）
    let out = winterjs()
        .args(["--eval", "40 + 2"])
        .env("WINTERJS_SENTRY_DSN", "http://key@127.0.0.1:9/42")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "unreachable dsn must not break cli");
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "42\n");

    // 未设置：无任何告警（默认关闭零成本）
    let out = winterjs()
        .args(["--eval", "40 + 2"])
        .env_remove("WINTERJS_SENTRY_DSN")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "42\n");
    assert!(!String::from_utf8_lossy(&out.stderr).contains("Sentry"), "no sentry noise");
    dir.close().unwrap();
}

// ── CLI 双语（-l/--lang > WINTERJS_LANG > 系统 > en）────────────────────────────
// 本机系统语言可能是中文，所有用例显式定语言，保证任何机器上确定性。

#[test]
fn i18n_help_zh() {
    // 正常：`-l zh` 顶层 help 全中；flag 放子命令后也认（预扫全 argv）
    let out = stdout_of(&mut winterjs().args(["-l", "zh", "--help"]));
    assert!(out.contains("运行 JS 文件"), "zh top help:\n{out}");
    assert!(out.contains("帮助文本语言"), "zh lang flag:\n{out}");
    // flag 世界：动作的值必须紧贴（--run 后直接跟别的 flag 会被当缺值）；
    // 跨 flag 写法是 -l 前置 + 动作给值（--help 短路只展示不执行；扁平 CLI 无 per-action 页）
    let out = stdout_of(&mut winterjs().args(["-l", "zh", "--run", "dummy.js", "--help"]));
    assert!(out.contains("--run") && out.contains("运行 JS 文件"), "zh run flag:\n{out}");
    assert!(out.contains("允许文件系统读取"), "zh flattened perms:\n{out}");
    // `--lang=` 连写
    let out = stdout_of(&mut winterjs().args(["--lang=zh", "--help"]));
    assert!(out.contains("求值内联 JS 代码"), "zh eval flag:\n{out}");
}

#[test]
fn i18n_help_en_pinned() {
    // 正常：`-l en` 在中文系统上也强制英文
    let out = stdout_of(&mut winterjs().args(["-l", "en", "--help"]));
    assert!(out.contains("Run a JS file"), "en top help:\n{out}");
    assert!(!out.contains("运行 JS 文件"), "must not leak zh:\n{out}");
}

#[test]
fn i18n_lang_env_and_precedence() {
    // 正常：无 flag 时 WINTERJS_LANG 生效；有 flag 时 flag 赢
    let out = stdout_of(&mut winterjs().args(["--help"]).env("WINTERJS_LANG", "zh"));
    assert!(out.contains("运行 JS 文件"), "env zh:\n{out}");
    let out = stdout_of(&mut winterjs().args(["-l", "en", "--help"]).env("WINTERJS_LANG", "zh"));
    assert!(out.contains("Run a JS file"), "flag beats env:\n{out}");
}

#[test]
fn i18n_invalid_lang_rejected() {
    // 报错：非法值走 clap 报错（exit=2，英文，clap 自带词不汉化）
    let out = winterjs().args(["-l", "fr", "--help"]).output().unwrap();
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid value"), "clap error:\n{err}");
}

#[test]
fn i18n_unknown_env_falls_back_to_en() {
    // 边界：未知 env 值回退英文（不炸）；`--` 后的 `-l` 不认（脚本参数）
    let out = stdout_of(&mut winterjs().args(["--help"]).env("WINTERJS_LANG", "fr"));
    assert!(out.contains("Run a JS file"), "fallback en:\n{out}");
}

// ── add（工程本地）/ install（全局）拆分 ─────────────────────────────────────

#[test]
fn pm_install_global_lands_in_global_root() {
    // 全局：包落 `WINTERJS_GLOBAL_ROOT/node_modules`，cwd 保持干净；打 PATH 指引。
    use base64::Engine as _;
    use sha2::Digest as _;
    let tgz = make_tgz(&[
        ("package.json", br#"{"name":"g-pkg","version":"1.0.0","main":"index.js","bin":{"g-bin":"cli.js"}}"#),
        ("index.js", b"exports.v = 1;\n"),
        ("cli.js", b"console.log(\"bin-ok\");\n"),
    ]);
    let integrity = format!("sha512-{}", base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&tgz)));
    let tgz_holder = std::sync::Arc::new(tgz);
    let int_holder = std::sync::Arc::new(integrity);
    let port = serve_http(2, move |head, _body| {
        let line = head.lines().next().unwrap_or("").to_owned();
        let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
        let port = head
            .lines()
            .find_map(|l| l.strip_prefix("Host:").or_else(|| l.strip_prefix("host:")))
            .and_then(|v| v.trim().split(':').nth(1))
            .unwrap_or("")
            .to_owned();
        if path == "/g-pkg" {
            let body = serde_json::json!({
                "name": "g-pkg",
                "dist-tags": { "latest": "1.0.0" },
                "versions": {
                    "1.0.0": {
                        "dist": {
                            "tarball": format!("http://127.0.0.1:{port}/g-pkg/-/g-pkg-1.0.0.tgz"),
                            "integrity": *int_holder,
                        },
                        "dependencies": {},
                    },
                },
            })
            .to_string();
            return (200, vec![("content-type", "application/json".into())], body.into_bytes());
        }
        if path == "/g-pkg/-/g-pkg-1.0.0.tgz" {
            return (200, vec![("content-type", "application/octet-stream".into())], (*tgz_holder).clone());
        }
        (404, vec![], b"nope".to_vec())
    });
    let reg = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    let groot = assert_fs::TempDir::new().unwrap();
    // 长 flag `--add` 覆盖（短 flag `-a` 已在迁移用例里全覆盖）
    let out = winterjs()
        .args(["--install", "g-pkg", "--registry", &reg])
        .env("WINTERJS_GLOBAL_ROOT", groot.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("added g-pkg@1.0.0"), "stdout: {stdout}");
    assert!(stdout.contains("global root:"), "PATH hint:\n{stdout}");
    assert!(groot.path().join("node_modules/g-pkg/package.json").is_file());
    assert!(groot.path().join("node_modules/.bin/g-bin").exists());
    // cwd 保持干净：全局安装不污染工程
    assert!(!dir.path().join("node_modules").exists(), "cwd must stay clean");
    assert!(!dir.path().join("winterjs-lock.json").exists(), "lockfile goes to global root");
    dir.close().unwrap();
    groot.close().unwrap();
}

#[test]
fn pm_install_global_dry_run_writes_nothing() {
    // dry-run 全局：只求解不落地（global root 连目录都不建）
    let port = serve_registry();
    let reg = format!("http://127.0.0.1:{port}");
    let groot = assert_fs::TempDir::new().unwrap();
    let target = groot.child("should-not-exist");
    let out = winterjs()
        .args(["--install", "left-pad@^1.0.0", "--dry-run", "--registry", &reg])
        .env("WINTERJS_GLOBAL_ROOT", target.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("left-pad@"));
    assert!(!target.path().exists(), "dry-run must not create global root");
    groot.close().unwrap();
}

#[test]
fn pm_add_requires_packages_flag() {
    // 边界：--add 无值被 clap 直接拦（exit=2），到不了 pm
    let out = winterjs().args(["--add", "--dry-run"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--add"), "stderr:\n{err}");
}

#[test]
fn cli_flag_spec_single_action() {
    // 正常：短 flag 全套（-r/-e/-a/-i）与子命令等价
    assert_eq!(stdout_of(&mut winterjs().args(["-e", "40 + 2"])), "42\n");
    // 报错：多动作互斥（exit=1，可读）
    let out = winterjs().args(["--run", "a.js", "--eval", "1"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("exactly one action"), "stderr:\n{err}");
    // 报错：无动作裸奔指路 --help
    let out = winterjs().args(["--dry-run"]).output().unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--help"), "stderr:\n{err}");
}

// ── SubtleCrypto c-4x（RSA-PSS/Ed25519/X25519/AES-192；向量经 openssl 独立生成）──

/// c-4x 通用：hex 串转 ArrayBuffer（各用例内联，避免 helper 依赖）。
const C4X_HEXJS: &str = r#"const bx = (s) => new Uint8Array(s.match(/../g).map(h => parseInt(h, 16))).buffer;"#;

#[test]
fn subtle_c4x_ed25519_vectors() {
    // 正常：openssl 向量验签 + 签名回环 + pkcs8/spki/jwk 往返；报错：坏长度/坏签；
    // 边界：64B 全零签（合法长度，验签 false 不抛）。
    let code = format!(r#"{C4X_HEXJS}
const MSG = new TextEncoder().encode("winterjs-vector");
const SEED = "b12d94858bb317baa5d40f669a784aa878bb17ad25e149e89594d7d9855b58a0";
const PUB = "a9a53ddffd0e9b2d2b83eb442fac6a95391d07160fe1926f51386d31e786c869";
const SIG = "541366402670fd6d20dbecfb6932e2cb5efe69dc92521d577ba70e355112b6ae40d0cb55a0f839d9b97b2841548edd4bf85da2665da35005bc7f6619463f800e";
const pub1 = await crypto.subtle.importKey("raw", bx(PUB), {{ name: "Ed25519" }}, true, ["verify"]);
const ok = await crypto.subtle.verify("Ed25519", pub1, bx(SIG), MSG);
if (ok !== true) throw new Error("openssl vector verify failed");
// 回环
const kp = await crypto.subtle.generateKey({{ name: "Ed25519" }}, true, ["sign", "verify"]);
const s2 = await crypto.subtle.sign("Ed25519", kp.privateKey, MSG);
if (await crypto.subtle.verify("Ed25519", kp.publicKey, s2, MSG) !== true) throw new Error("roundtrip failed");
const bad = new Uint8Array(s2); bad[0] ^= 1;
if (await crypto.subtle.verify("Ed25519", kp.publicKey, bad.buffer, MSG) !== false) throw new Error("tamper must be false");
const zero = await crypto.subtle.verify("Ed25519", kp.publicKey, new Uint8Array(64).buffer, MSG);
if (zero !== false) throw new Error("zero sig must be false");
// pkcs8/spki/jwk 往返
const skcs8 = await crypto.subtle.exportKey("pkcs8", kp.privateKey);
const k2 = await crypto.subtle.importKey("pkcs8", skcs8, {{ name: "Ed25519" }}, true, ["sign"]);
const s3 = await crypto.subtle.sign("Ed25519", k2, MSG);
if (await crypto.subtle.verify("Ed25519", kp.publicKey, s3, MSG) !== true) throw new Error("pkcs8 roundtrip failed");
const spki = await crypto.subtle.exportKey("spki", kp.publicKey);
const k3 = await crypto.subtle.importKey("spki", spki, {{ name: "Ed25519" }}, true, ["verify"]);
if (await crypto.subtle.verify("Ed25519", k3, s3, MSG) !== true) throw new Error("spki roundtrip failed");
const jwk = await crypto.subtle.exportKey("jwk", kp.privateKey);
if (jwk.kty !== "OKP" || jwk.crv !== "Ed25519" || jwk.alg !== "EdDSA" || typeof jwk.d !== "string") throw new Error("bad Ed JWK: " + JSON.stringify(jwk));
const k4 = await crypto.subtle.importKey("jwk", jwk, {{ name: "Ed25519" }}, true, ["sign"]);
if (await crypto.subtle.verify("Ed25519", kp.publicKey, await crypto.subtle.sign("Ed25519", k4, MSG), MSG) !== true) throw new Error("jwk roundtrip failed");
// 报错：raw 非 32B
try {{ await crypto.subtle.importKey("raw", new Uint8Array(31).buffer, {{ name: "Ed25519" }}, true, ["verify"]); throw new Error("must throw"); }}
catch (e) {{ if (!String(e.message).includes("32 bytes")) throw e; }}
// 报错：公钥验签用私钥对象
try {{ await crypto.subtle.verify("Ed25519", kp.privateKey, s2, MSG); throw new Error("must throw"); }}
catch (e) {{ if (!String(e.message).includes("public key")) throw e; }}
console.log("ed-ok");
"#);
    assert_eq!(stdout_of(&mut winterjs().args(["--eval", &code])), "ed-ok\n");
}

#[test]
fn subtle_c4x_x25519_vectors() {
    // 正常：openssl 向量 derive（双方一致）+ deriveKey 落 AES-GCM；报错：错对端类型；
    // 边界：deriveBits 长度越界。
    let code = format!(r#"{C4X_HEXJS}
const A_PRIV = "302e020100300506032b656e04220420a0e63ac582ee05d53337ba21c948389dc4e3bc0825fd506e2fa0719e038cc84d";
const B_PUB = "302a300506032b656e0321002c1c3ea839b4fb38c52c098df2af755e34cce1d2f657d8d58e3ec58529b56f73";
const EXPECT = "31526c245be4719dee9b1d1efe980c8ac796a6a2c6179a4ffe4ae018a3bc8763";
const hex = (b) => [...new Uint8Array(b)].map(x => x.toString(16).padStart(2, "0")).join("");
const pa = await crypto.subtle.importKey("pkcs8", bx(A_PRIV), {{ name: "X25519" }}, true, ["deriveBits"]);
const pb = await crypto.subtle.importKey("spki", bx(B_PUB), {{ name: "X25519" }}, true, []);
const bits = await crypto.subtle.deriveBits({{ name: "X25519", public: pb }}, pa, 256);
if (hex(bits) !== EXPECT) throw new Error("openssl vector derive failed: " + hex(bits));
// 自生成交换一致
const ka = await crypto.subtle.generateKey({{ name: "X25519" }}, true, ["deriveBits", "deriveKey"]);
const kb = await crypto.subtle.generateKey({{ name: "X25519" }}, true, ["deriveBits", "deriveKey"]);
const sab = hex(await crypto.subtle.deriveBits({{ name: "X25519", public: kb.publicKey }}, ka.privateKey, 256));
const sba = hex(await crypto.subtle.deriveBits({{ name: "X25519", public: ka.publicKey }}, kb.privateKey, 256));
if (sab !== sba) throw new Error("DH commutativity failed");
// deriveKey 落 AES-GCM 加解密
const aes = await crypto.subtle.deriveKey({{ name: "X25519", public: kb.publicKey }}, ka.privateKey, {{ name: "AES-GCM", length: 256 }}, false, ["encrypt", "decrypt"]);
const ct = await crypto.subtle.encrypt({{ name: "AES-GCM", iv: new Uint8Array(12) }}, aes, new TextEncoder().encode("x-secret"));
const pt = await crypto.subtle.decrypt({{ name: "AES-GCM", iv: new Uint8Array(12) }}, aes, ct);
if (new TextDecoder().decode(pt) !== "x-secret") throw new Error("derived AES failed");
// 报错：对端非 X25519 公钥
try {{ await crypto.subtle.deriveBits({{ name: "X25519", public: ka.privateKey }}, kb.privateKey, 256); throw new Error("must throw"); }}
catch (e) {{ if (!String(e.message).includes("public key")) throw e; }}
// 边界：长度越界
try {{ await crypto.subtle.deriveBits({{ name: "X25519", public: kb.publicKey }}, ka.privateKey, 257); throw new Error("must throw"); }}
catch (e) {{ if (!String(e.message).includes("length")) throw e; }}
// jwk 往返（X25519 无 alg，與 Node 一致省略）
const jwk = await crypto.subtle.exportKey("jwk", ka.publicKey);
if (jwk.kty !== "OKP" || jwk.crv !== "X25519" || "alg" in jwk) throw new Error("bad X JWK: " + JSON.stringify(jwk));
console.log("x-ok");
"#);
    assert_eq!(stdout_of(&mut winterjs().args(["--eval", &code])), "x-ok\n");
}

#[test]
fn subtle_c4x_pss_roundtrip() {
    // 正常：生成→签名→验签 + 篡改/错 salt 为 false + jwk PS256；
    // 报错：公钥签名、私钥验签；边界：saltLength 缺省 = digest 长。
    let code = r#"const MSG = new TextEncoder().encode("pss-hello");
const kp = await crypto.subtle.generateKey({ name: "RSA-PSS", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" }, true, ["sign", "verify"]);
const sig = await crypto.subtle.sign({ name: "RSA-PSS", saltLength: 32 }, kp.privateKey, MSG);
if (await crypto.subtle.verify({ name: "RSA-PSS", saltLength: 32 }, kp.publicKey, sig, MSG) !== true) throw new Error("roundtrip failed");
// 缺省 salt（=32）与显式一致口径：交叉验签
const sig2 = await crypto.subtle.sign("RSA-PSS", kp.privateKey, MSG);
if (await crypto.subtle.verify("RSA-PSS", kp.publicKey, sig2, MSG) !== true) throw new Error("default salt failed");
const bad = new Uint8Array(sig); bad[bad.length - 1] ^= 1;
if (await crypto.subtle.verify({ name: "RSA-PSS", saltLength: 32 }, kp.publicKey, bad.buffer, MSG) !== false) throw new Error("tamper must be false");
if (await crypto.subtle.verify({ name: "RSA-PSS", saltLength: 20 }, kp.publicKey, sig, MSG) !== false) throw new Error("wrong salt must be false");
const jwk = await crypto.subtle.exportKey("jwk", kp.privateKey);
if (jwk.kty !== "RSA" || jwk.alg !== "PS256" || typeof jwk.d !== "string") throw new Error("bad PSS JWK");
const k2 = await crypto.subtle.importKey("jwk", jwk, { name: "RSA-PSS", hash: "SHA-256" }, true, ["sign"]);
if (await crypto.subtle.verify({ name: "RSA-PSS", saltLength: 32 }, kp.publicKey, await crypto.subtle.sign({ name: "RSA-PSS", saltLength: 32 }, k2, MSG), MSG) !== true) throw new Error("jwk roundtrip failed");
try { await crypto.subtle.sign({ name: "RSA-PSS", saltLength: 32 }, kp.publicKey, MSG); throw new Error("must throw"); }
catch (e) { if (!String(e.message).includes("private key")) throw e; }
console.log("pss-ok");
"#;
    assert_eq!(stdout_of(&mut winterjs().args(["--eval", code])), "pss-ok\n");
}

#[test]
fn subtle_c4x_aes192() {
    // 正常：192 回环 + raw 24B 导入；报错：20B；边界：192 派生（deriveKey 落 192）。
    let code = r#"const k192 = await crypto.subtle.generateKey({ name: "AES-GCM", length: 192 }, true, ["encrypt", "decrypt"]);
if (k192.algorithm.length !== 192) throw new Error("bad length");
const ct = await crypto.subtle.encrypt({ name: "AES-GCM", iv: new Uint8Array(12) }, k192, new TextEncoder().encode("topsecret"));
if (new TextDecoder().decode(await crypto.subtle.decrypt({ name: "AES-GCM", iv: new Uint8Array(12) }, k192, ct)) !== "topsecret") throw new Error("roundtrip failed");
const raw = await crypto.subtle.exportKey("raw", k192);
if (raw.byteLength !== 24) throw new Error("raw must be 24B");
const k2 = await crypto.subtle.importKey("raw", raw, "AES-GCM", true, ["decrypt"]);
if (new TextDecoder().decode(await crypto.subtle.decrypt({ name: "AES-GCM", iv: new Uint8Array(12) }, k2, ct)) !== "topsecret") throw new Error("raw import failed");
try { await crypto.subtle.importKey("raw", new Uint8Array(20).buffer, "AES-GCM", true, ["decrypt"]); throw new Error("must throw"); }
catch (e) { if (!String(e.message).includes("16/24/32")) throw e; }
try { await crypto.subtle.generateKey({ name: "AES-GCM", length: 100 }, true, ["encrypt"]); throw new Error("must throw"); }
catch (e) { if (!String(e.message).includes("128/192/256")) throw e; }
console.log("aes192-ok");
"#;
    assert_eq!(stdout_of(&mut winterjs().args(["--eval", code])), "aes192-ok\n");
}

// ── Web 流收官：TextDecoder 流式 / Abort 事件 / BYOB ──────────────────────────

#[test]
fn streams_text_decoder_streaming() {
    // 正常：多字节跨片（€ 切两半不提前 FFFD）+ 多片 hello + BOM 跨片 + 空片 flush；
    // 报错：fatal 非法字节；边界：gbk 跨片 + 一次性路径不受影响。
    let code = r#"const d = new TextDecoder();
if (d.decode(new Uint8Array([0xE2]), { stream: true }) !== "") throw new Error("split head must buffer");
if (d.decode(new Uint8Array([0x82, 0xAC]), { stream: true }) !== "€") throw new Error("split tail must emit €");
if (d.decode() !== "") throw new Error("flush must be empty");
const h = new TextDecoder();
const parts = ["hel", "lo, ", "stream"].map(s => new TextEncoder().encode(s));
if (parts.map(p => h.decode(p, { stream: true })).join("") + h.decode() !== "hello, stream") throw new Error("hello split failed");
const b = new TextDecoder();
if (b.decode(new Uint8Array([0xEF]), { stream: true }) !== "") throw new Error("BOM head must buffer");
if (b.decode(new Uint8Array([0xBB, 0xBF, 0x68, 0x69])) !== "hi") throw new Error("BOM split failed");
const g = new TextDecoder("gbk");
if (g.decode(new Uint8Array([0xC4]), { stream: true }) !== "" || g.decode(new Uint8Array([0xE3])) !== "你") throw new Error("gbk split failed");
const f = new TextDecoder("utf-8", { fatal: true });
f.decode(new Uint8Array([0xE2]), { stream: true });
try { f.decode(new Uint8Array([0x28])); throw new Error("must throw"); }
catch (e) { if (!String(e.message).includes("not valid")) throw e; }
// 一次性路径不变：截断直接 FFFD
if (new TextDecoder().decode(new Uint8Array([0xE2])) !== "�") throw new Error("one-shot changed");
if (new TextEncoder().encode("hi", { stream: true }).length !== 2) throw new Error("encoder stream opt");
console.log("td-stream-ok");
"#;
    assert_eq!(stdout_of(&mut winterjs().args(["--eval", code])), "td-stream-ok\n");
}

#[test]
fn streams_abort_events() {
    // 正常：listener 收事件对象（type/target）+ onabort + dispatchEvent；
    // timeout/any；边界：abort 后再加监听不触发（已消费）。
    let code = r#"const c = new AbortController();
let got = null;
c.signal.addEventListener("abort", function (e) { got = e.type + ":" + (e.target === c.signal) + ":" + (this === c.signal); });
let on = null;
c.signal.onabort = (e) => { on = e.type; };
c.abort();
if (got !== "abort:true:true" || on !== "abort") throw new Error("event object failed: " + got + "/" + on);
let late = 0;
c.signal.addEventListener("abort", () => { late++; });
if (late !== 0) throw new Error("late listener must not fire");
const c2 = new AbortController();
if (c2.signal.dispatchEvent({ type: "abort" }) !== true || !c2.signal.aborted) throw new Error("dispatchEvent failed");
if (c2.signal.dispatchEvent({ type: "click" }) !== true) throw new Error("dispatchEvent non-abort");
const t = AbortSignal.timeout(5);
await new Promise(r => setTimeout(r, 30));
if (!t.aborted) throw new Error("timeout failed");
const pre = AbortSignal.abort("early");
const a1 = AbortSignal.any([pre]);
if (!a1.aborted || String(a1.reason) !== "early") throw new Error("any pre-aborted failed");
const c3 = new AbortController();
const a2 = AbortSignal.any([c3.signal]);
c3.abort("zzz");
if (!a2.aborted || String(a2.reason) !== "zzz") throw new Error("any follow failed");
try { AbortSignal.any([{}]); throw new Error("must throw"); }
catch (e) { if (!String(e.message).includes("AbortSignal")) throw e; }
console.log("abort-ev-ok");
"#;
    assert_eq!(stdout_of(&mut winterjs().args(["--eval", code])), "abort-ev-ok\n");
}

#[test]
fn streams_byob() {
    // 正常：enqueue 跨片 + read(view) 部分填充 + 读空 done + byobRequest/respond；
    // 报错：非字节流开 BYOB、非 view、空 view；边界：default reader 照读字节流。
    let code = r#"const rs = new ReadableStream({ type: "bytes", start(c) {
  c.enqueue(new Uint8Array([1, 2, 3]));
  c.enqueue(new Uint8Array([4, 5]));
  c.close();
} });
const r = rs.getReader({ mode: "byob" });
const r1 = await r.read(new Uint8Array(4));
if (r1.done || [...r1.value].join(",") !== "1,2,3,4") throw new Error("byob1 failed");
const r2 = await r.read(new Uint8Array(4));
if (r2.done || [...r2.value].join(",") !== "5") throw new Error("byob2 failed");
const r3 = await r.read(new Uint8Array(4));
if (!r3.done) throw new Error("byob done failed");
// pull + byobRequest/respond
const rs2 = new ReadableStream({ type: "bytes", pull(c) {
  const q = c.byobRequest;
  if (q) { const v = new Uint8Array(q.view.buffer, q.view.byteOffset, 2); v[0] = 7; v[1] = 8; q.respond(2); }
} });
const rr = rs2.getReader({ mode: "byob" });
const x = await rr.read(new Uint8Array(8));
if ([...x.value].join(",") !== "7,8") throw new Error("byobRequest failed");
rr.releaseLock(); await rs2.cancel();
// 报错面
try { new ReadableStream().getReader({ mode: "byob" }); throw new Error("must throw"); }
catch (e) { if (!String(e.message).includes("byte stream")) throw e; }
const rs3 = new ReadableStream({ type: "bytes" });
const r4 = rs3.getReader({ mode: "byob" });
try { await r4.read([1, 2]); throw new Error("must throw"); }
catch (e) { if (!String(e.message).includes("view")) throw e; }
try { await r4.read(new Uint8Array(0)); throw new Error("must throw"); }
catch (e) { if (!String(e.message).includes("empty")) throw e; }
try { new ReadableStream({ type: "stream" }); throw new Error("must throw"); }
catch (e) { if (!String(e.message).includes("'bytes'")) throw e; }
r4.releaseLock(); await rs3.cancel();
// 边界：default reader 照读字节流（整块）
const rs5 = new ReadableStream({ type: "bytes", start(c) { c.enqueue(new Uint8Array([9])); c.close(); } });
const d5 = await rs5.getReader().read();
if (d5.done || [...d5.value].join(",") !== "9") throw new Error("default-on-bytes failed");
console.log("byob-ok");
"#;
    assert_eq!(stdout_of(&mut winterjs().args(["--eval", code])), "byob-ok\n");
}

#[test]
fn node_buffer_global() {
    // 正常：from/toString(hex/base64/utf8)/concat/alloc/byteLength；报错：坏hex/未知编码；
    // 边界：allocUnsafe零填/subarray保持Buffer/compare/equals/copy/write/toJSON。
    let code = r#"const b = Buffer.from("hello");
if (b.toString("hex") !== "68656c6c6f" || b.toString("base64") !== "aGVsbG8=") throw new Error("basic failed");
if (!Buffer.isBuffer(b) || Buffer.isBuffer(new Uint8Array(1))) throw new Error("isBuffer failed");
if (Buffer.byteLength("€") !== 3) throw new Error("byteLength failed");
if (Buffer.concat([Buffer.from("a"), Buffer.from("b")]).toString() !== "ab") throw new Error("concat failed");
if (Buffer.alloc(4, "ab").toString() !== "abab") throw new Error("alloc fill failed");
if (Buffer.from([104, 105]).toString() !== "hi") throw new Error("array failed");
if (Buffer.from("ff", "hex")[0] !== 255) throw new Error("hex failed");
if (Buffer.from("aGVsbG8=", "base64").toString() !== "hello") throw new Error("b64 failed");
const z = Buffer.allocUnsafe(8);
if (z.length !== 8 || ![...z].every((x) => x === 0)) throw new Error("allocUnsafe must be zeroed");
const s = b.subarray(1, 3);
if (!(s instanceof Buffer) || s.toString() !== "el") throw new Error("subarray failed");
if (Buffer.compare(Buffer.from("a"), Buffer.from("b")) >= 0) throw new Error("compare failed");
if (!b.equals(Buffer.from("hello"))) throw new Error("equals failed");
const t = Buffer.alloc(5);
if (b.copy(t, 1) !== 4 || t.slice(1).toString() !== "hell") throw new Error("copy failed");
const w = Buffer.alloc(8);
if (w.write("hi", 2) !== 2 || w.slice(2, 4).toString() !== "hi") throw new Error("write failed");
if (JSON.parse(JSON.stringify(b)).type !== "Buffer") throw new Error("toJSON failed");
// fs 互操作：Buffer 进出 writeFile/readFile
try { Buffer.from("zz", "hex"); throw new Error("must throw"); }
catch (e) { if (!String(e.message).includes("hex")) throw e; }
try { Buffer.from("x", "nope-enc"); throw new Error("must throw"); }
catch (e) { if (!String(e.message).includes("encoding")) throw e; }
console.log("buffer-ok");
"#;
    assert_eq!(stdout_of(&mut winterjs().args(["--eval", code])), "buffer-ok\n");
}

#[test]
fn node_fs_streams() {
    // createReadStream 分块 + createWriteStream 落盘/追加。
    let dir = assert_fs::TempDir::new().unwrap();
    std::fs::write(dir.path().join("in.txt"), b"hello-fs-stream").unwrap();
    let code = r#"import fs from "node:fs";
const rs = fs.createReadStream("in.txt", { highWaterMark: 4 });
let s = "";
for await (const c of rs) s += new TextDecoder().decode(c);
if (s !== "hello-fs-stream") throw new Error("read failed: " + s);
const ws = fs.createWriteStream("out.txt");
const w = ws.getWriter();
await w.write(new TextEncoder().encode("ab"));
await w.write(new TextEncoder().encode("cd"));
await w.close();
if (fs.readFileSync("out.txt", "utf8") !== "abcd") throw new Error("write failed");
const wa = fs.createWriteStream("out.txt", { flags: "a" });
const w2 = wa.getWriter();
await w2.write("ef");
await w2.close();
if (fs.readFileSync("out.txt", "utf8") !== "abcdef") throw new Error("append failed: " + fs.readFileSync("out.txt", "utf8"));
console.log("fs-stream-ok");
"#;
    std::fs::write(dir.path().join("t.mjs"), code).unwrap();
    let out = stdout_of(&mut winterjs().arg("--run").arg(dir.path().join("t.mjs")).current_dir(dir.path()));
    assert_eq!(out, "fs-stream-ok\n", "fs streams: {out}");
    dir.close().unwrap();
}

#[test]
#[cfg(unix)]
fn node_spawn_pipe_streams() {
    // pipe：echo 回环 + cat stdin 写/关 + exit/close（--eval 经动态 import，见既有 spawn 用例）。
    // 注意：close 监听必须在 read 之前注册（echo 退出快，否则分发时无监听即摘除，后续 await 永挂）。
    let code = r#"const { spawn } = await import("node:child_process");
const c = spawn("/bin/echo", ["hi-echo"], { stdio: ["ignore", "pipe", "ignore"] });
const closed = new Promise((res) => c.on("close", res));
const x = await c.stdout.getReader().read();
if (new TextDecoder().decode(x.value).trim() !== "hi-echo") throw new Error("echo failed");
await closed;
const c2 = spawn("cat", [], { stdio: "pipe" });
const closed2 = new Promise((res) => c2.on("close", res));
const w = c2.stdin.getWriter();
await w.write("hi-stdin");
await w.close();
let out = "";
const r = c2.stdout.getReader();
for (;;) { const y = await r.read(); if (y.done) break; out += new TextDecoder().decode(y.value); }
if (out !== "hi-stdin") throw new Error("cat failed: " + JSON.stringify(out));
await closed2;
console.log("pipe-ok");
"#;
    assert_eq!(stdout_of(&mut winterjs().args(["--eval", code])), "pipe-ok\n");
}

// ── publish 真 PUT（stub registry 接 PUT；token 经 npmrc）──────────────────────

#[test]
fn pm_publish_put_end_to_end() {
    // 真发布闭环：打包→PUT→200；stub 验方法/路径/Bearer/包体形状；无 token 指路 login。
    use std::sync::{Arc, Mutex};
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen2 = seen.clone();
    let port = serve_http(1, move |head, body| {
        let mut log = seen2.lock().unwrap();
        let line = head.lines().next().unwrap_or("").to_owned();
        log.push(line.clone());
        let auth = head.lines().find_map(|l| l.strip_prefix("authorization:").or_else(|| l.strip_prefix("Authorization:"))).unwrap_or("").trim().to_owned();
        log.push(format!("auth:{auth}"));
        // 包体形状：versions + dist-tags + _attachments 带 b64
        let v: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&body)).unwrap_or_default();
        let ok = v.get("versions").and_then(|x| x.get("1.0.0")).is_some()
            && v.get("dist-tags").and_then(|x| x.get("latest")).and_then(|x| x.as_str()) == Some("1.0.0")
            && v.get("_attachments").and_then(|a| a.as_object()).is_some_and(|m| m.len() == 1);
        log.push(format!("body-ok:{ok}"));
        (200, vec![("content-type", "application/json".into())], b"{}".to_vec())
    });
    let reg = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("package.json").write_str(r#"{"name":"my-pkg","version":"1.0.0","license":"MIT"}"#).unwrap();
    dir.child("index.js").write_str("exports.v = 1;\n").unwrap();
    dir.child(".npmrc").write_str(&format!("registry={reg}\n//127.0.0.1/:_authToken=sekret\n")).unwrap();
    let out = winterjs().args(["--publish", "--registry", &reg]).current_dir(dir.path()).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("published my-pkg@1.0.0"), "stdout: {stdout}");
    let log = seen.lock().unwrap();
    assert!(log.iter().any(|l| l.starts_with("PUT /my-pkg ")), "method/path: {log:?}");
    assert!(log.iter().any(|l| l == "auth:Bearer sekret"), "auth: {log:?}");
    assert!(log.iter().any(|l| l == "body-ok:true"), "body: {log:?}");
    dir.close().unwrap();
}

#[test]
fn pm_publish_errors() {
    // 报错：无 token 指路 login（exit=1）；dry-run 不碰网络。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("package.json").write_str(r#"{"name":"p","version":"1.0.0","license":"MIT"}"#).unwrap();
    dir.child("index.js").write_str("1").unwrap();
    // 无 token（HOME 隔离防污染真机 npmrc）
    let home = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .args(["--publish", "--registry", "http://127.0.0.1:9/"])
        .env("HOME", home.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--login"), "stderr:\n{err}");
    // dry-run 不碰网络（坏 registry 也过）
    let out = winterjs()
        .args(["--publish", "--dry-run", "--registry", "http://127.0.0.1:9/"])
        .env("HOME", home.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8(out.stdout).unwrap().contains("p@1.0.0"));
    dir.close().unwrap();
    home.close().unwrap();
}

// ── ACME 自动证书（dry-run 只校验打印，不碰网络；真签发需公网 :80 + DNS）─────

#[test]
fn serve_acme_dry_run_plan() {
    // 正常：dry-run 打印 domain/email/directory/cache，不碰网络不绑端口。
    let dir = assert_fs::TempDir::new().unwrap();
    let cache = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .args(["--serve", ".", "--acme-domain", "winterjs.bemly.moe", "--acme-email", "a@b.c",
            "--acme-cache", cache.path().to_str().unwrap(), "--dry-run"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("winterjs.bemly.moe"), "stdout: {stdout}");
    assert!(stdout.contains("staging"), "default staging: {stdout}");
    // 生产旗标切换目录
    let out = winterjs()
        .args(["--serve", ".", "--acme-email", "a@b.c", "--acme-production", "--dry-run"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(!stdout.contains("staging") && stdout.contains("acme-v02"), "production: {stdout}");
    dir.close().unwrap();
    cache.close().unwrap();
}

#[test]
fn serve_acme_cert_conflict() {
    // 报错：--acme-* 与 --cert/--key 互斥（exit=1，可读）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .args(["--serve", ".", "--acme-email", "a@b.c", "--cert", "c.pem"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--acme-"), "stderr:\n{err}");
    dir.close().unwrap();
}

// ── loader http(s)（远端导入；stub 回环，不碰外网）────────────────────────────

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
        (200, vec![("content-type", "text/javascript".into())], body.as_bytes().to_vec())
    });
    let code = format!(
        "const m = await import(\"http://127.0.0.1:{port}/main.mjs\"); \
         const m2 = await import(\"http://127.0.0.1:{port}/main.mjs\"); \
         console.log(m.double, m === m2);"
    );
    assert_eq!(stdout_of(&mut winterjs().args(["--eval", &code])), "84 true\n");
}

#[test]
fn loader_http_errors() {
    // 报错：404 可读错（exit=1）；边界：超大/非 UTF-8 由单元口径覆盖，此处只钉 404。
    let port = serve_http(1, move |_head, _body| {
        (404, vec![], b"nope".to_vec())
    });
    let code = format!("await import(\"http://127.0.0.1:{port}/missing.mjs\")");
    let out = winterjs().args(["--eval", &code]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("404"), "stderr:\n{err}");
}

#[test]
fn run_entry_respects_package_json_type() {
    // 入口与 require() 同口径：`type: module` 包的 extensionless bin 走模块；
    // 无 type 即经典（Node 口径）；`type: commonjs` 的 ESM 语法自然报 SyntaxError。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("package.json").write_str(r#"{"type":"module"}"#).unwrap();
    dir.child("lib.mjs").write_str("export const x = 1;\n").unwrap();
    dir.child("bin-noext").write_str("import \"./lib.mjs\";\nconsole.log(\"esm-entry-ok\");\n").unwrap();
    let out = winterjs().arg("--run").arg(dir.path().join("bin-noext")).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "esm-entry-ok\n");
    // 无 type：同内容走经典，import 即 SyntaxError（exit=1，可读）。
    let dir2 = assert_fs::TempDir::new().unwrap();
    dir2.child("bin-noext").write_str("import \"./lib.mjs\";\n").unwrap();
    let out = winterjs().arg("--run").arg(dir2.path().join("bin-noext")).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    dir.close().unwrap();
    dir2.close().unwrap();
}

#[test]
fn pm_optional_deps_platform_and_tolerance() {
    // 仿 oxlint 形：tool-pkg（bin + 4 个 optional）→ 本平台命中装上、
    // 异平台跳过、packument 404 容忍、tarball 404 安装期容忍（skipped 行）；
    // .bin 链接可用；exit 0。
    use base64::Engine as _;
    use sha2::Digest as _;
    // 当前平台的 npm 名（与 src/pm/platform.rs 转译表同口径）。
    let npm_os = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    };
    let npm_cpu = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        "x86" => "ia32",
        other => other,
    };
    let mk = |pkg: &str| {
        let tgz = make_tgz(&[
            ("package.json", format!(r#"{{"name":"{pkg}","version":"1.0.0"}}"#).as_bytes()),
            ("index.js", b"exports.v = 1;\n"),
        ]);
        let integrity =
            format!("sha512-{}", base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&tgz)));
        (std::sync::Arc::new(tgz), std::sync::Arc::new(integrity))
    };
    let tool_tgz = make_tgz(&[
        ("package.json", br#"{"name":"tool-pkg","version":"1.0.0","main":"index.js","bin":{"tool-bin":"cli.js"}}"#),
        ("index.js", b"exports.v = 1;\n"),
        ("cli.js", b"console.log(\"tool-bin-ok\");\n"),
    ]);
    let tool_int =
        format!("sha512-{}", base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&tool_tgz)));
    let (ok_tgz, ok_int) = mk("tool-bind-ok");
    let (_nope_tgz, nope_int_c) = mk("tool-bind-nope");
    let (_bad_tgz, bad_int_c) = mk("tool-bind-badtar");
    let tool_tgz = std::sync::Arc::new(tool_tgz);
    let tool_int = std::sync::Arc::new(tool_int);
    let port = serve_http(8, move |head, _body| {
        let line = head.lines().next().unwrap_or("").to_owned();
        let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
        let port = head
            .lines()
            .find_map(|l| l.strip_prefix("Host:").or_else(|| l.strip_prefix("host:")))
            .and_then(|v| v.trim().split(':').nth(1))
            .unwrap_or("")
            .to_owned();
        let pack = |name: &str, ver: serde_json::Value| {
            serde_json::json!({ "name": name, "dist-tags": { "latest": "1.0.0" }, "versions": { "1.0.0": ver } })
                .to_string()
        };
        let ver = |tarball: String, integrity: &str, extra: serde_json::Value| {
            let mut v = serde_json::json!({
                "dist": { "tarball": tarball, "integrity": integrity },
                "dependencies": {},
            });
            for (k, val) in extra.as_object().unwrap() {
                v[k] = val.clone();
            }
            v
        };
        let tgz_of = |holder: &std::sync::Arc<Vec<u8>>| {
            (200, vec![("content-type", "application/octet-stream".into())], (**holder).clone())
        };
        if path == "/tool-pkg" {
            let body = pack(
                "tool-pkg",
                ver(
                    format!("http://127.0.0.1:{port}/tool-pkg/-/tool-pkg-1.0.0.tgz"),
                    &tool_int,
                    serde_json::json!({
                        "optionalDependencies": {
                            "tool-bind-ok": "*",
                            "tool-bind-nope": "*",
                            "tool-bind-404": "*",
                            "tool-bind-badtar": "*",
                        },
                    }),
                ),
            );
            return (200, vec![("content-type", "application/json".into())], body.into_bytes());
        }
        if path == "/tool-pkg/-/tool-pkg-1.0.0.tgz" {
            return tgz_of(&tool_tgz);
        }
        if path == "/tool-bind-ok" {
            let body = pack(
                "tool-bind-ok",
                ver(
                    format!("http://127.0.0.1:{port}/tool-bind-ok/-/tool-bind-ok-1.0.0.tgz"),
                    &ok_int,
                    serde_json::json!({ "os": [npm_os], "cpu": [npm_cpu] }),
                ),
            );
            return (200, vec![("content-type", "application/json".into())], body.into_bytes());
        }
        if path == "/tool-bind-ok/-/tool-bind-ok-1.0.0.tgz" {
            return tgz_of(&ok_tgz);
        }
        if path == "/tool-bind-nope" {
            let body = pack(
                "tool-bind-nope",
                ver(
                    format!("http://127.0.0.1:{port}/tool-bind-nope/-/tool-bind-nope-1.0.0.tgz"),
                    &nope_int_c,
                    serde_json::json!({ "os": ["nonexistent-os"] }),
                ),
            );
            return (200, vec![("content-type", "application/json".into())], body.into_bytes());
        }
        if path == "/tool-bind-badtar" {
            // packument 过、tarball 404 → 安装期容忍（skipped 行）。
            let body = pack(
                "tool-bind-badtar",
                ver(
                    format!("http://127.0.0.1:{port}/tool-bind-badtar/-/tool-bind-badtar-1.0.0.tgz"),
                    &bad_int_c,
                    serde_json::json!({}),
                ),
            );
            return (200, vec![("content-type", "application/json".into())], body.into_bytes());
        }
        // tool-bind-nope/-badtar 的 tarball 不应被请求（前者平台跳过）；
        // tool-bind-404 的 packument 直接 404。
        (404, vec![], b"nope".to_vec())
    });
    let reg = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .arg("--add")
        .arg("tool-pkg")
        .arg("--registry")
        .arg(&reg)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("added tool-pkg@1.0.0"), "stdout: {stdout}");
    assert!(stdout.contains("added tool-bind-ok@1.0.0"), "stdout: {stdout}");
    assert!(stdout.contains("skipped optional tool-bind-badtar"), "stdout: {stdout}");
    // 落地断言：命中装上（含 .bin 链）、异平台/404/坏包缺席、lockfile 只记装上的。
    assert!(dir.path().join("node_modules/tool-bind-ok/package.json").is_file());
    assert!(!dir.path().join("node_modules/tool-bind-nope").exists());
    assert!(!dir.path().join("node_modules/tool-bind-404").exists());
    assert!(!dir.path().join("node_modules/tool-bind-badtar").exists());
    assert!(dir.path().join("node_modules/.bin/tool-bin").exists());
    let lock = std::fs::read_to_string(dir.path().join("winterjs-lock.json")).unwrap();
    assert!(lock.contains("tool-bind-ok") && !lock.contains("tool-bind-nope"), "lock: {lock}");
    // 装完即跑（bin 链可用）。
    let out = winterjs()
        .arg("--run")
        .arg(dir.path().join("node_modules/tool-pkg/cli.js"))
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "tool-bin-ok\n");
    dir.close().unwrap();
}

#[test]
fn pm_release_binary_end_to_end() {
    // GitHub release 二进制闭环（stub API + stub tarball，不碰外网）：
    // asset 按平台挑选 → 下载解包 → `.bin/<name>` 可执行 → lockfile 落盘。
    let tool_tgz = {
        let mut tar_buf = Vec::new();
        {
            let mut tar = tar::Builder::new(&mut tar_buf);
            let data = b"#!/bin/sh\necho \"fake-tool ok\"\n";
            let mut hdr = tar::Header::new_gnu();
            hdr.set_size(data.len() as u64);
            hdr.set_mode(0o755);
            hdr.set_cksum();
            tar.append_data(&mut hdr, "mytool-x86_64-unknown-linux-gnu", &data[..]).unwrap();
        }
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        use std::io::Write as _;
        enc.write_all(&tar_buf).unwrap();
        std::sync::Arc::new(enc.finish().unwrap())
    };
    // stub GitHub API：release 表含命中/异平台/异前缀三个 asset。
    let port = serve_http(4, move |head, _body| {
        let line = head.lines().next().unwrap_or("").to_owned();
        let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
        if path == "/repos/o/r/releases/tags/v1" {
            let body = serde_json::json!({
                "assets": [
                    { "name": "other-x86_64-unknown-linux-gnu.tar.gz", "browser_download_url": "http://127.0.0.1:PORT/mytool.tgz" },
                    { "name": "mytool-x86_64-unknown-linux-gnu.tar.gz", "browser_download_url": "http://127.0.0.1:PORT/mytool.tgz" },
                    { "name": "mytool-aarch64-apple-darwin.tar.gz", "browser_download_url": "http://127.0.0.1:PORT/mytool.tgz" },
                ],
            })
            .to_string()
            .replace("PORT", &head.lines().find_map(|l| l.strip_prefix("Host:").or_else(|| l.strip_prefix("host:"))).and_then(|v| v.trim().split(':').nth(1)).unwrap_or("").to_owned());
            return (200, vec![("content-type", "application/json".into())], body.into_bytes());
        }
        if path == "/mytool.tgz" {
            return (200, vec![("content-type", "application/octet-stream".into())], (*tool_tgz).clone());
        }
        (404, vec![], b"nope".to_vec())
    });
    // stub tarball 是 x86_64-linux 形态：只在本机同平台断言二进制内容，
    // 跨平台 CI 上只验 asset 选择（dry-run 打印的 asset 名含本机 arch）。
    let api = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .args(["--add", "mytool@release:github/o/r@v1/mytool", "--dry-run"])
        .env("GITHUB_API", &api)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains(std::env::consts::ARCH),
        "dry-run must pick current-arch asset: {stdout}"
    );
    // 真装：同 asset 下载解包 → `.bin/mytool` 落盘可执行 → lockfile 落盘。
    let out = winterjs()
        .args(["--add", "mytool@release:github/o/r@v1/mytool"])
        .env("GITHUB_API", &api)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("added mytool@release:o/r@v1"), "stdout: {stdout}");
    let bin = dir.path().join("node_modules").join(".bin").join("mytool");
    assert!(bin.is_file(), ".bin/mytool must land");
    let lock = std::fs::read_to_string(dir.path().join("winterjs-lock.json")).unwrap();
    assert!(lock.contains("github-release:o/r@v1/"), "lock: {lock}");
    assert!(lock.contains("sha512-"), "lock integrity: {lock}");
    dir.close().unwrap();
}

#[cfg(unix)]
#[test]
fn pm_release_binary_runs() {
    // 落盘的二进制真可执行（fake sh 脚本回显）。
    let tool_tgz = {
        let mut tar_buf = Vec::new();
        {
            let mut tar = tar::Builder::new(&mut tar_buf);
            let data = b"#!/bin/sh\necho \"fake-tool ok\"\n";
            let mut hdr = tar::Header::new_gnu();
            hdr.set_size(data.len() as u64);
            hdr.set_mode(0o755);
            hdr.set_cksum();
            tar.append_data(&mut hdr, "mytool-x86_64-unknown-linux-gnu", &data[..]).unwrap();
        }
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        use std::io::Write as _;
        enc.write_all(&tar_buf).unwrap();
        std::sync::Arc::new(enc.finish().unwrap())
    };
    // 计数 4：1 API + 1 下载 + reqwest 对 connection:close 的备用连接余量。
    let port = serve_http(4, move |head, _body| {
        let line = head.lines().next().unwrap_or("").to_owned();
        let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
        if path == "/repos/o/r/releases/tags/v1" {
            let host = head.lines().find_map(|l| l.strip_prefix("Host:").or_else(|| l.strip_prefix("host:"))).and_then(|v| v.trim().split(':').nth(1)).unwrap_or("").to_owned();
            let body = serde_json::json!({
                "assets": [
                    { "name": "mytool-x86_64-unknown-linux-gnu.tar.gz", "browser_download_url": format!("http://127.0.0.1:{host}/mytool.tgz") },
                    { "name": "mytool-aarch64-apple-darwin.tar.gz", "browser_download_url": format!("http://127.0.0.1:{host}/mytool.tgz") },
                ],
            })
            .to_string();
            return (200, vec![("content-type", "application/json".into())], body.into_bytes());
        }
        if path == "/mytool.tgz" {
            return (200, vec![("content-type", "application/octet-stream".into())], (*tool_tgz).clone());
        }
        (404, vec![], b"nope".to_vec())
    });
    let api = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    // fake 二进制是 sh 脚本，unix 通用（本用例已 #[cfg(unix)] 门控）。
    let out = winterjs()
        .args(["--add", "mytool@release:github/o/r@v1/mytool"])
        .env("GITHUB_API", &api)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let status = std::process::Command::new(dir.path().join("node_modules").join(".bin").join("mytool"))
        .output()
        .unwrap();
    assert!(status.status.success());
    assert_eq!(String::from_utf8(status.stdout).unwrap(), "fake-tool ok\n");
    dir.close().unwrap();
}

// ── Phase 9a（plan2）：node:events / node:async_hooks / internal 小件 ────────
// 断言口径：Node test/parallel 原文语义按需转写（test-events.js / test-event-emitter*
// / test-async-hooks*），不整目录拉取；消息格式逐字。

#[test]
fn phase9a_events_basic_emit_on_off() {
    // test-events.js: emit 返回值/once/移除后不触发/eventNames
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("e.mjs");
    file.write_str(
        r#"import EE from "node:events";
const ee = new EE();
let calls = [];
function fn1() { calls.push("f1"); }
ee.on("x", fn1);
console.log(ee.emit("x"), ee.emit("nope"));
ee.once("y", () => calls.push("once"));
ee.emit("y"); ee.emit("y");
ee.removeListener("x", fn1);
console.log(calls.join(","), ee.listenerCount("x"), ee.emit("x"));
ee.on("z", () => {});
console.log(ee.eventNames().map(String).join(","));
"#,
    )
    .unwrap();
    let out = winterjs().args(["--run", file.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("true false"), "out: {out}");
    assert!(out.contains("f1,once"), "out: {out}");
    assert!(out.contains("z"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_events_unhandled_error_throws_original() {
    // test-events.js: 无 error 监听时 emit('error', er) 重抛原 Error（非包裹）
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("u.mjs");
    file.write_str(
        r#"import EE from "node:events";
const ee = new EE();
try { ee.emit("error", new TypeError("boom")); } catch (e) {
  console.log(e instanceof TypeError, e.message, "code" in e && e.code === undefined);
}
// 非 Error 实参 → ERR_UNHANDLED_ERROR 包裹
try { ee.emit("error", "str"); } catch (e) {
  console.log(e.code, e.message.startsWith("Unhandled error."));
}
"#,
    )
    .unwrap();
    let out = winterjs().args(["--run", file.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("true boom false"), "out: {out}");
    assert!(out.contains("ERR_UNHANDLED_ERROR true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_events_error_monitor_capture_rejections() {
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("c.mjs");
    file.write_str(
        r#"import EE from "node:events";
// errorMonitor: errorMonitor 监听在无 error 监听时也不抛（先于 doError 判定）
const em = new EE();
let seen = 0;
em.on(EE.errorMonitor, () => seen++);
try { em.emit("error", new Error("no-handler")); } catch {}
console.log("mon", seen);
// captureRejections: rejected listener 走 [captureRejectionSymbol] 而非 error
const cap = new EE({ captureRejections: true });
let handled = 0;
cap.on("x", async () => { throw new Error("rej"); });
cap[EE.captureRejectionSymbol] = (err, type) => { handled++; console.log("cap", type, err.message); };
cap.emit("x");
await new Promise((r) => setTimeout(r, 10));
console.log("handled", handled);
"#,
    )
    .unwrap();
    let out = winterjs().args(["--run", file.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("mon 1"), "out: {out}");
    assert!(out.contains("cap x rej") && out.contains("handled 1"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_events_max_listeners_warning_and_validation() {
    // test-event-emitter-max-listeners.js: 泄漏警告 + warning 事件；参数校验消息逐字
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("m.mjs");
    file.write_str(
        r#"import EE from "node:events";
const warnings = [];
process.on("warning", (w) => warnings.push(w));
const ee = new EE();
for (let i = 0; i < 12; i++) ee.on("l", () => {});
console.log("warned", warnings.length, warnings[0]?.name, warnings[0]?.count);
// Node 原文消息格式（test-events-common 断言口径）
try { ee.once("x", 42); } catch (e) {
  console.log(e.code, e.message);
}
try { EE.setMaxListeners(-1); } catch (e) {
  console.log(e.code, e.constructor.name);
}
"#,
    )
    .unwrap();
    let out = winterjs().args(["--run", file.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("warned 1 MaxListenersExceededWarning 11"), "out: {out}");
    assert!(
        out.contains(
            "ERR_INVALID_ARG_TYPE The \"listener\" argument must be of type function. Received type number (42)"
        ),
        "out: {out}"
    );
    assert!(out.contains("ERR_OUT_OF_RANGE RangeError"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_events_once_and_on_iterator() {
    // test-events-on.js + test-events-on-async-iterator.js 语义子集
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("i.mjs");
    file.write_str(
        r#"import EE, { once, on } from "node:events";
const ee = new EE();
setTimeout(() => ee.emit("tick", 7, "s"), 5);
const [n, s] = await once(ee, "tick");
console.log("once", n, s);
// 异步迭代器 + close 事件
const src = new EE();
setTimeout(() => { src.emit("data", "a"); src.emit("data", "b"); src.emit("end"); }, 5);
const got = [];
for await (const [v] of on(src, "data", { close: ["end"] })) got.push(v);
console.log("iter", got.join(""));
// once + AbortSignal（已中止即 AbortError）
try {
  await once(new EE(), "x", { signal: AbortSignal.abort(new Error("why")) });
} catch (e) { console.log("abort", e.code, e.cause?.message); }
"#,
    )
    .unwrap();
    let out = winterjs().args(["--run", file.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("once 7 s") && out.contains("iter ab"), "out: {out}");
    assert!(out.contains("abort ABORT_ERR why"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_events_require_and_error_boundary() {
    // require('node:events') CJS 面 + 非法 emitter 报可读错（边界三件之一）
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("r.cjs");
    file.write_str(
        r#"const { EventEmitter, getEventListeners } = require("node:events");
const ee = new EventEmitter();
ee.on("a", () => 1);
console.log(require("node:events").EventEmitter === EventEmitter, getEventListeners(ee, "a").length);
try { getEventListeners(42, "a"); } catch (e) { console.log(e.code); }
"#,
    )
    .unwrap();
    let out = winterjs().args(["--run", file.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("true 1") && out.contains("ERR_INVALID_ARG_TYPE"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_async_hooks_als_and_async_resource() {
    // test-async-local-storage* 子集（同步链路）+ AsyncResource runInAsyncScope
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("a.mjs");
    file.write_str(
        r#"import { AsyncLocalStorage, AsyncResource, createHook, executionAsyncId } from "node:async_hooks";
const als = new AsyncLocalStorage();
als.run({ id: 42 }, () => {
  console.log("store", als.getStore().id);
  const res = new AsyncResource("TEST");
  res.runInAsyncScope(() => console.log("in-res", als.getStore().id, executionAsyncId() > 1));
  console.log("bind", als.bind(() => als.getStore()?.id ?? "none")(), als.getStore()?.id ?? "none");
});
console.log("outside", als.getStore());
// snapshot
const snap = als.run({ s: 1 }, () => als.snapshot());
snap(() => console.log("snapshot", als.getStore()?.s));
// AsyncResource.bind 静态 + emitDestroy
const bound = AsyncResource.bind(() => executionAsyncId() > 1, "BOUND");
console.log("static-bind", bound(), AsyncResource.AsyncResource === AsyncResource);
const hook = createHook({ init() {} }).enable();
console.log("hook", typeof hook.disable);
"#,
    )
    .unwrap();
    let out = winterjs().args(["--run", file.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("store 42") && out.contains("in-res 42 true"), "out: {out}");
    assert!(out.contains("bind 42 42") && out.contains("outside undefined"), "out: {out}");
    assert!(out.contains("snapshot 1") && out.contains("static-bind true true"), "out: {out}");
    assert!(out.contains("hook function"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_async_hooks_stub_and_validation_boundary() {
    // stub 口径边界：createHook 非法回调 → ERR_ASYNC_CALLBACK；ALS 非法 callback → TypeError
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("b.mjs");
    file.write_str(
        r#"import { createHook, AsyncResource, AsyncLocalStorage } from "node:async_hooks";
try { createHook({ init: 1 }); } catch (e) { console.log("h", e.code); }
try { new AsyncResource(42); } catch (e) { console.log("t", e.code, e.message.includes("must be of type string")); }
try { new AsyncLocalStorage().run({}, "nope"); } catch (e) { console.log("r", e instanceof TypeError); }
try { new AsyncResource("X", { triggerAsyncId: "no" }); } catch (e) { console.log("o", e.code); }
"#,
    )
    .unwrap();
    let out = winterjs().args(["--run", file.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("h ERR_ASYNC_CALLBACK"), "out: {out}");
    assert!(out.contains("t ERR_INVALID_ARG_TYPE true"), "out: {out}");
    assert!(out.contains("r true") && out.contains("o ERR_INVALID_ARG_TYPE"), "out: {out}");
    dir.close().unwrap();
}

// ── Phase 9a-2：node:util / node:util/types ────────────────────────────────

#[test]
fn phase9a_util_format_inspect_inherits() {
    // test-util-format.js / test-util-inspect.js 命名子集
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("u.mjs");
    file.write_str(
        r#"import util, { format, inspect, inherits, stripVTControlCharacters } from "node:util";
console.log(format("%s=%d", "a", 1.5), format("%%"), format("%j", { x: 1 }));
console.log(format("%i:%f", 3.9, "1.5"));
console.log(util.inspect({ a: 1, b: [1, 2], c: new Map([["k", 1]]) }));
console.log(util.inspect("it's"), inspect(new Date(0).toISOString() === inspect(new Date(0)) ? { s: "q'a" } : 0));
class Base { base() { return 1; } }
class Sub {}
inherits(Sub, Base);
console.log("inh", new Sub().base(), Sub.super_ === Base);
console.log("strip", stripVTControlCharacters("\u001b[31mred\u001b[39m"), stripVTControlCharacters("plain"));
console.log("promise-inp", util.inspect(Promise.resolve()));
"#,
    )
    .unwrap();
    let out = winterjs().args(["--run", file.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("a=1.5 %% {\"x\":1}"), "out: {out}");
    assert!(out.contains("3:1"), "out: {out}");
    assert!(out.contains("{ a: 1, b: [ 1, 2 ], c: Map(1) { 'k' => 1 } }"), "out: {out}");
    assert!(out.contains("inh 1 true"), "out: {out}");
    assert!(out.contains("strip red plain"), "out: {out}");
    assert!(out.contains("Promise { <pending> }"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_util_promisify_callbackify_deep_equal() {
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("p.mjs");
    file.write_str(
        r#"import { promisify, callbackify, isDeepStrictEqual } from "node:util";
import assert from "node:assert";
const pb = promisify((a, cb) => cb(null, a * 2));
console.log("prom", await pb(21));
// multi-value + customPromisifyArgs
const pr = promisify((cb) => cb(null, "bytesRead", "buffer"));
pr[Symbol.for("nodejs.util.promisify.custom")] // 不设 custom
console.log("prom-obj", JSON.stringify(await (async () => {
  const fn = promisify((cb) => cb(null, 1, 2));
  fn[Symbol("customPromisifyArgs")] = ["a", "b"];
  const fn2 = promisify((cb) => { fn2args(cb); }) ; return 0;
})()));
// promisify.custom 通道
const raw = (a, cb) => cb(null, a);
raw[Symbol.for("nodejs.util.promisify.custom")] = (a) => Promise.resolve(a + 100);
console.log("custom", await promisify(raw)(1));
// callbackify 正常 + falsy rejection
const cf = callbackify(async (n) => n + 1);
const [err, val] = await new Promise((res) => cf(1, (e, v) => res([e, v])));
console.log("cb", err, val);
const cf2 = callbackify(async () => { throw null; });
await new Promise((res) => cf2((e) => res(console.log("falsy", e?.code, e instanceof Error))));
// isDeepStrictEqual 语义（test-assert 依赖同款）
console.log("eq", isDeepStrictEqual({ a: [1, { b: 2 }] }, { a: [1, { b: 2 }] }));
console.log("neq0", isDeepStrictEqual(1, "1"), isDeepStrictEqual({ a: 1 }, { a: 1, b: undefined }));
console.log("nan0", isDeepStrictEqual(NaN, NaN), isDeepStrictEqual(0, -0));
console.log("map", isDeepStrictEqual(new Map([[1, "a"]]), new Map([[1, "a"]])));
console.log("proto", isDeepStrictEqual(Object.create(null, { x: { value: 1, enumerable: true } }), { x: 1 }));
// 边界：非函数入参
try { promisify(42); } catch (e) { console.log("err1", e.code, e.message.includes("must be of type function")); }
try { callbackify("x"); } catch (e) { console.log("err2", e.code); }
assert.strictEqual(typeof promisify.custom, "symbol");
"#,
    )
    .unwrap();
    let out = winterjs().args(["--run", file.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("prom 42"), "out: {out}");
    assert!(out.contains("custom 101"), "out: {out}");
    assert!(out.contains("cb null 2"), "out: {out}");
    assert!(out.contains("falsy ERR_FALSY_VALUE_REJECTION true"), "out: {out}");
    assert!(out.contains("eq true"), "out: {out}");
    assert!(out.contains("neq0 false false"), "out: {out}");
    assert!(out.contains("nan0 true false"), "out: {out}");
    assert!(out.contains("map true") && out.contains("proto false"), "out: {out}");
    assert!(out.contains("err1 ERR_INVALID_ARG_TYPE true"), "out: {out}");
    assert!(out.contains("err2 ERR_INVALID_ARG_TYPE"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_util_types_surface() {
    // test-util-types* 命名子集（isProxy 恒 false 为记档偏差，不点名）
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("t.mjs");
    file.write_str(
        r#"import types from "node:util/types";
import { types as t2 } from "node:util";
console.log("same", types === t2, typeof types.isPromise);
console.log(
  types.isPromise(Promise.resolve()),
  types.isMap(new Map()),
  types.isSet(new Set()),
  types.isDate(new Date()),
  types.isRegExp(/r/),
  types.isTypedArray(new Uint8Array(1)),
  types.isUint8Array(new Uint8Array(1)),
  types.isUint32Array(new Uint32Array(1)),
  types.isDataView(new DataView(new ArrayBuffer(2))),
  types.isArrayBuffer(new ArrayBuffer(1)),
  types.isNativeError(new TypeError()),
  types.isNativeError(new Error()),
  types.isNativeError({}),
  types.isAsyncFunction(async () => {}),
  types.isGeneratorFunction(function* () {}),
  types.isPromise(new Map()),
  types.isWeakSet(new WeakSet()),
  types.isNumberObject(new Number(1)),
  types.isBoxedPrimitive(new Boolean(true)),
  types.isArgumentsObject((function () { return arguments; })()),
);
try { types.isUint8Array(42) === false; console.log("num-ok"); } catch (e) { console.log("num-throw"); }
"#,
    )
    .unwrap();
    let out = winterjs().args(["--run", file.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("same true function"), "out: {out}");
    assert!(
        out.contains("true true true true true true true true true true true true false true true false true true true true"),
        "out: {out}"
    );
    assert!(out.contains("num-ok"), "out: {out}");
    dir.close().unwrap();
}

// ── Phase 9a-3：node:querystring / node:punycode / node:string_decoder ─────

#[test]
fn phase9a_querystring_roundtrip() {
    // test-querystring.js 命名子集
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("q.mjs");
    file.write_str(
        r#"import qs from "node:querystring";
console.log(JSON.stringify(qs.parse("a=1&b=x%20y&b=2&c")));
console.log(qs.stringify({ a: "x y", b: [1, 2] }));
console.log(qs.escape("ä b"), qs.unescape("%C3%A4+b"));
console.log(JSON.stringify(qs.parse("a=1;a=2", ";", "=")));
console.log(Object.keys(qs.parse("a=1&b=2&c=3", null, null, { maxKeys: 2 })).length);
// 自定义 enc/dec
const p = qs.parse("a=%20", null, null, { decodeURIComponent: (s) => s });
console.log(JSON.stringify(p));
// 边界：非字符串入参 → 空对象；maxKeys=1 截断
console.log(JSON.stringify(qs.parse(null)), JSON.stringify(qs.parse("")));
console.log(typeof qs.parse("a=1&b=2", null, null, { maxKeys: 1 }).a);
console.log("fallback", qs.unescape("%E0%A4%A").includes("%"), qs.unescapeBuffer("a+b", true).toString());
"#,
    )
    .unwrap();
    let out = winterjs().args(["--run", file.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains(r#"{"a":"1","b":["x y","2"],"c":""}"#), "out: {out}");
    assert!(out.contains("a=x%20y&b=1&b=2"), "out: {out}");
    assert!(out.contains("%C3%A4%20b ä+b"), "out: {out}");
    assert!(out.contains(r#"{"a":["1","2"]}"#), "out: {out}");
    assert!(out.contains("2"), "out: {out}");
    assert!(out.contains(r#"{"a":"%20"}"#), "out: {out}");
    assert!(out.contains("{} {}"), "out: {out}");
    assert!(out.contains("fallback true a b"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_punycode_rfc3492() {
    // test-punycode.js 命名子集（RFC 3492 向量 + 域名 + ucs2）
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("p.mjs");
    file.write_str(
        r#"import punycode from "node:punycode";
console.log(punycode.encode("bücher"), punycode.decode("bcher-kva"));
console.log(punycode.toASCII("münchen.de"), punycode.toUnicode("xn--mnchen-3ya.de"));
console.log(punycode.toASCII("日本"), punycode.toUnicode("xn--wgv71a"));
console.log(punycode.ucs2.encode([0x1D306]) === "\u{1D306}", punycode.ucs2.decode("a\u{1D306}b").length);
console.log(punycode.toASCII("foo@bücher.de").split("@")[1]);
try { punycode.decode("!!!!!"); } catch (e) { console.log("err", e instanceof RangeError); }
"#,
    )
    .unwrap();
    let out = winterjs().args(["--run", file.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("bcher-kva bücher"), "out: {out}");
    assert!(out.contains("xn--mnchen-3ya.de münchen.de"), "out: {out}");
    assert!(out.contains("xn--wgv71a 日本"), "out: {out}");
    assert!(out.contains("true 3"), "out: {out}");
    assert!(out.contains("xn--bcher-kva.de"), "out: {out}");
    assert!(out.contains("err true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_string_decoder_encodings() {
    // test-string-decoder.js 命名子集：截断续读/end flush/全编码
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("s.mjs");
    file.write_str(
        r#"import { StringDecoder } from "node:string_decoder";
const utf8 = new StringDecoder("utf8");
let out = "";
out += utf8.write(Buffer.from([0xE4, 0xB8]));
out += utf8.write(Buffer.from([0xAD, "e".charCodeAt(0)]));
out += utf8.end();
console.log("utf8", out);
// invalid 序列 → FFFD 继续
const bad = new StringDecoder("utf8");
console.log("bad", bad.write(Buffer.from([0xFF, 0x41])).includes("\uFFFD"), bad.write(Buffer.from([0x42])));
const u16 = new StringDecoder("utf16le");
let o2 = u16.write(Buffer.from([0x61, 0]));
o2 += u16.write(Buffer.from([0x62]));
o2 += u16.end();
console.log("utf16", o2.length, o2.charCodeAt(1) === 0xFFFD);
const hex = new StringDecoder("hex");
console.log("hex", hex.write(Buffer.from([0xDE, 0xAD])), hex.end());
const b64 = new StringDecoder("base64");
let o3 = b64.write(Buffer.from("foobarb"));
o3 += b64.write(Buffer.from("az"));
o3 += b64.end();
console.log("b64", o3 === Buffer.from("foobarbaz").toString("base64"));
const latin = new StringDecoder("latin1");
console.log("latin1", latin.write(Buffer.from([0xE9, 0x41])), latin.end().length);
const ascii = new StringDecoder("ascii");
console.log("ascii", ascii.write(Buffer.from([0x80, 0x41])).length, ascii.write(Buffer.from([0x41])));
console.log("default", new StringDecoder().encoding, typeof utf8.lastChar, utf8.lastNeed === 0);
console.log("string-in", new StringDecoder().write("direct"));
try { new StringDecoder("nope"); } catch (e) { console.log("e1", e.code); }
try { new StringDecoder("utf8").write(42); } catch (e) { console.log("e2", e.code); }
try { new StringDecoder("utf8").write.call({ __wjsId: undefined }, Buffer.alloc(1)); } catch (e) { console.log("e3", e.code); }
"#,
    )
    .unwrap();
    let out = winterjs().args(["--run", file.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("utf8 中e"), "out: {out}");
    assert!(out.contains("bad true B"), "out: {out}");
    assert!(out.contains("utf16 2 true"), "out: {out}");
    assert!(out.contains("hex dead"), "out: {out}");
    assert!(out.contains("b64 true"), "out: {out}");
    assert!(out.contains("latin1 éA 0"), "out: {out}");
    assert!(out.contains("ascii 2 A"), "out: {out}");
    assert!(out.contains("default utf8 object true"), "out: {out}");
    assert!(out.contains("string-in direct"), "out: {out}");
    assert!(out.contains("e1 ERR_UNKNOWN_ENCODING") && out.contains("e2 ERR_INVALID_ARG_TYPE"), "out: {out}");
    dir.close().unwrap();
}

// ── Phase 9a-4：node:diagnostics_channel / node:trace_events / node:tty ────

#[test]
fn phase9a_diagnostics_channel_surface() {
    // test-diagnostics-channel.js 命名子集
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("d.mjs");
    file.write_str(
        r#"import dc, { channel, hasSubscribers, Channel } from "node:diagnostics_channel";
import { AsyncLocalStorage } from "node:async_hooks";
console.log("idle", hasSubscribers("t-ch"), channel("t-ch").hasSubscribers);
const seen = [];
const listener = (msg, name) => seen.push([name, msg]);
dc.subscribe("t-ch", listener);
console.log("active", hasSubscribers("t-ch"), channel("t-ch") instanceof Channel);
channel("t-ch").publish({ n: 1 });
console.log("got", JSON.stringify(seen));
console.log("unsub-wrong", dc.unsubscribe("t-ch", () => {}), dc.unsubscribe("t-ch", listener), hasSubscribers("t-ch"));
// bindStore + runStores
const als = new AsyncLocalStorage();
const ch = channel("s-ch");
ch.bindStore(als, (d) => ({ w: d }));
ch.runStores(7, () => console.log("store", als.getStore()?.w));
console.log("outside", als.getStore());
console.log("unbound", ch.unbindStore(als), ch.hasSubscribers);
// TracingChannel 全窗口
const { tracingChannel } = dc;
const tc = tracingChannel("tr-x");
const ev = [];
tc.subscribe({ start: () => ev.push("s"), end: (c) => ev.push(`e:${c.result}`), error: () => ev.push("err") });
console.log("sync", tc.traceSync(() => "R"), ev.join(","));
tc.tracePromise(async () => "P").then((v) => console.log("promise", v));
// 边界
try { channel(42); } catch (e) { console.log("e1", e.code); }
try { dc.subscribe("t-ch2", "nope"); } catch (e) { console.log("e2", e.message.includes("must be of type function")); }
"#,
    )
    .unwrap();
    let out = winterjs().args(["--run", file.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("idle false false") && out.contains("active true true"), "out: {out}");
    assert!(out.contains(r#"got [["t-ch",{"n":1}]]"#), "out: {out}");
    assert!(out.contains("unsub-wrong false true false"), "out: {out}");
    assert!(out.contains("store 7") && out.contains("outside undefined"), "out: {out}");
    assert!(out.contains("unbound true false"), "out: {out}");
    assert!(out.contains("sync R s,e:R"), "out: {out}");
    assert!(out.contains("promise P"), "out: {out}");
    assert!(out.contains("e1 ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("e2 true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_trace_events_categories() {
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("t.mjs");
    file.write_str(
        r#"import { createTracing, getEnabledCategories } from "node:trace_events";
const t = createTracing({ categories: ["node", "v8"] });
console.log("idle", t.enabled, t.categories, JSON.stringify(getEnabledCategories()));
t.enable();
console.log("on", t.enabled, getEnabledCategories());
t.disable();
console.log("off", t.enabled, JSON.stringify(getEnabledCategories()));
const t2 = createTracing({ categories: ["metro"] });
t2.enable();
console.log("multi", getEnabledCategories());
try { createTracing({ categories: [] }); } catch (e) { console.log("e1", e.code); }
try { createTracing({}); } catch (e) { console.log("e2", e.code); }
try { createTracing({ categories: [42] }); } catch (e) { console.log("e3", e.code); }
"#,
    )
    .unwrap();
    let out = winterjs().args(["--run", file.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("idle false node,v8 \"\""), "out: {out}");
    assert!(out.contains("on true node,v8"), "out: {out}");
    assert!(out.contains("off false \"\""), "out: {out}");
    assert!(out.contains("multi metro"), "out: {out}");
    assert!(out.contains("e1 ERR_TRACE_EVENTS_CATEGORY_REQUIRED"), "out: {out}");
    assert!(out.contains("e2 ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("e3 ERR_INVALID_ARG_TYPE"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_tty_thin_surface() {
    // test-tty* 命名子集（薄面；raw 为标志位跟踪，记档偏差）
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("y.mjs");
    file.write_str(
        r#"import tty, { isatty, ReadStream, WriteStream, getColorDepth } from "node:tty";
console.log("isatty", isatty(1), isatty(0), isatty(-1), isatty(1.5), isatty("1"));
const ws = new WriteStream(1);
console.log("ws", ws.fd, typeof ws.write, ws.columns > 0, ws.rows > 0);
const rs = new ReadStream(0);
console.log("rs", rs.isRaw, rs.setRawMode(true).isRaw, rs.setRawMode("raw").rawMode, rs.setRawMode(false).isRaw);
console.log("static", ReadStream.isatty === isatty, WriteStream.isatty(2));
console.log("depth", [1, 8, 24].includes(getColorDepth()), getColorDepth({ isTTY: false }) === 1);
try { new WriteStream(-5); } catch (e) { console.log("e1", e.code, e.constructor.name); }
try { rs.setRawMode("bogus"); } catch (e) { console.log("e2", e.code); }
"#,
    )
    .unwrap();
    let out = winterjs().args(["--run", file.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("isatty false"), "out: {out}");
    assert!(out.contains("ws 1 function true true"), "out: {out}");
    assert!(out.contains("rs false true raw false"), "out: {out}");
    assert!(out.contains("static true"), "out: {out}");
    assert!(out.contains("depth true true"), "out: {out}");
    assert!(out.contains("e1 ERR_INVALID_FD RangeError"), "out: {out}");
    assert!(out.contains("e2 ERR_INVALID_ARG_VALUE"), "out: {out}");
    dir.close().unwrap();
}
