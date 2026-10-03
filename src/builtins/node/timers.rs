//! `node:timers`（回调形态定时器；M5 vitest 牵引）。
//!
//! 忠实面：setTimeout/clearTimeout/setInterval/clearInterval（全局同语义，
//! 返回 Timeout 对象，unref/ref/hasRef/refresh 全）、setImmediate/
//! clearImmediate（`setTimeout(0)` 近似——本仓无 macrotask 分层，
//! `node:timers/promises` 同口径记档）+ `promises` 命名空间重导出。
//!
//! 偏差（记档）：`scheduler` 未导出（`node:timers/promises` 的 scheduler 面
//! 另看该模块）；`setImmediate` 非 check 阶段语义（时序近似）。
//! 10f 对拍：全局函数模块求值期捕获（套件 api-refs delete globalThis 后仍可用）。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// node:timers — callback timers over the global timer face (see module docs).
import __promisesDefault from 'node:timers/promises';
const promises = __promisesDefault;

// 全局函数在模块求值期捕获值（套件 api-refs：delete globalThis.setTimeout 等
// 之后 node:timers 面仍须可用——模块面不依赖全局可达，Node 同款口径）。
const gSetTimeout = globalThis.setTimeout;
const gClearTimeout = globalThis.clearTimeout;
const gSetInterval = globalThis.setInterval;
const gClearInterval = globalThis.clearInterval;
const gSetImmediate = globalThis.setImmediate;
const gClearImmediate = globalThis.clearImmediate;

function setTimeout(cb, ms, ...args) { return gSetTimeout(cb, ms, ...args); }
function clearTimeout(id) { return gClearTimeout(id); }
function setInterval(cb, ms, ...args) { return gSetInterval(cb, ms, ...args); }
function clearInterval(id) { return gClearInterval(id); }
function setImmediate(cb, ...args) { return gSetImmediate(cb, ...args); }
function clearImmediate(id) { return gClearImmediate(id); }

const __api = { setTimeout, clearTimeout, setInterval, clearInterval, setImmediate, clearImmediate, promises };
export default __api;
export { setTimeout, clearTimeout, setInterval, clearInterval, setImmediate, clearImmediate, promises };
"#;
