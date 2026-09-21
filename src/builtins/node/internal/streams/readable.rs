//! `node:internal/streams/readable`（Node lib/internal/streams/readable.js 逐字内嵌，MIT）。
/// 来源：nodejs/node `internal/streams/readable.js`（MIT 头见源内）逐字内嵌；require → 垫片映射，
/// primordials → node:internal/primordials。偏差见 docs/plan.md Phase 9b。
/// §0.9 按域分块：`readable_head.js`（构造/push 系）+ `readable_flow.js`
/// （流动/pipe/监听系）+ `readable_iter.js`（迭代器/收尾），concat 字节恒等。
pub const SOURCE: &str = concat!(
    include_str!("readable_head.js"),
    include_str!("readable_flow.js"),
    include_str!("readable_iter.js"),
);
