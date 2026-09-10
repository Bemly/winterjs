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
}

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
    let files = apply_filter(root, collect(root, &opts.paths)?, opts.filter.as_deref())?;
    tracing::info!(target: "winterjs::test", count = files.len(), "discovered");
    if files.is_empty() {
        println!("no test files found");
        return Ok(());
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
        // 文件名传绝对串（模块 hook 按文件名定位 referrer，见 §4.11）。
        match runtime::run(&source, &f.to_string_lossy(), runtime::Mode::Script, &[]).await {
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
    tracing::info!(target: "winterjs::test", pass, fail, "done");
    if fail > 0 {
        return Err(Error::Other(format!("{fail} test file(s) failed")));
    }
    Ok(())
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
}
