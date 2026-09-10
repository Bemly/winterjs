//! 发布与登录（plan Phase 5d-d3）：`publish --dry-run` 校验 + `login` token 落盘。
//!
//! - `publish --dry-run`：纯本地校验（名/版本/license/文件表），打印将发布内容，
//!   不碰网络；无 `--dry-run` 的远端 PUT 顺延（报错指路，见 `publish`）。
//! - `login --token`：把 `//<host>/:_authToken` upsert 进 `$HOME/.npmrc`
//!   （保留其他行，原子写）；无 token 且 TTY 时经 `dialoguer` 密码提示，
//!   非 TTY 即报缺 `--token`（hermetic 可测）。
//! - `login --oauth`：用 `oauth2` 轮子拼授权 URL（`{registry}/oauth/authorize`），
//!   经 `webbrowser` 尝试打开（headless 失败忽略，只打印 URL）；
//!   code 换 token 的网络交换顺延，提示以 `--token` 完成。
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

/// `publish`（dry-run 打印摘要；真发布顺延，报错指路）。
pub async fn publish(dir: &Path, dry_run: bool, registry: &str, tag: &str) -> Result<(), Error> {
    let v = validate_manifest(dir).map_err(Error::Other)?;
    if !dry_run {
        return Err(Error::Other(format!(
            "network publish of {}@{} to {registry} is deferred beyond 5d (validate with --dry-run)",
            v.name, v.version
        )));
    }
    println!("{}@{} (tag {tag})", v.name, v.version);
    println!("registry: {registry}");
    println!("license: {}", v.license.as_deref().unwrap_or("(none)"));
    println!("files: {}", v.files);
    tracing::info!(target: "winterjs::pm", package = v.name.as_str(), version = v.version.as_str(), files = v.files, "publish dry-run ok");
    Ok(())
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

/// `login` 分发（`--token` 直写 / `--oauth` 干跑 URL / TTY 密码提示）。
pub async fn login(registry: &str, token: Option<&str>, oauth: bool) -> Result<(), Error> {
    if oauth {
        let (url, _) = oauth_authorize_url(registry).map_err(Error::Other)?;
        // headless 下打不开忽略（只打印 URL，不失败）。
        let _ = webbrowser::open(&url);
        println!("open this URL to authorize:\n{url}");
        println!("then re-run with --token <token> (code exchange is deferred beyond 5d)");
        return Ok(());
    }
    if let Some(t) = token {
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
}
