//! 包管理（plan Phase 5）：spec 解析 + registry 拉取 + 版本求解。
//! 切片 a 只求解不落地（`install --dry-run`）；网络测试走本地 stub registry。

pub mod cache;
pub mod git;
pub mod install;
pub mod lifecycle;
pub mod npmrc;
pub mod platform;
pub mod publish;
pub mod registry;
pub mod resolve;
pub mod spec;
pub mod upgrade;

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

/// 全局安装根：`WINTERJS_GLOBAL_ROOT`（测试隔离/用户覆盖）> 系统数据目录
///（`dirs::data_dir/winterjs/global`；macOS 下即 `~/Library/Application Support/...`）。
pub fn global_root() -> Result<std::path::PathBuf, Error> {
    if let Ok(v) = std::env::var("WINTERJS_GLOBAL_ROOT")
        && !v.trim().is_empty()
    {
        return Ok(std::path::PathBuf::from(v));
    }
    let base = dirs::data_dir().ok_or_else(|| Error::Other("cannot find data directory".into()))?;
    Ok(base.join("winterjs").join("global"))
}

/// `winterjs add -a <pkgs> [--dry-run] [--registry URL]`（工程本地，root=cwd）与
/// `winterjs install -a <pkgs> ...`（全局，root=`global_root()`）共用体。
/// registry 优先级：flag > `NPM_CONFIG_REGISTRY` env > 作用域镜像
/// （`<root>/.npmrc` > `$HOME/.npmrc` 的 `@scope:registry`）> `<root>/.npmrc` >
/// `$HOME/.npmrc` > 内建默认（见 `npmrc`）；token 按生效 registry host 透传。
pub async fn install_to(
    root: &std::path::Path,
    packages: &[String],
    dry_run: bool,
    registry: Option<&str>,
) -> Result<(), Error> {
    if packages.is_empty() {
        return Err(Error::Other("specify packages with -a/--add".into()));
    }
    let (project, home) = npmrc::load_cwd_and_home(root);
    let env_reg = std::env::var("NPM_CONFIG_REGISTRY")
        .or_else(|_| std::env::var("npm_config_registry"))
        .ok();
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
        // 逐包决策（作用域镜像）+ 逐 registry 取 token（token 值永不进日志）。
        let (src, url) = npmrc::resolve_registry_for_package(
            registry,
            env_reg.as_deref(),
            &project,
            &home,
            &name,
        );
        tracing::debug!(target: "winterjs::pm", package = name.as_str(), source = src, registry = url.as_str(), "registry resolved");
        let token = project
            .auth_token_for(&url)
            .or_else(|| home.auth_token_for(&url));
        async move { registry::fetch_packument(&url, &name, token.as_deref()).await.map_err(|e| e.to_string()) }
    })
    .await
    .map_err(Error::Other)?;
    let mut git_shown = Vec::with_capacity(git_specs.len());
    for g in &git_specs {
        git_shown.push(git::resolve_for_dry_run(g, root).map_err(Error::Other)?);
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
    install::install_all(root, &tree, &git_specs).await
}
