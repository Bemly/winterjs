//! 真装（plan Phase 5b/5c）：下载 → ssri 校验 → 暂存解包 → `node_modules` 落地 →
//! bin 链接 → lifecycle → lockfile。
//! - 5c 缓存：`cache` 内容寻址（有 integrity 按内容，无则按 URL），命中跳过下载；
//!   落盘 `tmp + rename` 原子，读损坏即驱逐当 miss。
//! - 5c 中断续传：暂存 `.staging-*` 同盘 `rename` 原子提交，kill -9 只留孤儿暂存，
//!   下次 `install_all` 开头清掉；`node_modules` 内坏包永不以正式名可见；
//!   lockfile/cache 同样原子写；`fs4` 独占锁串行化并发安装。
//! - 5c lifecycle：落地后按 `preinstall/install/postinstall` 跑 shell（见 `lifecycle`）。

use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::pm::resolve::Resolved;

/// lockfile 名（工程根）。
pub const LOCKFILE: &str = "winterjs-lock.json";

/// registry 树 + git 包一次装完（锁只持一次；lockfile 合并写）。
pub async fn install_all(
    root: &Path,
    tree: &[Resolved],
    git_specs: &[crate::pm::spec::GitSpec],
) -> Result<(), Error> {
    let nm = root.join("node_modules");
    std::fs::create_dir_all(&nm).map_err(|e| Error::Other(format!("cannot create node_modules: {e}")))?;
    cleanup_staging(&nm);
    // 并发串行化（`fs4` 独占锁；守卫持到函数尾，drop 即解锁）。
    let _lock = acquire_install_lock(&nm)?;
    let nm_bin = nm.join(".bin");
    for r in tree {
        install_one(&nm, &nm_bin, r).await?;
        println!("added {}@{}", r.name, r.version);
    }
    let mut git_locked: Vec<(String, String, String)> = Vec::with_capacity(git_specs.len());
    for g in git_specs {
        let (name, commit) = super::git::install_one_git(&nm, &nm_bin, g, root).await?;
        println!("added {name}@git+{}#{}", g.url, commit.chars().take(12).collect::<String>());
        git_locked.push((name, commit, g.url.clone()));
    }
    write_lockfile(root, tree, &git_locked)?;
    Ok(())
}

/// 开头清孤儿暂存（上次 kill -9 残留；只删 `.staging-*`，正式包不动）。
fn cleanup_staging(nm: &Path) {
    let Ok(entries) = std::fs::read_dir(nm) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(".staging-") {
            let p = entry.path();
            if p.is_dir() {
                let _ = std::fs::remove_dir_all(&p);
            } else {
                let _ = std::fs::remove_file(&p);
            }
            tracing::debug!(target: "winterjs::pm", staging = name.as_str(), "cleaned stale staging");
        }
        // 缓存/锁的 tmp 残留（`.tmp-*`）同样清理，防堆积。
        if name.starts_with(".tmp-") || name.ends_with(".tmp") {
            let p = entry.path();
            let _ = std::fs::remove_file(&p);
        }
    }
}

/// `node_modules/.install.lock` 独占锁（阻塞等；失败只记 warn 不中断，单机多装极罕见）。
fn acquire_install_lock(nm: &Path) -> Result<std::fs::File, Error> {
    let path = nm.join(".install.lock");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| Error::Other(format!("cannot open install lock: {e}")))?;
    // 文件锁轮子用 `fs4`（与 std::fs::File::lock 同源 flock/LockFileEx，此处显式走 fs4）。
    if let Err(e) = fs4::FileExt::lock(&file) {
        tracing::warn!(target: "winterjs::pm", "install lock busy, proceeding without: {e}");
    } else {
        tracing::debug!(target: "winterjs::pm", "install lock acquired");
    }
    Ok(file)
}

