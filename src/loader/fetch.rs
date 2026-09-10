//! 模块抓取（切片 a）：file: 读盘 + data: 解码。http(s) 顺延 Phase 3。

use url::Url;

use crate::error::Error;

pub struct Fetched {
    pub text: String,
}

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
        "http" | "https" => Err(Error::Other(format!(
            "remote module '{url}' needs Phase 3 (fetch)"
        ))),
        s => Err(Error::Other(format!("unsupported module scheme '{s}:'"))),
    }
}
