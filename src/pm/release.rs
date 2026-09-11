//! GitHub release 二进制安装（`[<name>@]release:github/<owner>/<repo>@<tag>/<prefix>`）。
//!
//! - 场景：oxlint/oxfmt 这类“npm 壳 + standalone 二进制”工具——无 node 机器跑不了
//!   npm 版（JS wrapper + `.node`），直接装 GitHub releases 的预编译二进制
//!   （纯 Mach-O/ELF，零依赖）。
//! - 流程：`GET {api}/repos/{owner}/{repo}/releases/tags/{tag}` 取 asset 表 →
//!   `pick_asset` 按当前平台挑包（`{prefix}-{arch}-{os}` 惯例，oxc 系实测）→
//!   下载 → 解包（tar.gz/zip，取首个匹配 `prefix` 的文件）→ 落
//!   `node_modules/.bin/<name>`（unix `+x`）→ lockfile 记
//!   `github-release:<owner>/<repo>@<tag>/<asset>` + sha512。
//! - 可测性：`GITHUB_API` env 覆盖 API 根（stub 回环，黑盒 hermetic）；
//!   `GITHUB_TOKEN` 私人令牌（公开仓库可空；限流 60/h，token 后 5000/h）。
//! - 安全：只记 asset 名 + 成功与否进日志（token/字节不进日志）；tag 必须显式
//!   （不跟 latest 漂，可复现）。

use std::path::Path;

use crate::error::Error;
use crate::pm::spec::ReleaseSpec;

/// GitHub API 根（`GITHUB_API` 覆盖，末尾 `/` 容忍；默认公网 API）。
pub fn api_base() -> String {
    std::env::var("GITHUB_API")
        .map(|v| v.trim_end_matches('/').to_owned())
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "https://api.github.com".to_string())
}

/// release 应答的 asset 子集（其余字段忽略；`pub(crate)` 供单测/黑盒外不可见）。
#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct Asset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct Release {
    #[serde(default)]
    assets: Vec<Asset>,
}

/// 当前平台的 asset 关键词（arch 原样；os 按常见 target 三段式转译）。
fn platform_tokens() -> (String, Vec<&'static str>) {
    let arch = std::env::consts::ARCH.to_string();
    let os: Vec<&'static str> = match std::env::consts::OS {
        "macos" => vec!["apple-darwin", "darwin", "macos"],
        "linux" => vec!["unknown-linux-gnu", "linux-gnu", "linux"],
        "windows" => vec!["pc-windows-msvc", "windows-msvc", "windows"],
        _ => vec![],
    };
    (arch, os)
}

/// 按平台挑 asset（纯函数，单测覆盖判定表）。
/// 规则：`{prefix}-` 开头 + 含 arch + 含任一 os 关键词 + `.tar.gz`/`.zip` 结尾；
/// linux 优先 gnu（musl 靠后），打分后字典序稳选。
pub fn pick_asset<'a>(prefix: &str, assets: &'a [Asset]) -> Option<&'a Asset> {
    let (arch, os_keys) = platform_tokens();
    if os_keys.is_empty() {
        return None;
    }
    let head = format!("{prefix}-");
    let mut cands: Vec<(i32, &Asset)> = Vec::new();
    for a in assets {
        let n = a.name.as_str();
        if !n.starts_with(&head) {
            continue;
        }
        if !(n.ends_with(".tar.gz") || n.ends_with(".tgz") || n.ends_with(".zip")) {
            continue;
        }
        if !n.contains(arch.as_str()) {
            continue;
        }
        if !os_keys.iter().any(|k| n.contains(k)) {
            continue;
        }
        // 打分：gnu/msvc 系优先（musl/zip 靠后，同分字典序稳选）。
        let mut score = 0;
        if n.contains("unknown-linux-gnu") || n.contains("pc-windows-msvc") {
            score += 2;
        }
        if n.ends_with(".tar.gz") || n.ends_with(".tgz") {
            score += 1;
        }
        cands.push((score, a));
    }
    cands.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    cands.into_iter().next().map(|(_, a)| a)
}

