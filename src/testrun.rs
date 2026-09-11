//! 测试运行器（plan Phase 7-e1）：`winterjs test [paths...] [--filter <glob>]`。
//!
//! - 发现：无 paths 时 walk cwd（`ignore` 轮子：尊重 gitignore，跳过
//!   `node_modules/target/.git`）；文件名 `*.test.{js,mjs,cjs,ts,mts,cts}`
//!   或 `test-*.{…}`；有 paths 时文件直用、目录 walk 该目录；排序稳定。
//! - 过滤：`--filter` 为 `glob` 模式，匹配相对路径或文件名（任一命中即留）。
//!   test 名级过滤顺延（`node:test` 起步无名过滤，文档记录）。
//! - 执行：每文件独立 `runtime::run(Script)`（新引擎+新 state，天然隔离；
//!   ESM `import` 走模块重试，见 §4.17）；透传子测试 stdout，失败行进 stderr。
//! - 报告：TAP 对齐（`ok - <rel>` / `not ok - <rel>`）+ 汇总
//!   `# pass <p>, fail <f>`；空列表 exit 0 提示；有 fail 则 exit 1 可读错。

use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::runtime;

/// 测试选项（CLI 直传）。
pub struct TestOpts {
    pub paths: Vec<PathBuf>,
    pub filter: Option<String>,
    /// `--test-name-pattern`（名级过滤；经 env 传入 node:test harness，见下）。
    pub test_name_pattern: Option<String>,
    /// `--watch`（Phase 7-e5）：受监视文件变更即重跑，SIGINT/SIGTERM 退出。
    pub watch: bool,
}

/// 名过滤 env 键（`node:test` prelude 读取；子串或 `/re/flags`）。
pub const TEST_NAME_PATTERN_ENV: &str = "WINTERJS_TEST_NAME_PATTERN";

/// 可测后缀（`name.test.<ext>` / `test-name.<ext>` 的 `<ext>` 部）。
const EXTS: &[&str] = &["js", "mjs", "cjs", "ts", "mts", "cts"];

/// 文件名是否测试文件（纯函数，单测覆盖）。
pub fn is_test_file(name: &str) -> bool {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s, e),
        None => return false,
    };
    if !EXTS.contains(&ext) {
        return false;
    }
    stem.ends_with(".test") || stem.starts_with("test-") || stem.starts_with("test_")
}

/// 目录 walk（`ignore` 尊重 gitignore；跳过三目录；只收文件且 `is_test_file`）。
fn walk_dir(dir: &Path, out: &mut Vec<PathBuf>) {
    let walker = ignore::WalkBuilder::new(dir)
        .filter_entry(|e| {
            // 跳过三目录（文件名比较，与深度无关）。
            e.file_name().to_str().is_none_or(|n| {
                n != "node_modules" && n != "target" && n != ".git"
            })
        })
        .build();
    for entry in walker.flatten() {
        let p = entry.path().to_path_buf();
        if !entry.file_type().is_none_or(|t| t.is_file()) {
            continue;
        }
        if p.file_name().and_then(|n| n.to_str()).is_some_and(is_test_file) {
            out.push(p);
        }
    }
}

