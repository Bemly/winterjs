//! `node:test` 起步（纯 JS，无 natives）：`test/describe/it` + skip/todo/only +
//! `before/after/beforeEach/afterEach`（套件级，见下）+ 名过滤
//! （`WINTERJS_TEST_NAME_PATTERN`：子串或 `/re/flags`，不命中即 skip）。
//! 串行 microtask 泵，失败记数并置 `exitCode=1`，队空打印小结。
//! 口径（文档记录）：名过滤跳过测试本体与 Each 钩子，before/after 照跑（setup/teardown
//! 语义）；套件 `after` 在整轮末尾按深度由内向外跑（泵是全局串行，不精确跟踪套件闭合）；
//! hook 抛错：before 毒化其套件（后续该套件测试全 fail），Each 只 fail 当个，after 计 fail。
//! `t` 仅 `{name, skip}`。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
const __queue = [];
const __mkSuite = (name) => ({ name, hooks: { before: [], after: [], beforeEach: [], afterEach: [] }, beforeFired: false, poison: null, __depth: 0 });
const __suites = [__mkSuite("")];
const __suiteReg = [__suites[0]];
let __running = false;
let __ran = 0, __pass = 0, __fail = 0, __skip = 0, __todo = 0;
let __only = false;
const __SKIP = Symbol("skip");
// 名过滤（`winterjs test --test-name-pattern` 经 env 传入；子串或 /re/flags）。
const __namePat = (() => {
  try {
    const p = globalThis.process && globalThis.process.env && globalThis.process.env.WINTERJS_TEST_NAME_PATTERN;
    if (!p) return null;
    if (p.length > 1 && p.startsWith("/") && p.lastIndexOf("/") > 0) {
      return { re: new RegExp(p.slice(1, p.lastIndexOf("/")), p.slice(p.lastIndexOf("/") + 1)) };
    }
    return { sub: p };
  } catch { return null; }
})();
function __nameOk(full) {
  if (!__namePat) return true;
  if (__namePat.re) { try { return __namePat.re.test(full); } catch { return true; } }
  return full.includes(__namePat.sub);
}
function __fullName(name) { return [...__suites.map((s) => s.name).filter(Boolean), name].join(" > "); }
function __curSuite() { return __suites[__suites.length - 1]; }
function __enqueue(name, fn, mode) {
  if (mode === "only") __only = true;
  __queue.push({ name: __fullName(String(name)), fn, mode, suites: __suites.slice() });
  __pump();
}
function __failOne(t, e) {
  __fail++;
  globalThis.process.exitCode = 1;
  console.log(`not ok - ${t.name}`);
  console.log(String((e && e.stack) || (e && e.message) || e).split("\n").slice(0, 4).join("\n"));
}
async function __fireBefore(suite) {
  if (suite.beforeFired) return;
  suite.beforeFired = true;
  try {
    for (const h of suite.hooks.before) await h();
  } catch (e) {
    suite.poison = e;
  }
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
      if (!__nameOk(t.name)) { __skip++; continue; }
      __ran++;
      const ctx = { name: t.name, skip(msg) { throw { [__SKIP]: true, message: msg }; } };
      try {
        for (const s of t.suites) await __fireBefore(s);
        const poisoned = t.suites.find((s) => s.poison);
        if (poisoned) throw poisoned.poison;
        for (const s of t.suites) for (const h of s.hooks.beforeEach) await h(ctx);
        try {
          await t.fn(ctx);
        } finally {
          for (let i = t.suites.length - 1; i >= 0; i--) for (const h of t.suites[i].hooks.afterEach) await h(ctx);
        }
        __pass++;
      } catch (e) {
        if (e && e[__SKIP]) { __skip++; continue; }
        __failOne(t, e);
      }
    }
  }
  // 套件 after：整轮末尾由内向外（见头注口径）。
  const fired = [];
  const seen = new Set();
  for (const t of __allSuites()) {
    if (t.beforeFired && !seen.has(t)) { seen.add(t); fired.push(t); }
  }
  fired.sort((a, b) => b.__depth - a.__depth);
  for (const s of fired) {
    try {
      for (const h of s.hooks.after) await h();
    } catch (e) {
      __fail++;
      globalThis.process.exitCode = 1;
      console.log(`not ok - after hook: ${s.name || "<root>"}`);
      console.log(String((e && e.stack) || (e && e.message) || e).split("\n").slice(0, 4).join("\n"));
    }
  }
  __running = false;
  if (__ran + __skip + __todo > 0) {
    console.log(`# pass ${__pass}, fail ${__fail}, skip ${__skip}, todo ${__todo}`);
  }
}
function __allSuites() {
  // 嵌套套件在 describe 返回即弹栈，after 靠注册表找回（深度建时记死）。
  return __suiteReg.slice();
}
function __hook(kind, fn) {
  if (typeof fn !== "function") throw new TypeError(`${kind} needs a function`);
  __curSuite().hooks[kind].push(fn);
}
export function test(name, fn) {
  if (typeof name === "function") { fn = name; name = fn.name || "<anonymous>"; }
  __enqueue(name, fn || (() => {}), "run");
}
test.skip = (name, fn) => __enqueue(name, fn || (() => {}), "skip");
test.todo = (name, fn) => __enqueue(name, fn || (() => {}), "todo");
test.only = (name, fn) => __enqueue(name, fn || (() => {}), "only");
export function describe(name, fn) {
  if (typeof fn !== "function") throw new TypeError("describe needs a function");
  const suite = __mkSuite(String(name));
  suite.__depth = __suites.length;
  __suites.push(suite);
  __suiteReg.push(suite);
  try {
    fn();
  } finally {
    __suites.pop();
  }
  __pump();
}
describe.skip = () => {};
describe.before = (fn) => __hook("before", fn);
describe.after = (fn) => __hook("after", fn);
describe.beforeEach = (fn) => __hook("beforeEach", fn);
describe.afterEach = (fn) => __hook("afterEach", fn);
export function before(fn) { __hook("before", fn); }
export function after(fn) { __hook("after", fn); }
export function beforeEach(fn) { __hook("beforeEach", fn); }
export function afterEach(fn) { __hook("afterEach", fn); }
export const it = test;
it.before = (fn) => __hook("before", fn);
it.after = (fn) => __hook("after", fn);
it.beforeEach = (fn) => __hook("beforeEach", fn);
it.afterEach = (fn) => __hook("afterEach", fn);
export default { test, describe, it, before, after, beforeEach, afterEach };
"#;
