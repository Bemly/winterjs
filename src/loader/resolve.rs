//! specifier 解析：绝对 URL 直通；相对路径走 `oxc_resolver`（node 语义，
//! 自家 join+探测兜底）；裸导入走 `oxc_resolver`（node_modules + tsconfig 自动发现）。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use oxc_resolver::{ResolveOptions, Resolver, TsconfigDiscovery};
use url::Url;

use crate::error::Error;

/// 后缀探测顺序（兜底路径用；主路径由 resolver 的 extensions 覆盖）。
const PROBE_EXTS: &[&str] = &["ts", "tsx", "mts", "js", "mjs"];
const INDEX_FILES: &[&str] = &["index.ts", "index.tsx", "index.mts", "index.js", "index.mjs"];

/// 进程级单例 resolver（`tsconfig: Auto` 按每次查询目录发现，无需多例）。
fn resolver() -> &'static Resolver {
    static R: OnceLock<Resolver> = OnceLock::new();
    R.get_or_init(|| {
        Resolver::new(ResolveOptions {
            extensions: [".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs", ".json"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            // TS 约定：`./foo.js` 可指向 `./foo.ts` 源码
            extension_alias: [
                (".js".to_string(), vec![".ts".to_string(), ".tsx".to_string(), ".js".to_string()]),
                (".jsx".to_string(), vec![".tsx".to_string(), ".jsx".to_string()]),
                (".mjs".to_string(), vec![".mts".to_string(), ".mjs".to_string()]),
                (".cjs".to_string(), vec![".cts".to_string(), ".cjs".to_string()]),
            ]
            .into_iter()
            .collect(),
            condition_names: vec!["node".into(), "import".into()],
            main_files: vec!["index".into()],
            tsconfig: Some(TsconfigDiscovery::Auto),
            ..ResolveOptions::default()
        })
    })
}

fn is_file_url_exists(url: &Url) -> bool {
    url.to_file_path().map(|p| p.is_file()).unwrap_or(false)
}

/// file: URL 规范化为真实路径（symlink 展开；不存在则原样返回）。
/// 模块同一性按真实路径计，否则 `/var` 与 `/private/var` 会被当成两个模块（§4.12）。
fn canonical_file_url(url: &Url) -> Url {
    if url.scheme() != "file" {
        return url.clone();
    }
    let Ok(p) = url.to_file_path() else {
        return url.clone();
    };
    if !p.exists() {
        return url.clone();
    }
    match std::fs::canonicalize(&p) {
        Ok(c) => Url::from_file_path(&c).unwrap_or_else(|_| url.clone()),
        Err(_) => url.clone(),
    }
}

/// file: URL 不存在时做后缀/index 探测；非 file 原样返回。返回前一律规范化。
fn probe_file_url(url: &Url) -> Result<Url, Error> {
    if url.scheme() != "file" {
        return Ok(url.clone());
    }
    if is_file_url_exists(url) {
        return Ok(canonical_file_url(url));
    }
    let path = url
        .to_file_path()
        .map_err(|_| Error::Other(format!("bad file URL: {url}")))?;
    // ./foo → ./foo.ts ……
    if path.extension().is_none() {
        for ext in PROBE_EXTS {
            let cand = path.with_extension(ext);
            if cand.is_file() {
                let u = Url::from_file_path(&cand)
                    .map_err(|_| Error::Other(format!("bad file path: {}", cand.display())))?;
                return Ok(canonical_file_url(&u));
            }
        }
    }
    // ./dir → ./dir/index.ts ……
    if path.is_dir() {
        for name in INDEX_FILES {
            let cand = path.join(name);
            if cand.is_file() {
                let u = Url::from_file_path(&cand)
                    .map_err(|_| Error::Other(format!("bad file path: {}", cand.display())))?;
                return Ok(canonical_file_url(&u));
            }
        }
    }
    Err(Error::Other(format!("module not found: {url}")))
}

