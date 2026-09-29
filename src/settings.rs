//! 配置加载：config crate（TOML/JSON/INI/YAML + `WINTERJS2_` 前缀环境变量覆盖），
//! JSON Schema 输出用 schemars，取值枚举用 strum。
//! 文件：cwd 下 `winterjs2.toml` / `winterjs2.json` / `winterjs2.ini` / `winterjs2.yaml`
//! （config 按扩展名探测，缺省不存在也可；yaml 特性 2026-09-10 经纯度审计启用）。
//! 环境变量嵌套键用 `__` 分隔，如 `WINTERJS2_LOG__FILTER=debug`。

use std::path::PathBuf;

use config::{Config, Environment, File};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::EnumString;

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct Settings {
    #[serde(default)]
    pub log: LogSettings,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct LogSettings {
    /// EnvFilter 语法（如 `winterjs2=debug,tokio=info`）；未设置时按 `-v` 计数
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    /// 终端着色策略
    #[serde(default)]
    pub color: ColorChoice,
    /// 追加写日志文件路径
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, EnumString, strum::Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

/// 日志运行时的直读变量（`logging.rs` 直接读，不经 config）；见 `Settings::load`。
const RUNTIME_LOG_VARS: &[&str] = &["WINTERJS2_LOG", "WINTERJS2_LOG_FILE"];

impl Settings {
    pub fn load() -> Result<Self, config::ConfigError> {
        // §4.10：`WINTERJS2_LOG` / `WINTERJS2_LOG_FILE` 是日志运行时的直读变量，
        // 按前缀规则会被 config 误收进 `log` 表导致反序列化失败；加载期间暂存移出，完后恢复
        let stash: Vec<(String, std::ffi::OsString)> = RUNTIME_LOG_VARS
            .iter()
            .filter_map(|k| std::env::var_os(k).map(|v| (k.to_string(), v)))
            .collect();
        for (k, _) in &stash {
            // SAFETY: 进程启动期主线程独占（尚未 spawn 线程）；返回前全部恢复
            unsafe { std::env::remove_var(k) };
        }
        let built = Config::builder()
            .add_source(File::with_name("winterjs2").required(false))
            // 显式 prefix_separator("_")：config 会把 prefix 分隔符默认成 separator（"__"），
            // 不显式给的话 WINTERJS2_ 前缀永远匹配不上（WINTERJS2__LOG__COLOR 才行）
            .add_source(
                Environment::with_prefix("WINTERJS")
                    .prefix_separator("_")
                    .separator("__"),
            )
            .build();
        for (k, v) in stash {
            // SAFETY: 同上
            unsafe { std::env::set_var(k, v) };
        }
        built?.try_deserialize()
    }
}