/// 单包：缓存命中则跳下载 → 校验 → 暂存解包 → 搬家 → bin → lifecycle。
async fn install_one(nm: &Path, nm_bin: &Path, r: &Resolved) -> Result<(), Error> {
    tracing::info!(target: "winterjs::pm", package = r.name.as_str(), version = r.version.as_str(), "installing");
    let bytes = fetch_bytes(r).await?;
    crate::pm::cache::verify_bytes(r.integrity.as_deref(), &bytes)
        .map_err(|e| Error::Other(format!("integrity check failed for {}@{}: {e}", r.name, r.version)))?;
    let dest = nm.join(&r.name);
    // 暂存目录（同盘 rename；`install-<pid>-<rand>` 避并发撞名）。
    let mut rand = [0u8; 4];
    getrandom::fill(&mut rand)
        .map_err(|e| Error::Other(format!("cannot get random values: {e}")))?;
    let staging = nm.join(format!(".staging-{}-{:x}", std::process::id(), u32::from_ne_bytes(rand)));
    let unpacked = unpack_tgz(&bytes, &staging)
        .map_err(|e| Error::Other(format!("cannot unpack {}@{}: {e}", r.name, r.version)))?;
    // package.json 名校验（ warn 级，不中断：registry 元信息为准）。
    if let Ok(text) = std::fs::read_to_string(unpacked.join("package.json")) {
        if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&text) {
            let nm_name = pkg.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if !nm_name.is_empty() && nm_name != r.name {
                tracing::warn!(target: "winterjs::pm", expected = r.name.as_str(), found = nm_name, "package.json name mismatch");
            }
        }
    }
    if dest.exists() {
        std::fs::remove_dir_all(&dest)
            .map_err(|e| Error::Other(format!("cannot clear {}: {e}", dest.display())))?;
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::Other(format!("cannot create scope dir: {e}")))?;
    }
    std::fs::rename(&unpacked, &dest)
        .map_err(|e| Error::Other(format!("cannot move into node_modules: {e}")))?;
    let _ = std::fs::remove_dir_all(&staging);
    link_bins(nm, &r.name, &dest)?;
    crate::pm::lifecycle::run_package_scripts(&dest, &r.name, &r.version, nm_bin).await?;
    Ok(())
}

/// 缓存优先取字节（有 integrity 才查缓存；命中即返；miss 则下载后回填）。
async fn fetch_bytes(r: &Resolved) -> Result<Vec<u8>, Error> {
    if r.integrity.is_some()
        && let Some(hit) = crate::pm::cache::get(&r.tarball, r.integrity.as_deref())
    {
        return Ok(hit);
    }
    tracing::debug!(target: "winterjs::pm", package = r.name.as_str(), "tarball cache miss");
    let bytes = download(&r.tarball).await?;
    // 回填失败吞掉（cache 侧已记 trace），不中断安装。
    crate::pm::cache::put(&r.tarball, r.integrity.as_deref(), &bytes);
    Ok(bytes)
}

/// tarball 下载（registry 同源 client；状态码非 2xx 即错）。
async fn download(url: &str) -> Result<Vec<u8>, Error> {
    let resp = super::registry::client().get(url).send().await.map_err(|e| Error::Other(format!("download failed for '{url}': {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(Error::Other(format!("download returned {status} for '{url}'")));
    }
    resp.bytes().await.map(|b| b.to_vec()).map_err(|e| Error::Other(format!("download failed for '{url}': {e}")))
}

/// tgz 解包到暂存（`package/` 包裹剥离；越界条目拒绝；返回包根）。
fn unpack_tgz(bytes: &[u8], staging: &Path) -> Result<PathBuf, String> {
    use flate2::read::GzDecoder;
    let gz = GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(gz);
    let entries = archive.entries().map_err(|e| format!("bad tarball: {e}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("bad tarball entry: {e}"))?;
        let path = entry.path().map_err(|e| format!("bad entry path: {e}"))?.into_owned();
        // 越界拒绝（绝对/`..`；`unpack_in` 二次兜底）。
        if path.is_absolute() || path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err(format!("tarball escapes staging: {}", path.display()));
        }
        // 父目录自建（部分包缺目录条目；`unpack_in` 不保证建父）。
        if !entry.header().entry_type().is_dir() {
            if let Some(parent) = staging.join(&path).parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("cannot stage dir: {e}"))?;
            }
        }
        entry.unpack_in(staging).map_err(|e| format!("unpack failed: {e}"))?;
    }
    let root = staging.join("package");
    if root.is_dir() {
        return Ok(root);
    }
    // 非标准布局：暂存内唯一顶层目录即包根（否则报错，文档记录）。
    let mut tops: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(staging).map_err(|e| format!("cannot list staging: {e}"))? {
        let entry = entry.map_err(|e| format!("cannot list staging: {e}"))?;
        tops.push(entry.path());
    }
    if tops.len() == 1 && tops[0].is_dir() {
        return Ok(tops.remove(0));
    }
    Err("tarball has no package/ root".into())
}

