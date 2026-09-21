//! tests/pm/install.rs — 安装（对齐 src/pm/install.rs）。

use crate::common::*;
use assert_fs::prelude::*;
use super::helpers::*;

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
        winterjs()
            .args(["--add", "app", "--dry-run", "--registry"])
            .arg(&reg),
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
    let out = winterjs()
        .args(["--add", "no-such-pkg-xyz", "--dry-run", "--registry"])
        .arg(&reg)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not found"), "stderr: {stderr}");
    let out = winterjs()
        .args(["--add", "left-pad@^9.0.0", "--dry-run", "--registry"])
        .arg(&reg)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no version"), "stderr: {stderr}");
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
    let integrity = format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&tgz))
    );
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
            return (
                200,
                vec![("content-type", "application/json".into())],
                body.into_bytes(),
            );
        }
        if path == "/tiny-pkg/-/tiny-pkg-1.0.0.tgz" {
            return (
                200,
                vec![("content-type", "application/octet-stream".into())],
                (*tgz_holder).clone(),
            );
        }
        (404, vec![], b"nope".to_vec())
    });
    let reg = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .arg("--add")
        .arg("tiny-pkg")
        .arg("--registry")
        .arg(&reg)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("added tiny-pkg@1.0.0"));
    // 落地断言：包文件 + bin 链接 + lockfile。
    assert!(
        dir.path()
            .join("node_modules/tiny-pkg/package.json")
            .is_file()
    );
    assert!(dir.path().join("node_modules/.bin/tiny-bin").exists());
    let lock = std::fs::read_to_string(dir.path().join("winterjs-lock.json")).unwrap();
    assert!(
        lock.contains("\"tiny-pkg\"") && lock.contains("1.0.0") && lock.contains("sha512-"),
        "lock: {lock}"
    );
    // 装完即跑（裸导入走 node_modules 解析）。
    let app = dir.child("app.cjs");
    app.write_str("const t = require(\"tiny-pkg\");\nconsole.log(t.add(19, 23));\n")
        .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(app.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "42\n");
    dir.close().unwrap();
}

#[test]
fn phase5_install_dir_first_tarball_stub() {
    // 目录首条目 tarball（@types/chai 系：`chai/` 目录条目打头，非 `package/`
    // 布局；修前解包报 `failed to create <staging>`，见 AGENTS §4.69）。
    // 现场打 gz（首条目为显式目录），stub 下发→真装→require 可跑。
    use base64::Engine as _;
    use sha2::Digest as _;
    use std::io::Write as _;
    let mut tar_data = Vec::new();
    {
        let mut ar = tar::Builder::new(&mut tar_data);
        let mut dir_header = tar::Header::new_gnu();
        dir_header.set_entry_type(tar::EntryType::Directory);
        dir_header.set_path("oddball/").unwrap();
        dir_header.set_size(0);
        dir_header.set_mode(0o755);
        dir_header.set_cksum();
        ar.append(&dir_header, &[][..]).unwrap();
        for (name, body) in [
            (
                "oddball/package.json",
                br#"{"name":"oddball","version":"1.0.0","main":"index.js"}"#.as_slice(),
            ),
            ("oddball/index.js", b"exports.add = (a, b) => a + b;\n".as_slice()),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_path(name).unwrap();
            header.set_size(body.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            ar.append(&header, body).unwrap();
        }
        ar.finish().unwrap();
    }
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(&tar_data).unwrap();
    let tgz = enc.finish().unwrap();
    let integrity = format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&tgz))
    );
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
        if path == "/oddball" {
            let body = serde_json::json!({
                "name": "oddball",
                "dist-tags": { "latest": "1.0.0" },
                "versions": { "1.0.0": {
                    "dist": {
                        "tarball": format!("http://127.0.0.1:{port}/oddball/-/oddball-1.0.0.tgz"),
                        "integrity": *int_holder,
                    },
                    "dependencies": {},
                } },
            })
            .to_string();
            return (
                200,
                vec![("content-type", "application/json".into())],
                body.into_bytes(),
            );
        }
        if path == "/oddball/-/oddball-1.0.0.tgz" {
            return (
                200,
                vec![("content-type", "application/octet-stream".into())],
                (*tgz_holder).clone(),
            );
        }
        (404, vec![], b"nope".to_vec())
    });
    let reg = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .arg("--add")
        .arg("oddball")
        .arg("--registry")
        .arg(&reg)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("added oddball@1.0.0"));
    assert!(
        dir.path()
            .join("node_modules/oddball/package.json")
            .is_file()
    );
    let app = dir.child("app.cjs");
    app.write_str("const t = require(\"oddball\");\nconsole.log(t.add(19, 23));\n")
        .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(app.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "42\n");
    dir.close().unwrap();
}

#[test]
fn phase5_stale_staging_recovered() {
    // kill -9 模拟：孤儿 `.staging-*` + 半写 tmp 残留，下次安装自愈且不 corrupt。
    use base64::Engine as _;
    use sha2::Digest as _;
    let tgz = make_tgz(&[
        (
            "package.json",
            br#"{"name":"stale-pkg","version":"1.0.0","main":"index.js"}"#,
        ),
        ("index.js", b"exports.v = 1;\n"),
    ]);
    let integrity = format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&tgz))
    );
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
            return (
                200,
                vec![("content-type", "application/json".into())],
                body.into_bytes(),
            );
        }
        if path == "/stale-pkg/-/stale-pkg-1.0.0.tgz" {
            return (
                200,
                vec![("content-type", "application/octet-stream".into())],
                (*tgz_holder).clone(),
            );
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
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !nm.join(".staging-999-deadbeef").exists(),
        "stale staging must be cleaned"
    );
    assert!(
        nm.join("stale-pkg/package.json").is_file(),
        "real package must land"
    );
    assert!(
        !nm.join("stale-pkg/junk.txt").exists(),
        "orphan junk must not leak into package"
    );
    dir.close().unwrap();
    cache.close().unwrap();
}

