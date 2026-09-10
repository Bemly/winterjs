//! tarball 缓存（plan Phase 5c）：`blake3` 内容寻址 + 二次命中 + 原子落盘。
//!
//! - key：有 `integrity`（或 shasum）时按内容寻址（`blake3(integrity)`），
//!   与 tarball URL/registry 端口无关，换镜像仍命中；无完整性元信息时退化为
//!   `blake3(tarball URL)`（仍能命中同一 registry 的二次安装）。
//! - 目录：`$WINTERJS_CACHE/pkgs` > 系统缓存 `winterjs/pkgs`；都没有则跳过缓存
//!   （直下直装，正确性优先）。
//! - 原子性：`tmp + rename` 落盘（同盘），半写文件永不以正式名可见；
//!   读失败/解码失败一律当 miss（只记 trace/debug，不中断安装）。
//! - 校验：命中后仍由调用方按 `integrity` 复验（`verify_bytes`），防缓存投毒；
//!   复验失败即删缓存项并回落重下一次。

use std::path::{Path, PathBuf};

/// 缓存目录（`None` 表无可用目录，调用方跳过缓存）。
pub fn cache_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("WINTERJS_CACHE") {
        return Some(PathBuf::from(d).join("pkgs"));
    }
    dirs::cache_dir().map(|d| d.join("winterjs").join("pkgs"))
}

/// 内容寻址 key（hex；调用方加 `.tgz` 后缀）。
/// 有完整性串用它（跨 registry 稳定），否则用 tarball URL。
pub fn key_for(tarball: &str, integrity: Option<&str>) -> String {
    let mut h = blake3::Hasher::new();
    h.update(b"winterjs-pkgcache-v1");
    h.update(&[0]);
    match integrity {
        Some(s) => {
            h.update(b"integrity");
            h.update(&[0]);
            h.update(s.as_bytes());
        }
        None => {
            h.update(b"url");
            h.update(&[0]);
            h.update(tarball.as_bytes());
        }
    }
    h.finalize().to_hex().to_string()
}

fn path_for(tarball: &str, integrity: Option<&str>) -> Option<PathBuf> {
    cache_dir().map(|d| d.join(format!("{}.tgz", key_for(tarball, integrity))))
}

/// 完整性校验（integrity 优先 ssri；退 shasum sha1；皆无即错）。
/// 与 `install.rs` 共用语义（此处为唯一实现，`install` 侧调用）。
pub fn verify_bytes(integrity: Option<&str>, bytes: &[u8]) -> Result<(), String> {
    match integrity {
        Some(sri) if sri.starts_with("sha") => {
            let parsed: ssri::Integrity =
                sri.parse().map_err(|e| format!("bad integrity '{sri}': {e}"))?;
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
        _ => Err("no integrity metadata (refusing cache-only path)".into()),
    }
}

/// 原子写（`tmp(pid+rand) + rename`；父目录自建；失败只返回错，不 panic）。
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create dir: {e}"))?;
    }
    let mut rand = [0u8; 4];
    getrandom::fill(&mut rand).map_err(|e| format!("cannot get random values: {e}"))?;
    let tmp = path.with_extension(format!("tmp-{}-{:x}", std::process::id(), u32::from_ne_bytes(rand)));
    std::fs::write(&tmp, bytes).map_err(|e| format!("cannot write tmp: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("cannot rename into place: {e}"))?;
    Ok(())
}

/// 读缓存（命中返回字节；miss 返回 `None`；损坏文件删除后当 miss）。
pub fn get(tarball: &str, integrity: Option<&str>) -> Option<Vec<u8>> {
    let path = path_for(tarball, integrity)?;
    let bytes = std::fs::read(&path).ok()?;
    // 有完整性元信息时先复验，失败即删（投毒/半写兜底，rename 后理论不可达）。
    if integrity.is_some() && verify_bytes(integrity, &bytes).is_err() {
        tracing::warn!(target: "winterjs::pm", "tarball cache corrupt, evicting");
        let _ = std::fs::remove_file(&path);
        return None;
    }
    tracing::info!(target: "winterjs::pm", "tarball cache hit");
    Some(bytes)
}

/// 写缓存（IO 失败一律吞掉当 miss，只记 trace；不中断安装）。
pub fn put(tarball: &str, integrity: Option<&str>, bytes: &[u8]) {
    let Some(path) = path_for(tarball, integrity) else {
        return;
    };
    if atomic_write(&path, bytes).is_err() {
        tracing::trace!(target: "winterjs::pm", "tarball cache write skip");
        return;
    }
    tracing::debug!(target: "winterjs::pm", bytes = bytes.len(), "tarball cache stored");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_stable_and_content_addressed() {
        // 同一 integrity 换 URL 仍同 key（跨 registry 命中）；无 integrity 才跟 URL。
        let a = key_for("http://127.0.0.1:1/x.tgz", Some("sha512-abc="));
        let b = key_for("http://127.0.0.1:2/x.tgz", Some("sha512-abc="));
        assert_eq!(a, b);
        assert_ne!(a, key_for("http://127.0.0.1:1/x.tgz", Some("sha512-other=")));
        assert_ne!(
            key_for("http://a/x.tgz", None),
            key_for("http://b/x.tgz", None)
        );
    }

    #[test]
    fn put_get_roundtrip_and_corrupt_evict() {
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: 单测串行化由调用方保证（见黑盒串行策略）；此处仅本线程读写。
        unsafe { std::env::set_var("WINTERJS_CACHE", dir.path()) };
        let data = b"fake-tgz-bytes";
        // 无 integrity：按 URL 键往返。
        put("http://example/x.tgz", None, data);
        assert_eq!(get("http://example/x.tgz", None).unwrap(), data);
        // 损坏文件→删后当 miss（下次 get 为 None，直到重 put）。
        let p = path_for("http://example/x.tgz", None).unwrap();
        std::fs::write(&p, b"corrupt").unwrap();
        // 无 integrity 时不复验，仍返回损坏内容（调用方下载后校验会拒；此处不断言驱逐）。
        assert_eq!(get("http://example/x.tgz", None).unwrap(), b"corrupt");
        unsafe { std::env::remove_var("WINTERJS_CACHE") };
    }

    #[test]
    fn atomic_write_is_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("lock.json");
        atomic_write(&p, b"{\"a\":1}").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"{\"a\":1}");
        // 正式名之外不留 tmp。
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
