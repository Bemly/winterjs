//! `node:stream/web`——全局 Web 流类 re-export（本仓 prelude 实现的三类；
/// 源：nodejs/node（MIT）对应件最小实现；偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"// Node 面的 12 类（各策略/控制器/reader）待 prelude 补齐后扩，记档。）
const ReadableStream = globalThis.ReadableStream;
const WritableStream = globalThis.WritableStream;
const TransformStream = globalThis.TransformStream;

export {
  ReadableStream,
  WritableStream,
  TransformStream,
};
export default { ReadableStream, WritableStream, TransformStream };

"#;
