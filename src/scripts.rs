//! `--run` 的脚本解释（9i-10）：带脚本后缀 → 文件直跑（原路径零回归）；
//! 裸名 → 优先 package.json `scripts`（从 cwd 逐级向上，monorepo 命中根），
//! 未命中回落同名文件，再不行才报错（npm 风列出可用脚本）。
//!
//! 执行口径（对齐 npm/bun，零 node 目标优先）：
//! - 首词解析到本地 `node_modules/.bin/<name>` 且是 **JS bin**（后缀 .js/.mjs/.cjs
//!   或 shebang 含 node）→ 递归调自身 `--run <bin> -- <args>`——JS bin 不走系统
//!   shebang，机器上没有 node 也能跑（lint/fmt 穿透同思想的自身版）；
//!   `--` 收尾防子进程 clap 吃掉 `--version` 等本仓已知 flag；
//! - 首词解析到真实可执行文件（.bin 原生二进制或 PATH 上有）→ 直接 argv 派发
//!   （不经 shell；esbuild 这类平台二进制即此路）；
//! - 其余（shell 内建/复合命令/env 前缀/含元字符）走 shell（unix `/bin/sh -c`，
//!   win `cmd /C`，`lifecycle.rs` 同款），PATH 前置沿包目录向上的每一层
//!   `node_modules/.bin`（npm 同款）。
//! - 退出码透传（`Error::Exit` 静默，main 的 dispatch 管道收口）。
//! 权限：沙箱开启时过 `permissions::check_run`（run 类，opt-in 语义不变）。
//! 偏差记档：npm 的 pre/post 钩子不做（bun 同款）；`--workspace` 不做；
//! 透传参数原样拼接（npm 同款，含空格的参数不加引号——`--` 分隔符剥一个）。

use std::path::{Path, PathBuf};

use crate::error::Error;

/// `--run` 目标解析结果。
pub enum RunTarget {
    /// JS 文件（main 走 runtime::run）。
    File(PathBuf),
    /// package.json 脚本（包目录 + 脚本串）。
    Script(PathBuf, String),
}

/// 可识别的脚本后缀（最后一段含其一即按文件解释）。
pub fn has_script_extension(arg: &str) -> bool {
    let Some(name) = Path::new(arg).file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    match name.rsplit_once('.') {
        Some((_, ext)) => matches!(
            ext,
            "js" | "mjs" | "cjs" | "ts" | "mts" | "cts" | "tsx" | "jsx"
        ),
        None => false,
    }
}

/// 从 `base` 逐级向上找 package.json（含 base 本身；monorepo 命中根）。
pub fn find_package_json(base: &Path) -> Option<PathBuf> {
    let mut dir = Some(base);
    while let Some(d) = dir {
        let p = d.join("package.json");
        if p.is_file() {
            return Some(p);
        }
        dir = d.parent();
    }
    None
}

/// 取 `scripts[name]`；缺失即 Err（npm 风列出可用脚本）。
pub fn lookup_script(pkg: &Path, name: &str) -> Result<String, String> {
    let raw = std::fs::read_to_string(pkg)
        .map_err(|e| format!("failed to read {}: {e}", pkg.display()))?;
    let v: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("{} is not valid JSON: {e}", pkg.display()))?;
    let scripts = v.get("scripts").and_then(|s| s.as_object());
    if let Some(script) = scripts.and_then(|m| m.get(name)).and_then(|x| x.as_str()) {
        return Ok(script.to_string());
    }
    let mut names: Vec<&String> = scripts.map(|m| m.keys().collect()).unwrap_or_default();
    names.sort();
    let list = if names.is_empty() {
        "no scripts defined in package.json".to_string()
    } else {
        format!(
            "available scripts:\n{}",
            names.iter().map(|n| format!("  {n}")).collect::<Vec<_>>().join("\n")
        )
    };
    Err(format!("Missing script: {name}\n{list}"))
}

/// JS bin 判定：后缀 .js/.mjs/.cjs，或 shebang 首行含 "node"。
pub fn is_js_bin(path: &Path) -> bool {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if name.ends_with(".js") || name.ends_with(".mjs") || name.ends_with(".cjs") {
        return true;
    }
    if let Ok(head) = std::fs::read(path) {
        if head.starts_with(b"#!") {
            let upto = head.len().min(128);
            return String::from_utf8_lossy(&head[..upto]).contains("node");
        }
    }
    false
}

