//! `node:timers`（回调形态定时器；M5 vitest 牵引）。
//!
//! 忠实面：setTimeout/clearTimeout/setInterval/clearInterval（全局同语义，
//! 返回 Timeout 对象，unref/ref/hasRef/refresh 全）、setImmediate/
//! clearImmediate（`setTimeout(0)` 近似——本仓无 macrotask 分层，
//! `node:timers/promises` 同口径记档）+ `promises` 命名空间重导出。
//!
//! 偏差（记档）：`scheduler` 未导出（`node:timers/promises` 的 scheduler 面
//! 另看该模块）；`setImmediate` 非 check 阶段语义（时序近似）。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// node:timers — callback timers over the global timer face (see module docs).
import * as promises from 'node:timers/promises';

function setTimeout(cb, ms, ...args) { return globalThis.setTimeout(cb, ms, ...args); }
function clearTimeout(id) { return globalThis.clearTimeout(id); }
function setInterval(cb, ms, ...args) { return globalThis.setInterval(cb, ms, ...args); }
function clearInterval(id) { return globalThis.clearInterval(id); }
function setImmediate(cb, ...args) { return globalThis.setTimeout(cb, 0, ...args); }
function clearImmediate(id) { return globalThis.clearTimeout(id); }

const __api = { setTimeout, clearTimeout, setInterval, clearInterval, setImmediate, clearImmediate, promises };
export default __api;
export { setTimeout, clearTimeout, setInterval, clearInterval, setImmediate, clearImmediate, promises };
"#;
