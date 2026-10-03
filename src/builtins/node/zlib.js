import errors from 'node:internal/errors';
import { kMaxLength as __bufKMaxLength } from 'node:buffer';
import zipEntryMod from 'node:internal/zip/entry';
import zipArchiveMod from 'node:internal/zip/archive';
import zipBufferMod from 'node:internal/zip/buffer';
import zipFileMod from 'node:internal/zip/file';
import zipContentSizeMod from 'node:internal/zip/content-size';

const {
  codes: {
    ERR_INVALID_ARG_TYPE: { HideStackFramesError: ERR_INVALID_ARG_TYPE },
    ERR_BROTLI_INVALID_PARAM: { HideStackFramesError: ERR_BROTLI_INVALID_PARAM },
    ERR_ZLIB_INITIALIZATION_FAILED: { HideStackFramesError: ERR_ZLIB_INITIALIZATION_FAILED },
    ERR_BUFFER_TOO_LARGE: { HideStackFramesError: ERR_BUFFER_TOO_LARGE },
  },
} = errors;

function __zBytes(input, what) {
  if (typeof input === "string") return new TextEncoder().encode(input);
  if (input instanceof Uint8Array) return input;
  if (input instanceof ArrayBuffer) return new Uint8Array(input);
  if (ArrayBuffer.isView(input)) return new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
  throw new ERR_INVALID_ARG_TYPE(
    "buffer", ["string", "Buffer", "TypedArray", "DataView", "ArrayBuffer"], input);
}
// 字典严格校验（真机：只收 Buffer/TypedArray/DataView/ArrayBuffer——string 是
// 合法数据输入但非法字典，dictionary 套件 createBrotli*({dictionary:'string'})
// 口径）。一次性面与流类基座共用。
function __zDictBytes(opts) {
  const d = opts?.dictionary;
  if (d === undefined || d === null) return null;
  if (typeof d === "string" || !(d instanceof Uint8Array || d instanceof ArrayBuffer || ArrayBuffer.isView(d))) {
    throw new ERR_INVALID_ARG_TYPE("options.dictionary", ["Buffer", "TypedArray", "DataView", "ArrayBuffer"], d);
  }
  return __zBytes(d);
}
// pledgedSrcSize 校验（pledged 套件真机口径：'1'/null → ARG_TYPE；
// NaN/±Infinity/非整数/负/MAX_SAFE+1 → OUT_OF_RANGE）。返回 native 档
// （无选项 -1）；校验仅在 zstd 压缩侧调用点生效。
function __zCheckPledged(opts) {
  const p = opts?.pledgedSrcSize;
  if (p === undefined) return -1;
  if (typeof p !== "number") {
    throw new ERR_INVALID_ARG_TYPE("options.pledgedSrcSize", "number", p);
  }
  if (!Number.isSafeInteger(p) || p < 0) {
    const err = new RangeError(`The value of "options.pledgedSrcSize" is out of range. It must be a non-negative integer. Received ${p}`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  return p;
}
// spoofed length 校验（Node invalid-input 口径：length/byteLength getter 伪造的视图
// 实际缓冲不足即 ERR_OUT_OF_RANGE；真机读 length 分配，短读即范围错）。
function __zChecked(input) {
  const u8 = __zBytes(input);
  if (u8.length > u8.buffer.byteLength) {
    const err = new RangeError(`The value of "buffer.length" is out of range.`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  return u8;
}
function __zBuf(u8) {
  const b = Buffer.from(u8.buffer, u8.byteOffset, u8.byteLength);
  return b;
}
const __Z_ERRNO = {
  Z_OK: 0, Z_STREAM_END: 1, Z_NEED_DICT: 2, Z_ERRNO: -1, Z_STREAM_ERROR: -2,
  Z_DATA_ERROR: -3, Z_MEM_ERROR: -4, Z_BUF_ERROR: -5, Z_VERSION_ERROR: -6,
};
function __zErr(e) {
  // 参数校验错（ERR_*）直通，不套 zlib 形状
  if (e && typeof e.code === "string" && e.code.startsWith("ERR_")) throw e;
  const m = String((e && e.message) || e);
  const code = (m.match(/^([A-Z_]+): /) || [])[1] || "Z_DATA_ERROR";
  const rest = m.replace(/^[A-Z_]+: /, "");
  // message 裸形（真机 26.8.2 实测：err.message="unexpected end of file"、
  // err.code="Z_BUF_ERROR" 分离；assert.throws 正则对 String(err) 匹配，
  // trailing-garbage 套件 /^Error: unknown compression method$/ 才能过）
  const err = new Error(rest.trim() || code);
  err.code = code;
  err.errno = __Z_ERRNO[code] ?? -1;
  throw err;
}
// 引擎 feed 错误（JSON code/msg → JS 错误）：junk 是 TypeError（真机 name 口径），
// Z_* 是 Error；message 裸形同上。
function __zThrowEngine(code, msg) {
  const err = code === "ERR_TRAILING_JUNK_AFTER_STREAM_END"
    ? new TypeError(msg)
    : new Error(msg);
  err.code = code;
  // errno：Z_* 走 errno 表；ZSTD_error_* 走 constants（pledged 套件
  // err.errno === constants.ZSTD_error_srcSize_wrong 口径）。运行期取表。
  err.errno = __Z_ERRNO[code] ?? constants[code] ?? -1;
  throw err;
}
// 流类 native kind（__wjs2_zlib_stream_new 的 ZKind 同值）。
// 默认终结档（_flush）：zlib 族 Z_FINISH=4；brotli FINISH=2 / zstd end=2。
// flush() 无 kind 默认档：zlib 族 Z_FULL_FLUSH=3；brotli FLUSH=1 / zstd flush=1。
function __zKindDefaultFinish(kind) { return kind <= 7 ? 4 : 2; }
function __zKindDefaultFlush(kind) { return kind <= 7 ? 3 : 1; }
// rejectGarbageAfterEnd：boolean 校验（真机 ERR_INVALID_ARG_TYPE 逐字实测），
// 返回生效值。sync 便捷函数与流构造器共用。
function __zCheckRejectOpt(opts) {
  if (opts === undefined || opts === null) return false;
  const r = opts.rejectGarbageAfterEnd;
  if (r !== undefined && typeof r !== "boolean") {
    throw new ERR_INVALID_ARG_TYPE("options.rejectGarbageAfterEnd", "boolean", r);
  }
  return r === true;
}
// 一次性解压走流式引擎（与流面同状态机；真机错误口径：截断 Z_BUF_ERROR
// "unexpected end of file"、gzip 假头 "unknown compression method"）。
// dfltFinish：zlib 族 4（Z_FINISH）、zstd 2（end）；opts.finishFlush 覆盖
// （truncated 套件 zstd 段用 ZSTD_e_flush=1 触发截断报错）。
function __zEngineOnce(kind, data, opts, dfltFinish, lv) {
  const reject = __zCheckRejectOpt(opts);
  // 字典严格校验（真机：string 不收，dictionary 套件 createBrotli* 口径）；
  // pledgedSrcSize 仅 zstd 压缩侧（10）生效（Node 解压类校验器不含此键，忽略）。
  const dict = __zDictBytes(opts);
  const pledged = kind === 10 ? __zCheckPledged(opts) : -1;
  const id = __wjs2_zlib_stream_new(kind, lv ?? -1, dict, pledged, reject ? 1 : 0);
  try {
    const flag = (opts && opts.finishFlush !== undefined) ? opts.finishFlush : dfltFinish;
    const r = JSON.parse(__wjs2_zlib_stream_feed(id, data, flag));
    const out = __wjs2_zlib_stream_out(id);
    __wjs2_zlib_stream_free(id);
    if (r.code !== undefined) __zThrowEngine(r.code, r.msg);
    return out;
  } catch (e) {
    __wjs2_zlib_stream_free(id);
    throw e;
  }
}
// kMaxLength 守卫（Node kmaxlength 口径：解压输出超 `Buffer.kMaxLength` 即
// RangeError；套件劫持 kMaxLength=64 触发，不分配大 Buffer）。
// 注意：快照 `require('buffer')` 的 kMaxLength（Node lib/zlib.js 解构值拷贝——
// 劫持窗口内 require 即锁定 64，事后恢复不影响；live 读则恢复后失效）。
const __zKMaxSnap = (typeof __bufKMaxLength === "number" && __bufKMaxLength) || 2147483647;
function __zCheckKMax(out) {
  const max = __zKMaxSnap;
  if (out.length > max) {
    const err = new RangeError(`Cannot create a Buffer larger than ${max} bytes`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  return out;
}
// （__zCall 已随 brotli/zstd 一次性压缩改走引擎而移除——裸 native 路径删除）
function __zLevel(opts, dflt) {
  if (opts === undefined || opts === null) return dflt;
  if (typeof opts === "number") opts = { level: opts };
  const lv = opts.level ?? dflt;
  if (!Number.isInteger(lv) || lv < -1 || lv > 9) {
    const err = new RangeError(`options.level ${lv} out of range (-1..9)`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  return lv;
}
// Node 原文（lib/zlib.js Brotli 构造器）：params 键越界/重复即 ERR_BROTLI_INVALID_PARAM，
// 值非 number/boolean 即 ERR_INVALID_ARG_TYPE；bool 标志位非 0/1 即 INIT_FAILED。
// 构造期与 Sync/Async 共用（构造期先验，Sync 复验无害）。
function __zCheckBrotliParams(opts) {
  if (opts === undefined || opts === null) return;
  if (opts.params === undefined || opts.params === null) return;
  if (typeof opts.params !== "object") {
    throw new ERR_INVALID_ARG_TYPE("options.params", "object", opts.params);
  }
  const seen = new Set();
  for (const origKey of Object.keys(opts.params)) {
    const key = Number(origKey);
    if (!Number.isInteger(key) || key < 0 || key > 6 || seen.has(key)) {
      throw new ERR_BROTLI_INVALID_PARAM(origKey);
    }
    seen.add(key);
    const v = opts.params[origKey];
    if (typeof v !== "number" && typeof v !== "boolean") {
      throw new ERR_INVALID_ARG_TYPE("options.params[key]", "number", v);
    }
  }
  if (opts.params[4] !== undefined && opts.params[4] !== 0 && opts.params[4] !== 1 &&
      opts.params[4] !== false && opts.params[4] !== true) {
    throw new ERR_ZLIB_INITIALIZATION_FAILED();
  }
}
function __zQuality(opts) {
  if (opts === undefined || opts === null) return 11;
  __zCheckBrotliParams(opts);
  let q = opts.quality;
  if (q === undefined && opts.params !== undefined && opts.params !== null) {
    q = opts.params[1];
  }
  if (q === undefined) return 11;
  if (typeof q === "boolean") q = q ? 1 : 0;
  if (!Number.isInteger(q) || q < 0 || q > 11) {
    const err = new RangeError(`options.quality ${q} out of range (0..11)`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  return q;
}
// flush 系范围校验（flush/finishFlush/fullFlush；zlib 系 0..5，brotli 系 0..3）。
function __zFlush(opts, brotli) {
  if (opts === undefined || opts === null) return undefined;
  const lo = 0, hi = brotli ? 3 : 5;
  for (const k of ["flush", "finishFlush", "fullFlush"]) {
    const f = opts[k];
    if (f === undefined) continue;
    if (!Number.isInteger(f) || f < lo || f > hi) {
      const err = new RangeError(`The value of "options.${k}" is out of range. It must be >= ${lo} and <= ${hi}. Received ${f}`);
      err.code = "ERR_OUT_OF_RANGE";
      throw err;
    }
  }
  return opts.flush;
}
// maxOutputLength 校验（1..kMaxLength；Node checkRangesOrGetDefault 口径）。
function __zMaxOut(opts) {
  if (opts === undefined || opts === null) return undefined;
  const m = opts.maxOutputLength;
  if (m === undefined) return undefined;
  if (!Number.isInteger(m) || m < 1 || m > 2147483647) {
    const err = new RangeError(`The value of "options.maxOutputLength" is out of range.`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  return m;
}
function __zAsync(core, args, cb) {
  queueMicrotask(() => {
    try {
      cb(null, core(...args));
    } catch (e) {
      cb(e);
    }
  });
}
function __zNeedCb(cb, what) {
  if (typeof cb !== "function") throw new ERR_INVALID_ARG_TYPE("callback", "function", cb);
}
function deflateSync__core(buf, opts) {
  const data = __zChecked(buf, "deflate");
  __zCheckZlibOpts(opts, 8, false);
  const lv = __zLevel(opts, -1);
  __zFlush(opts, false);
  return __zBuf(__zEngineOnce(0, data, opts, 4, lv));
}
export function deflate(buf, opts, cb) {
  if (opts && opts.info && typeof cb === "function") { const C = Deflate; const eng = new C(opts); try { const r = deflateSync(buf, opts); cb(null, { buffer: r.buffer ?? r, engine: eng }); } catch (e) { cb(e); } return; }
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  __zNeedCb(cb, "deflate");
  const data = __zChecked(buf, "deflate");
  const lv = __zLevel(opts, -1);
  __zAsync((d) => deflateSync__core(d, opts), [data], cb);
}
function inflateSync__core(buf, opts) {
  const data = __zChecked(buf, "inflate");
  __zCheckZlibOpts(opts, 8, true);
  return __zCheckKMax(__zBuf(__zEngineOnce(3, data, opts, 4)));
}
// 10a：crc32（同步纯函数；真机逐项对过：空串 0、链式 seed、双报错）。
export function crc32(data, value = 0) {
  let bytes;
  if (typeof data === "string") {
    bytes = new TextEncoder().encode(data);
  } else if (data instanceof Uint8Array) {
    bytes = data;
  } else if (data instanceof ArrayBuffer) {
    bytes = new Uint8Array(data);
  } else if (ArrayBuffer.isView(data)) {
    bytes = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  } else {
    throw new ERR_INVALID_ARG_TYPE(
      "data", ["string", "Buffer", "TypedArray", "DataView"], data);
  }
  if (typeof value !== "number") {
    throw new ERR_INVALID_ARG_TYPE("value", "number", value);
  }
  return __wjs2_zlib_crc32(bytes, value >>> 0);
}
export function inflate(buf, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  if (opts && opts.info) { const eng = new Inflate(opts); try { const r = inflateSync(buf, opts); cb(null, { buffer: r.buffer ?? r, engine: eng }); } catch (e) { cb(e); } return; }
  __zNeedCb(cb, "inflate");
  const data = __zChecked(buf, "inflate");
  __zAsync((d) => inflateSync__core(d, opts), [data], cb);
}
function deflateRawSync__core(buf, opts) {
  const data = __zChecked(buf, "deflateRaw");
  __zCheckZlibOpts(opts, 8, false);
  const lv = __zLevel(opts, -1);
  __zFlush(opts, false);
  return __zBuf(__zEngineOnce(1, data, opts, 4, lv));
}
export function deflateRaw(buf, opts, cb) {
  if (opts && opts.info && typeof cb === "function") { const C = DeflateRaw; const eng = new C(opts); try { const r = deflateRawSync(buf, opts); cb(null, { buffer: r.buffer ?? r, engine: eng }); } catch (e) { cb(e); } return; }
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  __zNeedCb(cb, "deflateRaw");
  const data = __zChecked(buf, "deflateRaw");
  const lv = __zLevel(opts, -1);
  __zAsync((d) => deflateRawSync__core(d, opts), [data], cb);
}
function inflateRawSync__core(buf, opts) {
  const data = __zChecked(buf, "inflateRaw");
  __zCheckZlibOpts(opts, 8, true);
  const maxOut = __zMaxOut(opts);
  const out = __zBuf(__zEngineOnce(4, data, opts, 4));
  if (maxOut !== undefined && out.length > maxOut) throw new ERR_BUFFER_TOO_LARGE(maxOut);
  return __zCheckKMax(out);
}
export function inflateRaw(buf, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  if (opts && opts.info) { const eng = new InflateRaw(opts); try { const r = inflateRawSync(buf, opts); cb(null, { buffer: r.buffer ?? r, engine: eng }); } catch (e) { cb(e); } return; }
  __zNeedCb(cb, "inflateRaw");
  const data = __zChecked(buf, "inflateRaw");
  __zAsync((d) => inflateRawSync__core(d, opts), [data], cb);
}
function gzipSync__core(buf, opts) {
  const data = __zChecked(buf, "gzip");
  __zCheckZlibOpts(opts, 9, false);
  const lv = __zLevel(opts, -1);
  __zFlush(opts, false);
  return __zBuf(__zEngineOnce(2, data, opts, 4, lv));
}
export function gzip(buf, opts, cb) {
  if (opts && opts.info && typeof cb === "function") { const C = Gzip; const eng = new C(opts); try { const r = gzipSync(buf, opts); cb(null, { buffer: r.buffer ?? r, engine: eng }); } catch (e) { cb(e); } return; }
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  __zNeedCb(cb, "gzip");
  const data = __zChecked(buf, "gzip");
  const lv = __zLevel(opts, -1);
  __zAsync((d) => gzipSync__core(d, opts), [data], cb);
}
function gunzipSync__core(buf, opts) {
  const data = __zChecked(buf, "gunzip");
  __zCheckZlibOpts(opts, 8, true);
  return __zCheckKMax(__zBuf(__zEngineOnce(5, data, opts, 4)));
}
export function gunzip(buf, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  if (opts && opts.info) { const eng = new Gunzip(opts); try { const r = gunzipSync(buf, opts); cb(null, { buffer: r.buffer ?? r, engine: eng }); } catch (e) { cb(e); } return; }
  __zNeedCb(cb, "gunzip");
  const data = __zChecked(buf, "gunzip");
  __zAsync((d) => gunzipSync__core(d, opts), [data], cb);
}
function unzipSync__core(buf, opts) {
  const data = __zChecked(buf, "unzip");
  __zCheckZlibOpts(opts, 8, true);
  return __zCheckKMax(__zBuf(__zEngineOnce(7, data, opts, 4)));
}
export function unzip(buf, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  if (opts && opts.info) { const eng = new Unzip(opts); try { const r = unzipSync(buf, opts); cb(null, { buffer: r.buffer ?? r, engine: eng }); } catch (e) { cb(e); } return; }
  __zNeedCb(cb, "unzip");
  const data = __zChecked(buf, "unzip");
  __zAsync((d) => unzipSync__core(d, opts), [data], cb);
}
function brotliCompressSync__core(buf, opts) {
  const data = __zChecked(buf, "brotliCompress");
  const q = __zQuality(opts);
  __zFlush(opts, true);
  // 走引擎（dict/quality/错误口径全经既有接线；同 q 下与裸 native
  // CompressorWriter 输出逐字节一致，非 dict 场景零回归）。
  return __zBuf(__zEngineOnce(8, data, opts, 2, q));
}
export function brotliCompress(buf, opts, cb) {
  if (opts && opts.info && typeof cb === "function") { const C = BrotliCompress; const eng = new C(opts); try { const r = brotliCompressSync(buf, opts); cb(null, { buffer: r.buffer ?? r, engine: eng }); } catch (e) { cb(e); } return; }
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  __zNeedCb(cb, "brotliCompress");
  const data = __zChecked(buf, "brotliCompress");
  __zAsync((d) => brotliCompressSync__core(d, opts), [data], cb);
}
function brotliDecompressSync__core(buf, opts) {
  const data = __zChecked(buf, "brotliDecompress");
  const maxOut = __zMaxOut(opts);
  const out = __zBuf(__zEngineOnce(9, data, opts, 2));
  if (maxOut !== undefined && out.length > maxOut) throw new ERR_BUFFER_TOO_LARGE(maxOut);
  return __zCheckKMax(out);
}
export function brotliDecompress(buf, opts, cb) {
  if (opts && opts.info && typeof cb === "function") { const C = BrotliDecompress; const eng = new C(opts); try { const r = brotliDecompressSync(buf, opts); cb(null, { buffer: r.buffer ?? r, engine: eng }); } catch (e) { cb(e); } return; }
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  __zNeedCb(cb, "brotliDecompress");
  const data = __zChecked(buf, "brotliDecompress");
  __zAsync((d) => brotliDecompressSync__core(d, opts), [data], cb);
}
function zstdCompressSync__core(buf, opts) {
  const data = __zChecked(buf, "zstdCompress");
  // 走引擎：pledgedSrcSize 经 __zEngineOnce 接线终检（mismatch →
  // ZSTD_error_srcSize_wrong）；flag 2 单帧与裸 native compress_to_vec 同函数同字节。
  return __zBuf(__zEngineOnce(10, data, opts, 2));
}
export function zstdCompress(buf, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  if (opts && opts.info) { const eng = new ZstdCompress(opts); try { const r = zstdCompressSync(buf, opts); cb(null, { buffer: r.buffer ?? r, engine: eng }); } catch (e) { cb(e); } return; }
  __zNeedCb(cb, "zstdCompress");
  const data = __zChecked(buf, "zstdCompress");
  __zAsync((d) => zstdCompressSync__core(d, opts), [data], cb);
}
function zstdDecompressSync__core(buf, opts) {
  const data = __zChecked(buf, "zstdDecompress");
  const maxOut = __zMaxOut(opts);
  const out = __zBuf(__zEngineOnce(11, data, opts, 2));
  if (maxOut !== undefined && out.length > maxOut) throw new ERR_BUFFER_TOO_LARGE(maxOut);
  return __zCheckKMax(out);
}
export function zstdDecompress(buf, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  if (opts && opts.info) { const eng = new ZstdDecompress(opts); try { const r = zstdDecompressSync(buf, opts); cb(null, { buffer: r.buffer ?? r, engine: eng }); } catch (e) { cb(e); } return; }
  __zNeedCb(cb, "zstdDecompress");
  const data = __zChecked(buf, "zstdDecompress");
  __zAsync((d) => zstdDecompressSync__core(d, opts), [data], cb);
}
// 流式类（G9-2：走 G9-1 的增量引擎 __wjs2_zlib_stream_*，真增量语义——
// write 即时压出、flush 档位即时出边界、finishFlush 容忍截断、
// rejectGarbageAfterEnd 解压 junk 报错）。
// 覆盖 12 类 + createXxx 工厂 + info 选项（{buffer, engine}）+ bytesWritten。
// dict 已接引擎（严格校验：Buffer/TypedArray/DataView/ArrayBuffer；raw 族构造期
// 主动 set_dictionary，zlib 族 FDICT 被动恢复）；pledgedSrcSize 仅 zstd 压缩侧；
// windowBits/memLevel/strategy 接受忽略（构造校验只做 failed-init 套件口径：
// chunkSize 范围）。
import { Transform } from "node:stream";
function __zStreamBase(opts, syncFn, kind) {
  Transform.call(this);
  // 构造期 flush 系选项校验（flush-flags 套件，真机逐字）：三键 undefined 即跳过；
  // 非 number → ARG_TYPE；非整数/越界 → OUT_OF_RANGE（选项口径恒 0..5，
  // 与 flush() 方法的逐族集不同；Sync 便捷函数暂不复验）。
  for (const k of ["flush", "finishFlush", "fullFlush"]) {
    const f = opts?.[k];
    if (f === undefined) continue;
    if (typeof f !== "number") {
      throw new ERR_INVALID_ARG_TYPE(`options.${k}`, "number", f);
    }
    if (!Number.isInteger(f) || f < 0 || f > 5) {
      const err = new RangeError(`The value of "options.${k}" is out of range. It must be >= 0 and <= 5. Received ${f}`);
      err.code = "ERR_OUT_OF_RANGE";
      throw err;
    }
  }
  // 真机口径（实测）：_flushFlag（write 档，默认 Z_NO_FLUSH）/ _finishFlushFlag
  // （end 终结档，默认族内 Z_FINISH；finishFlush 选项覆盖——truncated 套件）。
  this.__kind = kind;
  this._flushFlag = opts?.flush ?? 0;
  this._finishFlushFlag = opts?.finishFlush ?? __zKindDefaultFinish(kind);
  this.__syncFn = syncFn;
  this.__opts = opts ?? {};
  this.bytesWritten = 0;
  // node Zlib 口径（destroy/close-after-error/reset-during-write 套件点名）：
  // _handle 开流非空、close/destroy 后置空；_closed 同步；_chunkSize/_outOffset
  // 内部计数（_processChunk 越界门）；__writeActive 标记分发中的写
  // （reset-during-write 套件：同 tick 内 reset 即抛）。
  const self = this;
  // 增量引擎：构造即建（真机 handle 同期）；reject01=rejectGarbageAfterEnd（解压族）。
  // 字典严格校验 + pledgedSrcSize（仅 zstd 压缩侧，Node 口径解压类忽略）。
  const reject = __zCheckRejectOpt(opts) ? 1 : 0;
  const dict = __zDictBytes(opts);
  const pledged = kind === 10 ? __zCheckPledged(opts) : -1;
  this.__zid = __wjs2_zlib_stream_new(
    kind,
    Number.isInteger(opts?.level) ? opts.level : -1,
    dict, pledged, reject);
  // bytesWritten 记账：feed 返回 unconsumed（引擎侧剩余未消费），consumed =
  // 本块长 + 上轮剩余 - 本轮剩余（premature-end 套件：trailing 垃圾不计入）。
  this.__pend = 0;
  this._handle = {
    reset: () => {
      if (self.__writeActive) throw new Error("Cannot reset zlib stream while a write is in progress");
      __wjs2_zlib_stream_reset(self.__zid);
      self.__pend = 0;
    },
  };
  this._closed = false;
  this._chunkSize = (opts && Number.isInteger(opts.chunkSize)) ? opts.chunkSize : 16384;
  this._outOffset = 0;
  this.__writeActive = false;
}
Object.setPrototypeOf(__zStreamBase.prototype, Transform.prototype);
Object.setPrototypeOf(__zStreamBase, Transform);
// 引擎喂入：即时压出（真增量），返回输出 Buffer（无输出 null）；
// 错误（引擎 Z_* / junk）抛 __zThrowEngine 形（裸 message + code）。
// bytesWritten 只计引擎实际消费（feed 回 unconsumed，见构造器记账）。
__zStreamBase.prototype.__zFeed = function (u8, flag) {
  const n = u8 ? u8.length : 0;
  const r = JSON.parse(__wjs2_zlib_stream_feed(this.__zid, u8 ?? null, flag));
  this.bytesWritten += n + this.__pend - (r.c || 0);
  this.__pend = r.c || 0;
  if (r.code !== undefined) __zThrowEngine(r.code, r.msg);
  const out = __wjs2_zlib_stream_out(this.__zid);
  return out.length ? Buffer.from(out.buffer, out.byteOffset, out.byteLength) : null;
};
function __zToU8(chunk) {
  if (typeof chunk === "string") return Buffer.from(chunk);
  if (chunk instanceof ArrayBuffer) return new Uint8Array(chunk);
  if (ArrayBuffer.isView(chunk)) return new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength);
  return chunk;
}
__zStreamBase.prototype._transform = function (chunk, encoding, cb) {
  const u8 = __zToU8(chunk);
  // 写分发标记（reset-during-write 套件）：microtask 清零——同 tick 内 reset 可见，
  // 下 tick 已落定不再抛（与真机"分发中"窗口对等）。
  this.__writeActive = true;
  // 喂入异步化（真机 native write 回调形态）：同步 push+cb 会重入 Writable
  // 完成路径（write 回调丢失/挂起），microtask 内 feed+push+cb 保序。
  queueMicrotask(() => {
    this.__writeActive = false;
    try {
      const out = this.__zFeed(u8, this._flushFlag);
      if (out) this.push(out);
      cb();
    } catch (e) { cb(e); }
  });
};
__zStreamBase.prototype._flush = function (cb) {
  try {
    const out = this.__zFeed(null, this._finishFlushFlag);
    if (out) this.push(out);
    cb();
  } catch (e) {
    // zlib 族流面终结：Z_BUF_ERROR 非致命（真机 processCallback 同款——
    // 输入已尽、数据已出的流照常终结，flush-write-sync-interleaved/
    // write-after-flush 套件；sync 面仍报——truncated 套件）。zstd 严格。
    if (e && e.code === "Z_BUF_ERROR" && this.__kind <= 7) { cb(); return; }
    cb(e);
  }
};
// flush(kind?, cb)：即时档位压出（G9-2 增量——feed 空 + kind 档，边界即出）。
// kind 逐族校验（flush-invalid-kind 套件，真机口径）：undefined/NaN/函数直通；
// 非 number → ARG_TYPE；zlib 族 {0,2,4} / brotli {0,1,2,3} / zstd {0,1,2} 之外 → OUT_OF_RANGE。
// 无 kind 默认族内 Full/Flush 档（真机 flush() 口径）；cb 异步触发（完成回调）。
__zStreamBase.prototype.flush = function (kind, cb) {
  if (typeof kind === "function") { cb = kind; kind = undefined; }
  if (kind !== undefined && !(typeof kind === "number" && Number.isNaN(kind))) {
    if (typeof kind !== "number") {
      throw new ERR_INVALID_ARG_TYPE("flush", "number", kind);
    }
    const nm = this.__engineName || "";
    // 真机集（flush-invalid-kind 套件逐字）：zlib {Z_NO_FLUSH,Z_FINISH,Z_BLOCK}={0,4,5}，
    // brotli {PROCESS,FLUSH,FINISH,EMIT_METADATA}={0,1,2,3}，zstd {continue,flush,end}={0,1,2}。
    const valid = /brotli/i.test(nm) ? [0, 1, 2, 3] : (/zstd/i.test(nm) ? [0, 1, 2] : [0, 1, 2, 3, 4, 5]);
    if (!valid.includes(kind)) {
      const err = new RangeError(`The value of "flush" is out of range. It must be one of ${valid.join(", ")}. Received ${kind}`);
      err.code = "ERR_OUT_OF_RANGE";
      throw err;
    }
  }
  const flag = (kind === undefined || Number.isNaN(kind))
    ? __zKindDefaultFlush(this.__kind) : kind;
  // 调度对齐真机 handle.write FIFO：writable 有排队 chunk 时排宏任务
  // （晚于派发链全部 microtask——write-after-end 套件）；空闲时排 microtask
  // （早于其后的 write/end 链——write-after-flush 套件：flush() 在 write 前）。
  const run = () => {
    try {
      const out = this.__zFeed(null, flag);
      if (out) this.push(out);
      if (typeof cb === "function") cb();
    } catch (e) {
      if (typeof cb === "function") cb(e);
      else this.emit("error", e);
    }
  };
  if (this.writableLength > 0) setTimeout(run, 0);
  else queueMicrotask(run);
};
__zStreamBase.prototype._destroy = function (err, cb) {
  // 收尾旗与柄同 _destroy 点置位（与 Node 同步点一致：destroy() 后同步可见）；
  // 引擎同点 free（真机 handle.destroy 同期）。
  this._closed = true;
  this._handle = null;
  if (this.__zid !== undefined) {
    __wjs2_zlib_stream_free(this.__zid);
    this.__zid = undefined;
  }
  if (typeof Transform.prototype._destroy === "function") Transform.prototype._destroy.call(this, err, cb);
  else cb(err);
};
__zStreamBase.prototype.close = function (cb) {
  // 真机口径（实测）：close 即撕毁、不落数据（write 后 close 无 data/finish/end，
  // 只有 close+cb），等价无错 destroy；_closed/空柄同步置位。
  this._closed = true;
  this._handle = null;
  if (typeof cb === "function") {
    if (this.closed) queueMicrotask(cb);
    else this.once("close", cb);
  }
  if (!this.destroyed) this.destroy();
  return this;
};
__zStreamBase.prototype.reset = function () {
  // 真机口径（实测）：已关闭即 ERR_INTERNAL_ASSERTION（zlib binding closed）。
  if (!this._handle) {
    const e = new Error("zlib binding closed");
    e.code = "ERR_INTERNAL_ASSERTION";
    throw e;
  }
  this._handle.reset();
};
// _processChunk(chunk, flushFlag)：同步内部处理（sync-no-event/invalid-input 套件）。
// G9-2：直接走引擎（flag 原样透传，Z_FINISH 即终结压出）；_outOffset 越界即 RangeError。
__zStreamBase.prototype._processChunk = function (chunk, flag) {
  if (this._outOffset > this._chunkSize) {
    const err = new RangeError(`The value of "_outOffset" is out of range. It must be <= ${this._chunkSize}. Received ${this._outOffset}`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  const out = this.__zFeed(__zToU8(chunk), flag);
  return out ?? Buffer.alloc(0);
};
__zStreamBase.prototype.params = function (level, strategy) {
  if (typeof level !== "number") {
    throw new ERR_INVALID_ARG_TYPE("level", "number", level);
  }
  if (!Number.isFinite(level)) {
    const err = new RangeError(`The value of "level" is out of range. It must be a finite number. Received ${level}`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  if (!Number.isInteger(level) || level < -1 || level > 9) {
    const err = new RangeError(`The value of "level" is out of range. It must be >= -1 and <= 9. Received ${level}`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  this._level = level;
  if (strategy !== undefined) {
    if (typeof strategy !== "number") {
      throw new ERR_INVALID_ARG_TYPE("strategy", "number", strategy);
    }
    if (!Number.isFinite(strategy)) {
      const err = new RangeError(`The value of "strategy" is out of range. It must be a finite number. Received ${strategy}`);
      err.code = "ERR_OUT_OF_RANGE";
      throw err;
    }
    if (!Number.isInteger(strategy) || strategy < 0 || strategy > 4) {
      const err = new RangeError(`The value of "strategy" is out of range. It must be >= 0 and <= 4. Received ${strategy}`);
      err.code = "ERR_OUT_OF_RANGE";
      throw err;
    }
    this._strategy = strategy;
  }
};
function __zMakeClass(kind, syncFn, check) {
  function C(opts) {
    // Node 口径：流类裸调返回新实例（DEP0184 deprecate 警告略）。
    if (!(this instanceof C)) return new C(opts);
    if (check) check(opts);
    __zStreamBase.call(this, opts, syncFn, kind);
    this.__engineName = syncFn.name || "Zlib";
    // failed-init 套件口径：_level/_strategy 属性（NaN 回落默认值）。
    const lv = opts?.level;
    this._level = Number.isInteger(lv) ? lv : constants.Z_DEFAULT_COMPRESSION;
    const st = opts?.strategy;
    this._strategy = Number.isInteger(st) ? st : constants.Z_DEFAULT_STRATEGY;
  }
  Object.setPrototypeOf(C.prototype, __zStreamBase.prototype);
  Object.setPrototypeOf(C, __zStreamBase);
  return C;
}
function __zCheckChunkSize(opts) {
  if (opts && opts.chunkSize !== undefined) {
    const c = opts.chunkSize;
    if (typeof c !== "number") {
      throw new ERR_INVALID_ARG_TYPE("options.chunkSize", "number", c);
    }
    if (!Number.isFinite(c)) {
      const err = new RangeError(`The value of "options.chunkSize" is out of range. It must be a finite number. Received ${c}`);
      err.code = "ERR_OUT_OF_RANGE";
      throw err;
    }
    if (c < 64) {
      const err = new RangeError(`The value of "options.chunkSize" is out of range. It must be >= 64. Received ${c}`);
      err.code = "ERR_OUT_OF_RANGE";
      throw err;
    }
  }
}
function __zCheckZlibOpts(opts, minWB = 8, allowWB0 = false) {
  __zCheckChunkSize(opts);
  if (opts && opts.dictionary !== undefined) {
    const d = opts.dictionary;
    if (typeof d === "string" || !(d instanceof Uint8Array || d instanceof ArrayBuffer || ArrayBuffer.isView(d))) {
      throw new ERR_INVALID_ARG_TYPE("options.dictionary", ["Buffer", "TypedArray", "DataView", "ArrayBuffer"], d);
    }
  }
  if (opts && opts.windowBits !== undefined) {
    const w = opts.windowBits;
    // 解压侧 windowBits 0 合法（用流头窗口；Node Zlib 原文口径）。
    if (w === 0 && allowWB0) return;
    if (typeof w !== "number") {
      throw new ERR_INVALID_ARG_TYPE("options.windowBits", "number", w);
    }
    if (!Number.isFinite(w)) {
      const err = new RangeError(`The value of "options.windowBits" is out of range. It must be a finite number. Received ${w}`);
      err.code = "ERR_OUT_OF_RANGE";
      throw err;
    }
    if (!Number.isInteger(w) || w < minWB || w > 15) {
      const err = new RangeError(`The value of "options.windowBits" is out of range. It must be >= ${minWB} and <= 15. Received ${w}`);
      err.code = "ERR_OUT_OF_RANGE";
      throw err;
    }
  }
  if (opts && opts.level !== undefined) {
    const lv = opts.level;
    if (typeof lv !== "number") {
      throw new ERR_INVALID_ARG_TYPE("options.level", "number", lv);
    }
    // NaN 回落默认值（Node checkRangesOrGetDefault 口径；failed-init 套件）。
    if (!Number.isNaN(lv)) {
      if (!Number.isFinite(lv)) {
        const err = new RangeError(`The value of "options.level" is out of range. It must be a finite number. Received ${lv}`);
        err.code = "ERR_OUT_OF_RANGE";
        throw err;
      }
      if (!Number.isInteger(lv) || lv < -1 || lv > 9) {
        const err = new RangeError(`The value of "options.level" is out of range. It must be >= -1 and <= 9. Received ${lv}`);
        err.code = "ERR_OUT_OF_RANGE";
        throw err;
      }
    }
  }
  if (opts && opts.memLevel !== undefined) {
    const m = opts.memLevel;
    if (typeof m !== "number") {
      throw new ERR_INVALID_ARG_TYPE("options.memLevel", "number", m);
    }
    if (!Number.isFinite(m)) {
      const err = new RangeError(`The value of "options.memLevel" is out of range. It must be a finite number. Received ${m}`);
      err.code = "ERR_OUT_OF_RANGE";
      throw err;
    }
    if (!Number.isInteger(m) || m < 1 || m > 9) {
      const err = new RangeError(`The value of "options.memLevel" is out of range. It must be >= 1 and <= 9. Received ${m}`);
      err.code = "ERR_OUT_OF_RANGE";
      throw err;
    }
  }
  if (opts && opts.strategy !== undefined) {
    const st2 = opts.strategy;
    if (typeof st2 !== "number") {
      throw new ERR_INVALID_ARG_TYPE("options.strategy", "number", st2);
    }
    // NaN 回落默认值（同 level）。
    if (!Number.isNaN(st2)) {
      if (!Number.isFinite(st2)) {
        const err = new RangeError(`The value of "options.strategy" is out of range. It must be a finite number. Received ${st2}`);
        err.code = "ERR_OUT_OF_RANGE";
        throw err;
      }
      if (!Number.isInteger(st2) || st2 < 0 || st2 > 4) {
        const err = new RangeError(`The value of "options.strategy" is out of range. It must be >= 0 and <= 4. Received ${st2}`);
        err.code = "ERR_OUT_OF_RANGE";
        throw err;
      }
    }
  }
}
export const Deflate = __zMakeClass(0, deflateSync, (o) => __zCheckZlibOpts(o, 8, false));
export const Inflate = __zMakeClass(3, inflateSync, (o) => __zCheckZlibOpts(o, 8, true));
export const Gzip = __zMakeClass(2, gzipSync, (o) => __zCheckZlibOpts(o, 9, false));
export const Gunzip = __zMakeClass(5, gunzipSync, (o) => __zCheckZlibOpts(o, 8, true));
export const DeflateRaw = __zMakeClass(1, deflateRawSync, (o) => __zCheckZlibOpts(o, 8, false));
export const InflateRaw = __zMakeClass(4, inflateRawSync, (o) => __zCheckZlibOpts(o, 8, true));
export const Unzip = __zMakeClass(7, unzipSync, (o) => __zCheckZlibOpts(o, 8, true));
export const BrotliCompress = __zMakeClass(8, brotliCompressSync, (o) => { __zCheckChunkSize(o); __zCheckBrotliParams(o); });
export const BrotliDecompress = __zMakeClass(9, brotliDecompressSync, (o) => { __zCheckChunkSize(o); __zCheckBrotliParams(o); });
export const ZstdCompress = __zMakeClass(10, zstdCompressSync, __zCheckChunkSize);
export const ZstdDecompress = __zMakeClass(11, zstdDecompressSync, __zCheckChunkSize);
export const BrotliEncode = BrotliCompress;
export const BrotliDecode = BrotliDecompress;
function __zCreate(C) {
  return (opts) => new C(opts);
}
export const createDeflate = __zCreate(Deflate);
export const createInflate = __zCreate(Inflate);
export const createGzip = __zCreate(Gzip);
export const createGunzip = __zCreate(Gunzip);
export const createDeflateRaw = __zCreate(DeflateRaw);
export const createInflateRaw = __zCreate(InflateRaw);
export const createUnzip = __zCreate(Unzip);
export const createBrotliCompress = __zCreate(BrotliCompress);
export const createBrotliDecompress = __zCreate(BrotliDecompress);
export const createZstdCompress = __zCreate(ZstdCompress);
export const createZstdDecompress = __zCreate(ZstdDecompress);
export function deflateSync(buf, opts) { if (opts && opts.info) return __zInfoWrap(deflateSync__core, Deflate, opts, buf); return deflateSync__core(buf, opts); }
export function inflateSync(buf, opts) { if (opts && opts.info) return __zInfoWrap(inflateSync__core, Inflate, opts, buf); return inflateSync__core(buf, opts); }
export function deflateRawSync(buf, opts) { if (opts && opts.info) return __zInfoWrap(deflateRawSync__core, DeflateRaw, opts, buf); return deflateRawSync__core(buf, opts); }
export function inflateRawSync(buf, opts) { if (opts && opts.info) return __zInfoWrap(inflateRawSync__core, InflateRaw, opts, buf); return inflateRawSync__core(buf, opts); }
export function gzipSync(buf, opts) { if (opts && opts.info) return __zInfoWrap(gzipSync__core, Gzip, opts, buf); return gzipSync__core(buf, opts); }
export function gunzipSync(buf, opts) { if (opts && opts.info) return __zInfoWrap(gunzipSync__core, Gunzip, opts, buf); return gunzipSync__core(buf, opts); }
export function unzipSync(buf, opts) { if (opts && opts.info) return __zInfoWrap(unzipSync__core, Unzip, opts, buf); return unzipSync__core(buf, opts); }
export function brotliCompressSync(buf, opts) { if (opts && opts.info) return __zInfoWrap(brotliCompressSync__core, BrotliCompress, opts, buf); return brotliCompressSync__core(buf, opts); }
export function brotliDecompressSync(buf, opts) { if (opts && opts.info) return __zInfoWrap(brotliDecompressSync__core, BrotliDecompress, opts, buf); return brotliDecompressSync__core(buf, opts); }
export function zstdCompressSync(buf, opts) { if (opts && opts.info) return __zInfoWrap(zstdCompressSync__core, ZstdCompress, opts, buf); return zstdCompressSync__core(buf, opts); }
export function zstdDecompressSync(buf, opts) { if (opts && opts.info) return __zInfoWrap(zstdDecompressSync__core, ZstdDecompress, opts, buf); return zstdDecompressSync__core(buf, opts); }
// convenience 的 info 选项：{buffer, engine}（Node zlibBuffer/zlibBufferSync 口径）。
function __zInfoWrap(syncFn, C, opts, buf) {
  if (opts && opts.info) {
    const engine = new C(opts);
    return { buffer: syncFn(buf, opts), engine };
  }
  return syncFn(buf, opts);
}
export const constants = {
  Z_OK: 0, Z_STREAM_END: 1, Z_NEED_DICT: 2, Z_ERRNO: -1, Z_STREAM_ERROR: -2,
  Z_DATA_ERROR: -3, Z_MEM_ERROR: -4, Z_BUF_ERROR: -5, Z_VERSION_ERROR: -6,
  Z_NO_FLUSH: 0, Z_PARTIAL_FLUSH: 1, Z_SYNC_FLUSH: 2, Z_FULL_FLUSH: 3,
  Z_FINISH: 4, Z_BLOCK: 5, Z_TREES: 6,
  Z_NO_COMPRESSION: 0, Z_BEST_SPEED: 1, Z_BEST_COMPRESSION: 9,
  Z_DEFAULT_COMPRESSION: -1,
  Z_FILTERED: 1, Z_HUFFMAN_ONLY: 2, Z_RLE: 3, Z_FIXED: 4, Z_DEFAULT_STRATEGY: 0,
  Z_DEFLATED: 8,
  Z_DEFAULT_WINDOWBITS: 15, Z_MIN_WINDOWBITS: 8, Z_MAX_WINDOWBITS: 15,
  Z_MIN_MEMLEVEL: 1, Z_MAX_MEMLEVEL: 9, Z_DEFAULT_MEMLEVEL: 8,
  Z_DEFAULT_CHUNK: 16384,
  Z_MAX_CHUNK: Infinity,
  BROTLI_OPERATION_PROCESS: 0, BROTLI_OPERATION_FLUSH: 1,
  BROTLI_OPERATION_FINISH: 2, BROTLI_OPERATION_EMIT_METADATA: 3,
  BROTLI_PARAM_MODE: 0, BROTLI_PARAM_QUALITY: 1, BROTLI_PARAM_LGWIN: 2,
  BROTLI_PARAM_LGBLOCK: 3, BROTLI_PARAM_DISABLE_LITERAL_CONTEXT_MODELING: 4,
  BROTLI_PARAM_SIZE_HINT: 5, BROTLI_PARAM_LARGE_WINDOW: 6,
  BROTLI_MODE_GENERIC: 0, BROTLI_MODE_TEXT: 1, BROTLI_MODE_FONT: 2,
  BROTLI_DEFAULT_QUALITY: 11, BROTLI_MIN_QUALITY: 0, BROTLI_MAX_QUALITY: 11,
  BROTLI_DECODE: 8, BROTLI_ENCODE: 9,
  // R3a：绑定层模式常量（真机值；旧 BROTLI_DECODE:0/ENCODE:1 系本仓流向自定，
  // 无他处引用，按真机 8/9 纠正）+ 缺失档补齐。
  DEFLATE: 1, INFLATE: 2, GZIP: 3, GUNZIP: 4,
  ZSTD_COMPRESS: 10, ZSTD_DECOMPRESS: 11,
  Z_MIN_CHUNK: 64, Z_MIN_LEVEL: -1, Z_MAX_LEVEL: 9,
  ZSTD_e_continue: 0, ZSTD_e_flush: 1, ZSTD_e_end: 2,
  // R3a：zstd param 族（真机值；iter/transform 按名取最大值定参数数组界）。
  ZSTD_c_compressionLevel: 100, ZSTD_c_windowLog: 101, ZSTD_c_hashLog: 102,
  ZSTD_c_chainLog: 103, ZSTD_c_searchLog: 104, ZSTD_c_minMatch: 105,
  ZSTD_c_targetLength: 106, ZSTD_c_strategy: 107,
  ZSTD_c_enableLongDistanceMatching: 160, ZSTD_c_ldmHashLog: 161,
  ZSTD_c_ldmMinMatch: 162, ZSTD_c_ldmBucketSizeLog: 163,
  ZSTD_c_ldmHashRateLog: 164, ZSTD_c_contentSizeFlag: 200,
  ZSTD_c_checksumFlag: 201, ZSTD_c_dictIDFlag: 202, ZSTD_c_nbWorkers: 400,
  ZSTD_c_jobSize: 401, ZSTD_c_overlapLog: 402, ZSTD_d_windowLogMax: 100,
  // ZSTD 错误码族（真机 26.8.2 zlib.constants 逐项导出为准；pledged 套件
  // 取 ZSTD_error_srcSize_wrong=72）。
  ZSTD_error_no_error: 0, ZSTD_error_GENERIC: 1,
  ZSTD_error_prefix_unknown: 10, ZSTD_error_version_unsupported: 12,
  ZSTD_error_frameParameter_unsupported: 14, ZSTD_error_frameParameter_windowTooLarge: 16,
  ZSTD_error_corruption_detected: 20, ZSTD_error_checksum_wrong: 22,
  ZSTD_error_literals_headerWrong: 24, ZSTD_error_dictionary_corrupted: 30,
  ZSTD_error_dictionary_wrong: 32, ZSTD_error_dictionaryCreation_failed: 34,
  ZSTD_error_parameter_unsupported: 40, ZSTD_error_parameter_combination_unsupported: 41,
  ZSTD_error_parameter_outOfBound: 42, ZSTD_error_tableLog_tooLarge: 44,
  ZSTD_error_maxSymbolValue_tooLarge: 46, ZSTD_error_maxSymbolValue_tooSmall: 48,
  ZSTD_error_stabilityCondition_notRespected: 50, ZSTD_error_stage_wrong: 60,
  ZSTD_error_init_missing: 62, ZSTD_error_memory_allocation: 64,
  ZSTD_error_workSpace_tooSmall: 66, ZSTD_error_dstSize_tooSmall: 70,
  ZSTD_error_srcSize_wrong: 72, ZSTD_error_dstBuffer_null: 74,
  ZSTD_error_noForwardProgress_destFull: 80, ZSTD_error_noForwardProgress_inputEmpty: 82,
};
export const codes = {
  Z_OK: 0, Z_STREAM_END: 1, Z_NEED_DICT: 2, Z_ERRNO: -1, Z_STREAM_ERROR: -2,
  Z_DATA_ERROR: -3, Z_MEM_ERROR: -4, Z_BUF_ERROR: -5, Z_VERSION_ERROR: -6,
  0: "Z_OK", 1: "Z_STREAM_END", 2: "Z_NEED_DICT", "-1": "Z_ERRNO",
  "-2": "Z_STREAM_ERROR", "-3": "Z_DATA_ERROR", "-4": "Z_MEM_ERROR",
  "-5": "Z_BUF_ERROR", "-6": "Z_VERSION_ERROR",
};
const __api = {
  deflate, deflateSync, inflate, inflateSync,
  deflateRaw, deflateRawSync, inflateRaw, inflateRawSync,
  gzip, gzipSync, gunzip, gunzipSync, unzip, unzipSync,
  brotliCompress, brotliCompressSync, brotliDecompress, brotliDecompressSync,
  zstdCompress, zstdCompressSync, zstdDecompress, zstdDecompressSync,
  crc32,
  Deflate, Inflate, Gzip, Gunzip, DeflateRaw, InflateRaw, Unzip,
  BrotliCompress, BrotliDecompress, BrotliEncode, BrotliDecode,
  ZstdCompress, ZstdDecompress,
  createDeflate, createInflate, createGzip, createGunzip,
  createDeflateRaw, createInflateRaw, createUnzip,
  createBrotliCompress, createBrotliDecompress,
  createZstdCompress, createZstdDecompress,
  constants, codes,
};
// Zip 实验面（node lib/zlib.js 口径：使用时警告，导入/访问不警告；
// instanceof 经 Symbol.hasInstance 透传内部实现）。
let __zipWarned = false;
function __zipWarn() {
  if (__zipWarned) return;
  __zipWarned = true;
  try {
    if (typeof process !== 'undefined' && typeof process.emitWarning === 'function') {
      process.emitWarning('The zlib ZIP archive API is an experimental feature and might change at any time', 'ExperimentalWarning');
    }
  } catch {}
}
function __zipFn(fn) {
  return function(...args) { __zipWarn(); return Reflect.apply(fn, undefined, args); };
}
class __ZipEntry extends (zipEntryMod.ZipEntry ?? Object) {
  static [Symbol.hasInstance](v) { try { return v instanceof (zipEntryMod.ZipEntry ?? Object); } catch { return false; } }
}
class __ZipFile extends (zipFileMod.ZipFile ?? Object) {
  static [Symbol.hasInstance](v) { try { return v instanceof (zipFileMod.ZipFile ?? Object); } catch { return false; } }
}
class __ZipBuffer extends (zipBufferMod.ZipBuffer ?? Object) {
  static [Symbol.hasInstance](v) { try { return v instanceof (zipBufferMod.ZipBuffer ?? Object); } catch { return false; } }
  constructor(...args) { __zipWarn(); super(...args); }
}
for (const n of ['read', 'create', 'createSync', 'createStream', 'createSymlink']) {
  if (typeof zipEntryMod.ZipEntry?.[n] === 'function') {
    const raw = zipEntryMod.ZipEntry[n];
    Object.defineProperty(__ZipEntry, n, { configurable: true, writable: true, value: __zipFn(raw.bind(zipEntryMod.ZipEntry)) });
  }
}
for (const n of ['open', 'openSync']) {
  if (typeof zipFileMod.ZipFile?.[n] === 'function') {
    const raw = zipFileMod.ZipFile[n];
    Object.defineProperty(__ZipFile, n, { configurable: true, writable: true, value: __zipFn(raw.bind(zipFileMod.ZipFile)) });
  }
}
Object.defineProperty(__ZipEntry, 'name', { value: 'ZipEntry' });
Object.defineProperty(__ZipFile, 'name', { value: 'ZipFile' });
Object.defineProperty(__ZipBuffer, 'name', { value: 'ZipBuffer' });
__api.ZipEntry = __ZipEntry;
__api.ZipFile = __ZipFile;
__api.ZipBuffer = __ZipBuffer;
__api.createZipArchive = __zipFn(zipArchiveMod.createZipArchive);
__api.createZipArchiveSync = __zipFn(zipArchiveMod.createZipArchiveSync);
__api.zipFiles = __zipFn(zipArchiveMod.zipFiles);
__api.getMaxZipContentSize = __zipFn(zipContentSizeMod.getMaxZipContentSize);
__api.setMaxZipContentSize = __zipFn(zipContentSizeMod.setMaxZipContentSize);
Object.defineProperty(__api, "codes", { writable: false });
// 顶层非 BROTLI 别名（Node 遗留口径，非枚举）。
for (const [k, v] of Object.entries(constants)) {
  if (!k.startsWith("BROTLI")) Object.defineProperty(__api, k, { value: v, enumerable: false });
}
Object.freeze(constants);
Object.freeze(codes);
export const ZipEntry = __api.ZipEntry;
export const ZipFile = __api.ZipFile;
export const ZipBuffer = __api.ZipBuffer;
export const createZipArchive = __api.createZipArchive;
export const createZipArchiveSync = __api.createZipArchiveSync;
export const zipFiles = __api.zipFiles;
export const getMaxZipContentSize = __api.getMaxZipContentSize;
export const setMaxZipContentSize = __api.setMaxZipContentSize;
export default __api;
