//! 包管理黑盒测试（tests/pm/ 按 src/pm 对齐；共享见 tests/common）。

mod common;
#[path = "pm/install.rs"]
mod install;
#[path = "pm/npmrc.rs"]
mod npmrc;
#[path = "pm/registry.rs"]
mod registry;
#[path = "pm/git.rs"]
mod git;
#[path = "pm/publish.rs"]
mod publish;
#[path = "pm/upgrade.rs"]
mod upgrade;
#[path = "pm/cache.rs"]
mod cache;
#[path = "pm/lifecycle.rs"]
mod lifecycle;
#[path = "pm/release.rs"]
mod release;
#[path = "pm/remove.rs"]
mod remove;
#[path = "pm/helpers.rs"]
mod helpers;
