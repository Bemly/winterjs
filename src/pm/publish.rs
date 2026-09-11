//! 发布与登录（plan Phase 5d-d3/c-4x）：`publish` 真 PUT + `login` token 落盘 + OAuth 交换。
//!
//! - `publish`：本地校验（名/版本/license/文件表）→ 打 tarball（`tar` + `flate2`
//!   gzip，经 `package/` 前缀打包，npm 口径）→ `PUT {registry}/{name}`
//!   （npm 注册表协议最小子集：versions + dist-tags + _attachments；
//!   `Authorization: Bearer <token>`，token 按 registry host 从 npmrc 取，
//!   无则报指路 `--login`）；`--dry-run` 只校验打印，不碰网络。
//! - `login --token`：把 `//<host>/:_authToken` upsert 进 `$HOME/.npmrc`
//!   （保留其他行，原子写）；无 token 且 TTY 时经 `dialoguer` 密码提示，
//!   非 TTY 即报缺 `--token`（hermetic 可测）。
//! - `login --oauth`：打印授权 URL（`webbrowser` 试开）；`--token code:<code>`
//!   时用 `oauth2` 走 Authorization Code 交换拿 token 并落盘（local redirect
//!   `http://localhost/callback`，与拼 URL 时一致）。
//! - license：`spdx` 轮子解析表达式；缺失放行（WARN），非法即错。
//! - 安全：token 值永不进日志（只记目标 host + 成功与否）。

use std::path::Path;

use crate::error::Error;

/// 校验通过的 manifest 摘要（dry-run 打印用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Validated {
    pub name: String,
    pub version: String,
    pub license: Option<String>,
    pub files: usize,
}

/// manifest 校验（名/版本必填，版本走 `semver`，license 走 `spdx`）。
pub fn validate_manifest(dir: &Path) -> Result<Validated, String> {
    let text =
        std::fs::read_to_string(dir.join("package.json")).map_err(|e| format!("cannot read package.json: {e}"))?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("bad package.json: {e}"))?;
    let name = v
        .get("name")
        .and_then(|n| n.as_str())
        .filter(|n| !n.trim().is_empty())
        .ok_or_else(|| "package.json has no name".to_string())?
        .to_owned();
    let version = v
        .get("version")
        .and_then(|n| n.as_str())
        .filter(|n| !n.trim().is_empty())
        .ok_or_else(|| format!("package '{name}' has no version"))?
        .to_owned();
    semver::Version::parse(&version)
        .map_err(|e| format!("package '{name}' has bad version '{version}': {e}"))?;
    let license = match v.get("license").and_then(|l| l.as_str()) {
        Some(l) if !l.trim().is_empty() => {
            l.parse::<spdx::Expression>()
                .map_err(|e| format!("package '{name}' has bad license '{l}': {e}"))?;
            Some(l.to_owned())
        }
        _ => {
            tracing::warn!(target: "winterjs::pm", package = name.as_str(), "no license field (publishing unlicensed)");
            None
        }
    };
    // 文件表：`files` 数组逐项存在性检查；缺省全量计数（跳过 `.git/node_modules/target`）。
    let files = match v.get("files") {
        Some(serde_json::Value::Array(list)) => {
            for entry in list {
                let rel = entry.as_str().ok_or_else(|| format!("package '{name}': files entries must be strings"))?;
                if !dir.join(rel).exists() {
                    return Err(format!("package '{name}': files entry '{rel}' does not exist"));
                }
            }
            list.len()
        }
        Some(_) => return Err(format!("package '{name}': files must be an array")),
        None => count_files(dir),
    };
    Ok(Validated { name, version, license, files })
}

/// 全量文件计数（跳过 `.git`/`node_modules`/`target` 顶层目录）。
fn count_files(dir: &Path) -> usize {
    walkdir::WalkDir::new(dir)
        .min_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            !e.path()
                .strip_prefix(dir)
                .ok()
                .and_then(|r| r.components().next())
                .is_some_and(|c| matches!(c.as_os_str().to_str(), Some(".git" | "node_modules" | "target")))
        })
        .filter(|e| e.file_type().is_file())
        .take(100_001)
        .count()
}

