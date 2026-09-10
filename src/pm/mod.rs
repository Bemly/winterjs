//! 包管理（plan Phase 5）：spec 解析 + registry 拉取 + 版本求解。
//! 切片 a 只求解不落地（`install --dry-run`）；网络测试走本地 stub registry。

pub mod cache;
pub mod git;
pub mod install;
pub mod lifecycle;
pub mod npmrc;
pub mod publish;
pub mod registry;
pub mod resolve;
pub mod spec;

use crate::error::Error;

/// 生效 registry（5d-d1 优先级；打 DEBUG 日志；token 有无只记布尔）。
pub fn effective_registry(cwd: &std::path::Path, cli: Option<&str>) -> String {
    let (project, home) = npmrc::load_cwd_and_home(cwd);
    let (src, url) = npmrc::resolve_registry(cli, &project, &home);
    // auth token 只查有无（值永不进日志；5d-d3 publish 才真正使用）。
    let has_auth = project.auth_token_for(&url).or_else(|| home.auth_token_for(&url)).is_some();
    tracing::debug!(target: "winterjs::pm", source = src, registry = url.as_str(), has_auth, "registry resolved");
    url
}

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
    let registry = effective_registry(&cwd, registry);
    // 请求分流（registry 走 packument 求解；git 走 rev 解析，各自独立）。
    let mut reg_specs = Vec::with_capacity(packages.len());
    let mut git_specs = Vec::new();
    for pkg in packages {
        match spec::parse_request(pkg).map_err(Error::Other)? {
            spec::Request::Registry(s) => reg_specs.push(s),
            spec::Request::Git(g) => git_specs.push(g),
        }
    }
    if reg_specs.is_empty() && git_specs.is_empty() {
        return Err(Error::Other("nothing to install".into()));
    }
    let tree = resolve::solve_tree(&reg_specs, |name: String| {
        let registry = registry.clone();
        async move { registry::fetch_packument(&registry, &name).await.map_err(|e| e.to_string()) }
    })
    .await
    .map_err(Error::Other)?;
    let mut git_shown = Vec::with_capacity(git_specs.len());
    for g in &git_specs {
        git_shown.push(git::resolve_for_dry_run(g, &cwd).map_err(Error::Other)?);
    }
    if dry_run {
        for r in &tree {
            println!("{}@{} {}", r.name, r.version, r.tarball);
        }
        for d in &git_shown {
            println!("{}@git+{}#{}", d.name, d.url, d.rev);
        }
        return Ok(());
    }
    install::install_all(&cwd, &tree, &git_specs).await
}
