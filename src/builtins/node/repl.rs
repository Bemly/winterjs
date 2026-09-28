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
//! - 补全为保守子集（R3 口径，核心住 winterjs 底座 prelude，本模块薄包
//!   注入 vm 求值器反向复用）：成员链/串数下标/fs 路径/bare 上下文键；
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

// 补全薄壳（本体拥有核心）：调 winterjs 底座
// `globalThis.__wjs_repl_default_complete`（prelude/repl_complete），
// 注入 vm 上下文求值器；公开面保持 node 真机同形。
// R3 保守子集口径见底座（成员链/串数下标/fs 路径/bare 上下文键；
// 调用·分组外结构拒答；getter 拒入；Proxy 不可探测记档）。
function __commonPrefix(list) {
  if (typeof globalThis.__wjs_repl_common_prefix === 'function') {
    try { return globalThis.__wjs_repl_common_prefix(list); } catch { /* fallthrough */ }
  }
  if (!Array.isArray(list) || list.length === 0) return '';
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
function __defaultComplete(context, line, callback) {
  const core = globalThis.__wjs_repl_default_complete;
  const s = typeof line === 'string' ? line : String(line);
  if (typeof core !== 'function') {
    callback(null, [[], s]);
    return;
  }
  const evalFn = (expr, ctx) => vm.runInContext(expr, ctx ?? context, 'repl-completion');
  try {
    core(context, s, callback, evalFn);
  } catch {
    callback(null, [[], s]);
  }
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

// P2-repl R4：模块级废弃表（DEP0142/DEP0191 门控；值取 node:module 全集）。
let __replBuiltinOverride = null;
const __defaultExport = { start, writer, REPLServer, REPL_MODE_SLOPPY, REPL_MODE_STRICT, Recoverable, isValidSyntax };
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
