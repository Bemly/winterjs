//! 错误模型：thiserror 定义类型，miette 渲染。
//! 非 TTY：未捕获 JS 异常按 node 形渲染（`file:line` + 源行 + `^` + `Name: msg` +
//! `    at …` 栈，2026-09-25 D4），其余错误一行格式 + `Caused by` 链；
//! TTY 走 miette 图形渲染（带代码框）。`Display` 仍是一行 `file:L:C: msg`
//! （worker 透传/退出码解析等程序化用途依赖它，不随渲染改）。

use std::io::IsTerminal;
use std::path::PathBuf;

use miette::{Diagnostic, GraphicalReportHandler, GraphicalTheme, NamedSource, SourceSpan};
use thiserror::Error;

use crate::settings::ColorChoice;

/// CLI 主进程的错误渲染配色（main 启动时登记一次）。登记过 = 由运行时就地渲染致命错
/// （node 序：先打印错误、再派发 process 'exit'）；testrun 等内嵌调用方不登记，照旧拿原错自理。
static RENDER_COLOR: std::sync::OnceLock<ColorChoice> = std::sync::OnceLock::new();

pub fn set_render_color(color: ColorChoice) {
    let _ = RENDER_COLOR.set(color);
}

pub fn render_color() -> Option<ColorChoice> {
    RENDER_COLOR.get().copied()
}

#[derive(Debug, Error, Diagnostic)]
pub enum Error {
    #[error("failed to read {path}")]
    #[diagnostic(code(winterjs2::io::read), help("check that the path exists and is readable"))]
    IoRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to load settings: {source}")]
    #[diagnostic(code(winterjs2::config))]
    Config {
        #[source]
        source: config::ConfigError,
    },

    #[error("{filename}:{line}:{col}: {message}")]
    #[diagnostic(code(winterjs2::js::uncaught_exception))]
    Script {
        filename: String,
        line: u32,
        col: u32,
        message: String,
        /// 异常类名（"SyntaxError"/"TypeError"/…；10f worker 错误形状透传用，
        /// 主进程渲染不感知）。None = 引擎侧未取（旧路径）。
        kind: Option<String>,
        #[label("uncaught here")]
        span: SourceSpan,
        #[source_code]
        source_code: NamedSource<String>,
        /// 异常对象的 `stack`（SM `fn@file:L:C` 形；渲染时转 node `    at` 形）。
        stack: Option<String>,
    },

    #[error("{0}")]
    #[diagnostic(code(winterjs2::internal))]
    Other(String),

    /// `process.exit(code)` / `exitCode` 收尾：静默以 code 退出（不渲染）。
    /// 由 `process.exit` 哨兵错逐层转换（见 `runtime::exit_code_from_message`）。
    #[error("process exit({0})")]
    #[diagnostic(code(winterjs2::process::exit))]
    Exit(i32),

    #[error("{0}")]
    #[diagnostic(code(winterjs2::io))]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    #[diagnostic(code(winterjs2::json))]
    Json(#[from] serde_json::Error),
}

/// 按 1-based line/col 求出错处到行尾的字节 span（行内近似按字节，够定位用）。
fn span_for(source: &str, line: u32, col: u32) -> SourceSpan {
    let line_idx = line.saturating_sub(1) as usize;
    let Some(line_text) = source.lines().nth(line_idx) else {
        return SourceSpan::new(0.into(), 0);
    };
    let prefix_len: usize = source
        .lines()
        .take(line_idx)
        .map(|l| l.len() + 1)
        .sum();
    let start = prefix_len
        .saturating_add(col.saturating_sub(1) as usize)
        .min(source.len());
    let within_line = start.saturating_sub(prefix_len).min(line_text.len());
    let len = line_text.len().saturating_sub(within_line).max(1);
    SourceSpan::new(start.into(), len)
}

thread_local! {
    /// 最近一次报错点记下的（message, stack）——`jsapi_glue::fill_message` 写，
    /// `script_with_kind` 按 message 相等取走（不等即丢弃，防串到别的错误上）。
    static NOTED_STACK: std::cell::RefCell<Option<(String, String, Option<String>)>> =
        const { std::cell::RefCell::new(None) };
}

/// 报错点记下异常栈 + 类名（见 `NOTED_STACK`）。
pub fn note_stack(message: &str, stack: String, kind: Option<String>) {
    NOTED_STACK.with(|c| *c.borrow_mut() = Some((message.to_owned(), stack, kind)));
}

fn take_noted(message: &str) -> Option<(String, Option<String>)> {
    NOTED_STACK.with(|c| {
        let got = c.borrow_mut().take()?;
        (got.0 == message).then_some((got.1, got.2))
    })
}

/// 入口 rejection 串（`file:L:C: message`，见 `state::entry_reason_string`）→ node 形
/// Script 错误；无位置的值串回 None（调用方走一行格式）。`source` 为入口源码（代码框用）。
pub fn from_entry_reason(reason: &str, entry_url: &str, source: &str) -> Option<Error> {
    let (head, message) = reason.split_once(": ")?;
    let mut it = head.rsplitn(3, ':');
    let col: u32 = it.next()?.parse().ok()?;
    let line: u32 = it.next()?.parse().ok()?;
    let file = it.next()?;
    let src = if file == entry_url { source } else { "" };
    Some(Error::script_with_kind(file, src, line, col, message.to_owned(), None))
}

/// SM 栈帧（`fn@file:L:C` / `@file:L:C`）→ node 形 `    at fn (file:L:C)`；
/// 宿主管线帧（`__wjs2_*` 文件）滤掉，空行丢弃。
fn node_stack_lines(stack: &str) -> Vec<String> {
    stack
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let (name, loc) = match l.rfind('@') {
                Some(i) => (&l[..i], &l[i + 1..]),
                None => ("", l),
            };
            if loc.starts_with("__wjs2_") {
                return None;
            }
            Some(if name.is_empty() {
                format!("    at {loc}")
            } else {
                format!("    at {name} ({loc})")
            })
        })
        .collect()
}

