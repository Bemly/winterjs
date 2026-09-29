//! winterjs2 init 黑盒测试(对齐 src/initpkg.rs)。

mod common;

use common::*;

#[test]
fn phase7_init_closed_loop() {
    // 正常：init 三件 + 内容含名 + 紧接着 `test` 即绿（闭环）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = stdout_of(
        winterjs2()
            .args(["--init", "my-pkg", "--yes"])
            .current_dir(dir.path()),
    );
    assert!(
        out.contains("created package.json") && out.contains("created hello.test.js"),
        "init:\n{out}"
    );
    let pkg = std::fs::read_to_string(dir.path().join("package.json")).unwrap();
    assert!(pkg.contains("\"my-pkg\""), "package.json:\n{pkg}");
    let out = winterjs2()
        .arg("--test")
        .arg(".")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("ok - hello.test.js"));
    dir.close().unwrap();
}

#[test]
fn phase7_init_bad_name() {
    // 报错：非法名 exit=1 且可读。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = winterjs2()
        .args(["--init", "Bad Name!", "--yes"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("bad package name"), "stderr: {stderr}");
    dir.close().unwrap();
}

#[test]
fn phase7_init_rebuild_keeps_existing() {
    // 重建：已存在文件不碰（exit=0），缺失的补齐。
    let dir = assert_fs::TempDir::new().unwrap();
    assert!(
        winterjs2()
            .args(["--init", "p", "--yes"])
            .current_dir(dir.path())
            .output()
            .unwrap()
            .status
            .success()
    );
    std::fs::write(dir.path().join("index.js"), b"mine\n").unwrap();
    std::fs::remove_file(dir.path().join("hello.test.js")).unwrap();
    let out = winterjs2()
        .args(["--init", "p", "--yes"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let so = String::from_utf8_lossy(&out.stdout);
    assert!(so.contains("exists, skipped index.js"), "stdout: {so}");
    assert!(so.contains("created hello.test.js"), "stdout: {so}");
    assert_eq!(
        std::fs::read(dir.path().join("index.js")).unwrap(),
        b"mine\n"
    );
    dir.close().unwrap();
}

#[test]
fn phase7_init_needs_yes_without_tty() {
    // 边界：非 TTY 缺 --yes 即报可读错（不挂起等输入）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = winterjs2()
        .args(["--init", "p"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--yes"));
    assert!(
        !dir.path().join("package.json").exists(),
        "nothing must be written"
    );
    dir.close().unwrap();
}

#[test]
fn phase9_init_adopts_existing_project() {
    // vue-project 案：已有 package.json 的项目只补缺失、不碰现有一字节。
    let dir = assert_fs::TempDir::new().unwrap();
    let pkg = r#"{"name":"vue-project","version":"0.0.0","private":true,"type":"module","scripts":{"dev":"vite"}}"#;
    std::fs::write(dir.path().join("package.json"), pkg).unwrap();
    let out = winterjs2()
        .args(["--init", "--yes"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let so = String::from_utf8_lossy(&out.stdout);
    assert!(so.contains("exists, skipped package.json"), "stdout: {so}");
    assert_eq!(std::fs::read_to_string(dir.path().join("package.json")).unwrap(), pkg);
    assert!(dir.path().join("index.js").exists());
    assert!(dir.path().join("hello.test.js").exists());
    // 全齐再跑：already initialized，exit=0。
    let out2 = winterjs2()
        .args(["--init", "--yes"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out2.status.success());
    assert!(
        String::from_utf8_lossy(&out2.stdout).contains("already initialized"),
        "stdout: {}",
        String::from_utf8_lossy(&out2.stdout)
    );
    dir.close().unwrap();
}

#[test]
fn phase9_init_short_flag_and_force() {
    // -I 简写可用；--force 逐个覆盖并报 overwrote。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = stdout_of(
        winterjs2().args(["-I", "short-pkg", "--yes"]).current_dir(dir.path()),
    );
    assert!(out.contains("created package.json"), "init:\n{out}");
    let pkg = std::fs::read_to_string(dir.path().join("package.json")).unwrap();
    assert!(pkg.contains("\"short-pkg\""), "package.json:\n{pkg}");

    std::fs::write(dir.path().join("index.js"), b"mine\n").unwrap();
    let out = winterjs2()
        .args(["--init", "short-pkg", "--yes", "--force"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("overwrote index.js"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_ne!(std::fs::read(dir.path().join("index.js")).unwrap(), b"mine\n");
    dir.close().unwrap();
}


// ── 依赖安装（2026-09-13 补，对齐 bun install / bun init 实测口径）────────

use base64::Engine as _;
use sha2::Digest as _;

/// 多包 stub registry：每个名字给 packument + tarball（真 integrity 校验可过）。
/// `missing` 列表里的名字 packument 直接 404（optional 容忍路径用）。
fn serve_pkg_registry(pkgs: &'static [&'static str], missing: &'static [&'static str]) -> u16 {
    let tarballs: std::sync::Arc<Vec<(String, Vec<u8>)>> = std::sync::Arc::new(
        pkgs.iter()
            .map(|name| {
                let mut tar_data = Vec::new();
                {
                    let mut ar = tar::Builder::new(&mut tar_data);
                    for (fname, fdata) in [
                        (
                            "package.json",
                            format!(r#"{{"name":"{name}","version":"1.0.0","main":"index.js","bin":{{"{name}":"cli.js"}}}}"#).as_bytes(),
                        ),
                        ("index.js", &b"exports.ok = () => 42;\n"[..]),
                        ("cli.js", &b"console.log('bin-ok');\n"[..]),
                    ] {
                        let mut header = tar::Header::new_gnu();
                        header.set_path(format!("package/{fname}")).unwrap();
                        header.set_size(fdata.len() as u64);
                        header.set_mode(0o644);
                        header.set_cksum();
                        ar.append(&header, fdata).unwrap();
                    }
                    ar.finish().unwrap();
                }
                let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                std::io::Write::write_all(&mut enc, &tar_data).unwrap();
                (name.to_string(), enc.finish().unwrap())
            })
            .collect(),
    );
    common::serve_http(64, move |head, _body| {
        let line = head.lines().next().unwrap_or("").to_owned();
        let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
        let host = head
            .lines()
            .find_map(|l| l.strip_prefix("Host:").or_else(|| l.strip_prefix("host:")))
            .map(|v| v.trim().to_owned())
            .unwrap_or_default();
        // packument：`/<name>`
        if let Some(name) = path.strip_prefix('/').filter(|p| !p.contains('/')) {
            if missing.contains(&name) {
                return (404, vec![], b"nope".to_vec());
            }
            if let Some((_, tgz)) = tarballs.iter().find(|(n, _)| *n == name) {
                let integrity = format!(
                    "sha512-{}",
                    base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(tgz))
                );
                let body = serde_json::json!({
                    "name": name,
                    "dist-tags": { "latest": "1.0.0" },
                    "versions": {
                        "1.0.0": {
                            "dist": {
                                "tarball": format!("http://{host}/{name}/-/{name}-1.0.0.tgz"),
                                "integrity": integrity,
                            },
                            "dependencies": {},
                        },
                    },
                })
                .to_string();
                return (200, vec![("content-type", "application/json".into())], body.into_bytes());
            }
        }
        // tarball：`/<name>/-/<name>-1.0.0.tgz`（`/-/` 前是名，后是 `<名>-1.0.0.tgz`）
        let tar_name = path.strip_prefix('/').and_then(|p| p.split_once("/-/")).and_then(
            |(name, file)| (file == format!("{name}-1.0.0.tgz")).then(|| name.to_string()),
        );
        if let Some(name) = tar_name {
            if let Some((_, tgz)) = tarballs.iter().find(|(n, _)| *n == name) {
                return (
                    200,
                    vec![("content-type", "application/octet-stream".into())],
                    tgz.clone(),
                );
            }
        }
        (404, vec![], b"nope".to_vec())
    })
}

fn init_cmd(dir: &assert_fs::TempDir, reg: &str, cache: &assert_fs::TempDir) -> assert_cmd::Command {
    let mut cmd = winterjs2();
    cmd.args(["--init", "--yes", "--registry", reg])
        .env("WINTERJS2_CACHE", cache.path())
        .current_dir(dir.path());
    cmd
}

#[test]
fn phase9_init_installs_manifest_deps() {
    // deps + devDeps 都装（npm 默认口径）+ .bin 链接 + lockfile 记指纹。
    let port = serve_pkg_registry(&["init-dep-a", "init-dep-b"], &[]);
    let reg = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    let cache = assert_fs::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"p","dependencies":{"init-dep-a":"^1.0.0"},"devDependencies":{"init-dep-b":"^1.0.0"}}"#,
    )
    .unwrap();
    let out = init_cmd(&dir, &reg, &cache).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let so = String::from_utf8_lossy(&out.stdout);
    assert!(so.contains("added init-dep-a@1.0.0"), "stdout: {so}");
    assert!(so.contains("added init-dep-b@1.0.0"), "stdout: {so}");
    assert!(dir.path().join("node_modules/init-dep-a/index.js").is_file());
    assert!(dir.path().join("node_modules/init-dep-b/package.json").is_file());
    assert!(dir.path().join("node_modules/.bin/init-dep-a").exists());
    let lock = std::fs::read_to_string(dir.path().join("winterjs2-lock.json")).unwrap();
    assert!(lock.contains("init-dep-a") && lock.contains("init-dep-b"), "lock: {lock}");
    assert!(lock.contains("manifest"), "lock missing manifest field: {lock}");
    dir.close().unwrap();
    cache.close().unwrap();
}

#[test]
fn phase9_init_manifest_up_to_date_is_idempotent() {
    // 二跑：指纹没变 + node_modules 在 → `dependencies up to date`，零网络
    //（serve 0 请求：二跑真发了请求必然 connection refused → exit≠0）。
    let port = serve_pkg_registry(&["init-dep-a"], &[]);
    let reg = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    let cache = assert_fs::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"p","dependencies":{"init-dep-a":"^1.0.0"}}"#,
    )
    .unwrap();
    let out = init_cmd(&dir, &reg, &cache).output().unwrap();
    assert!(out.status.success(), "first run stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out2 = init_cmd(&dir, &reg, &cache).output().unwrap();
    assert!(out2.status.success(), "second run stderr: {}", String::from_utf8_lossy(&out2.stderr));
    let so = String::from_utf8_lossy(&out2.stdout);
    assert!(so.contains("dependencies up to date"), "stdout: {so}");
    assert!(!so.contains("added"), "no reinstall expected: {so}");
    dir.close().unwrap();
    cache.close().unwrap();
}

#[test]
fn phase9_init_manifest_edit_reinstalls_incrementally() {
    // 清单编辑（加 dep）→ 指纹变 → 重装（新包装上、旧包保留、lockfile 合并不丢）。
    let port = serve_pkg_registry(&["init-dep-a", "init-dep-c"], &[]);
    let reg = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    let cache = assert_fs::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"p","dependencies":{"init-dep-a":"^1.0.0"}}"#,
    )
    .unwrap();
    let out = init_cmd(&dir, &reg, &cache).output().unwrap();
    assert!(out.status.success(), "first: {}", String::from_utf8_lossy(&out.stderr));
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"p","dependencies":{"init-dep-a":"^1.0.0","init-dep-c":"^1.0.0"}}"#,
    )
    .unwrap();
    let out2 = init_cmd(&dir, &reg, &cache).output().unwrap();
    assert!(out2.status.success(), "second: {}", String::from_utf8_lossy(&out2.stderr));
    let so = String::from_utf8_lossy(&out2.stdout);
    assert!(so.contains("added init-dep-c@1.0.0"), "stdout: {so}");
    assert!(dir.path().join("node_modules/init-dep-a/package.json").is_file(), "old dep kept");
    let lock = std::fs::read_to_string(dir.path().join("winterjs2-lock.json")).unwrap();
    assert!(lock.contains("init-dep-a") && lock.contains("init-dep-c"), "lock merged: {lock}");
    dir.close().unwrap();
    cache.close().unwrap();
}

#[test]
fn phase9_init_optional_deps_tolerate_failure() {
    // optionalDependencies：命中照装（optional 记账）；404 容忍跳过，exit=0。
    let port = serve_pkg_registry(&["init-dep-a"], &["init-opt-missing"]);
    let reg = format!("http://127.0.0.1:{port}");
    let dir = assert_fs::TempDir::new().unwrap();
    let cache = assert_fs::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"p","optionalDependencies":{"init-dep-a":"^1.0.0","init-opt-missing":"^1.0.0"}}"#,
    )
    .unwrap();
    let out = init_cmd(&dir, &reg, &cache).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let so = String::from_utf8_lossy(&out.stdout);
    assert!(so.contains("added init-dep-a@1.0.0"), "stdout: {so}");
    // 求解期 404 容忍跳过是静默的（tracing::debug；install 期失败才打
    // `skipped optional`，d5 既有口径）——缺席即验收。
    assert!(!so.contains("init-opt-missing"), "stdout: {so}");
    assert!(dir.path().join("node_modules/init-dep-a").is_dir());
    assert!(!dir.path().join("node_modules/init-opt-missing").exists());
    dir.close().unwrap();
    cache.close().unwrap();
}

#[test]
fn phase9_init_no_deps_and_bad_manifest() {
    // 无依赖段：不建 node_modules、零安装输出；坏 JSON：可读错 exit=1。
    let dir = assert_fs::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"p","scripts":{"dev":"vite"}}"#,
    )
    .unwrap();
    let out = winterjs2()
        .args(["--init", "--yes"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let so = String::from_utf8_lossy(&out.stdout);
    assert!(!so.contains("added"), "no install expected: {so}");
    assert!(!dir.path().join("node_modules").exists(), "no node_modules without deps");

    let bad = assert_fs::TempDir::new().unwrap();
    std::fs::write(bad.path().join("package.json"), r#"{"name": "#).unwrap();
    let out = winterjs2()
        .args(["--init", "--yes"])
        .current_dir(bad.path())
        .output()
        .unwrap();
    assert!(!out.status.success(), "bad manifest must fail");
    let se = String::from_utf8_lossy(&out.stderr);
    assert!(se.contains("cannot parse"), "stderr: {se}");
    bad.close().unwrap();
    dir.close().unwrap();
}
