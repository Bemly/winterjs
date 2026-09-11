//! `node:internal/async_hooks_int`——`internal/async_hooks` 的 eos 所需面存根。
/// 源：nodejs/node（MIT）对应 internal 件的最小对位实现；偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"function enabledHooksExist() {
  return false; // stub 口径：createHook 不触发（plan2 9a 记档）
}
export { enabledHooksExist };
export default { enabledHooksExist };

"#;
