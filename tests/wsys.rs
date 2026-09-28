//! 本体第二批黑盒（WinterJS.shell/hex/time/retry/graph/git/oauth/transpile/log/mime/cookie/httpdate）。

mod common;

use common::*;

fn eval_ok(code: &str) -> String {
    let out = winterjs().args(["--eval", code]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn wsys_shell_hex_faces() {
    let stdout = eval_ok(
        r#"console.log("expand", WinterJS.shell.expand("$HOME") !== "$HOME"); console.log("hex", WinterJS.hex.encode("hi"), Array.from(WinterJS.hex.decode("6869")).join(","));"#,
    );
    assert!(stdout.contains("expand true"), "out: {stdout}");
    assert!(stdout.contains("hex 6869 104,105"), "out: {stdout}");
}

#[test]
fn wsys_time_retry_faces() {
    let stdout = eval_ok(
        r#"const t0 = WinterJS.time.now(); console.log("now", t0 > 1700000000000); console.log("parse", WinterJS.time.parse("2026-01-02T03:04:05Z") === Date.parse("2026-01-02T03:04:05Z")); console.log("fmt", WinterJS.time.format(0, "%Y-%m-%d") === "1970-01-01"); console.log("delay", WinterJS.retry.delay("constant", 2, { minMs: 100, maxMs: 1000 }) === 100, WinterJS.retry.delay("exponential", 0, { minMs: 100, maxMs: 5000 }) === 100); const r = await WinterJS.retry.run(async (a) => { if (a < 2) throw new Error("x"); return "ok"; }, { attempts: 3, minMs: 1, maxMs: 2 }); console.log("run", r.value === "ok" && r.attempts === 3);"#,
    );
    assert!(stdout.contains("now true"), "out: {stdout}");
    assert!(stdout.contains("parse true"), "out: {stdout}");
    assert!(stdout.contains("fmt true"), "out: {stdout}");
    assert!(stdout.contains("delay true true"), "out: {stdout}");
    assert!(stdout.contains("run true"), "out: {stdout}");
}

#[test]
fn wsys_graph_faces() {
    let stdout = eval_ok(
        r#"const g = WinterJS.graph.create("directed"); const a = WinterJS.graph.addNode(g, "a"); const b = WinterJS.graph.addNode(g, "b"); WinterJS.graph.addEdge(g, a, b); console.log("topo", JSON.stringify(WinterJS.graph.toposort(g)) === JSON.stringify([a, b])); console.log("counts", JSON.stringify(WinterJS.graph.counts(g)) === "[2,1]"); WinterJS.graph.free(g);"#,
    );
    assert!(stdout.contains("topo true"), "out: {stdout}");
    assert!(stdout.contains("counts true"), "out: {stdout}");
}

#[test]
fn wsys_git_faces() {
    // 自家仓库即现成 fixture（只读 rev 走 check_read）。
    let dir = std::env::current_dir().unwrap();
    let out = winterjs()
        .args(["--eval", "console.log('sha', /^[0-9a-f]{40}$/.test(WinterJS.git.revParse('.', 'HEAD')));"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("sha true"), "out: {stdout}");
    let out = winterjs()
        .args(["--eval", "const l = WinterJS.git.log('.', 'HEAD', 2); console.log('log', Array.isArray(l) && l.length > 0 && typeof l[0].sha === 'string' && typeof l[0].title === 'string');"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("log true"), "out: {stdout}");
}

#[test]
fn wsys_oauth_transpile_faces() {
    let stdout = eval_ok(
        r#"const a = WinterJS.oauth.authorizeUrl({ authUrl: "https://ex.com/auth", clientId: "c", redirectUri: "https://app/cb", scope: "read", state: "s" }); console.log("auth", a.url.includes("response_type=code") && a.state === "s"); const p = WinterJS.oauth.pkce(); console.log("pkce", typeof p.challenge === "string" && typeof p.verifier === "string"); console.log("ts", WinterJS.transpile("const x: number = 1; export default x;").includes("const x = 1"));"#,
    );
    assert!(stdout.contains("auth true"), "out: {stdout}");
    assert!(stdout.contains("pkce true"), "out: {stdout}");
    assert!(stdout.contains("ts true"), "out: {stdout}");
}

#[test]
fn wsys_misc2_faces() {
    let stdout = eval_ok(
        r#"WinterJS.log.info("wsys-probe"); console.log("mime", WinterJS.mime.lookup("a.png") === "image/png"); console.log("cookie", JSON.stringify(WinterJS.cookie.parse("a=1; Path=/")) === '{"name":"a","value":"1"}'); console.log("ser", WinterJS.cookie.serialize("a", "1", { path: "/", httpOnly: true }) === "a=1; HttpOnly; Path=/"); const ms = WinterJS.httpdate.parse("Sun, 06 Nov 1994 08:49:37 GMT"); console.log("date", WinterJS.httpdate.format(ms) === "Sun, 06 Nov 1994 08:49:37 GMT");"#,
    );
    assert!(stdout.contains("mime true"), "out: {stdout}");
    assert!(stdout.contains("cookie true"), "out: {stdout}");
    assert!(stdout.contains("ser true"), "out: {stdout}");
    assert!(stdout.contains("date true"), "out: {stdout}");
}

#[test]
fn wsys_errors_boundary() {
    let stdout = eval_ok(
        r#"const t = (n, f) => { try { f(); console.log(n, "NO-THROW"); } catch (e) { console.log(n, "THROW", e.constructor.name); } }; t("hex", () => WinterJS.hex.decode("zz")); t("time", () => WinterJS.time.parse("nope")); t("tz", () => WinterJS.time.format(0, "%Y", "Mars/Olympus")); t("kind", () => WinterJS.retry.delay("nope", 0, {})); t("cycle", () => { const g = WinterJS.graph.create("directed"); const a = WinterJS.graph.addNode(g, "a"); WinterJS.graph.addEdge(g, a, a); WinterJS.graph.toposort(g); }); t("git", () => WinterJS.git.revParse("/nonexistent-dir-xyz", "HEAD")); t("log", () => WinterJS.log("verbose", "x")); t("cookie", () => WinterJS.cookie.serialize("bad name", "1"));"#,
    );
    for name in ["hex", "time", "tz", "kind", "cycle", "git", "log", "cookie"] {
        assert!(stdout.contains(&format!("{name} THROW TypeError")), "out: {stdout}");
    }
}
