//! 模块抓取：file: 读盘 + data: 解码 + http(s) 远端（reqwest，独立线程）。
//!
//! - http(s)：调用方是同步的 JS 线程（模块 hook 内），直接 `.await` 会嵌套
//!   runtime；此处起一次性 OS 线程 + 自建 current-thread runtime 跑异步 reqwest
//!   （与 `run_isolated` 同构的隔离，见 §4.24），join 回结果。
//! - 口径（文档记录）：跟随重定向（reqwest 默认 10 跳）；任意 content-type 当 JS
//!   收（Node 式宽容）；10MB 上限防 OOM；30s 超时；TLS 与全图同源 ring。
//!   远端模块不走转译磁盘缓存（内存 lru 照走，见 transpile）。

use url::Url;

use crate::error::Error;

pub struct Fetched {
    pub text: String,
}

/// 远端上限（10MB；超即报，不截断）。
pub const REMOTE_MAX_BYTES: usize = 10 * 1024 * 1024;

/// 取回已解析 URL 的源码。
pub fn fetch(url: &Url) -> Result<Fetched, Error> {
    match url.scheme() {
        "file" => {
            let path = url
                .to_file_path()
                .map_err(|_| Error::Other(format!("bad file URL: {url}")))?;
            let text = fs_err::read_to_string(&path)
                .map_err(|source| Error::IoRead { path, source })?;
            tracing::debug!(target: "winterjs::loader", url = url.as_str(), bytes = text.len(), "fetched file module");
            Ok(Fetched { text })
        }
        "data" => {
            let data_url = data_url::DataUrl::process(url.as_str())
                .map_err(|e| Error::Other(format!("bad data: URL ({e:?})")))?;
            let (bytes, _) = data_url
                .decode_to_vec()
                .map_err(|e| Error::Other(format!("bad data: body ({e:?})")))?;
            let text = String::from_utf8(bytes)
                .map_err(|e| Error::Other(format!("data: module is not UTF-8 ({e})")))?;
            tracing::debug!(target: "winterjs::loader", bytes = text.len(), "fetched data module");
            Ok(Fetched { text })
        }
        "http" | "https" => fetch_remote(url),
        s => Err(Error::Other(format!("unsupported module scheme '{s}:'"))),
    }
}

/// 远端抓取（同步外壳 → 独立线程 + 自建 runtime → 异步 reqwest）。
fn fetch_remote(url: &Url) -> Result<Fetched, Error> {
    let url = url.clone();
    std::thread::Builder::new()
        .name("wjs-loader-fetch".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| Error::Other(format!("remote fetch runtime failed: {e}")))?;
            rt.block_on(fetch_remote_async(&url))
        })
        .map_err(|e| Error::Other(format!("remote fetch thread failed: {e}")))?
        .join()
        .map_err(|_| Error::Other("remote fetch thread panicked".into()))?
}

/// 远端抓取本体（独立线程内跑；纯 reqwest，无 JS 交互）。
async fn fetch_remote_async(url: &Url) -> Result<Fetched, Error> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .user_agent(concat!("winterjs/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| Error::Other(format!("remote fetch client failed: {e}")))?;
    tracing::info!(target: "winterjs::loader", url = url.as_str(), "fetching remote module");
    let resp = client
        .get(url.clone())
        .header("Accept", "*/*")
        .send()
        .await
        .map_err(|e| Error::Other(format!("cannot fetch remote module '{url}': {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(Error::Other(format!(
            "remote module '{url}' returned {status}"
        )));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| Error::Other(format!("cannot read remote module '{url}': {e}")))?;
    if bytes.len() > REMOTE_MAX_BYTES {
        return Err(Error::Other(format!(
            "remote module '{url}' exceeds {} bytes",
            REMOTE_MAX_BYTES
        )));
    }
    let text = String::from_utf8(bytes.into())
        .map_err(|e| Error::Other(format!("remote module '{url}' is not UTF-8 ({e})")))?;
    tracing::debug!(target: "winterjs::loader", url = url.as_str(), bytes = text.len(), "fetched remote module");
    Ok(Fetched { text })
}
