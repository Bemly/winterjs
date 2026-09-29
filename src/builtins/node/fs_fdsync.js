// ── 10f：参数校验族（node lib/internal/validators 口径；code+message 对拍）──
function __vReceived(v) {
  if (v === null) return "null";
  if (v === undefined) return "undefined";
  const t = typeof v;
  if (t === "string") return `type string ('${v}')`;
  if (t === "boolean") return `type boolean (${v})`;
  if (t === "number") return `type number (${v})`;
  if (t === "object") {
    if (Array.isArray(v)) return "an instance of Array";
    const n = v.constructor && v.constructor.name ? v.constructor.name : "Object";
    return `an instance of ${n}`;
  }
  if (t === "function") return `function ${v.name}`;
  return `type ${t} (${String(v)})`;
}
function __vErrType(name, expected, v) {
  const e = new TypeError(`The "${name}" argument must be of type ${expected}. Received ${__vReceived(v)}`);
  e.code = "ERR_INVALID_ARG_TYPE"; throw e;
}
function __vIntRange(v, name, min, max) {
  if (typeof v !== "number") __vErrType(name, "number", v);
  if (!Number.isInteger(v)) {
    const e = new RangeError(`The value of "${name}" is out of range. It must be an integer. Received ${v}`);
    e.code = "ERR_OUT_OF_RANGE"; throw e;
  }
  if (v < min || v > max) {
    const e = new RangeError(`The value of "${name}" is out of range. It must be >= ${min} && <= ${max}. Received ${v}`);
    e.code = "ERR_OUT_OF_RANGE"; throw e;
  }
  return v;
}
function __vFd(fd) {
  // node getValidatedFd：int32 正数域（fchmod 套件点名 -1 / 2**32）。
  return __vIntRange(fd, "fd", 0, 2147483647);
}
function __fsParseMode(mode, def) {
  // node parseFileMode（validators.js 逐字；open-mode-mask/fchmod 套件 + 真机对拍）：
  // null/undefined → def；string 须全八进制位 [0-7]+（非八进制串 ARG_VALUE
  // 'must be a 32-bit unsigned integer or an octal string'，TypeError 类）；
  // 其余非 number → ARG_TYPE 'number'；整数域 [0, 2**32-1] OUT_OF_RANGE。
  mode ??= def;
  if (typeof mode === "string") {
    if (!/^[0-7]+$/.test(mode)) {
      const e = new TypeError(`The argument 'mode' must be a 32-bit unsigned integer or an octal string. Received '${mode}'`);
      e.code = "ERR_INVALID_ARG_VALUE"; throw e;
    }
    mode = parseInt(mode, 8);
  }
  if (typeof mode !== "number") __vErrType("mode", "number", mode);
  if (!Number.isInteger(mode)) {
    const e = new RangeError(`The value of "mode" is out of range. It must be an integer. Received ${mode}`);
    e.code = "ERR_OUT_OF_RANGE"; throw e;
  }
  if (mode < 0 || mode > 4294967295) {
    const e = new RangeError(`The value of "mode" is out of range. It must be >= 0 && <= 4294967295. Received ${mode}`);
    e.code = "ERR_OUT_OF_RANGE"; throw e;
  }
  return mode + 0;
}
function __vModeArg(mode) {
  if (typeof mode !== "number" && typeof mode !== "string") __vErrType("mode", "number or string", mode);
  return mode;
}
function __vIdNum(v, name) {
  if (typeof v !== "number" || !Number.isInteger(v)) __vErrType(name, "number", v);
  return v;
}
function __vCbArg(cb) {
  if (typeof cb !== "function") __vErrType("callback", "function", cb);
  return cb;
}
function __vBooleanProp(v, name) {
  // node validateBoolean：属性校验文案用 "property"（test-fs-mkdir 逐字断言）。
  if (typeof v !== "boolean") {
    const e = new TypeError(`The "${name}" property must be of type boolean. Received ${__vReceived(v)}`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  return v;
}
function __vInteger(v, name, min) {
  // node validateInteger 逐字：非 number ARG_TYPE；非整数 OOR 'an integer'。
  if (typeof v !== "number") __vErrType(name, "number", v);
  if (!Number.isInteger(v)) {
    const e = new RangeError(`The value of "${name}" is out of range. It must be an integer. Received ${v}`);
    e.code = "ERR_OUT_OF_RANGE"; throw e;
  }
  if (min !== undefined && v < min) {
    const e = new RangeError(`The value of "${name}" is out of range. It must be >= ${min}. Received ${v}`);
    e.code = "ERR_OUT_OF_RANGE"; throw e;
  }
  return v;
}
function __vOffsetLength(offset, length, byteLength) {
  if (offset < 0 || length < 0 || byteLength - offset < length) {
    const bad = offset < 0 ? "offset" : "length";
    const e = new RangeError(`The value of "${bad}" is out of range. It must be >= 0. Received ${bad === "offset" ? offset : length}`);
    e.code = "ERR_OUT_OF_RANGE"; throw e;
  }
}
function __vBuffer(buffer) {
  // node validateBuffer 逐字（test-fs-read 'options.buffer is null' 点名 message）。
  if (!ArrayBuffer.isView(buffer)) {
    const e = new TypeError(`The "buffer" argument must be an instance of Buffer, TypedArray, or DataView. Received ${__vReceived(buffer)}`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
}
function __vEmptyBuffer(buffer) {
  // node ERR_INVALID_ARG_VALUE('buffer', buffer, 'is empty and cannot be written')。
  if (buffer.byteLength === 0) __fsEmptyBufferErr(buffer);
}
export function readSync(fd, buffer, offsetOrOptions, length, position) {
  __vFd(fd);
  __vBuffer(buffer);
  // node：3 参即 options 形（{offset,length,position}；validateObject 允 null）。
  let offset = offsetOrOptions;
  if (arguments.length <= 3 || (offsetOrOptions !== null && typeof offsetOrOptions === "object")) {
    if (offsetOrOptions !== undefined && offsetOrOptions !== null &&
        (Array.isArray(offsetOrOptions) || typeof offsetOrOptions !== "object")) {
      __vErrType("options", "object", offsetOrOptions);
    }
    ({ offset = 0, length = buffer.byteLength - offset, position = null } = offsetOrOptions ?? {});
  }
  if (offset === undefined) offset = 0;
  else __vInteger(offset, "offset", 0);
  length |= 0;
  if (position == null) position = -1;
  else __fsValidatePosition(position, "position", length);
  if (length === 0) return 0;
  if (buffer.byteLength === 0) __fsEmptyBufferErr(buffer);
  __vOffsetLength(offset, length, buffer.byteLength);
  const chunk = __fsCall("read", "", () =>
    __wjs2_fs_read_fd(fd, length, typeof position === "bigint" ? Number(position) : position));
  buffer.set(chunk, offset);
  return chunk.length;
}
// node：ERR_INVALID_ARG_VALUE('buffer', buffer, 'is empty and cannot be written')
// （read-empty-buffer 套件逐字；inspect 形 Received Uint8Array(0) []）。
function __fsEmptyBufferErr(buffer) {
  // node inspect 口径：空 TypedArray 即 `Uint8Array(0) []`（套件仅空视图到达此点）。
  const ctor = (buffer && buffer.constructor && buffer.constructor.name) || "Uint8Array";
  const e = new TypeError(`The argument 'buffer' is empty and cannot be written. Received ${ctor}(0) []`);
  e.code = "ERR_INVALID_ARG_VALUE"; throw e;
}
// node validatePosition：number 整数 >= -1；bigint 域 [ -1, 2**63-1-length ]；余 ARG_TYPE。
function __fsValidatePosition(position, name, length) {
  if (typeof position === "number") {
    __vInteger(position, name, -1);
  } else if (typeof position === "bigint") {
    const maxPosition = 2n ** 63n - 1n - BigInt(length);
    if (!(position >= -1n && position <= maxPosition)) {
      const e = new RangeError(`The value of "${name}" is out of range. It must be >= -1 && <= ${maxPosition}. Received ${position}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
  } else {
    __vErrType(name, "integer or bigint", position);
  }
}
export function writeSync(fd, buffer, offsetOrOptions, length, position) {
  __vFd(fd);
  let data;
  let pos = -1;
  if (ArrayBuffer.isView(buffer)) {
    let offset = offsetOrOptions;
    // node：view + options 对象形（{offset,length,position}）。
    if (offsetOrOptions !== null && typeof offsetOrOptions === "object" && !ArrayBuffer.isView(offsetOrOptions)) {
      ({ offset = 0, length = buffer.byteLength - offset, position = null } = offsetOrOptions ?? {});
    }
    if (offset == null) offset = 0;
    else __vInteger(offset, "offset", 0);
    if (typeof length !== "number") length = buffer.byteLength - offset;
    length |= 0;
    const u8 = __fsData(buffer, "write");
    __fsValidateOffsetLengthWrite(offset, length, u8.length);
    data = u8.subarray(offset, offset + length);
    if (position != null) __fsValidatePosition(position, "position", length);
    pos = typeof position === "bigint" ? Number(position) : (position ?? -1);
  } else if (typeof buffer === "string") {
    pos = typeof offsetOrOptions === "number" ? offsetOrOptions : -1;
    data = __fsData(buffer, "write");
  } else {
    __vBuffer(buffer);
  }
  return Number(__fsCall("write", "", () => __wjs2_fs_write_fd(fd, data, pos)));
}
// node validateOffsetLengthWrite 逐字：length > byteLength - offset 即 OOR。
function __fsValidateOffsetLengthWrite(offset, length, byteLength) {
  if (length > byteLength - offset) {
    const e = new RangeError(`The value of "length" is out of range. It must be <= ${byteLength - offset}. Received ${length}`);
    e.code = "ERR_OUT_OF_RANGE"; throw e;
  }
  if (offset < 0 || length < 0) {
    const bad = offset < 0 ? "offset" : "length";
    const e = new RangeError(`The value of "${bad}" is out of range. It must be >= 0. Received ${bad === "offset" ? offset : length}`);
    e.code = "ERR_OUT_OF_RANGE"; throw e;
  }
}
// node validateBufferArray（validators 逐字口径）：数组 + 每项 ArrayBufferView。
function __fsValidateBufferArray(buffers) {
  if (!Array.isArray(buffers)) __vErrType("buffers", "ArrayBufferView[]", buffers);
  for (const b of buffers) {
    if (!ArrayBuffer.isView(b)) __vErrType("buffers", "ArrayBufferView[]", buffers);
  }
  return buffers;
}
export function ftruncateSync(fd, len) {
  __vFd(fd);
  const n = __fsLenArg(len);
  __fsCall("ftruncate", "", () => __wjs2_fs_ftruncate(fd, n));
}
export function fsyncSync(fd) {
  __vFd(fd);
  __fsCall("fsync", "", () => __wjs2_fs_fsync(fd, false));
}
export function fdatasyncSync(fd) {
  __vFd(fd);
  __fsCall("fsync", "", () => __wjs2_fs_fsync(fd, true));
}
export function fstatSync(fd) {
  __vFd(fd);
  // throwIfNoEntry 只豁免路径 ENOENT——fd EBADF 恒抛（stat-bigint 套件逐项）。
  const __o = arguments[1];
  return new __Stats(JSON.parse(__fsCall("fstat", "", () => __wjs2_fs_fstat(fd))), __o?.bigint === true);
}
export function fchmodSync(fd, mode) {
  __vFd(fd);
  __vModeArg(mode);
  __fsCall("fchmod", "", () => __wjs2_fs_fchmod(fd, __fsModeNum(mode)));
}
export function futimesSync(fd, atime, mtime) {
  __vFd(fd);
  __fsCall("futimes", "", () => __wjs2_fs_futimes(fd, __fsUtimeMs(atime, "atime"), __fsUtimeMs(mtime, "mtime")));
}
// ---- 9c：opendir / Dir（惰性游标，readdir 底座，记档非真流式）----
// node lib/internal/fs/dir.js 同构（10f 对拍）：path 原型 getter 带 brand 门
// （ERR_INVALID_THIS）、close 后 ERR_DIR_CLOSED、async 在途时同步操作
// ERR_DIR_CONCURRENT_OPERATION（async 互排队）、close(cb) 错误走回调。
export class Dir {
  #entries;
  #cursor = 0;
  #closed = false;
  #path;
  #opQueue = null;
  constructor(path) {
    // node：new fs.Dir() 无 handle → ERR_MISSING_ARGS（test-fs-read 点名）。
    if (path === undefined || path === null) {
      const e = new TypeError('The "handle" argument must be specified');
      e.code = "ERR_MISSING_ARGS"; throw e;
    }
    this.#path = path;
    this.#entries = readdirSync(path, { withFileTypes: true });
  }
  get path() {
    if (!(#path in this)) {
      const e = new TypeError('Value of "this" must be of type Dir');
      e.code = "ERR_INVALID_THIS"; throw e;
    }
    return this.#path;
  }
  #dirClosedErr() {
    const err = new Error("Directory handle was closed");
    err.code = "ERR_DIR_CLOSED";
    return err;
  }
  #dirClosed() { throw this.#dirClosedErr(); }
  #readOne() {
    return this.#cursor < this.#entries.length ? this.#entries[this.#cursor++] : null;
  }
  readSync() {
    if (this.#closed) this.#dirClosed();
    if (this.#opQueue !== null) {
      const err = new Error("Cannot do synchronous work on directory handle with concurrent asynchronous operations");
      err.code = "ERR_DIR_CONCURRENT_OPERATION"; throw err;
    }
    return this.#readOne();
  }
  // node 口径：目录项立即读（同步）再包 promise——延迟读会在 close() 后才
  // 执行抛 ERR_DIR_CLOSED（phase9c 现形）。promise 形在途时，后到的 async
  // read/close 进 #opQueue 排队（node operationQueue 同构）。
  read() {
    if (arguments.length > 0) {
      // node #readImpl 顺序：closed 先于 callback 校验。
      if (this.#closed) this.#dirClosed();
      const cb = arguments[0];
      if (typeof cb !== "function") __vErrType("callback", "function", cb);
      const self = this;
      queueMicrotask(() => { let d; try { d = self.#readOne(); } catch (e) { cb(e); return; } cb(null, d); });
      return;
    }
    if (this.#closed) return Promise.reject(this.#dirClosedErr());
    const self = this;
    if (this.#opQueue !== null) {
      return new Promise((res, rej) => this.#opQueue.push(() => {
        try { res(self.#readOne()); } catch (e) { rej(e); }
      }));
    }
    this.#opQueue = [];
    return Promise.resolve().then(() => {
      const d = self.#readOne();
      const q = self.#opQueue ?? []; self.#opQueue = null;
      for (const op of q) queueMicrotask(op);
      return d;
    });
  }
  closeSync() {
    if (this.#closed) this.#dirClosed();
    if (this.#opQueue !== null) {
      const err = new Error("Cannot do synchronous work on directory handle with concurrent asynchronous operations");
      err.code = "ERR_DIR_CONCURRENT_OPERATION"; throw err;
    }
    this.#closed = true;
  }
  close(cb) {
    if (cb === undefined) {
      if (this.#closed) return Promise.reject(this.#dirClosedErr());
      if (this.#opQueue !== null) {
        return new Promise((res, rej) => this.#opQueue.push(() => {
          try { this.#closed = true; res(); } catch (e) { rej(e); }
        }));
      }
      const self = this;
      return Promise.resolve().then(() => {
        self.#closed = true;
        const q = self.#opQueue ?? []; self.#opQueue = null;
        for (const op of q) queueMicrotask(op);
      });
    }
    if (typeof cb !== "function") __vErrType("callback", "function", cb);
    if (this.#closed) {
      const err = this.#dirClosedErr();
      queueMicrotask(() => cb(err));
      return;
    }
    const self = this;
    queueMicrotask(() => { self.#closed = true; cb(null); });
  }
  // node dir.js 逐字：dispose 幂等（已关即静默返回，不重复拒绝/抛出）。
  [Symbol.dispose]() { if (this.#closed) return; this.closeSync(); }
  async [Symbol.asyncDispose]() {
    if (this.#closed) return;
    await this.close();
  }
  [Symbol.iterator]() {
    const self = this;
    return {
      next() {
        const d = self.readSync();
        return d === null ? { done: true } : { value: d, done: false };
      },
    };
  }
  // node：for-await 用 async generator（entries()）；break/return/throw 即自动
  // close（AsyncIterBreak/Return/Throw 三套件）。
  [Symbol.asyncIterator]() {
    const self = this;
    return {
      async next() {
        const d = await self.read();
        return d === null ? { done: true } : { value: d, done: false };
      },
      async return() {
        try { await self.close(); } catch {}
        return { done: true };
      },
    };
  }
}
export function opendirSync(p, opts) {
  __fsEncoding(opts);
  // node Dir 构造：validateUint32(bufferSize, 'options.bufferSize', true)——
  // 非 number ARG_TYPE；0/负/非整 OOR（>= 1 && <= 4294967295）。
  const bs = (opts && typeof opts === "object" && !Array.isArray(opts)) ? opts.bufferSize : undefined;
  if (bs !== undefined) {
    if (typeof bs !== "number") {
      const e = new TypeError(`The "options.bufferSize" property must be of type number. Received ${__vReceived(bs)}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    if (!Number.isInteger(bs)) {
      const e = new RangeError(`The value of "options.bufferSize" is out of range. It must be an integer. Received ${bs}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
    if (bs < 1 || bs > 4294967295) {
      const e = new RangeError(`The value of "options.bufferSize" is out of range. It must be >= 1 && <= 4294967295. Received ${bs}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
  }
  p = __fsPath(p, "opendir");
  return __fsCall("opendir", p, () => new Dir(p));
}
// ---- 9c：FileHandle + fs.promises（promises 挂 node:fs 本体，fs/promises 反向
// re-export 免环；底层同步实现，文档口径不变）----
// node 口径：FileHandle 即 EventEmitter（'close' 事件）+ [Symbol.dispose]
// （触发 close，不 await——node 26 explicit resource management 面）。
export class FileHandle extends EventEmitter {
  constructor(fd) {
    super();
    this.fd = fd;
  }
  [Symbol.dispose]() {
    this.close();
  }
  async [Symbol.asyncDispose]() {
    await this.close();
  }
  close() {
    // node 口径：close 幂等——缓存 close promise，二次调用返回同一 promise
    //（流收尾会代调 handle.close()，test-fs-promises-file-handle-read 显式
    // 二次 close 不炸不重发）。
    if (this.__closePromise) return this.__closePromise;
    const fd = this.fd;
    this.__closePromise = Promise.resolve()
      .then(() => __fsCall("close", "", () => __wjs2_fs_close(fd)))
      .then(() => {
        this.fd = -1;
        this.emit("close");
      });
    return this.__closePromise;
  }
  // node internal/fs/promises.js read()/write() 同构（10f）：params 形 /
  // (buffer, options) 形 / null 位置归 -1，返回 { bytesRead, buffer }。
  read(buffer, offset, length, position) {
    return Promise.resolve().then(() => {
      let b = buffer;
      if (!ArrayBuffer.isView(b)) {
        // fh.read(params)
        ({ buffer: b = Buffer.alloc(16384), offset = 0, length = b.byteLength - offset, position = null } = buffer ?? {});
      }
      if (offset !== null && typeof offset === "object") {
        // fh.read(buffer, options)
        ({ offset = 0, length = b.byteLength - offset, position = null } = offset);
      }
      if (offset == null) offset = 0;
      length ??= b.byteLength - offset;
      if (position == null) position = -1;
      // node 序：length===0 先行返回（空 buffer + 零长读合法，回 {bytesRead: 0}；
      // 空 buffer 错只在 length>0 时抛——read 空形套件点名）。
      if (length === 0) return { bytesRead: 0, buffer: b };
      if (b.byteLength === 0) __fsEmptyBufferErr(b);
      const n = readSync(this.fd, b, offset, length, position);
      return { bytesRead: n || 0, buffer: b };
    });
  }
  write(buffer, offsetOrOptions, length, position) {
    return Promise.resolve().then(() => {
      if (typeof buffer !== "string" && !ArrayBuffer.isView(buffer)) {
        const e = new TypeError(`The "buffer" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received ${typeof buffer}`);
        e.code = "ERR_INVALID_ARG_TYPE"; throw e;
      }
      if (buffer?.byteLength === 0) return { bytesWritten: 0, buffer };
      let offset = offsetOrOptions;
      if (ArrayBuffer.isView(buffer)) {
        if (typeof offset === "object" && offset !== null) {
          ({ offset = 0, length = buffer.byteLength - offset, position = null } = offsetOrOptions ?? {});
        }
        if (offset == null) offset = 0;
        if (typeof length !== "number") length = buffer.byteLength - offset;
        if (typeof position !== "number") position = null;
        const n = writeSync(this.fd, buffer, offset, length, position);
        return { bytesWritten: n || 0, buffer };
      }
      // 字符串形态：(str[, position[, encoding]])
      const pos = typeof offset === "number" ? offset : -1;
      const enc = typeof length === "string" ? length : "utf8";
      const bytes = enc === "utf8" ? __fsData(buffer, "write") : Buffer.from(buffer, enc);
      const n = __fsCall("write", "", () => __wjs2_fs_write_fd(this.fd, bytes, pos));
      return { bytesWritten: n || 0, buffer };
    });
  }
  chown(uid, gid) { return Promise.resolve().then(() => fchownSync(this.fd, uid, gid)); }
  readv(buffers, position) {
    return Promise.resolve().then(() => {
      const n = readvSync(this.fd, buffers, typeof position === "number" ? position : null);
      return { bytesRead: n || 0, buffers };
    });
  }
  writev(buffers, position) {
    return Promise.resolve().then(() => {
      const n = writevSync(this.fd, buffers, typeof position === "number" ? position : null);
      return { bytesWritten: n || 0, buffers };
    });
  }
  async *readLines(options) {
    // node FileHandle.readLines：行异步迭代（readline 语义：\r\n 合一拆分，
    // 尾换行不产空尾行）。按 0x0A 字节切分——多字节字符不可能含换行字节，
    // 跨界天然安全；逐行再 utf8 解码。
    let rem = Buffer.alloc(0);
    while (true) {
      const b = Buffer.alloc(65536);
      const { bytesRead } = await this.read(b, 0, 65536, null);
      if (!bytesRead) {
        if (rem.length > 0) yield rem.toString("utf8");
        return;
      }
      rem = Buffer.concat([rem, Buffer.from(b.subarray(0, bytesRead))]);
      let start = 0;
      for (let i = 0; i < rem.length; i++) {
        if (rem[i] === 10) {
          let ln = rem.subarray(start, i).toString("utf8");
          if (ln.endsWith("\r")) ln = ln.slice(0, -1);
          yield ln;
          start = i + 1;
        }
      }
      rem = rem.subarray(start);
    }
  }
  createReadStream(options) {
    // node 口径：fs 流 + { ...options, fd: this }（fd 形流，非 path）。
    return new ReadStream(undefined, { ...options, fd: this });
  }
  createWriteStream(options) {
    return new WriteStream(undefined, { ...options, fd: this });
  }
  stat(options) {
    // node 口径：close 后 fd=-1 → binding 层 EBADF（非范围校验错误）。
    return Promise.resolve().then(() => {
      if (this.fd === -1) {
        const e = new Error("EBADF: bad file descriptor, fstat");
        e.code = "EBADF"; e.errno = -9; e.syscall = "fstat";
        throw e;
      }
      return fstatSync(this.fd, options);
    });
  }
  truncate(len) { return Promise.resolve().then(() => ftruncateSync(this.fd, len ?? 0)); }
  chmod(mode) { return Promise.resolve().then(() => fchmodSync(this.fd, mode)); }
  utimes(atime, mtime) { return Promise.resolve().then(() => futimesSync(this.fd, atime, mtime)); }
  sync() { return Promise.resolve().then(() => fsyncSync(this.fd)); }
  datasync() { return Promise.resolve().then(() => fdatasyncSync(this.fd)); }
  readFile(opts) {
    const enc = __fsEncoding(opts);
    const signal = opts && typeof opts === "object" ? opts.signal : undefined;
    if (signal) {
      if (typeof signal.addEventListener !== "function") {
        const e = new TypeError("The 'signal' option must be an AbortSignal-like object");
        e.code = "ERR_INVALID_ARG_TYPE"; throw e;
      }
      if (signal.aborted) throw __fsAbortErr(signal.reason);
    }
    const doRead = () => {
      // 前置尺寸门（readFile 2GiB 套件：稀疏大文件不得真读 2GB 再判）。
      const st = fstatSync(this.fd);
      if (st.size > __kIoMaxLength) throw __fsFileTooLarge(st.size);
      const parts = [];
      while (true) {
        const chunk = __fsCall("read", "", () => __wjs2_fs_read_fd(this.fd, 1 << 20, -1));
        if (chunk.length === 0) break;
        parts.push(chunk);
      }
      const out = new Uint8Array(parts.reduce((a, c) => a + c.length, 0));
      let off = 0;
      for (const c of parts) { out.set(c, off); off += c.length; }
      return __fsDecode(out, enc, "readFile");
    };
    if (signal) {
      // node 读为线程池异步——abort 竞速即拒（file-handle-readFile cancel 套件）。
      return Promise.race([
        new Promise((resolve, reject) => {
          setTimeout(() => {
            try {
              if (signal.aborted) { reject(__fsAbortErr(signal.reason)); return; }
              resolve(doRead());
            } catch (e) { reject(e); }
          }, 0);
        }),
        new Promise((_, reject) =>
          signal.addEventListener("abort", () => reject(__fsAbortErr(signal.reason)), { once: true })),
      ]);
    }
    return Promise.resolve().then(doRead);
  }
  async writeFile(data, opts) {
    // node 口径：流/同步+异步可迭代逐块收；signal abort 即 AbortError
    //（cause=reason）；第二参可 string（encoding）或 { encoding, signal }。
    if (typeof data !== "string" && !ArrayBuffer.isView(data) &&
        !(data && typeof data[Symbol.asyncIterator] === "function") &&
        !(data && typeof data[Symbol.iterator] === "function")) {
      const e = new TypeError(`The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received ${data === null ? "null" : typeof data}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    const enc = typeof opts === "string" ? opts : (opts && opts.encoding);
    const isChunk = (c) => typeof c === "string" || ArrayBuffer.isView(c);
    let parts = null;
    if (data && typeof data[Symbol.asyncIterator] === "function") {
      parts = [];
      for await (const c of data) {
        if (!isChunk(c)) { const e = new TypeError(`The "chunk" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received ${c === null ? "null" : typeof c}`); e.code = "ERR_INVALID_ARG_TYPE"; throw e; }
        parts.push(c);
      }
    } else if (data && !ArrayBuffer.isView(data) && typeof data[Symbol.iterator] === "function") {
      parts = [];
      for (const c of data) {
        if (!isChunk(c)) { const e = new TypeError(`The "chunk" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received ${c === null ? "null" : typeof c}`); e.code = "ERR_INVALID_ARG_TYPE"; throw e; }
        parts.push(c);
      }
    }
    const encChunk = (c) => {
      const u8 = typeof c === "string" && enc && enc !== "utf8" ? Buffer.from(c, enc) : __fsData(c, "writeFile");
      return new Uint8Array(u8.buffer, u8.byteOffset, u8.byteLength);
    };
    const bytes = parts
      ? (() => { const out = new Uint8Array(parts.reduce((a, c) => a + encChunk(c).length, 0)); let off = 0; for (const c of parts) { const u8 = encChunk(c); out.set(u8, off); off += u8.length; } return out; })()
      : __fsData(data, "writeFile");
    const signal = opts && typeof opts === "object" ? opts.signal : undefined;
    if (signal) {
      if (typeof signal.addEventListener !== "function") {
        const e = new TypeError("The 'signal' option must be an AbortSignal-like object");
        e.code = "ERR_INVALID_ARG_TYPE"; throw e;
      }
      if (signal.aborted) throw __fsAbortErr(signal.reason);
    }
    if (signal) {
      await Promise.race([
        new Promise((resolve, reject) => {
          // node 写经线程池异步完成；本仓为同步写——落宏任务使 abort 的
          // nextTick 检查点先于写触发（doWriteBufferAndCancel 点名）。
          setTimeout(() => {
            try {
              if (signal.aborted) { reject(__fsAbortErr(signal.reason)); return; }
              resolve(__fsCall("write", "", () => __wjs2_fs_write_fd(this.fd, bytes, 0)));
            } catch (e) { reject(e); }
          }, 0);
        }),
        new Promise((_, reject) =>
          signal.addEventListener("abort", () => reject(__fsAbortErr(signal.reason)), { once: true })),
      ]);
      return;
    }
    // position -1 = 当前位（writeFile 无 position；O_APPEND fd 落尾，
    // doWriteFileAndAppend 'HelloWorld' 点名）。
    await Promise.resolve().then(() => __fsCall("write", "", () => __wjs2_fs_write_fd(this.fd, bytes, -1)));
  }
  // node：appendFile data 面 = writeFile 同族（iterable/asyncIterable + signal
  // abort 契约；promises-file-handle-append-file 套件逐项）。
  async appendFile(data, opts) {
    const signal = opts && typeof opts === "object" ? opts.signal : undefined;
    if (signal) {
      if (typeof signal.addEventListener !== "function") {
        const e = new TypeError("The 'signal' option must be an AbortSignal-like object");
        e.code = "ERR_INVALID_ARG_TYPE"; throw e;
      }
      if (signal.aborted) throw __fsAbortErr(signal.reason);
    }
    let bytes;
    if (data && typeof data === "object" && !ArrayBuffer.isView(data) && typeof data[Symbol.asyncIterator] === "function") {
      const enc = typeof opts === "string" ? opts : (opts && opts.encoding);
      const isChunk = (c) => typeof c === "string" || ArrayBuffer.isView(c);
      const encChunk = (c) => {
        const u8 = typeof c === "string" && enc && enc !== "utf8" ? Buffer.from(c, enc) : __fsData(c, "appendFile");
        return new Uint8Array(u8.buffer, u8.byteOffset, u8.byteLength);
      };
      const parts = [];
      for await (const c of data) {
        if (signal && signal.aborted) throw __fsAbortErr(signal.reason);
        if (!isChunk(c)) __chunkArgErr(c);
        parts.push(c);
      }
      const out = new Uint8Array(parts.reduce((a, c) => a + encChunk(c).length, 0));
      let off = 0;
      for (const c of parts) { const u8 = encChunk(c); out.set(u8, off); off += u8.length; }
      bytes = out;
    } else {
      bytes = __fsDataSync(data, "data", opts);
    }
    if (signal && signal.aborted) throw __fsAbortErr(signal.reason);
    const doAppend = () => {
      const size = fstatSync(this.fd).size;
      __fsCall("write", "", () => __wjs2_fs_write_fd(this.fd, bytes, size));
    };
    if (signal) {
      // 写落宏任务 + abort 竞速（FileHandle.write 同构；bufferAndCancel 契约）。
      await Promise.race([
        new Promise((resolve, reject) => {
          setTimeout(() => {
            try {
              if (signal.aborted) { reject(__fsAbortErr(signal.reason)); return; }
              resolve(doAppend());
            } catch (e) { reject(e); }
          }, 0);
        }),
        new Promise((_, reject) =>
          signal.addEventListener("abort", () => reject(__fsAbortErr(signal.reason)), { once: true })),
      ]);
      return;
    }
    return Promise.resolve().then(doAppend);
  }
}
