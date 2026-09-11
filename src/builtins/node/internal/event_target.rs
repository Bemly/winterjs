//! `node:internal/event_target`（Node `lib/internal/event_target.js` 的 events 所需面，MIT）。
//!
//! 本仓无全局 EventTarget 基类（AbortSignal 为极简自有实现），`isEventTarget`
//! 以鸭子类型近似（有 addEventListener 且无 `_events` → EventTarget，如
//! AbortSignal）；`kEvents`/`kResistStopPropagation` 仅出符号（Web 内建事件面
//! 未按 Node internal 结构存储，`getEventListeners(target)` 对 EventTarget 只给
//! 空数组——与 util 所需无交，偏差记录）。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Subset of node:internal/event_target needed by node:events (see module docs).
const kEvents = Symbol('kEvents');
const kResistStopPropagation = Symbol.for('resistStopPropagation');
const kWatermarkData = Symbol.for('nodejs.watermarkData');

function isEventTarget(obj) {
  if (obj === null || (typeof obj !== 'object' && typeof obj !== 'function')) return false;
  return typeof obj.addEventListener === 'function' &&
    typeof obj.removeEventListener === 'function' &&
    obj._events === undefined;
}

function isNodeEventTarget(obj) {
  return false; // 本仓无 NodeEventTarget 子类（net/stream 未接）
}

export { kEvents, kResistStopPropagation, kWatermarkData, isEventTarget, isNodeEventTarget };
export default { kEvents, kResistStopPropagation, kWatermarkData, isEventTarget, isNodeEventTarget };
"#;
