//! 日志：tracing 家族接线。
//! 默认走 fmt 层；`--features tokio-console` 时改走 console 观测（dev 按需，见 Cargo 注释）。
//! 过滤优先级：`WINTERJS2_LOG`（EnvFilter 语法）> 配置文件 `log.filter` > `-v` 计数
//! （0=warn, 1=info, 2=debug, ≥3=trace）。
//! 输出：stderr（stdout 留给程序输出）；日志文件取 `WINTERJS2_LOG_FILE` > 配置文件 `log.file`。
//! `SubscriberInitExt::init()` 在 tracing-log 特性启用时会把 log crate 记录桥接进 tracing。

use std::path::PathBuf;

use crate::settings::ColorChoice;

#[cfg(not(feature = "tokio-console"))]
use std::env;
#[cfg(not(feature = "tokio-console"))]
use std::io::IsTerminal;
#[cfg(not(feature = "tokio-console"))]
use std::path::Path;

#[cfg(not(feature = "tokio-console"))]
use tracing_error::ErrorLayer;
#[cfg(not(feature = "tokio-console"))]
use tracing_subscriber::EnvFilter;
#[cfg(not(feature = "tokio-console"))]
use tracing_subscriber::fmt;
#[cfg(not(feature = "tokio-console"))]
use tracing_subscriber::layer::SubscriberExt;
#[cfg(not(feature = "tokio-console"))]
use tracing_subscriber::util::SubscriberInitExt;

#[cfg_attr(feature = "tokio-console", allow(dead_code))]
pub struct LogOptions {
    /// `-v` 计数（0=warn, 1=info, 2=debug, ≥3=trace）
    pub verbosity: u8,
    /// 配置文件里的 filter（`WINTERJS2_LOG` 环境变量仍优先于它）
    pub filter: Option<String>,
    pub color: ColorChoice,
    /// 配置文件里的日志文件（`WINTERJS2_LOG_FILE` 环境变量仍优先于它）
    pub file: Option<PathBuf>,
}

/// `tokio-console` 开关启用时：用 console 的 subscriber 替代默认 fmt 层
///（`tokio=trace,runtime=trace` 由它自动带上）。必须
/// `RUSTFLAGS="--cfg tokio_unstable" cargo build --features tokio-console`，
/// 否则编译期直接报错（上游 console 在运行时 assert 同一条，不如提前拦）。
#[cfg(feature = "tokio-console")]
#[cfg(not(tokio_unstable))]
compile_error!("feature `tokio-console` requires RUSTFLAGS=\"--cfg tokio_unstable\"");

#[cfg(feature = "tokio-console")]
#[cfg(tokio_unstable)]
pub fn init(_opts: LogOptions) {
    console_subscriber::init();
}

#[cfg(not(feature = "tokio-console"))]
pub fn init(opts: LogOptions) {
    let ansi = match opts.color {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => std::io::stderr().is_terminal() && env::var_os("NO_COLOR").is_none(),
    };
    let file = env::var_os("WINTERJS2_LOG_FILE").map(PathBuf::from).or(opts.file);
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

#[cfg(not(feature = "tokio-console"))]
fn env_filter(verbosity: u8, configured: Option<String>) -> EnvFilter {
    if let Ok(spec) = env::var("WINTERJS2_LOG") {
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
    EnvFilter::new(format!("winterjs2={level}"))
}

#[cfg(not(feature = "tokio-console"))]
fn file_appender(path: &Path) -> tracing_appender::rolling::RollingFileAppender {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let file = path.file_name().unwrap_or_default();
    tracing_appender::rolling::never(dir, file)
}
