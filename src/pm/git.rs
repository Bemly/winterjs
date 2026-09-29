//! git 依赖（plan Phase 5d-d2）：`[<name>@]git+<url>[#<rev>]`。
//!
//! - 本地（`file://` 或无 scheme 的本地路径）：`gix` 轮子 open + rev-parse，
//!   拷贝 worktree（除 `.git`）进暂存；不需要 git 二进制。
//! - 远端（`https://`/`ssh://`/`git://` 等）：`gix` 默认特性无网络客户端
//!   （补 `blocking-network-client` 会拖 transport/tokio，见 `dependencies.md`
//!   附记），此处走 `git` CLI 浅克隆（构建期 vergen-gitcl 同款前例）；
//!   缺 git 二进制即报可读错。
//! - dry-run：本地解析出 commit 后打印，远端无网络直接打印所求 rev
//!   （裸远端 spec 读不到名，要求显式名，报错指路）。
//! - 落地后与 tarball 同待遇：bin 链接 + lifecycle（复用 `install` 侧 helpers）。

use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::pm::spec::GitSpec;

/// dry-run 展示行（`name@git+url#rev_or_commit`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitDisplay {
    pub name: String,
    pub url: String,
    pub rev: String,
}

/// 是否本地源（`file://` 或根本无 scheme）。
pub fn is_local(url: &str) -> bool {
    url.starts_with("file://") || !url.contains("://")
}

/// 本地路径（`file://` 去前缀；相对路径按 `cwd` 解）。
pub fn local_path(url: &str, cwd: &Path) -> PathBuf {
    if let Some(rest) = url.strip_prefix("file://") {
        // `file://localhost/path` 容错（npm 写法）。
        let rest = rest.strip_prefix("localhost").unwrap_or(rest);
        return PathBuf::from(rest);
    }
    let p = PathBuf::from(url);
    if p.is_absolute() { p } else { cwd.join(p) }
}

/// 所求 rev（缺省 `HEAD`）。
pub fn wanted_rev(spec: &GitSpec) -> &str {
    spec.rev.as_deref().unwrap_or("HEAD")
}

/// `gix` rev-parse → commit hex（纯本地，无网络）。
fn parse_commit(repo_path: &Path, rev: &str) -> Result<String, String> {
    let repo = gix::open(repo_path).map_err(|e| format!("cannot open git repo '{}': {e}", repo_path.display()))?;
    Ok(repo
        .rev_parse_single(rev)
        .map_err(|e| format!("unknown revision '{rev}': {e}"))?
        .to_string())
}

/// 包版本（staging 的 package.json；缺失即错，由调用方回落）。
fn package_version(dir: &Path) -> Result<String, String> {
    let text =
        std::fs::read_to_string(dir.join("package.json")).map_err(|e| format!("git package has no package.json: {e}"))?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("bad package.json in git package: {e}"))?;
    v.get("version")
        .and_then(|n| n.as_str())
        .filter(|n| !n.is_empty())
        .map(|n| n.to_owned())
        .ok_or_else(|| "git package.json has no version".to_string())
}

/// 包名（staging/源目录的 package.json；缺失即错）。
fn package_name(dir: &Path) -> Result<String, String> {
    let text =
        std::fs::read_to_string(dir.join("package.json")).map_err(|e| format!("git package has no package.json: {e}"))?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("bad package.json in git package: {e}"))?;
    v.get("name")
        .and_then(|n| n.as_str())
        .filter(|n| !n.is_empty())
        .map(|n| n.to_owned())
        .ok_or_else(|| "git package.json has no name (use '<name>@git+…' form)".to_string())
}

/// dry-run 解析（本地走 `gix` 出 commit；远端回显所求 rev）。
pub fn resolve_for_dry_run(spec: &GitSpec, cwd: &Path) -> Result<GitDisplay, String> {
    if is_local(&spec.url) {
        let path = local_path(&spec.url, cwd);
        let rev = wanted_rev(spec);
        let commit = parse_commit(&path, rev)?;
        let name = match &spec.name {
            Some(n) => n.clone(),
            None => package_name(&path)?,
        };
        Ok(GitDisplay { name, url: spec.url.clone(), rev: commit })
    } else {
        let Some(name) = spec.name.clone() else {
            return Err(format!(
                "bare remote git spec 'git+{}' needs an explicit name ('<name>@git+…'; dry-run avoids network)",
                spec.url
            ));
        };
        Ok(GitDisplay { name, url: spec.url.clone(), rev: wanted_rev(spec).to_owned() })
    }
}