/// 打包 tarball（内存；`package/` 前缀，npm 口径；`files` 有则只包所列，无则全量）。
/// 返回 `(tgz_bytes, file_list)`（file_list 为包内相对路径，供 dry-run/PUT 共用）。
pub fn pack_tarball(dir: &Path, v: &Validated) -> Result<(Vec<u8>, Vec<String>), String> {
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("package.json")).map_err(|e| format!("cannot read package.json: {e}"))?,
    )
    .map_err(|e| format!("bad package.json: {e}"))?;
    let listed: Option<Vec<String>> = manifest
        .get("files")
        .and_then(|f| f.as_array())
        .map(|a| a.iter().filter_map(|e| e.as_str().map(|s| s.to_string())).collect());
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    if let Some(list) = listed {
        for rel in list {
            let bytes = std::fs::read(dir.join(&rel))
                .map_err(|e| format!("package '{}': files entry '{rel}' unreadable: {e}", v.name))?;
            files.push((rel, bytes));
        }
        // package.json 必带（npm 口径：即使 files 未列也含）。
        if !files.iter().any(|(r, _)| r == "package.json") {
            let bytes = std::fs::read(dir.join("package.json"))
                .map_err(|e| format!("cannot read package.json: {e}"))?;
            files.push(("package.json".into(), bytes));
        }
    } else {
        for entry in walkdir::WalkDir::new(dir)
            .min_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let rel = entry.path().strip_prefix(dir).unwrap_or(entry.path()).to_string_lossy().into_owned();
            if matches!(rel.split('/').next(), Some(".git" | "node_modules" | "target")) {
                continue;
            }
            let bytes = std::fs::read(entry.path()).map_err(|e| format!("cannot read '{}': {e}", entry.path().display()))?;
            files.push((rel, bytes));
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    // tar（package/ 前缀）→ gzip（flate2 默认后端 miniz_oxide，纯 Rust，见 §2 门控）。
    let mut tar_buf = Vec::new();
    {
        let mut tar = tar::Builder::new(&mut tar_buf);
        for (rel, bytes) in &files {
            let mut hdr = tar::Header::new_gnu();
            hdr.set_size(bytes.len() as u64);
            hdr.set_mode(0o644);
            hdr.set_mtime(0);
            hdr.set_cksum();
            tar.append_data(&mut hdr, format!("package/{rel}"), bytes.as_slice())
                .map_err(|e| format!("tar failed: {e}"))?;
        }
        tar.into_inner().map_err(|e| format!("tar failed: {e}"))?;
    }
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    use std::io::Write as _;
    enc.write_all(&tar_buf).map_err(|e| format!("gzip failed: {e}"))?;
    let tgz = enc.finish().map_err(|e| format!("gzip failed: {e}"))?;
    let names = files.into_iter().map(|(r, _)| r).collect();
    Ok((tgz, names))
}

/// `publish`（dry-run 打印摘要；真发布走 npm 注册表 `PUT {registry}/{name}`）。
pub async fn publish(dir: &Path, dry_run: bool, registry: &str, tag: &str) -> Result<(), Error> {
    let v = validate_manifest(dir).map_err(Error::Other)?;
    let (tgz, names) = pack_tarball(dir, &v).map_err(Error::Other)?;
    if dry_run {
        println!("{}@{} (tag {tag})", v.name, v.version);
        println!("registry: {registry}");
        println!("license: {}", v.license.as_deref().unwrap_or("(none)"));
        println!("files: {}", v.files);
        println!("tarball: {} bytes, {} file(s)", tgz.len(), names.len());
        tracing::info!(target: "winterjs::pm", package = v.name.as_str(), version = v.version.as_str(), files = v.files, "publish dry-run ok");
        return Ok(());
    }
    // token 按生效 registry host 从 npmrc 取（`<dir>/.npmrc` > `$HOME/.npmrc`）。
    let (project, home) = super::npmrc::load_cwd_and_home(dir);
    let token = project
        .auth_token_for(registry)
        .or_else(|| home.auth_token_for(registry));
    let Some(token) = token else {
        return Err(Error::Other(format!(
            "no auth token for {registry} (run `winterjs --login --token <token> --registry {registry}` first)"
        )));
    };
    put_package(registry, &token, &v, tag, &tgz).await
}

