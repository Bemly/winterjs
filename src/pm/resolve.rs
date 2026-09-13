//! 版本求解（`deno_semver` npm 语义 + dist-tag；纯逻辑，单元测试覆盖）。
//! - range 解析失败 → 当 dist-tag 查（`latest` 等）；tag 缺失即错。
//! - 精确版亦走 range 匹配（`1.2.3` 即 `=1.2.3`）；取满足的最大版（`semver` 排序）。
//! - 垃圾版本号（任一解析失败）跳过；全灭即错。
//! - `solve_tree` 广度遍历传递依赖（同名以先定为准，npm 简化树；深度守卫 50）。
//! - `optionalDependencies`：仅平台命中（`platform::platform_matches`）的进树，
//!   记 `optional`；其子树继承 optional（仅经 optional 边可达才容忍失败）；
//!   必需边可达即升级为必需（npm 口径：双重可达按必需算）。

use std::collections::{HashMap, VecDeque};

use crate::pm::registry::Packument;
use crate::pm::spec::Spec;

/// 解出的一项（tarball + 完整性；5b 落地用；`optional` 失败容忍，见 `install`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub name: String,
    pub version: String,
    pub tarball: String,
    pub integrity: Option<String>,
    pub optional: bool,
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
/// 边分必需/可选：可选包的子树继承可选；必需边可达即升级为必需。
/// 平台不命中（`os`/`cpu`）的版本直接跳过（不报错；npm 口径）。
/// 可选根（manifest `optionalDependencies`）走 `solve_tree_rooted`。
pub async fn solve_tree_rooted<F, Fut>(
    required_roots: &[Spec],
    optional_roots: &[Spec],
    mut fetch: F,
) -> Result<Vec<Resolved>, String>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<Packument, String>>,
{
    let mut out: Vec<Resolved> = Vec::new();
    // pin 表：名 → 版本（先定为准）；`required` 表：false 表仅经可选边可达。
    let mut pinned: HashMap<String, String> = HashMap::new();
    let mut required: HashMap<String, bool> = HashMap::new();
    // 必需根排前、可选根排后（同名时必需先 pin，可选边经 pin 短路）。
    let mut queue: VecDeque<(String, String, bool)> = required_roots
        .iter()
        .map(|s| (s.name.clone(), s.range.clone(), false))
        .chain(optional_roots.iter().map(|s| (s.name.clone(), s.range.clone(), true)))
        .collect();
    // 步数上限（防病态图；按弹出计，与深度成正比）。
    let mut steps = 0usize;
    while let Some((name, req, via_optional)) = queue.pop_front() {
        steps += 1;
        if steps > 5000 {
            return Err("dependency tree too large (cycle?)".into());
        }
        if let Some(ver) = pinned.get(&name).cloned() {
            // 必需边命中仅可选包 → 升级为必需，子树按必需重走（版本不动）。
            if !via_optional && required.get(&name).is_some_and(|r| !r) {
                required.insert(name.clone(), true);
                if let Some(r) = out.iter_mut().find(|r| r.name == name) {
                    r.optional = false;
                }
                // 子树重入队（版本已 pin，只为把必需性冒泡下去；去重由 pin 挡）。
                if let Ok(pack) = fetch(name.clone()).await {
                    if let Some(meta) = pack.versions.get(&ver) {
                        for (dep, dep_req) in &meta.dependencies {
                            queue.push_back((dep.clone(), dep_req.clone(), false));
                        }
                        for (dep, dep_req) in &meta.optional_dependencies {
                            queue.push_back((dep.clone(), dep_req.clone(), true));
                        }
                    }
                }
            }
            continue;
        }
        let pack = match fetch(name.clone()).await {
            Ok(p) => p,
            Err(e) => {
                // 可选边拉取失败即容忍（跳过整支；npm 口径）。
                if via_optional {
                    tracing::debug!(target: "winterjs::pm", package = name.as_str(), "optional fetch failed, skipping: {e}");
                    continue;
                }
                return Err(e);
            }
        };
        let (version, meta) = match pick(&pack, &req) {
            Ok(v) => v,
            Err(e) => {
                if via_optional {
                    tracing::debug!(target: "winterjs::pm", package = name.as_str(), "optional resolve failed, skipping: {e}");
                    continue;
                }
                return Err(e);
            }
        };
        // 平台过滤（按选定版本的 os/cpu；不命中即整支跳过，不报错）。
        if !crate::pm::platform::platform_matches(
            meta.os.as_deref(),
            meta.cpu.as_deref(),
        ) {
            tracing::debug!(target: "winterjs::pm", package = name.as_str(), version = version.as_str(), "platform skipped");
            continue;
        }
        pinned.insert(name.clone(), version.clone());
        required.insert(name.clone(), !via_optional);
        // 子边继承：可选包的子树全继承可选（仅经可选边可达才容忍失败）。
        for (dep, dep_req) in &meta.dependencies {
            queue.push_back((dep.clone(), dep_req.clone(), via_optional));
        }
        // 可选依赖：只收录（其子树继承可选）；拉取/求解失败在弹出时容忍。
        // 注意：平台过滤在弹出时按版本做（此处只收名 + req，不过滤）。
        for (dep, dep_req) in &meta.optional_dependencies {
            queue.push_back((dep.clone(), dep_req.clone(), true));
        }
        out.push(Resolved {
            name,
            version,
            tarball: meta.dist.tarball.clone(),
            integrity: meta.dist.integrity.clone().or(meta.dist.shasum.clone()),
            optional: via_optional,
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
            optional_dependencies: HashMap::new(),
            os: None,
            cpu: None,
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
                    optional_dependencies: [].into(),
                    os: None,
                    cpu: None,
                },
            )]
            .into(),
        };
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let out = rt
            .block_on(solve_tree_rooted(&[Spec { name: "lib".into(), range: "^2.0.0".into() }], &[], |name: String| {
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
        // 必需边全非可选。
        assert!(out.iter().all(|r| !r.optional));
    }

    fn opt_pack() -> (Packument, Packument, Packument, Packument) {
        // tool → 必需 dep + 可选 opt-hit（本平台）/ opt-miss（异平台）/ opt-404（拉取失败）。
        let meta = |deps: &[(&str, &str)], opts: &[(&str, &str)], os: Option<&str>| VersionMeta {
            dist: Dist { tarball: "https://r/x.tgz".into(), integrity: None, shasum: None },
            dependencies: deps.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            optional_dependencies: opts.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            os: os.map(|o| vec![o.to_string()]),
            cpu: None,
        };
        let one = |os: Option<&str>| Packument {
            name: "x".into(),
            dist_tags: [("latest".to_string(), "1.0.0".to_string())].into(),
            versions: [("1.0.0".to_string(), meta(&[], &[], os))].into(),
        };
        let tool = Packument {
            name: "tool".into(),
            dist_tags: [("latest".to_string(), "1.0.0".to_string())].into(),
            versions: [(
                "1.0.0".to_string(),
                meta(&[("dep", "^1.0.0")], &[("opt-hit", "*"), ("opt-miss", "*"), ("opt-404", "*")], None),
            )]
            .into(),
        };
        (tool, one(None), one(Some("nonexistent-os")), one(None))
    }

    #[test]
    fn solve_tree_optional_platform_and_failure() {
        let (tool, hit, miss, _) = opt_pack();
        let dep = Packument {
            name: "dep".into(),
            dist_tags: [].into(),
            versions: [(
                "1.0.0".to_string(),
                VersionMeta {
                    dist: Dist { tarball: "https://r/dep.tgz".into(), integrity: None, shasum: None },
                    dependencies: [].into(),
                    optional_dependencies: [].into(),
                    os: None,
                    cpu: None,
                },
            )]
            .into(),
        };
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let out = rt
            .block_on(solve_tree_rooted(&[Spec { name: "tool".into(), range: "*".into() }], &[], |name: String| {
                let (tool, hit, miss, dep) =
                    (tool.clone(), hit.clone(), miss.clone(), dep.clone());
                async move {
                    match name.as_str() {
                        "tool" => Ok(tool),
                        "opt-hit" => Ok(hit),
                        "opt-miss" => Ok(miss),
                        "dep" => Ok(dep),
                        // opt-404 拉取失败 → 容忍跳过。
                        other => Err(format!("missing stub '{other}'")),
                    }
                }
            }))
            .unwrap();
        let names: Vec<&str> = out.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"tool") && names.contains(&"dep"), "{names:?}");
        // 本平台可选命中且记 optional；异平台整支跳过；404 容忍跳过。
        let hit_r = out.iter().find(|r| r.name == "opt-hit").expect("opt-hit pinned");
        assert!(hit_r.optional, "opt-hit must be optional");
        assert!(!names.contains(&"opt-miss"), "platform mismatch must skip: {names:?}");
        assert!(!names.contains(&"opt-404"), "fetch failure must skip: {names:?}");
        assert!(!out.iter().find(|r| r.name == "tool").unwrap().optional);
        assert!(!out.iter().find(|r| r.name == "dep").unwrap().optional);
    }

    #[test]
    fn solve_tree_optional_roots() {
        // 可选根：命中的装上（记 optional）、404 容忍跳过；必需根 + 可选根同名
        // 时必需优先（版本不动、不降级为 optional）。
        let one = |name: &str| Packument {
            name: name.into(),
            dist_tags: [("latest".to_string(), "1.0.0".to_string())].into(),
            versions: [(
                "1.0.0".to_string(),
                VersionMeta {
                    dist: Dist { tarball: format!("https://r/{name}.tgz"), integrity: None, shasum: None },
                    dependencies: [].into(),
                    optional_dependencies: [].into(),
                    os: None,
                    cpu: None,
                },
            )]
            .into(),
        };
        let (hit, shared) = (one("opt-hit"), one("shared"));
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let out = rt
            .block_on(solve_tree_rooted(
                &[Spec { name: "shared".into(), range: "*".into() }],
                &[
                    Spec { name: "opt-hit".into(), range: "*".into() },
                    Spec { name: "opt-404".into(), range: "*".into() },
                    Spec { name: "shared".into(), range: ">=0.5.0".into() },
                ],
                |name: String| {
                    let (hit, shared) = (hit.clone(), shared.clone());
                    async move {
                        match name.as_str() {
                            "opt-hit" => Ok(hit),
                            "shared" => Ok(shared),
                            other => Err(format!("missing stub '{other}'")),
                        }
                    }
                },
            ))
            .unwrap();
        let shared_r = out.iter().find(|r| r.name == "shared").expect("shared pinned");
        assert!(!shared_r.optional, "required root must stay required: {out:?}");
        let hit_r = out.iter().find(|r| r.name == "opt-hit").expect("opt-hit pinned");
        assert!(hit_r.optional, "optional root must be optional");
        assert!(out.len() == 2, "opt-404 tolerated: {out:?}");
    }

    #[test]
    fn solve_tree_optional_upgraded_by_required() {
        // BFS 序决定 shared 先经可选边 pin（optional），随后被必需边命中 → 升级为必需
        //（版本不动；npm 口径：双重可达按必需算）。
        let meta = |deps: &[(&str, &str)], opts: &[(&str, &str)]| VersionMeta {
            dist: Dist { tarball: "https://r/x.tgz".into(), integrity: None, shasum: None },
            dependencies: deps.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            optional_dependencies: opts.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            os: None,
            cpu: None,
        };
        let one = |deps: &[(&str, &str)], opts: &[(&str, &str)]| Packument {
            name: "x".into(),
            dist_tags: [("latest".to_string(), "1.0.0".to_string())].into(),
            versions: [("1.0.0".to_string(), meta(deps, opts))].into(),
        };
        let a = one(&[], &[("shared", "*")]);
        let b = one(&[("shared", "*")], &[]);
        let shared = one(&[], &[]);
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let out = rt
            .block_on(solve_tree_rooted(
                &[Spec { name: "a".into(), range: "*".into() }, Spec { name: "b".into(), range: "*".into() }],
                &[],
                |name: String| {
                    let (a, b, shared) = (a.clone(), b.clone(), shared.clone());
                    async move {
                        match name.as_str() {
                            "a" => Ok(a),
                            "b" => Ok(b),
                            "shared" => Ok(shared),
                            other => Err(format!("missing stub '{other}'")),
                        }
                    }
                },
            ))
            .unwrap();
        let shared_r = out.iter().find(|r| r.name == "shared").expect("shared pinned");
        assert!(!shared_r.optional, "required-edge reachable must upgrade: {out:?}");
        assert_eq!(shared_r.version, "1.0.0");
    }
}
