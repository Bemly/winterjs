//! `winterjs lint/fmt`：外部工具命令穿透（plan Phase 8-a，2026-09-11 用户拍板）。
//!
//! `winterjs lint ...` → `oxlint ...`、`winterjs fmt ...` → `oxfmt ...`：
//! 参数原样转发、stdout/stderr 继承、退出码透传（非零经 `Error::Exit` 静默退出）。
//! 不在 winterjs 内重新定义语义（oxfmt 默认写回、`--check` 做 CI 检查，均为
//! 上游语义直通）。
//!
//! 查找顺序：① 项目本地 `node_modules/.bin/`——从 cwd 逐级向上（monorepo：
//! 在 packages/foo 里跑也能命中 repo 根安装的工具）→ ② PATH（`which` 轮子，
//! Windows 尊重 PATHEXT）→ ③ `npx --yes <pkg>@latest` 回退（stderr 明示，
//! 非静默；项目无安装、PATH 无命中时才走网络，lockfile 不受影响）→
//! ④ 可读错误（npx 也不存在时；指引本地安装）。
//! Windows 命名候选 `.exe`/`.cmd`（.cmd 经 `cmd /C` 起子进程；npx 本身就是
//! .cmd，走同一条路）。
//!
//! 平台注记：oxc 上游为 android/ohos 只出 N-API binding、无 standalone CLI
//! （release workflow 明确 `!android && !ohos` 才构建二进制），故 lint/fmt 在
//! 移动 target 上查找必然落空、报可读错——桌面开发期工具，文档记录即可。

use std::path::{Path, PathBuf};

use crate::error::Error;

/// npx 回退的版本（`@latest`；本地有安装时永远不用它，版本锁定靠本地安装）。
pub const NPX_TAG: &str = "latest";

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

/// 解析结果：直调本地二进制，或经 npx 回退。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// 直接可执行文件。
    Direct(PathBuf),
    /// `npx --yes <pkg>@latest`（npx 路径 + 包名，参数稍后拼）。
    Npx { npx: PathBuf, pkg: String },
}

/// 解析工具可执行文件（本地 → PATH → npx 回退；全落空给可读指引）。
fn resolve(tool: &str, path_lookup: impl Fn(&str) -> Option<PathBuf>) -> Result<Resolved, Error> {
    let cwd = std::env::current_dir()
        .map_err(|e| Error::Other(format!("cannot get cwd: {e}")))?;
    resolve_in(&cwd, tool, path_lookup)
}

/// `resolve` 的可注入形态（单测无全局 cwd 竞态）。
fn resolve_in(
    base: &std::path::Path,
    tool: &str,
    path_lookup: impl Fn(&str) -> Option<PathBuf>,
) -> Result<Resolved, Error> {
    if let Some(p) = find_local_from(base, tool) {
        tracing::debug!(target: "winterjs::lintfmt", tool, exe = %p.display(), "local bin");
        return Ok(Resolved::Direct(p));
    }
    if let Some(p) = path_lookup(tool) {
        tracing::debug!(target: "winterjs::lintfmt", tool, exe = %p.display(), "PATH bin");
        return Ok(Resolved::Direct(p));
    }
    // npx 回退（`npx oxlint@latest` 上游即发 npm 包；stderr 明示，非静默）。
    if let Some(npx) = path_lookup("npx") {
        tracing::info!(target: "winterjs::lintfmt", tool, exe = %npx.display(), "npx fallback");
        return Ok(Resolved::Npx { npx, pkg: format!("{tool}@{NPX_TAG}") });
    }
    Err(Error::Other(format!(
        "{tool} was not found (looked in node_modules/.bin up the directory tree, then PATH, then npx).\n\
         Install it with:\n  winterjs install {tool}\nor:\n  npm install -D {tool}"
    )))
}