/// 落地一行（返回 `(name, commit)` 供 lockfile；远端 commit 克隆后才知）。
/// 调用方（`install_all`）已持 `fs4` 锁并清过暂存。
pub(crate) async fn install_one_git(
    nm: &Path,
    nm_bin: &Path,
    spec: &GitSpec,
    cwd: &Path,
) -> Result<(String, String), Error> {
    tracing::info!(target: "winterjs2::pm", url = spec.url.as_str(), "installing git package");
    let mut rand = [0u8; 4];
    getrandom::fill(&mut rand)
        .map_err(|e| Error::Other(format!("cannot get random values: {e}")))?;
    let staging = nm.join(format!(".staging-git-{}-{:x}", std::process::id(), u32::from_ne_bytes(rand)));
    // 源进暂存（本地拷贝 / 远端克隆），顺带拿 commit。
    let commit = if is_local(&spec.url) {
        let path = local_path(&spec.url, cwd);
        match stage_local(&path, wanted_rev(spec), &staging) {
            Ok(c) => c,
            // 裸仓/打不开：回落 `git clone`（file:// 天然支持，含 bare 源）。
            Err(e) => {
                tracing::debug!(target: "winterjs2::pm", "local gix path failed, trying git clone: {e}");
                clone_remote(&path.to_string_lossy(), spec.rev.as_deref(), &staging).await?;
                parse_commit(&staging, "HEAD").map_err(Error::Other)?
            }
        }
    } else {
        clone_remote(&spec.url, spec.rev.as_deref(), &staging).await?;
        parse_commit(&staging, "HEAD").map_err(Error::Other)?
    };
    // 名：显式优先，否则读包；不一致 warn（registry 侧同语义）。
    let staged_name = package_name(&staging).map_err(Error::Other)?;
    let name = spec.name.clone().unwrap_or_else(|| staged_name.clone());
    if name != staged_name {
        tracing::warn!(target: "winterjs2::pm", expected = name.as_str(), found = staged_name.as_str(), "package.json name mismatch");
    }
    let dest = nm.join(&name);
    if dest.exists() {
        std::fs::remove_dir_all(&dest)
            .map_err(|e| Error::Other(format!("cannot clear {}: {e}", dest.display())))?;
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::Other(format!("cannot create scope dir: {e}")))?;
    }
    std::fs::rename(&staging, &dest)
        .map_err(|e| Error::Other(format!("cannot move into node_modules: {e}")))?;
    super::install::link_bins(nm, &name, &dest)?;
    // version 取包自述（读不到退 commit 短头；仅 lifecycle env 用）。
    let version = package_version(&dest).unwrap_or_else(|_| commit.chars().take(12).collect());
    super::lifecycle::run_package_scripts(&dest, &name, &version, nm_bin).await?;
    Ok((name.clone(), commit))
}

/// 本地 worktree 拷进暂存（除 `.git`；返回 rev 的 commit）。
/// `repo_path` 须为工作仓（bare 走调用方回落）。
fn stage_local(repo_path: &Path, rev: &str, staging: &Path) -> Result<String, String> {
    let repo = gix::open(repo_path).map_err(|e| format!("cannot open git repo '{}': {e}", repo_path.display()))?;
    if repo.is_bare() {
        return Err("bare repository (needs clone)".into());
    }
    let commit = repo
        .rev_parse_single(rev)
        .map_err(|e| format!("unknown revision '{rev}': {e}"))?
        .to_string();
    copy_worktree(repo_path, staging)?;
    Ok(commit)
}

/// worktree 拷贝（`.git` 顶层目录跳过；symlink 按 unix 重建，win 退拷内容）。
fn copy_worktree(src: &Path, dst: &Path) -> Result<(), String> {
    for entry in walkdir::WalkDir::new(src).min_depth(1) {
        let entry = entry.map_err(|e| format!("cannot walk git worktree: {e}"))?;
        let rel = entry.path().strip_prefix(src).map_err(|e| format!("bad git path: {e}"))?;
        if rel.components().next().is_some_and(|c| c.as_os_str() == ".git") {
            continue;
        }
        let target = dst.join(rel);
        let ft = entry.file_type();
        if ft.is_dir() {
            std::fs::create_dir_all(&target).map_err(|e| format!("cannot stage dir: {e}"))?;
        } else if ft.is_symlink() {
            let link = std::fs::read_link(entry.path()).map_err(|e| format!("cannot read link: {e}"))?;
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("cannot stage dir: {e}"))?;
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(&link, &target).map_err(|e| format!("cannot stage link: {e}"))?;
            #[cfg(not(unix))]
            {
                // win 退拷内容（垫片顺延，文档记录）。
                let resolved = entry.path().parent().unwrap_or(src).join(&link);
                std::fs::copy(&resolved, &target).map_err(|e| format!("cannot stage link target: {e}"))?;
            }
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("cannot stage dir: {e}"))?;
            }
            std::fs::copy(entry.path(), &target).map_err(|e| format!("cannot stage file: {e}"))?;
        }
    }
    Ok(())
}

