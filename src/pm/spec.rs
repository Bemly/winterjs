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

/// 安装请求（registry 包或 git 包；见 `parse_request`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Registry(Spec),
    Git(GitSpec),
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

/// 解析安装请求（先试 git 形，否则走 registry 解析；`github:` 缩写顺延，报错指路）。
pub fn parse_request(spec: &str) -> Result<Request, String> {
    if let Some(git) = parse_git(spec) {
        return git.map(Request::Git);
    }
    if spec.trim().starts_with("github:") {
        return Err(format!(
            "'{spec}' uses github: shorthand (deferred); use '<name>@git+https://github.com/<user>/<repo>.git#<rev>'"
        ));
    }
    parse(spec).map(Request::Registry)
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
        // 报错三件：空 URL / 空 rev / github 缩写指路。
        assert!(parse_request("pkg@git+#main").is_err());
        assert!(parse_request("pkg@git+https://h/r.git#").is_err());
        assert!(parse_request("github:user/repo").unwrap_err().contains("git+https"));
    }
}
