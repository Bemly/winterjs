//! `node:` 内建表（plan Phase 4）：注册名 → 内嵌 ESM 源。
//! resolve 规范名为 `node:X` URL（`fs` 与 `node:fs` 同一模块，见 `normalize_spec`）；
//! `prepare()` 取源走 `source()`，不经过 fetch。

pub mod assert;
pub mod child;
pub mod fs;
pub mod os;
pub mod path;
pub mod process_;
pub mod testmod;

/// 全局 `process` 等启动期求值的 JS（`runtime` 在主 PRELUDE 后求值）。
/// 版本占位 `26.9.0` 在求值前替换为 `CARGO_PKG_VERSION`（发版不漂移）。
pub fn node_prelude() -> String {
    process_::PROCESS_PRELUDE.replace("26.9.0", env!("CARGO_PKG_VERSION"))
}

/// 内建源表（规范名 → ESM 源；源内只用全局 natives，不互引）。
const BUILTINS: &[(&str, &str)] = &[
    ("node:path", path::SOURCE),
    ("node:os", os::SOURCE),
    ("node:process", process_::SOURCE),
    ("node:fs", fs::SOURCE),
    ("node:fs/promises", fs::PROMISES_SOURCE),
    ("node:child_process", child::SOURCE),
    ("node:assert", assert::SOURCE),
    ("node:test", testmod::SOURCE),
];

/// spec 规范化（`node:` 前缀可选；未知返回 None，调用方报可用列表）。
/// 纯函数，单元测试覆盖（`node_os_table`）。
pub fn normalize_spec(spec: &str) -> Option<&'static str> {
    match spec.strip_prefix("node:").unwrap_or(spec) {
        "path" => Some("node:path"),
        "os" => Some("node:os"),
        "process" => Some("node:process"),
        "fs" => Some("node:fs"),
        "fs/promises" => Some("node:fs/promises"),
        "child_process" => Some("node:child_process"),
        "assert" => Some("node:assert"),
        "test" => Some("node:test"),
        _ => None,
    }
}

/// 规范名 → 内嵌源。
pub fn source(canonical: &str) -> Option<&'static str> {
    BUILTINS.iter().find(|(name, _)| *name == canonical).map(|(_, src)| *src)
}

/// 可用内建列表（报错信息用）。
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
        assert!(source("node:path").is_some());
        assert!(source("node:fs").is_some());
        assert!(source("node:fs/promises").is_some());
        assert!(source("node:child_process").is_some());
        assert!(source("node:assert").is_some());
        assert!(source("node:test").is_some());
        assert_eq!(
            available(),
            vec!["node:path", "node:os", "node:process", "node:fs", "node:fs/promises", "node:child_process", "node:assert", "node:test"]
        );
    }
}
