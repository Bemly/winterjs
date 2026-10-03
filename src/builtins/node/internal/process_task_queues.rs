//! `node:internal/process/task_queues`——`queueMicrotask` 直通（iter/classic 用）。
//!
//! 源：nodejs/node（MIT）对应 internal 件的最小对位实现；本仓微任务即引擎
//! 原生，无排队语义差，直通全局。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
const queueMicrotask = (...args) => globalThis.queueMicrotask(...args);
export { queueMicrotask };
export default { queueMicrotask };
"#;
