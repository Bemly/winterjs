//! 包管理（plan Phase 5）：spec 解析 + registry 拉取 + 版本求解。
//! 切片 a 只求解不落地（`install --dry-run`）；网络测试走本地 stub registry。

pub mod cache;
pub mod install;
pub mod lifecycle;
pub mod npmrc;
pub mod registry;
pub mod resolve;
pub mod spec;

use crate::error::Error;

/// `winterjs install [pkgs...] [--dry-run] [--registry URL]`。
/// registry 优先级（5d-d1）：flag > `NPM_CONFIG_REGISTRY` env > `<cwd>/.npmrc` >
/// `$HOME/.npmrc` > 内建默认（见 `npmrc`）。
pub async fn install(packages: &[String], dry_run: bool, registry: Option<&str>) -> Result<(), Error> {
    if packages.is_empty() {
        return Err(Error::Other(
            "install with no packages needs package.json (slice 5b)".into(),
        ));
    }
    let cwd = std::env::current_dir().map_err(|e| Error::Other(format!("cannot get cwd: {e}")))?;
    let (project, home) = npmrc::load_cwd_and_home(&cwd);
    let (src, registry) = npmrc::resolve_registry(registry, &project, &home);
    // auth token 只查有无（值永不进日志；5d-d3 publish 才真正使用）。
    let has_auth = project
        .auth_token_for(&registry)
        .or_else(|| home.auth_token_for(&registry))
        .is_some();
    tracing::debug!(target: "winterjs::pm", source = src, registry = registry.as_str(), has_auth, "registry resolved");
    let mut specs = Vec::with_capacity(packages.len());
    for pkg in packages {
        specs.push(spec::parse(pkg).map_err(Error::Other)?);
    }
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
    install::install_tree(&cwd, &tree).await
}
