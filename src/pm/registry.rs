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
    #[serde(default)]
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
pub async fn fetch_packument(registry: &str, name: &str) -> Result<Packument, Error> {
    let base = registry.trim_end_matches('/');
    let url = format!("{base}/{name}");
    tracing::info!(target: "winterjs::pm", url = url.as_str(), "fetching packument");
    let resp = client().get(&url).header("Accept", "application/json").send().await.map_err(|e| {
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
