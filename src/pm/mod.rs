//! 包管理（plan Phase 5）：spec 解析 + registry 拉取 + 版本求解。
//! 切片 a 只求解不落地（`install --dry-run`）；网络测试走本地 stub registry。

pub mod cache;
pub mod git;
pub mod install;
pub mod lifecycle;
pub mod manifest;
pub mod npmrc;
pub mod platform;
pub mod publish;
pub mod release;
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
    install_request(root, packages, &[], dry_run, registry, None).await
}

/// install-all（`--init` 对齐 `bun install`）：读 `root/package.json` 依赖段
/// （`manifest`），清单没变且 `node_modules` 在即跳过；否则全量求解安装，
/// lockfile 记清单指纹。
pub async fn install_manifest(
    root: &std::path::Path,
    dry_run: bool,
    registry: Option<&str>,
) -> Result<ManifestOutcome, Error> {
    let Some(deps) = manifest::load(root)? else {
        return Ok(ManifestOutcome::NoManifest);
    };
    let fp = manifest::fingerprint(&deps);
    // 指纹相同且 node_modules 在 → 视为最新（幂等；逐包对账 npm 级不做到）。
    if lockfile_manifest(root).is_some_and(|m| m == fp) && root.join("node_modules").is_dir() {
        return Ok(ManifestOutcome::UpToDate);
    }
    let (required, optional) = manifest::spec_strings(&deps);
    if required.is_empty() && optional.is_empty() {
        // 有 package.json 但无依赖段：无事可做（不建 node_modules）。
        return Ok(ManifestOutcome::NoManifest);
    }
    install_request(root, &required, &optional, dry_run, registry, Some(&fp)).await?;
    Ok(ManifestOutcome::Installed)
}

/// install-all 结果（init 打印分支用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestOutcome {
    /// 无 package.json 或清单无依赖段。
    NoManifest,
    /// 清单指纹未变且 node_modules 在，跳过。
    UpToDate,
    /// 已执行安装。
    Installed,
}

/// lockfile 里的清单指纹（无/坏文件 → `None`）。
fn lockfile_manifest(root: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join(install::LOCKFILE)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("manifest")?.as_str().map(str::to_owned)
}

/// 安装共用体（`install_to` 与 `install_manifest` 共用）：
/// spec 串分流 registry/git/release，`optional` 段 spec 走可选根求解
/// （拉取/求解失败容忍跳过；git/release 形不做可选，见 manifest 头注）。
async fn install_request(
    root: &std::path::Path,
    packages: &[String],
    optional: &[String],
    dry_run: bool,
    registry: Option<&str>,
    manifest_fp: Option<&str>,
) -> Result<(), Error> {
    if packages.is_empty() && optional.is_empty() {
        return Err(Error::Other("specify packages with -a/--add".into()));
    }
    let (project, home) = npmrc::load_cwd_and_home(root);
    let env_reg = std::env::var("NPM_CONFIG_REGISTRY")
        .or_else(|_| std::env::var("npm_config_registry"))
        .ok();
    // 请求分流（registry 走 packument 求解；git 走 rev 解析；release 走 GitHub API）。
    let mut reg_specs = Vec::with_capacity(packages.len());
    let mut reg_optional = Vec::new();
    let mut git_specs = Vec::new();
    let mut rel_specs = Vec::new();
    for (pkg, via_optional) in packages.iter().map(|p| (p, false)).chain(optional.iter().map(|p| (p, true))) {
        let parsed = spec::parse_request(pkg).map_err(|e| {
            if via_optional {
                Error::Other(format!("bad optional dependency '{pkg}' in package.json: {e}"))
            } else {
                Error::Other(e)
            }
        })?;
        match parsed {
            spec::Request::Registry(s) => {
                if via_optional { reg_optional.push(s) } else { reg_specs.push(s) }
            }
            // git/release 形不做可选根（清单里出现即按必需装；偏差记 manifest 头注）。
            spec::Request::Git(g) => git_specs.push(g),
            spec::Request::Release(r) => rel_specs.push(r),
        }
    }
    if reg_specs.is_empty() && reg_optional.is_empty() && git_specs.is_empty() && rel_specs.is_empty() {
        return Err(Error::Other("nothing to install".into()));
    }
    let tree = resolve::solve_tree_rooted(&reg_specs, &reg_optional, |name: String| {
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
    let mut rel_shown = Vec::with_capacity(rel_specs.len());
    for r in &rel_specs {
        let (asset, url) = release::resolve_release(r).await?;
        rel_shown.push((release::bin_name(r).to_owned(), asset, url));
    }
    if dry_run {
        for r in &tree {
            println!("{}@{} {}", r.name, r.version, r.tarball);
        }
        for d in &git_shown {
            println!("{}@git+{}#{}", d.name, d.url, d.rev);
        }
        for (name, asset, url) in &rel_shown {
            println!("{name}@release ({asset} {url})");
        }
        return Ok(());
    }
    // release 二进制先落 `.bin`（与 registry/git 树独立；失败即整单失败，
    // 显式要的二进制无“容忍”语义——optional 只适用于传递依赖）。
    let nm_bin = root.join("node_modules").join(".bin");
    let mut rel_locked = Vec::with_capacity(rel_specs.len());
    for r in &rel_specs {
        rel_locked.push(release::install_one_release(&nm_bin, r).await?);
    }
    install::install_all(root, &tree, &git_specs, &rel_locked, manifest_fp).await
}