/// 收集测试文件（空 paths 即 walk cwd；显式路径缺失即错）。
pub fn collect(root: &Path, paths: &[PathBuf]) -> Result<Vec<PathBuf>, Error> {
    let mut out = Vec::new();
    if paths.is_empty() {
        walk_dir(root, &mut out);
    } else {
        for p in paths {
            let full = if p.is_absolute() { p.clone() } else { root.join(p) };
            let meta = std::fs::metadata(&full).map_err(|e| {
                Error::Other(format!("no such test path '{}': {e}", p.display()))
            })?;
            if meta.is_dir() {
                walk_dir(&full, &mut out);
            } else {
                out.push(full);
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// `--filter` 过滤（glob 匹配相对路径或文件名；模式非法即错）。纯逻辑可单测。
pub fn apply_filter(
    root: &Path,
    files: Vec<PathBuf>,
    filter: Option<&str>,
) -> Result<Vec<PathBuf>, Error> {
    let Some(pat) = filter else {
        return Ok(files);
    };
    let matcher =
        glob::Pattern::new(pat).map_err(|e| Error::Other(format!("bad --filter '{pat}': {e}")))?;
    Ok(files
        .into_iter()
        .filter(|p| {
            let rel = p.strip_prefix(root).unwrap_or(p).to_string_lossy();
            matcher.matches(&rel)
                || p.file_name().and_then(|n| n.to_str()).is_some_and(|n| matcher.matches(n))
        })
        .collect())
}

/// 跑完列表（TAP 行 + 汇总；有 fail 则 exit 1 可读错）。
pub async fn run_tests(root: &Path, opts: &TestOpts) -> Result<(), Error> {
    if opts.watch {
        return watch_loop(root, opts).await;
    }
    let (_, fail) = run_once(root, opts).await?;
    tracing::info!(target: "winterjs::test", fail, "done");
    if fail > 0 {
        return Err(Error::Other(format!("{fail} test file(s) failed")));
    }
    Ok(())
}

/// 单轮执行（TAP 打印；返回 (pass, fail)，不映射退出码 —— watch 的失败不退出）。
async fn run_once(root: &Path, opts: &TestOpts) -> Result<(u32, u32), Error> {
    let files = apply_filter(root, collect(root, &opts.paths)?, opts.filter.as_deref())?;
    tracing::info!(target: "winterjs::test", count = files.len(), "discovered");
    if files.is_empty() {
        println!("no test files found");
        return Ok((0, 0));
    }
    let (mut pass, mut fail) = (0u32, 0u32);
    for f in &files {
        let rel = f.strip_prefix(root).unwrap_or(f).display().to_string();
        let source = match std::fs::read_to_string(f) {
            Ok(s) => s,
            Err(e) => {
                fail += 1;
                println!("not ok - {rel}");
                eprintln!("Error: cannot read '{rel}': {e}");
                continue;
            }
        };
        // 名过滤经 env 传入 harness（各文件顺序跑，设/恢复配对，无并行竞态）。
        // SAFETY: 单轮循环内同步设置（run_isolated 内部是多线程，但 env 读写只在
        // 本线程串行点发生；并行跑文件尚未引入，引入时改传参）。
        let saved = std::env::var(TEST_NAME_PATTERN_ENV).ok();
        match &opts.test_name_pattern {
            Some(p) => unsafe { std::env::set_var(TEST_NAME_PATTERN_ENV, p) },
            None => unsafe { std::env::remove_var(TEST_NAME_PATTERN_ENV) },
        }
        let r = runtime::run_isolated(source, f.to_string_lossy().into_owned(), Vec::new());
        match saved {
            Some(v) => unsafe { std::env::set_var(TEST_NAME_PATTERN_ENV, v) },
            None => unsafe { std::env::remove_var(TEST_NAME_PATTERN_ENV) },
        }
        // 文件名传绝对串（模块 hook 按文件名定位 referrer，见 §4.11）。
        // 每文件独立线程跑（run_isolated，§4.24：同线程建第二个 Runtime 会炸，
        // Runtime 又必须泄漏 —— 线程生灭就是隔离边界）。
        match r {
            Ok(()) => {
                pass += 1;
                println!("ok - {rel}");
            }
            Err(Error::Exit(code)) => {
                fail += 1;
                println!("not ok - {rel} (exit {code})");
            }
            Err(e) => {
                fail += 1;
                println!("not ok - {rel}");
                eprintln!("Error: {e}");
            }
        }
    }
    println!("# pass {pass}, fail {fail}");
    tracing::info!(target: "winterjs::test", pass, fail, "run done");
    Ok((pass, fail))
}

// ── watch 模式（Phase 7-e5）───────────────────────────────────────────────

/// 变更是否值得重跑：跳过 node_modules/.git/target 与点文件，只认代码/配置后缀
/// （db/日志/产物不触发，防测试自写文件的无限循环）。纯函数，单测覆盖。
fn watchable(path: &Path) -> bool {
    for comp in path.components() {
        if let std::path::Component::Normal(c) = comp {
            if matches!(c.to_str(), Some("node_modules") | Some(".git") | Some("target")) {
                return false;
            }
        }
    }
    if path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'))
    {
        return false;
    }
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx" | "mts" | "cts" | "json")
    )
}

/// watch 循环：初始跑一轮 → 防抖事件（notify-debouncer-mini 300ms）→ 重跑
/// （重发现文件，新增/删除即生效）→ SIGINT/SIGTERM 退出。
/// 偏差：变更后全量重跑（Bun 按导入图只跑受影响文件；按需顺延，文档记录）。
async fn watch_loop(root: &Path, opts: &TestOpts) -> Result<(), Error> {
    use notify::RecursiveMode;
    use notify_debouncer_mini::{new_debouncer, DebounceEventResult};

    let roots: Vec<PathBuf> = if opts.paths.is_empty() {
        vec![root.to_path_buf()]
    } else {
        opts.paths
            .iter()
            .map(|p| if p.is_absolute() { p.clone() } else { root.join(p) })
            .collect()
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<usize>();
    let mut debouncer =
        new_debouncer(std::time::Duration::from_millis(300), move |res: DebounceEventResult| {
            if let Ok(events) = res {
                let n = events.iter().filter(|e| watchable(&e.path)).count();
                if n > 0 {
                    let _ = tx.send(n);
                }
            }
        })
        .map_err(|e| Error::Other(format!("cannot start file watcher: {e}")))?;
    for r in &roots {
        debouncer
            .watcher()
            .watch(r, RecursiveMode::Recursive)
            .map_err(|e| Error::Other(format!("cannot watch '{}': {e}", r.display())))?;
    }
    let watched = roots.iter().map(|r| r.display().to_string()).collect::<Vec<_>>().join(", ");
    eprintln!("watching: {watched} (Ctrl-C to stop)");

    // 初始一轮（失败不退出 watch）；随后清掉本轮自产的事件，避免启动期双跑。
    let _ = run_once(root, opts).await;
    while rx.try_recv().is_ok() {}

    loop {
        tokio::select! {
            _ = crate::serve::shutdown_signal() => {
                tracing::info!(target: "winterjs::test", "watch stopped");
                return Ok(());
            }
            Some(n) = rx.recv() => {
                eprintln!("watch: {n} change(s), re-running");
                let _ = run_once(root, opts).await;
                // 重跑期间积压的防抖事件并入下一轮（下轮 run_once 重新发现）。
                while rx.try_recv().is_ok() {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_test_file_table() {
        for good in [
            "a.test.js",
            "a.test.mjs",
            "a.test.cjs",
            "a.test.ts",
            "a.test.mts",
            "a.test.cts",
            "test-a.js",
            "test_a.ts",
        ] {
            assert!(is_test_file(good), "{good}");
        }
        for bad in ["a.js", "test.js", "latest.js", "a.test.py", "a.test.", "contest.js"] {
            assert!(!is_test_file(bad), "{bad}");
        }
    }

    #[test]
    fn collect_skips_build_dirs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
        std::fs::create_dir_all(dir.path().join("target/debug")).unwrap();
        std::fs::write(dir.path().join("a.test.js"), b"1").unwrap();
        std::fs::write(dir.path().join("node_modules/pkg/b.test.js"), b"1").unwrap();
        std::fs::write(dir.path().join("target/debug/c.test.js"), b"1").unwrap();
        std::fs::write(dir.path().join("helper.js"), b"1").unwrap();
        let files = collect(dir.path(), &[]).unwrap();
        assert_eq!(files, vec![dir.path().join("a.test.js")]);
    }

    #[test]
    fn filter_matches_rel_or_name() {
        let root = Path::new("/r");
        let files = vec![
            PathBuf::from("/r/a.test.js"),
            PathBuf::from("/r/sub/b.test.js"),
        ];
        let out = apply_filter(root, files.clone(), Some("sub/*")).unwrap();
        assert_eq!(out, vec![PathBuf::from("/r/sub/b.test.js")]);
        let out = apply_filter(root, files.clone(), Some("a.test.js")).unwrap();
        assert_eq!(out, vec![PathBuf::from("/r/a.test.js")]);
        assert!(apply_filter(root, files, Some("[bad")).is_err());
    }

    #[test]
    fn watchable_filters_noise() {
        for good in [
            "/w/a.test.js", "/w/src/lib.ts", "/w/sub/x.mts", "/w/tsconfig.json",
        ] {
            assert!(watchable(Path::new(good)), "{good}");
        }
        for bad in [
            "/w/node_modules/pkg/index.js",
            "/w/.git/index",
            "/w/target/debug/out.js",
            "/w/kv.db",
            "/w/.DS_Store",
            "/w/data.txt",
        ] {
            assert!(!watchable(Path::new(bad)), "{bad}");
        }
    }
}
