//! winterjs REPL 底座·补全面（本体拥有补全核心；`node:repl` 仅兼容壳，
//! 反向复用本域——CLI 专属能力一律住本域，禁进 node:* 公开导出面）。
//!
//! - `__wjs_repl_tla_wrap` 系（processTopLevelAwait 移植 + 本桥）随 REPL 会话
//!   注入（runtime/repl），不在 PRELUDE——acorn 6827 行全会话加载拖慢启动。
//! - `__wjs_repl_default_complete(context, line, callback, evalFn?)`：R3 子集
//!   补全核心（成员链逐步求值/fs 路径/bare 上下文键/大小写不敏感），本体拥有。
//!   `evalFn(expr, ctx)` 由调用方注入（`node:repl` 传 vm 求值器；CLI 传空走
//!   全局间接 eval）。fs 经 `__wjs_fs_*` native 直调，不 import `node:fs`。
//! - `__wjs_cli_complete(line)`：CLI reedline Tab 的补全桥——调本地核心，
//!   对 CLI 全局求值面工作，返回 `[[全文, 描述], ...], completeOn`
//!   （描述进 IdeMenu 右侧 pane）。不 require 任何 `node:*`。
//! - 签名摘要：SM native `toString()` 不带形参名（实测空括号），常用面
//!   手写 `__SIG` 表；用户函数抽 toString 真形参；非函数给类型/值摘要
//!   （描述符沿链安全读，不触发 getter）。
pub const REPL_COMPLETE_JS: &str = r#"
// ---- winterjs repl 底座：补全核心（本体拥有；node:repl 薄包反向复用）----
function __isCompIdent(n) {
  return /^[A-Za-z_$][\w$]*$/.test(n);
}
function __isIndexKey(n) {
  if (n === '') return false;
  const v = Number(n);
  return Number.isInteger(v) && v >= 0 && String(v) === n;
}
function __enumKeys(obj) {
  const out = [];
  const seen = new Set();
  let o = obj;
  while (o !== null && o !== undefined && (typeof o === 'object' || typeof o === 'function')) {
    let names = [];
    try { names = Object.getOwnPropertyNames(o); } catch { break; }
    for (const n of names) {
      if (typeof n !== 'string' || seen.has(n)) continue;
      seen.add(n);
      if (__isIndexKey(n) || !__isCompIdent(n)) continue;
      out.push(n);
    }
    try { o = Object.getPrototypeOf(o); } catch { break; }
  }
  return out;
}
function __descAt(obj, key) {
  let o = obj;
  while (o !== null && o !== undefined && (typeof o === 'object' || typeof o === 'function')) {
    let d = null;
    try { d = Object.getOwnPropertyDescriptor(o, key); } catch { return null; }
    if (d !== undefined && d !== null) return d;
    try { o = Object.getPrototypeOf(o); } catch { return null; }
  }
  return null;
}
function __ctxEval(expr, context, evalFn) {
  // 本体求值：调用方注入优先（node:repl 传 vm 求值器）；缺省全局间接 eval
  // （CLI 全局面同源）；vm 上下文无注入即抛给上层转拒答，不静默错配。
  if (typeof evalFn === 'function') return evalFn(expr, context);
  if (context === globalThis) return (0, eval)(expr);
  throw new Error('no evaluator');
}
// base 文本拆根 + 步进（括号配平扫描；非法即 null）。
function __parseSteps(base) {
  const root = /^[A-Za-z_$][\w$]*/.exec(base);
  if (root === null || root.index !== 0) return null;
  const steps = [];
  let i = root[0].length;
  while (i < base.length) {
    const rest = base.slice(i);
    let m = /^\s*\.\s*([A-Za-z_$][\w$]*)/.exec(rest);
    if (m !== null) {
      steps.push({ prop: m[1] });
      i += m[0].length;
      continue;
    }
    m = /^\s*\[/.exec(rest);
    if (m === null) return null;
    let j = i + m[0].length;
    let depth = 1;
    while (j < base.length && depth > 0) {
      const c = base[j];
      if (c === '"' || c === "'" || c === '`') {
        const q = c;
        j++;
        while (j < base.length && base[j] !== q) j += base[j] === '\\' ? 2 : 1;
        j++;
        continue;
      }
      if (c === '[') depth++;
      else if (c === ']') depth--;
      j++;
    }
    if (depth !== 0) return null;
    steps.push({ key: base.slice(i + m[0].length, j - 1).trim() });
    i = j;
    const ws = /^\s*/.exec(base.slice(i))[0];
    i += ws.length;
  }
  return { root: root[0], steps };
}
function __walkSteps(parsed, context, evalFn) {
  let obj;
  try { obj = __ctxEval(parsed.root, context, evalFn); }
  catch { return null; }
  for (let si = 0; si < parsed.steps.length; si++) {
    const st = parsed.steps[si];
    const last = si === parsed.steps.length - 1;
    if (obj === null || obj === undefined) return null;
    // 末段允许原始值（Number 原型面）；中段恒对象（描述符步进）。
    if (typeof obj !== 'object' && typeof obj !== 'function') {
      if (!last) return null;
      obj = Object(obj);
    }
    let key;
    if (st.prop !== undefined) {
      key = st.prop;
    } else {
      // 仅调用形括号拒答；箭头/插值/赋值一律拒；tag 模板拒、纯模板放行。
      if (/[A-Za-z_$][\w$]*\s*\(|=>|\$\{|=/.test(st.key)) return null;
      const __wide = st.key.trim();
      if (!/^`(?:[^`\\]|\\.)*`$/.test(__wide) && __wide.includes('`')) return null;
      try { key = __ctxEval(st.key, context, evalFn); }
      catch { return null; }
      if (typeof key !== 'string' && typeof key !== 'number') return null;
      key = String(key);
    }
    const d = __descAt(obj, key);
    if (d === null || d.get !== undefined || d.set !== undefined) return null;
    try { obj = obj[key]; }
    catch { return null; }
  }
  if (obj === null || obj === undefined) return null;
  return Object(obj);
}
function __fsComplete(dir, prefix) {
  // 真机 fs 补全口径：既存目录即列子项裸名（completeOn 置空），否则同级
  // 前缀过滤裸名；坏径即空（completeOn 回前缀）。经 __wjs_fs_* native 直调。
  const base = dir === '' ? '.' : dir;
  const full = prefix === '' ? base : base + '/' + prefix;
  let isDir = false;
  try {
    const meta = JSON.parse(__wjs_fs_stat(full, true));
    isDir = !!(meta && meta.isDirectory === true);
  } catch { isDir = false; }
  if (isDir) {
    let names = [];
    try { names = JSON.parse(__wjs_fs_readdir(full, false)); }
    catch { return [[], '']; }
    if (!Array.isArray(names)) return [[], ''];
    return [names.filter((n) => typeof n === 'string').sort(), ''];
  }
  let names = [];
  try { names = JSON.parse(__wjs_fs_readdir(base, false)); }
  catch { return [[], prefix]; }
  if (!Array.isArray(names)) return [[], prefix];
  return [names.filter((n) => typeof n === 'string' && n.startsWith(prefix)).sort(), prefix];
}
function __commonPrefix(list) {
  if (list.length === 0) return '';
  let p = list[0];
  for (let i = 1; i < list.length; i++) {
    const s = list[i];
    let j = 0;
    while (j < p.length && j < s.length && p[j] === s[j]) j++;
    p = p.slice(0, j);
    if (p === '') break;
  }
  return p;
}
// 同长掩码（串内逐字空格，定位用；`=` 剥离不偏）。
function __maskStrings(line) {
  return line.replace(/"(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'|`(?:[^`\\]|\\.)*`/g, (m) => ' '.repeat(m.length));
}
globalThis.__wjs_repl_common_prefix = __commonPrefix;
globalThis.__wjs_repl_default_complete = function __defaultComplete(context, line, callback, evalFn) {
  const done = (list, completeOn) => callback(null, [list, completeOn]);
  if (typeof line !== 'string') line = String(line);
  const masked0 = __maskStrings(line);
  // 调用结果成员恒拒答（nosideeffects 口径）。
  if (/\)\s*\.\s*[\w$]*$/.test(masked0)) return done([], line);
  // ① 成员形（末段点+前缀；new 剥除；声明赋值取等号后段）。
  const tryMember = (text) => {
    const mm = /^(.*?)\.\s*([\w$]*)$/.exec(text);
    if (mm === null) return null;
    let base = mm[1];
    const filter = mm[2];
    base = base.replace(/^new\s+/, '');
    const parsed = __parseSteps(base);
    if (parsed === null) return 'parse-fail';
    const obj = __walkSteps(parsed, context, evalFn);
    if (obj === null) return 'walk-fail';
    // 过滤大小写不敏感，回显原键。
    const lowFilter = filter.toLowerCase();
    const list = __enumKeys(obj).filter((k) => k.toLowerCase().startsWith(lowFilter))
      .map((k) => `${base}.${k}`);
    return [list, `${base}.${filter}`];
  };
  let mr = tryMember(line);
  if (Array.isArray(mr)) return done(...mr);
  // 解析成立而求值失败即拒答，不穿透 bare。
  if (mr === 'walk-fail') return done([], line);
  // mr 为 null 或 parse-fail：先试路径。
  const qm = /(['"`])((?:\\.|(?!\1).)*)$/.exec(line);
  if (qm !== null) {
    const content = qm[2];
    const slash = content.lastIndexOf('/');
    const dir = content.slice(0, slash);
    const prefix = content.slice(slash + 1);
    return done(...__fsComplete(dir, prefix));
  }
  if (mr === 'parse-fail') {
    const eq = masked0.lastIndexOf('=');
    if (eq > 0 && line.slice(eq + 1).trim() !== '') {
      const mr2 = tryMember(line.slice(eq + 1).trim());
      if (Array.isArray(mr2)) return done(...mr2);
      if (mr2 === 'walk-fail') return done([], line);
    }
  }
  // ③ 剥字面量后残留结构符即拒答。
  if (/[()=;{}=]|`/.test(masked0)) return done([], line);
  // ④ bare 词仅无点行。
  if (masked0.includes('.')) return done([], line);
  const bm = /([A-Za-z_$][\w$]*)$/.exec(line);
  if (bm === null) return done([], line);
  const prefix = bm[1];
  const head = line.slice(0, line.length - prefix.length);
  let keys = [];
  try {
    keys = keys.concat(Object.getOwnPropertyNames(context));
  } catch { /* ignore */ }
  try {
    keys = keys.concat(Object.getOwnPropertyNames(globalThis));
  } catch { /* ignore */ }
  const seen = new Set();
  const list = [];
  const lowPrefix = prefix.toLowerCase();
  for (const k of keys) {
    if (!k.toLowerCase().startsWith(lowPrefix) || seen.has(k)) continue;
    seen.add(k);
    list.push(head + k);
  }
  return done(list, line);
};

// ---- winterjs repl 底座：CLI 补全桥（__wjs_ 内部面；调本地核心）----
globalThis.__wjs_cli_complete = (line) => {
  const s = String(line);
  let list, completeOn;
  if (s.trim() === '') {
    // 空行 Tab：全局全枚举（node 真机同形：空行 Tab 即列全局）。
    // 核心 `bm === null` 回空集是模块保守口径——CLI 本体面在此展开。
    // `__wjs_` 内部面不计入（400+ plumbing 名淹没有菜单；显式前缀仍可触达）；
    // completeOn 置空（Rust 零宽 span 光标处插入）。排序保证稳定。
    // 词法绑定（let/const）不可枚举，记档。
    let keys = [];
    try { keys = Object.getOwnPropertyNames(globalThis); } catch { keys = []; }
    list = keys
      .filter((k) => /^[A-Za-z_$][\w$]*$/.test(k) && !k.startsWith('__wjs_'))
      .sort();
    completeOn = '';
  } else {
    // 本地核心直调（本体拥有，不 require 任何 node:*；经 globalThis
    // 属性取——具名函数表达式赋值不建词法绑定，裸名不可见）。
    const __core = globalThis.__wjs_repl_default_complete;
    if (typeof __core !== 'function') return [[], s];
    let out = null;
    try { __core(globalThis, s, (err, r) => { out = r; }); } catch { return [[], s]; }
    if (out === null || !Array.isArray(out)) return [[], s];
    [list, completeOn] = out;
  }
  // 描述的 base：成员形 = completeOn 去 `.filter` 的 base 表达式（核心 walk 已
  // 保证路径无 getter/调用，重求值无副作用）；bare 形 = globalThis。
  let base = globalThis;
  if (typeof completeOn === 'string' && completeOn.includes('.')) {
    const b = completeOn.slice(0, completeOn.lastIndexOf('.'));
    try { base = b === '' ? null : (0, eval)(b); } catch { base = null; }
  }
  const withSig = (Array.isArray(list) ? list : []).map((text) => {
    if (typeof text !== 'string') return [text, null];
    const key = text.slice(text.lastIndexOf('.') + 1);
    const parts = text.split('.');
    const head2 = parts.length >= 2 ? parts.slice(-2).join('.') : '';
    const sig = __sigDesc(base, key, head2);
    // 浮窗文档实时读语料（`.doc` 同源；表里禁贴文档句）：有文档即整块只放
    // 文档（描述盒按空白重排，签名文档拼一行恒挤成一段，故不拼）；缺页回签名。
    // 实例面（`u.get`）文本是变量名，再试 `Ctor.key`（普通 Object 跳过，
    // 用户自有方法不受染——与 `__sigDesc` 同规则）。
    const docFor = (t) => {
      try {
        if (typeof __wjs_doc_summary !== 'function') return null;
        const d = __wjs_doc_summary(t);
        return (typeof d === 'string' && d !== '') ? d : null;
      } catch { return null; }
    };
    let doc = docFor(text);
    if (doc === null) {
      try {
        const cn = __wjsReplCtorName(base);
        if (cn !== '' && cn !== 'Object') doc = docFor(`${cn}.${key}`);
      } catch { /* ignore */ }
    }
    if (doc === null) return [text, sig];
    // 右盒只放文档（候选框只放干净名；签名不进任何格——描述盒按空白重排，
    // 任何拼接恒挤成一段；缺页才回签名）。
    return [text, doc];
  });
  return [withSig, completeOn];
};

// ---- 成员签名表（签名 only，不搬运文档；文档只读语料，见 `.doc`）----
// null 原型（查表裸键不得沿原型链撞 Object.prototype 的同名方法——
// propertyIsEnumerable/toString 等 hits 自身，实测踩过）。
// 匹配序（`__sigDesc`）：精确 dotted 路径或 completed 名的表项优先——
// toString/Ctor 全兜底；用户自有同名方法因键不命中而不受遮蔽（4.227）。
// 整篇文档走 `mdn-content/` 语料（`.doc` 直读），此处禁贴文档句（2026-09-28
// 用户裁定：右盒纯文档，候选框干净名，文档更新只动语料）。
const __wjsReplSig = Object.assign(Object.create(null), {
  'Object.assign': '(target, ...sources) → object',
  'Object.keys': '(o) → string[]',
  'Object.values': '(o) → array',
  'Object.entries': '(o) → [key, value][]',
  'Object.freeze': '(o) → o',
  'Object.isFrozen': '(o) → boolean',
  'Object.create': '(proto, props?) → object',
  'Object.defineProperty': '(o, key, desc) → o',
  'Object.defineProperties': '(o, descs) → o',
  'Object.getOwnPropertyNames': '(o) → string[]',
  'Object.getOwnPropertySymbols': '(o) → symbol[]',
  'Object.getOwnPropertyDescriptor': '(o, key) → desc | undefined',
  'Object.getOwnPropertyDescriptors': '(o) → descs',
  'Object.getPrototypeOf': '(o) → object | null',
  'Object.setPrototypeOf': '(o, proto) → o',
  'Object.fromEntries': '(iter) → object',
  'Object.hasOwn': '(o, key) → boolean',
  'Object.is': '(a, b) → boolean',
  'Object.groupBy': '(items, cb) → object',
  'Array.from': '(iter, mapFn?, thisArg?) → array',
  'Array.of': '(...v) → array',
  'Array.isArray': '(v) → boolean',
  'JSON.parse': '(text, reviver?) → any',
  'JSON.stringify': '(value, replacer?, space?) → string | undefined',
  'JSON.rawJSON': '(text) → object',
  'Math.abs': '(x) → number',
  'Math.floor': '(x) → number',
  'Math.ceil': '(x) → number',
  'Math.round': '(x) → number',
  'Math.trunc': '(x) → number',
  'Math.sign': '(x) → -1 | 0 | 1',
  'Math.sqrt': '(x) → number',
  'Math.cbrt': '(x) → number',
  'Math.pow': '(x, y) → number',
  'Math.min': '(...x) → number',
  'Math.max': '(...x) → number',
  'Math.random': '() → number [0, 1)',
  'Math.log': '(x) → number',
  'console.log': '(...data) — stdout',
  'console.info': '(...data) — stdout',
  'console.warn': '(...data) — stderr',
  'console.error': '(...data) — stderr',
  'console.debug': '(...data) — stdout',
  'console.dir': '(obj, opts?) — stdout',
  'console.assert': '(cond, ...data) — throw when false',
  'console.table': '(tabular, props?)',
  'console.time': '(label?) — start timer',
  'console.timeLog': '(label?, ...data) — log elapsed',
  'console.timeEnd': '(label?) — stop & log',
  'console.count': '(label?) — count & log',
  'console.countReset': 'countReset(label?) — The console.countReset() static method resets counter used with console.count().',
  'console.group': '(...data) — indent',
  'console.groupEnd': '() — dedent',
  'console.trace': '(...data) — stderr + stack',
  'process.exit': '(code?) — force exit',
  'process.cwd': '() → string',
  'process.chdir': '(dir)',
  'process.on': '(event, listener)',
  'process.kill': '(pid, signal?)',
  'process.nextTick': '(cb, ...args)',
  'process.hrtime': '(time?) → [s, ns]',
  'Buffer.from': '(str, enc?) | (arrayLike) | (buffer)',
  'Buffer.alloc': '(size, fill?, enc?) → Buffer',
  'Buffer.allocUnsafe': '(size) → Buffer',
  'Buffer.isBuffer': '(v) → boolean',
  'Buffer.isEncoding': '(enc) → boolean',
  'Buffer.concat': '(list, totalLength?) → Buffer',
  'Buffer.byteLength': '(str, enc?) → number',
  'String.slice': '(start?, end?) → string',
  'String.substring': '(start, end?) → string',
  'String.indexOf': '(search, pos?) → number',
  'String.includes': '(search, pos?) → boolean',
  'String.startsWith': '(search, pos?) → boolean',
  'String.endsWith': '(search, end?) → boolean',
  'String.replace': '(pat, rep) → string',
  'String.replaceAll': '(pat, rep) → string',
  'String.split': '(sep?, limit?) → string[]',
  'String.trim': '() → string',
  'String.toUpperCase': '() → string',
  'String.toLowerCase': '() → string',
  'String.charAt': '(i) → string',
  'String.charCodeAt': '(i) → number',
  'String.at': '(i) → string | undefined',
  'String.padStart': '(len, pad?) → string',
  'String.padEnd': '(len, pad?) → string',
  'String.repeat': '(n) → string',
  'String.concat': '(...s) → string',
  'Number.toFixed': '(digits?) → string',
  'Number.toPrecision': '(p?) → string',
  'Number.toString': '(radix?) → string',
  'Number.isInteger': '(v) → boolean',
  'Number.isSafeInteger': '(v) → boolean',
  'Number.isNaN': '(v) → boolean',
  'Number.isFinite': '(v) → boolean',
  'Number.parseFloat': '(s) → number',
  'Number.parseInt': '(s, radix?) → number',
  'Promise.then': '(onFulfilled?, onRejected?) → Promise',
  'Promise.catch': '(onRejected?) → Promise',
  'Promise.finally': '(cb?) → Promise',
  'Array.push': '(...v) → new length',
  'Array.pop': '() → removed | undefined',
  'Array.shift': '() → removed | undefined',
  'Array.unshift': '(...v) → new length',
  'Array.slice': '(start?, end?) → array',
  'Array.splice': '(start, deleteCount?, ...items) → removed[]',
  'Array.indexOf': '(search, from?) → number',
  'Array.includes': '(search, from?) → boolean',
  'Array.join': '(sep?) → string',
  'Array.concat': '(...v) → array',
  'Array.map': '(cb, thisArg?) → array',
  'Array.filter': '(cb, thisArg?) → array',
  'Array.reduce': '(cb, init?) → any',
  'Array.forEach': '(cb, thisArg?) → undefined',
  'Array.find': '(cb, thisArg?) → v | undefined',
  'Array.findIndex': '(cb, thisArg?) → number',
  'Array.sort': '(cmp?) → this',
  'Array.reverse': '() → this',
  'Array.flat': '(depth?) → array',
  'Array.at': '(i) → v | undefined',
  // WinterCG 全局（bare 名经 completed 名命中；实例面经 constructor 名命中）。
  'Deno.readFile': '(path) → Promise<Uint8Array>',
  'Deno.writeFile': '(path, data) → Promise<void>',
  'Deno.readTextFile': '(path) → Promise<string>',
  'Deno.writeTextFile': '(path, text) → Promise<void>',
  'Deno.open': '(path) → Promise<FsFile>',
  'Deno.stat': '(path) → Promise<FileInfo>',
  'Deno.lstat': '(path) → Promise<FileInfo>',
  'Deno.mkdir': '(path, opts?) → Promise<void>',
  'Deno.remove': '(path, opts?) → Promise<void>',
  'Deno.rename': '(a, b) → Promise<void>',
  'Deno.copyFile': '(a, b) → Promise<void>',
  'Deno.symlink': '(a, b) → Promise<void>',
  'Deno.readLink': '(path) → Promise<string>',
  'Deno.realPath': '(path) → Promise<string>',
  'Deno.readDir': '(path) → Promise<DirEntry[]>',
  'Deno.makeTempDir': '(opts?) → Promise<string>',
  'Deno.makeTempFile': '(opts?) → Promise<string>',
  'Deno.truncate': '(path, len?) → Promise<void>',
  'Deno.chmod': '(path, mode) → Promise<void>',
  'Deno.chown': '(path, uid, gid) → Promise<void>',
  'Deno.utime': '(path, atime, mtime) → Promise<void>',
  'Deno.watchFs': '(path, opts?) → FSWatcher',
  'Deno.test': '(name, fn) → void',
  'Deno.serve': '(opts, handler) → Promise<Server>',
  'Deno.connect': '(opts) → Promise<Conn>',
  'Deno.listen': '(opts) → Promise<Listener>',
  'Deno.listenDatagram': '(opts) → DatagramConn (unstable)',
  'Deno.resolveDns': '(query, type?) → Promise<string[]>',
  'Deno.upgradeWebSocket': '() → throws (use serve handler)',
  'Deno.addSignalListener': '(sig, cb) → void',
  'Deno.removeSignalListener': '(sig, cb) → void',
  'Deno.cwd': '() → string',
  'Deno.chdir': '(dir) → void',
  'Deno.exit': '(code?) → never',
  'Deno.hostname': '() → string',
  'Deno.osRelease': '() → string',
  'Deno.networkInterfaces': '() → NetworkInterface[]',
  'Deno.systemMemoryInfo': '() → SystemMemoryInfo',
  'Deno.consoleSize': '() → { columns, rows }',
  'Deno.env.get': '(k) → string | undefined',
  'Deno.env.set': '(k, v) → void',
  'Deno.env.delete': '(k) → void',
  'Deno.env.has': '(k) → boolean',
  'Deno.env.toObject': '() → object',
  'Deno.permissions.query': '() → Promise<{ state }>',
  'Deno.Command': '(file, opts?) → Command',
  'Bun.file': '(path) → BunFile',
  'Bun.write': '(path, data) → Promise<number>',
  'Bun.spawn': '(file, opts?) → Subprocess',
  'Bun.spawnSync': '(file, opts?) → SyncSubprocess',
  'Bun.sleep': '(ms) → Promise<void>',
  'Bun.sleepSync': '(ms) → void',
  'Bun.nanoseconds': '() → number',
  'Bun.randomUUIDv7': '() → string',
  'Bun.sha': '(alg, data) → string',
  'Bun.serve': '(opts) → Promise<Server>',
  'Bun.listen': '(opts) → Server',
  'Bun.connect': '(opts) → Socket',
  'Bun.udpSocket': '() → UDPSocket',
  'Bun.fileURLToPath': '(url) → string',
  'Bun.pathToFileURL': '(path) → URL',
  'Bun.resolveSync': '(id, parent?) → string',
  'Bun.which': '(cmd) → string | null',
  'Bun.gc': '(force?) → void',
  'Bun.shrink': '() → void',
  'WinterJS.version': '() → string (getter)',
  'WinterJS.storage': 'WinterCG async KV',
  'WinterJS.args': 'string[]',
  'WinterJS.env': 'live env object',
  'WinterJS.fs': 'own file surface (fs === WinterJS.fs)',
  'WinterJS.memory': '() → { rss, allocator }',
  'WinterJS.alloc': '(size) → Uint8Array (zero-filled)',
  'WinterJS.unsafeAlloc': '(size) → id (--allow-ffi)',
  'WinterJS.unsafeSize': '(id) → size',
  'WinterJS.unsafeWrite': '(id, off, data) → void',
  'WinterJS.unsafeRead': '(id, off, len) → Uint8Array',
  'WinterJS.unsafeFree': '(id) → void',
  'WinterJS.unsafeList': '() → id[]',
  'fs.readFile': '(path) → Promise<Uint8Array>',
  'fs.readTextFile': '(path) → Promise<string>',
  'fs.writeFile': '(path, data) → Promise<void>',
  'fs.writeTextFile': '(path, text) → Promise<void>',
  'fs.stat': '(path) → Promise<{ isFile, isDirectory, size }>',
  'fs.mkdir': '(path, opts?) → Promise<void>',
  'fs.readdir': '(path) → Promise<DirEntry[]>',
  'fs.remove': '(path, opts?) → Promise<void>',
  'fs.rename': '(a, b) → Promise<void>',
  'fs.copyFile': '(a, b) → Promise<void>',
  'fs.exists': '(path) → Promise<boolean>',
  'WinterJS.image.formats': '() → { name, mime, decode, encode }[]',
  'WinterJS.image.info': '(bytes, format?) → { format, width, height, mime }',
  'WinterJS.image.decode': '(bytes, format?, scale?) → { format, width, height, data }',
  'WinterJS.image.encode': '({ data, width, height }, format, options?) → Uint8Array',
});

function __wjsReplCtorName(base) {
  try {
    if (typeof base === 'function' && base.name) return base.name;
    const p = Object.getPrototypeOf(base);
    return p && p.constructor && p.constructor.name ? p.constructor.name : '';
  } catch { return ''; }
}

// 签名摘要：描述符沿链安全读（不触发 getter）；函数先查 `__wjsReplSig`，
// 用户函数 toString 有真形参则直用；非函数给类型/值摘要。
// 描述符沿链安全读（不触发 getter；失败即 null）。
function __descOf(base, key) {
  if (base === null || base === undefined) return null;
  let d = null;
  let o = base;
  while (o !== null && o !== undefined && (typeof o === 'object' || typeof o === 'function')) {
    try { d = Object.getOwnPropertyDescriptor(o, key); } catch { return null; }
    if (d !== undefined && d !== null) break;
    try { o = Object.getPrototypeOf(o); } catch { return null; }
  }
  return (d === null || d === undefined) ? null : d;
}

function __sigDesc(base, key, head2) {
  const d = __descOf(base, key);
  if (d === null) return null;
  if (d.get !== undefined) return ': getter';
  const v = d.value;
  if (typeof v === 'function') {
    // 表优先：精确 dotted 路径（`console.log`）或 completed 名（bare `fetch`）；
    // 具体构造器（`URLSearchParams.get`，Ctor 非 Object）次之——原生短形参
    // （`get(n)`）不如一句话文档；普通对象（Ctor 为 Object）跳过此步，
    // 用户自有方法永远显示真相（4.227）。toString/Ctor 兜底。
    const exact = __wjsReplSig[head2] ?? __wjsReplSig[key];
    if (exact !== undefined) return exact;
    let src = '';
    try { src = Function.prototype.toString.call(v); } catch { /* ignore */ }
    const pm = /^[\s\S]*?\(([^)]*)\)/.exec(src);
    const ctor = __wjsReplCtorName(base);
    if (ctor !== '' && ctor !== 'Object') {
      const csig = __wjsReplSig[`${ctor}.${key}`];
      if (csig !== undefined) return csig;
    }
    if (pm !== null && pm[1] !== '') return `${key}(${pm[1]})`;
    const sig = __wjsReplSig[`${ctor}.${key}`];
    return sig ?? `${key}()`;
  }
  // 非函数：bare 命名空间给一句话（`crypto`），成员面沿用类型/值摘要。
  if (head2 === '') {
    const t = __wjsReplSig[key];
    if (t !== undefined) return t;
  }
  if (v === null) return ': null';
  if (typeof v === 'string') {
    const shown = v.length > 24 ? `${v.slice(0, 24)}…` : v;
    return `: "${shown}"`;
  }
  if (typeof v === 'number' || typeof v === 'boolean' || typeof v === 'bigint') return `: ${v}`;
  if (Array.isArray(v)) return ': []';
  if (typeof v === 'object') return ': {}';
  return `: ${typeof v}`;
}
"#;

/// TLA 包装（R6b；processTopLevelAwait 逐字移植 + 本桥）——**仅 REPL 会话注入**
/// （runtime/repl；acorn vendored 也在会话注入，PRELUDE 不带——全会话加载
/// 6827 行拖慢 worker/child 小窗口时序测试的启动）。
pub const REPL_TLA_JS: &str = r#"
// 末条顶层语句若非声明/return，改写为 `return { value: (expr) };`（node 原文
// 同款：包对象防 async 返回时对 Promise 值二次解包）。声明提升（`let a =
// await x` 跨行存活）为 node acorn AST 重写语义，另案拍板引包后逐字移植。
// processTopLevelAwait（node internal/repl/await.js 逐字移植，R6b；acorn 8.18.0）：
// 末表达式 return 化 + 顶层 let/const/var/class/function 声明提升（跨 async
// 边界存全局词法——`let a = await x` 跨行存活的正解）。primordials 按语义
// 直映原生方法；Recoverable（Unterminated 续行）CLI 由 validator 保证平衡，
// 统一抛 SyntaxError。acorn walk 用 recursive + 自定义 visitors（原文同款）。
const __wjsReplAwaitState = {
  containsAwait: false,
  containsReturn: false,
  body: null,
  ancestors: [],
  hoistedDeclarationStatements: [],
  // replace/prepend/append 由调用轮换绑（wrappedArray 持有）。
};
function __wjsAwaitIsTopLevelDeclaration(state) {
  return state.ancestors[state.ancestors.length - 2] === state.body;
}
const __wjsAwaitNoop = function () {};
const __wjsAwaitVisitorsWithoutAncestors = {
  ClassDeclaration(node, state, c) {
    if (__wjsAwaitIsTopLevelDeclaration(state)) {
      state.prepend(node, `${node.id.name}=`);
      state.hoistedDeclarationStatements.push(`let ${node.id.name}; `);
    }
    acornWalk.base.ClassDeclaration(node, state, c);
  },
  ForOfStatement(node, state, c) {
    if (node.await === true) state.containsAwait = true;
    acornWalk.base.ForOfStatement(node, state, c);
  },
  FunctionDeclaration(node, state, c) {
    state.prepend(node, `this.${node.id.name} = ${node.id.name}; `);
    state.hoistedDeclarationStatements.push(`var ${node.id.name}; `);
  },
  FunctionExpression: __wjsAwaitNoop,
  ArrowFunctionExpression: __wjsAwaitNoop,
  MethodDefinition: __wjsAwaitNoop,
  AwaitExpression(node, state, c) {
    state.containsAwait = true;
    acornWalk.base.AwaitExpression(node, state, c);
  },
  ReturnStatement(node, state, c) {
    state.containsReturn = true;
    acornWalk.base.ReturnStatement(node, state, c);
  },
  VariableDeclaration(node, state, c) {
    const variableKind = node.kind;
    const isIterableForDeclaration = ['ForOfStatement', 'ForInStatement']
      .includes(state.ancestors[state.ancestors.length - 2].type);
    if (variableKind === 'var' || __wjsAwaitIsTopLevelDeclaration(state)) {
      state.replace(
        node.start,
        node.start + variableKind.length + (isIterableForDeclaration ? 1 : 0),
        variableKind === 'var' && isIterableForDeclaration ? '' : 'void' + (node.declarations.length === 1 ? '' : ' ('),
      );
      if (!isIterableForDeclaration) {
        node.declarations.forEach((decl) => {
          state.prepend(decl, '(');
          state.append(decl, decl.init ? ')' : '=undefined)');
        });
        if (node.declarations.length !== 1) {
          state.append(node.declarations[node.declarations.length - 1], ')');
        }
      }
      const variableIdentifiersToHoist = [['var', []], ['let', []]];
      function registerVariableDeclarationIdentifiers(n) {
        switch (n.type) {
          case 'Identifier':
            variableIdentifiersToHoist[variableKind === 'var' ? 0 : 1][1].push(n.name);
            break;
          case 'ObjectPattern':
            n.properties.forEach((property) => {
              registerVariableDeclarationIdentifiers(property.value || property.argument);
            });
            break;
          case 'ArrayPattern':
            n.elements.forEach((element) => {
              registerVariableDeclarationIdentifiers(element);
            });
            break;
        }
      }
      node.declarations.forEach((decl) => registerVariableDeclarationIdentifiers(decl.id));
      variableIdentifiersToHoist.forEach(({ 0: kind, 1: identifiers }) => {
        if (identifiers.length > 0) {
          state.hoistedDeclarationStatements.push(`${kind} ${identifiers.join(', ')}; `);
        }
      });
    }
    acornWalk.base.VariableDeclaration(node, state, c);
  },
};
const __wjsAwaitVisitors = {};
for (const nodeType of Object.keys(acornWalk.base)) {
  const callback = __wjsAwaitVisitorsWithoutAncestors[nodeType] || acornWalk.base[nodeType];
  __wjsAwaitVisitors[nodeType] = (node, state, c) => {
    const isNew = node !== state.ancestors[state.ancestors.length - 1];
    if (isNew) state.ancestors.push(node);
    callback(node, state, c);
    if (isNew) state.ancestors.pop();
  };
}
function __wjsProcessTopLevelAwait(src) {
  const wrapPrefix = '(async () => { ';
  const wrapped = `${wrapPrefix}${src} })()`;
  const wrappedArray = wrapped.split('');
  let root;
  try {
    root = acorn.Parser.parse(wrapped, { ecmaVersion: 'latest' });
  } catch (e) {
    if (String(e.message).startsWith('Unterminated ')) return null;
    // 解析错在首个 await 之前 → 用执行错误（原码语义）；否则报本错（node 同款）。
    const awaitPos = src.indexOf('await');
    const errPos = e.pos - wrapPrefix.length;
    if (awaitPos > errPos) return null;
    if (errPos === awaitPos + 6 && String(e.message).includes('Expecting Unicode escape sequence')) return null;
    if (errPos === awaitPos + 7 && String(e.message).includes('Unexpected token')) return null;
    return null;
  }
  const body = root.body[0].expression.callee.body;
  const state = {
    body,
    ancestors: [],
    hoistedDeclarationStatements: [],
    replace(from, to, str) {
      for (let i = from; i < to; i++) wrappedArray[i] = '';
      if (from === to) str += wrappedArray[from];
      wrappedArray[from] = str;
    },
    prepend(node, str) {
      wrappedArray[node.start] = str + wrappedArray[node.start];
    },
    append(node, str) {
      wrappedArray[node.end - 1] += str;
    },
    containsAwait: false,
    containsReturn: false,
  };
  acornWalk.recursive(body, state, __wjsAwaitVisitors);
  // 无真 await / 顶层 return → 不改写（node 同款；null 走原码报错路径）。
  if (!state.containsAwait || state.containsReturn) return null;
  for (let i = body.body.length - 1; i >= 0; i--) {
    const node = body.body[i];
    if (node.type === 'EmptyStatement') continue;
    if (node.type === 'ExpressionStatement') {
      // 末表达式包 { value: (expr) }：防 async 返回对 Promise 值二次解包
      //（node await.js 同款注释语义）。
      state.prepend(node.expression, '{ value: (');
      state.prepend(node, 'return ');
      state.append(node.expression, ') }');
    }
    break;
  }
  return state.hoistedDeclarationStatements.join('') + wrappedArray.join('');
}
globalThis.__wjs_repl_tla_wrap = (src) => {
  src = String(src);
  if (!src.includes('await')) return null;
  const wrapped = __wjsProcessTopLevelAwait(src);
  if (wrapped === null) return null;
  // .then 双臂装标记对：rejected 不进 jobqueue 的 unhandled 收割；值解包
  // v?.value（node repl.js `(await promise)?.value` 同款——无 return 改写时
  // P1 resolve undefined，完成值即 undefined，node 同形）。
  return `${wrapped}.then(v => ({ __wjs_ok: 1, v: v === undefined || v === null ? undefined : v.value }),`
    + ' e => ({ __wjs_ok: 0, e }))';
};
"#;
