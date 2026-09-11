//! `node:internal/debuglog`——`internal/util/debuglog` 最小面（恒关闭，记档）。
/// 源：nodejs/node（MIT）对应 internal 件的最小对位实现；偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"function debuglog(_set, cb) {
  const fn = function debug() {};
  // Node 异步升级回调（同步会 TDZ：`let debug = debuglog('stream', fn => debug = fn)`）
  if (typeof cb === 'function') queueMicrotask(() => cb(fn));
  return fn;
}
export { debuglog };
export default { debuglog };

"#;
