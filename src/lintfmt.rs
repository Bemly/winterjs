//! `winterjs2 lint/fmt`：外部工具命令穿透（plan Phase 8-a，2026-09-11 用户拍板）。
//!
//! `winterjs2 lint ...` → `oxlint ...`、`winterjs2 fmt ...` → `oxfmt ...`：
//! 参数原样转发、stdout/stderr 继承、退出码透传（非零经 `Error::Exit` 静默退出）。
//! 不在 winterjs2 内重新定义语义（oxfmt 默认写回、`--check` 做 CI 检查，均为
//! 上游语义直通）。
//!
//! 查找顺序：① 项目本地 `node_modules/.bin/`——从 cwd 逐级向上（monorepo：
//! 在 packages/foo 里跑也能命中 repo 根安装的工具）→ ② PATH（`which` 轮子，
//! Windows 尊重 PATHEXT）→ ③ 可读错误（指引 `release:` 形装 standalone。
//! 注意：npx 回退曾存在（2026-09-11），同日删除——npx 必须跟 node 一起装，
//! 与零 node 目标冲突；要跑 oxlint/oxfmt 就用 release 二进制）。
//! Windows 命名候选 `.exe`/`.cmd`（.cmd 经 `cmd /C` 起子进程）。
//!
//! 平台注记：oxc 上游为 android/ohos 只出 N-API binding、无 standalone CLI
//! （release workflow 明确 `!android && !ohos` 才构建二进制），故 lint/fmt 在
//! 移动 target 上查找必然落空、报可读错——桌面开发期工具，文档记录即可。

use std::path::{Path, PathBuf};

use crate::error::Error;

/// Windows/Unix 的 bin 候选名（node_modules/.bin 内）。
fn bin_candidates(name: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![format!("{name}.exe"), format!("{name}.cmd"), format!("{name}.bat"), name.to_string()]
    } else {
        vec![name.to_string()]
    }
}

/// PATH 查找（可注入便于单测；生产用 `which` 轮子）。
fn which_real(name: &str) -> Option<PathBuf> {
    which::which(name).ok()
}

