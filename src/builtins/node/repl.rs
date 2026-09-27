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
//! - 无 `useGlobal`（恒隔离上下文）、无预览/高亮/自动补全（无 completer 面）、
//!   无 `reset` 方法（`.clear` 命令等价，`reset` 事件照发）。

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
    this.useGlobal = false;
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
    this.commands = Object.create(null);
    this._defineBuiltins();
    this.rli = createInterface({
      input: this.input,
      output: this.output,
      terminal: this.terminal,
      prompt: this._prompt,
      historySize: options.historySize,
    });
    this.rli.on('line', (line) => this._onLine(line));
    this.rli.on('close', () => this._onRlClose());
  }
  _defineBuiltins() {
    const def = (keyword, help, action) => {
      this.commands[keyword] = { help, action: action.bind(this) };
    };
    def('help', 'Show repl options', function () {
      const names = Object.keys(this.commands);
      this._writeOut('Commands: ' + names.map((n) => '.' + n).join(', ') + '\n');
      for (const n of names) this._writeOut(`.${n}  ${this.commands[n].help}\n`);
      this.displayPrompt();
    });
    def('exit', 'Exit the repl', function () { this.close(); });
    def('break', 'Abort multiline input', function () {
      this._buffer = '';
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
  defineCommand(keyword, { help = '', action }) {
    this.commands[keyword] = { help, action: action.bind(this) };
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
  write(data) {
    return this.rli.write(data);
  }
  getPrompt() {
    return this._prompt;
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
export default { start, writer, REPLServer, REPL_MODE_SLOPPY, REPL_MODE_STRICT, Recoverable, isValidSyntax };
"#;
