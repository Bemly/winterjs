//! `node:readline`（Node `lib/readline.js` 语义移植，MIT；plan 9j：vite 顶层 import）。
//!
//! 忠实面（真机 node 26.8.2 逐项对过）：`createInterface`（options/positional 双形态；
//! 无 input 即无码 TypeError）、行提交（`line` 事件/question/异步迭代器）、
//! history（terminal 下 recent-first、去空去连续重、historySize 上限）、
//! `emitKeypressEvents`（按键形状逐项对拍：sequence/name/ctrl/meta/shift/code）、
//! 终端编辑 Emacs 子集（见下）、pause/resume 转调 + 标志、`close`（input end
//! 自关/幂等）、`prompt`/`setPrompt` 回 undefined。
//!
//! Emacs 子集（terminal 下按键驱动）：可打印插入、Backspace/C-h 左删、
//! Delete/C-d 右删（空行 C-d 即关）、C-u 整行杀、C-k 杀至行尾、C-a/C-e 行首尾、
//! C-b/C-f 左右、C-n/C-p 历史、Up/Down 历史、Home/End、Enter 提交、
//! C-c（SIGINT 有监听则发、无则关）、C-l 清屏重绘；Tab（无 completer）忽略，
//! 其余 ctrl/meta 忽略。
//! 非终端 = 纯行 splitter（`\r\n`/`\n`；不碰 history/line/cursor，真机同款）。
//!
//! 偏差（记档）：
//! - 无 completer（存参不用；Tab 恒忽略）；无 SIGTSTP/SIGCONT（C-z 忽略）。
//! - `write(data, key)` 有 key 时只走按键动作（真机 `write("ab",{left})` 无输出同款）。
//! - 历史回退到顶丢弃编辑中行（真机保留，角落差）。
//! - 行渲染为简化重绘（`\r` + prompt + 行 + 光标回退 + 清尾），包级字节与真机未逐字对拍。

/// 内嵌 ESM 源（零 native，纯形）。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node lib/readline.js (see module docs for deviations).
import { EventEmitter } from 'node:events';
import errors from 'node:internal/errors';

const {
  codes: {
    ERR_INVALID_ARG_TYPE,
  },
} = errors;

