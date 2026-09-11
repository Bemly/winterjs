//! npmrc 解析（plan Phase 5d-d1/d2）：镜像覆盖 + auth 透传。
//!
//! - 格式：手写行解析（首个 `=` 切分，见 `parse`）。
//! - 支持键：`registry=<url>`（全局镜像）；`@<scope>:registry=<url>`（作用域镜像，
//!   精确匹配 `@scope`，project 优先于 home）；`//<host>/...:_authToken=<tok>`
//!   （私有仓 token，按 packument 请求的 registry host 透传 `Bearer`）。
//! - 优先级：`--registry` flag > `NPM_CONFIG_REGISTRY`/`npm_config_registry` env >
//!   作用域镜像（project > home）> `<cwd>/.npmrc` > `$HOME/.npmrc` > 内建默认。
//! - 安全：token 值永不进日志（只记有无）；auth 查询只按 registry host 匹配。

use std::path::Path;

/// 解析后的 npmrc 最小视图（原始键值保留，查询时再匹配）。
#[derive(Debug, Clone, Default)]
pub struct Npmrc {
    entries: Vec<(String, String)>,
}

impl Npmrc {
    /// 空配置。
    pub fn empty() -> Self {
        Self { entries: Vec::new() }
    }

    /// 文本解析（纯函数，可测；坏行忽略，不报错）。
    ///
    /// 刻意手写行解析而不用 `rust-ini` 轮子：`rust-ini` 把 `:` 也当键值分隔符
    /// （上游 `parse_str_until(&[Some('='), Some(':')])`），会把 npmrc 的
    /// `//<host>/:_authToken` 与 `@<scope>:registry` 键从冒号处切断；
    /// npmrc 语义只是“首个 `=` 切分”，20 行手写更正确（见 `docs/dependencies.md` 附记）。
    pub fn parse(text: &str) -> Self {
        let mut entries = Vec::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            // npmrc 无 section；误写的 `[x]` 行直接忽略。
            if line.starts_with('[') {
                continue;
            }
            let Some(eq) = line.find('=') else {
                continue;
            };
            let (k, v) = (line[..eq].trim(), line[eq + 1..].trim());
            if k.is_empty() {
                continue;
            }
            // 值两侧配对引号剥掉一层（`registry="https://…"` 容错）。
            let v = strip_one_quote(v);
            if v.is_empty() {
                continue;
            }
            entries.push((k.to_owned(), v.to_owned()));
        }
        Self { entries }
    }

    /// 文件读取（不存在/失败一律当空，调用方按需 DEBUG）。
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::empty();
        };
        Self::parse(&text)
    }

    /// 全局镜像（`registry=`；末尾空白已 trim）。
    pub fn registry(&self) -> Option<String> {
        self.entries.iter().rev().find_map(|(k, v)| {
            (k.trim() == "registry").then(|| v.trim().to_owned()).filter(|s| !s.is_empty())
        })
    }

    /// 按 registry host 找 token（key 形如 `//host/...:_authToken`；大小写敏感按 npm）。
    pub fn auth_token_for(&self, registry_url: &str) -> Option<String> {
        let host = registry_host(registry_url)?;
        // 后写覆盖先写：rev 找第一条命中。
        self.entries.iter().rev().find_map(|(k, v)| {
            let k = k.trim();
            let (left, is_token) = match k.strip_suffix(":_authToken") {
                Some(l) => (l, true),
                None => (k, false),
            };
            if !is_token || v.trim().is_empty() {
                return None;
            }
            // left 形如 `//host/path`（协议可省略）；含 host 即命中。
            left.contains(&host).then(|| v.trim().to_owned())
        })
    }

    /// 是否含作用域镜像键（提示用；`scoped_registry_for` 做精确匹配）。
    pub fn has_scoped_registry(&self) -> bool {
        self.entries.iter().any(|(k, _)| {
            let k = k.trim();
            k.starts_with('@') && k.contains(":registry")
        })
    }

    /// 作用域镜像（`@scope:registry=<url>` 精确匹配；后写覆盖先写）。
    /// `scope` 为 `@scope` 全形（含 `@`）。
    pub fn scoped_registry_for(&self, scope: &str) -> Option<String> {
        let want = format!("{scope}:registry");
        self.entries.iter().rev().find_map(|(k, v)| {
            (k.trim() == want).then(|| v.trim().to_owned()).filter(|s| !s.is_empty())
        })
    }
}

