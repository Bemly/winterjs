//! 真装（plan Phase 5b）：下载 → ssri 校验 → 暂存解包 → `node_modules` 落地 →
//! bin 链接 → lockfile。失败止于 rename 之前（坏包不进 `node_modules`）；
//! 中断续传/缓存命中顺延 5c（此处每次全量重下，文档记录）。

use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::pm::resolve::Resolved;

/// lockfile 名（工程根）。
pub const LOCKFILE: &str = "winterjs-lock.json";

/// 安装一棵解树到 `root/node_modules`（顺序执行；并发顺延 5c）。
pub async fn install_tree(root: &Path, tree: &[Resolved]) -> Result<(), Error> {
    let nm = root.join("node_modules");
    std::fs::create_dir_all(&nm).map_err(|e| Error::Other(format!("cannot create node_modules: {e}")))?;
    for r in tree {
        install_one(&nm, r).await?;
        println!("added {}@{}", r.name, r.version);
    }
    write_lockfile(root, tree)?;
    Ok(())
}

/// 单包：下载 → 校验 → 暂存解包 → 搬家 → bin。
async fn install_one(nm: &Path, r: &Resolved) -> Result<(), Error> {
    tracing::info!(target: "winterjs::pm", package = r.name.as_str(), version = r.version.as_str(), "installing");
    let bytes = download(&r.tarball).await?;
    verify_bytes(r.integrity.as_deref(), &bytes)
        .map_err(|e| Error::Other(format!("integrity check failed for {}@{}: {e}", r.name, r.version)))?;
    let dest = nm.join(&r.name);
    // 暂存目录（同盘 rename；`install-<pid>-<rand>` 避并发撞名，5c 再收编）。
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
    Ok(())
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

/// 完整性校验（integrity 优先 ssri；退 shasum sha1；皆无即错）。
fn verify_bytes(integrity: Option<&str>, bytes: &[u8]) -> Result<(), String> {
    match integrity {
        Some(sri) if sri.starts_with("sha") => {
            let parsed: ssri::Integrity = sri.parse().map_err(|e| format!("bad integrity '{sri}': {e}"))?;
            ssri::IntegrityChecker::new(parsed)
                .chain(bytes)
                .result()
                .map(|_| ())
                .map_err(|e| format!("integrity mismatch: {e}"))
        }
        Some(shasum) => {
            use sha1::Digest as _;
            let hex = const_hex::encode(sha1::Sha1::digest(bytes));
            if hex.eq_ignore_ascii_case(shasum.trim()) {
                Ok(())
            } else {
                Err("shasum mismatch".into())
            }
        }
        _ => Err("no integrity metadata (needs slice 5b strict)".into()),
    }
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
fn link_bins(nm: &Path, name: &str, dest: &Path) -> Result<(), Error> {
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

/// lockfile 写（`{version:1, packages:{name:{version,resolved,integrity}}}`）。
fn write_lockfile(root: &Path, tree: &[Resolved]) -> Result<(), Error> {
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
    let lock = serde_json::json!({ "version": 1, "packages": packages });
    let text = serde_json::to_string_pretty(&lock).map_err(Error::Json)?;
    std::fs::write(root.join(LOCKFILE), text).map_err(|e| Error::Other(format!("cannot write lockfile: {e}")))?;
    Ok(())
}
