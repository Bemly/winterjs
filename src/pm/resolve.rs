//! 版本求解（`deno_semver` npm 语义 + dist-tag；纯逻辑，单元测试覆盖）。
//! - range 解析失败 → 当 dist-tag 查（`latest` 等）；tag 缺失即错。
//! - 精确版亦走 range 匹配（`1.2.3` 即 `=1.2.3`）；取满足的最大版（`semver` 排序）。
//! - 垃圾版本号（任一解析失败）跳过；全灭即错。
//! - `solve_tree` 广度遍历传递依赖（同名以先定为准，npm 简化树；深度守卫 50）。

use std::collections::{HashMap, VecDeque};

use crate::pm::registry::Packument;
use crate::pm::spec::Spec;

/// 解出的一项（tarball + 完整性；5b 落地用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub name: String,
    pub version: String,
    pub tarball: String,
    pub integrity: Option<String>,
}

/// 在 packument 内按 req 选版（tag 或 range；返回版本号串 + 元信息）。
pub fn pick(pack: &Packument, req: &str) -> Result<(String, crate::pm::registry::VersionMeta), String> {
    // 先解析（`matches` 遇 tag 即 panic，故 tag 必须先分流，见 deno 文档）。
    let parsed = deno_semver::VersionReq::parse_from_npm(req)
        .map_err(|_| format!("bad version range '{req}'"))?;
    // dist-tag 走 tag 表（npm 语义；`latest` 等）。
    if let Some(tag) = parsed.tag() {
        if let Some(ver) = pack.dist_tags.get(tag) {
            if let Some(meta) = pack.versions.get(ver) {
                return Ok((ver.clone(), meta.clone()));
            }
            return Err(format!("dist-tag '{tag}' points to missing version '{ver}'"));
        }
        return Err(format!("unknown dist-tag '{tag}'"));
    }
    let range = parsed;
    // 候选：双解析（deno 匹配 + semver 排序），垃圾版跳过。
    let mut cands: Vec<(semver::Version, String)> = Vec::new();
    for ver in pack.versions.keys() {
        let (Ok(dv), Ok(sv)) = (
            deno_semver::Version::parse_from_npm(ver),
            semver::Version::parse(ver),
        ) else {
            continue;
        };
        if range.matches(&dv) {
            cands.push((sv, ver.clone()));
        }
    }
    cands.sort_by(|a, b| a.0.cmp(&b.0));
    match cands.pop() {
        Some((_, ver)) => {
            let meta = pack.versions.get(&ver).cloned().unwrap();
            Ok((ver, meta))
        }
        None => Err(format!("no version of '{}' satisfies '{req}'", pack.name)),
    }
}

/// 顶层 specs → 传递闭包（BFS；同名先定为准；fetch 由调用方注入，便于单测）。
pub async fn solve_tree<F, Fut>(
    specs: &[Spec],
    mut fetch: F,
) -> Result<Vec<Resolved>, String>
where
    // owned 传参（闭包返回 future 借用外名则生命周期无解；调用方 clone 即可）。
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<Packument, String>>,
{
    let mut out: Vec<Resolved> = Vec::new();
    let mut pinned: HashMap<String, String> = HashMap::new();
    let mut queue: VecDeque<(String, String)> =
        specs.iter().map(|s| (s.name.clone(), s.range.clone())).collect();
    // 步数上限（防病态图；按弹出计，与深度成正比）。
    let mut steps = 0usize;
    while let Some((name, req)) = queue.pop_front() {
        steps += 1;
        if steps > 5000 {
            return Err("dependency tree too large (cycle?)".into());
        }
        if pinned.contains_key(&name) {
            continue;
        }
        let pack = fetch(name.clone()).await?;
        let (version, meta) = pick(&pack, &req)?;
        pinned.insert(name.clone(), version.clone());
        for (dep, dep_req) in &meta.dependencies {
            if !pinned.contains_key(dep) {
                queue.push_back((dep.clone(), dep_req.clone()));
            }
        }
        out.push(Resolved {
            name,
            version,
            tarball: meta.dist.tarball.clone(),
            integrity: meta.dist.integrity.clone().or(meta.dist.shasum.clone()),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pm::registry::{Dist, Packument, VersionMeta};

    fn pack() -> Packument {
        let v = |tarball: &str, deps: &[(&str, &str)]| VersionMeta {
            dist: Dist { tarball: tarball.into(), integrity: None, shasum: None },
            dependencies: deps.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        };
        Packument {
            name: "lib".into(),
            dist_tags: [("latest".to_string(), "2.1.0".to_string())].into(),
            versions: [
                ("1.0.0".to_string(), v("https://r/lib-1.0.0.tgz", &[])),
                ("2.0.0".to_string(), v("https://r/lib-2.0.0.tgz", &[("dep", "^1.0.0")])),
                ("2.1.0".to_string(), v("https://r/lib-2.1.0.tgz", &[("dep", "^1.0.0")])),
                ("garbage!!".to_string(), v("https://r/g.tgz", &[])),
            ]
            .into(),
        }
    }

    #[test]
    fn pick_range_max() {
        let (ver, _) = pick(&pack(), "^2.0.0").unwrap();
        assert_eq!(ver, "2.1.0");
        let (ver, _) = pick(&pack(), "1.x").unwrap();
        assert_eq!(ver, "1.0.0");
        assert!(pick(&pack(), "^3.0.0").is_err());
    }

    #[test]
    fn pick_tag_and_exact() {
        let (ver, _) = pick(&pack(), "latest").unwrap();
        assert_eq!(ver, "2.1.0");
        let (ver, _) = pick(&pack(), "1.0.0").unwrap();
        assert_eq!(ver, "1.0.0");
        assert!(pick(&pack(), "beta").is_err());
        assert!(pick(&pack(), ">=2.0.0 <2.1.0").unwrap().0 == "2.0.0");
    }

    #[test]
    fn solve_tree_transitive() {
        let lib = pack();
        let dep = Packument {
            name: "dep".into(),
            dist_tags: [].into(),
            versions: [(
                "1.2.0".to_string(),
                VersionMeta {
                    dist: Dist { tarball: "https://r/dep-1.2.0.tgz".into(), integrity: Some("sha512-abc=".into()), shasum: None },
                    dependencies: [].into(),
                },
            )]
            .into(),
        };
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let out = rt
            .block_on(solve_tree(&[Spec { name: "lib".into(), range: "^2.0.0".into() }], |name: String| {
                let lib = lib.clone();
                let dep = dep.clone();
                async move {
                    match name.as_str() {
                        "lib" => Ok(lib),
                        "dep" => Ok(dep),
                        other => Err(format!("missing stub '{other}'")),
                    }
                }
            }))
            .unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].name, "dep");
        assert_eq!(out[1].version, "2.1.0");
        assert_eq!(out[1].integrity, None);
        assert_eq!(out[0].integrity.as_deref(), Some("sha512-abc="));
    }
}