/// 本地 bin 解析：从 `base` 逐级向上找 `node_modules/.bin/<name>`（lintfmt 同款）。
pub fn find_local_bin(base: &Path, name: &str) -> Option<PathBuf> {
    let mut dir = Some(base);
    while let Some(d) = dir {
        let bin = d.join("node_modules").join(".bin").join(name);
        if bin.is_file() {
            return Some(bin);
        }
        dir = d.parent();
    }
    None
}

/// PATH 前置串：沿 `pkg_dir` 向上的每一层 `.bin` + 原 PATH。
pub fn path_with_bins(pkg_dir: &Path) -> String {
    let mut parts: Vec<PathBuf> = Vec::new();
    let mut dir = Some(pkg_dir);
    while let Some(d) = dir {
        parts.push(d.join("node_modules").join(".bin"));
        dir = d.parent();
    }
    if let Some(existing) = std::env::var_os("PATH") {
        parts.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(parts)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// shell 元字符（有即整串走 shell，保留引号/展开/管道语义）。
fn has_shell_meta(s: &str) -> bool {
    s.contains(|c: char| {
        matches!(c, '|' | ';' | '&' | '<' | '>' | '(' | ')' | '`' | '$' | '*' | '?' | '"' | '\'' | '\\' | '\n')
    })
}

/// 解析 `--run <target>`：带后缀 → 文件；裸名 → scripts 优先、同名文件回落。
pub fn resolve(target: &str) -> Result<RunTarget, Error> {
    if has_script_extension(target) {
        return Ok(RunTarget::File(PathBuf::from(target)));
    }
    let cwd = std::env::current_dir().map_err(|e| Error::Other(e.to_string()))?;
    if let Some(pkg) = find_package_json(&cwd) {
        match lookup_script(&pkg, target) {
            Ok(script) => {
                let dir = pkg.parent().unwrap_or(Path::new(".")).to_path_buf();
                return Ok(RunTarget::Script(dir, script));
            }
            Err(missing) => {
                let fallback = cwd.join(target);
                if fallback.is_file() {
                    return Ok(RunTarget::File(fallback));
                }
                return Err(Error::Other(missing));
            }
        }
    }
    Ok(RunTarget::File(PathBuf::from(target)))
}

/// 执行脚本（`RunTarget::Script` 的后半程；退出码即进程退出码）。
pub fn run(pkg_dir: &Path, script: &str, extra_args: &[String]) -> Result<i32, Error> {
    let cmd_label = script.split_whitespace().next().unwrap_or(script);
    if let Err(e) = crate::permissions::check_run(cmd_label) {
        return Err(Error::Other(e));
    }
    // npm 口径：`--` 分隔符剥一个，其余原样拼到脚本串后。
    let extra: &[String] = match extra_args.split_first() {
        Some((first, rest)) if first == "--" => rest,
        _ => extra_args,
    };
    let full = if extra.is_empty() {
        script.to_string()
    } else {
        format!("{script} {}", extra.join(" "))
    };
    let words = shlex::split(&full).unwrap_or_default();
    let resolved = words
        .first()
        .map(|name| find_local_bin(pkg_dir, name).or_else(|| which::which(name).ok()))
        .unwrap_or(None);
    if !has_shell_meta(&full) {
        if let Some((_, rest)) = words.split_first() {
            if let Some(bin) = &resolved {
                if is_js_bin(bin) {
                    // 零 node 快路径：JS bin 递归调自身（子进程无沙箱旗，语义同 npm 直跑）。
                    // `--` 收尾旗解析：rest 里的 `--version`/`--help` 这类**本仓已知 flag**
                    // 不加会被子进程 clap 吃掉（2026-09-13 探针实测：
                    // `scripts: {"v": "node-which --version"}` 打出 winterjs 版本横幅）。
                    let exe = std::env::current_exe().map_err(|e| {
                        Error::Other(format!("failed to locate current exe: {e}"))
                    })?;
                    let mut cmd = std::process::Command::new(exe);
                    cmd.arg("--run").arg(bin).arg("--");
                    for a in rest {
                        cmd.arg(a);
                    }
                    return wait(cmd, script);
                }
                // 原生二进制：直接 argv（PATH 兜底其它可执行文件）。
                let mut cmd = std::process::Command::new(bin);
                for a in rest {
                    cmd.arg(a);
                }
                cmd.env("PATH", path_with_bins(pkg_dir));
                return wait(cmd, script);
            }
        }
    }
    // shell 路径：复合命令 / 内建 / 未解析到可执行文件。
    let mut cmd = std::process::Command::new(if cfg!(windows) { "cmd" } else { "/bin/sh" });
    if cfg!(windows) {
        cmd.arg("/C").arg(&full);
    } else {
        cmd.arg("-c").arg(&full);
    }
    cmd.env("PATH", path_with_bins(pkg_dir));
    wait(cmd, &full)
}

fn wait(mut cmd: std::process::Command, label: &str) -> Result<i32, Error> {
    match cmd.status() {
        Ok(status) => Ok(status.code().unwrap_or(1)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(Error::Other(format!(
            "sh: {label}: command not found"
        ))),
        Err(e) => Err(Error::Other(format!("failed to spawn '{label}': {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_extension_table() {
        for s in ["dev.ts", "a/b.js", "x.mjs", "y.cjs", "z.tsx", "w.jsx", "v.mts", "u.cts"] {
            assert!(has_script_extension(s), "{s}");
        }
        for s in ["dev", "build", "my.file", ".bin", "a/b/dev", ""] {
            assert!(!has_script_extension(s), "{s}");
        }
    }

    #[test]
    fn package_json_walk_up() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        let sub = root.join("packages").join("foo");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(root.join("package.json"), "{}").unwrap();
        // 从子目录向上命中根
        assert_eq!(find_package_json(&sub), Some(root.join("package.json")));
        // 无 package.json → None
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(find_package_json(empty.path()), None);
    }

    #[test]
    fn script_lookup_and_missing_list() {
        let dir = tempfile::tempdir().unwrap();
        let pkg = dir.path().join("package.json");
        std::fs::write(
            &pkg,
            r#"{"name":"x","scripts":{"dev":"vite","build":"vite build"}}"#,
        )
        .unwrap();
        assert_eq!(lookup_script(&pkg, "dev").unwrap(), "vite");
        assert_eq!(lookup_script(&pkg, "build").unwrap(), "vite build");
        let missing = lookup_script(&pkg, "nope").unwrap_err();
        assert!(missing.contains("Missing script: nope"), "{missing}");
        assert!(missing.contains("  build"), "{missing}");
        assert!(missing.contains("  dev"), "{missing}");
        // 无 scripts 字段
        std::fs::write(&pkg, r#"{"name":"x"}"#).unwrap();
        let err = lookup_script(&pkg, "dev").unwrap_err();
        assert!(err.contains("no scripts defined"), "{err}");
    }

    #[test]
    fn js_bin_detection() {
        let dir = tempfile::tempdir().unwrap();
        let js = dir.path().join("tool.js");
        std::fs::write(&js, "#!/usr/bin/env node\nconsole.log(1)\n").unwrap();
        let sh = dir.path().join("native");
        std::fs::write(&sh, "#!/bin/sh\necho hi\n").unwrap();
        let no_shebang = dir.path().join("plain");
        std::fs::write(&no_shebang, "binary-ish").unwrap();
        assert!(is_js_bin(&js));
        assert!(!is_js_bin(&sh));
        assert!(!is_js_bin(&no_shebang));
        // 无后缀但 shebang 含 node → JS bin
        let bare = dir.path().join("barebin");
        std::fs::write(&bare, "#!/usr/bin/env node\nrequire('x')\n").unwrap();
        assert!(is_js_bin(&bare));
    }

    #[test]
    fn local_bin_walk_and_path_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        let bin_dir = root.join("node_modules").join(".bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::write(bin_dir.join("vite"), "#!/usr/bin/env node\n").unwrap();
        let sub = root.join("packages").join("foo");
        std::fs::create_dir_all(&sub).unwrap();
        // monorepo 向上命中根 .bin
        assert_eq!(
            find_local_bin(&sub, "vite"),
            Some(bin_dir.join("vite"))
        );
        assert_eq!(find_local_bin(&sub, "nope"), None);
        // PATH 前置：根 .bin 在最前，原 PATH 跟后
        let joined = path_with_bins(&sub);
        let first = joined.split(':').next().unwrap();
        assert!(first.ends_with("node_modules/.bin"), "{joined}");
    }

    #[test]
    fn shell_meta_table() {
        assert!(!has_shell_meta("vite --host"));
        assert!(!has_shell_meta("echo ok"));
        for s in ["a && b", "x | y", "echo 'a b'", "a > f", "echo $X"] {
            assert!(has_shell_meta(s), "{s}");
        }
        // `=` 非元字符：env 前缀脚本由解析回退兜住（首词解析不到 → shell 路径）。
    }
}
