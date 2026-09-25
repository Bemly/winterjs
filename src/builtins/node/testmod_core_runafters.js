// 套件 after：整轮末尾按深度由内向外（见头注口径）。
async function __runAfters() {
  const fired = [];
  const seen = new Set();
  for (const t of __suiteReg.slice()) {
    if ((t.beforeFired || t._ran) && !seen.has(t)) { seen.add(t); fired.push(t); }
  }
  fired.sort((a, b) => b.depth - a.depth);
  for (const s of fired) {
    const sc = __suiteCtx(s);
    __ctxStack.push(sc);
    try {
      for (const h of s.hooks.after) await h.call(sc, sc);
    } catch (e) {
      __fail++;
      globalThis.process.exitCode = 1;
      console.log(`not ok - after hook: ${s.name || "<root>"}`);
      console.log(String((e && e.stack) || (e && e.message) || e).split("\n").slice(0, 4).join("\n"));
    } finally {
      __ctxStack.pop();
    }
  }
}
function __subtest(parentRec, args) {
  const { name, options, fn } = __normCall(args);
  __validateTestOptions(options);
  // 子测试计入父 plan（真机 TestContext.test 口径）。
  if (parentRec.plan !== null) parentRec.plan.count();
  // 子测试文件归属取调用点（helper 内 t.test 口径）：先进 __mkTest 再改
  // 已经迟了（enqueue 先发），故暂换 __currentFile 再建。
  let savedFile = null;
  try {
    const caller = __callerFile();
    if (caller) {
      savedFile = __currentFile;
      __currentFile = caller;
    }
  } catch {}
  const rec = __mkTest(name, options, fn, parentRec.suites, parentRec);
  if (savedFile !== null) __currentFile = savedFile;
  parentRec.children.push(rec);
  if (__randomSeed !== null && __randomSeed !== undefined) {
    // 随机序：收齐同轮兄弟，父 fn 收尾后按种子序跑（真机 pending 队列口径）。
    // 注意：父内 await t.test 会死锁（子等父收尾），套件无此形状。
    return new Promise((resolve) => {
      parentRec.deferred.push({ rec, resolve });
    });
  }
  const p = __runOne(rec).then(() => {
    if (rec.failed) parentRec.childFailed = true;
  });
  parentRec.pending.push(p);
  return p;
}
function __hook(kind, fn) {
  __checkHookFn(kind, fn);
  __suites[__suites.length - 1].hooks[kind].push(fn);
}
export function test(...args) {
  const { name, options, fn } = __normCall(args);
  __validateTestOptions(options);
  const rec = __mkTest(name, options, fn, __suites.slice(), null);
  __queue.push(rec);
  __pump();
  return rec.done;
}
test.skip = (...args) => {
  const { name, options, fn } = __normCall(args);
  __validateTestOptions(options);
  const rec = __mkTest(name, { ...options, skip: true }, fn, __suites.slice(), null);
  __queue.push(rec);
  __pump();
  return rec.done;
};
test.todo = (...args) => {
  const { name, options, fn } = __normCall(args);
  __validateTestOptions(options);
  const rec = __mkTest(name, { ...options, todo: true }, fn, __suites.slice(), null);
  __queue.push(rec);
  __pump();
  return rec.done;
};
test.only = (...args) => {
  const { name, options, fn } = __normCall(args);
  __validateTestOptions(options);
  const rec = __mkTest(name, { ...options, only: true }, fn, __suites.slice(), null);
  __queue.push(rec);
  __pump();
  return rec.done;
};
export function describe(...args) {
  const { name, options, fn } = __normCall(args);
  __validateTestOptions(options);
  const ownTags = options.tags !== undefined ? __canonTags(options.tags, "options.tags") : __EMPTY_TAGS;
  const parent = __suites[__suites.length - 1];
  const suite = __mkSuite(name, ownTags, parent);
  suite.skip = !!options.skip;
  suite.todo = !options.skip && !!options.todo;
  // 建套件即 kick 父级 before（先于本回调；runOnce 去重）。
  if (parent) __kickBefore(parent);
  if (options.only === true) {
    suite.runOnlySubtests = true;
    let s = suite;
    while (s) {
      s.hasOnlyTests = true;
      s = s.parent;
    }
  }
  __suites.push(suite);
  __suiteReg.push(suite);
  // skip 套件不跑回调（真机：跳过即不构建；todo-skip 套件靠此不触发 mustNotCall）。
  if (!options.skip) {
    const sc = __suiteCtx(suite);
    __ctxStack.push(sc);
    try {
      if (typeof fn === "function") fn.call(sc, sc);
    } finally {
      __ctxStack.pop();
      __suites.pop();
    }
  } else {
    __suites.pop();
  }
  __pump();
  return Promise.resolve();
}
describe.skip = () => {};
export const suite = describe;
suite.skip = () => {};
export function before(fn) { __hook("before", fn); }
export function after(fn) { __hook("after", fn); }
export function beforeEach(fn) { __hook("beforeEach", fn); }
export function afterEach(fn) { __hook("afterEach", fn); }
export function getTestContext() {
  if (__ctxStack.length === 0) return undefined;
  return __ctxStack[__ctxStack.length - 1];
}
export const it = test;
it.skip = test.skip;
it.todo = test.todo;
it.only = test.only;
// require('node:test') 口径（真机 26.8.2 实测）：module.exports = test 本体
// （可调用，属性挂 test/describe/it/suite/skip/todo/only/Each 钩）——default 即 test。
test.test = test;
test.describe = describe;
test.suite = suite;
test.it = it;
test.skip = test.skip;
test.todo = test.todo;
test.only = test.only;
test.before = (fn) => __hook("before", fn);
test.after = (fn) => __hook("after", fn);
test.beforeEach = (fn) => __hook("beforeEach", fn);
test.afterEach = (fn) => __hook("afterEach", fn);
// CJS require('node:test') 取默认导出本体，具名导出经此挂载才可见。
test.getTestContext = getTestContext;
test.assert = testAssert;
test.mock = topMock;
test.run = run;
export { testAssert as assert };
export { topMock as mock };
export default test;