/// node 形未捕获异常块（非 TTY）。
fn render_script_node_style(
    filename: &str,
    line: u32,
    col: u32,
    message: &str,
    kind: Option<&str>,
    source: &str,
    stack: Option<&str>,
) -> String {
    // 头行定位：引擎报告的行列若与栈顶帧不符（错误在内建模块里构造、报告丢了文件名，
    // 如 AssertionError 报 `node:assert:9` 的行号却安上入口名），以栈顶帧为准。
    let frames = stack.map(node_stack_lines).unwrap_or_default();
    let top = frames.first().and_then(|f| {
        let loc = f.trim_start().strip_prefix("at ")?;
        let loc = loc.rsplit_once(" (").map(|(_, l)| l.trim_end_matches(')')).unwrap_or(loc);
        let mut it = loc.rsplitn(3, ':');
        let (c, l, file) = (it.next()?.parse::<u32>().ok()?, it.next()?.parse::<u32>().ok()?, it.next()?);
        Some((file.to_owned(), l, c))
    });
    let (filename, line, col, source) = match &top {
        Some((file, l, c)) => {
            let base = |p: &str| p.rsplit('/').next().unwrap_or(p).to_owned();
            let same = file.ends_with(filename.trim_start_matches("./")) || base(file) == base(filename);
            if (*l, *c) != (line, col) || !same {
                (file.as_str(), *l, *c, if same { source } else { "" })
            } else {
                (filename, line, col, source)
            }
        }
        None => (filename, line, col, source),
    };
    let mut out = format!("{filename}:{line}\n");
    if let Some(text) = source.lines().nth(line.saturating_sub(1) as usize) {
        out.push_str(text);
        out.push('\n');
        out.push_str(&" ".repeat(col.saturating_sub(1) as usize));
        out.push_str("^\n");
    }
    out.push('\n');
    // 头行：`Name: msg`；message 已自带同名前缀（native report_error 形）则不重复。
    // 非对象抛出（`throw 42`）：node 直接打印值本身。
    if kind.is_none() {
        if let Some(v) = message.strip_prefix("uncaught exception: ") {
            out.push_str(v);
            out.push('\n');
            return out;
        }
    }
    let name = kind.unwrap_or("Error");
    if message.starts_with(&format!("{name}:")) || message.starts_with(&format!("{name} [")) {
        out.push_str(message);
    } else if message.is_empty() {
        out.push_str(name);
    } else {
        out.push_str(&format!("{name}: {message}"));
    }
    out.push('\n');
    for l in frames {
        out.push_str(&l);
        out.push('\n');
    }
    out
}

impl Error {
    pub fn script(filename: &str, source: &str, line: u32, col: u32, message: String) -> Self {
        Self::script_with_kind(filename, source, line, col, message, None)
    }