/// 项目本地查找：从 `base` 逐级向上找 `node_modules/.bin/<name>`（monorepo 命中根）。
fn find_local_from(base: &Path, name: &str) -> Option<PathBuf> {
    let mut dir = base.to_path_buf();
    loop {
        let bin = dir.join("node_modules").join(".bin");
        for cand in bin_candidates(name) {
            let p = bin.join(&cand);
            if p.is_file() {
                return Some(p);
            }
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// 解析工具可执行文件（本地 → PATH；全落空给可读指引，见 `install_hint`）。
fn resolve(tool: &str, path_lookup: impl Fn(&str) -> Option<PathBuf>) -> Result<PathBuf, Error> {
    let cwd = std::env::current_dir()
        .map_err(|e| Error::Other(format!("cannot get cwd: {e}")))?;
    resolve_in(&cwd, tool, path_lookup)
}

/// `resolve` 的可注入形态（单测无全局 cwd 竞态）。
fn resolve_in(
    base: &std::path::Path,
    tool: &str,
    path_lookup: impl Fn(&str) -> Option<PathBuf>,
) -> Result<PathBuf, Error> {
    if let Some(p) = find_local_from(base, tool) {
        tracing::debug!(target: "winterjs2::lintfmt", tool, exe = %p.display(), "local bin");
        return Ok(p);
    }
    if let Some(p) = path_lookup(tool) {
        tracing::debug!(target: "winterjs2::lintfmt", tool, exe = %p.display(), "PATH bin");
        return Ok(p);
    }
    Err(Error::Other(install_hint(tool)))
}

/// 未找到时的指引（`release:` 形装 standalone；npm 版是 JS 壳、要 node，
/// 不再推荐——见头注 npx 删除记录）。
fn install_hint(tool: &str) -> String {
    format!(
        "{tool} was not found (looked in node_modules/.bin up the directory tree, then PATH).\n\
         Install the standalone binary with:\n  winterjs2 --add '{tool}@release:github/oxc-project/oxc@<tag>/{tool}'\n\
         (pick <tag> from https://github.com/oxc-project/oxc/releases;\n \
         the npm '{tool}' package needs node, do not use it without node)"
    )
}

/// 转发一条外部工具调用（同步：CLI 生命周期内独占，无并发 JS）。
/// stdout/stderr 继承（用户终端直出）；退出码透传。
pub fn run(tool: &str, args: &[String]) -> Result<(), Error> {
    let exe = resolve(tool, which_real)?;
    tracing::info!(target: "winterjs2::lintfmt", tool, exe = %exe.display(), argc = args.len(), "forwarding");
    let status = if cfg!(windows) && exe.extension().is_some_and(|e| e == "cmd" || e == "bat") {
        // .cmd/.bat 无法直接 exec，经 cmd /C 起子进程
        std::process::Command::new("cmd")
            .args(["/C"])
            .arg(&exe)
            .args(args)
            .status()
    } else {
        std::process::Command::new(&exe).args(args).status()
    }
    .map_err(|e| Error::Other(format!("failed to run '{}': {e}", exe.display())))?;
    let code = status.code().unwrap_or(-1);
    tracing::info!(target: "winterjs2::lintfmt", tool, code, "forwarded exit");
    match status.code() {
        Some(0) => Ok(()),
        Some(c) => Err(Error::Exit(c)),
        None => Err(Error::Other(format!("'{tool}' was terminated by a signal"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 单测脚手架：在 tempdir 造 `repo/node_modules/.bin/<name>`，
    /// 返回 (repo 根, packages/foo 子目录)。
    fn make_repo(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("node_modules").join(".bin");
        std::fs::create_dir_all(&bin).unwrap();
        let tool = bin.join(name);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::write(&tool, b"#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        #[cfg(not(unix))]
        std::fs::write(&tool, b"").unwrap();
        let sub = dir.path().join("packages").join("foo");
        std::fs::create_dir_all(&sub).unwrap();
        (dir, sub)
    }

    #[test]
    fn candidates_table() {
        // unix：单候选；win 分支编译期由 CI 证明（此处只验 unix 形状）
        if cfg!(not(windows)) {
            assert_eq!(bin_candidates("oxlint"), vec!["oxlint".to_string()]);
        }
    }

    #[test]
    fn local_lookup_walks_up_for_monorepo() {
        let (dir, sub) = make_repo("oxlint");
        // 在 packages/foo 里跑 → 向上命中 repo 根的 node_modules/.bin
        let found = find_local_from(&sub, "oxlint").expect("must find repo-root tool from subdir");
        assert!(found.ends_with("node_modules/.bin/oxlint"));
        // 非候选名不命中
        assert!(find_local_from(&sub, "nope-not-here").is_none());
        drop(dir);
    }

    #[test]
    fn resolve_prefers_local_over_path() {
        let (dir, sub) = make_repo("oxfmt");
        let marker = dir.path().join("from-path");
        let r = resolve_in(&sub, "oxfmt", |_| Some(marker.clone())).unwrap();
        assert!(r.ends_with("node_modules/.bin/oxfmt"));
        drop(dir);
    }

    #[test]
    fn resolve_falls_back_to_path_then_readable_error() {
        let dir = tempfile::tempdir().unwrap();
        // PATH 命中
        let marker = dir.path().join("pathhit");
        let r = resolve_in(dir.path(), "oxlint", |n| {
            (n == "oxlint").then(|| marker.clone())
        })
        .unwrap();
        assert_eq!(r, marker);
        // 双双落空 → 可读指引（release 形，不再提 npm/npx）
        let err = resolve_in(dir.path(), "oxlint", |_| None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("oxlint was not found"), "{msg}");
        assert!(msg.contains("release:github/oxc-project/oxc"), "{msg}");
        assert!(!msg.contains("npx"), "{msg}");
        assert!(!msg.contains("npm install"), "{msg}");
        drop(dir);
    }
}
