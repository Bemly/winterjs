//! package.json 依赖清单读取（install-all 入口，plan Phase 9j 后补）：
//! `--init` 在已有依赖的项目里对齐 `bun install`——读 `dependencies` +
//! `devDependencies`（npm 默认同装）+ `optionalDependencies` 三个顶层段，
//! 拼成根 spec 交求解器。
//! - 同名优先级（npm 口径）：optionalDependencies > dependencies > devDependencies。
//! - 空 range `""` 视为 `*`。
//! - 不支持形（`workspace:`/`file:`/`link:`/`portal:`/`npm:` 别名/裸 URL）报可读错
//!   （清单是作者写的，fail fast；optional 段同样拒绝，与 registry 解析失败的
//!   "容忍跳过" 不同——那是运行期不可达，这是清单本身不合法）。
//! - 指纹：去重后的根 spec 排序列表 JSON 序列化，存 lockfile `manifest` 字段，
//!   用于"清单没变且 node_modules 在 → 跳过重装"。

use std::path::Path;

use crate::error::Error;

/// 清单根依赖（`required` = dependencies+devDependencies 去重合并；
/// `optional` = optionalDependencies 中未出现在 required 的项）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RootDeps {
    pub required: Vec<(String, String)>,
    pub optional: Vec<(String, String)>,
}

/// 不支持的依赖值形态（清单位错误，fail fast）。
fn reject_unsupported(name: &str, range: &str) -> Result<(), Error> {
    for p in ["workspace:", "file:", "link:", "portal:", "npm:", "http://", "https://"] {
        if range.starts_with(p) {
            return Err(Error::Other(format!(
                "unsupported dependency '{name}: {range}' in package.json ('{p}' form is not supported yet)"
            )));
        }
    }
    Ok(())
}

