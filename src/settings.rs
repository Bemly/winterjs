//! 配置加载：config crate（TOML/JSON/INI + `WINTERJS_` 前缀环境变量覆盖），
//! JSON Schema 输出用 schemars，取值枚举用 strum。
//! 文件：cwd 下 `winterjs.toml` / `winterjs.json` / `winterjs.ini`（config 按扩展名探测，
//! 缺省不存在也可；yaml 特性按 §2 禁用，`.yaml` 不会被读取）。
//! 环境变量嵌套键用 `__` 分隔，如 `WINTERJS_LOG__FILTER=debug`。

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
    /// EnvFilter 语法（如 `winterjs=debug,tokio=info`）；未设置时按 `-v` 计数
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

impl Settings {
    pub fn load() -> Result<Self, config::ConfigError> {
        Config::builder()
            .add_source(File::with_name("winterjs").required(false))
            // 显式 prefix_separator("_")：config 会把 prefix 分隔符默认成 separator（"__"），
            // 不显式给的话 WINTERJS_ 前缀永远匹配不上（WINTERJS__LOG__COLOR 才行）
            .add_source(
                Environment::with_prefix("WINTERJS")
                    .prefix_separator("_")
                    .separator("__"),
            )
            .build()?
            .try_deserialize()
    }
}
