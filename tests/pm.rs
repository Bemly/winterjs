//! 包管理黑盒测试(对齐 src/pm/:install/add/publish/login/upgrade/git/缓存/lifecycle/release)。

mod common;

use common::*;

use assert_fs::prelude::*;

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
fn phase5_npmrc_registry_mirror() {
    // 正常：项目 `.npmrc` 的 registry 生效（不传 --registry 也命中 stub）。
    let port = serve_registry();
    let reg = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    let home = assert_fs::TempDir::new().unwrap();
    dir.child(".npmrc")
        .write_str(&format!("registry={reg}/\n"))
        .unwrap();
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
    dir.child(".npmrc")
        .write_str("registry=http://127.0.0.1:9/\n")
        .unwrap();
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
    dir.child(".npmrc")
        .write_str("registry=http://127.0.0.1:9/\n")
        .unwrap();
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
    dir.child(".npmrc")
        .write_str("registry=http://127.0.0.1:9/\n")
        .unwrap();
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
    assert!(
        out.starts_with(&format!("git-pkg@git+{url}#")),
        "dry-run: {out}"
    );
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
        .args([
            "--add",
            &format!("git-pkg@git+{url}#no-such-ref"),
            "--dry-run",
        ])
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
    assert!(
        out.starts_with(&format!("bare-pkg@git+{url}#")),
        "bare name: {out}"
    );
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
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        dir.path()
            .join("node_modules/git-e2e/package.json")
            .is_file()
    );
    assert!(
        !dir.path().join("node_modules/git-e2e/.git").exists(),
        ".git must not land"
    );
    let lock = std::fs::read_to_string(dir.path().join("winterjs-lock.json")).unwrap();
    assert!(
        lock.contains("\"git-e2e\"") && lock.contains(&format!("git+{url}#")),
        "lock: {lock}"
    );
    let app = dir.child("app.cjs");
    app.write_str("const t = require(\"git-e2e\");\nconsole.log(t.add(19, 23));\n")
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
            .args([
                "--publish",
                "--dry-run",
                "--registry",
                "http://127.0.0.1:9/",
            ])
            .current_dir(dir.path()),
    );
    assert!(out.contains("pub-pkg@1.2.3"), "summary: {out}");
    assert!(
        out.contains("registry: http://127.0.0.1:9/"),
        "summary: {out}"
    );
    assert!(out.contains("files:"), "summary: {out}");
    dir.close().unwrap();
}

