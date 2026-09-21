//! tests/pm/helpers.rs — pm 黑盒共享脚手架（stub registry/git 仓库）。

use crate::common::*;

/// 本地 stub registry（packument JSON；tarball URL 指回本端口，5b 用）。
pub(crate) fn serve_registry() -> u16 {
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
pub(crate) fn make_git_repo(name: &str, tagged: bool) -> assert_fs::TempDir {
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
