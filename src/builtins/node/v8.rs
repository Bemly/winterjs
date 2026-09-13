//! `node:v8`（Node `lib/v8.js` 最小桥，MIT；plan 9j：解 vite `-r dev` 挡路）。
//!
//! 忠实面：`startupSnapshot` 守卫（vite 仅在 try 内调 `isBuildingSnapshot()`）。
//!
//! 偏差（记档，plan2 §4 的"v8 口径跳过"仍然有效）：堆统计
//! （`getHeapStatistics`/`getHeapSpaceStatistics`/`getHeapCodeStatistics`，
//! V8 口径数字在本引擎无意义）、`Serializer`/`Deserializer`（v8 序列化格式与
//! mozjs structuredClone 不互通）、coverage/`setFlagsFromString` 等一律不导出——
//! 用到即报"not a function" TypeError（与缺内建相比，至少 import 不炸）。

/// 内嵌 ESM 源（零 native，纯形）。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node lib/v8.js (startupSnapshot-only bridge; see module docs).
const startupSnapshot = {
  isBuildingSnapshot() {
    return false;
  },
  addSerializeCallback() {
    return undefined;
  },
  addDeserializeCallback() {
    return undefined;
  },
  setDeserializeMainFunction() {
    return undefined;
  },
};

export { startupSnapshot };
export default { startupSnapshot };
"#;
