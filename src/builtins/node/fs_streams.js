// ---- fs 流：node lib/internal/fs/streams.js 逐段移植（26.8.2，MIT）。----
// 读写经 `this[kFs]`（缺省 = node:fs 默认导出对象 `__api`，用户 mock 可见；
// options.fs 自定义 / FileHandle 走 FileHandleOperations）。底座 fs 回调为同步 +
// 微任务，IO 进行中旗（kIsPerformingIO/kIoDone）照搬以保 destroy 时序。
// 偏差：windowsHandle 仅做 node 在非 Windows 上的报错面；FileHandle kRef/kUnref 无引用计数（no-op）。
import __fsStreamsDestroy from 'node:internal/streams/destroy';
import { finished as __fsStreamFinished } from 'node:stream';
import {
  validateBoolean as __fsVBoolean,
  validateFunction as __fsVFunction,
  validateInteger as __fsVInteger,
  validateInt32 as __fsVInt32,
  validateAbortSignal as __fsVAbortSignal,
  validateThisInternalField as __fsVThisInternalField,
} from 'node:internal/validators';

const __fsSC = errors.codes;
const kIoDone = Symbol('kIoDone');
const kIsPerformingIO = Symbol('kIsPerformingIO');
const kFs = Symbol('kFs');
const kHandle = Symbol('kHandle');

function __fsCodeErr(Base, code, msg) {
  const e = new Base(msg);
  e.code = code;
  return e;
}
// internal/fs/utils：copyObject / getOptions / getValidatedFd / validatePath；internal/url：toPathIfFileURL。
function __fsCopyObject(source) {
  const target = {};
  for (const key in source) target[key] = source[key];
  return target;
}
function __fsGetOptions(options, defaultOptions = {}) {
  if (options == null || typeof options === 'function') return defaultOptions;
  if (typeof options === 'string') {
    defaultOptions = { ...defaultOptions };
    defaultOptions.encoding = options;
    options = defaultOptions;
  } else if (typeof options !== 'object') {
    throw new __fsSC.ERR_INVALID_ARG_TYPE('options', ['string', 'Object'], options);
  }
  if (options.encoding !== 'buffer' && options.encoding && !Buffer.isEncoding(options.encoding)) {
    throw new __fsSC.ERR_INVALID_ARG_VALUE('encoding', options.encoding, 'is invalid encoding');
  }
  if (options.signal !== undefined) __fsVAbortSignal(options.signal, 'options.signal');
  return options;
}
function __fsGetValidatedFd(fd, propName = 'fd') {
  if (Object.is(fd, -0)) return 0;
  __fsVInt32(fd, propName, 0);
  return fd;
}
function __fsToPathIfFileURL(p) {
  if (!(p instanceof URL)) return p;
  __fsPath(p, 'open');
  return decodeURIComponent(p.pathname);
}
function __fsValidateStreamPath(path, propName = 'path') {
  if (typeof path !== 'string' && !(path instanceof Uint8Array)) {
    throw new __fsSC.ERR_INVALID_ARG_TYPE(propName, ['string', 'Buffer', 'URL'], path);
  }
  __fsPath(path, 'open');
}
function __fsIsFileHandle(fd) {
  return fd instanceof FileHandle;
}

function _construct(callback) {
  const stream = this;
  if (typeof stream.fd === 'number') {
    callback();
    return;
  }

  if (typeof stream.open === 'function') {
    // Backwards compat for monkey patching open().
    const orgEmit = stream.emit;
    stream.emit = function(...args) {
      if (args[0] === 'open') {
        this.emit = orgEmit;
        callback();
        Reflect.apply(orgEmit, this, args);
      } else if (args[0] === 'error') {
        this.emit = orgEmit;
        callback(args[1]);
      } else {
        Reflect.apply(orgEmit, this, args);
      }
    };
    stream.open();
  } else {
    stream[kFs].open(stream.path, stream.flags, stream.mode, (er, fd) => {
      if (er) {
        callback(er);
      } else {
        stream.fd = fd;
        callback();
        stream.emit('open', stream.fd);
        stream.emit('ready');
      }
    });
  }
}

