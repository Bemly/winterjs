//! `node:tty`（Node `lib/tty.js` 薄面，MIT；plan2 9a：薄，多为类型判定）。
//!
//! 10c-1：net.Socket 基座 + ioctl winsize（unix，`nix/ioctl` + `libc` 直引，
//! 用户拍板）+ setRawMode 真标志（unix，`nix/term`，用户拍板；原 Termios 按
//! fd 寄存，关 raw 时恢复）；win 全回落记档。
//! 构造器真机口径：fd 非 TTY 即 `ERR_TTY_INIT_FAILED`（含 WriteStream；
//! `isatty(99)` 这类纯查询仍回 false，不抛）。
//!
//! 忠实面：isatty（走 `__wjs_stdio_istty` native，0/1/2 有效）、
//! ReadStream/WriteStream（fd 校验/isTTY/isRaw/setRawMode 形态）、
//! ReadStream/WriteStream 静态 isatty 便捷、getColorDepth/hasColors（internal/tty 面）。
//!
//! 偏差（记档）：
//! - columns/rows 只认 ioctl winsize（无 env 回落——真机 `COLUMNS=100` 照样
//!   undefined，已对拍）；非 TTY 即 undefined。
//! - WriteStream.write 仅 fd 1/2 直通 stdout/stderr natives，其余 fd 报错。
//! - win：winsize 回空、setRawMode 回 ENOSYS（`getSystemErrorName(-78)`）。

use mozjs::jsval::{JSVal, Int32Value};
use mozjs::rooted;

use crate::jsapi_glue::{wrap_cx, Frame};

#[cfg(unix)]
nix::ioctl_read_bad!(tiocgwinsz, libc::TIOCGWINSZ, libc::winsize);

/// 原 Termios 寄存（fd → enable 前的设置；disable 时恢复）。
#[cfg(unix)]
static RAW_STASH: std::sync::LazyLock<
    parking_lot::Mutex<std::collections::HashMap<i32, nix::sys::termios::Termios>>,
> = std::sync::LazyLock::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));

/// `__wjs_tty_winsize(fd)` → `"COLSxROWS"`，失败回 `""`（JS 侧落 undefined）。
/// 前置：引擎回调提供的 raw cx 有效。
pub unsafe extern "C" fn tty_winsize(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let fd = if frame.argc() > 0 && frame.arg(0).is_number() {
        frame.arg(0).to_number() as i32
    } else {
        1
    };
    let text = winsize_text(fd);
    rooted!(&in(cx) let mut v = mozjs::jsval::UndefinedValue());
    {
        use mozjs::conversions::ToJSValConvertible as _;
        text.as_str().to_jsval(&mut cx, v.handle_mut());
    }
    frame.set_rval(v.get());
    true
}

#[cfg(unix)]
fn winsize_text(fd: i32) -> String {
    // SAFETY: tiocgwinsz 只写传入的 winsize 结构体；失败（ENOTTY 等）即回空串
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let ok = unsafe { tiocgwinsz(fd as libc::c_int, &mut ws) }.is_ok();
    if ok {
        format!("{}x{}", ws.ws_col, ws.ws_row)
    } else {
        String::new()
    }
}

#[cfg(not(unix))]
fn winsize_text(_fd: i32) -> String {
    String::new()
}

#[cfg(unix)]
fn errno_to_i32(e: nix::errno::Errno) -> i32 {
    e as i32
}

/// `__wjs_tty_set_raw_mode(fd, on)` → errno（0 = 成功；JS 侧经
/// `getSystemErrorName(-rc)` 组错）。
pub unsafe extern "C" fn tty_set_raw_mode(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let fd = if frame.argc() > 0 && frame.arg(0).is_number() {
        frame.arg(0).to_number() as i32
    } else {
        0
    };
    let on = frame.argc() > 1 && frame.arg(1).is_number() && frame.arg(1).to_number() != 0.0;
    let _ = &mut cx;
    frame.set_rval(Int32Value(set_raw_mode(fd, on)));
    true
}

#[cfg(unix)]
fn set_raw_mode(fd: i32, on: bool) -> i32 {
    use nix::sys::termios::*;
    use std::os::fd::BorrowedFd;
    // SAFETY: borrow_raw 只包装编号，不拥有不关闭；tc* 调用内即时返回
    let bfd = unsafe { BorrowedFd::borrow_raw(fd) };
    if on {
        let orig = match tcgetattr(&bfd) {
            Ok(t) => t,
            Err(e) => return errno_to_i32(e),
        };
        RAW_STASH.lock().insert(fd, orig.clone());
        let mut raw = orig;
        cfmakeraw(&mut raw);
        match tcsetattr(&bfd, SetArg::TCSANOW, &raw) {
            Ok(()) => 0,
            Err(e) => errno_to_i32(e),
        }
    } else if let Some(orig) = RAW_STASH.lock().remove(&fd) {
        match tcsetattr(&bfd, SetArg::TCSANOW, &orig) {
            Ok(()) => 0,
            Err(e) => errno_to_i32(e),
        }
    } else {
        0
    }
}

