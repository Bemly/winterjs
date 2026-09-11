//! `node:` 内建表（plan Phase 4；Phase 9a 起含互引 + `node:internal/*`）：
//! 注册名 → 内嵌 ESM 源。resolve 规范名为 `node:X` URL（`fs` 与 `node:fs` 同一
//! 模块，见 `normalize_spec`）；`prepare()` 取源走 `source()`，不经过 fetch。
//!
//! Phase 9a 起：内嵌源可经**绝对 `node:` URL** 互引（loader resolve 不依赖 base）；
//! `node:internal/*` 共享小件走 `internal::INTERNALS`，只供内建互引——
//! `normalize_spec` 接受（宽松），但**不进 `available()` 报错列表**。

pub mod assert;
pub mod async_hooks;
pub mod child;
pub mod events;
pub mod fs;
pub mod internal;
pub mod os;
pub mod path;
pub mod process_;
pub mod require;
pub mod testmod;
pub mod util;
pub mod util_types;

/// 全局 `process` 等启动期求值的 JS（`runtime` 在主 PRELUDE 后求值）。
/// 版本占位 `26.9.11` 在求值前替换为 `CARGO_PKG_VERSION`（发版时两处同步改，不漂移）。
pub fn node_prelude() -> String {
    let base = process_::PROCESS_PRELUDE.replace("26.9.11", env!("CARGO_PKG_VERSION"));
    format!("{base}\n{}", require::REQUIRE_PRELUDE)
}

/// 内建源表（规范名 → ESM 源；互引走绝对 `node:` URL，natives 走全局 `__wjs_*`）。
/// `node:internal/*` 不在本表——`internal::INTERNALS` 为其唯一源（source() 先查它），
/// 保证 `available()` 天然不含 internal。
const BUILTINS: &[(&str, &str)] = &[
    ("node:path", path::SOURCE),
    ("node:os", os::SOURCE),
    ("node:process", process_::SOURCE),
    ("node:fs", fs::SOURCE),
    ("node:fs/promises", fs::PROMISES_SOURCE),
    ("node:child_process", child::SOURCE),
    ("node:assert", assert::SOURCE),
    ("node:test", testmod::SOURCE),
    // Phase 9a
    ("node:async_hooks", async_hooks::SOURCE),
    ("node:events", events::SOURCE),
    ("node:util", util::SOURCE),
    ("node:util/types", util_types::SOURCE),
];

/// spec 规范化（`node:` 前缀可选；internal 走 `internal::normalize_internal`；
/// 未知返回 None，调用方报可用列表）。
/// 纯函数，单元测试覆盖（`node_spec_table`）。
pub fn normalize_spec(spec: &str) -> Option<&'static str> {
    if let Some(canonical) = internal::normalize_internal(spec) {
        return Some(canonical);
    }
    match spec.strip_prefix("node:").unwrap_or(spec) {
        "path" => Some("node:path"),
        "os" => Some("node:os"),
        "process" => Some("node:process"),
        "fs" => Some("node:fs"),
        "fs/promises" => Some("node:fs/promises"),
        "child_process" => Some("node:child_process"),
        "assert" => Some("node:assert"),
        "test" => Some("node:test"),
        "async_hooks" => Some("node:async_hooks"),
        "events" => Some("node:events"),
        "util" => Some("node:util"),
        "util/types" => Some("node:util/types"),
        _ => None,
    }
}

/// 规范名 → 内嵌源（internal 表一并可查）。
pub fn source(canonical: &str) -> Option<&'static str> {
    if let Some(src) = internal::source(canonical) {
        return Some(src);
    }
    BUILTINS.iter().find(|(name, _)| *name == canonical).map(|(_, src)| *src)
}

/// 可用内建列表（报错信息用；`node:internal/*` 只供互引，不列出）。
pub fn available() -> Vec<&'static str> {
    BUILTINS.iter().map(|(name, _)| *name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_spec_table() {
        assert_eq!(normalize_spec("node:path"), Some("node:path"));
        assert_eq!(normalize_spec("path"), Some("node:path"));
        assert_eq!(normalize_spec("node:os"), Some("node:os"));
        assert_eq!(normalize_spec("os"), Some("node:os"));
        assert_eq!(normalize_spec("node:process"), Some("node:process"));
        assert_eq!(normalize_spec("process"), Some("node:process"));
        assert_eq!(normalize_spec("node:fs"), Some("node:fs"));
        assert_eq!(normalize_spec("fs"), Some("node:fs"));
        assert_eq!(normalize_spec("node:fs/promises"), Some("node:fs/promises"));
        assert_eq!(normalize_spec("fs/promises"), Some("node:fs/promises"));
        assert_eq!(normalize_spec("node:child_process"), Some("node:child_process"));
        assert_eq!(normalize_spec("child_process"), Some("node:child_process"));
        assert_eq!(normalize_spec("node:assert"), Some("node:assert"));
        assert_eq!(normalize_spec("node:test"), Some("node:test"));
        assert_eq!(normalize_spec("node:fs/watch"), None);
        assert_eq!(normalize_spec("node:"), None);
        assert_eq!(normalize_spec(""), None);
        // Phase 9a
        assert_eq!(normalize_spec("node:events"), Some("node:events"));
        assert_eq!(normalize_spec("events"), Some("node:events"));
        assert_eq!(normalize_spec("node:async_hooks"), Some("node:async_hooks"));
        assert_eq!(normalize_spec("async_hooks"), Some("node:async_hooks"));
        assert_eq!(normalize_spec("node:util"), Some("node:util"));
        assert_eq!(normalize_spec("util"), Some("node:util"));
        assert_eq!(normalize_spec("node:util/types"), Some("node:util/types"));
        assert_eq!(normalize_spec("util/types"), Some("node:util/types"));
        assert!(source("node:util").is_some());
        assert!(source("node:util/types").is_some());
        // internal：可解析、可取源，但不在 available()
        assert_eq!(normalize_spec("node:internal/errors"), Some("node:internal/errors"));
        assert_eq!(normalize_spec("internal/errors"), Some("node:internal/errors"));
        assert_eq!(
            normalize_spec("internal/events/abort_listener"),
            Some("node:internal/events/abort_listener")
        );
        assert!(source("node:internal/errors").is_some());
        assert!(source("node:events").is_some());
        assert!(source("node:async_hooks").is_some());
        assert_eq!(normalize_spec("node:internal/nope"), None);
        assert_eq!(normalize_spec("node:nope"), None);
        for name in available() {
            assert!(!name.starts_with("node:internal/"), "{name} leaked into available()");
            assert!(source(name).is_some(), "{name} missing source");
        }
        assert_eq!(available().len(), BUILTINS.len());
    }
}
