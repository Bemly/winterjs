//! `node:internal/snapshot`——`internal/v8/startup_snapshot` 对位（恒非快照构建）。
/// 源：nodejs/node（MIT）对应 internal 件的最小对位实现；偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"const namespace = { isBuildingSnapshot: () => false };
export { namespace };
export default { namespace };

"#;
