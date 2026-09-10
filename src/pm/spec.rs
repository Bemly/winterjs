//! 包 spec 解析（`name[@range]` + scope；纯函数，单元测试覆盖）。
//! - `left-pad` → (left-pad, `*`)；`left-pad@^1.0.0` → range；`@scope/pkg@1.x` → scope 拆分。
//! - 空名/空 range 报错；`@scope` 无包名报错。

/// 解析结果（`range` 为 npm 语义串，`*` 表任意）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub name: String,
    pub range: String,
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
}
