//! `node:test` 起步（纯 JS，无 natives）：`test/describe(suite)/it` + skip/todo/only +
//! `before/after/beforeEach/afterEach`（套件级 + 测试级 `t.*`）+ `t.test` 子测试 +
//! `t.plan`/`t.waitFor`/`t.assert`（全 assert 键 + snapshot/fileSnapshot 桩 +
//! 模块级 `assert.register` 自定义断言）+ `t.tags`（校验/小写规范/父优先并集/冻结）+
//! `getTestContext`（当前上下文栈，串行泵内跨 setImmediate 有效）+ 名过滤
//! （`WINTERJS_TEST_NAME_PATTERN`：子串或 `/re/flags`，不命中即 skip）。
//! 串行泵，失败记数并置 `exitCode=1`，队空打印小结。
//! 口径（文档记录）：名过滤跳过测试本体与 Each 钩子，before/after 照跑；套件
//! `after` 在整轮末尾按深度由内向外跑；hook 抛错：before 毒化其套件，Each 只
//! fail 当个，after 计 fail。子测试 depth-first 即时跑，父等子齐（`pending`）；
//! 子失败父亦 fail。`suite` 回调同步执行并收 SuiteContext。plan 不匹配即 fail。
//! 超出范围（另轮）：`run()` 编程 API、CLI `--test`、TAP reporter、coverage、
//! mock 全家（`t.mock` 仅 `fn` 最小面）、top-level only/skip/todo/expectFailure。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
import assertMod from "node:assert";
import validators from "node:internal/validators";
import { codes } from "node:internal/errors";
import { readFileSync } from "node:fs";
import { MockTracker } from "node:internal/test/mock";

