//! winterjs REPL 底座·补全面（2026-09-28 R5 方向纠正：CLI REPL 是本体，
//! node:repl 兼容面反向复用底座；CLI 专属能力一律住本域，禁进 node:* 公开导出面）。
//!
//! - `__wjs_cli_complete(line)`：CLI reedline Tab 的补全桥——骑
//!   `globalThis.__wjs_repl_default_complete`（`node:repl` 模块加载时注册的
//!   R3 子集补全核心：成员链逐步求值/fs 路径/bare 上下文键/大小写不敏感；
//!   `require('node:repl')` 幂等触发加载），对 CLI 全局求值面工作，
//!   返回 `[[全文, 描述], ...], completeOn`（描述进 IdeMenu 右侧 pane）。
//! - 签名摘要：SM native `toString()` 不带形参名（实测空括号），常用面
//!   手写 `__SIG` 表；用户函数抽 toString 真形参；非函数给类型/值摘要
//!   （描述符沿链安全读，不触发 getter）。
pub const REPL_COMPLETE_JS: &str = r#"
// ---- winterjs repl 底座：CLI 补全桥（__wjs_ 内部面；调用时才骑模块核心）----
globalThis.__wjs_cli_complete = (line) => {
  require('node:repl'); // 幂等：触发 node:repl 模块加载 → 注册补全核心
  const core = globalThis.__wjs_repl_default_complete;
  if (typeof core !== 'function') return [[], String(line)];
  let out = null;
  try { core(globalThis, String(line), (err, r) => { out = r; }); } catch { return [[], String(line)]; }
  if (out === null || !Array.isArray(out)) return [[], String(line)];
  const [list, completeOn] = out;
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
    return [text, __sigDesc(base, key, head2)];
  });
  return [withSig, completeOn];
};

// ---- 成员签名表（SM native toString 无形参名，常用面手写）----
// null 原型（查表裸键不得沿原型链撞 Object.prototype 的同名方法——
// propertyIsEnumerable/toString 等 hits 自身，实测踩过）。
// 匹配序：候选尾二段（`globalThis.Object.assign` → `Object.assign`）→
// constructor 名（实例面 `"".trim` → `String.trim`）→ 裸方法名。
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
  'URL.canParse': '(url, base?) → boolean',
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
function __sigDesc(base, key, head2) {
  if (base === null || base === undefined) return null;
  let d = null;
  let o = base;
  while (o !== null && o !== undefined && (typeof o === 'object' || typeof o === 'function')) {
    try { d = Object.getOwnPropertyDescriptor(o, key); } catch { return null; }
    if (d !== undefined && d !== null) break;
    try { o = Object.getPrototypeOf(o); } catch { return null; }
  }
  if (d === null || d === undefined) return null;
  if (d.get !== undefined) return ': getter';
  const v = d.value;
  if (typeof v === 'function') {
    let src = '';
    try { src = Function.prototype.toString.call(v); } catch { /* ignore */ }
    const pm = /^[\s\S]*?\(([^)]*)\)/.exec(src);
    if (pm !== null && pm[1] !== '') return `${key}(${pm[1]})`;
    const sig = __wjsReplSig[head2] ?? __wjsReplSig[`${__wjsReplCtorName(base)}.${key}`] ?? __wjsReplSig[key];
    return sig ?? `${key}()`;
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
