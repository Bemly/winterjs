//! 日志：tracing 家族接线。
//! 过滤优先级：`WINTERJS_LOG`（EnvFilter 语法）> 配置文件 `log.filter` > `-v` 计数
//! （0=warn, 1=info, 2=debug, ≥3=trace）。
//! 输出：stderr（stdout 留给程序输出）；日志文件取 `WINTERJS_LOG_FILE` > 配置文件 `log.file`。
//! `SubscriberInitExt::init()` 在 tracing-log 特性启用时会把 log crate 记录桥接进 tracing。

use std::env;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use tracing_error::ErrorLayer;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::settings::ColorChoice;

pub struct LogOptions {
    /// `-v` 计数（0=warn, 1=info, 2=debug, ≥3=trace）
    pub verbosity: u8,
    /// 配置文件里的 filter（`WINTERJS_LOG` 环境变量仍优先于它）
    pub filter: Option<String>,
    pub color: ColorChoice,
    /// 配置文件里的日志文件（`WINTERJS_LOG_FILE` 环境变量仍优先于它）
    pub file: Option<PathBuf>,
}

pub fn init(opts: LogOptions) {
    let ansi = match opts.color {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => std::io::stderr().is_terminal() && env::var_os("NO_COLOR").is_none(),
    };
    let file = env::var_os("WINTERJS_LOG_FILE").map(PathBuf::from).or(opts.file);
    let registry = tracing_subscriber::registry()
        .with(env_filter(opts.verbosity, opts.filter))
        .with(ErrorLayer::default());

    match file {
        Some(path) => registry
            .with(fmt::layer().with_ansi(false).with_writer(file_appender(&path)))
            .with(fmt::layer().with_ansi(ansi).with_writer(std::io::stderr))
            .init(),
        None => registry
            .with(fmt::layer().with_ansi(ansi).with_writer(std::io::stderr))
            .init(),
    }
}

fn env_filter(verbosity: u8, configured: Option<String>) -> EnvFilter {
    if let Ok(spec) = env::var("WINTERJS_LOG") {
        if let Ok(filter) = EnvFilter::try_new(&spec) {
            return filter;
        }
    }
    if let Some(spec) = configured {
        if let Ok(filter) = EnvFilter::try_new(&spec) {
            return filter;
        }
    }
    let level = match verbosity {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    // 默认只对本 crate 放行，依赖库保持安静
    EnvFilter::new(format!("winterjs={level}"))
}

fn file_appender(path: &Path) -> tracing_appender::rolling::RollingFileAppender {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let file = path.file_name().unwrap_or_default();
    tracing_appender::rolling::never(dir, file)
}