/// 值两侧配对引号剥一层（双/单引号；不成对则原样）。
fn strip_one_quote(v: &str) -> &str {
    let b = v.as_bytes();
    if b.len() >= 2
        && ((b[0] == b'"' && b[b.len() - 1] == b'"')
            || (b[0] == b'\'' && b[b.len() - 1] == b'\''))
    {
        &v[1..v.len() - 1]
    } else {
        v
    }
}

/// registry URL 的 host（`url` 轮子；解析失败回落简易切分，仍失败则 None）。
/// `pub(crate)`：login 落盘复用。
pub(crate) fn registry_host(registry_url: &str) -> Option<String> {
    if let Ok(u) = url::Url::parse(registry_url) {
        if let Some(h) = u.host_str() {
            return Some(h.to_owned());
        }
    }
    // 兜底：`scheme://host/...` 手切（stub 的 `http://127.0.0.1:port` 必经此或上路）。
    let after = registry_url.split("://").nth(1).unwrap_or(registry_url);
    let hostport = after.split('/').next().unwrap_or("");
    let host = hostport.rsplit(':').next().unwrap_or(hostport);
    (!host.is_empty()).then(|| host.to_owned())
}

/// registry 决策（优先级见头注；返回 (url, 来源) 便于 DEBUG/测试断言）。
/// `env` 传 `NPM_CONFIG_REGISTRY` 取值（`None` 表未设；显式传参保单测 hermetic）。
pub fn resolve_registry_with_env(
    cli: Option<&str>,
    env: Option<&str>,
    project: &Npmrc,
    home: &Npmrc,
) -> (&'static str, String) {
    if let Some(u) = cli.filter(|s| !s.trim().is_empty()) {
        return ("cli", u.to_owned());
    }
    if let Some(u) = env.filter(|s| !s.trim().is_empty()) {
        return ("env", u.trim().to_owned());
    }
    if let Some(u) = project.registry() {
        return ("project-npmrc", u);
    }
    if let Some(u) = home.registry() {
        return ("home-npmrc", u);
    }
    ("default", super::registry::DEFAULT_REGISTRY.to_owned())
}

/// 进程 env 版（`NPM_CONFIG_REGISTRY` 大写优先，小写兜底；读不到当未设）。
pub fn resolve_registry(
    cli: Option<&str>,
    project: &Npmrc,
    home: &Npmrc,
) -> (&'static str, String) {
    let env = std::env::var("NPM_CONFIG_REGISTRY")
        .or_else(|_| std::env::var("npm_config_registry"))
        .ok();
    resolve_registry_with_env(cli, env.as_deref(), project, home)
}

/// 按包决策（含作用域镜像；`name` 如 `@scope/pkg`）。
/// 优先级：cli > env > 作用域（project > home）> project 全局 > home 全局 > 默认。
/// 返回 (来源, url)；来源串供 DEBUG/测试断言。
pub fn resolve_registry_for_package(
    cli: Option<&str>,
    env: Option<&str>,
    project: &Npmrc,
    home: &Npmrc,
    name: &str,
) -> (&'static str, String) {
    if let Some(u) = cli.filter(|s| !s.trim().is_empty()) {
        return ("cli", u.to_owned());
    }
    if let Some(u) = env.filter(|s| !s.trim().is_empty()) {
        return ("env", u.trim().to_owned());
    }
    if let Some(scope) = name.strip_prefix('@').and_then(|r| r.split_once('/').map(|(s, _)| s)) {
        let scoped = format!("@{scope}");
        if let Some(u) = project.scoped_registry_for(&scoped) {
            return ("project-scoped", u);
        }
        if let Some(u) = home.scoped_registry_for(&scoped) {
            return ("home-scoped", u);
        }
    }
    if let Some(u) = project.registry() {
        return ("project-npmrc", u);
    }
    if let Some(u) = home.registry() {
        return ("home-npmrc", u);
    }
    ("default", super::registry::DEFAULT_REGISTRY.to_owned())
}

