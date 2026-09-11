//! 包 spec 解析（`name[@range]` + scope；纯函数，单元测试覆盖）。
//! - `left-pad` → (left-pad, `*`)；`left-pad@^1.0.0` → range；`@scope/pkg@1.x` → scope 拆分。
//! - 空名/空 range 报错；`@scope` 无包名报错。

/// 解析结果（`range` 为 npm 语义串，`*` 表任意）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub name: String,
    pub range: String,
}

/// git 依赖 spec（`[<name>@]git+<url>[#<rev>]`；`rev` 缺省 `HEAD`）。
/// 例：`pkg@git+https://h/r.git#v1`、`git+file:///tmp/r`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSpec {
    /// 显式名（`None` 表裸 spec，名从包的 package.json 读）。
    pub name: Option<String>,
    /// 去掉 `git+` 前缀的 URL（`https://…`/`file://…`/本地路径）。
    pub url: String,
    /// commit-ish（分支/tag/commit；`None` 表 `HEAD`）。
    pub rev: Option<String>,
}

/// 安装请求（registry 包、git 包或 GitHub release 二进制；见 `parse_request`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Registry(Spec),
    Git(GitSpec),
    Release(ReleaseSpec),
}

/// 解析 spec（见头注规则）。
pub fn parse(spec: &str) -> Result<Spec, String> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err("empty package spec".to_string());
    }
    // scope 包 `@scope/name[@range]`：首个 @ 属名，第二个 @ 切 range。
    if let Some(rest) = spec.strip_prefix('@') {
        let mut parts = rest.splitn(2, '@');
        let scoped = parts.next().unwrap_or("");
        let range = parts.next();
        let mut names = scoped.splitn(2, '/');
        let (scope, pkg) = (names.next().unwrap_or(""), names.next().unwrap_or(""));
        if scope.is_empty() || pkg.is_empty() {
            return Err(format!("bad scoped package spec '{spec}'"));
        }
        let range = range.unwrap_or("*");
        if range.is_empty() {
            return Err(format!("empty version range in '{spec}'"));
        }
        return Ok(Spec { name: format!("@{scoped}"), range: range.to_string() });
    }
    match spec.split_once('@') {
        Some((name, range)) => {
            if name.is_empty() {
                return Err(format!("bad package spec '{spec}'"));
            }
            if range.is_empty() {
                return Err(format!("empty version range in '{spec}'"));
            }
            Ok(Spec { name: name.to_string(), range: range.to_string() })
        }
        None => Ok(Spec { name: spec.to_string(), range: "*".to_string() }),
    }
}

/// 解析 git spec（`[<name>@]git+<url>[#<rev>]`；非 git 形返回 `None`）。
/// 纯函数，单元测试覆盖。
pub fn parse_git(spec: &str) -> Option<Result<GitSpec, String>> {
    let spec = spec.trim();
    // 裸形：`git+…`（名从 package.json 读）。
    if let Some(rest) = spec.strip_prefix("git+") {
        return Some(finish_git(None, rest, spec));
    }
    // 具名形：`<name>@git+…`（`@git+` 不在首位；scope 包的打头 `@` 不干扰）。
    if let Some(i) = spec.find("@git+") {
        if i > 0 {
            let (name, rest) = (&spec[..i], &spec[i + 1..]);
            if name.is_empty() {
                return Some(Err(format!("bad git package spec '{spec}'")));
            }
            return Some(finish_git(Some(name.to_string()), &rest["git+".len()..], spec));
        }
    }
    None
}

fn finish_git(name: Option<String>, rest: &str, orig: &str) -> Result<GitSpec, String> {
    let (url, rev) = match rest.split_once('#') {
        Some((u, r)) => {
            if r.is_empty() {
                return Err(format!("empty revision in '{orig}'"));
            }
            (u, Some(r.to_string()))
        }
        None => (rest, None),
    };
    if url.trim().is_empty() {
        return Err(format!("empty git url in '{orig}'"));
    }
    Ok(GitSpec { name, url: url.to_string(), rev })
}

/// GitHub release 二进制 spec（`[<name>@]release:github/<owner>/<repo>@<tag>/<prefix>`）。
/// 例：`oxlint@release:github/oxc-project/oxc@apps_v1.82.0/oxlint`
///（`name` 缺省取 `prefix`；`tag` 必须显式，锁定可复现）。
/// asset 名平台相关（如 `oxlint-aarch64-apple-darwin.tar.gz`），由 `release`
/// 按当前平台挑选（见 `pick_asset`），spec 里只写前缀。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseSpec {
    /// 安装后的 bin 名（`None` 表取 `prefix`）。
    pub name: Option<String>,
    pub owner: String,
    pub repo: String,
    pub tag: String,
    pub prefix: String,
}

