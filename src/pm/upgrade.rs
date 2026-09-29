//! 自升级（plan Phase 5d-d4）：`upgrade [--dry-run]` 经 `self_update` GitHub 后端。
//!
//! - 渠道：`WINTERJS2_UPDATE_GITHUB=<owner>/<repo>`（未设即无渠道）。
//! - dry-run：打印当前版 + 渠道状态，不碰网络。
//! - 真升：有渠道走 `Update::update()`（同步阻塞；upgrade 命令独占进程、无其他
//!   并发任务，故直接调用不绕 `spawn_blocking`）；无渠道即报顺延错。
//! - 日志：只记版本/渠道/结果，不记 token（`GH_TOKEN` 由轮子自行读取）。

use crate::error::Error;

/// 更新渠道（`owner/repo`；格式不对当未设，调用方按无渠道报错）。
pub fn channel() -> Option<(String, String)> {
    let raw = std::env::var("WINTERJS2_UPDATE_GITHUB").ok()?;
    parse_channel(&raw)
}

/// 渠道串解析（纯函数，单元测试覆盖）。
pub fn parse_channel(raw: &str) -> Option<(String, String)> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let (owner, repo) = raw.split_once('/')?;
    let (owner, repo) = (owner.trim(), repo.trim());
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return None;
    }
    Some((owner.to_owned(), repo.to_owned()))
}

/// 自升级（见头注语义）。
pub async fn upgrade(dry_run: bool) -> Result<(), Error> {
    const CURRENT: &str = env!("CARGO_PKG_VERSION");
    match (dry_run, channel()) {
        (true, ch) => {
            println!("winterjs2 {CURRENT}");
            match ch {
                Some((owner, repo)) => println!("channel: github:{owner}/{repo}"),
                None => println!("channel: (none; set WINTERJS2_UPDATE_GITHUB=owner/repo to enable)"),
            }
            tracing::info!(target: "winterjs2::pm", version = CURRENT, "upgrade dry-run ok");
            Ok(())
        }
        (false, None) => Err(Error::Other(
            "no update channel (set WINTERJS2_UPDATE_GITHUB=owner/repo to enable; --dry-run only reports)".into(),
        )),
        (false, Some((owner, repo))) => {
            tracing::info!(target: "winterjs2::pm", owner = owner.as_str(), repo = repo.as_str(), "checking for updates");
            let mut cfg = self_update::backends::github::Update::configure();
            cfg.repo_owner(&owner).repo_name(&repo).bin_name("winterjs2").current_version(CURRENT);
            let status = cfg
                .build()
                .map_err(|e| Error::Other(format!("bad update channel github:{owner}/{repo}: {e}")))?
                .update()
                .map_err(|e| Error::Other(format!("upgrade check failed: {e}")))?;
            if status.is_updated() {
                println!("upgraded to {}", status.version());
            } else {
                println!("already up to date (winterjs2 {CURRENT})");
            }
            tracing::info!(target: "winterjs2::pm", updated = status.is_updated(), "upgrade done");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_table() {
        assert_eq!(parse_channel("owner/repo"), Some(("owner".into(), "repo".into())));
        assert_eq!(parse_channel("  owner/repo  "), Some(("owner".into(), "repo".into())));
        assert!(parse_channel("").is_none());
        assert!(parse_channel("owner").is_none());
        assert!(parse_channel("owner/").is_none());
        assert!(parse_channel("/repo").is_none());
        assert!(parse_channel("a/b/c").is_none());
    }
}