/// 真 PUT（npm 注册表最小子集；`tag` 默认为 `latest`，调用方已默认）。
async fn put_package(registry: &str, token: &str, v: &Validated, tag: &str, tgz: &[u8]) -> Result<(), Error> {
    use base64::Engine as _;
    let base = registry.trim_end_matches('/');
    let url = format!("{base}/{}", v.name);
    let filename = format!("{}-{}.tgz", v.name.rsplit('/').next().unwrap_or(&v.name), v.version);
    let data = base64::engine::general_purpose::STANDARD.encode(tgz);
    let tarball_url = format!("{base}/{}/-/{}", v.name, filename);
    let body = serde_json::json!({
        "_id": v.name,
        "name": v.name,
        "dist-tags": { tag: v.version },
        "versions": {
            v.version.clone(): {
                "name": v.name,
                "version": v.version,
                "dist": { "tarball": tarball_url, "integrity": ssri_of(tgz) },
            }
        },
        "_attachments": {
            filename: {
                "content_type": "application/octet-stream",
                "data": data,
                "length": tgz.len(),
            }
        },
    });
    tracing::info!(target: "winterjs::pm", package = v.name.as_str(), version = v.version.as_str(), bytes = tgz.len(), "publishing");
    let resp = super::registry::client()
        .put(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| Error::Other(format!("publish request failed: {e}")))?;
    let status = resp.status();
    if status.is_success() {
        println!("published {}@{} (tag {tag}) to {registry}", v.name, v.version);
        tracing::info!(target: "winterjs::pm", package = v.name.as_str(), version = v.version.as_str(), "published");
        return Ok(());
    }
    // 409 已存在（npm 口径：不可覆盖已发布版本）。
    if status == reqwest::StatusCode::CONFLICT {
        return Err(Error::Other(format!(
            "package {}@{} already exists in {registry} (cannot overwrite published versions)",
            v.name, v.version
        )));
    }
    // 401/403 指到 login。
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(Error::Other(format!(
            "publish rejected by {registry} ({status}; check `winterjs --login --token` for this registry)"
        )));
    }
    let text = resp.text().await.unwrap_or_default();
    let short = text.chars().take(300).collect::<String>();
    Err(Error::Other(format!("publish failed ({status}): {short}")))
}

/// tarball 的 ssri 完整性串（`sha512-<b64>`；与 install 校验同形）。
fn ssri_of(tgz: &[u8]) -> String {
    use base64::Engine as _;
    use sha2::Digest as _;
    format!("sha512-{}", base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(tgz)))
}

/// npmrc 文本内 upsert token 行（按 host 匹配 `//…:_authToken`；无则追加）。
/// 纯函数，单元测试覆盖。
pub fn upsert_auth_token(text: &str, registry_url: &str, token: &str) -> Result<String, String> {
    let host = super::npmrc::registry_host(registry_url)
        .ok_or_else(|| format!("bad registry url '{registry_url}'"))?;
    let mut out: Vec<String> = Vec::new();
    let mut replaced = false;
    for raw in text.lines() {
        let line = raw.trim();
        if !replaced
            && let Some(key) = line.split_once('=').map(|(k, _)| k.trim())
            && key.ends_with(":_authToken")
            && key.contains(&host)
        {
            out.push(format!("{key}={token}"));
            replaced = true;
        } else {
            out.push(raw.to_owned());
        }
    }
    if !replaced {
        out.push(format!("//{host}/:_authToken={token}"));
    }
    let mut s = out.join("\n");
    s.push('\n');
    Ok(s)
}

/// `login --token`（token 写 `$HOME/.npmrc`，原子；值永不进日志）。
pub fn login_with_token(home: &Path, registry_url: &str, token: &str) -> Result<(), Error> {
    if token.trim().is_empty() {
        return Err(Error::Other("empty token (pass --token <token>)".into()));
    }
    let host = super::npmrc::registry_host(registry_url)
        .ok_or_else(|| Error::Other(format!("bad registry url '{registry_url}'")))?;
    let path = home.join(".npmrc");
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let new = upsert_auth_token(&old, registry_url, token.trim()).map_err(Error::Other)?;
    super::cache::atomic_write(&path, new.as_bytes()).map_err(Error::Other)?;
    tracing::info!(target: "winterjs::pm", host = host.as_str(), "login token saved");
    println!("logged in to {registry_url} (token saved to {})", path.display());
    Ok(())
}

