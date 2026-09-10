//! 包管理（plan Phase 5）：spec 解析 + registry 拉取 + 版本求解。
//! 切片 a 只求解不落地（`install --dry-run`）；网络测试走本地 stub registry。

pub mod cache;
pub mod install;
pub mod lifecycle;
pub mod registry;
pub mod resolve;
pub mod spec;

use crate::error::Error;

/// `winterjs install [pkgs...] [--dry-run] [--registry URL]`。
/// 切片 a：空包列表报错（读 package.json 顺延 5b）；非 dry-run 报顺延错。
pub async fn install(packages: &[String], dry_run: bool, registry: Option<&str>) -> Result<(), Error> {
    if packages.is_empty() {
        return Err(Error::Other(
            "install with no packages needs package.json (slice 5b)".into(),
        ));
    }
    let registry = registry.unwrap_or(registry::DEFAULT_REGISTRY);
    let mut specs = Vec::with_capacity(packages.len());
    for pkg in packages {
        specs.push(spec::parse(pkg).map_err(Error::Other)?);
    }
    let registry = registry.to_owned();
    let tree = resolve::solve_tree(&specs, |name: String| {
        let registry = registry.clone();
        async move { registry::fetch_packument(&registry, &name).await.map_err(|e| e.to_string()) }
    })
    .await
    .map_err(Error::Other)?;
    if dry_run {
        for r in &tree {
            println!("{}@{} {}", r.name, r.version, r.tarball);
        }
        return Ok(());
    }
    let root = std::env::current_dir().map_err(|e| Error::Other(format!("cannot get cwd: {e}")))?;
    install::install_tree(&root, &tree).await
}