/// 读 `root/package.json` 三个依赖段（无清单 → `None`；坏 JSON → 可读错）。
pub fn load(root: &Path) -> Result<Option<RootDeps>, Error> {
    let path = root.join("package.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let v: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| Error::Other(format!("cannot parse '{}': {e}", path.display())))?;
    let section = |key: &str| -> Result<Vec<(String, String)>, Error> {
        let Some(serde_json::Value::Object(map)) = v.get(key) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::with_capacity(map.len());
        for (name, val) in map {
            let Some(range) = val.as_str() else {
                return Err(Error::Other(format!(
                    "bad dependency '{name}' in package.json '{key}' (want a version range string)"
                )));
            };
            reject_unsupported(name, range)?;
            out.push((name.clone(), if range.is_empty() { "*".into() } else { range.to_string() }));
        }
        Ok(out)
    };
    let deps = section("dependencies")?;
    let dev = section("devDependencies")?;
    let opt = section("optionalDependencies")?;
    // 优先级（npm 口径）：optionalDependencies 整项覆盖同名 dependencies
    // （包转为可选）；dependencies 同名压 devDependencies。
    let mut required: Vec<(String, String)> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (name, range) in deps.into_iter().chain(dev) {
        if seen.insert(name.clone()) {
            required.push((name, range));
        }
    }
    let mut optional = Vec::new();
    for (name, range) in opt {
        required.retain(|(n, _)| n != &name);
        optional.push((name, range));
    }
    Ok(Some(RootDeps { required, optional }))
}

/// 指纹（排序去重后的根 spec；lockfile `manifest` 字段比对用，确定性序列化）。
pub fn fingerprint(deps: &RootDeps) -> String {
    let mut all: Vec<[String; 2]> = deps
        .required
        .iter()
        .map(|(n, r)| [n.clone(), r.clone()])
        .chain(deps.optional.iter().map(|(n, r)| [n.clone(), r.clone()]))
        .collect();
    all.sort();
    all.dedup();
    serde_json::to_string(&all).unwrap_or_default()
}

/// 根 spec 串（`{name}@{range}`；git/github/release 值自带 scheme，
/// 具名形经 `spec::parse_request` 照常解析）。
pub fn spec_strings(deps: &RootDeps) -> (Vec<String>, Vec<String>) {
    let build = |list: &[(String, String)]| {
        list.iter()
            .map(|(n, r)| format!("{n}@{r}"))
            .collect::<Vec<String>>()
    };
    (build(&deps.required), build(&deps.optional))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, text: &str) {
        std::fs::write(dir.join("package.json"), text).unwrap();
    }

    #[test]
    fn load_sections_and_precedence() {
        let dir = tempfile::tempdir().unwrap();
        // 无清单 → None。
        assert_eq!(load(dir.path()).unwrap(), None);
        write(
            dir.path(),
            r#"{
                "name": "p",
                "dependencies": {"a": "^1.0.0", "b": "^2.0.0"},
                "devDependencies": {"b": "^2.1.0", "c": "*"},
                "optionalDependencies": {"a": "^1.2.0", "d": "latest"}
            }"#,
        );
        // npm 口径：optional 整项覆盖同名 dependencies（a 转可选 ^1.2.0）；
        // dependencies 同名压 devDependencies（b 取 ^2.0.0）。
        let d = load(dir.path()).unwrap().unwrap();
        assert_eq!(
            d.required,
            vec![("b".into(), "^2.0.0".into()), ("c".into(), "*".into())]
        );
        assert_eq!(
            d.optional,
            vec![("a".into(), "^1.2.0".into()), ("d".into(), "latest".into())]
        );
    }

    #[test]
    fn load_empty_range_and_bad_forms() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), r#"{"dependencies": {"a": "", "b": "workspace:*"}}"#);
        let err = load(dir.path()).unwrap_err().to_string();
        assert!(err.contains("workspace:"), "{err}");
        // 可读错含包名。
        assert!(err.contains('b'), "{err}");
        // 非 string 值。
        write(dir.path(), r#"{"dependencies": {"a": 3}}"#);
        assert!(load(dir.path()).is_err());
        // 坏 JSON。
        write(dir.path(), r#"{"name": "#);
        assert!(load(dir.path()).is_err());
    }

    #[test]
    fn fingerprint_deterministic_and_dedup() {
        let d = RootDeps {
            required: vec![("b".into(), "^2.0.0".into()), ("a".into(), "^1.0.0".into())],
            optional: vec![("a".into(), "^1.0.0".into())],
        };
        // 排序 + 同名同 range 去重；与输入顺序无关。
        let f1 = fingerprint(&d);
        let flipped = RootDeps {
            required: d.required.iter().rev().cloned().collect(),
            optional: d.optional.clone(),
        };
        assert_eq!(f1, fingerprint(&flipped));
        assert_eq!(f1, r#"[["a","^1.0.0"],["b","^2.0.0"]]"#);
        // 清单变化 → 指纹变化。
        let edited = RootDeps { required: vec![("a".into(), "^1.1.0".into())], optional: vec![] };
        assert_ne!(f1, fingerprint(&edited));
    }

    #[test]
    fn spec_strings_roundtrip_parse() {
        let d = RootDeps {
            required: vec![
                ("left-pad".into(), "^1.0.0".into()),
                ("@scope/pkg".into(), "1.x".into()),
                ("gh".into(), "github:user/repo#v1".into()),
            ],
            optional: vec![("opt".into(), "latest".into())],
        };
        let (req, opt) = spec_strings(&d);
        assert_eq!(req[0], "left-pad@^1.0.0");
        assert_eq!(req[1], "@scope/pkg@1.x");
        assert_eq!(req[2], "gh@github:user/repo#v1");
        assert_eq!(opt[0], "opt@latest");
        for s in req.iter().chain(opt.iter()) {
            crate::pm::spec::parse_request(s).unwrap_or_else(|e| panic!("{s}: {e}"));
        }
    }
}
