import { Readable, Writable } from 'node:stream';
import { EventEmitter } from 'node:events';
import { inspect } from 'node:util';
import errors from 'node:internal/errors';
const {
  codes: {
    ERR_INVALID_ARG_TYPE: { HideStackFramesError: ERR_INVALID_ARG_TYPE },
  },
} = errors;

function __fsErr(e, syscall, path) {
  const m = String((e && e.message) || e);
  // JS 侧已带 code 的类型/范围错误直通（__fsCall 闭包内抛的校验错误
  // 不得被重包成 UNKNOWN——fchmod '123x' 套件现形）。
  if (e && typeof e.code === "string" && e.code !== "" && e.code !== "UNKNOWN") {
    if (e.path === undefined) e.path = path;
    throw e;
  }
  // 权限拒绝直通（不套 io 形状；Deno NotCapable 同款可读错）。
  // 前缀必须保留原文：__fsCall 嵌套时同一错误会过 __fsErr 两次，剥掉即失认、
  // 二次被重包成 UNKNOWN（allow-list 读路径现形）。
  if (m.startsWith("PermissionError:")) {
    const perr = new Error(m);
    perr.name = "PermissionError";
    perr.path = path;
    throw perr;
  }
  const code = (m.match(/^([A-Z_]+): /) || [])[1] || "UNKNOWN";
  // native report_io 已产出 node 形状（`CODE: msg, syscall 'path'`）→ 直通，
  // 内层 syscall 为准（truncate 的 open 失败 node 口径 syscall='open'，
  // truncate 套件 message 逐字点名）。
  const ioShape = m.match(/^([A-Z_]+): (.*), ([a-z_]+) '(.*)'$/);
  if (ioShape) {
    const err = new Error(m);
    err.code = ioShape[1];
    err.syscall = ioShape[3];
    err.path = ioShape[4];
    throw err;
  }
  const rest = m.replace(/^[A-Z_]+: /, "");
  const err = new Error(`${code}: ${rest}, ${syscall} '${path}'`);
  err.code = code;
  err.syscall = syscall;
  err.path = path;
  throw err;
}
function __fsCall(syscall, path, fn) {
  try {
    return fn();
  } catch (e) {
    __fsErr(e, syscall, path);
  }
}
function __fsPath(p, what, argName) {
  if (typeof p === "string") {
    // node getValidatedPath：\0 即 ERR_INVALID_ARG_VALUE（null-bytes 套件全 API 面）。
    if (p.includes("\0")) {
      const e = new TypeError(`The argument 'path' must be a string, Uint8Array, or URL without null bytes. Received '${p}'`);
      e.code = "ERR_INVALID_ARG_VALUE"; throw e;
    }
    return p;
  }
  if (p instanceof URL) {
    if (p.protocol !== "file:") throw new TypeError(`${what}: only file: URLs supported`);
    // URL 形同样禁 \0（null-bytes 套件 fileUrl/fileUrl2 两形 + 真机 26.8.2
    // 逐字对拍：ARG_VALUE 'without null bytes'）。pathname 解码后判。
    let dec = p.pathname;
    try { dec = decodeURIComponent(p.pathname); } catch { }
    if (dec.includes("\0")) {
      const e = new TypeError(`The argument 'path' must be a string, Uint8Array, or URL without null bytes. Received '${p.href}'`);
      e.code = "ERR_INVALID_ARG_VALUE"; throw e;
    }
    return p;
  }
  if (ArrayBuffer.isView(p)) {
    const s = Buffer.from(p).toString("utf8");
    if (s.includes("\0")) {
      const e = new TypeError(`The argument 'path' must be a string, Uint8Array, or URL without null bytes. Received '${s}'`);
      e.code = "ERR_INVALID_ARG_VALUE"; throw e;
    }
    return s;
  }
  // node getValidatedPath 口径（test-fs-buffer 点名 message 逐字）。
  __vErrType(argName ?? "path", "string or an instance of Buffer or URL", p);
}
function __fsAbortErr(reason) {
  const e = new Error("The operation was aborted");
  e.name = "AbortError";
  e.code = "ABORT_ERR";
  if (reason !== undefined) e.cause = reason;
  return e;
}
// options.signal 面：键存在（≠undefined）即必须是 AbortSignal 形
//（aborted:boolean + addEventListener 函数；跨域 instanceof 不可靠，§4.57
// 结构判），非法即同步 TypeError ERR_INVALID_ARG_TYPE（readfile 套件
// signal:'hello' 点名）；返回 signal 供 aborted 检查。
function __fsSignalCheck(opts) {
  if (opts && typeof opts === "object" && opts.signal !== undefined) {
    const s = opts.signal;
    if (!s || typeof s !== "object" ||
        typeof s.aborted !== "boolean" || typeof s.addEventListener !== "function") {
      const e = new TypeError('The "options.signal" property must be of type AbortSignal. Received ' + String(s));
      e.code = "ERR_INVALID_ARG_TYPE";
      throw e;
    }
    return s;
  }
  return null;
}
function __fsData(d, what) {
  if (typeof d === "string") return new TextEncoder().encode(d);
  if (d instanceof Uint8Array) return d;
  if (d instanceof ArrayBuffer) return new Uint8Array(d);
  if (ArrayBuffer.isView(d)) return new Uint8Array(d.buffer, d.byteOffset, d.byteLength);
  // node validateBufferData 口径：fs.write/writeSync 参数名 "buffer"，
  // writeFile/appendFile 系 "data"（append-file/buffertype 套件 message 断言）。
  __vErrType(what === "write" ? "buffer" : "data",
             "string or an instance of Buffer, TypedArray, or DataView", d);
}
// 顶层 string data 的 encoding 面（'hex'/'base64'/'latin1' 等解码为字节；
// write-file-sync 套件 hex roundtrip 点名——旧形按 utf8 直写原串）。
function __fsDataEnc(d, what, enc) {
  if (typeof d === "string" && enc && enc !== "utf8" && enc !== "utf-8") {
    return Buffer.from(d, enc);
  }
  return __fsData(d, what);
}
const __fsEncodings = new Set([
  "utf8", "utf-8", "utf16le", "utf-16le", "ucs2", "ucs-2", "ascii", "latin1",
  "binary", "base64", "base64url", "hex", "buffer",
]);
// node validateOffset（lib/internal/fs/streams.js）：start 经 validateInteger(v,
// name, 0)；end 同但 Infinity 显式放行（undefined 落 Infinity 由调用方处理）。
// 非 number（含 '4' 字符串形）ARG_TYPE；NaN/小数/负数/超 MAX_SAFE 即 OUT_OF_RANGE。
function __fsValidateOffset(v, name) {
  if (v === undefined) return;
  if (name === "end" && v === Infinity) return;
  __vIntRange(v, name, 0, 9007199254740991);
}
// node streams.js：start/end 双定且 end 非 Infinity 时 start > end 即 RangeError
//（文案逐字，read-stream.js:155 / inherit 同段点名 message 全文）。
function __fsValidateStartEnd(start, end) {
  if (start !== undefined && end !== undefined && end !== Infinity && start > end) {
    const e = new RangeError(`The value of "start" is out of range. It must be <= "end" (here: ${end}). Received ${start}`);
    e.code = "ERR_OUT_OF_RANGE"; throw e;
  }
}
function __fsEncoding(opts) {
  const check = (enc) => {
    // node assertEncoding 口径：非串（含数字）或未知名即 ARG_VALUE。
    if (typeof enc !== "string" || !__fsEncodings.has(enc.toLowerCase())) {
      // node validateEncoding：`ERR_INVALID_ARG_VALUE` + TypeError。
      const e = new TypeError(`The argument 'encoding' is invalid encoding. Received '${enc}'`);
      e.code = "ERR_INVALID_ARG_VALUE"; throw e;
    }
    return enc;
  };
  if (opts === undefined || opts === null) return null;
  if (typeof opts === "string") return check(opts);
  if (opts.encoding !== undefined && opts.encoding !== null) check(opts.encoding);
  return opts.encoding ?? null;
}
// node getOptions（streams 构造器口径）：null/undefined/function → 缺省{}；
// string → { encoding }；非对象（数字/布尔）→ ARG_TYPE 'options'。
function __fsStreamOpts(opts) {
  if (opts == null || typeof opts === "function") return {};
  if (typeof opts === "string") return { encoding: opts };
  if (typeof opts !== "object") __vErrType("options", "string or Object", opts);
  return opts;
}
function __fsMode(opts) {
  if (opts && typeof opts === "object" && opts.mode !== undefined) {
    const n = Number(opts.mode);
    if (!Number.isInteger(n) || n < 0) throw new TypeError("mode must be an integer");
    return n;
  }
  return 0;
}
function __fsDecode(bytes, encoding, what) {
  if (encoding === null) {
    // Node 语义：无编码读返回 Buffer（非裸 Uint8Array）——String(buf)/
    // toString() = utf8 内容、isBuffer = true。裸 Uint8Array 会 join 成
    // "byte,byte,…"（vite PostCSS 配置加载 JSON.parse(buf) 实测误判）。
    return Buffer.from(bytes);
  }
  // 10f crypto二轮：hex/base64 系经 Buffer（TextDecoder 无此表，读 hex 固件点名）。
  const enc = String(encoding).toLowerCase();
  if (enc === "hex" || enc === "base64" || enc === "base64url") {
    return Buffer.from(bytes).toString(enc);
  }
  // latin1/binary：字节直映码点（node 'latin1' = binary）。TextDecoder 的
  // latin1 标签是 windows-1252——0x80-0x9F 段与 latin1 不同，roundtrip
  // 套件 ✓/😀 文本现形。
  if (enc === "latin1" || enc === "binary") {
    return Buffer.from(bytes).toString("latin1");
  }
  return new TextDecoder(String(encoding)).decode(bytes);
}
// node 口径：Stats 可无 new 调用（DEP0180 弃用警告 + 位置参数形；
// stat 套件 `fs.Stats(dev, mode, ...)` 直调 + instanceof 断言）。
function __Stats(jOrDev, mode, nlink, uid, gid, rdev, blksize, ino, size, blocks, atime, mtime, ctime, birthtime) {
  if (!(this instanceof __Stats)) {
    process.emitWarning("fs.Stats constructor is deprecated.", "DeprecationWarning", "DEP0180");
    return new __Stats(jOrDev, mode, nlink, uid, gid, rdev, blksize, ino, size, blocks, atime, mtime, ctime, birthtime);
  }
  const isPos = typeof jOrDev === "number";
  if (isPos) {
    jOrDev = {
      dev: jOrDev, mode, nlink, uid, gid, rdev, blksize, ino, size, blocks,
      atimeMs: +atime, mtimeMs: +mtime, ctimeMs: +ctime, birthtimeMs: +birthtime,
    };
    // isX 由 mode 类型位推导（S_IFMT 族，constants 同值）。
    const t = mode & 61440;
    jOrDev.isFile = t === 32768;
    jOrDev.isDirectory = t === 16384;
    jOrDev.isSymlink = t === 40960;
    jOrDev.isFifo = t === 4096;
    jOrDev.isSocket = t === 49152;
    jOrDev.isBlock = t === 24576;
    jOrDev.isChar = t === 8192;
  }
  const bigint = !isPos && mode === true;
  {
    const j = jOrDev;
    // bigint 选项（stat-bigint 套件）：数值字段 BigInt 包装（同源 JSON，与
    // BigInt(numStats[key]) 逐键 strictEqual）；日期恒 Date（getTime 比对）。
    const B = bigint ? (v) => BigInt(Math.round(v)) : (v) => v;
    this.dev = B(j.dev ?? 0);
    this.ino = B(j.ino ?? 0);
    this.mode = B(j.mode);
    this.nlink = B(j.nlink ?? 1);
    this.uid = B(j.uid ?? 0);
    this.gid = B(j.gid ?? 0);
    this.rdev = B(j.rdev ?? 0);
    this.size = B(j.size);
    this.blksize = B(j.blksize ?? 4096);
    this.blocks = B(j.blocks ?? 0);
    this.atimeMs = B(j.atimeMs);
    this.mtimeMs = B(j.mtimeMs);
    this.ctimeMs = B(j.mtimeMs);
    this.birthtimeMs = B(j.birthtimeMs);
    if (bigint) {
      // Ns 形（stat-bigint 套件：Ms/Ns 双键，Ns = ms×1e6 取整）。
      this.atimeNs = BigInt(Math.round(j.atimeMs * 1e6));
      this.mtimeNs = BigInt(Math.round(j.mtimeMs * 1e6));
      this.ctimeNs = BigInt(Math.round(j.mtimeMs * 1e6));
      this.birthtimeNs = BigInt(Math.round(j.birthtimeMs * 1e6));
    }
    this.atime = new Date(j.atimeMs);
    this.mtime = new Date(j.mtimeMs);
    this.ctime = new Date(j.mtimeMs);
    this.birthtime = new Date(j.birthtimeMs);
    this.__f = j.isFile;
    this.__d = j.isDirectory;
    this.__l = j.isSymlink;
    this.__fifo = j.isFifo;
    this.__sock = j.isSocket;
    this.__blk = j.isBlock;
    this.__chr = j.isChar;
  }
}
__Stats.prototype.isFile = function () { return this.__f; };
__Stats.prototype.isDirectory = function () { return this.__d; };
__Stats.prototype.isSymbolicLink = function () { return this.__l; };
__Stats.prototype.isFIFO = function () { return !!this.__fifo; };
__Stats.prototype.isSocket = function () { return !!this.__sock; };
__Stats.prototype.isBlockDevice = function () { return !!this.__blk; };
__Stats.prototype.isCharacterDevice = function () { return !!this.__chr; };
class __Dirent {
  constructor(name, isDir, isFile, isLink, parentPath) {
    this.name = name;
    this.__d = isDir;
    this.__f = isFile;
    this.__l = isLink;
    // node getDirent：parentPath（26 线新名）+ path（旧别名，同值）。
    this.parentPath = parentPath;
    this.path = parentPath;
  }
  isFile() { return this.__f; }
  isDirectory() { return this.__d; }
  isSymbolicLink() { return this.__l; }
  isBlockDevice() { return false; }
  isCharacterDevice() { return false; }
  isFIFO() { return false; }
  isSocket() { return false; }
}
// StatsFs 纯数据面（type/bsize/blocks/bfree/bavail/files/ffree，无方法，Node 口径）。
class __StatsFs {
  constructor(j, bigint = false) {
    // bigint 选项：数值字段 BigInt 包装（statfs 套件 verifyStatFsObject 逐键 typeof）。
    const B = bigint ? (v) => BigInt(Math.round(v)) : (v) => v;
    this.type = B(j.type ?? 0);
    this.bsize = B(j.bsize ?? 4096);
    this.frsize = B(j.frsize ?? j.bsize ?? 4096);
    this.blocks = B(j.blocks ?? 0);
    this.bfree = B(j.bfree ?? 0);
    this.bavail = B(j.bavail ?? 0);
    this.files = B(j.files ?? 0);
    this.ffree = B(j.ffree ?? 0);
  }
}
// flags 字符串 → OpenFlags JSON（Node 口径子集；`s` 后缀忽略；数字只认本运行时
// constants 暴露的位值，O_CREAT/O_EXCL/O_TRUNC/O_APPEND 用 Linux 位值，记档）。
function __fsFlags(flag, what) {
  if (typeof flag === "number") {
    const rd = (flag & 3) !== 1;
    const wr = (flag & 3) !== 0;
    return JSON.stringify({
      read: rd, write: wr, append: (flag & 1024) !== 0,
      truncate: (flag & 512) !== 0, create: (flag & 64) !== 0, createNew: (flag & 128) !== 0,
    });
  }
  let f = String(flag ?? "r").replace(/s/g, "");
  const base = {
    r: { read: true },
    "r+": { read: true, write: true },
    w: { write: true, truncate: true, create: true },
    "w+": { read: true, write: true, truncate: true, create: true },
    a: { write: true, append: true, create: true },
    "a+": { read: true, write: true, append: true, create: true },
    wx: { write: true, truncate: true, createNew: true },
    "wx+": { read: true, write: true, truncate: true, createNew: true },
    ax: { write: true, append: true, createNew: true },
    "ax+": { read: true, write: true, append: true, createNew: true },
  };
  if (!base[f]) throw new TypeError(`${what}: invalid flag '${flag}'`);
  return JSON.stringify(base[f]);
}
// node readFile 大文件帽：>2^31-1 即 ERR_FS_FILE_TOO_LARGE（真机 26.8.2 实测
// RangeError，message 带 size：`File size (2147483648) is greater than 2 GiB`；
// readfile 套件造 2GiB 稀疏文件点名——无帽时 native 真 2GiB 分配 abort）。
const __kIoMaxLength = 2 ** 31 - 1;
function __fsFileTooLarge(size) {
  const e = new RangeError(`File size (${size}) is greater than 2 GiB`);
  e.code = "ERR_FS_FILE_TOO_LARGE";
  return e;
}
function __fsReadWhole(p, flag) {
  if (flag === undefined || flag === "r") {
    // 帽先于 native 读：stat 跟随符号链接，与 readFile 语义同；ENOENT 落到
    // native open 再报（不在此吞）。
    const st = statSync(p, { throwIfNoEntry: false });
    if (st && st.size > __kIoMaxLength) throw __fsFileTooLarge(st.size);
    return __wjs_fs_read_file(p);
  }
  const fd = Number(__wjs_fs_open(p, __fsFlags(flag, "readFile")));
  try {
    const parts = [];
    let total = 0;
    while (true) {
      const chunk = __wjs_fs_read_fd(fd, 1 << 20, -1);
      if (chunk.length === 0) break;
      total += chunk.length;
      if (total > __kIoMaxLength) throw __fsFileTooLarge(total);
      parts.push(chunk);
    }
    const out = new Uint8Array(parts.reduce((a, c) => a + c.length, 0));
    let off = 0;
    for (const c of parts) { out.set(c, off); off += c.length; }
    return out;
  } finally {
    __wjs_fs_close(fd);
  }
}
function __fsFdOf(p) {
  // node readFile/writeFile 系：fd number 或带 .fd 的句柄 → fd；否则 null。
  if (typeof p === "number" && Number.isInteger(p)) return __vFd(p);
  if (p && typeof p === "object" && typeof p.fd === "number") return p.fd;
  return null;
}
export function readFileSync(p, opts) {
  const enc = __fsEncoding(opts);
  // signal 面：已 abort 即 AbortError（node readFileSync 同步检查同口径）。
  const sig = __fsSignalCheck(opts);
  if (sig && sig.aborted) throw __fsAbortErr(sig.reason);
  const fd = __fsFdOf(p);
  if (fd !== null) {
    // fd 形：从当前位读到 EOF（node readFileHandle 同口径）。
    const parts = [];
    let total = 0;
    for (;;) {
      const chunk = __fsCall("read", "", () => __wjs_fs_read_fd(fd, 1 << 20, -1));
      if (chunk.length === 0) break;
      total += chunk.length;
      if (total > __kIoMaxLength) throw __fsFileTooLarge(total);
      parts.push(chunk);
    }
    const out = new Uint8Array(parts.reduce((a, c) => a + c.length, 0));
    let off = 0;
    for (const c of parts) { out.set(c, off); off += c.length; }
    return __fsDecode(out, enc, "readFile");
  }
  p = __fsPath(p, "readFile");
  const flag = opts && typeof opts === "object" ? opts.flag : undefined;
  return __fsCall("open", p, () => __fsDecode(__fsReadWhole(p, flag), enc, "readFile"));
}
// node 口径：options.flush 布尔（真机逐字 `The "options.flush" property
// must be of type boolean.`；write/append/stream 三面共用）。
function __fsFlushOpt(opts) {
  const v = opts ? opts.flush : undefined;
  if (v === undefined) return false;
  if (typeof v !== "boolean") throw new ERR_INVALID_ARG_TYPE("options.flush", "boolean", v);
  return v;
}
// flush:true 的 one-shot 写后 fsync（另开 'r' fd 落盘；fd 形直刷原 fd）。
function __fsFlushFile(p) {
  const fd = Number(__wjs_fs_open(p, __fsFlags("r", "writeFile")));
  try { fsyncSync(fd); }
  finally { try { __wjs_fs_close(fd); } catch {} }
}
export function writeFileSync(p, data, opts) {
  const enc = __fsEncoding(opts);
  // node 口径：signal 面校验（'hello' 即同步 ERR_INVALID_ARG_TYPE）+
  // signal.aborted 即 AbortError（走回调拒绝路径，writefile-with-fd 点名）。
  const wsig = __fsSignalCheck(opts);
  if (wsig && wsig.aborted) throw __fsAbortErr(wsig.reason);
  const needFlush = __fsFlushOpt(opts);
  const fd = __fsFdOf(p);
  if (fd !== null) {
    // fd 形：写现位（node writeFileHandle 同口径）。
    const bytes = __fsDataEnc(data, "writeFile", enc);
    writeSync(fd, bytes, 0, bytes.byteLength, null);
    if (needFlush) fsyncSync(fd);
    return;
  }
  p = __fsPath(p, "writeFile");
  const flag = opts && typeof opts === "object" ? opts.flag : undefined;
  const bytes = __fsDataEnc(data, "writeFile", enc);
  __fsCall("open", p, () => {
    if (flag === undefined || flag === "w") {
      __wjs_fs_write_file(p, bytes, __fsMode(opts));
      if (needFlush) __fsFlushFile(p);
      return;
    }
    const fd = Number(__wjs_fs_open(p, __fsFlags(flag, "writeFile")));
    try {
      __wjs_fs_write_fd(fd, bytes, flag.startsWith("a") ? -1 : 0);
      if (needFlush) fsyncSync(fd);
    }
    finally { __wjs_fs_close(fd); }
  });
  // mode 语义：仅新建文件时应用（存在性预判，记档近似）
  const mode = __fsMode(opts);
  if (mode > 0 && !existsSync(p)) chmodSync(p, mode);
}
// node writeFile/appendFile data 面（10f）：string/Buffer/TypedArray/DataView +
// 同步可迭代逐块收；chunk 非法即 'chunk' ARG_TYPE；顶层非法 data 为 'data' ARG_TYPE。
function __chunkArgErr(c) {
  const e = new TypeError(`The "chunk" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received ${c === null ? "null" : typeof c}`);
  e.code = "ERR_INVALID_ARG_TYPE"; throw e;
}
function __fsDataSync(data, what, opts) {
  const enc = typeof opts === "string" ? opts : (opts && opts.encoding);
  const isChunk = (c) => typeof c === "string" || ArrayBuffer.isView(c);
  const encChunk = (c) => {
    const u8 = typeof c === "string" && enc && enc !== "utf8" ? Buffer.from(c, enc) : __fsData(c, what);
    return new Uint8Array(u8.buffer, u8.byteOffset, u8.byteLength);
  };
  // node 口径：Array 实例整体拒收（真机 appendFileSync(file, []) →
  // ERR_INVALID_ARG_TYPE /data/；可迭代仅限非数组形态，append-file-sync 套件）。
  if (Array.isArray(data)) __vErrType(what, "string or an instance of Buffer, TypedArray, or DataView", data);
  if (data && typeof data === "object" && !ArrayBuffer.isView(data) && typeof data[Symbol.iterator] === "function") {
    const parts = [];
    for (const c of data) {
      if (!isChunk(c)) __chunkArgErr(c);
      parts.push(c);
    }
    const out = new Uint8Array(parts.reduce((a, c) => a + encChunk(c).length, 0));
    let off = 0;
    for (const c of parts) { const u8 = encChunk(c); out.set(u8, off); off += u8.length; }
    return out;
  }
  return __fsData(data, what);
}
// promises.appendFile 异步面：asyncIterable 全量收 + signal 两点 abort
//（收集前预检 + 收集后复查；doAppendStreamWithCancel 套件 AbortError 契约）。
async function __fsAppendFileAsync(p, data, opts) {
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
  if (signal) {
    // node 写为线程池异步——abort 竞速即拒；本仓写为同步，落宏任务（setTimeout 0）
    // 使 abort 的 nextTick 检查点先于写（FileHandle.write signal 路径同构）。
    await Promise.race([
      new Promise((resolve, reject) => {
        setTimeout(() => {
          try {
            if (signal.aborted) { reject(__fsAbortErr(signal.reason)); return; }
            resolve(appendFileSync(p, bytes, opts));
          } catch (e) { reject(e); }
        }, 0);
      }),
      new Promise((_, reject) =>
        signal.addEventListener("abort", () => reject(__fsAbortErr(signal.reason)), { once: true })),
    ]);
    return;
  }
  return appendFileSync(p, bytes, opts);
}
export function appendFileSync(p, data, opts) {
  __fsEncoding(opts);
  // node 口径：signal 面校验（非法 signal 同步 ARG_TYPE）+ aborted 即
  // AbortError（promises-appendfile cancel 套件）。
  const asig = __fsSignalCheck(opts);
  if (asig && asig.aborted) throw __fsAbortErr(asig.reason);
  const needFlush = __fsFlushOpt(opts);
  // node 口径：data 校验先于 open（非法 data 不得留下已创建的文件）；
  // 同步可迭代逐块收（promises-appendfile doAppendStream 族）。
  const bytes = __fsDataSync(data, "data", opts);
  if (typeof p === "number" && Number.isInteger(p)) {
    // fd 形：写现位（fd 'a+' 打开即尾）。
    __vFd(p);
    writeSync(p, bytes, 0, bytes.byteLength, null);
    if (needFlush) fsyncSync(p);
    return;
  }
  if (p && typeof p === "object" && typeof p.fd === "number") {
    writeSync(p.fd, bytes, 0, bytes.byteLength, null);
    if (needFlush) fsyncSync(p.fd);
    return;
  }
  p = __fsPath(p, "appendFile");
  __fsCall("open", p, () => __wjs_fs_append_file(p, bytes, __fsMode(opts)));
  if (needFlush) __fsFlushFile(p);
}
export function statSync(p) {
  p = __fsPath(p, "stat");
  // options：bigint（BigInt Stats）+ throwIfNoEntry:false（缺失回 undefined，node 口径）。
  const __o = arguments[1];
  if (__o && __o.throwIfNoEntry === false) {
    try {
      return new __Stats(JSON.parse(__wjs_fs_stat(p, true)), __o.bigint === true);
    } catch (e) {
      // 裸 native 错误无 code——先过 __fsErr 归一化（ENOENT 豁免，其余照抛）
      try { __fsErr(e, "stat", p); } catch (e2) { if (e2 && e2.code === "ENOENT") return undefined; throw e2; }
    }
  }
  return new __Stats(JSON.parse(__fsCall("stat", p, () => __wjs_fs_stat(p, true))), __o?.bigint === true);
}
// 文件系统级状态（M5 vitest 牵引；unix 经 statvfs，type 取 filesystem_id 记档）。
export function statfsSync(p) {
  if (typeof p !== "string" && !(p instanceof URL)) {
    const e = new TypeError(`The "path" argument must be of type string or an instance of Buffer or URL. Received ${p === null ? "null" : typeof p}`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  p = __fsPath(p, "statfs");
  const __o = arguments[1];
  return new __StatsFs(JSON.parse(__fsCall("statfs", p, () => __wjs_fs_statfs(p))), __o?.bigint === true);
}
export function lstatSync(p) {
  p = __fsPath(p, "lstat");
  const __o = arguments[1];
  if (__o && __o.throwIfNoEntry === false) {
    try {
      return new __Stats(JSON.parse(__wjs_fs_stat(p, false)), __o.bigint === true);
    } catch (e) {
      try { __fsErr(e, "lstat", p); } catch (e2) { if (e2 && e2.code === "ENOENT") return undefined; throw e2; }
    }
  }
  return new __Stats(JSON.parse(__fsCall("stat", p, () => __wjs_fs_stat(p, false))), __o?.bigint === true);
}
export function existsSync(p) {
  try {
    return __wjs_fs_exists(__fsPath(p, "exists"));
  } catch {
    return false;
  }
}
function __firstMissing(p) {
  // node 递归 mkdir 返回首个**新建**路径 = 最浅的不存在祖先。
  const abs = p.startsWith("/");
  const segs = p.split("/").filter((x) => x !== "");
  let acc = abs ? "/" : "";
  for (const seg of segs) {
    acc = acc === "/" ? "/" + seg : acc + "/" + seg;
    if (!existsSync(acc)) return acc;
  }
  return null;
}
export function mkdirSync(p, opts) {
  p = __fsPath(p, "mkdir");
  // node 口径：opts 可为 number/string（mode 简写，mkdir-mode-mask 套件
  // 0o10644/"10644" 点名）或 object{recursive,mode}。
  let recursive = false;
  let mode;
  if (typeof opts === "number" || typeof opts === "string") {
    mode = __fsModeNum(opts);
  } else if (opts && typeof opts === "object") {
    recursive = opts.recursive !== undefined
      ? __vBooleanProp(opts.recursive, "options.recursive")
      : false;
    if (opts.mode !== undefined) mode = __fsModeNum(opts.mode);
  }
  // node 口径：recursive 只容忍已存在的**目录**；路径是文件即 EEXIST
  //（syscall 'mkdir'，test-fs-mkdir 点名）。
  if (recursive && existsSync(p) && !statSync(p).isDirectory()) {
    const e = new Error(`EEXIST: file already exists, mkdir '${p}'`);
    e.code = "EEXIST"; e.errno = -17; e.syscall = "mkdir"; e.path = p;
    throw e;
  }
  const firstCreated = recursive ? __firstMissing(p) : null;
  __fsCall("mkdir", p, () => __wjs_fs_mkdir(p, recursive, mode ?? 0o777));
  return firstCreated ?? undefined;
}
export function rmSync(p, opts) {
  p = __fsPath(p, "rm");
  const recursive = !!(opts && (opts.recursive ?? false));
  const force = !!(opts && (opts.force ?? false));
  __fsCall("rm", p, () => __wjs_fs_rm(p, recursive, force));
}
export function rmdirSync(p, opts) {
  p = __fsPath(p, "rmdir");
  const recursive = !!(opts && (opts.recursive ?? false));
  __fsCall("rmdir", p, () => __wjs_fs_rmdir(p, recursive));
}
export function unlinkSync(p) {
  p = __fsPath(p, "unlink");
  __fsCall("unlink", p, () => __wjs_fs_unlink(p));
}
export function readdirSync(p, opts) {
  p = __fsPath(p, "readdir");
  const withTypes = !!(opts && (opts.withFileTypes ?? false));
  const enc = __fsEncoding(opts);
  const asBuf = enc === "buffer";
  if (opts && opts.recursive) {
    // 递归读（async-filter 验证面）：串形相对路径（`/` 分隔）；Dirent 形
    // parentPath 逐级（真机逐项）；仅 lstat 目录下钻（链接目录不跟随）。
    const out = [];
    const walk = (rel) => {
      const dir = rel === "" ? p : p + "/" + rel;
      const ents = readdirSync(dir, { ...opts, recursive: false });
      for (const e of ents) {
        const nm = String(withTypes ? e.name : e);
        const r = rel === "" ? nm : rel + "/" + nm;
        const isDir = withTypes ? e.isDirectory()
          : statSync(dir + "/" + nm).isDirectory();
        if (withTypes && rel !== "") {
          // 嵌套项 parentPath 逐级（顶层沿用底层值）。
          const base = p + "/" + rel;
          out.push(new __Dirent(asBuf ? Buffer.from(nm) : nm,
            isDir, e.isFile(), e.isSymbolicLink(),
            asBuf ? Buffer.from(base) : base));
        } else if (withTypes) {
          out.push(e);
        } else {
          out.push(asBuf ? Buffer.from(r) : r);
        }
        if (isDir) walk(r);
      }
    };
    walk("");
    return out;
  }
  const out = JSON.parse(__fsCall("scandir", p, () => __wjs_fs_readdir(p, withTypes)));
  if (!withTypes) return asBuf ? out.map((n) => Buffer.from(n)) : out;
  // node getDirent：dirent.parentPath = 目录路径（Dirent 亦挂 path 别名）。
  return out.map(([name, isDir, isFile, isLink]) => new __Dirent(asBuf ? Buffer.from(name) : name, isDir, isFile, isLink, p));
}
export function renameSync(a, b) {
  // node 口径：位置参数名 oldPath/newPath（rename-type-check 套件 message 逐字）。
  a = __fsPath(a, "rename", "oldPath");
  b = __fsPath(b, "rename", "newPath");
  __fsCall("rename", a, () => __wjs_fs_rename(a, b));
}
export function copyFileSync(src, dst, mode) {
  if (mode !== undefined && typeof mode !== "number") {
    const e = new TypeError(`The "mode" argument must be of type number. Received ${typeof mode} (${JSON.stringify(String(mode))})`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  src = __fsPath(src, "copyFile");
  dst = __fsPath(dst, "copyFile");
  __fsCall("copyfile", src, () => __wjs_fs_copy_file(src, dst));
}
export function realpathSync(p, opts) {
  __fsEncoding(opts);
  p = __fsPath(p, "realpath");
  return __fsCall("lstat", p, () => __wjs_fs_realpath(p));
}
// Node 口径：.native = binding 级 realpath（无 JS 层缓存/规范化）。本仓两者
// 同底座（std canonicalize），直接自引用（vite 8 的 safeRealpathSync 取此面）。
realpathSync.native = realpathSync;
export function mkdtempSync(prefix, opts) {
  // node：prefix 经 getValidatedPath(prefix, 'prefix')（undefined/null/数字/布尔
  // 全 ARG_TYPE；mkdtemp-prefix-check 套件）。
  prefix = __fsPath(prefix, "prefix");
  __fsEncoding(opts);
  return __fsCall("mkdir", String(prefix), () => __wjs_fs_mkdtemp(String(prefix)));
}
export const constants = {
  F_OK: 0, R_OK: 4, W_OK: 2, X_OK: 1,
  O_RDONLY: 0, O_WRONLY: 1, O_RDWR: 2, O_CREAT: 512, O_EXCL: 2048, O_NOCTTY: 131072,
  O_TRUNC: 1024, O_APPEND: 8, O_DIRECTORY: 1048576, O_NOFOLLOW: 256, O_SYNC: 128,
  O_DSYNC: 4194304, O_SYMLINK: 2097152, O_NONBLOCK: 4,
  S_IFMT: 61440, S_IFREG: 32768, S_IFDIR: 16384, S_IFCHR: 8192, S_IFBLK: 24576,
  S_IFIFO: 4096, S_IFLNK: 40960, S_IFSOCK: 49152,
  S_IRWXU: 448, S_IRUSR: 256, S_IWUSR: 128, S_IXUSR: 64,
  S_IRWXG: 56, S_IRGRP: 32, S_IWGRP: 16, S_IXGRP: 8,
  S_IRWXO: 7, S_IROTH: 4, S_IWOTH: 2, S_IXOTH: 1,
  COPYFILE_EXCL: 1, COPYFILE_FICLONE: 2, COPYFILE_FICLONE_FORCE: 4,
  UV_FS_COPYFILE_EXCL: 1, UV_FS_COPYFILE_FICLONE: 2, UV_FS_COPYFILE_FICLONE_FORCE: 4,
  UV_DIRENT_UNKNOWN: 0, UV_DIRENT_FILE: 1, UV_DIRENT_DIR: 2, UV_DIRENT_LINK: 3,
  UV_DIRENT_FIFO: 4, UV_DIRENT_SOCKET: 5, UV_DIRENT_CHAR: 6, UV_DIRENT_BLOCK: 7,
  // 平台相关 uv 开文件旗（真机 26.8.2 macOS 全 0，逐项导出对拍）
  UV_FS_O_FILEMAP: 0, UV_FS_O_RANDOM: 0, UV_FS_O_SEQUENTIAL: 0,
  UV_FS_O_SHORT_LIVED: 0, UV_FS_O_TEMPORARY: 0,
  UV_FS_SYMLINK_DIR: 1, UV_FS_SYMLINK_JUNCTION: 2,
};
// node 口径：constants 无原型（stat-constants 套件 getPrototypeOf === null）。
Object.setPrototypeOf(constants, null);
// node 口径（lib/internal/validators.js validateIgnoreOption 逐字）：
// null/undefined 过；数组逐元校验（名 `options.ignore[i]`）；单元素 string
// 非空（空串 ARG_VALUE 'must be a non-empty string'）/RegExp/Function 过，
// 余下 ARG_TYPE ['string','RegExp','Function']。
function __validateIgnoreOption(v, name) {
  if (v === undefined || v === null) return;
  if (Array.isArray(v)) {
    for (let i = 0; i < v.length; i++) __validateIgnoreElement(v[i], `${name}[${i}]`);
    return;
  }
  __validateIgnoreElement(v, name);
}
function __validateIgnoreElement(m, name) {
  if (typeof m === "string") {
    if (m.length === 0) {
      const e = new TypeError(`The argument '${name}' must be a non-empty string. Received ''`);
      e.code = "ERR_INVALID_ARG_VALUE"; throw e;
    }
    return;
  }
  if (Object.prototype.toString.call(m) === "[object RegExp]") return;
  if (typeof m === "function") return;
  const e = new TypeError(`The "${name}" argument must be one of type string, RegExp, or Function. Received ${Object.prototype.toString.call(m)}`);
  e.code = "ERR_INVALID_ARG_TYPE"; throw e;
}
// node 口径（lib/internal/fs/watchers.js createIgnoreMatcher）：string 经
// minimatch（matchBase + win/mac nocase，本仓 `__wjs_glob_match` 近似）/
// RegExp 经 exec 判空/Function 直透；filename 为 null 时不滤（调用方守卫）。
const __globNocase = process.platform === "win32" || process.platform === "darwin";
function __ignoreMatcher(ignore) {
  if (ignore === undefined || ignore === null) return null;
  const list = Array.isArray(ignore) ? ignore : [ignore];
  const compiled = list.map((m) => {
    if (typeof m === "string") {
      return (fn) => __wjs_glob_match(m, fn, fn.split("/").pop(), __globNocase);
    }
    if (Object.prototype.toString.call(m) === "[object RegExp]") {
      return (fn) => m.exec(fn) !== null;
    }
    return m;
  });
  return (fn) => {
    for (const c of compiled) {
      if (c(fn)) return true;
    }
    return false;
  };
}
// 存活 watch 句柄集（`process._getActiveHandles` 桥；close/stop 即摘）。
globalThis.__wjsFsHandles ??= new Set();
// node 口径：filename 按 options.encoding 转码（hex/buffer/base64…；
// null 直通——部分后端 filename 为 null，encoding 套件接受 null）。
function __encodeWatchFilename(fn, encoding) {
  if (fn === null || fn === undefined) return fn;
  if (encoding === "buffer") return Buffer.from(fn, "utf8");
  const enc = String(encoding ?? "utf8").toLowerCase();
  if (enc === "utf8" || enc === "utf-8") return fn;
  return Buffer.from(fn, "utf8").toString(enc);
}
// FSWatcher（10f，node 口径）：EventEmitter 形（'change'/'close' 事件面 +
// on/once/off），options.listener 可选、{ signal } abort 即 close。
class __FSWatcher extends EventEmitter {
  #id;
  constructor() { super(); this.#id = 0; }
  __attach(id) { this.#id = id; return this; }
  close() {
    // node 口径（lib/internal/fs/watchers.js FSWatcher.close）：已关即 noop；
    // 'close' 经 nextTick 异步发（handler 内自调 close 安全）。
    if (this.#id !== 0) {
      __wjs_watch_close(this.#id); this.#id = 0;
      globalThis.__wjsFsHandles.delete(this);
      process.nextTick(() => this.emit("close"));
    }
  }
  // node 口径：ref/unref 取/释底层句柄引用（watch-ref-unref 套件：unref 后
  // 进程可退；Rust 侧 watch_open 计数联动，幂等）。
  ref() { if (this.#id !== 0) __wjs_watch_persistent(this.#id, true); return this; }
  unref() { if (this.#id !== 0) __wjs_watch_persistent(this.#id, false); return this; }
  get closed() { return this.#id === 0; }
}
export function watch(p, opts, listener) {
  if (typeof opts === "function") { listener = opts; opts = {}; }
  if (listener !== undefined && typeof listener !== "function") throw new TypeError("watch: listener must be a function");
  if (opts !== undefined && opts !== null && typeof opts !== "function") __fsEncoding(opts);
  p = __fsPath(p, "watch");
  const recursive = !!(opts && opts.recursive);
  const persistent = !(opts && opts.persistent === false);
  // encoding 校验（非法即 ARG_VALUE）+ filename 转码位（encoding 套件）。
  const watchEncoding = __fsEncoding(opts) ?? "utf8";
  const watcher = new __FSWatcher();
  if (typeof listener === "function") watcher.on("change", listener);
  // node 26 口径（lib/internal/validators.js validateIgnoreOption +
  // lib/internal/fs/watchers.js createIgnoreMatcher）：string（含 glob，
  // matchBase）/RegExp/Function/数组混排；非法即 ARG_TYPE（空串 ARG_VALUE）。
  const ignoreOpt = opts ? opts.ignore : undefined;
  __validateIgnoreOption(ignoreOpt, "options.ignore");
  const ignoreFn = __ignoreMatcher(ignoreOpt);
  const id = __fsCall("watch", p, () => __wjs_watch_start(p, recursive, persistent, (ev, fn) => {
    if (fn != null && ignoreFn && ignoreFn(fn)) return;
    watcher.emit("change", ev, __encodeWatchFilename(fn, watchEncoding));
  }));
  watcher.__attach(id);
  globalThis.__wjsFsHandles.add(watcher);
  if (opts && opts.signal) {
    if (opts.signal.aborted) watcher.close();
    else opts.signal.addEventListener("abort", () => watcher.close(), { once: true });
  }
  return watcher;
}
// stat 轮询（watchFile 底座；interval 経 setInterval，statSync 取样）。
// node 口径（lib/internal/fs/watchers.js StatWatcher）：EventEmitter 形
// （'change'/'stop' + listenerCount）；stop() 经 nextTick 发 'stop'（已停即
// noop，不重发）；ref/unref 链式（定时器 keep-alive 由 Rust 表决定，记档）；
// 缺席侧零 Stats 派发（watchfile 套件：缺席首轮 (zero,zero)，出现轮 prev.ino<=0）。
const __statWatchers = new Map();
function __zeroStats() {
  return new __Stats(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
}
function __statPoll(p) {
  const rec = __statWatchers.get(p);
  if (!rec) return;
  let curr = null;
  try { curr = statSync(p); } catch { curr = null; }
  const prev = rec.prev;
  rec.prev = curr;
  const first = !rec.seen;
  rec.seen = true;
  // 缺席文件首轮即发 (zero,zero)（真机实测）；首轮即有真变迁走正常路径
  //（不得吞——unlink-then-poll 形首轮 (null,real) 即发）。
  if (first && curr === null && prev === null) {
    const z = __zeroStats();
    rec.watcher.emit("change", z, __zeroStats());
    return;
  }
  if (curr === null && prev === null) return;
  if (curr !== null && prev !== null && curr.size === prev.size && curr.mtimeMs === prev.mtimeMs) return;
  const c = curr ?? __zeroStats();
  const v = prev ?? __zeroStats();
  rec.watcher.emit("change", c, v);
}
class __StatWatcher extends EventEmitter {
  #path;
  #stopped;
  constructor(p) { super(); this.#path = p; this.#stopped = false; }
  stop() {
    // 已停即 noop（不重发 stop，watchfile 套件点名）；同路径单例：停即全停
    //（真机 w2.stop 关共享句柄口径），摘表停 timer + nextTick 发 stop。
    if (this.#stopped) return this;
    this.#stopped = true;
    globalThis.__wjsFsHandles.delete(this);
    const rec = __statWatchers.get(this.#path);
    if (rec && rec.watcher === this) {
      clearInterval(rec.timer);
      __statWatchers.delete(this.#path);
    }
    process.nextTick(() => this.emit("stop"));
    return this;
  }
  close() { return this.stop(); }
  // node 口径：ref/unref 取/释轮询 timer 引用（watchfile-ref-unref 套件：
  // 全 unref 后进程可退；单例共享 timer，直通即可）。
  ref() {
    const rec = __statWatchers.get(this.#path);
    if (rec && rec.watcher === this && rec.timer && typeof rec.timer.ref === "function") rec.timer.ref();
    return this;
  }
  unref() {
    const rec = __statWatchers.get(this.#path);
    if (rec && rec.watcher === this && rec.timer && typeof rec.timer.unref === "function") rec.timer.unref();
    return this;
  }
}
export function watchFile(p, opts, listener) {
  if (typeof opts === "function") { listener = opts; opts = {}; }
  if (typeof listener !== "function") {
    const e = new TypeError(`The "listener" argument must be of type function. Received ${listener}`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  p = __fsPath(p, "watchFile");
  const interval = (opts && Number(opts.interval) > 0) ? Number(opts.interval) : 5007;
  // node 口径：同路径单例 watcher（多次 watchFile 同一对象，监听累积；
  // watchfile-ref-unref 套件 listenerCount 点名）。
  let rec = __statWatchers.get(p);
  if (!rec) {
    rec = { watcher: new __StatWatcher(p), prev: null, seen: false, timer: null };
    try { rec.prev = statSync(p); } catch { rec.prev = null; }
    rec.timer = setInterval(() => __statPoll(p), interval);
    __statWatchers.set(p, rec);
  }
  rec.watcher.on("change", listener);
  globalThis.__wjsFsHandles.add(rec.watcher);
  return rec.watcher;
}
export function unwatchFile(p, listener) {
  p = __fsPath(p, "unwatchFile");
  const rec = __statWatchers.get(p);
  if (!rec) return;
  // node 口径：摘指定监听（缺省全摘）；归零即 stop（恰发一次 stop）。
  if (typeof listener === "function") rec.watcher.removeListener("change", listener);
  else rec.watcher.removeAllListeners("change");
  if (rec.watcher.listenerCount("change") === 0) rec.watcher.stop();
}
