const __as = (fn) => function (...args) { return Promise.resolve().then(() => fn(...args)); };
// node 口径（lib/internal/fs/watchers.js async watch 选项校验）：
// persistent/recursive 布尔（ARG_TYPE）、encoding（ARG_VALUE）、
// signal AbortSignal 形（ARG_TYPE）、maxQueue 整数（ARG_TYPE/OUT_OF_RANGE）、
// overflow 'ignore'/'error'（ARG_VALUE）、ignore 全形态复用。
function __watchOptsValidate(opts) {
  if (opts === undefined || opts === null) return {};
  if (typeof opts !== "object") {
    const e = new TypeError(`The "options" argument must be of type object. Received ${Object.prototype.toString.call(opts)}`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  const o = {};
  for (const k of ["persistent", "recursive"]) {
    if (opts[k] !== undefined && typeof opts[k] !== "boolean") {
      const e = new TypeError(`The "options.${k}" argument must be of type boolean. Received ${typeof opts[k]}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    if (opts[k] !== undefined) o[k] = opts[k];
  }
  if (opts.encoding !== undefined) {
    __fsEncoding({ encoding: opts.encoding });
    o.encoding = opts.encoding;
  }
  if (opts.signal !== undefined) o.signal = __fsSignalCheck(opts);
  if (opts.maxQueue !== undefined) {
    if (typeof opts.maxQueue !== "number") {
      const e = new TypeError(`The "options.maxQueue" argument must be of type number. Received ${typeof opts.maxQueue}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    if (!Number.isInteger(opts.maxQueue)) {
      const e = new RangeError(`The value of "options.maxQueue" is out of range. It must be an integer. Received ${opts.maxQueue}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
    o.maxQueue = opts.maxQueue;
  }
  if (opts.overflow !== undefined) {
    if (opts.overflow !== "ignore" && opts.overflow !== "error") {
      const e = new TypeError(`The argument 'options.overflow' must be one of: 'ignore', 'error'. Received '${opts.overflow}'`);
      e.code = "ERR_INVALID_ARG_VALUE"; throw e;
    }
    o.overflow = opts.overflow;
  }
  if (opts.ignore !== undefined) {
    __validateIgnoreOption(opts.ignore, "options.ignore");
    o.ignore = opts.ignore;
  }
  return o;
}
// node 口径（lib/internal/fs/watchers.js async watch）：for-await 迭代
// {eventType, filename}；校验错走 reject；abort 即 AbortError；break 后
// 重迭代即 done（finally 关 watcher）；背压 error 形入队。
async function* __promisesWatch(p, opts) {
  p = __fsPath(p, "watch");
  const o = __watchOptsValidate(opts);
  const maxQueue = o.maxQueue ?? 2048;
  const overflow = o.overflow ?? "ignore";
  const queue = [];
  let wake = null;
  let aborted = false;
  const onAbort = () => { aborted = true; if (wake) { const k = wake; wake = null; k(); } };
  if (o.signal) {
    if (o.signal.aborted) throw __fsAbortErr(o.signal.reason);
    o.signal.addEventListener("abort", onAbort, { once: true });
  }
  const w = watch(p, {
    persistent: o.persistent, recursive: o.recursive, encoding: o.encoding,
    ignore: o.ignore,
  }, (ev, fn) => {
    if (queue.length >= maxQueue) {
      if (overflow === "error") {
        queue.length = 0;
        const e = new Error(`fs.watch queue overflow (maxQueue ${maxQueue})`);
        e.code = "ERR_FS_WATCH_QUEUE_OVERFLOW";
        queue.push(e);
      } else {
        process.emitWarning("fs.watch maxQueue exceeded");
      }
    } else {
      queue.push({ eventType: ev, filename: fn });
    }
    if (wake) { const k = wake; wake = null; k(); }
  });
  try {
    for (;;) {
      while (queue.length > 0) {
        const item = queue.shift();
        if (item instanceof Error) throw item;
        yield item;
      }
      if (aborted) throw __fsAbortErr(o.signal.reason);
      await new Promise((res) => { wake = res; });
      if (aborted) throw __fsAbortErr(o.signal.reason);
    }
  } finally {
    if (o.signal) { try { o.signal.removeEventListener("abort", onAbort); } catch {} }
    try { w.close(); } catch {}
  }
}
export const promises = {
  access: __as(accessSync),
  appendFile: (p, data, opts) => Promise.resolve().then(() => __fsAppendFileAsync(p, data, opts)),
  chmod: __as(chmodSync),
  close: __as(closeSync),
  constants,
  copyFile: __as(copyFileSync),
  cp: __as(__cpAsync),
  FileHandle,
  lstat: __as(lstatSync),
  link: __as(linkSync),
  mkdir: __as(mkdirSync),
  mkdtemp: __as(mkdtempSync),
  open: (...args) => Promise.resolve().then(() => new FileHandle(openSync(...args))),
  opendir: __as(opendirSync),
  readFile: function (...args) {
    // node 读走线程池——abort 竞速（nextTick 形）必须能赢同步底座：
    // signal 在场即让一轮 macrotask 再复查 aborted，否则同步读恒赢
    // （file-handle-readFile tick-0 套件点名）。无 signal 走直路零开销。
    const opts = args[1];
    const sig = opts && typeof opts === "object" ? __fsSignalCheck(opts) : null;
    if (!sig) return Promise.resolve().then(() => readFileSync(...args));
    return new Promise((resolve, reject) => {
      setTimeout(() => {
        try {
          if (sig.aborted) { reject(__fsAbortErr(sig.reason)); return; }
          resolve(readFileSync(...args));
        } catch (e) { reject(e); }
      }, 0);
    });
  },
  readdir: __as(readdirSync),
  readlink: __as(readlinkSync),
  realpath: __as(realpathSync),
  rename: __as(renameSync),
  rm: __as(rmSync),
  rmdir: __as(rmdirSync),
  mkdtempDisposable: mkdtempDisposableProm,
  readv: (fd, buffers, position) => Promise.resolve().then(() => ({ bytesRead: readvSync(fd, buffers, position) || 0, buffers })),
  writev: (fd, buffers, position) => Promise.resolve().then(() => ({ bytesWritten: writevSync(fd, buffers, position) || 0, buffers })),
  stat: __as(statSync),
  statfs: __as(statfsSync),
  symlink: __as(symlinkSync),
  truncate: __as(truncateSync),
  unlink: __as(unlinkSync),
  utimes: __as(utimesSync),
  lutimes: __as(lutimesSync),
  writeFile: __as(writeFileSync),
  chown: __as(chownSync),
  lchown: __as(lchownSync),
  fchown: __as(fchownSync),
  futimes: __as(futimesSync),
  ...(typeof lchmodSync === "function" ? { lchmod: __as(lchmodSync) } : {}),
  watch: __promisesWatch,
};
// ---- 9c：回调全家（err-first；promise 底座经 queueMicrotask 派发）----
function __nodeify(p, cb) {
  __vCbArg(cb);
  p.then(
    // node 口径：无结果 API（close/access 等）回调只带 (err)，不补 undefined
    //（test-fs-close：deepStrictEqual(args, [null]) 点名）。
    (v) => queueMicrotask(() => v === undefined ? cb(null) : cb(null, v)),
    (e) => queueMicrotask(() => cb(e)),
  );
}
const __cb1 = (syncFn, name, before) => function (...args) {
  let cb = args[args.length - 1];
  __vCbArg(cb);
  const rest = args.slice(0, -1);
  // node 口径：参数校验错误（ERR_INVALID_ARG_* / ERR_OUT_OF_RANGE）同步抛，
  // 操作错误（ENOENT 等）走回调（syncFn 立即执行，回调仍经 queueMicrotask）。
  let p;
  try {
    p = Promise.resolve(syncFn(...before(rest)));
  } catch (e) {
    if (e && typeof e.code === "string" &&
        (e.code === "ERR_INVALID_ARG_TYPE" || e.code === "ERR_INVALID_ARG_VALUE" || e.code === "ERR_OUT_OF_RANGE")) throw e;
    p = Promise.reject(e);
  }
  __nodeify(p, cb);
};
const __fdCb = (syncFn, name) => function (...args) {
  let cb = args[args.length - 1];
  if (typeof cb !== "function") {
    // node 口径：值校验错误优先抛（/fd|uid|gid/），操作错误让位于
    // callback 校验（lchown 套件：值错误 ARG_TYPE / 操作错误 + 无效 cb
    // → ARG_TYPE /callback/；不得造孤儿 rejected promise → unhandled）。
    try { syncFn(...args); } catch (e) {
      if (e && typeof e.code === "string" &&
          (e.code === "ERR_INVALID_ARG_TYPE" || e.code === "ERR_INVALID_ARG_VALUE" || e.code === "ERR_OUT_OF_RANGE")) throw e;
    }
    __vCbArg(cb);
    return;
  }
  const rest = args.slice(0, -1);
  let p;
  try {
    p = Promise.resolve(syncFn(...rest));
  } catch (e) {
    if (e && typeof e.code === "string" &&
        (e.code === "ERR_INVALID_ARG_TYPE" || e.code === "ERR_INVALID_ARG_VALUE" || e.code === "ERR_OUT_OF_RANGE")) throw e;
    p = Promise.reject(e);
  }
  __nodeify(p, cb);
};

const __id = (a) => a;
// readFile 回调面定制（非泛型 __cb1）：node 读在线程池异步执行，abort 可落
// "读期间"；本仓读同步完成，等价窗口 = 同步读之后、交付之前——在 promise
// then（microtask，先于 __nodeify 的交付）里复查 signal.aborted，成功转
// AbortError（readfile 套件 "cancellation, during read" 点名；nextTick abort
// 先于 microtask 交付，纯同步复查抓不到）。校验/已abort 同步抛路径不变。
export const readFile = function (...args) {
  let cb = args[args.length - 1];
  __vCbArg(cb);
  const rest = args.slice(0, -1);
  const sig = (() => {
    const opts = rest[1];
    return opts && typeof opts === "object" && typeof opts.signal === "object" && opts.signal !== null && typeof opts.signal.addEventListener === "function" ? opts.signal : null;
  })();
  let p;
  try {
    p = Promise.resolve(readFileSync(...rest));
  } catch (e) {
    if (e && typeof e.code === "string" &&
        (e.code === "ERR_INVALID_ARG_TYPE" || e.code === "ERR_INVALID_ARG_VALUE" || e.code === "ERR_OUT_OF_RANGE")) throw e;
    p = Promise.reject(e);
  }
  if (sig && !sig.aborted) {
    p = p.then((v) => (sig.aborted ? Promise.reject(__fsAbortErr(sig.reason)) : v));
  }
  __nodeify(p, cb);
};
// writeFile 回调面定制（镜像 readFile）：本仓写同步完成，abort 只能落在
// "同步写之后、交付之前"——then（microtask，先于 __nodeify 交付）里复查
// signal.aborted，成功转 AbortError（write-file 套件 cancellable 点名）。
export const writeFile = function (...args) {
  let cb = args[args.length - 1];
  __vCbArg(cb);
  const rest = args.slice(0, -1);
  // node writeFile 形：(path, data, options, callback)——opts 在 rest[2]
  //（readFile 是 (path, options, cb)，opts 在 rest[1]，两者勿混）。
  const sig = (() => {
    const opts = rest[2];
    return opts && typeof opts === "object" && typeof opts.signal === "object" && opts.signal !== null && typeof opts.signal.addEventListener === "function" ? opts.signal : null;
  })();
  let p;
  try {
    p = Promise.resolve(writeFileSync(...rest));
  } catch (e) {
    if (e && typeof e.code === "string" &&
        (e.code === "ERR_INVALID_ARG_TYPE" || e.code === "ERR_INVALID_ARG_VALUE" || e.code === "ERR_OUT_OF_RANGE")) throw e;
    p = Promise.reject(e);
  }
  if (sig && !sig.aborted) {
    p = p.then((v) => (sig.aborted ? Promise.reject(__fsAbortErr(sig.reason)) : v));
  }
  __nodeify(p, cb);
};
export const appendFile = __cb1(appendFileSync, "appendFile", __id);
export const stat = __cb1(statSync, "stat", __id);
export const statfs = __cb1(statfsSync, "statfs", __id);
export const lstat = __cb1(lstatSync, "lstat", __id);
export const mkdir = __cb1(mkdirSync, "mkdir", __id);
export const rmdir = __cb1(rmdirSync, "rmdir", __id);
export const rm = __cb1(rmSync, "rm", __id);
export const unlink = __cb1(unlinkSync, "unlink", __id);
export const readdir = __cb1(readdirSync, "readdir", __id);
export const rename = __cb1(renameSync, "rename", __id);
export const copyFile = __cb1(copyFileSync, "copyFile", __id);
export const realpath = __cb1(realpathSync, "realpath", __id);
export const mkdtemp = __cb1(mkdtempSync, "mkdtemp", __id);
export const access = __cb1(accessSync, "access", __id);
export const truncate = __fdCb(truncateSync, "truncate");
export const utimes = __fdCb(utimesSync, "utimes");
export const lutimes = __fdCb(lutimesSync, "lutimes");
export const chmod = __cb1(chmodSync, "chmod", __id);
export const link = __cb1(linkSync, "link", __id);
export const symlink = __cb1(symlinkSync, "symlink", __id);
export const readlink = __cb1(readlinkSync, "readlink", __id);
export const opendir = __cb1(opendirSync, "opendir", __id);
export const chown = __cb1(chownSync, "chown", __id);
// f 系回调包装：node 口径 fd/mode 等实参校验先于 cb（fchmod(1,'123x') →
// ARG_VALUE 而非 cb 错误），cb 缺省仍抛 callback 类型错。
export const fchown = __fdCb(fchownSync, "fchown");
export const lchown = __fdCb(lchownSync, "lchown");
export const lchmod = lchmodSync ? __fdCb(lchmodSync, "lchmod") : undefined;
// node 内部门（lib/fs.js 导出；utimes/timestamp-parsing 套件直用）。
export const _toUnixTimestamp = __toUnixTimestamp;
export const fchmod = __fdCb(fchmodSync, "fchmod");
export const fstat = __fdCb(fstatSync, "fstat");
export const ftruncate = __fdCb(ftruncateSync, "ftruncate");
export const fsync = __fdCb(fsyncSync, "fsync");
export const fdatasync = __fdCb(fdatasyncSync, "fdatasync");
export const futimes = __fdCb(futimesSync, "futimes");
// mkdtempDisposable（10f，node 26 口径）：{ path, remove, [Symbol.dispose /
// asyncDispose] }。remove 锁创建期绝对路径（"Stash the full path in case of
// process.chdir()"）；promises 版 remove 为 async（assert.rejects 契约）。
function __pathResolve(p) {
  if (typeof p === "string" && p.startsWith("/")) return p;
  const cwd = process.cwd();
  return cwd.endsWith("/") ? cwd + p : cwd + "/" + p;
}
export function mkdtempDisposableSync(prefix, opts) {
  const p = mkdtempSync(prefix, opts);
  const fullPath = __pathResolve(p);
  return {
    path: p,
    remove() { rmSync(fullPath, { recursive: true, force: true }); },
    [Symbol.dispose]() { this.remove(); },
    async [Symbol.asyncDispose]() { this.remove(); },
  };
}
async function mkdtempDisposableProm(prefix, opts) {
  const p = mkdtempSync(prefix, opts);
  const fullPath = __pathResolve(p);
  return {
    path: p,
    async remove() {
      // node rimraf：缺失路径静默（幂等）；EACCES/EPERM 等真实错误必须透传
      //（只读父目录用例断言 rejects /EACCES|EPERM/）。
      try { rmSync(fullPath, { recursive: true, force: true }); }
      catch (e) { if (e && e.code === "ENOENT") return; throw e; }
    },
    async [Symbol.asyncDispose]() { await this.remove(); },
  };
}
export const cp = __cb1((src, dst, opts) => {
  // 同步校验前置（真机异步 cp 选项错同步抛，操作错才走回调；__cb1 只对
  // 同步抛的 ARG_* 系直抛，async 内的校验会落成 rejection）。
  __cpValidateOptions(opts);
  return __cpAsync(src, dst, opts);
}, "cp", __id);
export const open = __cb1(openSync, "open", __id);
export function close(fd, cb) {
  __vFd(fd);
  if (cb === undefined) cb = __nop;
  __vCbArg(cb);
  __nodeify(Promise.resolve().then(() => closeSync(fd)), cb);
}
function __nop() {}
export function exists(p, cb) {
  __vCbArg(cb);
  queueMicrotask(() => cb(existsSync(p)));
}
// promisify(fs.exists) → boolean（node：回调非 err-first，走 custom promisified）。
exists[Symbol.for("nodejs.util.promisify.custom")] = function (path) {
  // 内层引外层导出（不得命名内函数——遮蔽后自递归，promisified 套件现形）。
  return new Promise((resolve) => exists(path, resolve));
};
// 回调 read/write 全形态（10f，node lib/fs.js read()/write() 同构）：
// read(fd, cb) / read(fd, params, cb) / read(fd, buffer, options, cb) /
// read(fd, buffer, offset, length, position, cb)；write 同族 + 字符串形态。
export function read(fd, buffer, offsetOrOptions, length, position, callback) {
  __vFd(fd);
  let cb = callback;
  let offset = offsetOrOptions;
  let params = null;
  if (arguments.length <= 4) {
    if (arguments.length === 4) {
      // fs.read(fd, buffer, options, cb)
      cb = length;
      params = offsetOrOptions;
    } else if (arguments.length === 3) {
      // fs.read(fd, bufferOrParams, cb)
      if (!ArrayBuffer.isView(buffer)) {
        // node 同构：({ buffer = Buffer.alloc(16384) } = params ?? {})——
        // params.buffer 为 null 时 buffer 恒 null（不得落默认；read 套件点名）。
        params = buffer;
        ({ buffer = Buffer.alloc(16384) } = params ?? {});
      }
      cb = offsetOrOptions;
    } else {
      // fs.read(fd, cb)
      cb = buffer;
      buffer = Buffer.alloc(16384);
    }
    // node 原文：buffer?.byteLength（params.buffer 为 null 时由 validateBuffer 报）。
    ({ offset = 0, length = buffer?.byteLength - offset, position = null } = params ?? {});
  }
  if (typeof cb !== "function") {
    const e = new TypeError(`The "cb" argument must be of type function. Received ${cb === null ? "null" : typeof cb}`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  __vBuffer(buffer);
  if (offset == null) offset = 0;
  else __vInteger(offset, "offset", 0);
  length |= 0;
  if (position == null) position = -1;
  if (length === 0) { queueMicrotask(() => cb(null, 0, buffer)); return; }
  __vEmptyBuffer(buffer);
  __vOffsetLength(offset, length, buffer.byteLength);
  Promise.resolve().then(() => readSync(fd, buffer, offset, length, position))
    .then(
      (n) => queueMicrotask(() => cb(null, n || 0, buffer)),
      (e) => queueMicrotask(() => cb(e)),
    );
}
// util.promisify(fs.read) → { bytesRead, buffer }（test-fs-promisified 点名）。
read[Symbol.for("nodejs.util.promisify.customArgs")] = ["bytesRead", "buffer"];

export function write(fd, buffer, offsetOrOptions, length, position, callback) {
  __vFd(fd);
  let offset = offsetOrOptions;
  if (ArrayBuffer.isView(buffer)) {
    callback ||= position || length || offset;
    if (typeof callback !== "function") {
      const e = new TypeError(`The "cb" argument must be of type function. Received ${callback === null ? "null" : typeof callback}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    let cb = callback;
    if (typeof offset === "object" && offset !== null) {
      // fs.write(fd, buffer, options, cb)
      ({ offset = 0, length = buffer.byteLength - offset, position = null } = offsetOrOptions ?? {});
    }
    if (offset == null || typeof offset === "function") offset = 0;
    else __vInteger(offset, "offset", 0);
    if (typeof length !== "number") length = buffer.byteLength - offset;
    if (typeof position !== "number") position = null;
    __fsValidateOffsetLengthWrite(offset, length, buffer.byteLength);
    Promise.resolve().then(() => writeSync(fd, buffer, offset, length, position))
      .then(
        (n) => queueMicrotask(() => cb(null, n || 0, buffer)),
        (e) => queueMicrotask(() => cb(e)),
      );
    return;
  }
  // node：非 view 非串（含 {} / Date / Promise / function / primitive）一律
  // validateBuffer ARG_TYPE（write-optional-params 'first argument not wrongly
  // interpreted' 族；不得走字符串分支静默成功）。
  if (typeof buffer !== "string") {
    __vBuffer(buffer);
  }
  // 字符串形态：(fd, str, cb) / (fd, str, position, cb) / (fd, str, position, encoding, cb)
  if (typeof position !== "function") {
    if (typeof offset === "function") { position = offset; offset = null; }
    else position = length;
    length = "utf8";
  }
  const cb = position;
  if (typeof cb !== "function") {
    const e = new TypeError(`The "cb" argument must be of type function. Received ${cb === null ? "null" : typeof cb}`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  const pos = typeof offset === "number" ? offset : -1;
  Promise.resolve().then(() => writeSync(fd, buffer, pos))
    .then(
      (n) => queueMicrotask(() => cb(null, n || 0, buffer)),
      (e) => queueMicrotask(() => cb(e)),
    );
}
write[Symbol.for("nodejs.util.promisify.customArgs")] = ["bytesWritten", "buffer"];

// readv/writev（10f：JS 顺序合成，非原子——测试可见面 {bytesRead, buffers} 同构；
// 校验同步先抛——node getValidatedFd/validateBufferArray/validateFunction 同序）。
export function readvSync(fd, buffers, position) {
  __vFd(fd);
  __fsValidateBufferArray(buffers);
  let total = 0;
  for (const b of buffers) {
    const n = readSync(fd, b, 0, b.byteLength, typeof position === "bigint" ? Number(position) + total : (typeof position === "number" ? position + total : null));
    total += n;
    if (n < b.byteLength) break;
  }
  return total;
}
export function writevSync(fd, buffers, position) {
  __vFd(fd);
  __fsValidateBufferArray(buffers);
  let total = 0;
  for (const b of buffers) {
    const n = writeSync(fd, b, 0, b.byteLength, typeof position === "bigint" ? Number(position) + total : (typeof position === "number" ? position + total : null));
    total += n;
    if (n < b.byteLength) break;
  }
  return total;
}
export function readv(fd, buffers, position, cb) {
  if (typeof position === "function") { cb = position; position = null; }
  __vFd(fd);
  __fsValidateBufferArray(buffers);
  if (typeof cb !== "function") __vErrType("cb", "function", cb);
  Promise.resolve().then(() => readvSync(fd, buffers, position))
    .then(
      (n) => queueMicrotask(() => cb(null, n || 0, buffers)),
      (e) => queueMicrotask(() => cb(e)),
    );
}
readv[Symbol.for("nodejs.util.promisify.customArgs")] = ["bytesRead", "buffers"];
export function writev(fd, buffers, position, cb) {
  if (typeof position === "function") { cb = position; position = null; }
  __vFd(fd);
  __fsValidateBufferArray(buffers);
  if (typeof cb !== "function") __vErrType("cb", "function", cb);
  Promise.resolve().then(() => writevSync(fd, buffers, position))
    .then(
      (n) => queueMicrotask(() => cb(null, n || 0, buffers)),
      (e) => queueMicrotask(() => cb(e)),
    );
}
writev[Symbol.for("nodejs.util.promisify.customArgs")] = ["bytesWritten", "buffers"];

const __api = {
  // 同步（Phase 4 基础面）
  readFileSync, writeFileSync, appendFileSync, statSync, lstatSync, existsSync,
  mkdirSync, rmSync, rmdirSync, unlinkSync, readdirSync, renameSync, copyFileSync,
  realpathSync, mkdtempSync, watch, watchFile, unwatchFile, constants, createReadStream, createWriteStream, ReadStream, WriteStream,
  // 同步（Phase 9c 增补）
  accessSync, truncateSync, utimesSync, chmodSync, chownSync, fchownSync, linkSync, symlinkSync, readlinkSync,
  cpSync, opendirSync, openSync, closeSync, readSync, writeSync, ftruncateSync,
  fstatSync, fchmodSync, futimesSync, fsyncSync, fdatasyncSync, statfsSync,
  readvSync, writevSync,
  // 回调面（Phase 9c）
  readFile, writeFile, appendFile, stat, statfs, lstat, exists, mkdir, rmdir, rm, unlink,
  readdir, rename, copyFile, realpath, mkdtemp, access, truncate, utimes, chmod,
  link, symlink, readlink, open, close, read, write, readv, writev, chown, fchown, fchmod, fstat, ftruncate, fsync, fdatasync, futimes, opendir, cp,
  lchown, lchownSync, _toUnixTimestamp, lutimes, lutimesSync,
  ...(lchmod ? { lchmod, lchmodSync } : {}),
  mkdtempDisposable: mkdtempDisposableSync,
  // node 26 两条名都在（mkdtempDisposableSync 套件 `require('fs')` 点名）。
  mkdtempDisposableSync,
  // 类 + promises
  Stats: __Stats, Dirent: __Dirent, StatsFs: __StatsFs, Dir, FileHandle, promises,
};
export default __api;
export { __Stats as Stats, __Dirent as Dirent, __StatsFs as StatsFs };
