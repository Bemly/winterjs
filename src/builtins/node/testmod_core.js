import assertMod from "node:assert";
import validators from "node:internal/validators";
import { codes } from "node:internal/errors";
import { readFileSync, readdirSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { resolve as resolvePath } from "node:path";
import { MockTracker } from "node:internal/test/mock";
import { Worker } from "node:worker_threads";

const { validateArray, validateBoolean, validateFunction, validateInteger, validateNumber, validateObject, validateString, validateUint32 } = validators;
// node internal/timers TIMEOUT_MAX（2**31 - 1）同值。
const TIMEOUT_MAX = 2147483647;
const __UNCOPIED = new Set(["AssertionError", "strict", "Assert", "options"]);
const __EMPTY_TAGS = Object.freeze([]);

// 模块级自定义断言（node `assert` 具名导出 + `register`，真机 26 口径）。
const __customAsserts = new Map();
const testAssert = {
  register(name, fn) {
    validateString(name, "name");
    validateFunction(fn, "fn");
    __customAsserts.set(name, fn);
  },
};

// 标签校验/规范（node tag_filter 口径：非空串、无空白与算子字符、非保留词；
// 小写规范 + 首见序去重；空数组回共享冻结空表且不发警告；非空发一次性实验警告）。
const __FORBIDDEN = new Set([0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x20, 0x26, 0x21, 0x28, 0x7C, 0x29, 0x2A]);
const __RESERVED = new Set(["and", "or", "not"]);
let __tagsWarned = false;
function __canonTags(tags, optName) {
  validateArray(tags, optName);
  if (tags.length === 0) return __EMPTY_TAGS;
  const seen = new Set();
  const out = [];
  for (let i = 0; i < tags.length; i++) {
    const tag = tags[i];
    const elem = `${optName}[${i}]`;
    validateString(tag, elem);
    if (tag.length === 0) throw new codes.ERR_INVALID_ARG_VALUE(elem, tag, "must not be empty");
    for (let j = 0; j < tag.length; j++) {
      if (__FORBIDDEN.has(tag.charCodeAt(j))) {
        throw new codes.ERR_INVALID_ARG_VALUE(elem, tag,
          "contains a forbidden character. Tags must not contain whitespace, " +
          "operator characters (&, |, !, (, ), *), or be the reserved words " +
          String.fromCharCode(39) + "and" + String.fromCharCode(39) + ", " +
          String.fromCharCode(39) + "or" + String.fromCharCode(39) + ", or " +
          String.fromCharCode(39) + "not" + String.fromCharCode(39));
      }
    }
    const lower = tag.toLowerCase();
    if (__RESERVED.has(lower)) {
      throw new codes.ERR_INVALID_ARG_VALUE(elem, tag,
        "must not be the reserved word " +
        String.fromCharCode(39) + "and" + String.fromCharCode(39) + ", " +
        String.fromCharCode(39) + "or" + String.fromCharCode(39) + ", or " +
        String.fromCharCode(39) + "not" + String.fromCharCode(39));
    }
    if (!seen.has(lower)) { seen.add(lower); out.push(lower); }
  }
  if (!__tagsWarned) {
    __tagsWarned = true;
    process.emitWarning("Test tags is an experimental feature and might change at any time", "ExperimentalWarning");
  }
  return Object.freeze(out);
}

// 超时/并发校验（option-validation 套件口径：码逐字）。
function __checkTimeout(timeout) {
  if (timeout == null || timeout === Infinity) return;
  validateNumber(timeout, "options.timeout", 0, TIMEOUT_MAX);
}
function __checkConcurrency(concurrency) {
  if (concurrency == null) return;
  if (typeof concurrency === "boolean") return;
  if (typeof concurrency === "number") {
    validateUint32(concurrency, "options.concurrency", true);
    return;
  }
  throw new codes.ERR_INVALID_ARG_TYPE("options.concurrency", ["boolean", "number"], concurrency);
}
function __validateTestOptions(options) {
  __checkTimeout(options.timeout);
  __checkConcurrency(options.concurrency);
  if (options.expectFailure !== undefined) __validateExpectFailure(options.expectFailure);
}
// expectFailure 校验（本轮仅空对象门逐字 + true 生效；matcher 全家另轮）。
function __validateExpectFailure(v) {
  if (v === undefined || v === true || v === false) return;
  if (typeof v === "string" || typeof v === "function") return;
  if (v instanceof RegExp) return;
  if (v !== null && typeof v === "object") {
    if (Object.keys(v).length === 0) {
      throw new codes.ERR_INVALID_ARG_VALUE("options.expectFailure", v, "must not be empty");
    }
    return;
  }
}

// 参数归一（node createSubtest 口径）：(fn)/(options[, fn])/(name[, options][, fn])；
// options.name/options.fn 覆盖位置参数；无名回 fn 名或 <anonymous>。
function __normCall(args) {
  let name, options, fn;
  const a0 = args[0], a1 = args[1], a2 = args[2];
  if (typeof a0 === "function") {
    fn = a0;
  } else if (a0 !== null && typeof a0 === "object") {
    options = a0;
    fn = a1;
  } else {
    name = a0;
    if (typeof a1 === "function") {
      fn = a1;
    } else {
      if (a1 !== null && typeof a1 === "object") options = a1;
      fn = a2;
    }
  }
  if (options == null || typeof options !== "object") options = {};
  if (options.name !== undefined) name = options.name;
  if (options.fn !== undefined) fn = options.fn;
  if (typeof name !== "string" || name === "") name = (fn && fn.name) || "<anonymous>";
  return { name, options, fn };
}

// 套件树与队列。
function __mkSuite(name, ownTags, parent) {
  return {
    name, ownTags, parent,
    // 根判定走位不走名（run 内层另建 innerRoot 与外层同名，见 todo-skip 双跑案）。
    isRoot: parent === null,
    hooks: { before: [], after: [], beforeEach: [], afterEach: [] },
    beforeFired: false, poison: null, depth: parent ? parent.depth + 1 : 0,
    // node only-过滤三件（applyFilters 口径）+ run() 套件计数。
    runOnlySubtests: false, hasOnlyTests: false,
    testId: ++__testIdCounter,
    _pass: 0, _fail: 0,
  };
}
// 根套件名即 <root>（真机 globalRoot 口径；fullName 链跳过它，见下）。
// 计数器前置（__mkSuite 在模块求值期即调用，TDZ 敏感）。
let __testIdCounter = 0;
const __rootSuite = __mkSuite("<root>", __EMPTY_TAGS, null);
const __suites = [__rootSuite];
const __suiteReg = [__rootSuite];
const __queue = [];
let __running = false;
let __ran = 0, __pass = 0, __fail = 0, __skip = 0, __todo = 0;
// run() 事件槽（常态 null；run(none) 执行期暂存 stream；注册/执行点直发）。
let __eventSink = null;
// run(none) 载入中的文件（注册测试时记 file 归属；常态 null）。
let __currentFile = null;
function __emit(type, data) {
  if (__eventSink) {
    try {
      __eventSink._emit(type, data);
    } catch {}
  }
}
// 当前上下文栈（串行泵：测试/钩子/suite 回调执行期压栈，跨 setImmediate 有效）。
const __ctxStack = [];
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
function __suiteFullName(suite) {
  const parts = [];
  for (let s = suite; s !== null && !s.isRoot; s = s.parent) {
    if (s.name) parts.unshift(s.name);
  }
  if (parts.length === 0) return "<root>";
  return parts.join(" > ");
}
function __flatTags(suites, own) {
  const seen = new Set();
  const out = [];
  for (const s of suites) {
    for (const t of s.ownTags) {
      if (!seen.has(t)) { seen.add(t); out.push(t); }
    }
  }
  for (const t of own) {
    if (!seen.has(t)) { seen.add(t); out.push(t); }
  }
  return Object.freeze(out);
}
function __freshSignal() {
  try {
    return new AbortController().signal;
  } catch {
    return undefined;
  }
}
function __suiteCtx(suite) {
  return {
    name: suite.name,
    fullName: __suiteFullName(suite),
    signal: __freshSignal(),
    tags: __flatTags(__suiteChain(suite), suite.ownTags),
    passed: false,
    attempt: 0,
    diagnostic(msg) { console.log(`# ${String(msg)}`); },
  };
}
function __suiteChain(suite) {
  const chain = [];
  for (let s = suite.parent; s !== null; s = s.parent) chain.unshift(s);
  return chain;
}

// t.assert 构造（node TestContext 口径）：全 assert 键（除 4 不拷贝）+
// snapshot/fileSnapshot 桩 + register 自定义（含覆盖）；调用计数进 plan；
// 自定义断言的 this 即 ctx；ok 失败补调用点源码行。
function __buildAssert(ctx, rec) {
  const a = {};
  for (const k of Object.keys(assertMod)) {
    if (__UNCOPIED.has(k)) continue;
    const base = assertMod[k];
    if (typeof base !== "function") continue;
    if (__customAsserts.has(k)) {
      const impl = __customAsserts.get(k);
      a[k] = function (...args) { rec.planActual++; return Reflect.apply(impl, ctx, args); };
    } else if (k === "ok") {
      a[k] = function (...args) { rec.planActual++; return __okWithSource(base, args); };
    } else {
      a[k] = function (...args) { rec.planActual++; return Reflect.apply(base, ctx, args); };
    }
  }
  for (const [k, fn] of __customAsserts) {
    if (!(k in a)) {
      a[k] = function (...args) { rec.planActual++; return Reflect.apply(fn, ctx, args); };
    }
  }
  if (!("snapshot" in a)) {
    a.snapshot = function () { throw new Error("t.assert.snapshot() is not supported"); };
  }
  if (!("fileSnapshot" in a)) {
    a.fileSnapshot = function () { throw new Error("t.assert.fileSnapshot() is not supported"); };
  }
  return a;
}
// 调用点源码行（SM 栈 `fn@file:line:col`；跳过 node:test 自身帧）。
function __callerLine() {
  let stack = "";
  try { stack = String(new Error().stack || ""); } catch { return null; }
  const lines = stack.split("\n");
  for (const line of lines) {
    const m = /@([^@\s]+):(\d+):(\d+)\s*$/.exec(line);
    if (!m) continue;
    let file = m[1];
    if (file === "node:test" || file.endsWith("/node:test")) continue;
    const lineNo = Number(m[2]);
    if (file.startsWith("file://")) {
      try { file = decodeURIComponent(file.slice(7)); } catch {}
    } else if (!file.startsWith("/")) {
      continue;
    }
    try {
      const text = readFileSync(file, "utf8");
      const all = text.split("\n");
      // CJS 包装头（require.rs 五连柯里化）在源码前垫一行，栈行号整体 +1；
      // ESM 则精确。窗口向上回扫，首个含 ok( 的行即调用点。
      for (let ln = lineNo; ln >= Math.max(1, lineNo - 5); ln--) {
        const cand = all[ln - 1];
        if (typeof cand === "string" && cand.includes("ok(")) {
          const t = cand.trim();
          if (t) return t.slice(0, 500);
        }
      }
      const src = all[lineNo - 1];
      if (typeof src === "string" && src.trim()) return src.trim().slice(0, 500);
    } catch {}
    return null;
  }
  return null;
}
function __okWithSource(base, args) {
  if (args.length > 1) return Reflect.apply(base, undefined, args);
  const src = __callerLine();
  try {
    return Reflect.apply(base, undefined, args);
  } catch (e) {
    if (src && e && typeof e.message === "string" && !e.message.includes(src)) {
      try { e.message = `The expression evaluated to a falsy value:\n\n  ${src}\n`; } catch {}
    }
    throw e;
  }
}

// t.waitFor（node TestContext 口径：校验同步抛 + 串行轮询 + 超时 cause）。
// 注意：校验必须在同步段执行——async 函数内抛即变 rejection，同步
// `t.assert.throws` 够不着（wait-for 套件 input validation 现形）。
function __waitFor(condition, options) {
  validateFunction(condition, "condition");
  if (options === undefined) options = {};
  validateObject(options, "options");
  const interval = options.interval ?? 50;
  const timeout = options.timeout ?? 1000;
  validateNumber(interval, "options.interval", 0, TIMEOUT_MAX);
  validateNumber(timeout, "options.timeout", 0, TIMEOUT_MAX);
  return __waitForRun(condition, interval, timeout);
}
async function __waitForRun(condition, interval, timeout) {
  const deadline = Date.now() + timeout;
  let cause;
  let hasCause = false;
  for (;;) {
    const remaining = deadline - Date.now();
    let settled = false, ok = false, value, err;
    const attempt = (async () => {
      try {
        value = await condition();
        ok = true;
      } catch (e) {
        err = e;
      }
      settled = true;
    })();
    // 输掉竞速的 sleep 必须清掉——60s 残留 timer 会给事件循环续命（真机同为
    // cancel 语义；polls/limits 套件 60s timeout 现形）。
    let timer = null;
    const sleep = new Promise((r) => { timer = setTimeout(() => r(false), Math.max(remaining, 0)); });
    const did = await Promise.race([attempt.then(() => true), sleep]);
    try { clearTimeout(timer); } catch {}
    if (did === true && ok) return value;
    if (settled) { cause = err; hasCause = true; }
    if (Date.now() >= deadline) break;
    const wait = Math.min(interval, Math.max(deadline - Date.now(), 0));
    if (wait > 0) await new Promise((r) => setTimeout(r, wait));
    else break;
  }
  const e = new Error("waitFor() timed out");
  if (hasCause) e.cause = cause;
  throw e;
}

// t.mock（真机 MockTracker 逐测试实例；顶层 mock 为进程级实例）。
// Slice B1：fn/method/getter/setter/property/reset/restore 全家；timers 另片。
function __mkMock() {
  return new MockTracker();
}
const topMock = new MockTracker();

function __testCtx(rec) {
  if (rec.ctx) return rec.ctx;
  const ctx = {
    name: rec.name,
    fullName: rec.fullName,
    signal: rec.signal,
    tags: rec.tags,
    passed: false,
    attempt: 0,
    diagnostic(msg) { console.log(`# ${String(msg)}`); },
    log(msg) { console.log(String(msg)); },
    // 置旗语义（真机：body 继续执行；终局 skip 优先；message 回显到事件）。
    skip(msg) {
      rec.skipped = true;
      if (msg !== undefined) rec.skipMessage = String(msg);
    },
    todo(msg) {
      rec.isTodo = true;
      if (msg !== undefined) rec.todoMessage = String(msg);
    },
    plan(count, options) {
      if (rec.planExpected !== null) {
        const e = new Error("cannot set plan more than once");
        e.code = "ERR_TEST_FAILURE";
        throw e;
      }
      validateUint32(count, "count");
      if (options !== undefined) validateObject(options, "options");
      // plan.mjs 套件口径：wait 仅 boolean/number（数则进范围门）。
      if (options != null && options.wait !== undefined) {
        if (typeof options.wait !== "boolean" && typeof options.wait !== "number") {
          throw new codes.ERR_INVALID_ARG_TYPE("options.wait", ["boolean", "number"], options.wait);
        }
        if (typeof options.wait === "number") {
          validateNumber(options.wait, "options.wait", 0, TIMEOUT_MAX);
        }
      }
      rec.planExpected = count;
    },
    get assert() {
      if (!rec.assertObj) rec.assertObj = __buildAssert(ctx, rec);
      return rec.assertObj;
    },
    get mock() {
      if (!rec.mockObj) rec.mockObj = __mkMock();
      return rec.mockObj;
    },
    test(...args) { return __subtest(rec, args); },
    before(fn) { __checkHookFn("before", fn); rec.testHooks.before.push(fn); },
    after(fn) { __checkHookFn("after", fn); rec.testHooks.after.push(fn); },
    beforeEach(fn) { __checkHookFn("beforeEach", fn); rec.testHooks.beforeEach.push(fn); },
    afterEach(fn) { __checkHookFn("afterEach", fn); rec.testHooks.afterEach.push(fn); },
    waitFor(condition, options) { return __waitFor(condition, options); },
  };
  rec.ctx = ctx;
  return ctx;
}
function __checkHookFn(kind, fn) {
  if (typeof fn !== "function") throw new TypeError(`${kind} needs a function`);
}

function __mkTest(name, options, fn, suites, parent) {
  const ownTags = options.tags !== undefined ? __canonTags(options.tags, "options.tags") : __EMPTY_TAGS;
  const baseTags = parent ? parent.tags : __flatTags(suites, __EMPTY_TAGS);
  const seen = new Set(baseTags);
  const tags = baseTags.slice();
  for (const t of ownTags) {
    if (!seen.has(t)) { seen.add(t); tags.push(t); }
  }
  let resolve = null;
  const done = new Promise((r) => { resolve = r; });
  const rec = {
    name,
    fullName: parent ? `${parent.fullName} > ${name}` : [...suites.filter((s) => !s.isRoot).map((s) => s.name).filter(Boolean), name].join(" > "),
    // 显式 only:false 即 noop 空转（真机 createSubtest 口径）。
    fn: (typeof fn === "function" && options.only !== false) ? fn : undefined,
    mode: options.only === true ? "only" : (options.skip ? "skip" : (options.todo ? "todo" : "run")),
    skipMessage: typeof options.skip === "string" ? options.skip : undefined,
    todoMessage: typeof options.todo === "string" ? options.todo : undefined,
    skipped: false,
    isTodo: !!options.todo,
    suites, parent: parent || null,
    tags: Object.freeze(tags),
    signal: __freshSignal(),
    testId: ++__testIdCounter,
    file: __currentFile,
    nesting: suites.length,
    testHooks: { before: [], after: [], beforeEach: [], afterEach: [] },
    children: [], pending: [], childFailed: false,
    planExpected: null, planActual: 0,
    assertObj: null, mockObj: null, ctx: null,
    passed: false, failed: false, beforeFired: false,
    onlyFlag: options.only === true,
    expectFailure: options.expectFailure,
    hasOnlyTests: false,
    runOnlySubtests: false,
    done, resolve,
  };
  if (rec.onlyFlag) __markOnly(rec);
  if (__eventSink) {
    // name 取短名（test-id 套件按短名找 e2e；全名另有 fullName 键）。
    const data = { name: rec.name, fullName: rec.fullName, testId: rec.testId, nesting: rec.nesting };
    if (rec.file != null) data.file = rec.file;
    __emit("test:enqueue", data);
  }
  return rec;
}

function __failOne(rec, e) {
  rec.failed = true;
  __fail++;
  globalThis.process.exitCode = 1;
  console.log(`not ok - ${rec.fullName}`);
  // message 在前（stack 头是内部帧）；stack 只挑 @ 帧行补定位。
  const msg = e && e.message !== undefined ? String(e.message) : String(e);
  console.log(msg.split("\n").slice(0, 4).join("\n"));
  if (e && e.stack) {
    const frames = String(e.stack).split("\n").filter((l) => l.includes("@")).slice(0, 3).join("\n");
    if (frames) console.log(frames);
  }
}
// before 钩子（runOnce）：describe 建套件时即 kick（真机 Suite 构建序——
// parent befores 先于 suite 回调，探针 order-probe），测试起跑时 await。
// 回调同步跑，不等异步 before 落定（node 同款）。
// before 钩子逐个 runOnce（真机 TestHook 口径）：每次 kick 只跑新增未跑项——
// 后注册的钩子（如后载入文件的根 before）在下一次 kick 时补跑；一律串行。
// 首 kick 内联起跑：同步钩子在 describe() 返回前即落定（BEFORE-HOOK 先于
// SUITE-CB，真机序）；遇首个异步即挂链，回调不等（node 同款）。
function __kickBefore(suite) {
  if (!suite.beforeRan) suite.beforeRan = new Set();
  const pending = suite.hooks.before.filter((h) => !suite.beforeRan.has(h));
  for (const h of pending) suite.beforeRan.add(h);
  if (pending.length === 0) return suite.beforePromise ?? Promise.resolve();
  const sc = __suiteCtx(suite);
  const runList = async (list) => {
    __ctxStack.push(sc);
    try {
      for (const h of list) {
        try {
          await h.call(sc, sc);
        } catch (e) {
          if (!suite.poison) suite.poison = e;
        }
      }
    } finally {
      __ctxStack.pop();
    }
  };
  // 落定清槽：链空后槽位清空，下一次 kick 重新内联起跑（同步钩子不吃
  // 一轮 microtask）；全同步则根本不占槽。
  const track = (cur) => {
    cur.then(() => {
      if (suite.beforePromise === cur) suite.beforePromise = undefined;
    });
    return cur;
  };
  if (!suite.beforePromise) {
    // 空闲：同步前缀内联执行（BEFORE-HOOK 先于 SUITE-CB，真机序）。
    let idx = 0;
    let tail = null;
    __ctxStack.push(sc);
    try {
      while (idx < pending.length) {
        let r;
        try {
          r = pending[idx].call(sc, sc);
        } catch (e) {
          if (!suite.poison) suite.poison = e;
          idx++;
          continue;
        }
        if (r && typeof r.then === "function") {
          const rest = pending.slice(idx + 1);
          tail = r.then(
            () => runList(rest),
            (e) => {
              if (!suite.poison) suite.poison = e;
              return runList(rest);
            },
          );
          break;
        }
        idx++;
      }
    } finally {
      __ctxStack.pop();
    }
    if (tail) {
      suite.beforePromise = tail;
      track(tail);
      suite.beforeFired = true;
      return tail;
    }
    suite.beforeFired = true;
    return Promise.resolve();
  }
  const cur = suite.beforePromise.then(() => runList(pending));
  suite.beforePromise = cur;
  track(cur);
  suite.beforeFired = true;
  return cur;
}
async function __fireBefore(suite) {
  await __kickBefore(suite);
}
// 测试级 owner before 同理（逐钩 runOnce；失败抛给当前子）。
async function __kickTestBefore(owner) {
  if (!owner.beforeRan) owner.beforeRan = new Set();
  const pending = owner.testHooks.before.filter((h) => !owner.beforeRan.has(h));
  if (pending.length === 0) return;
  const pctx = __testCtx(owner);
  __ctxStack.push(pctx);
  try {
    for (const h of pending) {
      if (owner.beforeRan.has(h)) continue;
      owner.beforeRan.add(h);
      await h.call(pctx, pctx);
    }
  } finally {
    __ctxStack.pop();
  }
}
// 套件钩子跑在套件上下文（getTestContext 见套件名），参数传当前测试 ctx。
async function __runSuiteHook(suite, fn, testCtx) {
  const sc = __suiteCtx(suite);
  __ctxStack.push(sc);
  try {
    await fn.call(testCtx, testCtx);
  } finally {
    __ctxStack.pop();
  }
}
// 测试级钩子跑在 owner 上下文（getTestContext 见 owner 名），参数传子测试
// ctx（get-test-context 测试级钩子套件钉住；套件级同理见 __runSuiteHook）。
async function __runTestHook(fn, ownerCtx, argCtx) {
  __ctxStack.push(ownerCtx);
  try {
    await fn.call(argCtx, argCtx);
  } finally {
    __ctxStack.pop();
  }
}
// node only-过滤（applyFilters 口径）：only 测试标记直接 owner 的
// runOnlySubtests，并沿祖先链置 hasOnlyTests；执行期不过滤门即静默跳过。
function __markOnly(rec) {
  const owner = rec.parent || rec.suites[rec.suites.length - 1];
  if (owner) owner.runOnlySubtests = true;
  const seen = new Set();
  let s = owner;
  while (s && !seen.has(s)) {
    seen.add(s);
    s.hasOnlyTests = true;
    s = s.parent || null;
  }
  for (const suite of rec.suites) {
    if (!seen.has(suite)) {
      seen.add(suite);
      suite.hasOnlyTests = true;
    }
  }
}
function __onlyFiltered(rec) {
  if (rec.onlyFlag || rec.hasOnlyTests) return false;
  const owner = rec.parent || rec.suites[rec.suites.length - 1];
  if (!owner) return false;
  return !!(owner.runOnlySubtests || owner.hasOnlyTests);
}
// pass 事件发射（skip/todo/expectFailure 互斥出现，真机口径：仅真值键在场）。
function __emitPass(rec, flags) {
  if (!__eventSink) return;
  const data = __baseEvent(rec);
  if (flags) {
    for (const k of Object.keys(flags)) data[k] = flags[k];
  }
  __emit("test:pass", data);
  __emit("test:complete", __baseEvent(rec));
}
// run() 事件数据（name 短名 + testId  pairing，test-id 套件口径）。
function __baseEvent(rec) {
  const data = { name: rec.name, fullName: rec.fullName, testId: rec.testId, nesting: rec.nesting, tags: rec.tags };
  if (rec.file != null) data.file = rec.file;
  return data;
}
async function __runOne(rec) {
  try {
    if (rec.mode === "skip") {
      __skip++;
      __emitPass(rec, { skip: rec.skipMessage ?? true });
      return;
    }
    if (!__nameOk(rec.fullName)) { __skip++; return; }
    // only-过滤（applyFilters 口径）：门外即静默跳过，无事件。
    if (__onlyFiltered(rec)) { __skip++; return; }
    for (const s of rec.suites) s._ran = true;
    __ran++;
    if (__eventSink) {
      __emit("test:dequeue", __baseEvent(rec));
      __emit("test:start", __baseEvent(rec));
    }
    const ctx = __testCtx(rec);
    __ctxStack.push(ctx);
    try {
      // before（runOnce）：套件由外向内；测试级 owner 在首个子测试时跑一次。
      // beforeEach/afterEach 跑在子测试身上（带子 ctx），owner 自身的不为自己跑
      // （真机 Test.run 口径：`this.parent.hooks.*` + 自身 after；探针 hook.cjs）。
      for (const s of rec.suites) await __fireBefore(s);
      const poisoned = rec.suites.find((s) => s.poison);
      if (poisoned) throw poisoned.poison;
      if (rec.parent) await __kickTestBefore(rec.parent);
      // beforeEach：套件由外向内，再是测试级 owner 的（注册序）。
      for (const s of rec.suites) for (const h of s.hooks.beforeEach) await __runSuiteHook(s, h, ctx);
      if (rec.parent) {
        const pctx = __testCtx(rec.parent);
        for (const h of rec.parent.testHooks.beforeEach) await __runTestHook(h, pctx, ctx);
      }
      try {
        if (rec.fn) await rec.fn.call(ctx, ctx);
      } finally {
        // afterEach：测试级 owner 先，再套件由内向外（注册序，runHook 口径）。
        if (rec.parent) {
          const pctx = __testCtx(rec.parent);
          for (const h of rec.parent.testHooks.afterEach) await __runTestHook(h, pctx, ctx);
        }
        for (let i = rec.suites.length - 1; i >= 0; i--) {
          const s = rec.suites[i];
          for (const h of s.hooks.afterEach) await __runSuiteHook(s, h, ctx);
        }
      }
      if (rec.pending.length > 0) await Promise.all(rec.pending);
      for (const h of rec.testHooks.after) await __runTestHook(h, ctx, ctx);
      // 运行时 skip/todo 终局判定（skip 优先；抛错仍失败，错误粘滞）。
      if (rec.skipped) {
        __skip++;
        __emitPass(rec, { skip: rec.skipMessage ?? true });
        return;
      }
      if (rec.isTodo) {
        __todo++;
        console.log(`todo - ${rec.fullName}`);
        __emitPass(rec, { todo: rec.todoMessage ?? true });
        return;
      }
      if (rec.childFailed) {
        const e = new Error("subtests failed");
        e.code = "ERR_TEST_FAILURE";
        throw e;
      }
      if (rec.planExpected !== null && rec.planActual !== rec.planExpected) {
        const e = new Error(`Expected ${rec.planExpected} assertions, but ${rec.planActual} were run`);
        e.code = "ERR_TEST_FAILURE";
        throw e;
      }
      if (rec.expectFailure === true) {
        // 期望失败却通过 → 真失败（expect-error-but-pass 口径）。
        const e = new Error("test was expected to fail but passed");
        e.code = "ERR_TEST_FAILURE";
        e.failureType = "expectedFailure";
        throw e;
      }
      rec.passed = true;
      ctx.passed = true;
      __pass++;
      for (const s of rec.suites) s._pass++;
      if (__eventSink) {
        __emitPass(rec, null);
      }
    } finally {
      __ctxStack.pop();
    }
  } catch (e) {
    if (rec.expectFailure === true && (!e || e.failureType !== "expectedFailure")) {
      // 期望失败且真失败 → 记 pass（事件带 expectFailure 旗）。
      // 合成 expectedFailure 错（期望失败却通过）落下走真失败。
      rec.passed = true;
      try { ctx.passed = true; } catch {}
      __pass++;
      for (const s of rec.suites) s._pass++;
      if (__eventSink) {
        __emitPass(rec, { expectFailure: true });
      }
    }
    else {
      // 失败标注（真机口径：failureType 缺省 testCodeFailure；既有 code 不动）。
      if (e && (typeof e === "object" || typeof e === "function")) {
        if (e.failureType === undefined) e.failureType = "testCodeFailure";
        if (e.code === undefined) e.code = "ERR_TEST_FAILURE";
      }
      __failOne(rec, e);
      for (const s of rec.suites) s._fail++;
      if (__eventSink) {
        __emit("test:fail", { ...__baseEvent(rec), details: { error: e } });
        __emit("test:complete", __baseEvent(rec));
      }
    }
  } finally {
    try {
      if (rec.signal && typeof rec.signal.aborted === "boolean" && !rec.signal.aborted) {
        // 释放信号：测试结束即中止其 signal（node 同款收尾）。
        if (rec.signalCtrl) rec.signalCtrl.abort();
      }
    } catch {}
    // 本测试 mock 全家自动复原（node Test 收尾 `mock.reset()` 口径：含 timers）。
    try {
      if (rec.mockObj) rec.mockObj.reset();
    } catch {}
    rec.resolve();
  }
}
let __innerActive = false;
function __pump() {
  if (__running || __innerActive) return;
  __running = true;
  __next();
}
async function __next() {
  await __drainLoop();
  await __runAfters();
  __running = false;
  if (__ran + __skip + __todo > 0) {
    console.log(`# pass ${__pass}, fail ${__fail}, skip ${__skip}, todo ${__todo}`);
  }
}
// 批量排空（run(none) 复用：直接调，不碰 __running 泵卫，保证可重入）。
// only-过滤走 __runOne 内逐项门（applyFilters 口径），此处不再批量过滤。
async function __drainLoop() {
  for (;;) {
    const batch = __queue.splice(0);
    if (!batch.length) break;
    for (const t of batch) {
      await __runOne(t);
    }
  }
}
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
  const rec = __mkTest(name, options, fn, parentRec.suites, parentRec);
  parentRec.children.push(rec);
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