#[test]
fn phase5_publish_manifest_errors() {
    // 报错：缺名 / 坏 license，皆 exit=1 且可读。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("package.json")
        .write_str(r#"{"version":"1.0.0"}"#)
        .unwrap();
    let out = winterjs()
        .args(["--publish", "--dry-run"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("no name"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    dir.child("package.json")
        .write_str(r#"{"name":"p","version":"1.0.0","license":"Not-A-License!!"}"#)
        .unwrap();
    let out = winterjs()
        .args(["--publish", "--dry-run"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("license"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    dir.close().unwrap();
}

#[test]
fn phase5_login_token_writes_npmrc() {
    // 正常：`login --token` 把 token 行写进 `$HOME/.npmrc`（其他行保留）。
    let home = assert_fs::TempDir::new().unwrap();
    home.child(".npmrc")
        .write_str("registry=http://127.0.0.1:4873/\n")
        .unwrap();
    let dir = assert_fs::TempDir::new().unwrap();
    let out = winterjs()
        .args([
            "--login",
            "--token",
            "sekret",
            "--registry",
            "http://127.0.0.1:4873/",
        ])
        .env("HOME", home.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let npmrc = std::fs::read_to_string(home.path().join(".npmrc")).unwrap();
    assert!(
        npmrc.contains("//127.0.0.1/:_authToken=sekret"),
        "npmrc: {npmrc}"
    );
    assert!(
        npmrc.contains("registry=http://127.0.0.1:4873/"),
        "npmrc: {npmrc}"
    );
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
    assert!(
        stderr.contains("WINTERJS_UPDATE_GITHUB"),
        "stderr: {stderr}"
    );
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
    assert!(
        out.contains("http://127.0.0.1:4873/oauth/authorize?"),
        "url: {out}"
    );
    assert!(out.contains("--token"), "hint: {out}");
    dir.close().unwrap();
    home.close().unwrap();
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
fn phase5_cache_second_install_hits_cache() {
    // 二次安装全命中缓存：tarball 只下一次，第二次删 node_modules 重装仍成功，
    // 此时 stub 的 tarball 端点已翻为 404（若回源必败），证明走缓存。
    use base64::Engine as _;
    use sha2::Digest as _;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let tgz = make_tgz(&[
        (
            "package.json",
            br#"{"name":"cached-pkg","version":"1.0.0","main":"index.js"}"#,
        ),
        ("index.js", b"exports.v = 1;\n"),
    ]);
    let integrity = format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&tgz))
    );
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
            return (
                200,
                vec![("content-type", "application/json".into())],
                body.into_bytes(),
            );
        }
        if path == "/cached-pkg/-/cached-pkg-1.0.0.tgz" {
            let n = hits.fetch_add(1, Ordering::SeqCst);
            if n >= 1 {
                return (404, vec![], b"gone".to_vec());
            }
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
    let out = winterjs()
        .arg("--add")
        .arg("cached-pkg")
        .arg("--registry")
        .arg(&reg)
        .env("WINTERJS_CACHE", cache.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "first: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(tarball_hits.load(Ordering::SeqCst), 1);
    // 缓存文件已落（pkgs/*.tgz）。
    let cached: Vec<_> = std::fs::read_dir(cache.path().join("pkgs"))
        .unwrap()
        .collect();
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
    assert!(
        out.status.success(),
        "second (cache hit): {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        tarball_hits.load(Ordering::SeqCst),
        1,
        "tarball must not be re-downloaded"
    );
    assert!(
        dir.path()
            .join("node_modules/cached-pkg/package.json")
            .is_file()
    );
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
            return (
                200,
                vec![("content-type", "application/json".into())],
                body.into_bytes(),
            );
        }
        if path == "/life-pkg/-/life-pkg-1.0.0.tgz" {
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
    let out = winterjs()
        .arg("--add")
        .arg("life-pkg")
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
    let order =
        std::fs::read_to_string(dir.path().join("node_modules/life-pkg/order.txt")).unwrap();
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
        (
            "package.json",
            br#"{"name":"badlife","version":"1.0.0","scripts":{"postinstall":"exit 3"}}"#,
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
            return (
                200,
                vec![("content-type", "application/json".into())],
                body.into_bytes(),
            );
        }
        if path == "/badlife/-/badlife-1.0.0.tgz" {
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
fn pm_publish_put_end_to_end() {
    // 真发布闭环：打包→PUT→200；stub 验方法/路径/Bearer/包体形状；无 token 指路 login。
    use std::sync::{Arc, Mutex};
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen2 = seen.clone();
    let port = serve_http(1, move |head, body| {
        let mut log = seen2.lock().unwrap();
        let line = head.lines().next().unwrap_or("").to_owned();
        log.push(line.clone());
        let auth = head
            .lines()
            .find_map(|l| {
                l.strip_prefix("authorization:")
                    .or_else(|| l.strip_prefix("Authorization:"))
            })
            .unwrap_or("")
            .trim()
            .to_owned();
        log.push(format!("auth:{auth}"));
        // 包体形状：versions + dist-tags + _attachments 带 b64
        let v: serde_json::Value =
            serde_json::from_str(&String::from_utf8_lossy(&body)).unwrap_or_default();
        let ok = v.get("versions").and_then(|x| x.get("1.0.0")).is_some()
            && v.get("dist-tags")
                .and_then(|x| x.get("latest"))
                .and_then(|x| x.as_str())
                == Some("1.0.0")
            && v.get("_attachments")
                .and_then(|a| a.as_object())
                .is_some_and(|m| m.len() == 1);
        log.push(format!("body-ok:{ok}"));
        (
            200,
            vec![("content-type", "application/json".into())],
            b"{}".to_vec(),
        )
    });
    let reg = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("package.json")
        .write_str(r#"{"name":"my-pkg","version":"1.0.0","license":"MIT"}"#)
        .unwrap();
    dir.child("index.js").write_str("exports.v = 1;\n").unwrap();
    dir.child(".npmrc")
        .write_str(&format!("registry={reg}\n//127.0.0.1/:_authToken=sekret\n"))
        .unwrap();
    let out = winterjs()
        .args(["--publish", "--registry", &reg])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("published my-pkg@1.0.0"),
        "stdout: {stdout}"
    );
    let log = seen.lock().unwrap();
    assert!(
        log.iter().any(|l| l.starts_with("PUT /my-pkg ")),
        "method/path: {log:?}"
    );
    assert!(
        log.iter().any(|l| l == "auth:Bearer sekret"),
        "auth: {log:?}"
    );
    assert!(log.iter().any(|l| l == "body-ok:true"), "body: {log:?}");
    dir.close().unwrap();
}

#[test]
fn pm_publish_errors() {
    // 报错：无 token 指路 login（exit=1）；dry-run 不碰网络。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("package.json")
        .write_str(r#"{"name":"p","version":"1.0.0","license":"MIT"}"#)
        .unwrap();
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
        .args([
            "--publish",
            "--dry-run",
            "--registry",
            "http://127.0.0.1:9/",
        ])
        .env("HOME", home.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8(out.stdout).unwrap().contains("p@1.0.0"));
    dir.close().unwrap();
    home.close().unwrap();
}

// ── ACME 自动证书（dry-run 只校验打印，不碰网络；真签发需公网 :80 + DNS）─────

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
            tar.append_data(&mut hdr, "mytool-x86_64-unknown-linux-gnu", &data[..])
                .unwrap();
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
            return (
                200,
                vec![("content-type", "application/json".into())],
                body.into_bytes(),
            );
        }
        if path == "/mytool.tgz" {
            return (
                200,
                vec![("content-type", "application/octet-stream".into())],
                (*tool_tgz).clone(),
            );
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
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
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
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("added mytool@release:o/r@v1"),
        "stdout: {stdout}"
    );
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
            tar.append_data(&mut hdr, "mytool-x86_64-unknown-linux-gnu", &data[..])
                .unwrap();
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
            let host = head
                .lines()
                .find_map(|l| l.strip_prefix("Host:").or_else(|| l.strip_prefix("host:")))
                .and_then(|v| v.trim().split(':').nth(1))
                .unwrap_or("")
                .to_owned();
            let body = serde_json::json!({
                "assets": [
                    { "name": "mytool-x86_64-unknown-linux-gnu.tar.gz", "browser_download_url": format!("http://127.0.0.1:{host}/mytool.tgz") },
                    { "name": "mytool-aarch64-apple-darwin.tar.gz", "browser_download_url": format!("http://127.0.0.1:{host}/mytool.tgz") },
                ],
            })
            .to_string();
            return (
                200,
                vec![("content-type", "application/json".into())],
                body.into_bytes(),
            );
        }
        if path == "/mytool.tgz" {
            return (
                200,
                vec![("content-type", "application/octet-stream".into())],
                (*tool_tgz).clone(),
            );
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
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let status =
        std::process::Command::new(dir.path().join("node_modules").join(".bin").join("mytool"))
            .output()
            .unwrap();
    assert!(status.status.success());
    assert_eq!(String::from_utf8(status.stdout).unwrap(), "fake-tool ok\n");
    dir.close().unwrap();
}

// ── Phase 9a（plan2）：node:events / node:async_hooks / internal 小件 ────────
// 断言口径：Node test/parallel 原文语义按需转写（test-events.js / test-event-emitter*
// / test-async-hooks*），不整目录拉取；消息格式逐字。

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
        let ver = |tarball: String, deps: serde_json::Value| serde_json::json!({ "dist": { "tarball": tarball, "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==" }, "dependencies": deps });
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
        (
            200,
            vec![("content-type", "application/json".into())],
            body.into_bytes(),
        )
    });
    *holder.lock().unwrap() = Some(port);
    port
}

/// 现场建 git 仓（`git` CLI；`user.*` 经 `-c` 注入，不碰全局配置；返回仓目录）。
/// 含 `package.json(name/index.js)` + 一个 commit + 可选 tag。
fn make_git_repo(name: &str, tagged: bool) -> assert_fs::TempDir {
    fn git(dir: &std::path::Path, args: &[&str]) {
        let mut c = std::process::Command::new("git");
        c.args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1");
        let out = c.output().expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let dir = assert_fs::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        format!(r#"{{"name":"{name}","version":"0.1.0","main":"index.js"}}"#),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("index.js"),
        b"exports.add = (a, b) => a + b;\n",
    )
    .unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["add", "-A"]);
    git(
        dir.path(),
        &[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-qm",
            "init",
        ],
    );
    if tagged {
        git(dir.path(), &["tag", "v1.0.0"]);
    }
    dir
}

