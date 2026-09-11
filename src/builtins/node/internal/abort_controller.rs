//! `node:internal/abort_controller`——直通全局 AbortController（prelude 实现）。
/// 源：nodejs/node（MIT）对应 internal 件的最小对位实现；偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"const AbortController = globalThis.AbortController;
export { AbortController };
export default { AbortController };

"#;
