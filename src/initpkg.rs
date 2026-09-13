//! 脚手架（plan Phase 7-e2/c-4x）：`winterjs --init/-I [name] [--yes] [--force]`。
//!
//! - 模板经 `askama` 内联渲染（单体二进制，不另建模板目录）。
//! - 生成三件：`package.json` + `index.js` + `hello.test.js`
//!   （init 后 `winterjs test` 即绿，闭环验收）。
//! - 无配置新建、有配置重建（2026-09-13 改，旧"冲突即整体报错"作废）：
//!   `package.json` 不存在 → 三件全建；已存在 → 采用（名沿用其 `name` 字段，
//!   文件本体非 `--force` 不碰），只补齐缺失的 `index.js`/`hello.test.js`，
//!   已存在文件逐个跳过并报告（`exists, skipped`），全齐则 `already initialized`。
//!   `--force` 逐个覆盖（报告 `overwrote`），保留给明确想要重开的人。
//! - 名优先级：显式 `--init [name]`/`--name` > 现有 `package.json` 的 `name` >
//!   当前目录名；只有显式名才走 `check_name` 校验（已落盘的怪名不拦路）。
//! - `--yes` 跳过确认，非 TTY 下缺 `--yes` 即报可读错。

use std::path::{Path, PathBuf};

use askama::Template;

use crate::error::Error;

/// 包名校验（npm 最小子集 + scope 形；纯函数，单测覆盖）。
pub fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("package name is empty".into());
    }
    // `@scope/name` 拆 scope 后同规则。
    let body = match name.strip_prefix('@') {
        Some(rest) => match rest.split_once('/') {
            Some((scope, pkg)) => {
                if scope.is_empty() || pkg.is_empty() || pkg.contains('/') {
                    return Err(format!("bad scoped package name '{name}'"));
                }
                pkg
            }
            None => return Err(format!("bad scoped package name '{name}'")),
        },
        None => {
            if name.contains('/') {
                return Err(format!("bad package name '{name}'"));
            }
            name
        }
    };
    if !body
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || ".-_~".contains(c))
        || body.starts_with(['.', '_', '-'])
    {
        return Err(format!(
            "bad package name '{name}' (lowercase letters, digits, .-_~ only)"
        ));
    }
    Ok(())
}

#[derive(Template)]
#[template(
    source = r#"{
  "name": "{{ name }}",
  "version": "0.1.0",
  "main": "index.js",
  "scripts": {
    "test": "winterjs test"
  },
  "license": "MIT"
}
"#,
    ext = "txt"
)]
struct PackageJson<'a> {
    name: &'a str,
}

#[derive(Template)]
#[template(
    source = r#"export function add(a, b) {
  return a + b;
}

console.log(add(19, 23));
"#,
    ext = "txt"
)]
struct IndexJs;

#[derive(Template)]
#[template(
    source = r#"import { test } from "node:test";
import { add } from "./index.js";

test("adds", () => {
  if (add(19, 23) !== 42) throw new Error("math broke");
});
"#,
    ext = "txt"
)]
struct HelloTest;

/// 待写文件表（纯函数，单测覆盖渲染内容）。
pub fn planned(name: &str) -> Vec<(PathBuf, String)> {
    vec![
        (
            PathBuf::from("package.json"),
            PackageJson { name }.render().expect("template renders"),
        ),
        (
            PathBuf::from("index.js"),
            IndexJs.render().expect("template renders"),
        ),
        (
            PathBuf::from("hello.test.js"),
            HelloTest.render().expect("template renders"),
        ),
    ]
}

/// 现有 `package.json` 的 `name` 字段（宽容解析：坏文件/无名一律 `None`）。
/// 纯函数（除 fs 外），单测覆盖。
pub fn adopted_name(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("package.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("name")?.as_str().map(str::to_owned)
}

/// 落盘（有配置重建、无配置新建：已存在文件非 `--force` 一律跳过；
/// `package.json` 存在即采用不碰；成功打印清单，全齐则报 already initialized）。
pub async fn init(dir: &Path, name: Option<&str>, yes: bool, force: bool) -> Result<(), Error> {
    let default_name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("app")
        .to_owned();
    // 显式名才校验；采用名/目录名不拦路（已落盘的怪名照用）。
    if let Some(explicit) = name {
        check_name(explicit.trim()).map_err(Error::Other)?;
    }
    let name = name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| adopted_name(dir))
        .unwrap_or(default_name);
    if !yes {
        if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
            return Err(Error::Other(
                "refusing to prompt on non-TTY stdin (pass --yes)".into(),
            ));
        }
        let ok = dialoguer::Confirm::new()
            .with_prompt(format!("create package '{name}' here?"))
            .default(true)
            .interact()
            .map_err(|e| Error::Other(format!("prompt failed: {e}")))?;
        if !ok {
            return Err(Error::Other("cancelled".into()));
        }
    }
    let files = planned(&name);
    let mut skipped = 0;
    for (p, content) in &files {
        let dest = dir.join(p);
        if dest.exists() && !force {
            println!("exists, skipped {}", p.display());
            skipped += 1;
            continue;
        }
        let was_there = dest.exists();
        std::fs::write(&dest, content)
            .map_err(|e| Error::Other(format!("cannot write '{}': {e}", p.display())))?;
        if was_there {
            println!("overwrote {}", p.display());
        } else {
            println!("created {}", p.display());
        }
    }
    if skipped == files.len() {
        println!("already initialized, nothing to do");
    }
    tracing::info!(target: "winterjs::init", package = name.as_str(), "initialized");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_table() {
        for good in ["my-pkg", "a", "x.y_z~1", "@scope/pkg"] {
            assert!(check_name(good).is_ok(), "{good}");
        }
        for bad in ["", "Bad Name!", "UPPER", "@scope", "@/x", "@scope/", "a/b", ".dot", "-dash", "sp ace"] {
            assert!(check_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn templates_render_name() {
        let files = planned("demo-pkg");
        assert_eq!(files.len(), 3);
        assert!(files[0].1.contains("\"demo-pkg\""), "package.json: {}", files[0].1);
        assert!(files[2].1.contains("./index.js"), "test: {}", files[2].1);
    }

    #[test]
    fn adopted_name_table() {
        let dir = tempfile::tempdir().unwrap();
        // 无清单 → None
        assert_eq!(adopted_name(dir.path()), None);
        // 正常采用
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name":"vue-project","scripts":{"dev":"vite"}}"#,
        )
        .unwrap();
        assert_eq!(adopted_name(dir.path()).as_deref(), Some("vue-project"));
        // 坏 JSON/无名 → None（不拦路）
        std::fs::write(dir.path().join("package.json"), r#"{"name": "#).unwrap();
        assert_eq!(adopted_name(dir.path()), None);
        std::fs::write(dir.path().join("package.json"), r#"{"scripts":{}}"#).unwrap();
        assert_eq!(adopted_name(dir.path()), None);
    }
}
