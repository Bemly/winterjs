//! tests/pm/release.rs — 二进制分发（对齐 src/pm/release.rs）。

use crate::common::*;

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
    let out = winterjs2()
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
    let out = winterjs2()
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
    let lock = std::fs::read_to_string(dir.path().join("winterjs2-lock.json")).unwrap();
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
    let out = winterjs2()
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