// This generates an fs operations structure for a FileHandle
const FileHandleOperations = (handle) => {
  return {
    open: (path, flags, mode, cb) => {
      throw new __fsSC.ERR_METHOD_NOT_IMPLEMENTED('open()');
    },
    close: (fd, cb) => {
      handle.close().then(() => cb(), cb);
    },
    fsync: (fd, cb) => {
      handle.sync().then(() => cb(), cb);
    },
    read: (fd, buf, offset, length, pos, cb) => {
      handle.read(buf, offset, length, pos).then(
        (r) => cb(null, r.bytesRead, r.buffer),
        (err) => cb(err, 0, buf));
    },
    write: (fd, buf, offset, length, pos, cb) => {
      handle.write(buf, offset, length, pos).then(
        (r) => cb(null, r.bytesWritten, r.buffer),
        (err) => cb(err, 0, buf));
    },
    writev: (fd, buffers, pos, cb) => {
      handle.writev(buffers, pos).then(
        (r) => cb(null, r.bytesWritten, r.buffers),
        (err) => cb(err, 0, buffers));
    },
  };
};

function __fsStreamClose(stream, err, cb) {
  if (!stream.fd) {
    cb(err);
  } else if (stream.flush) {
    stream[kFs].fsync(stream.fd, (flushErr) => {
      __fsStreamCloseFd(stream, err || flushErr, cb);
    });
  } else {
    __fsStreamCloseFd(stream, err, cb);
  }
}

function __fsStreamCloseFd(stream, err, cb) {
  stream[kFs].close(stream.fd, (er) => {
    cb(er || err);
  });
  stream.fd = null;
}

function importFd(stream, options) {
  if (typeof options.fd === 'number') {
    // When fd is a raw descriptor, we must keep our fingers crossed
    // that the descriptor won't get closed, or worse, replaced with
    // another one
    // https://github.com/nodejs/node/issues/35862
    stream[kFs] = options.fs || __api;
    return options.fd;
  } else if (typeof options.fd === 'object' && __fsIsFileHandle(options.fd)) {
    // When fd is a FileHandle we can listen for 'close' events
    if (options.fs) {
      // FileHandle is not supported with custom fs operations
      throw new __fsSC.ERR_METHOD_NOT_IMPLEMENTED('FileHandle with fs');
    }
    stream[kHandle] = options.fd;
    stream[kFs] = FileHandleOperations(stream[kHandle]);
    options.fd.on('close', stream.close.bind(stream));
    return options.fd.fd;
  }

  throw new __fsSC.ERR_INVALID_ARG_TYPE('options.fd', ['number', 'FileHandle'], options.fd);
}

function importWindowsHandle(stream, options) {
  if (options.windowsHandle == null) {
    throw __fsCodeErr(TypeError, 'ERR_MISSING_OPTION', 'options.windowsHandle is required');
  }
  throw __fsCodeErr(TypeError, 'ERR_FEATURE_UNAVAILABLE_ON_PLATFORM',
    'The feature windowsHandle is unavailable on the current platform, which is being used to run Node.js');
}

function __fsIncompatiblePair() {
  return __fsCodeErr(TypeError, 'ERR_INCOMPATIBLE_OPTION_PAIR',
    'Option "windowsHandle" cannot be used in combination with option "fd"');
}