/// 发起方文件（file: 取自身路径；无 base 取 cwd 锚点；其余报错）。
/// 返回 `(resolve 用路径, 是否可用 tsconfig 自动发现)`: 只有真实存在的文件才走
/// `resolve_file`（`TsconfigDiscovery::Auto` 仅该 API 生效），cwd 锚点走 `resolve`。
fn caller_file(base: Option<&Url>) -> Result<(PathBuf, bool), Error> {
    match base {
        Some(u) if u.scheme() == "file" => {
            let p = u
                .to_file_path()
                .map_err(|_| Error::Other(format!("bad file URL: {u}")))?;
            Ok((p, true))
        }
        Some(u) => Err(Error::Other(format!(
            "cannot resolve from '{u}' (only file: bases for now)"
        ))),
        None => {
            let cwd = std::env::current_dir().map_err(|e| Error::Other(format!("cannot get cwd: {e}")))?;
            Ok((cwd, false))
        }
    }
}

/// 统一走 resolver：有真实文件用 `resolve_file`（tsconfig 生效），否则 `resolve`。
fn resolve_with(specifier: &str, base: Option<&Url>) -> Result<PathBuf, Error> {
    let (anchor, use_file) = caller_file(base)?;
    let dir = if anchor.is_dir() {
        anchor.clone()
    } else {
        anchor.parent().map(|d| d.to_path_buf()).unwrap_or_else(|| anchor.clone())
    };
    let r = if use_file && !anchor.is_dir() {
        resolver().resolve_file(&anchor, specifier)
    } else {
        resolver().resolve(&dir, specifier)
    };
    r.map(|r| r.full_path()).map_err(|e| {
        Error::Other(format!(
            "cannot resolve '{specifier}' (checked node_modules from {}): {}",
            dir.display(),
            first_line(e.to_string())
        ))
    })
}

fn first_line(mut s: String) -> String {
    s.truncate(s.find('\n').unwrap_or(s.len()));
    s
}

fn path_to_file_url(p: PathBuf) -> Result<Url, Error> {
    Url::from_file_path(&p).map_err(|_| Error::Other(format!("bad file path: {}", p.display())))
}

/// 裸导入：node 内建优先 → node_modules + tsconfig（paths）+ exports 条件。
fn resolve_bare(specifier: &str, base: Option<&Url>) -> Result<Url, Error> {
    // node 内建优先于 node_modules（与 Node 一致；`fs` 与 `node:fs` 同一模块）。
    if let Some(canonical) = crate::builtins::node::normalize_spec(specifier) {
        tracing::debug!(target: "winterjs::loader", specifier, canonical, "builtin module");
        return Url::parse(canonical).map_err(|e| Error::Other(format!("bad builtin URL: {e}")));
    }    if matches!(base.map(|u| u.scheme()), Some("data")) {
        return Err(Error::Other(format!(
            "cannot resolve bare specifier '{specifier}' from a data: module"
        )));
    }
    match resolve_with(specifier, base) {
        Ok(p) => {
            tracing::debug!(target: "winterjs::loader", specifier, path = %p.display(), "resolved via oxc_resolver");
            path_to_file_url(p)
        }
        Err(e) => Err(e),
    }
}

/// 相对路径：resolver 主路径，失败回落自家 join+探测（行为保护网）。
fn resolve_relative(specifier: &str, base: Option<&Url>) -> Result<Url, Error> {
    // 绝对文件路径可不依赖 base
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
    if let Ok(p) = resolve_with(specifier, Some(base)) {
        tracing::debug!(target: "winterjs::loader", specifier, path = %p.display(), "resolved via oxc_resolver");
        return path_to_file_url(p);
    }
    // 兜底：自家实现（无 node_modules 参与的纯相对场景与之等价）
    let joined = base.join(specifier).map_err(|e| {
        Error::Other(format!("cannot resolve '{specifier}' from '{base}': {e}"))
    })?;
    tracing::debug!(target: "winterjs::loader", specifier, "resolved via fallback join+probe");
    probe_file_url(&joined)
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
            "node" => match crate::builtins::node::normalize_spec(specifier) {
                Some(canonical) => {
                    tracing::debug!(target: "winterjs::loader", specifier, canonical, "builtin module");
                    Url::parse(canonical).map_err(|e| Error::Other(format!("bad builtin URL: {e}")))
                }
                None => Err(Error::Other(format!(
                    "'{specifier}' is not a builtin (available: {})",
                    crate::builtins::node::available().join(", ")
                ))),
            },
            s => Err(Error::Other(format!(
                "unsupported module scheme '{s}:': {specifier}"
            ))),
        };
    }
    if specifier.starts_with("./") || specifier.starts_with("../") || specifier.starts_with('/') {
        resolve_relative(specifier, base)
    } else {
        resolve_bare(specifier, base)
    }
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
