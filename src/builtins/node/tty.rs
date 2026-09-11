//! `node:tty`（Node `lib/tty.js` 薄面，MIT；plan2 9a：薄，多为类型判定）。
//!
//! 忠实面：isatty（走 `__wjs_stdio_istty` native，0/1/2 有效）、
//! ReadStream/WriteStream（fd 校验/isTTY/isRaw/setRawMode 形态）、
//! ReadStream/WriteStream 静态 isatty 便捷、getColorDepth/hasColors（internal/tty 面）。
//!
//! 偏差（记档）：
//! - 基座为 EventEmitter 而非 net.Socket（node:net 未落地，9d 随 http 栈补齐后
//!   可换基座）；无底层 TTY handle：setRawMode 只跟踪标志位（无 termios），
//!   WriteStream.write 仅 fd 1/2 直通 stdout/stderr natives，其余 fd 报错。
//! - columns/rows 取 env（COLUMNS/LINES），无 ioctl winsize。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node lib/tty.js (thin face; see module docs for deviations).
import { EventEmitter } from 'node:events';
import errors from 'node:internal/errors';

const {
  ErrnoException: _ErrnoException,
  codes: {
    ERR_INVALID_ARG_VALUE,
    ERR_INVALID_FD,
  },
} = errors;

function isatty(fd) {
  return Number.isInteger(fd) && fd >= 0 && fd <= 2147483647 &&
         __wjs_stdio_istty(fd);
}

function ReadStream(fd, options) {
  if (!(this instanceof ReadStream)) return new ReadStream(fd, options);
  if (typeof fd !== 'number' || fd >> 0 !== fd || fd < 0)
    throw new ERR_INVALID_FD(fd);
  EventEmitter.call(this);
  this.fd = fd;
  this.isRaw = false;
  this.rawMode = false;
  this.isTTY = isatty(fd);
}
Object.setPrototypeOf(ReadStream.prototype, EventEmitter.prototype);
Object.setPrototypeOf(ReadStream, EventEmitter);

ReadStream.prototype.setRawMode = function setRawMode(mode) {
  let rawMode;
  if (mode === 'io' || mode === 'raw') {
    rawMode = mode;
  } else if (typeof mode === 'string') {
    throw new ERR_INVALID_ARG_VALUE('mode', mode, "must be true, false, 'raw', or 'io'");
  } else {
    rawMode = mode ? 'raw' : false;
  }
  // 无 termios 底座：只跟踪标志位（偏差记档）
  this.isRaw = rawMode !== false;
  this.rawMode = rawMode;
  return this;
};

function WriteStream(fd) {
  if (!(this instanceof WriteStream)) return new WriteStream(fd);
  if (typeof fd !== 'number' || fd >> 0 !== fd || fd < 0)
    throw new ERR_INVALID_FD(fd);
  EventEmitter.call(this);
  this.fd = fd;
  this.isTTY = isatty(fd);
  this.columns = Number(process.env.COLUMNS) || 80;
  this.rows = Number(process.env.LINES) || 24;
  this._writable = fd === 1 ? (s) => __wjs_stdout_write(String(s)) :
    fd === 2 ? (s) => __wjs_stderr_write(String(s)) :
      null;
}
Object.setPrototypeOf(WriteStream.prototype, EventEmitter.prototype);
Object.setPrototypeOf(WriteStream, EventEmitter);

WriteStream.prototype.write = function write(str) {
  if (this._writable === null) {
    const err = new Error(`tty.WriteStream: fd ${this.fd} is not writable`);
    this.emit('error', err);
    return false;
  }
  return this._writable(str) !== undefined ? true : true;
};

WriteStream.prototype._refreshSize = function _refreshSize() {
  this.columns = Number(process.env.COLUMNS) || this.columns;
  this.rows = Number(process.env.LINES) || this.rows;
};

ReadStream.isatty = isatty;
WriteStream.isatty = isatty;

// internal/tty 面：getColorDepth/hasColors（env 口径）
function getColorDepth(stream = process.stdout) {
  if (process.env.NO_COLOR !== undefined) return 1;
  if (process.env.FORCE_COLOR !== undefined) {
    const n = Number(process.env.FORCE_COLOR);
    return Number.isInteger(n) && n >= 0 ? Math.min(n, 24) : 24;
  }
  if (process.env.TERM === 'dumb') return 1;
  const isTTY = stream?.isTTY ?? false;
  if (!isTTY) return 1;
  if (process.env.COLORTERM === 'truecolor' || process.env.COLORTERM === '24bit') return 24;
  return 8;
}

function hasColors(stream, depth) {
  if (depth === undefined && typeof stream !== 'object') depth = stream;
  const d = depth === undefined ? getColorDepth(typeof stream === 'object' ? stream : process.stdout) : depth;
  return d > 1;
}

export { isatty, ReadStream, WriteStream, getColorDepth, hasColors };
export default { isatty, ReadStream, WriteStream, getColorDepth, hasColors };
"#;