export function ReadStream(path, options) {
  if (!(this instanceof ReadStream))
    return new ReadStream(path, options);

  // A little bit bigger buffer and water marks by default
  options = __fsCopyObject(__fsGetOptions(options, {}));
  if (options.highWaterMark === undefined)
    options.highWaterMark = 64 * 1024;

  if (options.autoDestroy === undefined) {
    options.autoDestroy = false;
  }

  if (options.fd != null && options.windowsHandle != null) {
    throw __fsIncompatiblePair();
  } else if (options.windowsHandle != null) {
    this.fd = __fsGetValidatedFd(importWindowsHandle(this, options));
  } else if (options.fd == null) {
    this.fd = null;
    this[kFs] = options.fs || __api;
    __fsVFunction(this[kFs].open, 'options.fs.open');

    // Path will be ignored when fd is specified, so it can be falsy
    this.path = __fsToPathIfFileURL(path);
    this.flags = options.flags === undefined ? 'r' : options.flags;
    this.mode = options.mode === undefined ? 0o666 : options.mode;

    __fsValidateStreamPath(this.path);
  } else {
    this.fd = __fsGetValidatedFd(importFd(this, options));
  }

  options.autoDestroy = options.autoClose === undefined ?
    true : options.autoClose;

  __fsVFunction(this[kFs].read, 'options.fs.read');

  if (options.autoDestroy) {
    __fsVFunction(this[kFs].close, 'options.fs.close');
  }

  this.start = options.start;
  this.end = options.end;
  this.pos = undefined;
  this.bytesRead = 0;
  this[kIsPerformingIO] = false;

  if (this.start !== undefined) {
    __fsVInteger(this.start, 'start', 0);

    this.pos = this.start;
  }


  if (this.end === undefined) {
    this.end = Infinity;
  } else if (this.end !== Infinity) {
    __fsVInteger(this.end, 'end', 0);

    if (this.start !== undefined && this.start > this.end) {
      throw new __fsSC.ERR_OUT_OF_RANGE(
        'start',
        `<= "end" (here: ${this.end})`,
        this.start,
      );
    }
  }

  Reflect.apply(Readable, this, [options]);
}
Object.setPrototypeOf(ReadStream.prototype, Readable.prototype);
Object.setPrototypeOf(ReadStream, Readable);

Object.defineProperty(ReadStream.prototype, 'autoClose', {
  __proto__: null,
  get() {
    __fsVThisInternalField(this, kFs, 'ReadStream');
    return this._readableState.autoDestroy;
  },
  set(val) {
    __fsVThisInternalField(this, kFs, 'ReadStream');
    this._readableState.autoDestroy = val;
  },
});

ReadStream.prototype._construct = _construct;

ReadStream.prototype._read = function(n) {
  n = this.pos !== undefined ?
    Math.min(this.end - this.pos + 1, n) :
    Math.min(this.end - this.bytesRead + 1, n);

  if (n <= 0) {
    this.push(null);
    return;
  }

  const buf = Buffer.allocUnsafeSlow(n);

  this[kIsPerformingIO] = true;
  this[kFs]
    .read(this.fd, buf, 0, n, this.pos, (er, bytesRead, buf) => {
      this[kIsPerformingIO] = false;

      // Tell ._destroy() that it's safe to close the fd now.
      if (this.destroyed) {
        this.emit(kIoDone, er);
        return;
      }

      if (er) {
        __fsStreamsDestroy.errorOrDestroy(this, er);
      } else if (bytesRead > 0) {
        if (this.pos !== undefined) {
          this.pos += bytesRead;
        }

        this.bytesRead += bytesRead;

        if (bytesRead !== buf.length) {
          // Slow path. Shrink to fit.
          // Copy instead of slice so that we don't retain
          // large backing buffer for small reads.
          const dst = Buffer.allocUnsafeSlow(bytesRead);
          buf.copy(dst, 0, 0, bytesRead);
          buf = dst;
        }

        this.push(buf);
      } else {
        this.push(null);
      }
    });
};

ReadStream.prototype._destroy = function(err, cb) {
  // Wait for any pending IO (kIsPerformingIO) to complete (kIoDone)
  // before closing the fd (node: thread-pool IO is not safe to race close).
  if (this[kIsPerformingIO]) {
    this.once(kIoDone, (er) => __fsStreamClose(this, err || er, cb));
  } else {
    __fsStreamClose(this, err, cb);
  }
};