    /// 带异常类名的变体（worker 错误形状：SyntaxError/TypeError 等跨线程还原）。
    pub fn script_with_kind(
        filename: &str,
        source: &str,
        line: u32,
        col: u32,
        message: String,
        kind: Option<String>,
    ) -> Self {
        let noted = take_noted(&message);
        Error::Script {
            filename: filename.to_owned(),
            line,
            col,
            span: span_for(source, line, col),
            source_code: NamedSource::new(filename, source.to_owned()),
            stack: noted.as_ref().map(|n| n.0.clone()),
            kind: kind.or_else(|| noted.and_then(|n| n.1)),
            message,
        }
    }

    /// 渲染为文本（不落流）：三路同形——TTY（或 color=always）miette 图形、
    /// NO_COLOR / color=never 无色图形、非 TTY JS 异常 node 形 / 其余一行格式。
    /// `render()` 据此落 stderr；REPL 据此拿文本做 CRLF 化（raw mode 终端）。
    pub(crate) fn render_string(&self, color: ColorChoice) -> String {
        let tty = std::io::stderr().is_terminal();
        let fancy = match color {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => tty,
        };
        let no_color = std::env::var_os("NO_COLOR").is_some();
        if fancy {
            let theme = if no_color {
                GraphicalTheme::unicode_nocolor()
            } else {
                GraphicalTheme::unicode()
            };
            let handler = GraphicalReportHandler::new_themed(theme);
            let mut out = String::new();
            let _ = handler.render_report(&mut out, self);
            out
        } else if let Error::Script { filename, line, col, message, kind, source_code, stack, .. } = self {
            render_script_node_style(
                filename, *line, *col, message, kind.as_deref(), source_code.inner(), stack.as_deref(),
            )
        } else {
            let mut out = format!("Error: {self}\n");
            let mut source = std::error::Error::source(self);
            let mut first = true;
            while let Some(err) = source {
                if first {
                    out.push('\n');
                    out.push_str("Caused by:\n");
                    first = false;
                }
                out.push_str(&format!("    {err}\n"));
                source = err.source();
            }
            out
        }
    }

    /// 渲染到 stderr 并返回退出码 1（文本与 `render_string` 恒等，见其注记）。
    pub fn render(&self, color: ColorChoice) -> std::process::ExitCode {
        eprint!("{}", self.render_string(color));
        std::process::ExitCode::from(1)
    }
}

#[cfg(test)]
mod node_shape_tests {
    use super::*;

    #[test]
    fn stack_frames_to_node_form() {
        let st = "f@file:///a.js:3:21\n@file:///a.js:4:1\n@__wjs2_main_bootstrap.js:1:1\n\n";
        assert_eq!(node_stack_lines(st), ["    at f (file:///a.js:3:21)", "    at file:///a.js:4:1"]);
    }

    #[test]
    fn render_block_shapes() {
        let out = render_script_node_style("a.js", 1, 7, "boom", None, "throw new Error(\"boom\")", Some("@a.js:1:7"));
        assert_eq!(out, "a.js:1\nthrow new Error(\"boom\")\n      ^\n\nError: boom\n    at a.js:1:7\n");
        // 报错：message 自带同名前缀不重复；无源行不出代码框。
        let out = render_script_node_style("x", 2, 1, "TypeError: t", Some("TypeError"), "", None);
        assert_eq!(out, "x:2\n\nTypeError: t\n");
        // 边界：非对象抛出直接打印值。
        let out = render_script_node_style("e.js", 1, 1, "uncaught exception: 42", None, "throw 42", None);
        assert!(out.ends_with("^\n\n42\n"), "{out}");
    }

    #[test]
    fn header_follows_stack_top_when_report_disagrees() {
        // 引擎报告行列（9:5，内建模块构造点）与栈顶不符 → 头行取栈顶帧，他文件不出代码框。
        let out = render_script_node_style(
            "t.js", 9, 5, "boom", Some("AssertionError"), "l1\n",
            Some("AssertionError@node:assert:9:5\n@file:///x/t.js:26:16"),
        );
        assert!(out.starts_with("node:assert:9\n\nAssertionError: boom"), "{out}");
    }

    #[test]
    fn entry_reason_parsing() {
        let e = from_entry_reason("file:///m.mjs:3:7: bad: thing", "file:///m.mjs", "a\nb\nthrow x\n").unwrap();
        let Error::Script { filename, line, col, message, .. } = e else { panic!() };
        assert_eq!((filename.as_str(), line, col, message.as_str()), ("file:///m.mjs", 3, 7, "bad: thing"));
        // 值串（无位置）回 None。
        assert!(from_entry_reason("just a value", "file:///m.mjs", "").is_none());
    }
}
