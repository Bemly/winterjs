//! 错误模型：thiserror 定义类型，miette 渲染。
//! 非 TTY（stderr 非终端或 NO_COLOR）输出稳定的一行格式 + `Caused by` 链——
//! 脚本与测试依赖该格式（AGENTS.md §3 验收样例）；TTY 走 miette 图形渲染（带代码框）。

use std::io::IsTerminal;
use std::path::PathBuf;

use miette::{Diagnostic, GraphicalReportHandler, GraphicalTheme, NamedSource, SourceSpan};
use thiserror::Error;

use crate::settings::ColorChoice;

#[derive(Debug, Error, Diagnostic)]
pub enum Error {
    #[error("failed to read {path}")]
    #[diagnostic(code(winterjs::io::read), help("check that the path exists and is readable"))]
    IoRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to load settings: {source}")]
    #[diagnostic(code(winterjs::config))]
    Config {
        #[source]
        source: config::ConfigError,
    },

    #[error("{filename}:{line}:{col}: {message}")]
    #[diagnostic(code(winterjs::js::uncaught_exception))]
    Script {
        filename: String,
        line: u32,
        col: u32,
        message: String,
        #[label("uncaught here")]
        span: SourceSpan,
        #[source_code]
        source_code: NamedSource<String>,
    },

    #[error("{0}")]
    #[diagnostic(code(winterjs::internal))]
    Other(String),

    #[error("{0}")]
    #[diagnostic(code(winterjs::io))]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    #[diagnostic(code(winterjs::json))]
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

impl Error {
    pub fn script(filename: &str, source: &str, line: u32, col: u32, message: String) -> Self {
        Error::Script {
            filename: filename.to_owned(),
            line,
            col,
            message,
            span: span_for(source, line, col),
            source_code: NamedSource::new(filename, source.to_owned()),
        }
    }

    /// 渲染到 stderr 并返回退出码 1。TTY（或 color=always）时用 miette 图形渲染，
    /// NO_COLOR / color=never 降为无色图形，非 TTY 走稳定一行格式。
    pub fn render(&self, color: ColorChoice) -> std::process::ExitCode {
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
            eprint!("{out}");
        } else {
            eprintln!("Error: {self}");
            let mut source = std::error::Error::source(self);
            let mut first = true;
            while let Some(err) = source {
                if first {
                    eprintln!();
                    eprintln!("Caused by:");
                    first = false;
                }
                eprintln!("    {err}");
                source = err.source();
            }
        }
        std::process::ExitCode::from(1)
    }
}