/// 取 release 的 asset 表（`GET .../releases/tags/{tag}`；`GITHUB_TOKEN` 可选）。
async fn list_assets(spec: &ReleaseSpec) -> Result<Vec<Asset>, Error> {
    let url = format!(
        "{}/repos/{}/{}/releases/tags/{}",
        api_base(),
        spec.owner,
        spec.repo,
        spec.tag
    );
    let mut req = super::registry::client()
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", concat!("winterjs/", env!("CARGO_PKG_VERSION")));
    if let Ok(tok) = std::env::var("GITHUB_TOKEN")
        && !tok.trim().is_empty()
    {
        req = req.header("Authorization", format!("Bearer {}", tok.trim()));
    }
    tracing::info!(target: "winterjs::pm", owner = spec.owner.as_str(), repo = spec.repo.as_str(), tag = spec.tag.as_str(), "listing release assets");
    let resp = req.send().await.map_err(|e| Error::Other(format!("release lookup failed: {e}")))?;
    match resp.status() {
        s if s.is_success() => {}
        reqwest::StatusCode::NOT_FOUND => {
            return Err(Error::Other(format!(
                "no release '{}/{}@{}' (check tag; need --token? no, this is public API)",
                spec.owner, spec.repo, spec.tag
            )));
        }
        reqwest::StatusCode::FORBIDDEN | reqwest::StatusCode::UNAUTHORIZED => {
            return Err(Error::Other(
                "GitHub API rate limited (set GITHUB_TOKEN to raise the limit)".into(),
            ));
        }
        s => {
            return Err(Error::Other(format!("GitHub API returned {s} for release lookup")));
        }
    }
    let rel: Release = resp
        .json()
        .await
        .map_err(|e| Error::Other(format!("bad release JSON: {e}")))?;
    Ok(rel.assets)
}

/// 求解：asset 名 + 下载 URL（dry-run 打印用；纯网络，无副作用）。
pub async fn resolve_release(spec: &ReleaseSpec) -> Result<(String, String), Error> {
    let assets = list_assets(spec).await?;
    let Some(hit) = pick_asset(&spec.prefix, &assets) else {
        let names: Vec<&str> = assets.iter().map(|a| a.name.as_str()).collect();
        return Err(Error::Other(format!(
            "no {} asset for {}-{} in {}/{}@{} (available: {})",
            spec.prefix,
            std::env::consts::ARCH,
            std::env::consts::OS,
            spec.owner,
            spec.repo,
            spec.tag,
            names.join(", ")
        )));
    };
    Ok((hit.name.clone(), hit.browser_download_url.clone()))
}

/// bin 名（显式名 > prefix）。
pub fn bin_name(spec: &ReleaseSpec) -> &str {
    spec.name.as_deref().unwrap_or(&spec.prefix)
}