// ── 按键解码（真机按键形状逐项对拍）────────────────────────────────────
// 事件：emit('keypress', s, key)，key = { sequence, name, ctrl, meta, shift, code? }。
// s：可打印/单字节即原文；`\x1b[`/`\x1bO` 转义形即 undefined。
const __ESC = '\x1b';
function __ctrlName(code) {
  return String.fromCharCode(code + 96);
}
function __decodeKeys(buf, state) {
  // buf: 待解字节数组；state: { esc: 待定转义串 }。返回 { evs, rest }，
  // rest 为未消费尾（截断 UTF-8），由调用方留待下包。
  const evs = [];
  let i = 0;
  const takeUtf8 = () => {
    const b0 = buf[i];
    let len = 1;
    if ((b0 & 0x80) === 0) len = 1;
    else if ((b0 & 0xe0) === 0xc0) len = 2;
    else if ((b0 & 0xf0) === 0xe0) len = 3;
    else if ((b0 & 0xf8) === 0xf0) len = 4;
    else { i++; return { sequence: String.fromCharCode(b0), name: String.fromCharCode(b0), ctrl: false, meta: false, shift: false }; }
    if (i + len > buf.length) return null;
    const slice = buf.slice(i, i + len);
    i += len;
    const sequence = new TextDecoder().decode(new Uint8Array(slice));
    return { sequence, name: sequence, ctrl: false, meta: false, shift: false };
  };
  while (i < buf.length) {
    if (state.esc !== '') {
      state.esc += String.fromCharCode(buf[i]);
      const e = state.esc;
      const mBracket = /^\x1b\[([0-9;]*)([A-Za-z~])$/.exec(e);
      const mO = /^\x1bO([A-HF])$/.exec(e);
      if (mBracket !== null || mO !== null) {
        const hasSemi = e.includes(';');
        state.esc = '';
        i++; // 消费终字节（漏加即重解一遍，10c-2 实测抓到）
        if (hasSemi) continue; // 修饰形（`[1;5C` 类）真机无事件，同款吞掉
        let name, code;
        if (mO !== null) {
          const f = mO[1];
          name = { A: 'up', B: 'down', C: 'right', D: 'left', H: 'home', F: 'end' }[f];
          code = 'O' + f;
        } else {
          const params = mBracket[1], fin = mBracket[2];
          code = '[' + params + fin;
          if (fin === 'A') name = 'up';
          else if (fin === 'B') name = 'down';
          else if (fin === 'C') name = 'right';
          else if (fin === 'D') name = 'left';
          else if (fin === 'H') name = 'home';
          else if (fin === 'F') name = 'end';
          else if (fin === 'Z') name = 'tab';
          else if (fin === '~') {
            name = { 1: 'home', 2: undefined, 3: 'delete', 4: 'end', 5: 'pageup', 6: 'pagedown', 7: 'home', 8: 'end' }[params];
          }
        }
        if (name === undefined) continue;
        const key = { sequence: e, name, ctrl: false, meta: false, shift: finOf(e) === 'Z' };
        if (code !== undefined) key.code = code;
        evs.push([undefined, key]);
        continue;
      }
      if (/^\x1b\[[0-9;]*$/.test(e) || e === '\x1b[' || e === '\x1bO' || e === '\x1b') {
        i++;
        continue; // 等更多字节
      }
      if (e.length === 2 && e[0] === '\x1b') {
        // meta + 单字符
        const ch = e[1];
        state.esc = '';
        i++;
        evs.push([e, { sequence: e, name: ch, ctrl: false, meta: true, shift: false }]);
        continue;
      }
      // 不是转义：吐出 `\x1b` 本体，余下重解
      state.esc = '';
      evs.push(['\x1b', { sequence: '\x1b', name: 'escape', ctrl: false, meta: false, shift: false }]);
      continue;
    }
    const b = buf[i];
    if (b === 0x1b) {
      state.esc = '\x1b';
      i++;
      continue;
    }
    if (b === 0x0d) {
      i++;
      evs.push(['\r', { sequence: '\r', name: 'return', ctrl: false, meta: false, shift: false }]);
      continue;
    }
    if (b === 0x0a) {
      i++;
      evs.push(['\n', { sequence: '\n', name: 'enter', ctrl: false, meta: false, shift: false }]);
      continue;
    }
    if (b === 0x09) {
      i++;
      evs.push(['\t', { sequence: '\t', name: 'tab', ctrl: false, meta: false, shift: false }]);
      continue;
    }
    if (b === 0x7f) {
      i++;
      evs.push(['\x7f', { sequence: '\x7f', name: 'backspace', ctrl: false, meta: false, shift: false }]);
      continue;
    }
    if (b >= 0x01 && b <= 0x1a) {
      i++;
      const ch = String.fromCharCode(b);
      evs.push([ch, { sequence: ch, name: __ctrlName(b), ctrl: true, meta: false, shift: false }]);
      continue;
    }
    const k = takeUtf8();
    if (k === null) break; // 等更多字节
    evs.push([k.sequence, k]);
  }
  return { evs, rest: buf.slice(i) };
}
function finOf(e) {
  const m = /^\x1b\[([0-9;]*)([A-Za-z~])$/.exec(e);
  return m !== null ? m[2] : '';
}

function emitKeypressEvents(stream, iface) {
  if (stream === undefined || stream === null || typeof stream.on !== 'function') {
    throw new ERR_INVALID_ARG_TYPE('stream', 'object', stream);
  }
  if (stream.__wjsKeypress) return undefined;
  const state = { esc: '', pending: [] };
  stream.__wjsKeypress = state;
  const feed = (chunk) => {
    const bytes = typeof chunk === 'string'
      ? Array.from(new TextEncoder().encode(chunk))
      : Array.from(chunk instanceof Uint8Array ? chunk
        : chunk instanceof ArrayBuffer ? new Uint8Array(chunk)
        : ArrayBuffer.isView(chunk) ? new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength)
        : new TextEncoder().encode(String(chunk)));
    const all = state.pending.concat(bytes);
    state.pending = [];
    const { evs, rest } = __decodeKeys(all, state);
    state.pending = rest;
    for (const [s, key] of evs) {
      stream.emit('keypress', s, key);
    }
  };
  stream.on('data', feed);
  return undefined;
}

class Interface extends EventEmitter {
  constructor(input, output, completer, terminal) {
    super();
    this.input = input ?? null;
    this.output = output ?? null;
    this.completer = typeof completer === 'function' ? completer : undefined;
    this.terminal = !!terminal;
    this.history = [];
    this.historySize = 30;
    this._prompt = '';
    this.closed = false;
    this.paused = false;
    this.line = '';
    this.cursor = terminal ? 0 : undefined;
    this._historyIndex = -1;
    this._savedLine = '';
    this._questionCb = null;
    this._lineBuf = '';
    this._iterWaiters = [];
    this._onData = (b) => this._onInputData(b);
    this._onKey = (s, k) => this._ttyWrite(s, k);
    this._onEnd = () => this.close();
    if (this.terminal) {
      emitKeypressEvents(input, this);
      input.on('keypress', this._onKey);
    } else {
      input.on('data', this._onData);
    }
    input.on('end', this._onEnd);
  }
  // ── dumb 模式：纯行 splitter ──
  _onInputData(chunk) {
    if (this.closed) return;
    const s = typeof chunk === 'string' ? chunk : new TextDecoder().decode(
      chunk instanceof Uint8Array ? chunk
        : chunk instanceof ArrayBuffer ? new Uint8Array(chunk)
        : ArrayBuffer.isView(chunk) ? new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength)
        : new TextEncoder().encode(String(chunk)));
    this._lineBuf += s;
    if (this.paused) return;
    this._drainLines();
  }
  _drainLines() {
    const parts = this._lineBuf.split(/\r?\n/);
    this._lineBuf = parts.pop();
    for (const line of parts) this._submit(line);
  }
  // ── 提交（两模式共用）──
  _submit(line) {
    if (this.terminal) {
      if (line.length > 0 && this.history[0] !== line) {
        this.history.unshift(line);
        if (this.history.length > this.historySize) this.history.pop();
      }
      this._historyIndex = -1;
      this._savedLine = '';
      if (this.output !== null) {
        try { this.output.write('\r\n'); } catch { /* ignore */ }
      }
    }
    this.line = '';
    this.cursor = this.terminal ? 0 : undefined;
    const cb = this._questionCb;
    this._questionCb = null;
    if (cb !== null && cb !== undefined) {
      cb(line);
    } else {
      this.emit('line', line);
      this._feedIter(line);
    }
  }
  _feedIter(line) {
    if (this._iterWaiters.length > 0) {
      const w = this._iterWaiters.shift();
      w(line);
    } else {
      if (this._iterBuf === undefined) this._iterBuf = [];
      this._iterBuf.push(line);
    }
  }
  // ── 终端编辑 ──
  _writeOut(s) {
    if (this.output !== null && this.output !== undefined) {
      try { this.output.write(s); } catch { /* ignore */ }
    }
  }
  _refreshLine() {
    if (this.output === null || this.output === undefined) return;
    const back = this.line.length - this.cursor;
    this._writeOut('\r' + this._prompt + this.line + '\x1b[K' +
      (back > 0 ? `\x1b[${back}D` : ''));
  }
  _insert(s) {
    this.line = this.line.slice(0, this.cursor) + s + this.line.slice(this.cursor);
    this.cursor += s.length;
    this._refreshLine();
  }
  _deleteLeft() {
    if (this.cursor === 0) return;
    this.line = this.line.slice(0, this.cursor - 1) + this.line.slice(this.cursor);
    this.cursor--;
    this._refreshLine();
  }
  _deleteRight() {
    if (this.cursor >= this.line.length) return;
    this.line = this.line.slice(0, this.cursor) + this.line.slice(this.cursor + 1);
    this._refreshLine();
  }
  _historyGo(dir) {
    if (dir > 0) {
      if (this._historyIndex + 1 >= this.history.length) return;
      if (this._historyIndex === -1) this._savedLine = this.line;
      this._historyIndex++;
      this.line = this.history[this._historyIndex];
    } else {
      if (this._historyIndex === -1) return;
      this._historyIndex--;
      this.line = this._historyIndex === -1 ? this._savedLine : this.history[this._historyIndex];
    }
    this.cursor = this.line.length;
    this._refreshLine();
  }
  _ttyWrite(s, key) {
    if (this.closed || this.paused || key === undefined) return;
    if (key.ctrl) {
      const n = key.name;
      if (n === 'c') {
        if (this.listenerCount('SIGINT') > 0) this.emit('SIGINT');
        else this.close();
        return;
      }
      if (n === 'd') {
        if (this.line.length === 0) { this.close(); return; }
        this._deleteRight();
        return;
      }
      if (n === 'h') { this._deleteLeft(); return; }
      if (n === 'u') { this.line = ''; this.cursor = 0; this._refreshLine(); return; }
      if (n === 'k') { this.line = this.line.slice(0, this.cursor); this._refreshLine(); return; }
      if (n === 'a') { this.cursor = 0; this._refreshLine(); return; }
      if (n === 'e') { this.cursor = this.line.length; this._refreshLine(); return; }
      if (n === 'b') { if (this.cursor > 0) this.cursor--; this._refreshLine(); return; }
      if (n === 'f') { if (this.cursor < this.line.length) this.cursor++; this._refreshLine(); return; }
      if (n === 'n') { this._historyGo(1); return; }
      if (n === 'p') { this._historyGo(-1); return; }
      if (n === 'l') { this._writeOut('\x1b[2J\x1b[0;0H'); this._refreshLine(); return; }
      if (n === 'w') {
        const left = this.line.slice(0, this.cursor);
        const cut = left.replace(/[^\s]+\s*$/, '');
        this.line = cut + this.line.slice(this.cursor);
        this.cursor = cut.length;
        this._refreshLine();
        return;
      }
      return;
    }
    if (key.meta) return;
    switch (key.name) {
      case 'return':
      case 'enter': {
        const line = this.line;
        this.line = '';
        this.cursor = 0;
        this._submit(line);
        return;
      }
      case 'backspace': this._deleteLeft(); return;
      case 'delete': this._deleteRight(); return;
      case 'left': if (this.cursor > 0) this.cursor--; this._refreshLine(); return;
      case 'right': if (this.cursor < this.line.length) this.cursor++; this._refreshLine(); return;
      case 'home': this.cursor = 0; this._refreshLine(); return;
      case 'end': this.cursor = this.line.length; this._refreshLine(); return;
      case 'up': this._historyGo(1); return;
      case 'down': this._historyGo(-1); return;
      case 'tab': return;
      default:
        if (typeof s === 'string') this._insert(s);
        return;
    }
  }
  setPrompt(prompt) {
    this._prompt = String(prompt);
  }
  getPrompt() {
    return this._prompt;
  }
  prompt(preserveCursor) {
    if (this.output !== null && this.output !== undefined) {
      try { this.output.write(this._prompt); } catch { /* ignore */ }
    }
    return undefined;
  }
  question(query, cb) {
    if (this.output !== null && this.output !== undefined) {
      try { this.output.write(String(query)); } catch { /* ignore */ }
    }
    this._questionCb = cb ?? null;
  }
  write(data, key) {
    if (key !== undefined && key !== null) {
      this._ttyWrite(undefined, typeof key === 'object' ? key : { name: String(key) });
      return undefined;
    }
    if (this.terminal) {
      if (data !== undefined && data !== null) this._insert(String(data));
    } else {
      if (data !== undefined && data !== null) this._lineBuf += String(data);
    }
    return undefined;
  }
  pause() {
    if (!this.paused) {
      this.paused = true;
      try { this.input.pause?.(); } catch { /* ignore */ }
      this.emit('pause');
    }
    return this;
  }
  resume() {
    if (this.paused) {
      this.paused = false;
      try { this.input.resume?.(); } catch { /* ignore */ }
      if (!this.terminal) this._drainLines();
      this.emit('resume');
    }
    return this;
  }
  close() {
    if (this.closed) return undefined;
    this.closed = true;
    try { this.input.removeListener('data', this._onData); } catch { /* ignore */ }
    try { this.input.removeListener('keypress', this._onKey); } catch { /* ignore */ }
    try { this.input.removeListener('end', this._onEnd); } catch { /* ignore */ }
    for (const w of this._iterWaiters.splice(0)) w(null);
    this.emit('close');
    return undefined;
  }
  [Symbol.asyncIterator]() {
    return {
      next: () => {
        if (this._iterBuf !== undefined && this._iterBuf.length > 0) {
          return Promise.resolve({ value: this._iterBuf.shift(), done: false });
        }
        if (this.closed) return Promise.resolve({ value: undefined, done: true });
        return new Promise((resolve) => {
          this._iterWaiters.push((line) => {
            if (line === null) resolve({ value: undefined, done: true });
            else resolve({ value: line, done: false });
          });
        });
      },
      return: () => {
        this.close();
        return Promise.resolve({ value: undefined, done: true });
      },
    };
  }
}

function createInterface(input, output, completer, terminal) {
  // options 形：首参含 input/terminal/completer/prompt/historySize 任一即判 options。
  if (input !== undefined && input !== null && typeof input === 'object' && !Array.isArray(input) &&
      (input.input !== undefined || input.terminal !== undefined || input.completer !== undefined ||
       input.prompt !== undefined || input.historySize !== undefined)) {
    const o = input;
    input = o.input;
    output = o.output ?? output;
    completer = o.completer ?? completer;
    terminal = o.terminal ?? terminal;
    var prompt = o.prompt;
    var historySize = o.historySize;
  }
  if (input === undefined || input === null || typeof input.on !== 'function') {
    throw new TypeError('input.on is not a function');
  }
  if (terminal === undefined) {
    terminal = output != null ? !!output.isTTY : !!input.isTTY;
  }
  const rl = new Interface(input, output, completer, terminal);
  if (prompt !== undefined) rl.setPrompt(prompt);
  if (historySize !== undefined) rl.historySize = historySize;
  return rl;
}

// 顶层光标件：有方法的流转调（tty.WriteStream 全家），否则 false（旧口径）。
function __delegate(stream, method, args) {
  if (stream !== null && stream !== undefined && typeof stream[method] === 'function') {
    return stream[method](...args);
  }
  return false;
}
function cursorTo(stream, x, y, cb) {
  if (typeof y === 'function') { cb = y; y = undefined; }
  const r = __delegate(stream, 'cursorTo', y === undefined ? [x] : [x, y]);
  if (typeof cb === 'function') queueMicrotask(cb);
  return r === undefined ? true : r;
}
function clearLine(stream, dir, cb) {
  if (typeof dir === 'function') { cb = dir; dir = undefined; }
  const r = __delegate(stream, 'clearLine', [dir ?? 0]);
  if (typeof cb === 'function') queueMicrotask(cb);
  return r === undefined ? true : r;
}
function clearScreenDown(stream, cb) {
  const r = __delegate(stream, 'clearScreenDown', []);
  if (typeof cb === 'function') queueMicrotask(cb);
  return r === undefined ? true : r;
}
function moveCursor(stream, dx, dy, cb) {
  const r = __delegate(stream, 'moveCursor', [dx, dy]);
  if (typeof cb === 'function') queueMicrotask(cb);
  return r === undefined ? true : r;
}

export {
  Interface,
  createInterface,
  emitKeypressEvents,
  cursorTo,
  clearLine,
  clearScreenDown,
  moveCursor,
};
export default {
  Interface,
  createInterface,
  emitKeypressEvents,
  cursorTo,
  clearLine,
  clearScreenDown,
  moveCursor,
};
"#;
