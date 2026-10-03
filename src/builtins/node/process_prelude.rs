//! 启动期全局 `process` JS（对齐 process_.rs；求值见 node/mod.rs `node_prelude`）。

/// 启动期全局 `process`（§0.9 三段切片经 `concat!(include_str!…)` 拼回，字节恒等）。
pub const PROCESS_PRELUDE: &str = concat!(
    include_str!("process_prelude_head.js"),
    include_str!("process_prelude_flags.js"),
    include_str!("process_prelude_tail.js"),
);

/// EventEmitter 原型链修正（test-process-prototype 口径；须在 `require` 可用后执行，
/// 由 `node_prelude()` 拼在 REQUIRE_PRELUDE 之后——prelude 主体求值时 require 尚无。
/// 真机形态：proto ≠ EE.prototype 本身但链上含之；constructor 为 proto 自有不可枚举槽）。
pub const PROCESS_PROTO_FIXUP: &str = r#"
{
  const EE = require("node:events").EventEmitter;
  const processProto = Object.create(EE.prototype);
  const ProcessCtor = function Process() {};
  ProcessCtor.prototype = processProto;
  Object.defineProperty(processProto, "constructor", { value: ProcessCtor, writable: true, enumerable: false, configurable: true });
  Object.setPrototypeOf(globalThis.process, processProto);
}
"#;