ReadStream.prototype.close = function(cb) {
  if (typeof cb === 'function') __fsStreamFinished(this, cb);
  this.destroy();
};

Object.defineProperty(ReadStream.prototype, 'pending', {
  __proto__: null,
  get() { return this.fd === null; },
  configurable: true,
});

export function WriteStream(path, options) {
  if (!(this instanceof WriteStream))
    return new WriteStream(path, options);

  options = __fsCopyObject(__fsGetOptions(options, {}));

  // Only buffers are supported.
  options.decodeStrings = true;

  if (options.fd != null && options.windowsHandle != null) {
    throw __fsIncompatiblePair();
  } else if (options.windowsHandle != null) {
    this.fd = __fsGetValidatedFd(importWindowsHandle(this, options));
  } else if (options.fd == null) {
    this.fd = null;
    this[kFs] = options.fs || __api;
    __fsVFunction(this[kFs].open, 'options.fs.open');

    // Path will be ignored when fd is specified, so it can be falsy
    this.path = __fsToPathIfFileURL(path);
    this.flags = options.flags === undefined ? 'w' : options.flags;
    this.mode = options.mode === undefined ? 0o666 : options.mode;

    __fsValidateStreamPath(this.path);
  } else {
    this.fd = __fsGetValidatedFd(importFd(this, options));
  }

  options.autoDestroy = options.autoClose === undefined ?
    true : options.autoClose;

  if (!this[kFs].write && !this[kFs].writev) {
    throw new __fsSC.ERR_INVALID_ARG_TYPE('options.fs.write', 'function', this[kFs].write);
  }

  if (this[kFs].write) {
    __fsVFunction(this[kFs].write, 'options.fs.write');
  }

  if (this[kFs].writev) {
    __fsVFunction(this[kFs].writev, 'options.fs.writev');
  }

  if (options.autoDestroy) {
    __fsVFunction(this[kFs].close, 'options.fs.close');
  }

  this.flush = options.flush;
  if (this.flush == null) {
    this.flush = false;
  } else {
    __fsVBoolean(this.flush, 'options.flush');
    __fsVFunction(this[kFs].fsync, 'options.fs.fsync');
  }

  // It's enough to override either, in which case only one will be used.
  if (!this[kFs].write) {
    this._write = null;
  }
  if (!this[kFs].writev) {
    this._writev = null;
  }

  this.start = options.start;
  this.pos = undefined;
  this.bytesWritten = 0;
  this[kIsPerformingIO] = false;

  if (this.start !== undefined) {
    __fsVInteger(this.start, 'start', 0);

    this.pos = this.start;
  }

  Reflect.apply(Writable, this, [options]);

  if (options.encoding)
    this.setDefaultEncoding(options.encoding);
}
Object.setPrototypeOf(WriteStream.prototype, Writable.prototype);
Object.setPrototypeOf(WriteStream, Writable);

Object.defineProperty(WriteStream.prototype, 'autoClose', {
  __proto__: null,
  get() {
    __fsVThisInternalField(this, kFs, 'WriteStream');
    return this._writableState.autoDestroy;
  },
  set(val) {
    __fsVThisInternalField(this, kFs, 'WriteStream');
    this._writableState.autoDestroy = val;
  },
});

WriteStream.prototype._construct = _construct;

