//! 转译缓存：`blake3` 内容 key → `postcard` blob；`lru` 内存层 + 磁盘层。
//!
//! key 指纹 = 前缀版本（oxc/选项变更时 bump）+ 本包版本 + 后缀 + 源码。
//! 缓存 IO 失败一律当 miss（正确性优先，只记 trace）。
//! 磁盘目录：`$WINTERJS_CACHE` > 系统缓存目录 `winterjs/modules`；都没有则只用内存。

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::LazyLock;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// bump 条件：oxc 升级、转译选项变更、blob 结构变更。
const CACHE_PREFIX: &str = "winterjs-modcache-v1";
const MEM_CAP: usize = 128;

#[derive(Serialize, Deserialize)]
struct Blob {
    v: u32,
    js: String,
    imports: Vec<String>,
    is_module: bool,
    map: Option<String>,
}

pub struct Cached {
    pub js: String,
    pub imports: Vec<String>,
    pub is_module: bool,
    pub map: Option<String>,
}

static MEM: LazyLock<Mutex<lru::LruCache<String, Blob>>> =
    LazyLock::new(|| Mutex::new(lru::LruCache::new(NonZeroUsize::new(MEM_CAP).unwrap())));

fn disk_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("WINTERJS_CACHE") {
        return Some(PathBuf::from(d));
    }
    dirs::cache_dir().map(|d| d.join("winterjs").join("modules"))
}

fn key(source: &str, ext: &str) -> String {
    let mut h = blake3::Hasher::new();
    h.update(CACHE_PREFIX.as_bytes());
    h.update(&[0]);
    h.update(env!("CARGO_PKG_VERSION").as_bytes());
    h.update(&[0]);
    h.update(ext.as_bytes());
    h.update(&[0]);
    h.update(source.as_bytes());
    h.finalize().to_hex().to_string()
}

fn decode(bytes: &[u8]) -> Option<Cached> {
    let blob: Blob = postcard::from_bytes(bytes).ok()?;
    if blob.v != 1 {
        return None;
    }
    Some(Cached { js: blob.js, imports: blob.imports, is_module: blob.is_module, map: blob.map })
}

pub fn get(source: &str, ext: &str) -> Option<Cached> {
    let k = key(source, ext);
    if let Some(b) = MEM.lock().get(&k) {
        if b.v != 1 {
            return None;
        }
        tracing::trace!(target: "winterjs::loader", "transpile cache memory hit");
        return Some(Cached {
            js: b.js.clone(),
            imports: b.imports.clone(),
            is_module: b.is_module,
            map: b.map.clone(),
        });
    }
    let dir = disk_dir()?;
    let bytes = fs_err::read(dir.join(format!("{k}.postcard"))).ok()?;
    let cached = decode(&bytes)?;
    tracing::debug!(target: "winterjs::loader", "transpile cache disk hit");
    MEM.lock().put(
        k,
        Blob {
            v: 1,
            js: cached.js.clone(),
            imports: cached.imports.clone(),
            is_module: cached.is_module,
            map: cached.map.clone(),
        },
    );
    Some(cached)
}

pub fn put(source: &str, ext: &str, cached: &Cached) {
    let blob = Blob {
        v: 1,
        js: cached.js.clone(),
        imports: cached.imports.clone(),
        is_module: cached.is_module,
        map: cached.map.clone(),
    };
    let k = key(source, ext);
    let bytes = match postcard::to_stdvec(&blob) {
        Ok(b) => b,
        Err(e) => {
            tracing::trace!(target: "winterjs::loader", "transpile cache encode skip: {e}");
            return;
        }
    };
    MEM.lock().put(k.clone(), blob);
    let Some(dir) = disk_dir() else { return };
    if fs_err::create_dir_all(&dir).is_err() {
        return;
    }
    if fs_err::write(dir.join(format!("{k}.postcard")), &bytes).is_err() {
        tracing::trace!(target: "winterjs::loader", "transpile cache disk write skip");
    }
}