/// OAuth 授权 URL 拼装（纯 `oauth2` 类型操作，无网络；返回 `(url, csrf)`）。
pub fn oauth_authorize_url(registry_url: &str) -> Result<(String, String), String> {
    use oauth2::{AuthUrl, ClientId, CsrfToken, RedirectUrl, TokenUrl};
    let base = registry_url.trim_end_matches('/');
    let client = oauth2::basic::BasicClient::new(ClientId::new("winterjs".to_string()))
        .set_auth_uri(AuthUrl::new(format!("{base}/oauth/authorize")).map_err(|e| format!("bad registry url: {e}"))?)
        .set_token_uri(TokenUrl::new(format!("{base}/oauth/token")).map_err(|e| format!("bad registry url: {e}"))?)
        .set_redirect_uri(RedirectUrl::new("http://localhost/callback".to_string()).map_err(|e| e.to_string())?);
    let (url, csrf) = client.authorize_url(CsrfToken::new_random).url();
    Ok((url.to_string(), csrf.secret().clone()))
}

/// OAuth code 换 token（Authorization Code 交换；`--token code:<code>` 入口）。
/// 成功返回 access token 串（值由调用方落盘，永不进日志）。
pub async fn exchange_code(registry_url: &str, code: &str) -> Result<String, String> {
    use oauth2::{AuthUrl, ClientId, RedirectUrl, TokenResponse as _, TokenUrl};
    let base = registry_url.trim_end_matches('/');
    let client = oauth2::basic::BasicClient::new(ClientId::new("winterjs".to_string()))
        .set_auth_uri(AuthUrl::new(format!("{base}/oauth/authorize")).map_err(|e| format!("bad registry url: {e}"))?)
        .set_token_uri(TokenUrl::new(format!("{base}/oauth/token")).map_err(|e| format!("bad registry url: {e}"))?)
        .set_redirect_uri(RedirectUrl::new("http://localhost/callback".to_string()).map_err(|e| e.to_string())?);
    let http = oauth2::reqwest::ClientBuilder::new()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("oauth http client failed: {e}"))?;
    let tok = client
        .exchange_code(oauth2::AuthorizationCode::new(code.to_string()))
        .request_async(&http)
        .await
        .map_err(|e| format!("code exchange failed: {e}"))?;
    Ok(tok.access_token().secret().clone())
}

