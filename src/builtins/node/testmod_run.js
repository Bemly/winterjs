// run() 事件流（精简 TestsStream：on + for-await；compose/reporter 另片）。
function __makeStream() {
  const listeners = {};
  const buf = [];
  let done = false;
  const waiters = [];
  const stream = {
    on(ev, fn) {
      if (typeof fn === "function") (listeners[String(ev)] ??= []).push(fn);
      return stream;
    },
    off(ev, fn) {
      const list = listeners[String(ev)];
      if (list) {
        const i = list.indexOf(fn);
        if (i >= 0) list.splice(i, 1);
      }
      return stream;
    },
    _emit(type, data) {
      // 监听抛错不吞（真机 EventEmitter 口径：mustNotCall 之类即时断言靠抛错
      // 现形；吞掉即假绿）。
      for (const fn of (listeners[type] ?? []).slice()) fn(data);
      const evt = { type, data };
      if (waiters.length > 0) waiters.shift()(evt);
      else buf.push(evt);
    },
    _end() {
      done = true;
      for (const w of waiters.splice(0)) w(null);
    },
    [Symbol.asyncIterator]() {
      let i = 0;
      const it = {
        next() {
          if (i < buf.length) return Promise.resolve({ value: buf[i++], done: false });
          if (done) return Promise.resolve({ value: undefined, done: true });
          return new Promise((resolve) => waiters.push((evt) => {
            if (evt === null) resolve({ value: undefined, done: true });
            else { i = buf.length; resolve({ value: evt, done: false }); }
          }));
        },
        [Symbol.asyncIterator]() { return it; },
      };
      return it;
    },
  };
  return stream;
}
// 测试发现（run 无 files + 有 cwd 时）：*.test.{mjs,cjs,js}，跳过 node_modules。
function __discoverTests(cwd) {
  const base = cwd ?? process.cwd();
  let entries = [];
  try {
    entries = readdirSync(base, { recursive: true });
  } catch {
    return [];
  }
  return entries
    .map((e) => String(e))
    .filter((p) => !p.includes("node_modules") && /(^|\/)[^/]+\.test\.(mjs|cjs|js)$/.test(p))
    .map((p) => `${base}/${p}`)
    .sort();
}
export function run(options = {}) {
  validateObject(options, "options");
  if (options.globalSetupPath !== undefined) {
    validateString(options.globalSetupPath, "options.globalSetupPath");
  }
  // testTagFilters 校验与归一（tags-validation 套件口径；过滤语义另片）。
  // 注意门序：校验先于隔离门（validation 测试不带 files，须先抛校验错）。
  if (options.testTagFilters !== undefined) {
    let tagFilters = options.testTagFilters;
    if (typeof tagFilters === "string") tagFilters = [tagFilters];
    validateArray(tagFilters, "options.testTagFilters");
    for (let i = 0; i < tagFilters.length; i++) {
      validateString(tagFilters[i], `options.testTagFilters[${i}]`);
    }
  }
  // coverage 选项校验（run-coverage 套件口径：码逐字；覆盖率本身另案）。
  if (options.coverage !== undefined) {
    validateBoolean(options.coverage, "options.coverage");
  }
  for (const k of ["coverageExcludeGlobs", "coverageIncludeGlobs"]) {
    const v = options[k];
    if (v === undefined) continue;
    const name = `options.${k}`;
    if (typeof v === "string") continue;
    validateArray(v, name);
    for (let i = 0; i < v.length; i++) validateString(v[i], `${name}[${i}]`);
  }
  for (const k of ["lineCoverage", "branchCoverage", "functionCoverage"]) {
    const v = options[k];
    if (v === undefined) continue;
    validateInteger(v, `options.${k}`, 0, 100);
  }
  const isolation = options.isolation ?? "process";
  const stream = __makeStream();
  const files = options.files ?? [];
  if (isolation !== "none" && files.length > 0) {
    // process 隔离经 worker 线程传输（每文件独立会话，9f 底座哲学）。
    __runFilesWorker(options, files, stream);
    return stream;
  }
  __runFilesAsync(options, stream).catch((e) => {
    try { stream._end(); } catch {}
    // 监听抛错（如 mustNotCall）不可吞：异步重抛走 uncaught，文件可见失败。
    queueMicrotask(() => { throw e; });
  });
  return stream;
}
async function __runFilesAsync(options, stream) {
  // 外层状态快照（run 可在测试内调用；内层 exclusively 跑完再还原）。
  const saved = {
    queue: __queue.splice(0),
    suites: __suites.slice(),
    regLen: __suiteReg.length,
    ran: __ran, pass: __pass, fail: __fail, skip: __skip, todo: __todo,
    sink: __eventSink,
    curFile: __currentFile,
    exitCode: globalThis.process.exitCode,
    innerActive: __innerActive,
  };
  const innerRoot = __mkSuite("<root>", __EMPTY_TAGS, null);
  __suites.length = 0;
  __suites.push(innerRoot);
  __suiteReg.push(innerRoot);
  __ran = 0; __pass = 0; __fail = 0; __skip = 0; __todo = 0;
  __eventSink = stream;
  __innerActive = true;
  try {
    let files = options.files ?? [];
    if (files.length === 0 && options.cwd) files = __discoverTests(options.cwd);
    const cwd = options.cwd ?? process.cwd();
    for (const f of files) {
      // name 取调用方原样（相对路径即相对名），file 取绝对（filetest 口径）；
      // 载入失败综合成文件级 fail（行列 1:1，真机口径），并回滚本次注册
      // （loader 目标回退可能重复求值；残留注册即重复执行）。
      const given = String(f);
      // 路径规范化（process.cwd 可能含 ..，filetest 断言绝对路径全等）。
      const abs = resolvePath(cwd, given);
      __currentFile = abs;
      __emit("test:enqueue", { name: given, file: abs, testId: ++__testIdCounter });
      const qLen = __queue.length;
      const rLen = __suiteReg.length;
      try {
        await import(pathToFileURL(abs).href);
      } catch (e) {
        __queue.length = qLen;
        __suiteReg.length = rLen;
        if (e && (typeof e === "object" || typeof e === "function") && e.failureType === undefined) {
          e.failureType = "testCodeFailure";
        }
        __emit("test:fail", { name: given, file: abs, line: 1, column: 1, testId: ++__testIdCounter, details: { error: e } });
      }
    }
    __currentFile = null;
    await __drainLoop();
    await __runAfters();
    // 套件落定事件（run 口径：有执行后代且零失败即 pass，否则有失败即 fail）。
    for (const s of __suiteReg.slice(saved.regLen)) {
      if (s === innerRoot) continue;
      if (s._fail > 0) {
        const e = new Error(`${s._fail} subtests failed`);
        e.code = "ERR_TEST_FAILURE";
        __emit("test:fail", { name: s.name, fullName: __suiteFullName(s), testId: s.testId, nesting: s.depth, tags: s.ownTags, details: { error: e } });
        __emit("test:complete", { name: s.name, testId: s.testId, nesting: s.depth });
      } else if (s._pass > 0) {
        __emit("test:pass", { name: s.name, fullName: __suiteFullName(s), testId: s.testId, nesting: s.depth, tags: s.ownTags });
        __emit("test:complete", { name: s.name, testId: s.testId, nesting: s.depth });
      } else if (s.skip) {
        __emit("test:pass", { name: s.name, fullName: __suiteFullName(s), testId: s.testId, nesting: s.depth, tags: s.ownTags, skip: true });
        __emit("test:complete", { name: s.name, testId: s.testId, nesting: s.depth });
      } else if (s.todo) {
        __emit("test:pass", { name: s.name, fullName: __suiteFullName(s), testId: s.testId, nesting: s.depth, tags: s.ownTags, todo: true });
        __emit("test:complete", { name: s.name, testId: s.testId, nesting: s.depth });
      }
    }
  } finally {
    __queue.length = 0;
    __queue.push(...saved.queue);
    __suites.length = 0;
    __suites.push(...saved.suites);
    __suiteReg.length = saved.regLen;
    __ran = saved.ran; __pass = saved.pass; __fail = saved.fail;
    __skip = saved.skip; __todo = saved.todo;
    __eventSink = saved.sink;
    __currentFile = saved.curFile;
    globalThis.process.exitCode = saved.exitCode;
    __innerActive = saved.innerActive;
  }
  stream._end();
}
// ---- run() process 隔离（worker 线程传输，D 轮） ----
//
// 设计：每文件一个 worker（eval  harness，独立会话）；子内跑
// run({isolation:"none"}) 并经 parentPort 逐事件回传（错误序列化为 plain）。
// 父端重组为 Error 后重发到 stream。串行跑文件（并发另轮）。
// NODE_TEST_CONTEXT 由子端置位（与父隔离，探针实证不互染）。
const __RUN_CHILD = [
  'import { parentPort, workerData } from "node:worker_threads";',
  'process.env.NODE_TEST_CONTEXT = "1";',
  'const mod = await import("node:test");',
  'const stream = mod.run({ files: workerData.files, isolation: "none", cwd: workerData.cwd });',
  'function serError(e) {',
  '  if (e !== null && (typeof e === "object" || typeof e === "function")) {',
  '    let stack = undefined;',
  '    try { stack = String(e.stack); } catch (x) {}',
  '    return { name: e.name, message: e.message, code: e.code, failureType: e.failureType, stack };',
  '  }',
  '  return e;',
  '}',
  'function ser(data) {',
  '  const d = {};',
  '  for (const k of Object.keys(data)) d[k] = data[k];',
  '  if (d.details && d.details.error !== undefined) {',
  '    const details = {};',
  '    for (const k of Object.keys(d.details)) details[k] = d.details[k];',
  '    details.error = serError(d.details.error);',
  '    d.details = details;',
  '  }',
  '  return d;',
  '}',
  'for (const kind of ["test:enqueue", "test:dequeue", "test:start", "test:pass", "test:fail", "test:complete"]) {',
  '  stream.on(kind, (data) => {',
  '    try { parentPort.postMessage({ kind, data: ser(data) }); } catch (x) {}',
  '  });',
  '}',
  'try {',
  '  for await (const _ of stream) { }',
  '  parentPort.postMessage({ done: true });',
  '} catch (e) {',
  '  try { parentPort.postMessage({ fatal: String((e && e.message) || e) }); } catch (x) {}',
  '}',
].join("\n");
function __rehydrateError(s) {
  if (s === null || (typeof s !== "object" && typeof s !== "function")) return s;
  const e = new Error(s.message);
  try { e.name = s.name ?? "Error"; } catch {}
  if (s.code !== undefined) {
    try { e.code = s.code; } catch {}
  }
  if (s.failureType !== undefined) {
    try { e.failureType = s.failureType; } catch {}
  }
  return e;
}
function __runOneWorker(given, abs, stream) {
  return new Promise((resolve) => {
    let w;
    try {
      w = new Worker(__RUN_CHILD, {
        eval: true,
        workerData: { files: [given], cwd: process.cwd() },
      });
    } catch (e) {
      stream._emit("test:fail", { name: given, file: abs, line: 1, column: 1, testId: ++__testIdCounter, details: { error: e } });
      resolve();
      return;
    }
    let settled = false;
    const done = () => {
      if (!settled) {
        settled = true;
        resolve();
      }
    };
    w.on("message", (m) => {
      if (!m || typeof m !== "object") return;
      if (m.done) return;
      if (m.fatal !== undefined) {
        const e = new Error(String(m.fatal));
        e.code = "ERR_TEST_FAILURE";
        stream._emit("test:fail", { name: given, file: abs, line: 1, column: 1, testId: ++__testIdCounter, details: { error: e } });
        return;
      }
      if (typeof m.kind === "string") {
        const data = m.data && typeof m.data === "object" ? m.data : {};
        if (data.details && data.details.error !== undefined) {
          data.details = { ...data.details, error: __rehydrateError(data.details.error) };
        }
        stream._emit(m.kind, data);
      }
    });
    w.on("error", (e) => {
      stream._emit("test:fail", { name: given, file: abs, line: 1, column: 1, testId: ++__testIdCounter, details: { error: e } });
      done();
    });
    w.on("exit", () => done());
  });
}
async function __runFilesWorker(options, files, stream) {
  const cwd = options.cwd ?? process.cwd();
  for (const f of files) {
    const given = String(f);
    const abs = given.startsWith("/") ? given : `${cwd}/${given}`;
    await __runOneWorker(given, abs, stream);
  }
  stream._end();
}
