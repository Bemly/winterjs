//! 本体小工具黑盒（WinterJS2.semver/yaml/jsonc/ip/shlex/spdx/qrcode）。
//! 正常 + 报错 + 边界三件。

mod common;

use common::*;

fn eval_ok(code: &str) -> String {
    let out = winterjs2().args(["--eval", code]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn wstd_semver_faces() {
    let stdout = eval_ok(
        r#"console.log("valid", WinterJS2.semver.valid("1.2.3"), WinterJS2.semver.valid("nope")); console.log("parse", JSON.stringify(WinterJS2.semver.parse("1.2.3-beta.1+build"))); console.log("sat", WinterJS2.semver.satisfies("1.2.3", "^1.0.0"), WinterJS2.semver.satisfies("2.0.0", "^1.0.0")); console.log("cmp", WinterJS2.semver.compare("1.2.3", "1.2.4"), WinterJS2.semver.compare("1.0.0", "1.0.0-alpha"), WinterJS2.semver.compare("2.0.0", "2.0.0"));"#,
    );
    assert!(stdout.contains("valid true false"), "out: {stdout}");
    assert!(stdout.contains(r#""major":1,"minor":2,"patch":3"#), "out: {stdout}");
    assert!(stdout.contains(r#""pre":["beta","1"]"#), "out: {stdout}");
    assert!(stdout.contains("sat true false"), "out: {stdout}");
    assert!(stdout.contains("cmp -1 1 0"), "out: {stdout}");
}

#[test]
fn wstd_yaml_jsonc_faces() {
    let stdout = eval_ok(
        r#"console.log("yaml", JSON.stringify(WinterJS2.yaml.parse("a: 1\nb: [x, true]\n"))); console.log("yamlstr", WinterJS2.yaml.stringify({a: 1}).includes("a: 1")); console.log("jsonc", JSON.stringify(WinterJS2.jsonc.parse("{ \"a\": 1, // c\n }"))); console.log("empty", JSON.stringify(WinterJS2.yaml.parse("")));"#,
    );
    assert!(stdout.contains(r#"yaml {"a":1,"b":["x",true]}"#), "out: {stdout}");
    assert!(stdout.contains("yamlstr true"), "out: {stdout}");
    assert!(stdout.contains(r#"jsonc {"a":1}"#), "out: {stdout}");
    assert!(stdout.contains("empty null"), "out: {stdout}");
}

#[test]
fn wstd_ip_faces() {
    let stdout = eval_ok(
        r#"console.log("net", WinterJS2.ip.isNet("10.0.0.0/8"), WinterJS2.ip.isNet("nope")); console.log("addr", WinterJS2.ip.isAddr("::1"), WinterJS2.ip.isAddr("999.1.1.1")); console.log("contains", WinterJS2.ip.contains("10.0.0.0/8", "10.1.2.3"), WinterJS2.ip.contains("10.0.0.0/8", "11.0.0.1")); console.log("parse", JSON.stringify(WinterJS2.ip.parse("192.168.1.0/24")));"#,
    );
    assert!(stdout.contains("net true false"), "out: {stdout}");
    assert!(stdout.contains("addr true false"), "out: {stdout}");
    assert!(stdout.contains("contains true false"), "out: {stdout}");
    assert!(stdout.contains(r#""network":"192.168.1.0","prefixLen":24"#), "out: {stdout}");
}

#[test]
fn wstd_misc_faces() {
    let stdout = eval_ok(
        r#"console.log("shlex", JSON.stringify(WinterJS2.shlex.split("a 'b c' d"))); console.log("spdx", WinterJS2.spdx.valid("MIT OR Apache-2.0"), WinterJS2.spdx.valid("nope-not-a-license")); const q = WinterJS2.qrcode("hi"); console.log("qr", typeof q === "string" && q.length > 0);"#,
    );
    assert!(stdout.contains(r#"shlex ["a","b c","d"]"#), "out: {stdout}");
    assert!(stdout.contains("spdx true false"), "out: {stdout}");
    assert!(stdout.contains("qr true"), "out: {stdout}");
}

#[test]
fn wstd_errors_boundary() {
    let stdout = eval_ok(
        r#"const t = (n, f) => { try { f(); console.log(n, "NO-THROW"); } catch (e) { console.log(n, "THROW", e.constructor.name); } }; t("semver", () => WinterJS2.semver.parse("nope")); t("range", () => WinterJS2.semver.satisfies("1.0.0", "latest")); t("yaml", () => WinterJS2.yaml.parse("a: [1,\n")); t("jsonc", () => WinterJS2.jsonc.parse("{bad")); t("cidr", () => WinterJS2.ip.contains("nope", "1.1.1.1")); t("shlex", () => WinterJS2.shlex.split("a 'b")); t("qr", () => WinterJS2.qrcode("")); t("num", () => WinterJS2.semver.valid(42));"#,
    );
    for name in ["semver", "range", "yaml", "jsonc", "cidr", "shlex", "qr", "num"] {
        assert!(stdout.contains(&format!("{name} THROW TypeError")), "out: {stdout}");
    }
}