const { validateArray, validateFunction, validateNumber, validateObject, validateString, validateUint32 } = validators;
// node internal/timers TIMEOUT_MAX（2**31 - 1）同值。
const TIMEOUT_MAX = 2147483647;
const __UNCOPIED = new Set(["AssertionError", "strict", "Assert", "options"]);
const __SKIP = Symbol("skip");
const __TODO = Symbol("todo");
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
    hooks: { before: [], after: [], beforeEach: [], afterEach: [] },
    beforeFired: false, poison: null, depth: parent ? parent.depth + 1 : 0,
  };
}
const __rootSuite = __mkSuite("", __EMPTY_TAGS, null);
const __suites = [__rootSuite];
const __suiteReg = [__rootSuite];
const __queue = [];
let __running = false;
let __ran = 0, __pass = 0, __fail = 0, __skip = 0, __todo = 0;
let __only = false;
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
  for (let s = suite; s !== null; s = s.parent) {
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
    skip(msg) {
      const e = { [__SKIP]: true };
      if (msg !== undefined) e.message = msg;
      throw e;
    },
    todo(msg) {
      const e = { [__TODO]: true };
      if (msg !== undefined) e.message = msg;
      throw e;
    },
    plan(count, options) {
      if (rec.planExpected !== null) {
        const e = new Error("cannot set plan more than once");
        e.code = "ERR_TEST_FAILURE";
        throw e;
      }
      validateUint32(count, "count");
      if (options !== undefined) validateObject(options, "options");
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
  return {
    name,
    fullName: parent ? `${parent.fullName} > ${name}` : [...suites.map((s) => s.name).filter(Boolean), name].join(" > "),
    fn: typeof fn === "function" ? fn : undefined,
    mode: options.only === true ? "only" : (options.skip ? "skip" : (options.todo ? "todo" : "run")),
    suites, parent: parent || null,
    tags: Object.freeze(tags),
    signal: __freshSignal(),
    testHooks: { before: [], after: [], beforeEach: [], afterEach: [] },
    children: [], pending: [], childFailed: false,
    planExpected: null, planActual: 0,
    assertObj: null, mockObj: null, ctx: null,
    passed: false, failed: false, beforeFired: false,
    done, resolve,
  };
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
async function __fireBefore(suite) {
  if (suite.beforeFired) return;
  suite.beforeFired = true;
  const sc = __suiteCtx(suite);
  __ctxStack.push(sc);
  try {
    for (const h of suite.hooks.before) await h(sc);
  } catch (e) {
    suite.poison = e;
  } finally {
    __ctxStack.pop();
  }
}
// 套件钩子跑在套件上下文（getTestContext 见套件名），参数传当前测试 ctx。
async function __runSuiteHook(suite, fn, testCtx) {
  const sc = __suiteCtx(suite);
  __ctxStack.push(sc);
  try {
    await fn(testCtx);
  } finally {
    __ctxStack.pop();
  }
}
// 测试级钩子跑在 owner 上下文（getTestContext 见 owner 名），参数传子测试
// ctx（get-test-context 测试级钩子套件钉住；套件级同理见 __runSuiteHook）。
async function __runTestHook(fn, ownerCtx, argCtx) {
  __ctxStack.push(ownerCtx);
  try {
    await fn(argCtx);
  } finally {
    __ctxStack.pop();
  }
}
async function __runOne(rec) {
  try {
    if (rec.mode === "skip") { __skip++; return; }
    if (rec.mode === "todo") { __todo++; console.log(`todo - ${rec.fullName}`); return; }
    if (rec.mode === "only") { /* 顶层 only 过滤在泵批量层；子测试照跑 */ }
    if (!__nameOk(rec.fullName)) { __skip++; return; }
    __ran++;
    const ctx = __testCtx(rec);
    __ctxStack.push(ctx);
    try {
      // before（runOnce）：套件由外向内；测试级 owner 在首个子测试时跑一次。
      // beforeEach/afterEach 跑在子测试身上（带子 ctx），owner 自身的不为自己跑
      // （真机 Test.run 口径：`this.parent.hooks.*` + 自身 after；探针 hook.cjs）。
      for (const s of rec.suites) await __fireBefore(s);
      const poisoned = rec.suites.find((s) => s.poison);
      if (poisoned) throw poisoned.poison;
      if (rec.parent && !rec.parent.beforeFired) {
        rec.parent.beforeFired = true;
        const pctx = __testCtx(rec.parent);
        __ctxStack.push(pctx);
        try {
          for (const h of rec.parent.testHooks.before) await h(pctx);
        } finally {
          __ctxStack.pop();
        }
      }
      // beforeEach：套件由外向内，再是测试级 owner 的（注册序）。
      for (const s of rec.suites) for (const h of s.hooks.beforeEach) await __runSuiteHook(s, h, ctx);
      if (rec.parent) {
        const pctx = __testCtx(rec.parent);
        for (const h of rec.parent.testHooks.beforeEach) await __runTestHook(h, pctx, ctx);
      }
      try {
        if (rec.fn) await rec.fn(ctx);
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
      for (const h of rec.testHooks.after) await __runTestHook(h, ctx);
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
      rec.passed = true;
      ctx.passed = true;
      __pass++;
    } finally {
      __ctxStack.pop();
    }
  } catch (e) {
    if (e && e[__SKIP]) { __skip++; }
    else if (e && e[__TODO]) { __todo++; console.log(`todo - ${rec.fullName}`); }
    else __failOne(rec, e);
  } finally {
    try {
      if (rec.signal && typeof rec.signal.aborted === "boolean" && !rec.signal.aborted) {
        // 释放信号：测试结束即中止其 signal（node 同款收尾）。
        if (rec.signalCtrl) rec.signalCtrl.abort();
      }
    } catch {}
    // 本测试 mock 全家自动复原（node Test 收尾语义；子测试各有 tracker）。
    try {
      if (rec.mockObj) rec.mockObj.restoreAll();
    } catch {}
    rec.resolve();
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
      await __runOne(t);
    }
  }
  // 套件 after：整轮末尾按深度由内向外（见头注口径）。
  const fired = [];
  const seen = new Set();
  for (const t of __suiteReg.slice()) {
    if (t.beforeFired && !seen.has(t)) { seen.add(t); fired.push(t); }
  }
  fired.sort((a, b) => b.depth - a.depth);
  for (const s of fired) {
    const sc = __suiteCtx(s);
    __ctxStack.push(sc);
    try {
      for (const h of s.hooks.after) await h(sc);
    } catch (e) {
      __fail++;
      globalThis.process.exitCode = 1;
      console.log(`not ok - after hook: ${s.name || "<root>"}`);
      console.log(String((e && e.stack) || (e && e.message) || e).split("\n").slice(0, 4).join("\n"));
    } finally {
      __ctxStack.pop();
    }
  }
  __running = false;
  if (__ran + __skip + __todo > 0) {
    console.log(`# pass ${__pass}, fail ${__fail}, skip ${__skip}, todo ${__todo}`);
  }
}
function __subtest(parentRec, args) {
  const { name, options, fn } = __normCall(args);
  __validateTestOptions(options);
  const rec = __mkTest(name, options, fn, parentRec.suites, parentRec);
  if (rec.mode === "only") __only = true;
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
  if (rec.mode === "only") __only = true;
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
  __only = true;
  const rec = __mkTest(name, { ...options, only: true }, fn, __suites.slice(), null);
  __queue.push(rec);
  __pump();
  return rec.done;
};
export function describe(...args) {
  const { name, options, fn } = __normCall(args);
  const ownTags = options.tags !== undefined ? __canonTags(options.tags, "options.tags") : __EMPTY_TAGS;
  const suite = __mkSuite(name, ownTags, __suites[__suites.length - 1]);
  __suites.push(suite);
  __suiteReg.push(suite);
  const sc = __suiteCtx(suite);
  __ctxStack.push(sc);
  try {
    if (typeof fn === "function") fn(sc);
  } finally {
    __ctxStack.pop();
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
export { testAssert as assert };
export { topMock as mock };
export default test;
"#;
