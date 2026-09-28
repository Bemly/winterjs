//! 本体小工具黑盒（WinterJS.semver/yaml/jsonc/ip/shlex/spdx/qrcode）。
//! 正常 + 报错 + 边界三件。

mod common;

use common::*;

fn eval_ok(code: &str) -> String {
    let out = winterjs().args(["--eval", code]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn wstd_semver_faces() {
    let stdout = eval_ok(
        r#"console.log("valid", WinterJS.semver.valid("1.2.3"), WinterJS.semver.valid("nope")); console.log("parse", JSON.stringify(WinterJS.semver.parse("1.2.3-beta.1+build"))); console.log("sat", WinterJS.semver.satisfies("1.2.3", "^1.0.0"), WinterJS.semver.satisfies("2.0.0", "^1.0.0")); console.log("cmp", WinterJS.semver.compare("1.2.3", "1.2.4"), WinterJS.semver.compare("1.0.0", "1.0.0-alpha"), WinterJS.semver.compare("2.0.0", "2.0.0"));"#,
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
        r#"console.log("yaml", JSON.stringify(WinterJS.yaml.parse("a: 1\nb: [x, true]\n"))); console.log("yamlstr", WinterJS.yaml.stringify({a: 1}).includes("a: 1")); console.log("jsonc", JSON.stringify(WinterJS.jsonc.parse("{ \"a\": 1, // c\n }"))); console.log("empty", JSON.stringify(WinterJS.yaml.parse("")));"#,
    );
    assert!(stdout.contains(r#"yaml {"a":1,"b":["x",true]}"#), "out: {stdout}");
    assert!(stdout.contains("yamlstr true"), "out: {stdout}");
    assert!(stdout.contains(r#"jsonc {"a":1}"#), "out: {stdout}");
    assert!(stdout.contains("empty null"), "out: {stdout}");
}

#[test]
fn wstd_ip_faces() {
    let stdout = eval_ok(
        r#"console.log("net", WinterJS.ip.isNet("10.0.0.0/8"), WinterJS.ip.isNet("nope")); console.log("addr", WinterJS.ip.isAddr("::1"), WinterJS.ip.isAddr("999.1.1.1")); console.log("contains", WinterJS.ip.contains("10.0.0.0/8", "10.1.2.3"), WinterJS.ip.contains("10.0.0.0/8", "11.0.0.1")); console.log("parse", JSON.stringify(WinterJS.ip.parse("192.168.1.0/24")));"#,
    );
    assert!(stdout.contains("net true false"), "out: {stdout}");
    assert!(stdout.contains("addr true false"), "out: {stdout}");
    assert!(stdout.contains("contains true false"), "out: {stdout}");
    assert!(stdout.contains(r#""network":"192.168.1.0","prefixLen":24"#), "out: {stdout}");
}

#[test]
fn wstd_misc_faces() {
    let stdout = eval_ok(
        r#"console.log("shlex", JSON.stringify(WinterJS.shlex.split("a 'b c' d"))); console.log("spdx", WinterJS.spdx.valid("MIT OR Apache-2.0"), WinterJS.spdx.valid("nope-not-a-license")); const q = WinterJS.qrcode("hi"); console.log("qr", typeof q === "string" && q.length > 0);"#,
    );
    assert!(stdout.contains(r#"shlex ["a","b c","d"]"#), "out: {stdout}");
    assert!(stdout.contains("spdx true false"), "out: {stdout}");
    assert!(stdout.contains("qr true"), "out: {stdout}");
}

#[test]
fn wstd_errors_boundary() {
    let stdout = eval_ok(
        r#"const t = (n, f) => { try { f(); console.log(n, "NO-THROW"); } catch (e) { console.log(n, "THROW", e.constructor.name); } }; t("semver", () => WinterJS.semver.parse("nope")); t("range", () => WinterJS.semver.satisfies("1.0.0", "latest")); t("yaml", () => WinterJS.yaml.parse("a: [1,\n")); t("jsonc", () => WinterJS.jsonc.parse("{bad")); t("cidr", () => WinterJS.ip.contains("nope", "1.1.1.1")); t("shlex", () => WinterJS.shlex.split("a 'b")); t("qr", () => WinterJS.qrcode("")); t("num", () => WinterJS.semver.valid(42));"#,
    );
    for name in ["semver", "range", "yaml", "jsonc", "cidr", "shlex", "qr", "num"] {
        assert!(stdout.contains(&format!("{name} THROW TypeError")), "out: {stdout}");
    }
}
