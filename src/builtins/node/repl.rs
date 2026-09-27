//! `node:repl`（Node `lib/repl.js` 语义移植，MIT；plan 10c-3：骑 10c Interface）。
//!
//! 忠实面（真机 node 26.8.2 逐项对过）：`start(options)`/`REPLServer`（prompt/
//! 求值/打印/错误行）、`writer`（`node:util` inspect 口径）、
//! `REPL_MODE_SLOPPY/STRICT`（strict 包 `'use strict'` 前缀）、`Recoverable`
//! （自定义 eval 回传即续行）、`isValidSyntax`（编译试探）、续行提示符 `"| "`
//! （26 口径）、`displayPrompt`/`setPrompt`/`getPrompt`、`context`（vm 持久
//! 上下文，跨行绑定）、内建命令（`.help/.exit/.break/.clear/.save/.load`）、
//! `defineCommand`、`close` 即 `exit` 事件。
//!
//! 求值走 `node:vm`（`createContext` + `runInContext`；context 进 `r.context`）。
//! 不完整输入判定：`SyntaxError` 且文案命中
//! `/missing [\}\)\]]|got end of script|unterminated/i`（本引擎文案实测，
//! 见 10c-3）即续行（`.break` 可逃）；确定性语法错直接 `Uncaught`。
//!
//! 偏差（记档）：
//! - 错误行无 caret 定位行（`Uncaught Name: msg` 单行；真机另有 `^^^` 行）。
//! - context 为创建期快照（console/process/Buffer/定时器复制；之后全局变更
//!   不同步，vm 快照语义沿用）。
//! - 无 `useGlobal`（恒隔离上下文）、无预览/高亮（无 completer 面）、
//!   无 `reset` 方法（`.clear` 命令等价，`reset` 事件照发）。
//! - 补全为保守子集（P2-repl R3）：成员链/串数下标/fs 路径/bare 上下文键；
//!   调用·分组外结构一律拒答；路径求值 getter 拒入；Proxy 不可探测（
//!   `util.types.isProxy` 恒 false，引擎缺口）；bare 词法作用域不可枚举；
//!   unicode 标识符过滤缺口；`resetContext`/`setupHistory` 未做。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node lib/repl.js (see module docs for deviations).
import { EventEmitter } from 'node:events';
import { createInterface } from 'node:readline';
import { inspect } from 'node:util';
import * as vm from 'node:vm';
import * as fs from 'node:fs';
import errors from 'node:internal/errors';
import { getOptionValue } from 'node:internal/options';
import { builtinModules as __nodeBuiltinModules } from 'node:module';

// P2-repl R4：pendingDeprecation 门控（harness 以 --pending-deprecation 子进程重跑
// 废弃断言套件；D1 DEP0005 同款读旗）。
function __isPendingDeprecation() {
  try { return !!getOptionValue('--pending-deprecation'); }
  catch { return false; }
}
// P2-repl R4b：废弃警告按 CODE 进程去重（node codesWarned 口径）。
const __replWarnedCodes = new Set();
function __replDepWarn(code, msg) {
  if (!__isPendingDeprecation() || __replWarnedCodes.has(code)) return;
  __replWarnedCodes.add(code);
  try {
    process.emitWarning(msg, { type: 'DeprecationWarning', code });
  } catch { /* ignore */ }
}

export const REPL_MODE_SLOPPY = Symbol('repl-sloppy');
export const REPL_MODE_STRICT = Symbol('repl-strict');

export class Recoverable extends SyntaxError {
  constructor(err) {
    super(err !== undefined && err !== null && err.message !== undefined ? err.message : String(err));
    this.name = 'SyntaxError';
    if (err !== undefined && err !== null && err.stack !== undefined) this.stack = err.stack;
  }
}

export function isValidSyntax(code) {
  try {
    vm.createScript(String(code));
    return true;
  } catch {
    return false;
  }
}

const __INCOMPLETE = /missing [\}\)\]]|got end of script|unterminated/i;

function defaultWriter(value) {
  // P2-repl：node 口径 `(obj) => inspect(obj, writer.options)`（repl.js 244 行；
  // `writer.options` 可写，preview 套件改 colors 即此）。
  return inspect(value, defaultWriter.options);
}
defaultWriter.options = { ...inspect.defaultOptions, showProxy: true };

