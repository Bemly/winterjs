//! 日志：tracing 家族接线。
//! 过滤优先级：`WINTERJS_LOG`（EnvFilter 语法）> `-v` 计数（0=warn, 1=info, 2=debug, ≥3=trace）。
//! 输出：stderr（stdout 留给程序输出）；`WINTERJS_LOG_FILE=path` 追加落盘文件层（无 ANSI）。

use std::env;
use std::ffi::OsString;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use tracing_error::ErrorLayer;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub fn init(verbosity: u8) {
    // SubscriberInitExt::init()（tracing-log 特性启用时）会把 log crate 的记录桥接进 tracing

    let ansi = std::io::stderr().is_terminal() && env::var_os("NO_COLOR").is_none();
    let registry = tracing_subscriber::registry()
        .with(env_filter(verbosity))
        .with(ErrorLayer::default());

    match env::var_os("WINTERJS_LOG_FILE") {
        Some(path) => registry
            .with(fmt::layer().with_ansi(false).with_writer(file_appender(&path)))
            .with(fmt::layer().with_ansi(ansi).with_writer(std::io::stderr))
            .init(),
        None => registry
            .with(fmt::layer().with_ansi(ansi).with_writer(std::io::stderr))
            .init(),
    }
}

fn env_filter(verbosity: u8) -> EnvFilter {
    if let Ok(spec) = env::var("WINTERJS_LOG") {
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

fn file_appender(path: &OsString) -> tracing_appender::rolling::RollingFileAppender {
    let path = PathBuf::from(path);
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let file = path.file_name().unwrap_or_default();
    tracing_appender::rolling::never(dir, file)
}
