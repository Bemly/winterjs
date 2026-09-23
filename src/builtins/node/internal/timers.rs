//! `node:internal/timers`——kTimeout 符号（套件直引 `internal/timers`）。
//!
//! 源：nodejs/node（MIT）对应 internal 件的最小对位实现；`setTimeout` 本体在
//! 全局（`src/builtins/prelude/bootstrap.rs`，Timeout 类自带 `_idleTimeout`），
//! 此处只出符号。`Socket[kTimeout]` 接线见 `node/net_socket.js`（无计时 null，
//! 有计时即 timer 对象，timeout-on-connect 套件）。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// node 内部符号（真机 26.8.2 实测键）。
export const kTimeout = Symbol("kTimeout");
export const kRefed = Symbol("kRefed");
export const kHasPrimitive = Symbol("kHasPrimitive");
"#;
