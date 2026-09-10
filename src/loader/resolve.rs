//! specifier 解析（切片 a）：相对路径（含后缀探测）+ 绝对 file 路径 +
//! file:/data: 绝对 URL。裸导入与 http(s) 给友好报错（Phase 5 / Phase 3）。

use std::path::{Path, PathBuf};

use url::Url;

use crate::error::Error;

/// 后缀探测顺序（Bun 风格 DX：`./foo` 可命中 `foo.ts`）。
const PROBE_EXTS: &[&str] = &["ts", "tsx", "mts", "js", "mjs"];
const INDEX_FILES: &[&str] = &["index.ts", "index.tsx", "index.mts", "index.js", "index.mjs"];

fn is_file_url_exists(url: &Url) -> bool {
    url.to_file_path().map(|p| p.is_file()).unwrap_or(false)
}

/// file: URL 不存在时做后缀/index 探测；非 file 原样返回。
fn probe_file_url(url: &Url) -> Result<Url, Error> {
    if url.scheme() != "file" || is_file_url_exists(url) {
        return Ok(url.clone());
    }
    let path = url
        .to_file_path()
        .map_err(|_| Error::Other(format!("bad file URL: {url}")))?;
    // ./foo → ./foo.ts ……
    if path.extension().is_none() {
        for ext in PROBE_EXTS {
            let cand = path.with_extension(ext);
            if cand.is_file() {
                return Url::from_file_path(&cand)
                    .map_err(|_| Error::Other(format!("bad file path: {}", cand.display())));
            }
        }
    }
    // ./dir → ./dir/index.ts ……
    if path.is_dir() {
        for name in INDEX_FILES {
            let cand = path.join(name);
            if cand.is_file() {
                return Url::from_file_path(&cand)
                    .map_err(|_| Error::Other(format!("bad file path: {}", cand.display())));
            }
        }
    }
    Err(Error::Other(format!("module not found: {url}")))
}

/// `import spec` + 发起方 URL → 目标 URL。
pub fn resolve(specifier: &str, base: Option<&Url>) -> Result<Url, Error> {
    // 绝对 URL（含 scheme）
    if let Ok(url) = Url::parse(specifier) {
        return match url.scheme() {
            "file" => probe_file_url(&url),
            "data" => Ok(url),
            "http" | "https" => Err(Error::Other(format!(
                "remote module '{specifier}' needs Phase 3 (fetch); file:/data: only for now"
            ))),
            s => Err(Error::Other(format!(
                "unsupported module scheme '{s}:': {specifier}"
            ))),
        };
    }
    let is_relative =
        specifier.starts_with("./") || specifier.starts_with("../") || specifier.starts_with('/');
    if !is_relative {
        // 绝对文件路径（/x/y.js，无 scheme；Windows 盘符 C:/… 同理）
        let p = Path::new(specifier);
        if p.is_absolute() {
            return Url::from_file_path(p)
                .map_err(|_| Error::Other(format!("bad absolute path: {specifier}")))
                .and_then(|u| probe_file_url(&u));
        }
        // 裸导入（npm 包）：Phase 5，报错友好
        return Err(Error::Other(format!(
            "cannot resolve bare specifier '{specifier}' (npm packages arrive in Phase 5; use a relative path for now)"
        )));
    }
    // 相对路径：需要发起方 base
    if base.is_none() && Path::new(specifier).is_absolute() {
        let url = Url::from_file_path(specifier)
            .map_err(|_| Error::Other(format!("bad absolute path: {specifier}")))?;
        return probe_file_url(&url);
    }
    let base = base.ok_or_else(|| {
        Error::Other(format!(
            "cannot resolve relative import '{specifier}' here (eval has no file base URL)"
        ))
    })?;
    if base.scheme() == "data" {
        return Err(Error::Other(format!(
            "cannot resolve relative import '{specifier}' from a data: module"
        )));
    }
    if base.scheme() != "file" {
        return Err(Error::Other(format!(
            "cannot resolve '{specifier}' from '{base}' (only file: bases for now)"
        )));
    }
    let joined = base.join(specifier).map_err(|e| {
        Error::Other(format!("cannot resolve '{specifier}' from '{base}': {e}"))
    })?;
    probe_file_url(&joined)
}

/// 入口路径（CLI 传来的文件参数）→ file: URL。
pub fn entry_url(path: &Path) -> Result<Url, Error> {
    let abs: PathBuf = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| Error::Other(format!("cannot get cwd: {e}")))?
            .join(path)
    };
    let url =
        Url::from_file_path(&abs).map_err(|_| Error::Other(format!("bad path: {}", abs.display())))?;
    probe_file_url(&url)
}