/// 远端克隆（`git clone --quiet <url> <dst>` + 可选 `checkout <rev>`；tokio 异步等）。
async fn clone_remote(url: &str, rev: Option<&str>, dst: &Path) -> Result<(), Error> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("clone").arg("--quiet").arg(url).arg(dst);
    cmd.kill_on_drop(true);
    let out = cmd.output().await.map_err(|e| {
        Error::Other(format!("git clone failed for '{url}': cannot run git ({e}; is git installed?)"))
    })?;
    if !out.status.success() {
        let tail = String::from_utf8_lossy(&out.stderr);
        let tail: String = tail.lines().last().unwrap_or("clone failed").chars().take(200).collect();
        return Err(Error::Other(format!("git clone failed for '{url}': {}", tail.trim())));
    }
    if let Some(rev) = rev {
        let mut cmd = tokio::process::Command::new("git");
        cmd.arg("-C").arg(dst).arg("checkout").arg("--quiet").arg(rev);
        cmd.kill_on_drop(true);
        let out = cmd.output().await.map_err(|e| Error::Other(format!("git checkout failed: {e}")))?;
        if !out.status.success() {
            let tail = String::from_utf8_lossy(&out.stderr);
            let tail: String = tail.lines().last().unwrap_or("checkout failed").chars().take(200).collect();
            return Err(Error::Other(format!("git checkout '{rev}' failed: {}", tail.trim())));
        }
    }
    // 克隆自带的 `.git` 不进 node_modules（tarball 无此物，对齐）。
    let _ = std::fs::remove_dir_all(dst.join(".git"));
    tracing::debug!(target: "winterjs2::pm", url, rev = rev.unwrap_or("HEAD"), "git cloned");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 现场建 git 仓（`git` CLI；`user.*` 经 `-c` 注入，不碰全局配置）。
    fn fixture_repo(tagged: bool) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let mut c = std::process::Command::new("git");
            c.args(args).current_dir(dir.path());
            c.env("GIT_CONFIG_NOSYSTEM", "1");
            let out = c.output().expect("git runs");
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        std::fs::write(dir.path().join("package.json"), r#"{"name":"git-pkg","version":"0.1.0"}"#).unwrap();
        std::fs::write(dir.path().join("index.js"), b"exports.v = 7;\n").unwrap();
        git(&["init", "-q", "-b", "main"]);
        git(&["add", "-A"]);
        git(&["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "init"]);
        if tagged {
            git(&["tag", "v1.0.0"]);
        }
        dir
    }

    #[test]
    fn dry_run_local_resolves_commit() {
        let repo = fixture_repo(true);
        let url = format!("file://{}", repo.path().display());
        let spec = GitSpec { name: None, url: url.clone(), rev: Some("v1.0.0".into()) };
        let d = resolve_for_dry_run(&spec, Path::new("/")).unwrap();
        // commit 即 tag 所指（40 hex）。
        assert_eq!(d.name, "git-pkg");
        assert_eq!(d.url, url);
        assert_eq!(d.rev.len(), 40, "rev: {}", d.rev);
        // 缺省 HEAD 同 HEAD。
        let head = resolve_for_dry_run(
            &GitSpec { name: Some("x".into()), url: url.clone(), rev: None },
            Path::new("/"),
        )
        .unwrap();
        assert_eq!(head.rev.len(), 40);
    }

    #[test]
    fn dry_run_errors_unknown_rev_and_bare_remote() {
        let repo = fixture_repo(false);
        let url = format!("file://{}", repo.path().display());
        assert!(resolve_for_dry_run(
            &GitSpec { name: None, url: url.clone(), rev: Some("no-such-ref".into()) },
            Path::new("/"),
        )
        .is_err());
        // 裸远端 dry-run 读不到名（要联网），必须显式名。
        assert!(resolve_for_dry_run(
            &GitSpec { name: None, url: "https://h/r.git".into(), rev: None },
            Path::new("/"),
        )
        .is_err());
        let d = resolve_for_dry_run(
            &GitSpec { name: Some("r".into()), url: "https://h/r.git".into(), rev: None },
            Path::new("/"),
        )
        .unwrap();
        assert_eq!((d.name.as_str(), d.rev.as_str()), ("r", "HEAD"));
    }
}
