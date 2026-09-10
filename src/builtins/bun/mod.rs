//! `bun:` 内建表（plan Phase 7-e4）：注册名 → 内嵌 ESM 源。
//! resolve 经 `bun:` scheme 截获（`import "bun:sqlite"` 整体是合法 URL）；
//! `prepare()` 取源走 `source()`，不经过 fetch。裸导入 `sqlite` 不截获（是 npm 包名）。

pub mod ffi;
pub mod sqlite;

/// 内建源表（规范名 → ESM 源；源内只用全局 natives，不互引）。
const BUILTINS: &[(&str, &str)] = &[("bun:ffi", ffi::SOURCE), ("bun:sqlite", sqlite::SOURCE)];

/// spec 规范化（只认 `bun:` 前缀；未知返回 None，调用方报可用列表）。
/// 纯函数，单元测试覆盖（`bun_spec_table`）。
pub fn normalize_spec(spec: &str) -> Option<&'static str> {
    match spec.strip_prefix("bun:")? {
        "ffi" => Some("bun:ffi"),
        "sqlite" => Some("bun:sqlite"),
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
    fn bun_spec_table() {
        assert_eq!(normalize_spec("bun:sqlite"), Some("bun:sqlite"));
        assert_eq!(normalize_spec("bun:mysql"), None);
        assert_eq!(normalize_spec("bun:"), None);
        assert_eq!(normalize_spec("sqlite"), None);
        assert_eq!(normalize_spec(""), None);
        assert_eq!(normalize_spec("node:sqlite"), None);
        assert!(source("bun:sqlite").is_some());
        assert!(source("bun:ffi").is_some());
        assert_eq!(available(), vec!["bun:ffi", "bun:sqlite"]);
    }
}