/// 下载 → 解包 → 落 `.bin/<name>`；返回 lockfile 四元组
///（name, tag, `github-release:…` resolved 串, integrity）。
pub async fn install_one_release(
    nm_bin: &Path,
    spec: &ReleaseSpec,
) -> Result<(String, String, String, Option<String>), Error> {
    let name = bin_name(spec).to_owned();
    let (asset, url) = resolve_release(spec).await?;
    tracing::info!(target: "winterjs::pm", package = name.as_str(), asset = asset.as_str(), "downloading release binary");
    let bytes = super::registry::client()
        .get(&url)
        .header("User-Agent", concat!("winterjs/", env!("CARGO_PKG_VERSION")))
        .send()
        .await
        .map_err(|e| Error::Other(format!("release download failed: {e}")))?
        .error_for_status()
        .map_err(|e| Error::Other(format!("release download failed: {e}")))?
        .bytes()
        .await
        .map_err(|e| Error::Other(format!("release download failed: {e}")))?;
    let integ = {
        use base64::Engine as _;
        use sha2::Digest as _;
        format!("sha512-{}", base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&bytes)))
    };
    let bin = extract_binary(&bytes, &asset, &spec.prefix)?;
    std::fs::create_dir_all(nm_bin)
        .map_err(|e| Error::Other(format!("cannot create .bin: {e}")))?;
    let dest = nm_bin.join(&name);
    // 原子落盘（tmp+rename；沿 install 同模式）。
    let tmp = nm_bin.join(format!(".tmp-{name}"));
    std::fs::write(&tmp, &bin).map_err(|e| Error::Other(format!("cannot write '{}': {e}", tmp.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| Error::Other(format!("cannot chmod '{}': {e}", tmp.display())))?;
    }
    std::fs::rename(&tmp, &dest).map_err(|e| Error::Other(format!("cannot link .bin/{name}: {e}")))?;
    println!("added {name}@release:{}/{}@{} ({asset})", spec.owner, spec.repo, spec.tag);
    tracing::info!(target: "winterjs::pm", package = name.as_str(), bytes = bin.len(), "release binary installed");
    let resolved = format!("github-release:{}/{}@{}/{}", spec.owner, spec.repo, spec.tag, asset);
    Ok((name, spec.tag.clone(), resolved, Some(integ)))
}

