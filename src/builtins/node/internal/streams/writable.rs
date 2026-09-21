//! `node:internal/streams/writable`（Node lib/internal/streams/writable.js 逐字内嵌，MIT）。
/// 来源：nodejs/node `internal/streams/writable.js`（MIT 头见源内）逐字内嵌；require → 垫片映射，
/// primordials → node:internal/primordials。偏差见 docs/plan.md Phase 9b。
/// §0.9 按域分块：`writable_head.js`（构造/setDefaultEncoding 前）+
/// `writable_flow.js`（writeOrBuffer 起收尾），concat 字节恒等。
pub const SOURCE: &str = concat!(
    include_str!("writable_head.js"),
    include_str!("writable_flow.js"),
);
