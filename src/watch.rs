//! 文件变更监听共享件（`--watch`）：`--test`（见 `testrun.rs` 自有循环）、
//! `--run` 文件重跑、`--serve` 子进程重启共用。
//!
//! - 防抖：`notify-debouncer-mini` 300ms（与 test watch 同值；持续写下不饿死，
//!   见 §4.152 前沿触发讨论——此处只取"有无变更"，天然前沿语义）。
//! - 过滤：`watchable` 与 test watch 同表（跳过 node_modules/.git/target 与点文件，
//!   只认代码/配置后缀；测试自写产物不触发无限循环）。
//! - 偏差（与 test watch 同款文档记录）：变更后全量重做，不按导入图裁剪。

use std::path::{Path, PathBuf};

use crate::error::Error;

/// 变更是否值得重做（纯函数，单测覆盖；与 `testrun::watchable` 同表，改一处记两处）。
pub fn watchable(path: &Path) -> bool {
    !ignored(path)
        && matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx" | "mts" | "cts" | "json")
        )
}

/// `--serve --watch` 用：静态资源（html/css/图）改了也要重启，只去噪音不卡后缀。
pub fn watch_any(path: &Path) -> bool {
    !ignored(path)
}

/// 噪音（node_modules/.git/target + 点文件；三处 watch 共用）。
fn ignored(path: &Path) -> bool {
    for comp in path.components() {
        if let std::path::Component::Normal(c) = comp {
            if matches!(c.to_str(), Some("node_modules") | Some(".git") | Some("target")) {
                return true;
            }
        }
    }
    if path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'))
    {
        return true;
    }
    false
}

/// 防抖监听器：`changed().await` 返回本轮 watchable 变更数（0 不可能——过滤后才发）。
pub struct Watcher {
    rx: tokio::sync::mpsc::UnboundedReceiver<usize>,
    // debouncer 必须活到 Watcher 尾（drop 即停监视）；以下划线持有。
    _debouncer: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
}

/// 起监视（`roots` 不存在即错；递归监视；`filter` 判 worthiness）。
pub fn watch(roots: &[PathBuf]) -> Result<Watcher, Error> {
    watch_with(roots, watchable)
}

/// `watch` 的可注入过滤形态（serve 用 `watch_any`，单测直验分发）。
pub fn watch_with(roots: &[PathBuf], filter: fn(&Path) -> bool) -> Result<Watcher, Error> {
    use notify::RecursiveMode;
    use notify_debouncer_mini::{new_debouncer, DebounceEventResult};

    for r in roots {
        if !r.exists() {
            return Err(Error::Other(format!("cannot watch '{}': no such path", r.display())));
        }
    }
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<usize>();
    let mut debouncer =
        new_debouncer(std::time::Duration::from_millis(300), move |res: DebounceEventResult| {
            if let Ok(events) = res {
                let n = events.iter().filter(|e| filter(&e.path)).count();
                if n > 0 {
                    let _ = tx.send(n);
                }
            }
        })
        .map_err(|e| Error::Other(format!("cannot start file watcher: {e}")))?;
    for r in roots {
        debouncer
            .watcher()
            .watch(r, RecursiveMode::Recursive)
            .map_err(|e| Error::Other(format!("cannot watch '{}': {e}", r.display())))?;
    }
    let watched = roots.iter().map(|r| r.display().to_string()).collect::<Vec<_>>().join(", ");
    eprintln!("watching: {watched} (Ctrl-C to stop)");
    Ok(Watcher { rx, _debouncer: debouncer })
}

impl Watcher {
    /// 下一批变更（Ctrl-C/SIGTERM 即 `None`，调用方退出）。
    pub async fn changed(&mut self) -> Option<usize> {
        tokio::select! {
            _ = crate::serve::shutdown_signal() => None,
            n = self.rx.recv() => n,
        }
    }

    /// 重跑期间积压的事件清掉（并入下一轮，避免启动期/慢跑双触发）。
    pub fn drain(&mut self) {
        while self.rx.try_recv().is_ok() {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watchable_table_matches_test_runner() {
        for good in ["/w/a.js", "/w/src/lib.ts", "/w/handler.mjs", "/w/tsconfig.json"] {
            assert!(watchable(Path::new(good)), "{good}");
        }
        for bad in [
            "/w/node_modules/pkg/index.js",
            "/w/.git/index",
            "/w/target/debug/out.js",
            "/w/kv.db",
            "/w/.DS_Store",
            "/w/data.txt",
            "/w/index.html",
            "/w/style.css",
        ] {
            assert!(!watchable(Path::new(bad)), "{bad}");
        }
    }

    #[test]
    fn watch_any_sees_static_assets_but_not_noise() {
        for good in ["/w/index.html", "/w/style.css", "/w/a.js", "/w/logo.png"] {
            assert!(watch_any(Path::new(good)), "{good}");
        }
        for bad in [
            "/w/node_modules/pkg/index.js",
            "/w/.git/index",
            "/w/.DS_Store",
            "/w/target/out.css",
        ] {
            assert!(!watch_any(Path::new(bad)), "{bad}");
        }
    }
}
