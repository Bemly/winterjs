//! `node:test` 起步（纯 JS，无 natives）：`test/describe/it` + skip/todo/only，
//! 串行 microtask 泵，失败记数并置 `exitCode=1`，队空打印小结。
//! 偏差：`before/after` 钩子顺延；`t` 仅 `{name, skip}`（文档记录）。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
const __queue = [];
const __suites = [];
let __running = false;
let __ran = 0, __pass = 0, __fail = 0, __skip = 0, __todo = 0;
let __only = false;
const __SKIP = Symbol("skip");
function __fullName(name) { return [...__suites, name].join(" > "); }
function __enqueue(name, fn, mode) {
  if (mode === "only") __only = true;
  __queue.push({ name: __fullName(String(name)), fn, mode });
  __pump();
}
function __pump() {
  if (__running) return;
  __running = true;
  __next();
}
async function __next() {
  for (;;) {
    const batch = __queue.splice(0);
    if (!batch.length) break;
    const active = __only ? batch.filter((t) => t.mode === "only") : batch;
    __skip += batch.length - active.length;
    for (const t of active) {
      if (t.mode === "skip") { __skip++; continue; }
      if (t.mode === "todo") { __todo++; console.log(`todo - ${t.name}`); continue; }
      __ran++;
      const ctx = { name: t.name, skip(msg) { throw { [__SKIP]: true, message: msg }; } };
      try {
        await t.fn(ctx);
        __pass++;
      } catch (e) {
        if (e && e[__SKIP]) { __skip++; continue; }
        __fail++;
        globalThis.process.exitCode = 1;
        console.log(`not ok - ${t.name}`);
        console.log(String((e && e.stack) || (e && e.message) || e).split("\n").slice(0, 4).join("\n"));
      }
    }
  }
  __running = false;
  if (__ran + __skip + __todo > 0) {
    console.log(`# pass ${__pass}, fail ${__fail}, skip ${__skip}, todo ${__todo}`);
  }
}
export function test(name, fn) {
  if (typeof name === "function") { fn = name; name = fn.name || "<anonymous>"; }
  __enqueue(name, fn || (() => {}), "run");
}
test.skip = (name, fn) => __enqueue(name, fn || (() => {}), "skip");
test.todo = (name, fn) => __enqueue(name, fn || (() => {}), "todo");
test.only = (name, fn) => __enqueue(name, fn || (() => {}), "only");
export function describe(name, fn) {
  __suites.push(String(name));
  try {
    fn();
  } finally {
    __suites.pop();
  }
  __pump();
}
describe.skip = () => {};
export const it = test;
export default { test, describe, it };
"#;
