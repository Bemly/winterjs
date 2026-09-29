//! 卸载（`--remove` 工程本地 / `--uninstall` 全局）：`add`/`install` 的对称逆操作。
//!
//! - 只动本地 state：`node_modules/<pkg>` 目录 + `.bin` 里指向它的链接 +
//!   `winterjs2-lock.json` 的 `packages` 条目；`package.json` 不碰（`add` 亦不写
//!   依赖段，对称；见 `install.rs`）。
//! - 原子性：先预检全部存在，任一缺失即整单报错不动盘；通过后再逐个删
//!   （删是幂等单包操作，半路失败报第几个，前面删掉的不回滚——npm 同款）。
//! - `--dry-run`：只打印 `would remove <name>@<version>`，不动盘。
//! - `.bin` 修剪：只删 symlink 目标落进被删包目录的；win 拷贝回退的无法溯源，
//!   留下（文档记录）。
//! - scope 包（`@s/p`）：删完包后父 `@s` 空即顺手删（npm 同款）。

use std::path::Path;

use crate::error::Error;

/// 包名校验（空/绝对/`..`/反斜杠/裸名含 `/` 即错；scope 形恰一 `/` 且 scope 首 `@`）。
fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("package name is empty".into());
    }
    let p = Path::new(name);
    if p.is_absolute() || name.contains('\\') {
        return Err(format!("bad package name '{name}'"));
    }
    if p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(format!("bad package name '{name}'"));
    }
    if let Some(rest) = name.strip_prefix('@') {
        match rest.split_once('/') {
            Some((scope, pkg)) => {
                if scope.is_empty() || pkg.is_empty() || pkg.contains('/') {
                    return Err(format!("bad scoped package name '{name}'"));
                }
            }
            None => return Err(format!("bad scoped package name '{name}'")),
        }
    } else if name.contains('/') {
        return Err(format!("bad package name '{name}'"));
    }
    Ok(())
}

