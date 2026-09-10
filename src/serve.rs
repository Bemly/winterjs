//! 静态文件服务（plan Phase 6-d1）：`winterjs serve [dir] [--host] [--port]`。
//!
//! - `tower-http` `ServeDir` 直服目录：mime（`mime_guess` 内建）、etag、
//!   range（206）全由轮子提供；目录自动拼 `index.html`（行为由黑盒钉住）。
//! - 就绪后 stdout 打印 `serving <dir> on http://<addr>`（`--port 0` 时回显
//!   实际端口，黑盒据此连接）；LAN 行 best-effort（拿不到就跳过）。
//! - SIGINT/SIGTERM 到即优雅停机（d3 再补 in-flight 排空语义，此处即停）。
//! - 复用 main 的 current-thread runtime（serve 路径无 JS，无 §6 线程模型冲突）。

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use axum::Router;
use tower_http::services::ServeDir;

use crate::error::Error;

/// 服务选项（CLI 直传）。
pub struct ServeOpts {
    pub dir: PathBuf,
    pub host: String,
    pub port: u16,
}

/// 目录校验（存在 + 是目录；返回规范绝对路径，日志/banner 用）。
pub fn validate_dir(dir: &Path) -> Result<PathBuf, Error> {
    let meta = std::fs::metadata(dir).map_err(|e| {
        Error::Other(format!("cannot serve '{}': {e}", dir.display()))
    })?;
    if !meta.is_dir() {
        return Err(Error::Other(format!(
            "cannot serve '{}': not a directory",
            dir.display()
        )));
    }
    std::fs::canonicalize(dir)
        .map_err(|e| Error::Other(format!("cannot serve '{}': {e}", dir.display())))
}

/// 就绪行（纯函数，单测覆盖；黑盒用此前缀解析实际端口）。
pub fn ready_line(root: &Path, addr: &SocketAddr) -> String {
    format!("serving {} on http://{addr}", root.display())
}

/// LAN 地址（best-effort；拿不到返回 `None`，不中断服务）。
pub fn lan_addr(port: u16) -> Option<SocketAddr> {
    let ip = local_ip_address::local_ip().ok()?;
    format!("{ip}:{port}").parse().ok()
}

/// 启动并跑到信号到来。调用方（main）已在 tokio runtime 内。
pub async fn serve(opts: &ServeOpts) -> Result<(), Error> {
    let root = validate_dir(&opts.dir)?;
    let listener = tokio::net::TcpListener::bind((opts.host.as_str(), opts.port))
        .await
        .map_err(|e| {
            Error::Other(format!("cannot bind {}:{}: {e}", opts.host, opts.port))
        })?;
    let addr = listener
        .local_addr()
        .map_err(|e| Error::Other(format!("cannot read bound address: {e}")))?;
    println!("{}", ready_line(&root, &addr));
    if let Some(lan) = lan_addr(addr.port()) {
        println!("lan: http://{lan}");
    }
    tracing::info!(target: "winterjs::serve", %addr, dir = %root.display(), "serving");
    let app = Router::new().fallback_service(ServeDir::new(root));
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| Error::Other(format!("serve failed: {e}")))?;
    tracing::info!(target: "winterjs::serve", "stopped");
    Ok(())
}

/// SIGINT（Ctrl-C）或 SIGTERM（unix）到即返回；注册失败则只等 Ctrl-C。
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = ctrl_c => {},
                    _ = term.recv() => {},
                }
            }
            Err(e) => {
                tracing::warn!(target: "winterjs::serve", "SIGTERM handler unavailable: {e}");
                ctrl_c.await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        ctrl_c.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_line_shape() {
        let addr: SocketAddr = "127.0.0.1:3000".parse().unwrap();
        let line = ready_line(Path::new("/tmp/site"), &addr);
        assert_eq!(line, "serving /tmp/site on http://127.0.0.1:3000");
    }

    #[test]
    fn validate_dir_table() {
        let dir = tempfile::tempdir().unwrap();
        assert!(validate_dir(dir.path()).is_ok());
        // 不存在 → 错；文件 → 错（边界两件）。
        assert!(validate_dir(&dir.path().join("nope")).is_err());
        let f = dir.path().join("f.txt");
        std::fs::write(&f, b"x").unwrap();
        let err = validate_dir(&f).unwrap_err().to_string();
        assert!(err.contains("not a directory"), "err: {err}");
    }

    #[test]
    fn lan_addr_shape_or_none() {
        // 本机一定有回环之外的判断不稳定：只断言形状（有则必带端口）。
        if let Some(addr) = lan_addr(1234) {
            assert_eq!(addr.port(), 1234);
        }
    }
}