// ── Phase 6-d1：serve 静态文件 ─────────────────────────────────────────────

#[test]
fn pm_install_global_lands_in_global_root() {
    // 全局：包落 `WINTERJS_GLOBAL_ROOT/node_modules`，cwd 保持干净；打 PATH 指引。
    use base64::Engine as _;
    use sha2::Digest as _;
    let tgz = make_tgz(&[
        (
            "package.json",
            br#"{"name":"g-pkg","version":"1.0.0","main":"index.js","bin":{"g-bin":"cli.js"}}"#,
        ),
        ("index.js", b"exports.v = 1;\n"),
        ("cli.js", b"console.log(\"bin-ok\");\n"),
    ]);
    let integrity = format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&tgz))
    );
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
            return (
                200,
                vec![("content-type", "application/json".into())],
                body.into_bytes(),
            );
        }
        if path == "/g-pkg/-/g-pkg-1.0.0.tgz" {
            return (
                200,
                vec![("content-type", "application/octet-stream".into())],
                (*tgz_holder).clone(),
            );
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
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("added g-pkg@1.0.0"), "stdout: {stdout}");
    assert!(stdout.contains("global root:"), "PATH hint:\n{stdout}");
    assert!(
        groot
            .path()
            .join("node_modules/g-pkg/package.json")
            .is_file()
    );
    assert!(groot.path().join("node_modules/.bin/g-bin").exists());
    // cwd 保持干净：全局安装不污染工程
    assert!(
        !dir.path().join("node_modules").exists(),
        "cwd must stay clean"
    );
    assert!(
        !dir.path().join("winterjs-lock.json").exists(),
        "lockfile goes to global root"
    );
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
        .args([
            "--install",
            "left-pad@^1.0.0",
            "--dry-run",
            "--registry",
            &reg,
        ])
        .env("WINTERJS_GLOBAL_ROOT", target.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("left-pad@"));
    assert!(
        !target.path().exists(),
        "dry-run must not create global root"
    );
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
            (
                "package.json",
                format!(r#"{{"name":"{pkg}","version":"1.0.0"}}"#).as_bytes(),
            ),
            ("index.js", b"exports.v = 1;\n"),
        ]);
        let integrity = format!(
            "sha512-{}",
            base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&tgz))
        );
        (std::sync::Arc::new(tgz), std::sync::Arc::new(integrity))
    };
    let tool_tgz = make_tgz(&[
        ("package.json", br#"{"name":"tool-pkg","version":"1.0.0","main":"index.js","bin":{"tool-bin":"cli.js"}}"#),
        ("index.js", b"exports.v = 1;\n"),
        ("cli.js", b"console.log(\"tool-bin-ok\");\n"),
    ]);
    let tool_int = format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&tool_tgz))
    );
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
            (
                200,
                vec![("content-type", "application/octet-stream".into())],
                (**holder).clone(),
            )
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
            return (
                200,
                vec![("content-type", "application/json".into())],
                body.into_bytes(),
            );
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
            return (
                200,
                vec![("content-type", "application/json".into())],
                body.into_bytes(),
            );
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
            return (
                200,
                vec![("content-type", "application/json".into())],
                body.into_bytes(),
            );
        }
        if path == "/tool-bind-badtar" {
            // packument 过、tarball 404 → 安装期容忍（skipped 行）。
            let body = pack(
                "tool-bind-badtar",
                ver(
                    format!(
                        "http://127.0.0.1:{port}/tool-bind-badtar/-/tool-bind-badtar-1.0.0.tgz"
                    ),
                    &bad_int_c,
                    serde_json::json!({}),
                ),
            );
            return (
                200,
                vec![("content-type", "application/json".into())],
                body.into_bytes(),
            );
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
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("added tool-pkg@1.0.0"), "stdout: {stdout}");
    assert!(
        stdout.contains("added tool-bind-ok@1.0.0"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("skipped optional tool-bind-badtar"),
        "stdout: {stdout}"
    );
    // 落地断言：命中装上（含 .bin 链）、异平台/404/坏包缺席、lockfile 只记装上的。
    assert!(
        dir.path()
            .join("node_modules/tool-bind-ok/package.json")
            .is_file()
    );
    assert!(!dir.path().join("node_modules/tool-bind-nope").exists());
    assert!(!dir.path().join("node_modules/tool-bind-404").exists());
    assert!(!dir.path().join("node_modules/tool-bind-badtar").exists());
    assert!(dir.path().join("node_modules/.bin/tool-bin").exists());
    let lock = std::fs::read_to_string(dir.path().join("winterjs-lock.json")).unwrap();
    assert!(
        lock.contains("tool-bind-ok") && !lock.contains("tool-bind-nope"),
        "lock: {lock}"
    );
    // 装完即跑（bin 链可用）。
    let out = winterjs()
        .arg("--run")
        .arg(dir.path().join("node_modules/tool-pkg/cli.js"))
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "tool-bin-ok\n");
    dir.close().unwrap();
}
