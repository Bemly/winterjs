//! `node:console`（Node `lib/console.js` 模块面，MIT；M5 vitest 牵引补齐）。
//!
//! 忠实面：具名导出全表（Console/assert/clear/count/countReset/debug/dir/
//! dirxml/error/group/groupCollapsed/groupEnd/info/log/table/time/timeEnd/
//! timeLog/trace/warn/context/createTask/profile/profileEnd/timeStamp）——
//! 无流方法直绑全局 console（同行为）；`Console` 类（`new Console(stdout[,
//! stderr][, ignoreErrors])` / `new Console({stdout, stderr})`，输出经
//! `node:util` 的 `format` 格式化后写流）。
//!
//! 偏差（记档）：
//! - `table` 只做 `format` 落盘（无列对齐渲染）；`profile/profileEnd/timeStamp`
//!   为无操作（真机同为 inspector 旁路，返回 undefined）；`createTask` 只回
//!   `{ run(f, ...a) }`（无 async_hooks 链路）；`context()` 回新 `Console`
//!   （真机回原生 console 上下文对象）；实例流校验宽松（无 `.write` 即回落
//!   全局 console，真机 TypeError）；实例 `dir` 不读 inspect options。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node lib/console.js (module face; see module docs for deviations).
import { format } from 'node:util';

const g = globalThis.console;
const bind = (name) => (...args) => g[name](...args);

class Console {
  constructor(stdout, stderr, ignoreErrors) {
    // Node 双形态：Console(stream[, stderr]) / Console({ stdout, stderr })。
    if (stdout !== null && typeof stdout === "object" && typeof stdout.write !== "function") {
      ignoreErrors = stderr;
      stderr = stdout.stderr;
      stdout = stdout.stdout;
    }
    this._stdout = stdout ?? null;
    this._stderr = stderr ?? null;
    this._ignoreErrors = ignoreErrors;
    this._counts = new Map();
    this._times = new Map();
  }
  _w(stream, text, fb) {
    if (stream !== null && typeof stream.write === "function") {
      try {
        stream.write(text + "\n");
      } catch {
        if (!this._ignoreErrors) throw new Error("Console: stream write failed");
      }
      return;
    }
    // 无可用流回落全局（真机 TypeError，宽松记档；回落方向与方法一致）。
    fb(text);
  }
  _out(...args) { const t = format(...args); this._w(this._stdout, t, (s) => g.log(s)); }
  _err(...args) { const t = format(...args); this._w(this._stderr, t, (s) => g.error(s)); }
  log(...args) { this._out(...args); }
  info(...args) { this._out(...args); }
  debug(...args) { this._out(...args); }
  warn(...args) { this._err(...args); }
  error(...args) { this._err(...args); }
  dir(obj) { this._out(obj); }
  dirxml(...args) { this._out(...args); }
  trace(...args) { this._err("Trace:", ...args); }
  table(data) { this._out(data); }
  assert(value, ...args) {
    if (!value) this._err("Assertion failed:", ...args);
  }
  count(label = "default") {
    const n = (this._counts.get(label) ?? 0) + 1;
    this._counts.set(label, n);
    this._out(`${label}: ${n}`);
  }
  countReset(label = "default") { this._counts.delete(label); }
  time(label = "default") { this._times.set(label, Date.now()); }
  timeLog(label = "default", ...args) {
    const t = this._times.get(label);
    if (t === undefined) this._err(`Timer '${label}' does not exist`);
    else this._out(`${label}: ${Date.now() - t}ms`, ...args);
  }
  timeEnd(label = "default") {
    const t = this._times.get(label);
    this._times.delete(label);
    if (t === undefined) this._err(`Timer '${label}' does not exist`);
    else this._out(`${label}: ${Date.now() - t}ms`);
  }
  group(...args) { if (args.length > 0) this._out(...args); }
  groupCollapsed(...args) { if (args.length > 0) this._out(...args); }
  groupEnd() {}
  clear() {}
  profile() {}
  profileEnd() {}
  timeStamp() {}
}

function context() { return new Console(); }
function createTask() { return { run(f, ...args) { return f(...args); } }; }
function profile() {}
function profileEnd() {}
function timeStamp() {}

const __api = {
  Console,
  assert: bind("assert"),
  clear: bind("clear"),
  context,
  count: bind("count"),
  countReset: bind("countReset"),
  createTask,
  debug: bind("debug"),
  dir: bind("dir"),
  dirxml: (...args) => g.dir(...args),
  error: bind("error"),
  group: bind("group"),
  groupCollapsed: (...args) => g.group(...args),
  groupEnd: bind("groupEnd"),
  info: bind("info"),
  log: bind("log"),
  profile,
  profileEnd,
  table: (...args) => g.log(...args),
  time: bind("time"),
  timeEnd: bind("timeEnd"),
  timeLog: bind("timeLog"),
  timeStamp,
  trace: bind("trace"),
  warn: bind("warn"),
};
export default __api;
export {
  Console,
  context,
  createTask,
  profile,
  profileEnd,
  timeStamp,
};
export const assert = __api.assert;
export const clear = __api.clear;
export const count = __api.count;
export const countReset = __api.countReset;
export const debug = __api.debug;
export const dir = __api.dir;
export const dirxml = __api.dirxml;
export const error = __api.error;
export const group = __api.group;
export const groupCollapsed = __api.groupCollapsed;
export const groupEnd = __api.groupEnd;
export const info = __api.info;
export const log = __api.log;
export const table = __api.table;
export const time = __api.time;
export const timeEnd = __api.timeEnd;
export const timeLog = __api.timeLog;
export const trace = __api.trace;
export const warn = __api.warn;
"#;