/// 解包取二进制（tar.gz/tgz/zip；首个文件名以 `prefix` 开头的常规文件；
/// 找不着即错——不静默拿第一个，防包结构漂移装错东西）。
fn extract_binary(archive: &[u8], asset: &str, prefix: &str) -> Result<Vec<u8>, Error> {
    if asset.ends_with(".zip") {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive))
            .map_err(|e| Error::Other(format!("bad zip asset '{asset}': {e}")))?;
        let mut hit: Option<usize> = None;
        for i in 0..zip.len() {
            let f = zip.by_index(i).map_err(|e| Error::Other(format!("bad zip asset '{asset}': {e}")))?;
            let fname = f.name().rsplit('/').next().unwrap_or("").to_owned();
            if !f.is_dir() && fname.starts_with(prefix) {
                hit = Some(i);
                break;
            }
        }
        let Some(i) = hit else {
            return Err(Error::Other(format!("no '{prefix}' binary in asset '{asset}'")));
        };
        let mut f = zip.by_index(i).map_err(|e| Error::Other(format!("bad zip asset '{asset}': {e}")))?;
        let mut out = Vec::new();
        use std::io::Read as _;
        f.read_to_end(&mut out).map_err(|e| Error::Other(format!("bad zip asset '{asset}': {e}")))?;
        return Ok(out);
    }
    // tar.gz/tgz（flate2 默认后端 miniz_oxide，纯 Rust，见 §2 门控）。
    let gz = flate2::read::GzDecoder::new(archive);
    let mut tar = tar::Archive::new(gz);
    let entries = tar.entries().map_err(|e| Error::Other(format!("bad tar asset '{asset}': {e}")))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| Error::Other(format!("bad tar asset '{asset}': {e}")))?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path().map_err(|e| Error::Other(format!("bad tar asset '{asset}': {e}")))?;
        let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if fname.starts_with(prefix) {
            let mut out = Vec::new();
            use std::io::Read as _;
            entry.read_to_end(&mut out).map_err(|e| Error::Other(format!("bad tar asset '{asset}': {e}")))?;
            return Ok(out);
        }
    }
    Err(Error::Other(format!("no '{prefix}' binary in asset '{asset}'")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> Asset {
        Asset { name: name.into(), browser_download_url: format!("https://r/{name}") }
    }

    #[test]
    fn pick_asset_oxc_shapes() {
        // oxc 真实命名（apps_v1.82.0 release 表子集）。
        let assets = [
            "oxlint-aarch64-apple-darwin.tar.gz",
            "oxlint-x86_64-apple-darwin.tar.gz",
            "oxlint-x86_64-unknown-linux-gnu.tar.gz",
            "oxlint-x86_64-unknown-linux-musl.tar.gz",
            "oxlint-aarch64-pc-windows-msvc.zip",
            "oxfmt-aarch64-apple-darwin.tar.gz",
            "checksums.txt",
        ]
        .iter()
        .map(|n| asset(n))
        .collect::<Vec<_>>();
        let hit = pick_asset("oxlint", &assets).expect("must match current platform");
        assert!(hit.name.starts_with("oxlint-"), "{}", hit.name);
        assert!(
            hit.name.contains(std::env::consts::ARCH),
            "arch must match: {}",
            hit.name
        );
        // 前缀隔离：oxfmt 不串到 oxlint。
        let hit = pick_asset("oxfmt", &assets).expect("oxfmt present");
        assert!(hit.name.starts_with("oxfmt-"), "{}", hit.name);
        // 无命中：未知前缀/空表。
        assert!(pick_asset("nope", &assets).is_none());
        assert!(pick_asset("oxlint", &[]).is_none());
        // checksums.txt 类非包跳过。
        assert!(pick_asset("checksums", &assets).is_none());
    }

    #[test]
    fn pick_asset_prefers_gnu() {
        // linux 下 gnu 优先 musl（同分字典序兜底，确定性）。
        let assets =
            ["tool-x86_64-unknown-linux-musl.tar.gz", "tool-x86_64-unknown-linux-gnu.tar.gz"]
                .iter()
                .map(|n| asset(n))
                .collect::<Vec<_>>();
        if std::env::consts::OS == "linux" && std::env::consts::ARCH == "x86_64" {
            assert_eq!(pick_asset("tool", &assets).unwrap().name, "tool-x86_64-unknown-linux-gnu.tar.gz");
        }
    }

    #[test]
    fn api_base_override() {
        assert_eq!(api_base(), "https://api.github.com");
    }

    #[test]
    fn bin_name_falls_back_to_prefix() {
        let s = ReleaseSpec {
            name: None,
            owner: "o".into(),
            repo: "r".into(),
            tag: "t".into(),
            prefix: "oxlint".into(),
        };
        assert_eq!(bin_name(&s), "oxlint");
    }

    #[test]
    fn extract_tar_gz_and_zip() {
        // tar.gz：单文件包。
        let mut tar_buf = Vec::new();
        {
            let mut tar = tar::Builder::new(&mut tar_buf);
            let data = b"fake-binary";
            let mut hdr = tar::Header::new_gnu();
            hdr.set_size(data.len() as u64);
            hdr.set_mode(0o755);
            hdr.set_cksum();
            tar.append_data(&mut hdr, "mytool-x86_64-linux-gnu", &data[..]).unwrap();
        }
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        use std::io::Write as _;
        enc.write_all(&tar_buf).unwrap();
        let tgz = enc.finish().unwrap();
        assert_eq!(
            extract_binary(&tgz, "mytool-x86_64-linux-gnu.tar.gz", "mytool").unwrap(),
            b"fake-binary"
        );
        // 前缀不对即错（不静默拿第一个）。
        assert!(extract_binary(&tgz, "mytool-x86_64-linux-gnu.tar.gz", "other").is_err());
        // zip：需开写入（zip 2 系读+写同 crate，deflate 门控内）。
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            zip.start_file("mytool-x86_64-linux-gnu", zip::write::SimpleFileOptions::default()).unwrap();
            use std::io::Write as _;
            zip.write_all(b"fake-zip").unwrap();
            zip.finish().unwrap();
        }
        assert_eq!(
            extract_binary(buf.get_ref(), "mytool-x86_64-linux-gnu.zip", "mytool").unwrap(),
            b"fake-zip"
        );
    }
}