#[cfg(not(unix))]
fn set_raw_mode(_fd: i32, _on: bool) -> i32 {
    // ENOSYS（功能未实现；JS 侧 `getSystemErrorName(-78)` 即原文）。
    78
}

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node lib/tty.js (thin face; see module docs for deviations).
import { Socket } from 'node:net';
import errors from 'node:internal/errors';
import { getSystemErrorName } from 'node:util';

const {
  ErrnoException: _ErrnoException,
  codes: {
    ERR_INVALID_ARG_VALUE,
    ERR_INVALID_FD,
    ERR_TTY_INIT_FAILED,
  },
} = errors;

function isatty(fd) {
  return Number.isInteger(fd) && fd >= 0 && fd <= 2147483647 &&
         __wjs_stdio_istty(fd);
}

// ioctl winsize（失败即 [undefined, undefined]；真机 COLUMNS/LINES 不认，只认
// 内核尺寸——`COLUMNS=100` 管道下仍 undefined，已对拍）。
function __readSize(fd) {
  const m = /^(\d+)x(\d+)$/.exec(__wjs_tty_winsize(fd));
  if (!m) return [undefined, undefined];
  const c = Number(m[1]), r = Number(m[2]);
  return [c > 0 ? c : undefined, r > 0 ? r : undefined];
}

class ReadStream extends Socket {
  constructor(fd, options) {
    super();
    if (typeof fd !== 'number' || fd >> 0 !== fd || fd < 0)
      throw new ERR_INVALID_FD(fd);
    if (!isatty(fd)) throw new ERR_TTY_INIT_FAILED(`fd ${fd} is not a TTY`);
    this.fd = fd;
    this.isRaw = false;
    this.rawMode = false;
    this.isTTY = true;
    this.readable = true;
    this.writable = false;
  }
  setRawMode(mode) {
    let rawMode;
    if (mode === 'io' || mode === 'raw') {
      rawMode = mode;
    } else if (typeof mode === 'string') {
      throw new ERR_INVALID_ARG_VALUE('mode', mode, "must be true, false, 'raw', or 'io'");
    } else {
      rawMode = mode ? 'raw' : false;
    }
    // 真 termios（unix；win 回 ENOSYS，见模块头注）。
    const rc = __wjs_tty_set_raw_mode(this.fd, rawMode === false ? 0 : 1);
    if (rc !== 0) {
      const name = getSystemErrorName(-rc);
      const err = new Error(`${name}: setRawMode ${rc}`);
      err.code = name;
      err.errno = -rc;
      err.syscall = 'setRawMode';
      throw err;
    }
    this.isRaw = rawMode !== false;
    this.rawMode = rawMode;
    return this;
  }
}

class WriteStream extends Socket {
  constructor(fd) {
    super();
    if (typeof fd !== 'number' || fd >> 0 !== fd || fd < 0)
      throw new ERR_INVALID_FD(fd);
    if (!isatty(fd)) throw new ERR_TTY_INIT_FAILED(`fd ${fd} is not a TTY`);
    this.fd = fd;
    this.isTTY = true;
    this.readable = false;
    this.writable = true;
    this.columns = undefined;
    this.rows = undefined;
    this._refreshSize();
    this._writable = fd === 1 ? (s) => __wjs_stdout_write(String(s)) :
      fd === 2 ? (s) => __wjs_stderr_write(String(s)) :
        null;
  }
  write(str) {
    if (this._writable === null) {
      const err = new Error(`tty.WriteStream: fd ${this.fd} is not writable`);
      this.emit('error', err);
      return false;
    }
    this._writable(str);
    return true;
  }
  _writeAnsi(seq) {
    if (this._writable !== null) {
      try { this._writable(seq); } catch { /* ignore */ }
    }
    return true;
  }
  clearLine(dir, cb) {
    const d = Number(dir);
    const seq = d < 0 ? '\x1b[1K' : d > 0 ? '\x1b[0K' : '\x1b[2K';
    if (typeof cb === 'function') queueMicrotask(cb);
    return this._writeAnsi(seq);
  }
  clearScreenDown(cb) {
    if (typeof cb === 'function') queueMicrotask(cb);
    return this._writeAnsi('\x1b[0J');
  }
  cursorTo(x, y, cb) {
    if (typeof y === 'function') { cb = y; y = undefined; }
    if (typeof cb === 'function') queueMicrotask(cb);
    return this._writeAnsi(`\x1b[${y ?? 1};${x}H`);
  }
  moveCursor(dx, dy, cb) {
    if (typeof cb === 'function') queueMicrotask(cb);
    let seq = '';
    if (dx < 0) seq += `\x1b[${-dx}D`;
    else if (dx > 0) seq += `\x1b[${dx}C`;
    if (dy < 0) seq += `\x1b[${-dy}A`;
    else if (dy > 0) seq += `\x1b[${dy}B`;
    return this._writeAnsi(seq);
  }
  getWindowSize() {
    return __readSize(this.fd);
  }
  _refreshSize() {
    const [c, r] = __readSize(this.fd);
    this.columns = c;
    this.rows = r;
  }
}

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
