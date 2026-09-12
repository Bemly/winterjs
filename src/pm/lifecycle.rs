//! lifecycle 脚本（plan Phase 5c/5d）：`preinstall → install → postinstall` + `prepare`。
//! 9i-10 勘误（npm 口径实测回归）：**prepare 只对 git/本地依赖跑**——registry
//! tarball 依赖不跑（npm 只在 git/local 包上跑 prepare；lightningcss 的 prepare
//! 引 patch-package，对 tarball 跑它会把整个 vite 安装炸掉）。
//!
//! - 时机：单包 `node_modules/<pkg>` 落地 + bin 链接**之后**，cwd 即包目录。
//!   `prepare` 跑在最后（npm 口径：本地/git 依赖装完构建；发包时另由 publish 干跑校验）。
//! - 执行：unix `/bin/sh -c <script>`，win `cmd.exe /C <script>`；
//!   stdio 继承（用户可见，与 npm 行为一致）；`kill_on_drop(true)` 防孤儿。
//! - 组杀：unix 起 setsid 组长（`nix` 轮子复用 child 侧模式），失败回退普通子进程；
//!   超时不设（脚本跑多久等多久，Ctrl-C 由调用方信号路径处理；组长保证树可收）。
//! - 环境：继承 + `npm_package_name/version` + `npm_lifecycle_event`，
//!   `PATH` 前置 `<root>/node_modules/.bin`（跨包 bin 互调，npm 同语义）。
//! - 失败：非零退出即整个 `install` 失败（可读错误，含 event + 退出码）。
//! - 日志纪律（AGENTS §6）：只记 event/脚本长度等元信息，禁打脚本原文。

use std::path::Path;

use crate::error::Error;

/// 按序执行的 lifecycle 事件（npm 子集；`prepublishOnly` 等发包事件不跑——
/// publish 只有 dry-run 校验，无远端发布流程）。
pub const STAGES: &[&str] = &["preinstall", "install", "postinstall", "prepare"];
/// registry tarball 依赖的 lifecycle 段（npm 口径：无 prepare）。
pub const TAR_STAGES: &[&str] = &["preinstall", "install", "postinstall"];

/// 读包的 scripts 表（缺失/非法一律当空，不中断）。
fn scripts_of(pkg_dir: &Path) -> serde_json::Map<String, serde_json::Value> {
    let text = std::fs::read_to_string(pkg_dir.join("package.json")).unwrap_or_default();
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.get("scripts").cloned())
        .and_then(|v| match v {
            serde_json::Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

/// 跑单个脚本（shell 包装；返回退出码语义错误）。
async fn run_one(pkg_dir: &Path, event: &str, script: &str, env_extra: &[(String, String)]) -> Result<(), Error> {
    tracing::info!(target: "winterjs::pm", event, script_len = script.len(), "lifecycle start");
    let mut cmd = if cfg!(windows) {
        let mut c = tokio::process::Command::new("cmd.exe");
        c.arg("/C").arg(script);
        c
    } else {
        let mut c = tokio::process::Command::new("/bin/sh");
        c.arg("-c").arg(script);
        c
    };
    cmd.current_dir(pkg_dir);
    for (k, v) in env_extra {
        cmd.env(k, v);
    }
    #[cfg(unix)]
    {
        // SAFETY: pre_exec 跑在 fork 后 exec 前，只调 setsid（async-signal-safe），
        // 不触 Rust 运行时/锁/堆；失败忽略（退化普通子进程）。
        use std::os::unix::process::CommandExt as _;
        unsafe {
            cmd.as_std_mut().pre_exec(|| {
                let _ = nix::unistd::setsid();
                Ok(())
            });
        }
    }
    cmd.kill_on_drop(true);
    let status = cmd.status().await.map_err(|e| Error::Other(format!("lifecycle '{event}' spawn failed: {e}")))?;
    if status.success() {
        tracing::info!(target: "winterjs::pm", event, "lifecycle done");
        Ok(())
    } else {
        tracing::warn!(target: "winterjs::pm", event, ?status, "lifecycle failed");
        Err(Error::Other(format!("lifecycle '{event}' failed with {status}")))
    }
}

/// 跑包的全部 lifecycle 脚本（按 `STAGES` 序；空即直接 Ok）。
/// `nm_bin` 为 `<root>/node_modules/.bin`（PATH 前置用；不存在也无妨）。
pub async fn run_package_scripts(
    pkg_dir: &Path,
    name: &str,
    version: &str,
    nm_bin: &Path,
) -> Result<(), Error> {
    run_stage_list(pkg_dir, name, version, nm_bin, STAGES).await
}

/// 指定段执行（tarball 走 `TAR_STAGES` 无 prepare；git/local 走 `STAGES` 全四段）。
pub async fn run_stage_list(
    pkg_dir: &Path,
    name: &str,
    version: &str,
    nm_bin: &Path,
    stages: &[&str],
) -> Result<(), Error> {
    let scripts = scripts_of(pkg_dir);
    for event in stages {
        let Some(script) = scripts.get(*event).and_then(|v| v.as_str()) else {
            continue;
        };
        let script = script.trim();
        if script.is_empty() {
            continue;
        }
        // PATH 前置（跨包 bin；原 PATH 保留）。
        let path = match std::env::var_os("PATH") {
            Some(p) => {
                let mut parts = vec![nm_bin.to_path_buf().into_os_string()];
                parts.push(p);
                std::env::join_paths(parts).unwrap_or_default()
            }
            None => nm_bin.as_os_str().to_owned(),
        };
        let env_extra = vec![
            ("npm_package_name".to_string(), name.to_string()),
            ("npm_package_version".to_string(), version.to_string()),
            ("npm_lifecycle_event".to_string(), event.to_string()),
            ("PATH".to_string(), path.to_string_lossy().into_owned()),
        ];
        run_one(pkg_dir, event, script, &env_extra).await.map_err(|e| {
            Error::Other(format!("package {name}@{version}: {e}"))
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_order_pinned() {
        assert_eq!(STAGES, &["preinstall", "install", "postinstall", "prepare"]);
    }

    #[test]
    fn missing_scripts_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name":"x"}"#).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(run_stage_list(dir.path(), "x", "1.0.0", dir.path(), STAGES)).unwrap();
    }

    #[test]
    fn scripts_run_in_order_and_env() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name":"x","version":"1.0.0","scripts":{
                "preinstall": "printf '%s' pre >> order.txt",
                "install": "printf '%s' \"$npm_lifecycle_event\" >> order.txt",
                "postinstall": "printf '%s' post >> order.txt",
                "prepare": "printf '%s' prep >> order.txt"
            }}"#,
        )
        .unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(run_stage_list(dir.path(), "x", "1.0.0", dir.path(), STAGES)).unwrap();
        let order = std::fs::read_to_string(dir.path().join("order.txt")).unwrap();
        assert_eq!(order, "preinstallpostprep");
    }

    #[test]
    fn failing_script_errors() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name":"x","scripts":{"postinstall":"exit 3"}}"#,
        )
        .unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let err = rt
            .block_on(run_package_scripts(dir.path(), "x", "1.0.0", dir.path()))
            .unwrap_err();
        assert!(err.to_string().contains("postinstall"), "err: {err}");
    }
}