/// cwd + home 的 npmrc 加载（失败当空；scoped 键计数进 DEBUG）。
pub fn load_cwd_and_home(cwd: &Path) -> (Npmrc, Npmrc) {
    let project = Npmrc::load(&cwd.join(".npmrc"));
    let home = dirs::home_dir().map(|h| Npmrc::load(&h.join(".npmrc"))).unwrap_or_default();
    tracing::debug!(
        target: "winterjs::pm",
        has_project_registry = project.registry().is_some(),
        has_home_registry = home.registry().is_some(),
        has_scoped = project.has_scoped_registry() || home.has_scoped_registry(),
        "npmrc loaded"
    );
    (project, home)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_registry_and_token() {
        let n = Npmrc::parse("registry=https://mirror.example/npm/\n//mirror.example/:_authToken=abc123==\n");
        assert_eq!(n.registry().as_deref(), Some("https://mirror.example/npm/"));
        assert_eq!(
            n.auth_token_for("https://mirror.example/npm/").as_deref(),
            Some("abc123==")
        );
        // host 不匹配不透 token。
        assert!(n.auth_token_for("https://registry.npmjs.org").is_none());
    }

    #[test]
    fn bad_content_is_empty_no_panic() {
        // rust-ini 极少报错；空/注释/坏行一律当空读。
        assert!(Npmrc::parse("").registry().is_none());
        assert!(Npmrc::parse("# just a comment\n").registry().is_none());
        // 后写覆盖先写（npm 语义）。
        let n = Npmrc::parse("registry=https://a/\nregistry=https://b/\n");
        assert_eq!(n.registry().as_deref(), Some("https://b/"));
    }

    #[test]
    fn scoped_keys_ignored_but_detected() {
        let n = Npmrc::parse("@my:registry=https://scoped.example/\n");
        assert!(n.has_scoped_registry());
        // 全局 registry 仍无（作用域键不污染全局决策）。
        assert!(n.registry().is_none());
    }

    #[test]
    fn scoped_registry_exact_match() {
        let n = Npmrc::parse("@my:registry=https://a/\n@my:registry=https://b/\n@other:registry=https://c/\n");
        // 后写覆盖先写；精确匹配 scope。
        assert_eq!(n.scoped_registry_for("@my").as_deref(), Some("https://b/"));
        assert_eq!(n.scoped_registry_for("@other").as_deref(), Some("https://c/"));
        assert!(n.scoped_registry_for("@missing").is_none());
        // 前缀不算数（@myx ≠ @my）。
        assert!(n.scoped_registry_for("@myx").is_none());
    }

    #[test]
    fn for_package_priority() {
        let proj = Npmrc::parse("@acme:registry=https://scoped.example/\nregistry=https://proj.example/\n");
        let home = Npmrc::parse("@acme:registry=https://home-scoped.example/\n");
        // cli/env 最高（作用域也压不住）。
        let (src, _) = resolve_registry_for_package(Some("https://cli.example/"), None, &proj, &home, "@acme/pkg");
        assert_eq!(src, "cli");
        let (src, _) = resolve_registry_for_package(None, Some("https://env.example/"), &proj, &home, "@acme/pkg");
        assert_eq!(src, "env");
        // 作用域：project 赢 home；非 scope 包走全局。
        let (src, url) = resolve_registry_for_package(None, None, &proj, &home, "@acme/pkg");
        assert_eq!((src, url.as_str()), ("project-scoped", "https://scoped.example/"));
        let (src, url) = resolve_registry_for_package(None, None, &Npmrc::empty(), &home, "@acme/pkg");
        assert_eq!((src, url.as_str()), ("home-scoped", "https://home-scoped.example/"));
        let (src, url) = resolve_registry_for_package(None, None, &proj, &home, "plain-pkg");
        assert_eq!((src, url.as_str()), ("project-npmrc", "https://proj.example/"));
    }

    #[test]
    fn priority_cli_env_npmrc_default() {
        let proj = Npmrc::parse("registry=https://proj.example/\n");
        let home = Npmrc::parse("registry=https://home.example/\n");
        // cli 最高。
        let (src, url) = resolve_registry_with_env(Some("https://cli.example/"), Some("https://env.example/"), &proj, &home);
        assert_eq!((src, url.as_str()), ("cli", "https://cli.example/"));
        // env 次之。
        let (src, url) = resolve_registry_with_env(None, Some("https://env.example/"), &proj, &home);
        assert_eq!((src, url.as_str()), ("env", "https://env.example/"));
        // 无 cli/env 时项目 npmrc 赢 home。
        let (src, url) = resolve_registry_with_env(None, None, &proj, &home);
        assert_eq!((src, url.as_str()), ("project-npmrc", "https://proj.example/"));
        let (src, url) = resolve_registry_with_env(None, None, &Npmrc::empty(), &home);
        assert_eq!((src, url.as_str()), ("home-npmrc", "https://home.example/"));
        let (src, url) = resolve_registry_with_env(None, None, &Npmrc::empty(), &Npmrc::empty());
        assert_eq!((src, url.as_str()), ("default", super::super::registry::DEFAULT_REGISTRY));
    }
}
