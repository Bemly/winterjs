//! `node:internal/buffer`——FastBuffer（Buffer 薄子类；kMaxLength 取 2^31-1，记档）。
/// 源：nodejs/node（MIT）对应 internal 件的最小对位实现；偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"class FastBuffer extends Uint8Array {
  // Using an empty constructor is intentional; avoids Buffer checks overhead.
  constructor(...args) {
    super(...args);
  }
}
// lib/buffer.js:157 口径：FastBuffer 实例在 instanceof/isBuffer/constructor.name
// 下须表现为 Buffer（Node 用共享原型 Buffer.prototype = FastBuffer.prototype 实现；
// 此处用原型链桥接，`Object.getPrototypeOf(chunk) === Buffer.prototype` 为假，记档偏差）。
if (typeof Buffer !== 'undefined') {
  FastBuffer.prototype.constructor = Buffer;
  Object.setPrototypeOf(FastBuffer.prototype, Buffer.prototype);
}
const kMaxLength = 2147483647;
const kStringMaxLength = 536870888;
export { FastBuffer, kMaxLength, kStringMaxLength };
export default { FastBuffer, kMaxLength, kStringMaxLength };

"#;
