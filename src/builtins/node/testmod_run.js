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
      for (const fn of (listeners[type] ?? []).slice()) {
        try { fn(data); } catch {}
      }
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
  const isolation = options.isolation ?? "process";
  const stream = __makeStream();
  const files = options.files ?? [];
  if (isolation !== "none" && files.length > 0) {
    // process 隔离（子进程 + TAP/事件传输）另片；此处明确拒绝。
    // 空文件列表直接成功（无物可隔离，真机同款空转）。
    throw new codes.ERR_INVALID_ARG_VALUE("options.isolation", isolation, 'only "none" is supported');
  }
  __runFilesAsync(options, stream).catch(() => {
    try { stream._end(); } catch {}
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
      const abs = String(f).startsWith("/") ? String(f) : `${cwd}/${f}`;
      __currentFile = abs;
      __emit("test:enqueue", { name: abs, file: abs, testId: ++__testIdCounter });
      try {
        await import(pathToFileURL(abs).href);
      } catch (e) {
        __emit("test:fail", { name: abs, file: abs, testId: ++__testIdCounter, details: { error: e } });
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
        __emit("test:pass", { name: s.name, fullName: __suiteFullName(s), testId: s.testId, nesting: s.depth, tags: s.ownTags, skip: false, todo: false });
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
