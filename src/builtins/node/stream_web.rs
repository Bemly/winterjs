//! `node:stream/web`——全局 Web 流类 re-export（本仓 prelude 实现的流类；
/// 源：nodejs/node（MIT）对应件最小实现；偏差见 docs/plan.md Phase 9b。
/// CompressionStream/DecompressionStream（G9-3）：真机 26.8.2 对拍——
/// proto 链独立（不继承 TransformStream）、format 枚举 TypeError、
/// 解压尾垃圾 readable TypeError（ERR_TRAILING_JUNK_AFTER_STREAM_END）。
pub const SOURCE: &str = r#"// Node 面的 12 类（各策略/控制器/reader）待 prelude 补齐后扩，记档。）
const ReadableStream = globalThis.ReadableStream;
const WritableStream = globalThis.WritableStream;
const TransformStream = globalThis.TransformStream;
const CompressionStream = globalThis.CompressionStream;
const DecompressionStream = globalThis.DecompressionStream;

export {
  ReadableStream,
  WritableStream,
  TransformStream,
  CompressionStream,
  DecompressionStream,
};
export default { ReadableStream, WritableStream, TransformStream, CompressionStream, DecompressionStream };

"#;
