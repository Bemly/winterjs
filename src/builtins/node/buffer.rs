//! `node:buffer`——全局 Buffer（Uint8Array 子类，prelude 实现）的模块面。
/// 源：nodejs/node（MIT）对应件最小实现；偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"const Buffer = globalThis.Buffer;

const kMaxLength = 2147483647;
const kStringMaxLength = 536870888;

const constants = {
  MAX_LENGTH: kMaxLength,
  MAX_STRING_LENGTH: kStringMaxLength,
  kMaxLength,
  kStringMaxLength,
};

// Deprecated alias（Node 口径）：等价 new Buffer(size)。
// 注意不能写 `new Buffer.alloc`——class 静态方法无 [[Construct]]，直接 TypeError。
const SlowBuffer = function SlowBuffer(size) {
  return new Buffer(size);
};

const buffer = {
  Buffer,
  SlowBuffer,
  constants,
  INSPECT_MAX_BYTES: 50,
  kMaxLength,
  kStringMaxLength,
};

export default buffer;
export { Buffer, SlowBuffer, constants, kMaxLength, kStringMaxLength };
export const INSPECT_MAX_BYTES = 50;

"#;
