//! `node:internal/streams/iter_pull`（Node internal/streams/iter/pull.js 逐字内嵌，MIT）。
/// §0.9 按方法边界切片：`iter_pull_head.js`（前半）+ `iter_pull_tail.js`（后半），concat 字节恒等。
pub const SOURCE: &str = concat!(
    include_str!("iter_pull_head.js"),
    include_str!("iter_pull_tail.js"),
);