/// bin 链接（`node_modules/.bin/<name>` → `../<pkg>/<file>`；unix symlink，win 退拷贝）。
/// `pub(crate)`：git 包落地复用（`git.rs`）。
pub(crate) fn link_bins(nm: &Path, name: &str, dest: &Path) -> Result<(), Error> {
    let text = std::fs::read_to_string(dest.join("package.json")).unwrap_or_default();
    let pkg: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    let bins: Vec<(String, String)> = match pkg.get("bin") {
        Some(serde_json::Value::String(s)) => vec![(name.rsplit('/').next().unwrap_or(name).to_string(), s.clone())],
        Some(serde_json::Value::Object(map)) => {
            map.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect()
        }
        _ => Vec::new(),
    };
    if bins.is_empty() {
        return Ok(());
    }
    let bindir = nm.join(".bin");
    std::fs::create_dir_all(&bindir).map_err(|e| Error::Other(format!("cannot create .bin: {e}")))?;
    for (bin_name, rel) in bins {
        let target = dest.join(&rel);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            if target.is_file() {
                let mut perm = std::fs::metadata(&target)
                    .map(|m| m.permissions())
                    .unwrap_or_else(|_| std::fs::Permissions::from_mode(0o755));
                perm.set_mode(0o755);
                let _ = std::fs::set_permissions(&target, perm);
            }
        }
        let link = bindir.join(&bin_name);
        let _ = std::fs::remove_file(&link);
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&target, &link).is_ok();
        #[cfg(not(unix))]
        let linked = false;
        if !linked {
            // win/无权限回退拷贝（文档记录；`.cmd` 垫片顺延）。
            if let Err(e) = std::fs::copy(&target, &link) {
                return Err(Error::Other(format!("cannot link bin '{bin_name}': {e}")));
            }
        }
    }
    Ok(())
}

/// lockfile 写（`{version:1, packages:{name:{version,resolved,integrity}}}`，原子）。
/// git 行：`version` 记 commit 全 hex，`resolved` 记 `git+<url>#<commit>`，无 integrity。
fn write_lockfile(
    root: &Path,
    tree: &[Resolved],
    git: &[(String, String, String)],
) -> Result<(), Error> {
    let mut packages = serde_json::Map::new();
    for r in tree {
        packages.insert(
            r.name.clone(),
            serde_json::json!({
                "version": r.version,
                "resolved": r.tarball,
                "integrity": r.integrity,
            }),
        );
    }
    for (name, commit, url) in git {
        packages.insert(
            name.clone(),
            serde_json::json!({
                "version": commit,
                "resolved": format!("git+{url}#{commit}"),
                "integrity": serde_json::Value::Null,
            }),
        );
    }
    let lock = serde_json::json!({ "version": 1, "packages": packages });
    let text = serde_json::to_string_pretty(&lock).map_err(Error::Json)?;
    crate::pm::cache::atomic_write(&root.join(LOCKFILE), text.as_bytes())
        .map_err(Error::Other)?;
    Ok(())
}
