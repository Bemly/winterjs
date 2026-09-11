//! registry 客户端（packument 拉取；`reqwest` 复用 Phase 3 的 ring 门控）。
//! 默认 `https://registry.npmjs.org`（`--registry` 覆盖；镜像/npmrc 顺延 5d）。

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use crate::error::Error;

pub const DEFAULT_REGISTRY: &str = "https://registry.npmjs.org";

/// packument 最小子集（版本表 + dist-tag + 依赖；其余字段忽略）。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Packument {
    pub name: String,
    // npm 线名含 `-`/驼峰，serde 缺省按 Rust 名匹配会静默丢字段（`dist-tags`/
    // `optionalDependencies` 曾因此全空：tag 安装与可选依赖双双失效，见 §4 记）。
    #[serde(rename = "dist-tags", default)]
    pub dist_tags: HashMap<String, String>,
    #[serde(default)]
    pub versions: HashMap<String, VersionMeta>,
}

/// 单版本元信息（tarball + 完整性 + 依赖）。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct VersionMeta {
    pub dist: Dist,
    #[serde(default)]
    pub dependencies: HashMap<String, String>,
    /// 可选依赖（失败容忍，见 `resolve`；oxlint 类平台二进制全在此列）。
    #[serde(rename = "optionalDependencies", default)]
    pub optional_dependencies: HashMap<String, String>,
    /// 平台限定（`os`/`cpu` 数组；缺省表全平台，见 `platform`）。
    #[serde(default)]
    pub os: Option<Vec<String>>,
    #[serde(default)]
    pub cpu: Option<Vec<String>>,
}

/// 分发信息（integrity 优先，shasum 兜底；皆无则 5b 拒绝）。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Dist {
    pub tarball: String,
    #[serde(default)]
    pub integrity: Option<String>,
    #[serde(default)]
    pub shasum: Option<String>,
}

pub(crate) fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        // TLS provider 与 fetch 同源（顶层 ring；reqwest 侧 no-provider，见 §2 门控）。
        let _ = rustls::crypto::ring::default_provider().install_default();
        reqwest::Client::builder()
            .user_agent(concat!("winterjs/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest client builds")
    })
}

/// 拉取 packument（`{registry}/{name}`；name 含 scope 即原样拼）。
/// `token` 为 npmrc `_authToken`（按 registry host 匹配到来）时带
/// `Authorization: Bearer`（npm 私有仓口径；无则匿名）。
pub async fn fetch_packument(
    registry: &str,
    name: &str,
    token: Option<&str>,
) -> Result<Packument, Error> {
    let base = registry.trim_end_matches('/');
    let url = format!("{base}/{name}");
    tracing::info!(target: "winterjs::pm", url = url.as_str(), has_auth = token.is_some(), "fetching packument");
    let mut req = client().get(&url).header("Accept", "application/json");
    if let Some(t) = token.filter(|s| !s.trim().is_empty()) {
        req = req.header("Authorization", format!("Bearer {}", t.trim()));
    }
    let resp = req.send().await.map_err(|e| {
        Error::Other(format!("registry request failed for '{name}': {e}"))
    })?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(Error::Other(format!("package '{name}' not found in registry")));
    }
    if !resp.status().is_success() {
        return Err(Error::Other(format!(
            "registry returned {} for '{name}'",
            resp.status()
        )));
    }
    resp.json::<Packument>().await.map_err(|e| {
        Error::Other(format!("bad packument for '{name}': {e}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn npm_field_names_deserialize() {
        // 回归：`dist-tags`（kebab）/`optionalDependencies`（驼峰）必须落进结构体；
        // 缺 rename 即静默全空（tag 安装与可选依赖双双失效，曾实发）。
        let v: Packument = serde_json::from_str(
            r#"{"name":"p","dist-tags":{"latest":"1.0.0"},"versions":{"1.0.0":{
                "dist":{"tarball":"https://r/p.tgz"},
                "dependencies":{"a":"^1.0.0"},
                "optionalDependencies":{"b":"*"},
                "os":["darwin"],"cpu":["arm64"]}}}"#,
        )
        .unwrap();
        assert_eq!(v.dist_tags.get("latest").map(String::as_str), Some("1.0.0"));
        let m = v.versions.get("1.0.0").unwrap();
        assert_eq!(m.dependencies.get("a").map(String::as_str), Some("^1.0.0"));
        assert_eq!(m.optional_dependencies.get("b").map(String::as_str), Some("*"));
        assert_eq!(m.os.as_deref(), Some(["darwin".to_string()].as_slice()));
        assert_eq!(m.cpu.as_deref(), Some(["arm64".to_string()].as_slice()));
    }
}