/// 由 `Resolved` 组装 `Command`（`.cmd/.bat` 经 `cmd /C`；npx 拼 `--yes <pkg>@latest`）。
fn command_for(resolved: &Resolved, args: &[String]) -> std::process::Command {
    // Windows 的 .cmd/.bat（含 npx.cmd）无法直接 exec，经 cmd /C 起子进程
    #[cfg(windows)]
    fn via_cmd(exe: &Path, extra: &[&str], args: &[String]) -> std::process::Command {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C"]).arg(exe).args(extra).args(args);
        c
    }
    match resolved {
        Resolved::Direct(exe) => {
            #[cfg(windows)]
            if exe.extension().is_some_and(|e| e == "cmd" || e == "bat") {
                return via_cmd(exe, &[], args);
            }
            let mut c = std::process::Command::new(exe);
            c.args(args);
            c
        }
        Resolved::Npx { npx, pkg } => {
            // `--yes` 跳过 npx 首装确认（非 TTY/CI 必备）；用户参数原样透传
            #[cfg(windows)]
            if npx.extension().is_some_and(|e| e == "cmd" || e == "bat") {
                return via_cmd(npx, &["--yes", pkg], args);
            }
            let mut c = std::process::Command::new(npx);
            c.args(["--yes", pkg]).args(args);
            c
        }
    }
}

/// 转发一条外部工具调用（同步：CLI 生命周期内独占，无并发 JS）。
/// stdout/stderr 继承（用户终端直出）；退出码透传；npx 回退时 stderr 明示一行。
pub fn run(tool: &str, args: &[String]) -> Result<(), Error> {
    let resolved = resolve(tool, which_real)?;
    match &resolved {
        Resolved::Direct(exe) => {
            tracing::info!(target: "winterjs::lintfmt", tool, exe = %exe.display(), argc = args.len(), "forwarding");
        }
        Resolved::Npx { pkg, .. } => {
            eprintln!("winterjs: {tool} not found locally, falling back to `npx --yes {pkg}` (install locally for pinned versions)");
            tracing::info!(target: "winterjs::lintfmt", tool, pkg = pkg.as_str(), argc = args.len(), "npx fallback");
        }
    }
    let exe_label = match &resolved {
        Resolved::Direct(exe) => exe.display().to_string(),
        Resolved::Npx { npx, pkg } => format!("{} --yes {pkg}", npx.display()),
    };
    let status = command_for(&resolved, args)
        .status()
        .map_err(|e| Error::Other(format!("failed to run '{exe_label}': {e}")))?;
    let code = status.code().unwrap_or(-1);
    tracing::info!(target: "winterjs::lintfmt", tool, code, "forwarded exit");
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
        assert!(matches!(r, Resolved::Direct(p) if p.ends_with("node_modules/.bin/oxfmt")));
        drop(dir);
    }

    #[test]
    fn resolve_falls_back_to_path_then_npx_then_readable_error() {
        let dir = tempfile::tempdir().unwrap();
        // PATH 命中（Direct）
        let marker = dir.path().join("pathhit");
        let r = resolve_in(dir.path(), "oxlint", |n| {
            (n == "oxlint").then(|| marker.clone())
        })
        .unwrap();
        assert_eq!(r, Resolved::Direct(marker));
        // 本地+PATH 双落空 → npx 回退（包名带 @latest）
        let npx = dir.path().join("npx");
        let r = resolve_in(dir.path(), "oxlint", |n| {
            (n == "npx").then(|| npx.clone())
        })
        .unwrap();
        assert_eq!(r, Resolved::Npx { npx: npx.clone(), pkg: "oxlint@latest".into() });
        // npx 也无 → 可读指引（含两种安装方式）
        let err = resolve_in(dir.path(), "oxlint", |_| None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("oxlint was not found"), "{msg}");
        assert!(msg.contains("winterjs install oxlint"), "{msg}");
        assert!(msg.contains("npm install -D oxlint"), "{msg}");
        drop(dir);
    }

    #[test]
    fn command_for_shapes() {
        // Direct 原样透传参数
        let c = command_for(&Resolved::Direct(PathBuf::from("/bin/oxlint")), &["a".into()]);
        assert_eq!(c.get_program(), "/bin/oxlint");
        assert_eq!(c.get_args().collect::<Vec<_>>(), ["a"]);
        // Npx 拼 --yes <pkg>@latest 在前
        let c = command_for(
            &Resolved::Npx { npx: PathBuf::from("/bin/npx"), pkg: "oxlint@latest".into() },
            &["--check".into()],
        );
        assert_eq!(c.get_program(), "/bin/npx");
        assert_eq!(c.get_args().collect::<Vec<_>>(), ["--yes", "oxlint@latest", "--check"]);
    }
}
