//! 脚手架（plan Phase 7-e2）：`winterjs init [name] [--yes]`。
//!
//! - 模板经 `askama` 内联渲染（单体二进制，不另建模板目录）。
//! - 生成三件：`package.json` + `index.js` + `hello.test.js`
//!   （init 后 `winterjs test` 即绿，闭环验收）。
//! - 已存在文件不覆盖，冲突即整体报错（删了重来；`--force` 顺延，文档记录）。
//! - 名缺省取当前目录名；`--yes` 跳过确认，非 TTY 下缺 `--yes` 即报可读错。

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

/// 落盘（冲突即整体报错，一个不写；成功打印清单）。
pub async fn init(dir: &Path, name: Option<&str>, yes: bool) -> Result<(), Error> {
    let default_name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("app")
        .to_owned();
    let name = name.unwrap_or(&default_name).trim().to_owned();
    check_name(&name).map_err(Error::Other)?;
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
    let clashes: Vec<String> = files
        .iter()
        .filter(|(p, _)| dir.join(p).exists())
        .map(|(p, _)| p.display().to_string())
        .collect();
    if !clashes.is_empty() {
        return Err(Error::Other(format!(
            "refusing to overwrite: {}",
            clashes.join(", ")
        )));
    }
    for (p, content) in &files {
        std::fs::write(dir.join(p), content)
            .map_err(|e| Error::Other(format!("cannot write '{}': {e}", p.display())))?;
        println!("created {}", p.display());
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
}