function writeAll(data, size, pos, cb, retries = 0) {
  this[kFs].write(this.fd, data, 0, size, pos, (er, bytesWritten, buffer) => {
    // No data currently available and operation should be retried later.
    if (er?.code === 'EAGAIN') {
      er = null;
      bytesWritten = 0;
    }

    if (this.destroyed || er) {
      return cb(er || new __fsSC.ERR_STREAM_DESTROYED('write'));
    }

    this.bytesWritten += bytesWritten;

    retries = bytesWritten ? 0 : retries + 1;
    size -= bytesWritten;
    pos += bytesWritten;

    // Try writing non-zero number of bytes up to 5 times.
    if (retries > 5) {
      cb(new __fsSC.ERR_SYSTEM_ERROR('write failed'));
    } else if (size) {
      writeAll.call(this, buffer.slice(bytesWritten), size, pos, cb, retries);
    } else {
      cb();
    }
  });
}

function writevAll(chunks, size, pos, cb, retries = 0) {
  this[kFs].writev(this.fd, chunks, this.pos, (er, bytesWritten, buffers) => {
    // No data currently available and operation should be retried later.
    if (er?.code === 'EAGAIN') {
      er = null;
      bytesWritten = 0;
    }

    if (this.destroyed || er) {
      return cb(er || new __fsSC.ERR_STREAM_DESTROYED('writev'));
    }

    this.bytesWritten += bytesWritten;

    retries = bytesWritten ? 0 : retries + 1;
    size -= bytesWritten;
    pos += bytesWritten;

    // Try writing non-zero number of bytes up to 5 times.
    if (retries > 5) {
      cb(new __fsSC.ERR_SYSTEM_ERROR('writev failed'));
    } else if (size) {
      writevAll.call(this, [Buffer.concat(buffers).slice(bytesWritten)], size, pos, cb, retries);
    } else {
      cb();
    }
  });
}

WriteStream.prototype._write = function(data, encoding, cb) {
  this[kIsPerformingIO] = true;
  writeAll.call(this, data, data.length, this.pos, (er) => {
    this[kIsPerformingIO] = false;
    if (this.destroyed) {
      // Tell ._destroy() that it's safe to close the fd now.
      cb(er);
      return this.emit(kIoDone, er);
    }

    cb(er);
  });

  if (this.pos !== undefined)
    this.pos += data.length;
};

WriteStream.prototype._writev = function(data, cb) {
  const len = data.length;
  const chunks = new Array(len);
  let size = 0;

  for (let i = 0; i < len; i++) {
    const chunk = data[i].chunk;

    chunks[i] = chunk;
    size += chunk.length;
  }

  this[kIsPerformingIO] = true;
  writevAll.call(this, chunks, size, this.pos, (er) => {
    this[kIsPerformingIO] = false;
    if (this.destroyed) {
      // Tell ._destroy() that it's safe to close the fd now.
      cb(er);
      return this.emit(kIoDone, er);
    }

    cb(er);
  });

  if (this.pos !== undefined)
    this.pos += size;
};

WriteStream.prototype._destroy = function(err, cb) {
  // Wait for any pending IO (kIsPerformingIO) to complete (kIoDone).
  if (this[kIsPerformingIO]) {
    this.once(kIoDone, (er) => __fsStreamClose(this, err || er, cb));
  } else {
    __fsStreamClose(this, err, cb);
  }
};

WriteStream.prototype.close = function(cb) {
  if (cb) {
    if (this.closed) {
      process.nextTick(cb);
      return;
    }
    this.on('close', cb);
  }

  // If we are not autoClosing, we should call
  // destroy on 'finish'.
  if (!this.autoClose) {
    this.on('finish', this.destroy);
  }

  // We use end() instead of destroy() because of
  // https://github.com/nodejs/node/issues/2006
  this.end();
};

// There is no shutdown() for files.
WriteStream.prototype.destroySoon = WriteStream.prototype.end;

Object.defineProperty(WriteStream.prototype, 'pending', {
  __proto__: null,
  get() { return this.fd === null; },
  configurable: true,
});

// node lib/fs.js：createReadStream/createWriteStream 直构（校验在构造器内）。
export function createReadStream(path, options) {
  return new ReadStream(path, options);
}

export function createWriteStream(path, options) {
  return new WriteStream(path, options);
}
