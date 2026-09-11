//! `node:internal/async_context_frame`——eos 所需面存根（无引擎 async 框架）。
/// 源：nodejs/node（MIT）对应 internal 件的最小对位实现；偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"const AsyncContextFrame = {
  current: () => null,
};
export { AsyncContextFrame };
export default AsyncContextFrame;

"#;
