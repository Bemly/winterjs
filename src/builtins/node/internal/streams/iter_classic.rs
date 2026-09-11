//! `node:internal/streams/iter_classic`——`--experimental-stream-iter` 旗后的
/// 源：nodejs/node（MIT）对应件最小实现；偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"// batched 迭代路径；旗恒关（options stub），到不了这里——保留形状（记档）。
const kValidatedSource = Symbol('kValidatedSource');

function createBatchedAsyncIterator() {
  throw new Error('stream/iter batched iterator requires --experimental-stream-iter');
}

function normalizeBatch() {
  throw new Error('stream/iter batched iterator requires --experimental-stream-iter');
}

export { createBatchedAsyncIterator, normalizeBatch, kValidatedSource };
export default { createBatchedAsyncIterator, normalizeBatch, kValidatedSource };

"#;