/// `login` 分发（`--token` 直写 / `--token code:<code>` 交换落盘 / `--oauth` 干跑 URL / TTY 密码提示）。
pub async fn login(registry: &str, token: Option<&str>, oauth: bool) -> Result<(), Error> {
    if oauth {
        let (url, _) = oauth_authorize_url(registry).map_err(Error::Other)?;
        // headless 下打不开忽略（只打印 URL，不失败）。
        let _ = webbrowser::open(&url);
        println!("open this URL to authorize:\n{url}");
        println!("then re-run with --token <token> (or --token code:<code> to exchange automatically)");
        return Ok(());
    }
    if let Some(t) = token {
        // `--token code:<code>` 形：code 换 token 再落盘。
        if let Some(code) = t.strip_prefix("code:") {
            let code = code.trim();
            if code.is_empty() {
                return Err(Error::Other("empty oauth code (pass --token code:<code>)".into()));
            }
            let tok = exchange_code(registry, code).await.map_err(Error::Other)?;
            let home = dirs::home_dir().ok_or_else(|| Error::Other("cannot find home directory".into()))?;
            return login_with_token(&home, registry, &tok);
        }
        let home = dirs::home_dir().ok_or_else(|| Error::Other("cannot find home directory".into()))?;
        return login_with_token(&home, registry, t);
    }
    if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        let t = dialoguer::Password::new()
            .with_prompt(format!("npm token for {registry}"))
            .interact()
            .map_err(|e| Error::Other(format!("login prompt failed: {e}")))?;
        let home = dirs::home_dir().ok_or_else(|| Error::Other("cannot find home directory".into()))?;
        return login_with_token(&home, registry, &t);
    }
    Err(Error::Other("need --token <token> (non-interactive stdin)".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(dir: &Path, manifest: &str) {
        std::fs::write(dir.join("package.json"), manifest).unwrap();
        std::fs::write(dir.join("index.js"), b"exports.v = 1;\n").unwrap();
    }

    #[test]
    fn validate_ok_and_missing() {
        let dir = tempfile::tempdir().unwrap();
        pkg(&dir.path().to_path_buf(), r#"{"name":"p","version":"1.2.3","license":"MIT"}"#);
        let v = validate_manifest(dir.path()).unwrap();
        assert_eq!((v.name.as_str(), v.version.as_str()), ("p", "1.2.3"));
        assert_eq!(v.license.as_deref(), Some("MIT"));
        // 缺名/坏版本/坏 license 三件。
        pkg(&dir.path().to_path_buf(), r#"{"version":"1.0.0"}"#);
        assert!(validate_manifest(dir.path()).is_err());
        pkg(&dir.path().to_path_buf(), r#"{"name":"p","version":"notaversion"}"#);
        assert!(validate_manifest(dir.path()).is_err());
        pkg(&dir.path().to_path_buf(), r#"{"name":"p","version":"1.0.0","license":"Not-A-License!!"}"#);
        assert!(validate_manifest(dir.path()).is_err());
    }

    #[test]
    fn files_array_checked() {
        let dir = tempfile::tempdir().unwrap();
        pkg(&dir.path().to_path_buf(), r#"{"name":"p","version":"1.0.0","files":["index.js","missing.js"]}"#);
        assert!(validate_manifest(dir.path()).unwrap_err().contains("missing.js"));
        pkg(&dir.path().to_path_buf(), r#"{"name":"p","version":"1.0.0","files":["index.js"]}"#);
        assert_eq!(validate_manifest(dir.path()).unwrap().files, 1);
    }

    #[test]
    fn upsert_insert_and_replace() {
        let s = upsert_auth_token("", "https://r.example/npm/", "tok1").unwrap();
        assert!(s.contains("//r.example/:_authToken=tok1"), "insert: {s}");
        let s2 = upsert_auth_token(&s, "https://r.example/npm/", "tok2").unwrap();
        assert!(s2.contains("tok2") && !s2.contains("tok1"), "replace: {s2}");
        assert_eq!(s2.lines().filter(|l| l.contains("_authToken")).count(), 1);
        // 他 host 的行不动。
        let s3 = upsert_auth_token("//other/:_authToken=keep\n", "https://r.example/", "new").unwrap();
        assert!(s3.contains("keep") && s3.contains("//r.example/:_authToken=new"), "merge: {s3}");
    }

    #[test]
    fn oauth_url_points_at_registry() {
        let (url, csrf) = oauth_authorize_url("https://r.example/npm/").unwrap();
        assert!(url.starts_with("https://r.example/npm/oauth/authorize?"), "url: {url}");
        assert!(url.contains("client_id=winterjs"), "url: {url}");
        assert!(!csrf.is_empty());
    }

    #[test]
    fn pack_tarball_roundtrip() {
        use std::io::Read as _;
        let dir = tempfile::tempdir().unwrap();
        pkg(&dir.path().to_path_buf(), r#"{"name":"p","version":"1.0.0","license":"MIT"}"#);
        let v = validate_manifest(dir.path()).unwrap();
        let (tgz, names) = pack_tarball(dir.path(), &v).unwrap();
        assert!(names.contains(&"package.json".to_string()), "names: {names:?}");
        assert!(names.contains(&"index.js".to_string()), "names: {names:?}");
        // 解包验证：gzip→tar→package/ 前缀 + 内容一致。
        let tar = flate2::read::GzDecoder::new(tgz.as_slice());
        let mut ar = tar::Archive::new(tar);
        let mut found = Vec::new();
        for entry in ar.entries().unwrap() {
            let mut e = entry.unwrap();
            let path = e.path().unwrap().to_string_lossy().into_owned();
            assert!(path.starts_with("package/"), "prefix: {path}");
            let mut s = String::new();
            e.read_to_string(&mut s).unwrap();
            found.push((path, s));
        }
        assert!(found.iter().any(|(p, s)| p == "package/package.json" && s.contains("\"p\"")), "found: {found:?}");
        // files 子集：只包所列 + package.json 必带。
        pkg(&dir.path().to_path_buf(), r#"{"name":"p","version":"1.0.0","files":["index.js"]}"#);
        let v = validate_manifest(dir.path()).unwrap();
        let (_, names) = pack_tarball(dir.path(), &v).unwrap();
        assert_eq!(names, vec!["index.js".to_string(), "package.json".to_string()], "names: {names:?}");
    }
}
