//! `node:internal/assert`（Node internal/assert.js 最小面，MIT）。
/// 源：nodejs/node（MIT）对应 internal 件的最小对位实现；偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"'use strict';
function assert(value, message) {
  if (!value) {
    throw new Error('Internal Assertion Violation: ' + message);
  }
}
export { assert };
export default assert;

"#;