/// `release:` 形解析（`[<name>@]release:github/<owner>/<repo>@<tag>/<prefix>`）。
/// 纯函数，单元测试覆盖。`tag` 必须显式（锁定可复现，不跟 latest 漂）。
pub fn parse_release(spec: &str) -> Option<Result<ReleaseSpec, String>> {
    let spec = spec.trim();
    // 具名形 `<name>@release:…`（`@release:` 不在首位）。
    let (name, rest) = match spec.find("@release:") {
        Some(i) if i > 0 => (Some(spec[..i].to_string()), &spec[i + 1..]),
        _ if spec.starts_with("release:") => (None, spec),
        _ => return None,
    };
    if let Some(n) = &name
        && (n.is_empty() || n.contains('/') || n.contains('@'))
    {
        return Some(Err(format!("bad release package name in '{spec}'")));
    }
    let rest = &rest["release:".len()..];
    // 目前只支持 github（host 位留扩展）。
    let rest = match rest.strip_prefix("github/") {
        Some(r) => r,
        None => return Some(Err(format!("bad release spec '{spec}' (want release:github/<owner>/<repo>@<tag>/<prefix>)"))),
    };
    let mut parts = rest.splitn(3, '/');
    let (owner, repo_tag, prefix) =
        (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    if owner.is_empty() || prefix.is_empty() {
        return Some(Err(format!("bad release spec '{spec}' (want release:github/<owner>/<repo>@<tag>/<prefix>)")));
    }
    let (repo, tag) = match repo_tag.split_once('@') {
        Some((r, t)) if !r.is_empty() && !t.is_empty() => (r, t),
        _ => {
            return Some(Err(format!("bad release spec '{spec}' (tag is required: <repo>@<tag>)")));
        }
    };
    Some(Ok(ReleaseSpec {
        name,
        owner: owner.to_string(),
        repo: repo.to_string(),
        tag: tag.to_string(),
        prefix: prefix.to_string(),
    }))
}
pub fn parse_request(spec: &str) -> Result<Request, String> {
    // 解析顺序：release 形 → git 形 → `github:` 缩写 → registry（先精确后宽泛）。
    if let Some(rel) = parse_release(spec) {
        return rel.map(Request::Release);
    }
    if let Some(git) = parse_git(spec) {
        return git.map(Request::Git);
    }
    if let Some(git) = parse_github(spec) {
        return git.map(Request::Git);
    }
    parse(spec).map(Request::Registry)
}

/// `github:` 缩写 → git 依赖（`[<name>@]github:<user>/<repo>[#<rev>]`；
/// 名缺省从包读，URL 固定 `https://github.com/<user>/<repo>.git`）。
/// 纯函数，单元测试覆盖。
pub fn parse_github(spec: &str) -> Option<Result<GitSpec, String>> {
    let spec = spec.trim();
    // 具名形 `<name>@github:…`（`@github:` 不在首位）。
    let (name, rest) = match spec.find("@github:") {
        Some(i) if i > 0 => (Some(spec[..i].to_string()), &spec[i + 1..]),
        _ if spec.starts_with("github:") => (None, spec),
        _ => return None,
    };
    let rest = &rest["github:".len()..];
    let (path, rev) = match rest.split_once('#') {
        Some((p, r)) => {
            if r.is_empty() {
                return Some(Err(format!("empty revision in '{spec}'")));
            }
            (p, Some(r.to_string()))
        }
        None => (rest, None),
    };
    let mut parts = path.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(user), Some(repo), None) if !user.is_empty() && !repo.is_empty() => {
            // `.git` 后缀有则保留、无则补（git CLI 两可，统一补齐便缓存键稳定）。
            let repo = repo.strip_suffix(".git").unwrap_or(repo);
            Some(Ok(GitSpec {
                name,
                url: format!("https://github.com/{user}/{repo}.git"),
                rev,
            }))
        }
        _ => Some(Err(format!("bad github shorthand '{spec}' (want github:<user>/<repo>[#<rev>])"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_table() {
        assert_eq!(parse("left-pad").unwrap(), Spec { name: "left-pad".into(), range: "*".into() });
        assert_eq!(
            parse("left-pad@^1.0.0").unwrap(),
            Spec { name: "left-pad".into(), range: "^1.0.0".into() }
        );
        assert_eq!(
            parse("@scope/pkg").unwrap(),
            Spec { name: "@scope/pkg".into(), range: "*".into() }
        );
        assert_eq!(
            parse("@scope/pkg@1.x").unwrap(),
            Spec { name: "@scope/pkg".into(), range: "1.x".into() }
        );
        assert_eq!(
            parse("@scope/pkg@latest").unwrap(),
            Spec { name: "@scope/pkg".into(), range: "latest".into() }
        );
        assert!(parse("").is_err());
        assert!(parse("@scope").is_err());
        assert!(parse("@scope@1.0.0").is_err());
        assert!(parse("@/x").is_err());
        assert!(parse("pkg@").is_err());
        assert!(parse("@pkg").is_err());
    }

    #[test]
    fn git_spec_table() {
        assert_eq!(
            parse_request("pkg@git+https://h/r.git#v1.0.0").unwrap(),
            Request::Git(GitSpec {
                name: Some("pkg".into()),
                url: "https://h/r.git".into(),
                rev: Some("v1.0.0".into()),
            })
        );
        assert_eq!(
            parse_request("@scope/pkg@git+https://h/r.git").unwrap(),
            Request::Git(GitSpec {
                name: Some("@scope/pkg".into()),
                url: "https://h/r.git".into(),
                rev: None,
            })
        );
        assert_eq!(
            parse_request("git+file:///tmp/r").unwrap(),
            Request::Git(GitSpec { name: None, url: "file:///tmp/r".into(), rev: None })
        );
        // registry 形不受影响（含 ssh 形包名巧合也不误判：无 `@git+` 分隔即 registry）。
        assert!(matches!(parse_request("left-pad@^1.0.0").unwrap(), Request::Registry(_)));
        // 报错三件：空 URL / 空 rev。
        assert!(parse_request("pkg@git+#main").is_err());
        assert!(parse_request("pkg@git+https://h/r.git#").is_err());
    }

    #[test]
    fn github_shorthand_table() {
        assert_eq!(
            parse_request("github:user/repo").unwrap(),
            Request::Git(GitSpec {
                name: None,
                url: "https://github.com/user/repo.git".into(),
                rev: None,
            })
        );
        assert_eq!(
            parse_request("pkg@github:user/repo#v1.0.0").unwrap(),
            Request::Git(GitSpec {
                name: Some("pkg".into()),
                url: "https://github.com/user/repo.git".into(),
                rev: Some("v1.0.0".into()),
            })
        );
        // `.git` 后缀归一 + 空 rev/坏形报错。
        assert_eq!(
            parse_request("github:user/repo.git").unwrap(),
            Request::Git(GitSpec {
                name: None,
                url: "https://github.com/user/repo.git".into(),
                rev: None,
            })
        );
        assert!(parse_request("github:user/repo#").is_err());
        assert!(parse_request("github:user").is_err());
        assert!(parse_request("github:user/a/b").is_err());
        // registry 形不受影响。
        assert!(matches!(parse_request("left-pad@^1.0.0").unwrap(), Request::Registry(_)));
    }

    #[test]
    fn release_spec_table() {
        assert_eq!(
            parse_request("oxlint@release:github/oxc-project/oxc@apps_v1.82.0/oxlint").unwrap(),
            Request::Release(ReleaseSpec {
                name: Some("oxlint".into()),
                owner: "oxc-project".into(),
                repo: "oxc".into(),
                tag: "apps_v1.82.0".into(),
                prefix: "oxlint".into(),
            })
        );
        // 名缺省取 prefix。
        assert_eq!(
            parse_request("release:github/oxc-project/oxc@apps_v1.82.0/oxfmt").unwrap(),
            Request::Release(ReleaseSpec {
                name: None,
                owner: "oxc-project".into(),
                repo: "oxc".into(),
                tag: "apps_v1.82.0".into(),
                prefix: "oxfmt".into(),
            })
        );
        // 报错：缺 tag / 非 github host / 坏名。
        assert!(parse_request("release:github/o/r/prefix").is_err());
        assert!(parse_request("release:gitlab/o/r@t/p").is_err());
        assert!(parse_request("release:github/o/r@/p").is_err());
        assert!(parse_request("release:github//r@t/p").is_err());
        assert!(parse_request("release:github/o/r@t/").is_err());
        // registry/git 形不受影响。
        assert!(matches!(parse_request("left-pad@^1.0.0").unwrap(), Request::Registry(_)));
        assert!(matches!(parse_request("pkg@git+https://h/r.git").unwrap(), Request::Git(_)));
    }
}