// P2-repl R3：保守子集补全（见 complete 方法注记）。
// P2-repl R3d：键枚举（node filteredOwnPropertyNames 口径子集：去数组下标、
// 去非标识符；unicode 标识符缺口记档，套件全 ASCII）。
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
function __ctxEval(expr, context) {
  // P2-repl R5：CLI 桥传 globalThis（非 vm context）——走全局间接 eval，
  // 与 CLI REPL 的经典脚本求值面同源（成员链/bare 键真上下文）。
  if (context === globalThis) return (0, eval)(expr);
  return vm.runInContext(expr, context, 'repl-completion');
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
function __walkSteps(parsed, context) {
  let obj;
  try { obj = __ctxEval(parsed.root, context); }
  catch { return null; }
  for (let si = 0; si < parsed.steps.length; si++) {
    const st = parsed.steps[si];
    const last = si === parsed.steps.length - 1;
    if (obj === null || obj === undefined) return null;
    // P2-repl R3b：末段允许原始值（`obj["one"].toFi` 的 Number 原型面）；
    // 中段恒对象（描述符步进）。
    if (typeof obj !== 'object' && typeof obj !== 'function') {
      if (!last) return null;
      obj = Object(obj);
    }
    let key;
    if (st.prop !== undefined) {
      key = st.prop;
    } else {
      // P2-repl R3c：仅调用形括号拒答（`f(`；分组/三元/算术求值，真机口径）；
      // 箭头/插值/赋值一律拒；tag 模板拒、纯模板放行。
      if (/[A-Za-z_$][\w$]*\s*\(|=>|\$\{|=/.test(st.key)) return null;
      const __wide = st.key.trim();
      if (!/^`(?:[^`\\]|\\.)*`$/.test(__wide) && __wide.includes('`')) return null;
      try { key = __ctxEval(st.key, context); }
      catch { return null; }
      if (typeof key !== 'string' && typeof key !== 'number') return null;
      key = String(key);
    }
    const d = __descAt(obj, key);
    if (d === null || d.get !== undefined || d.set !== undefined) return null;
    try { obj = obj[key]; }
    catch { return null; }
  }
  // 末值装箱再枚举（Number 原型面；getter 拒入已在描述符处截停）。
  if (obj === null || obj === undefined) return null;
  return Object(obj);
}
function __fsComplete(dir, prefix) {
  // P2-repl R3e：真机 fs 补全口径（allowBlockingCompletions 下实测）——
  // 既存目录即列子项裸名（completeOn 置空），否则同级前缀过滤裸名；
  // 坏径即空（completeOn 回前缀）。
  const base = dir === '' ? '.' : dir;
  const full = prefix === '' ? base : base + '/' + prefix;
  let isDir = false;
  try { isDir = fs.statSync(full).isDirectory(); }
  catch { /* ignore */ }
  if (isDir) {
    let names = [];
    try { names = fs.readdirSync(full); }
    catch { return [[], '']; }
    return [names.filter((n) => typeof n === 'string').sort(), ''];
  }
  let names = [];
  try { names = fs.readdirSync(base); }
  catch { return [[], prefix]; }
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
function __defaultComplete(context, line, callback) {
  const done = (list, completeOn) => callback(null, [list, completeOn]);
  if (typeof line !== 'string') line = String(line);
  const masked0 = __maskStrings(line);
  // 调用结果成员（`f().x`）恒拒答（nosideeffects 口径）。
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
    const obj = __walkSteps(parsed, context);
    if (obj === null) return 'walk-fail';
    // P2-repl R3c：过滤大小写不敏感（真机 `tofi`→`toFixed`），回显原键。
    const lowFilter = filter.toLowerCase();
    const list = __enumKeys(obj).filter((k) => k.toLowerCase().startsWith(lowFilter))
      .map((k) => `${base}.${k}`);
    return [list, `${base}.${filter}`];
  };
  let mr = tryMember(line);
  if (Array.isArray(mr)) return done(...mr);
  // 解析成形成立而求值失败（缺键/getter/不可达）即拒答，不穿透 bare。
  if (mr === 'walk-fail') return done([], line);
  // mr 为 null（无成员形）或 parse-fail：先试路径。
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
  // ③ 剥字面量后残留结构符（含赋值）即拒答。
  if (/[()=;{}=]|`/.test(masked0)) return done([], line);
  // ④ bare 词仅无点行（SM 全局键非枚举，走 getOwnPropertyNames；
  // 词法作用域不可枚举记档）。
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
}

function defaultEval(cmd, context, filename, callback) {
  let result;
  try {
    result = vm.runInContext(cmd, context, filename ?? 'repl');
  } catch (e) {
    callback(e);
    return;
  }
  callback(null, result);
}

export class REPLServer extends EventEmitter {
  // P2-repl：双形态（node lib/repl.js 口径）——options 形，或 legacy 位置形
  // (prompt, stream, eval, useGlobal, ignoreUndefined, replMode)。
  constructor(prompt = {}, stream, eval_, useGlobal, ignoreUndefined, replMode) {
    super();
    let options;
    if (prompt !== null && typeof prompt === 'object') {
      options = { ...prompt };
      stream = options.stream ?? options.socket;
      eval_ = options.eval;
      ignoreUndefined = options.ignoreUndefined;
      replMode = options.replMode;
      prompt = options.prompt;
    } else {
      options = {};
    }
    if (!options.input && !options.output) {
      // 双缺即 stdio（node 299 行；legacy duplex 取 stdin/stdout）。
      const stdioIn = stream?.stdin ?? stream ?? globalThis.process?.stdin;
      const stdioOut = stream?.stdout ?? stream ?? globalThis.process?.stdout;
      options.input = stdioIn ?? null;
      options.output = stdioOut ?? null;
      if (typeof prompt === 'string') options.prompt = prompt;
      if (typeof eval_ === 'function') options.eval = eval_;
      if (ignoreUndefined !== undefined) options.ignoreUndefined = ignoreUndefined;
      if (replMode !== undefined) options.replMode = replMode;
    } else if (typeof prompt === 'string' && options.prompt === undefined) {
      options.prompt = prompt;
    }
    // P2-repl R4：breakEvalOnSigint 与 eval 并存即抛（ERR_INVALID_REPL_EVAL_CONFIG）。
    if (options.breakEvalOnSigint && options.eval) {
      const err = new TypeError('Cannot specify both "breakEvalOnSigint" and "eval" for REPL');
      err.code = 'ERR_INVALID_REPL_EVAL_CONFIG';
      throw err;
    }
    this.input = options.input ?? null;
    this.output = options.output ?? null;
    this.terminal = options.terminal ?? (this.output != null ? !!this.output.isTTY : false);
    this._prompt = options.prompt ?? '> ';
    this._basePrompt = this._prompt;
    this._contPrompt = '| ';
    this._buffer = '';
    this._lines = [];
    this._exited = false;
    this.useColors = options.useColors ?? (this.output != null ? !!this.output.isTTY : false);
    this.ignoreUndefined = options.ignoreUndefined ?? false;
    this.replMode = options.replMode ?? REPL_MODE_SLOPPY;
    // P2-repl R4：useGlobal 仅存旗（真共享上下文另案；options 套件断旗）。
    this.useGlobal = options.useGlobal ?? false;
    const seed = {
      console: globalThis.console,
      process: globalThis.process,
      Buffer: globalThis.Buffer,
      setTimeout: globalThis.setTimeout.bind(globalThis),
      clearTimeout: globalThis.clearTimeout.bind(globalThis),
      setInterval: globalThis.setInterval.bind(globalThis),
      clearInterval: globalThis.clearInterval.bind(globalThis),
      queueMicrotask: globalThis.queueMicrotask.bind(globalThis),
    };
    this.context = vm.createContext(seed);
    const customEval = options.eval;
    if (typeof customEval === 'function') {
      this.eval = (cmd, context, filename, callback) =>
        customEval(cmd, context ?? this.context, filename ?? 'repl', callback);
    } else {
      this.eval = (cmd, context, filename, callback) => {
        let code = cmd;
        if (this.replMode === REPL_MODE_STRICT) code = '"use strict";\n' + code;
        defaultEval(code, context ?? this.context, filename ?? 'repl', callback);
      };
    }
    this.writer = options.writer ?? defaultWriter;
    // P2-repl R3：默认子集补全器；自定义同步返回形按 node 口径包回调形。
    // editor 模式结果收敛公共前缀（node completeOnEditorMode 口径）。
    const __custom = options.completer;
    if (typeof __custom === 'function') {
      this.completer = __custom.length !== 2
        ? ((line, cb) => cb(null, __custom(line)))
        : __custom;
    } else {
      this.completer = (line, cb) => {
        if (this._editorMode) {
          const orig = cb;
          cb = (err, data) => {
            if (err || !Array.isArray(data)) return orig(err, data);
            const [list, on] = data;
            if (!Array.isArray(list) || list.length === 0) return orig(err, data);
            orig(null, [[__commonPrefix(list)], on]);
          };
        }
        __defaultComplete(this.context, line, cb);
      };
    }
    this._editorMode = false;
    this._editorBuf = '';
    this.commands = Object.create(null);
    this._defineBuiltins();
    // P2-repl：editor 命令仅终端有（node 1454 行口径；列宽影响 help 版式）。
    if (this.terminal) {
      this.commands.editor = {
        help: 'Enter editor mode',
        action: (() => {
          this._editorMode = true;
          this._editorBuf = '';
          this._setPrompt('');
          this._writeOut('// Entering editor mode (Ctrl+D to finish, Ctrl+C to cancel)\n');
        }).bind(this),
      };
    }
    this.rli = createInterface({
      input: this.input,
      output: this.output,
      terminal: this.terminal,
      prompt: this._prompt,
      historySize: options.historySize,
    });
    this.rli.on('line', (line) => this._onLine(line));
    this.rli.on('close', () => this._onRlClose());
    // P2-repl R4：historySize 随 rli；inputStream/outputStream 废弃访问器（DEP0141 门控）。
    this.historySize = this.rli.historySize;
    Object.defineProperties(this, {
      inputStream: {
        get: () => {
          __replDepWarn('DEP0141',
            'repl.inputStream and repl.outputStream are deprecated. Use repl.input and repl.output instead');
          return this.input;
        },
        set: (v) => {
          __replDepWarn('DEP0141',
            'repl.inputStream and repl.outputStream are deprecated. Use repl.input and repl.output instead');
          this.input = v;
        },
        enumerable: false, configurable: true,
      },
      outputStream: {
        get: () => {
          __replDepWarn('DEP0141',
            'repl.inputStream and repl.outputStream are deprecated. Use repl.input and repl.output instead');
          return this.output;
        },
        set: (v) => {
          __replDepWarn('DEP0141',
            'repl.inputStream and repl.outputStream are deprecated. Use repl.input and repl.output instead');
          this.output = v;
        },
        enumerable: false, configurable: true,
      },
    });
  }
  _defineBuiltins() {
    const def = (keyword, help, action) => {
      this.commands[keyword] = { help, action: action.bind(this) };
    };
    def('help', 'Print this help message', function () {
      // P2-repl：node help 版式（排序 + 最长+3 空格 + 无 help 即裸名 + Ctrl 尾行）。
      const names = Object.keys(this.commands).sort();
      const longest = names.reduce((m, n) => Math.max(m, n.length), 0);
      for (const n of names) {
        const h = this.commands[n].help;
        this._writeOut(`.${n}${h ? ' '.repeat(longest - n.length + 3) + h : ''}\n`);
      }
      this._writeOut('\nPress Ctrl+C to abort current expression, Ctrl+D to exit the REPL\n');
      this.displayPrompt();
    });
    def('exit', 'Exit the repl', function () { this.close(); });
    def('break', 'Abort multiline input', function () {
      this._buffer = '';
      this._editorMode = false;
      this._editorBuf = '';
      this._setPrompt(this._prompt);
      this.displayPrompt();
    });
    def('clear', 'Reset the context', function () {
      const seed = {
        console: globalThis.console,
        process: globalThis.process,
        Buffer: globalThis.Buffer,
        setTimeout: globalThis.setTimeout.bind(globalThis),
        clearTimeout: globalThis.clearTimeout.bind(globalThis),
        setInterval: globalThis.setInterval.bind(globalThis),
        clearInterval: globalThis.clearInterval.bind(globalThis),
        queueMicrotask: globalThis.queueMicrotask.bind(globalThis),
      };
      this.context = vm.createContext(seed);
      this._buffer = '';
      this._setPrompt(this._prompt);
      this._writeOut('Clearing context...\n');
      this.emit('reset', this.context);
      this.displayPrompt();
    });
    // .save/.load：node lib/repl.js action 体逐字（缺参 ERR_MISSING_ARGS 文案、非文件 /
    // 失败分支文案、每次收尾 displayPrompt）。load 体仍逐行喂 _onLine（无 editor 模式）。
    const missingFile = () => new errors.codes.ERR_MISSING_ARGS('file');
    def('save', 'Save all evaluated commands in this REPL session to a file', function (file) {
      try {
        if (file === '') {
          throw missingFile();
        }
        fs.writeFileSync(file, this._lines.join('\n'));
        this.output.write(`Session saved to: ${file}\n`);
      } catch (error) {
        if (error !== null && typeof error === 'object' && error.code === 'ERR_MISSING_ARGS') {
          this.output.write(`${error.message}\n`);
        } else {
          this.output.write(`Failed to save: ${file}\n`);
        }
      }
      this.displayPrompt();
    });
    def('load', 'Load JS from a file into the REPL session', function (file) {
      try {
        if (file === '') {
          throw missingFile();
        }
        const stats = fs.statSync(file);
        if (stats && stats.isFile()) {
          const data = fs.readFileSync(file, 'utf8');
          for (const line of data.split('\n')) {
            if (line !== '') this._onLine(line);
          }
        } else {
          this.output.write(
            `Failed to load: ${file} is not a valid file\n`,
          );
        }
      } catch (error) {
        if (error !== null && typeof error === 'object' && error.code === 'ERR_MISSING_ARGS') {
          this.output.write(`${error.message}\n`);
        } else {
          this.output.write(`Failed to load: ${file}\n`);
        }
      }
      this.displayPrompt();
    });
  }
  // node 口径：`replServer.lines` 为已求值行（.save 落盘源）。
  get lines() {
    return this._lines;
  }
  defineCommand(keyword, cmd) {
    // P2-repl：node 口径（函数即 { action }；对象形校验 action 可调）。
    if (typeof cmd === 'function') {
      cmd = { action: cmd };
    } else if (cmd === null || cmd === undefined || typeof cmd.action !== 'function') {
      const err = new TypeError('The "cmd.action" property must be of type function');
      err.code = 'ERR_INVALID_ARG_TYPE';
      throw err;
    }
    this.commands[keyword] = { help: cmd.help ?? '', action: cmd.action.bind(this) };
  }
  _writeOut(s) {
    if (this.output !== null && this.output !== undefined) {
      try { this.output.write(s); } catch { /* ignore */ }
    }
  }
  _setPrompt(p) {
    this._prompt = p;
    try { this.rli.setPrompt(p); } catch { /* ignore */ }
  }
   setPrompt(prompt) {
    this._basePrompt = String(prompt);
    this._setPrompt(this._basePrompt);
  }
  // P2-repl：喂入行（node Interface.write 口径；`start()` 无 input 套件靠它驱动）。
  // editor 模式 C-d 即求值收尾（save-load-editor-mode 口径）。
  write(data, key) {
    if (key !== null && key !== undefined && typeof key === 'object' &&
        key.ctrl === true && key.name === 'd' && this._editorMode) {
      this._finishEditor();
      return undefined;
    }
    return this.rli.write(data, key);
  }
  _finishEditor() {
    const buf = this._editorBuf;
    this._editorBuf = '';
    this._editorMode = false;
    this.eval(buf, this.context, 'repl', (err, result) => {
      if (err !== null && err !== undefined) {
        this._printError(err);
      } else if (!(result === undefined && this.ignoreUndefined)) {
        this._writeOut(this.writer(result) + '\n');
      }
      // node editor 收尾附一空行（lines 尾 '' 使 .save 落盘带末换行）。
      this._lines.push('');
      this._setPrompt(this._basePrompt);
      this.displayPrompt();
    });
  }
  getPrompt() {
    return this._prompt;
  }
  // P2-repl R3b：行镜像（node REPLServer 承 Interface 面；save-load 套件读 `repl.line`）。
  get line() {
    return this.rli.line;
  }
  set line(v) {
    this.rli.line = v;
  }
  // P2-repl R3：保守子集补全（node internal/repl/completion 口径子集，无 acorn）。
  // 规则（按序）：① 未闭合串内即 fs 路径补全；② 成员链（标识/串/数下标，
  // 方括号表达式求值，含调用/分组即拒答）；③ 剥除字面量后残留 `()=;{}` 反引号
  // 即拒答（nosideeffects 口径）；④ new 前缀剥除后走②；⑤ bare 词凑上下文键。
  // 路径求值逐段经描述符（getter/setter 拒入，不读值）；Proxy 不可探测记档；
  // bare 词法作用域不可枚举记档。
  complete(...args) {
    return Reflect.apply(this.completer, this, args);
  }
  displayPrompt(preserveCursor) {
    try { this.rli.prompt(preserveCursor); } catch { /* ignore */ }
  }
  _resetBuffer() {
    this._buffer = '';
    this._setPrompt(this._basePrompt);
  }
  _printError(err) {
    const name = (err !== null && err !== undefined && err.name !== undefined) ? err.name : 'Error';
    const message = (err !== null && err !== undefined && err.message !== undefined) ? err.message : String(err);
    this._writeOut(`Uncaught ${name}: ${message}\n`);
  }
  _onLine(line) {
    const trimmed = line.trim();
    // P2-repl：editor 模式逐行缓冲（点行仍走普通分支以便 .break/.exit 可逃）。
    if (this._editorMode && !(this._buffer === '' && trimmed.startsWith('.'))) {
      this._editorBuf += (this._editorBuf !== '' ? '\n' : '') + line;
      this._lines.push(line);
      return;
    }
    if (this._buffer === '' && trimmed.startsWith('.')) {
      const [keyword, ...rest] = trimmed.slice(1).split(/\s+/);
      const cmd = this.commands[keyword];
      if (cmd !== undefined) {
        cmd.action(rest.join(' '));
        return;
      }
      this._writeOut('Invalid REPL keyword\n');
      this.displayPrompt();
      return;
    }
    this._buffer += (this._buffer !== '' ? '\n' : '') + line;
    this._lines.push(line);
    this.eval(this._buffer + '\n', this.context, 'repl', (err, result) => {
      if (err !== null && err !== undefined) {
        if (err instanceof Recoverable ||
            (err.name === 'SyntaxError' && __INCOMPLETE.test(err.message ?? ''))) {
          this._setPrompt(this._contPrompt);
          this.displayPrompt();
          return;
        }
        this._printError(err);
        this._resetBuffer();
        this.displayPrompt();
        return;
      }
      if (result === undefined && this.ignoreUndefined) {
        // 跳过 undefined 回显（缺省全打，真机口径）。
      } else {
        this._writeOut(this.writer(result) + '\n');
      }
      this._resetBuffer();
      this.displayPrompt();
    });
  }
  _onRlClose() {
    if (!this._exited) {
      this._exited = true;
      this.emit('exit');
    }
  }
  close() {
    try { this.rli.close(); } catch { /* ignore */ }
    this._onRlClose();
  }
}

export function start(prompt, source, eval_, useGlobal, ignoreUndefined, replMode) {
  const r = new REPLServer(prompt, source, eval_, useGlobal, ignoreUndefined, replMode);
  r.displayPrompt();
  return r;
}

export const writer = defaultWriter;

// P2-repl R5：CLI reedline 补全桥（同步返回形）——同款子集规则（成员链/fs
// 路径/bare 上下文键），求值面为全局（见 __ctxEval 的 globalThis 分支），
// CLI REPL 与 node:repl 模块补全同源。返回 [list, completeOn]：list 元素为
// 应写入的完整文本，completeOn 为行尾被替换段（CLI 侧换算 reedline span）。
export function cliComplete(line) {
  let out = null;
  __defaultComplete(globalThis, line, (err, r) => { out = r; });
  return out ?? [[], String(line)];
}
// P2-repl R4：模块级废弃表（DEP0142/DEP0191 门控；值取 node:module 全集）。
let __replBuiltinOverride = null;
// cliComplete 同挂默认导出（4.218：具名导出≠默认导出；CLI 桥经 require 取用）。
const __defaultExport = { start, writer, REPLServer, REPL_MODE_SLOPPY, REPL_MODE_STRICT, Recoverable, isValidSyntax, cliComplete };
Object.defineProperties(__defaultExport, {
  builtinModules: {
    get: () => {
      __replDepWarn('DEP0191',
        'repl.builtinModules is deprecated. Check module.builtinModules instead');
      return __replBuiltinOverride ?? [...__nodeBuiltinModules];
    },
    set: (v) => {
      __replDepWarn('DEP0191',
        'repl.builtinModules is deprecated. Check module.builtinModules instead');
      __replBuiltinOverride = v;
    },
    enumerable: false, configurable: true,
  },
  _builtinLibs: {
    get: () => {
      __replDepWarn('DEP0142',
        'repl._builtinLibs is deprecated. Check module.builtinModules instead');
      return __replBuiltinOverride ?? [...__nodeBuiltinModules];
    },
    set: (v) => {
      __replDepWarn('DEP0142',
        'repl._builtinLibs is deprecated. Check module.builtinModules instead');
      __replBuiltinOverride = v;
    },
    enumerable: false, configurable: true,
  },
});
export default __defaultExport;
"#;