/// lockfile 里记录的版本（无/坏文件/无条目 → `None`，打印时回落 `unknown`）。
fn locked_version(root: &Path, name: &str) -> Option<String> {
    let text = std::fs::read_to_string(root.join(crate::pm::install::LOCKFILE)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("packages")?.get(name)?.get("version")?.as_str().map(str::to_owned)
}

/// `winterjs2 remove/uninstall <pkgs> [--dry-run]`（`root` = 工程 cwd / 全局根）。
pub async fn remove_from(root: &Path, packages: &[String], dry_run: bool) -> Result<(), Error> {
    if packages.is_empty() {
        return Err(Error::Other("specify packages with -R/--remove".into()));
    }
    for name in packages {
        check_name(name).map_err(Error::Other)?;
    }
    let nm = root.join("node_modules");
    // 预检：任一缺失整单报错不动盘。
    let missing: Vec<&str> = packages
        .iter()
        .filter(|n| !nm.join(n).is_dir())
        .map(String::as_str)
        .collect();
    if !missing.is_empty() {
        return Err(Error::Other(format!(
            "package{} not installed: {}",
            if missing.len() == 1 { " is" } else { "s are" },
            missing.join(", ")
        )));
    }
    if dry_run {
        for name in packages {
            let ver = locked_version(root, name).unwrap_or_else(|| "unknown".into());
            println!("would remove {name}@{ver}");
        }
        return Ok(());
    }
    for name in packages {
        remove_one(root, &nm, name)?;
        println!("removed {name}");
    }
    Ok(())
}

/// 单包落地删（目录 + 空 scope 父 + `.bin` 溯源链接 + lockfile 条目）。
fn remove_one(root: &Path, nm: &Path, name: &str) -> Result<(), Error> {
    let dest = nm.join(name);
    std::fs::remove_dir_all(&dest)
        .map_err(|e| Error::Other(format!("cannot remove {}: {e}", dest.display())))?;
    // scope 父空即删（`@s/p` 删完 `p` 后 `@s` 空着也是垃圾）。
    if let Some((scope, _)) = name.split_once('/')
        && scope.starts_with('@')
    {
        let parent = nm.join(scope);
        if parent.is_dir() && std::fs::read_dir(&parent).is_ok_and(|mut it| it.next().is_none()) {
            let _ = std::fs::remove_dir(&parent);
        }
    }
    prune_bins(nm, &dest);
    prune_lockfile(root, name)?;
    Ok(())
}

/// `.bin` 修剪：symlink 目标落进 `dest` 的才删（相对目标按 `.bin` 为基展开）。
fn prune_bins(nm: &Path, dest: &Path) {
    let bindir = nm.join(".bin");
    let Ok(entries) = std::fs::read_dir(&bindir) else {
        return;
    };
    for entry in entries.flatten() {
        let link = entry.path();
        let Ok(target) = std::fs::read_link(&link) else {
            continue;
        };
        let abs = if target.is_absolute() {
            target
        } else {
            bindir.join(&target)
        };
        if abs.starts_with(dest) {
            let _ = std::fs::remove_file(&link);
            tracing::debug!(target: "winterjs2::pm", link = %link.display(), "pruned bin link");
        }
    }
}

/// lockfile 条目删除（无/坏文件当空表；`manifest` 指纹保留——`add` 亦不动它）。
fn prune_lockfile(root: &Path, name: &str) -> Result<(), Error> {
    let path = root.join(crate::pm::install::LOCKFILE);
    let mut lock = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .filter(|v| v.is_object())
        .unwrap_or_else(|| serde_json::json!({ "version": 1, "packages": {}, "manifest": null }));
    if let Some(pkgs) = lock.get_mut("packages").and_then(|v| v.as_object_mut()) {
        pkgs.remove(name);
    }
    let text = serde_json::to_string_pretty(&lock).map_err(Error::Json)?;
    crate::pm::cache::atomic_write(&path, text.as_bytes()).map_err(Error::Other)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 假工程：node_modules/{a,@s/p} + .bin/{ba→a,bp→@s/p,keep} + lockfile 三条目。
    fn fake_root() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let nm = dir.path().join("node_modules");
        std::fs::create_dir_all(nm.join("a")).unwrap();
        std::fs::write(nm.join("a/package.json"), r#"{"name":"a","bin":{"ba":"cli.js"}}"#).unwrap();
        std::fs::create_dir_all(nm.join("@s/p")).unwrap();
        let bin = nm.join(".bin");
        std::fs::create_dir_all(&bin).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("../a", bin.join("ba")).unwrap();
            std::os::unix::fs::symlink("../@s/p", bin.join("bp")).unwrap();
            std::os::unix::fs::symlink("../elsewhere", bin.join("keep")).unwrap();
        }
        #[cfg(not(unix))]
        {
            std::fs::write(bin.join("ba"), b"x").unwrap();
            std::fs::write(bin.join("bp"), b"x").unwrap();
            std::fs::write(bin.join("keep"), b"x").unwrap();
        }
        std::fs::create_dir_all(nm.join("elsewhere")).unwrap();
        std::fs::write(
            dir.path().join(crate::pm::install::LOCKFILE),
            r#"{"version":1,"packages":{"a":{"version":"1.0.0"},"@s/p":{"version":"2.0.0"},"elsewhere":{"version":"3.0.0"}},"manifest":null}"#,
        )
        .unwrap();
        dir
    }

    #[test]
    fn name_validation_table() {
        for good in ["a", "@s/p", "left-pad", "a.b_c~1"] {
            assert!(check_name(good).is_ok(), "{good}");
        }
        for bad in ["", "/abs", "../esc", "a/b", "@s", "@s/", "@/p", "@s/a/b", "a\\b"] {
            assert!(check_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn missing_is_all_or_nothing() {
        let dir = fake_root();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        // 缺失整单报错，且已存在的 a 不动盘。
        let err = rt
            .block_on(remove_from(dir.path(), &["a".into(), "nope".into()], false))
            .unwrap_err();
        assert!(err.to_string().contains("not installed: nope"), "{err}");
        assert!(dir.path().join("node_modules/a").is_dir());
        // dry-run 同样预检（不动盘）。
        let err = rt
            .block_on(remove_from(dir.path(), &["ghost".into()], true))
            .unwrap_err();
        assert!(err.to_string().contains("not installed: ghost"), "{err}");
    }

    #[test]
    fn remove_prunes_dir_bins_and_lockfile() {
        let dir = fake_root();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(remove_from(dir.path(), &["@s/p".into()], false)).unwrap();
        // 包目录 + 空 scope 父都没了；无关联包不动。
        assert!(!dir.path().join("node_modules/@s").exists());
        assert!(dir.path().join("node_modules/a").is_dir());
        let text = std::fs::read_to_string(dir.path().join(crate::pm::install::LOCKFILE)).unwrap();
        assert!(!text.contains("@s/p"), "{text}");
        assert!(text.contains("\"elsewhere\""), "{text}");
        #[cfg(unix)]
        {
            // 溯源链接 bp 删了，不相关 keep 留着。
            assert!(!dir.path().join("node_modules/.bin/bp").exists());
            assert!(dir.path().join("node_modules/.bin/keep").exists());
        }
    }

    #[test]
    fn dry_run_changes_nothing() {
        let dir = fake_root();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(remove_from(dir.path(), &["a".into()], true)).unwrap();
        assert!(dir.path().join("node_modules/a").is_dir());
        let text = std::fs::read_to_string(dir.path().join(crate::pm::install::LOCKFILE)).unwrap();
        assert!(text.contains("\"a\""), "{text}");
    }
}
