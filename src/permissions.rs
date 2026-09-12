//! `--allow-*` 权限开关（plan Phase 8-b）：opt-in 沙箱。
//!
//! 模型（Bun 同款）：不传任何 `--allow-*` → 全开放（行为与历史版本一致，
//! 141 例黑盒不受影响）；传了任一 `--allow-*`（或 `--allow-all`）→ 沙箱开启，
//! 未授权的类默认拒绝并给出可读错误（`PermissionError: ...`）。
//!
//! 落法：边界校验（fs/env/run/ffi/sqlite 各 native 入口检查）而非 cap-std 的
//! ambient-authority Dir 化——后者需重构 15+ 个 fs native，与收益不成比
//! （书面偏离记录 plan Phase 8-b；cap-std 依赖保留批准单内后续可用）。
//! 路径匹配在 canonicalize 之后做前缀包含（防 symlink 逃逸）；不存在的目标
//! 归一化到最近的存在祖先再匹配。纯匹配逻辑全部可单测。

use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// 一类权限的授权集。`None` = 旗标未给（沙箱关闭时无意义；沙箱内=拒绝该类）。
/// `Some(list)`：`list` 空 = 该类全开；非空 = 按 entries 匹配。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Grant {
    pub allowed: Option<Vec<String>>,
}

impl Grant {
    /// 旗标给了但没值（`--allow-read`）→ 全开。
    pub fn all() -> Grant {
        Grant { allowed: Some(Vec::new()) }
    }
    pub fn of(entries: Vec<String>) -> Grant {
        Grant { allowed: Some(entries) }
    }
    fn is_all(&self) -> bool {
        matches!(&self.allowed, Some(v) if v.is_empty())
    }
}

/// 权限集（CLI 解析结果）。
#[derive(Debug, Clone, Default)]
pub struct Permissions {
    pub read: Grant,
    pub write: Grant,
    pub env: Grant,
    pub run: Grant,
    pub ffi: bool,
    pub allow_all: bool,
}

impl Permissions {
    /// 默认：全开放（无沙箱）。
    pub fn open() -> Permissions {
        Permissions {
            read: Grant::all(),
            write: Grant::all(),
            env: Grant::all(),
            run: Grant::all(),
            ffi: true,
            allow_all: true,
        }
    }

    /// 是否进了沙箱（任一旗标出现）。
    pub fn sandboxed(&self) -> bool {
        self.allow_all
            || self.read.allowed.is_some()
            || self.write.allowed.is_some()
            || self.env.allowed.is_some()
            || self.run.allowed.is_some()
            || self.ffi
    }
}

static PERMS: RwLock<Option<Permissions>> = RwLock::new(None);

/// 安装（dispatch 早期调用一次；重复安装覆盖——test 子进程各装各的）。
pub fn install(p: Permissions) {
    if let Ok(mut slot) = PERMS.write() {
        *slot = Some(p);
    }
}

/// 读当前权限（未安装 = 全开放，模块单测/库用法）。
pub fn current() -> Permissions {
    PERMS
        .read()
        .ok()
        .and_then(|s| s.clone())
        .unwrap_or_else(Permissions::open)
}

// ── 检查（native 边界调用）────────────────────────────────────────────────

/// fs 读检查。Err = 可读的拒绝消息。
pub fn check_read(path: &str) -> Result<(), String> {
    let p = current();
    if !p.sandboxed() || p.allow_all || p.read.is_all() {
        return Ok(());
    }
    check_path(&p.read, path, "read", "--allow-read")
}

/// fs 写检查。
pub fn check_write(path: &str) -> Result<(), String> {
    let p = current();
    if !p.sandboxed() || p.allow_all || p.write.is_all() {
        return Ok(());
    }
    check_path(&p.write, path, "write", "--allow-write")
}

/// 环境变量检查。
pub fn check_env(name: &str) -> Result<(), String> {
    let p = current();
    if !p.sandboxed() || p.allow_all {
        return Ok(());
    }
    let Some(list) = &p.env.allowed else {
        return Err(format!(
            "PermissionError: access to environment variable '{name}' is not allowed (pass --allow-env)"
        ));
    };
    if list.is_empty() || list.iter().any(|n| n == name) {
        return Ok(());
    }
    Err(format!(
        "PermissionError: access to environment variable '{name}' is not allowed (pass --allow-env={name})"
    ))
}

/// 子进程检查（exec 字符串取首词做匹配）。
pub fn check_run(cmd: &str) -> Result<(), String> {
    let p = current();
    if !p.sandboxed() || p.allow_all {
        return Ok(());
    }
    let Some(list) = &p.run.allowed else {
        return Err(format!(
            "PermissionError: spawning '{cmd}' is not allowed (pass --allow-run)"
        ));
    };
    if list.is_empty() {
        return Ok(());
    }
    let base = cmd.split_whitespace().next().unwrap_or(cmd);
    let base = base.rsplit(['/', '\\']).next().unwrap_or(base);
    if list.iter().any(|c| c == base) {
        return Ok(());
    }
    Err(format!(
        "PermissionError: spawning '{cmd}' is not allowed (pass --allow-run={base})"
    ))
}

/// 环境变量枚举检查（部分授权时禁止全列）。
pub fn check_env_keys() -> Result<(), String> {
    let p = current();
    if !p.sandboxed() || p.allow_all {
        return Ok(());
    }
    match &p.env.allowed {
        Some(list) if list.is_empty() => Ok(()),
        _ => Err(
            "PermissionError: enumerating environment variables is not allowed (pass --allow-env)"
                .to_string(),
        ),
    }
}

