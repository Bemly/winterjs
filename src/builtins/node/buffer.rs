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

// isAscii/isUtf8（Node 21.1+/22+；@exodus/bytes 等取此面；真机 26.8.2 全集
// 就这两个——isUtf16Le/Be 非 Node 面，曾超集误加已删，§4.65 纪律）：
// 实参收 ArrayBuffer/视图，非法形 TypeError（validateBuffer 口径），
// detached 视 false。校验用 fatal TextDecoder（解码抛错即非法）。
function toU8(value) {
  if (value instanceof ArrayBuffer) return new Uint8Array(value);
  if (ArrayBuffer.isView(value)) {
    const ab = value.buffer;
    if (ab && ab.detached) return null;
    return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
  }
  throw new TypeError(
    `The "value" argument must be an ArrayBuffer or ArrayBufferView`,
  );
}
function isValidWith(label, value) {
  const u8 = toU8(value);
  if (u8 === null) return false;
  try {
    new TextDecoder(label, { fatal: true }).decode(u8);
    return true;
  } catch {
    return false;
  }
}
export function isAscii(value) {
  const u8 = toU8(value);
  if (u8 === null) return false;
  for (let i = 0; i < u8.length; i++) {
    if (u8[i] > 0x7f) return false;
  }
  return true;
}
export function isUtf8(value) {
  return isValidWith("utf-8", value);
}

const buffer = {
  Buffer,
  SlowBuffer,
  constants,
  INSPECT_MAX_BYTES: 50,
  kMaxLength,
  kStringMaxLength,
  isAscii,
  isUtf8,
  File: globalThis.File,
};

export default buffer;
export { Buffer, SlowBuffer, constants, kMaxLength, kStringMaxLength };
export const File = globalThis.File;
export const INSPECT_MAX_BYTES = 50;

"#;
