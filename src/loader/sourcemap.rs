//! sourcemap 查询：转译产物行列 → 原始行列（`sourcemap` crate）。
//! 无 map/解析失败/查无 token 时原样返回（永不报错）。

/// 转译产物 `(line, col)`（1-based）→ 原始 `(line, col)`。
pub fn remap_location(map_json: Option<&str>, line: u32, col: u32) -> (u32, u32) {
    let Some(map_json) = map_json else {
        return (line, col);
    };
    let Ok(sm) = sourcemap::SourceMap::from_slice(map_json.as_bytes()) else {
        return (line, col);
    };
    // sourcemap crate 行列均为 0-based
    let Some(tok) = sm.lookup_token(line.saturating_sub(1), col.saturating_sub(1)) else {
        return (line, col);
    };
    (tok.get_src_line() + 1, tok.get_src_col() + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_without_usable_map() {
        assert_eq!(remap_location(None, 3, 5), (3, 5));
        assert_eq!(remap_location(Some("not-json"), 3, 5), (3, 5));
        // 合法 JSON 但无 sourcesContent/token 覆盖行 → 原样
        assert_eq!(
            remap_location(Some(r#"{"version":3,"sources":["a.ts"],"mappings":""}"#), 3, 5),
            (3, 5)
        );
    }
}