/// FFI（dlopen）检查。
pub fn check_ffi() -> Result<(), String> {
    let p = current();
    if !p.sandboxed() || p.allow_all || p.ffi {
        return Ok(());
    }
    Err("PermissionError: FFI (dlopen) is not allowed (pass --allow-ffi)".to_string())
}

// ── 匹配逻辑（纯函数，单测覆盖）────────────────────────────────────────────

/// 路径归一化：相对路径锚 cwd；canonicalize 最近的存在的祖先（目标不存在时），
/// 防 symlink 逃逸；完全无祖先可归一（磁盘外路径）则原样。
fn normalize(p: &Path) -> PathBuf {
    let joined = if p.is_absolute() { p.to_path_buf() } else {
        std::env::current_dir().map(|c| c.join(p)).unwrap_or_else(|_| p.to_path_buf())
    };
    if let Ok(c) = std::fs::canonicalize(&joined) {
        return c;
    }
    // 逐级向上找最近存在的祖先
    let mut anc: Option<&Path> = joined.parent();
    while let Some(a) = anc {
        if let Ok(c) = std::fs::canonicalize(a) {
            return c.join(joined.strip_prefix(a).unwrap_or(&joined).to_path_buf());
        }
        anc = a.parent();
    }
    joined
}

fn check_path(grant: &Grant, path: &str, verb: &str, flag: &str) -> Result<(), String> {
    let Some(list) = &grant.allowed else {
        return Err(format!(
            "PermissionError: {verb} of '{path}' is not allowed (pass {flag} to enable)"
        ));
    };
    if list.is_empty() {
        return Ok(());
    }
    let target = normalize(Path::new(path));
    for root in list {
        let root_norm = normalize(Path::new(root));
        if target.starts_with(&root_norm) {
            return Ok(());
        }
    }
    Err(format!(
        "PermissionError: {verb} of '{path}' is not allowed (pass {flag}=<path> to allow it)"
    ))
}

/// CLI 旗标值 → Grant：无值/空串集合 = 全开；否则按 `,` 与多次出现聚合。
pub fn grant_from_values(values: Option<Vec<String>>) -> Grant {
    match values {
        None => Grant { allowed: None },
        Some(v) => {
            let items: Vec<String> = v.into_iter().filter(|s| !s.is_empty()).collect();
            Grant::of(items)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn perms(read: Grant, write: Grant, env: Grant, run: Grant, ffi: bool) -> Permissions {
        Permissions { read, write, env, run, ffi, allow_all: false }
    }

    /// 全局槽清回未安装（`None` = 全开放默认态）。`#[serial]` 只保互斥不保顺序，
    /// 读环境态的用例必须先复位，否则跑序一变即挂（2026-09-12 全量并行暴露）。
    fn reset() {
        if let Ok(mut slot) = PERMS.write() {
            *slot = None;
        }
    }

    #[test]
    #[serial]
    fn grant_semantics() {
        // 未装权限 = 全开放（先复位：别的 serial 用例可能先跑并装了沙箱）
        reset();
        assert!(check_read("/etc/passwd").is_ok());
        install(Permissions::open());
        assert!(check_read("/etc/passwd").is_ok());
        assert!(check_env("SECRET").is_ok());
        assert!(check_run("ls -la").is_ok());
        assert!(check_ffi().is_ok());
    }

    #[test]
    #[serial]
    fn sandbox_denies_undropped_classes() {
        // 只开 read → write/env/run/ffi 全拒
        install(perms(Grant::all(), Grant { allowed: None }, Grant { allowed: None }, Grant { allowed: None }, false));
        assert!(check_write("/tmp/x").is_err());
        assert!(check_env("HOME").is_err());
        assert!(check_run("ls").is_err());
        assert!(check_ffi().is_err());
        let msg = check_write("/tmp/x").unwrap_err();
        assert!(msg.contains("PermissionError") && msg.contains("--allow-write"), "{msg}");
    }

    #[test]
    #[serial]
    fn path_containment() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap().to_string();
        let inside = dir.path().join("a/b.txt").to_str().unwrap().to_string();
        let outside = dir.path().parent().unwrap().join("zz.txt").to_str().unwrap().to_string();
        install(perms(Grant::of(vec![root.clone()]), Grant::all(), Grant::all(), Grant::all(), true));
        assert!(check_read(&inside).is_ok());
        assert!(check_read(&root).is_ok());
        assert!(check_read(&outside).is_err());
        // 相对路径锚 cwd 归一后同样匹配
        let rel = std::path::Path::new(&root)
            .strip_prefix(std::env::current_dir().unwrap())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or(root.clone());
        if !rel.starts_with('/') {
            assert!(check_read(&format!("{rel}/x.txt")).is_ok());
        }
        // 不存在的目标归一化到存在祖先
        assert!(check_read(&format!("{root}/no/such/file.txt")).is_ok());
        assert!(check_read(&format!("{outside}/no/such.txt")).is_err());
    }

    #[test]
    #[serial]
    fn env_run_matching() {
        install(perms(
            Grant::all(),
            Grant::all(),
            Grant::of(vec!["HOME".into(), "PATH".into()]),
            Grant::of(vec!["node".into(), "git".into()]),
            false,
        ));
        assert!(check_env("HOME").is_ok());
        assert!(check_env("SECRET").is_err());
        assert!(check_run("node script.js").is_ok());
        assert!(check_run("/usr/bin/git status").is_ok());
        assert!(check_run("rm -rf /").is_err());
    }

    #[test]
    fn grant_from_cli_values() {
        assert_eq!(grant_from_values(None), Grant { allowed: None });
        assert_eq!(grant_from_values(Some(vec![])), Grant::all());
        assert_eq!(
            grant_from_values(Some(vec!["/a".into(), "".into(), "/b".into()])),
            Grant::of(vec!["/a".into(), "/b".into()])
        );
    }
}
