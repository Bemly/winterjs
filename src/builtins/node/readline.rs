//! `node:readline`（Node `lib/readline.js` 最小桥，MIT；plan 9j：vite 顶层 import）。
//!
//! 忠实面：`createInterface`（EventEmitter 基座：setPrompt/prompt/close/question
//! 抛未实现/pause/resume 无操作）、`emitKeypressEvents`（无操作）、
//! `cursorTo`/`clearLine`/`clearScreenDown`/`moveCursor`（非 TTY 恒回 false，
//! 不写 ANSI；TTY 下同样不写，偏差记档）。
//!
//! 偏差（记档）：无真逐行编辑（无 termios/行缓冲底座）；`question` 抛
//! `ERR_METHOD_NOT_IMPLEMENTED`（不自动答空串，免静默错）；Interface 无异步
//! 迭代器；`createInterface` 不触碰传入的流（只存引用）。

/// 内嵌 ESM 源（零 native，纯形）。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node lib/readline.js (minimal bridge; see module docs for deviations).
import { EventEmitter } from 'node:events';
import errors from 'node:internal/errors';

const {
  codes: {
    ERR_INVALID_ARG_TYPE,
    ERR_METHOD_NOT_IMPLEMENTED,
  },
} = errors;

function emitKeypressEvents(stream) {
  return undefined;
}

class Interface extends EventEmitter {
  constructor(input, output) {
    super();
    this.input = input ?? null;
    this.output = output ?? null;
    this._prompt = '';
    this.closed = false;
  }
  setPrompt(prompt) {
    this._prompt = String(prompt);
  }
  getPrompt() {
    return this._prompt;
  }
  prompt(preserveCursor) {
    return undefined;
  }
  question(query, cb) {
    throw new ERR_METHOD_NOT_IMPLEMENTED('readline.question');
  }
  pause() {
    return this;
  }
  resume() {
    return this;
  }
  write(data) {
    return undefined;
  }
  close() {
    if (!this.closed) {
      this.closed = true;
      this.emit('close');
    }
    return undefined;
  }
}

function normalizeInput(input, output) {
  if (input !== undefined && input !== null && typeof input !== 'object') {
    throw new ERR_INVALID_ARG_TYPE('input', ['object'], input);
  }
  return [input ?? null, output ?? null];
}

function createInterface(input, output) {
  if (input !== undefined && input !== null && typeof input === 'object' && !Array.isArray(input)) {
    output = input.output ?? output;
    input = input.input ?? input.terminal ?? null;
  }
  const [i, o] = normalizeInput(input, output);
  return new Interface(i, o);
}

// 非 TTY 恒 false，不写 ANSI（偏差记档）。
function cursorTo(stream, x, y) {
  return false;
}

function clearLine(stream, dir) {
  return false;
}

function clearScreenDown(stream) {
  return false;
}

function moveCursor(stream, dx, dy) {
  return false;
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
