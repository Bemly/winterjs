//! `node:zlib`：convenience 压缩面（flate2 + brotli + ruzstd，零新 crate，
//! dependencies2 §9d-剩余表口径；flate2 默认后端 miniz_oxide 纯 Rust，§2 门控沿用）。
//!
//! 覆盖：deflate/inflate/deflateRaw/inflateRaw/gzip/gunzip/unzip/
//! brotliCompress/brotliDecompress/zstdCompress/zstdDecompress（Sync + 回调），
//! `constants`（Z_* 取 zlib.h 通用值，BROTLI_* 经 brotli 9.0.0 轮子源码实测）+
//! `codes` 双向表 + 顶层非 BROTLI 别名（Node 口径）。
//! 偏差记档：
//! - 异步回调底层同步实现（`queueMicrotask` 派发；`node:fs` 同款"同步底层"口径，
//!   无线程池；大块阻塞 JS 线程，文档记录）。
//! - 流式类（Deflate/Inflate/Gzip…/`createXxx`）顺延（需 Transform 集成，另切片）。
//! - `crc32`、Zip 实验面（上游 experimental）不做。
//! - options 只 honor `level`（gzip/deflate 系，-1..9）与 `quality`/`params[1]`
//!   （brotli，0..11）；windowBits/memLevel/strategy/dictionary/flush 系接受忽略。
//! - zstd 编码恒用 `CompressionLevel::Fastest`——ruzstd 0.9.0 的 Default/Better/
//!   Best 标 UNIMPLEMENTED（源码实测），`level` 接受忽略；解码全量。

use mozjs::jsval::JSVal;
use mozjs::rooted;

use crate::jsapi_glue::{report_error, value_to_string, view_bytes, wrap_cx, Frame};

/// Node 压缩层级 → flate2（纯函数，单元测试覆盖）。
/// -1/越界外由 JS 侧先验（ERR_OUT_OF_RANGE），此处 -1 与缺省同归 default。
fn zlib_level(level: i32) -> flate2::Compression {
    if level < 0 {
        flate2::Compression::default()
    } else {
        flate2::Compression::new(level.clamp(0, 9) as u32)
    }
}

/// 数据实参（string 按 utf8；Uint8Array/ArrayBuffer 视图；其余 TypeError）。
fn arg_bytes(
    cx: &mut mozjs::context::JSContext,
    frame: &Frame,
    i: u32,
    what: &str,
) -> Option<Vec<u8>> {
    if frame.argc() <= i {
        report_error(cx, &format!("TypeError: {what} needs data"));
        return None;
    }
    let v = frame.arg(i);
    if v.is_string() {
        return Some(value_to_string(cx, v).into_bytes());
    }
    view_bytes(cx, v, &format!("{what} data"))
}

/// 层级/质量实参（缺省回 `dflt`；非数回 None 由调用方报 TypeError）。
fn arg_level(frame: &Frame, i: u32, dflt: i32) -> Option<i32> {
    if frame.argc() <= i || frame.arg(i).is_null_or_undefined() {
        return Some(dflt);
    }
    let v = frame.arg(i);
    if v.is_number() {
        Some(v.to_number() as i32)
    } else {
        None
    }
}

/// Uint8Array 返回值（`node:fs` 同款小 helper，不跨模块引）。
fn set_rval_bytes(cx: &mut mozjs::context::JSContext, frame: &Frame, out: &[u8]) -> bool {
    rooted!(&in(cx) let mut obj: *mut mozjs::jsapi::JSObject = std::ptr::null_mut());
    // SAFETY: realm 内创建；obj 为 rooted 出参；out 存活到调用返回
    let ok = unsafe {
        mozjs::typedarray::TypedArray::<mozjs::typedarray::Uint8, *mut mozjs::jsapi::JSObject>::create(
            cx,
            mozjs::typedarray::CreateWith::Slice(out),
            obj.handle_mut(),
        )
    };
    if ok.is_err() || obj.is_null() {
        report_error(cx, "RangeError: cannot allocate output");
        return false;
    }
    frame.set_rval(mozjs::jsval::ObjectValue(obj.get()));
    true
}

fn read_all<R: std::io::Read>(r: R) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    let mut out = Vec::new();
    r.take(1 << 31)
        .read_to_end(&mut out)
        .map_err(|e| format!("Z_DATA_ERROR: {e}"))?;
    Ok(out)
}

/// `__wjs_zlib_deflate_lv(dataU8, level)`（level -1=默认；JS 侧已验 -1..9）。
pub unsafe extern "C" fn zlib_deflate_lv(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(data) = arg_bytes(&mut cx, &frame, 0, "deflate") else {
        return false;
    };
    let Some(level) = arg_level(&frame, 1, -1) else {
        report_error(&mut cx, "TypeError: deflate level must be a number");
        return false;
    };
    use std::io::Write as _;
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), zlib_level(level));
    if let Err(e2) = e.write_all(&data) {
        report_error(&mut cx, &format!("Z_STREAM_ERROR: {e2}"));
        return false;
    }
    match e.finish() {
        Ok(out) => set_rval_bytes(&mut cx, &frame, &out),
        Err(e2) => {
            report_error(&mut cx, &format!("Z_STREAM_ERROR: {e2}"));
            false
        }
    }
}

/// `__wjs_zlib_inflate(dataU8)`（zlib 包裹）。
pub unsafe extern "C" fn zlib_inflate(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(data) = arg_bytes(&mut cx, &frame, 0, "inflate") else {
        return false;
    };
    match read_all(flate2::read::ZlibDecoder::new(&data[..])) {
        Ok(out) => set_rval_bytes(&mut cx, &frame, &out),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_zlib_deflate_raw(dataU8, level)`（裸 deflate）。
pub unsafe extern "C" fn zlib_deflate_raw(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(data) = arg_bytes(&mut cx, &frame, 0, "deflateRaw") else {
        return false;
    };
    let Some(level) = arg_level(&frame, 1, -1) else {
        report_error(&mut cx, "TypeError: deflateRaw level must be a number");
        return false;
    };
    use std::io::Write as _;
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), zlib_level(level));
    if let Err(e2) = e.write_all(&data) {
        report_error(&mut cx, &format!("Z_STREAM_ERROR: {e2}"));
        return false;
    }
    match e.finish() {
        Ok(out) => set_rval_bytes(&mut cx, &frame, &out),
        Err(e2) => {
            report_error(&mut cx, &format!("Z_STREAM_ERROR: {e2}"));
            false
        }
    }
}

/// `__wjs_zlib_inflate_raw(dataU8)`（裸 deflate 解码）。
pub unsafe extern "C" fn zlib_inflate_raw(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(data) = arg_bytes(&mut cx, &frame, 0, "inflateRaw") else {
        return false;
    };
    match read_all(flate2::read::DeflateDecoder::new(&data[..])) {
        Ok(out) => set_rval_bytes(&mut cx, &frame, &out),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_zlib_gzip(dataU8, level)`。
pub unsafe extern "C" fn zlib_gzip(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(data) = arg_bytes(&mut cx, &frame, 0, "gzip") else {
        return false;
    };
    let Some(level) = arg_level(&frame, 1, -1) else {
        report_error(&mut cx, "TypeError: gzip level must be a number");
        return false;
    };
    use std::io::Write as _;
    let mut e = flate2::write::GzEncoder::new(Vec::new(), zlib_level(level));
    if let Err(e2) = e.write_all(&data) {
        report_error(&mut cx, &format!("Z_STREAM_ERROR: {e2}"));
        return false;
    }
    match e.finish() {
        Ok(out) => set_rval_bytes(&mut cx, &frame, &out),
        Err(e2) => {
            report_error(&mut cx, &format!("Z_STREAM_ERROR: {e2}"));
            false
        }
    }
}

/// `__wjs_zlib_gunzip(dataU8)`。
pub unsafe extern "C" fn zlib_gunzip(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(data) = arg_bytes(&mut cx, &frame, 0, "gunzip") else {
        return false;
    };
    match read_all(flate2::read::GzDecoder::new(&data[..])) {
        Ok(out) => set_rval_bytes(&mut cx, &frame, &out),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_zlib_unzip(dataU8)`（gzip 优先、zlib 兜底；Node Unzip 自动识别口径）。
pub unsafe extern "C" fn zlib_unzip(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(data) = arg_bytes(&mut cx, &frame, 0, "unzip") else {
        return false;
    };
    if let Ok(out) = read_all(flate2::read::GzDecoder::new(&data[..])) {
        return set_rval_bytes(&mut cx, &frame, &out);
    }
    match read_all(flate2::read::ZlibDecoder::new(&data[..])) {
        Ok(out) => set_rval_bytes(&mut cx, &frame, &out),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_zlib_brotli_compress(dataU8, quality)`（quality 缺省 11）。
pub unsafe extern "C" fn zlib_brotli_compress(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(data) = arg_bytes(&mut cx, &frame, 0, "brotliCompress") else {
        return false;
    };
    let Some(q) = arg_level(&frame, 1, 11) else {
        report_error(&mut cx, "TypeError: brotli quality must be a number");
        return false;
    };
    let q = q.clamp(0, 11) as u32;
    use std::io::Write as _;
    let mut out = Vec::new();
    let r = (|| -> std::io::Result<()> {
        let mut w = brotli::CompressorWriter::new(&mut out, 4096, q, 22);
        w.write_all(&data)?;
        w.flush()?;
        Ok(())
    })();
    match r {
        Ok(()) => set_rval_bytes(&mut cx, &frame, &out),
        Err(e) => {
            report_error(&mut cx, &format!("Z_STREAM_ERROR: {e}"));
            false
        }
    }
}

/// `__wjs_zlib_brotli_decompress(dataU8)`。
pub unsafe extern "C" fn zlib_brotli_decompress(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(data) = arg_bytes(&mut cx, &frame, 0, "brotliDecompress") else {
        return false;
    };
    use std::io::Write as _;
    let mut out = Vec::new();
    let r = (|| -> std::io::Result<()> {
        let mut w = brotli::DecompressorWriter::new(&mut out, 4096);
        w.write_all(&data)?;
        w.flush()?;
        Ok(())
    })();
    match r {
        Ok(()) => set_rval_bytes(&mut cx, &frame, &out),
        Err(e) => {
            report_error(&mut cx, &format!("Z_DATA_ERROR: {e}"));
            false
        }
    }
}

/// `__wjs_zlib_zstd_compress(dataU8)`（恒 Fastest；见头注）。
pub unsafe extern "C" fn zlib_zstd_compress(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(data) = arg_bytes(&mut cx, &frame, 0, "zstdCompress") else {
        return false;
    };
    let out = ruzstd::encoding::compress_to_vec(
        &data[..],
        ruzstd::encoding::CompressionLevel::Fastest,
    );
    set_rval_bytes(&mut cx, &frame, &out)
}

/// `__wjs_zlib_zstd_decompress(dataU8)`。
pub unsafe extern "C" fn zlib_zstd_decompress(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(data) = arg_bytes(&mut cx, &frame, 0, "zstdDecompress") else {
        return false;
    };
    // `StreamingDecoder::new` 即验帧头（坏魔数此处报 Z_DATA_ERROR）。
    let dec = match ruzstd::decoding::StreamingDecoder::new(&data[..]) {
        Ok(d) => d,
        Err(e) => {
            report_error(&mut cx, &format!("Z_DATA_ERROR: {e}"));
            return false;
        }
    };
    match read_all(dec) {
        Ok(out) => set_rval_bytes(&mut cx, &frame, &out),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// 内嵌 ESM 源（`node:zlib`）。
pub const SOURCE: &str = r#"
function __zBytes(input, what) {
  if (typeof input === "string") return new TextEncoder().encode(input);
  if (input instanceof Uint8Array) return input;
  if (input instanceof ArrayBuffer) return new Uint8Array(input);
  if (ArrayBuffer.isView(input)) return new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
  throw new TypeError(`${what}: data must be string or BufferSource`);
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
  const err = new Error(`${code}: ${rest}`.trim() || code);
  err.code = code;
  err.errno = __Z_ERRNO[code] ?? -1;
  throw err;
}
function __zCall(fn) {
  try {
    return __zBuf(fn());
  } catch (e) {
    __zErr(e);
  }
}
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
function __zQuality(opts) {
  if (opts === undefined || opts === null) return 11;
  let q = opts.quality;
  if (q === undefined && opts.params !== undefined && opts.params !== null) {
    q = opts.params[1];
  }
  if (q === undefined) return 11;
  if (!Number.isInteger(q) || q < 0 || q > 11) {
    const err = new RangeError(`options.quality ${q} out of range (0..11)`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  return q;
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
  if (typeof cb !== "function") throw new TypeError(`${what}: callback must be a function`);
}
export function deflateSync(buf, opts) {
  const data = __zBytes(buf, "deflate");
  const lv = __zLevel(opts, -1);
  return __zCall(() => __wjs_zlib_deflate_lv(data, lv));
}
export function deflate(buf, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  __zNeedCb(cb, "deflate");
  const data = __zBytes(buf, "deflate");
  const lv = __zLevel(opts, -1);
  __zAsync((d, l) => __zCall(() => __wjs_zlib_deflate_lv(d, l)), [data, lv], cb);
}
export function inflateSync(buf) {
  const data = __zBytes(buf, "inflate");
  return __zCall(() => __wjs_zlib_inflate(data));
}
export function inflate(buf, cb) {
  __zNeedCb(cb, "inflate");
  const data = __zBytes(buf, "inflate");
  __zAsync((d) => __zCall(() => __wjs_zlib_inflate(d)), [data], cb);
}
export function deflateRawSync(buf, opts) {
  const data = __zBytes(buf, "deflateRaw");
  const lv = __zLevel(opts, -1);
  return __zCall(() => __wjs_zlib_deflate_raw(data, lv));
}
export function deflateRaw(buf, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  __zNeedCb(cb, "deflateRaw");
  const data = __zBytes(buf, "deflateRaw");
  const lv = __zLevel(opts, -1);
  __zAsync((d, l) => __zCall(() => __wjs_zlib_deflate_raw(d, l)), [data, lv], cb);
}
export function inflateRawSync(buf) {
  const data = __zBytes(buf, "inflateRaw");
  return __zCall(() => __wjs_zlib_inflate_raw(data));
}
export function inflateRaw(buf, cb) {
  __zNeedCb(cb, "inflateRaw");
  const data = __zBytes(buf, "inflateRaw");
  __zAsync((d) => __zCall(() => __wjs_zlib_inflate_raw(d)), [data], cb);
}
export function gzipSync(buf, opts) {
  const data = __zBytes(buf, "gzip");
  const lv = __zLevel(opts, -1);
  return __zCall(() => __wjs_zlib_gzip(data, lv));
}
export function gzip(buf, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  __zNeedCb(cb, "gzip");
  const data = __zBytes(buf, "gzip");
  const lv = __zLevel(opts, -1);
  __zAsync((d, l) => __zCall(() => __wjs_zlib_gzip(d, l)), [data, lv], cb);
}
export function gunzipSync(buf) {
  const data = __zBytes(buf, "gunzip");
  return __zCall(() => __wjs_zlib_gunzip(data));
}
export function gunzip(buf, cb) {
  __zNeedCb(cb, "gunzip");
  const data = __zBytes(buf, "gunzip");
  __zAsync((d) => __zCall(() => __wjs_zlib_gunzip(d)), [data], cb);
}
export function unzipSync(buf) {
  const data = __zBytes(buf, "unzip");
  return __zCall(() => __wjs_zlib_unzip(data));
}
export function unzip(buf, cb) {
  __zNeedCb(cb, "unzip");
  const data = __zBytes(buf, "unzip");
  __zAsync((d) => __zCall(() => __wjs_zlib_unzip(d)), [data], cb);
}
export function brotliCompressSync(buf, opts) {
  const data = __zBytes(buf, "brotliCompress");
  const q = __zQuality(opts);
  return __zCall(() => __wjs_zlib_brotli_compress(data, q));
}
export function brotliCompress(buf, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  __zNeedCb(cb, "brotliCompress");
  const data = __zBytes(buf, "brotliCompress");
  const q = __zQuality(opts);
  __zAsync((d, x) => __zCall(() => __wjs_zlib_brotli_compress(d, x)), [data, q], cb);
}
export function brotliDecompressSync(buf) {
  const data = __zBytes(buf, "brotliDecompress");
  return __zCall(() => __wjs_zlib_brotli_decompress(data));
}
export function brotliDecompress(buf, cb) {
  __zNeedCb(cb, "brotliDecompress");
  const data = __zBytes(buf, "brotliDecompress");
  __zAsync((d) => __zCall(() => __wjs_zlib_brotli_decompress(d)), [data], cb);
}
export function zstdCompressSync(buf) {
  const data = __zBytes(buf, "zstdCompress");
  return __zCall(() => __wjs_zlib_zstd_compress(data));
}
export function zstdCompress(buf, cb) {
  __zNeedCb(cb, "zstdCompress");
  const data = __zBytes(buf, "zstdCompress");
  __zAsync((d) => __zCall(() => __wjs_zlib_zstd_compress(d)), [data], cb);
}
export function zstdDecompressSync(buf) {
  const data = __zBytes(buf, "zstdDecompress");
  return __zCall(() => __wjs_zlib_zstd_decompress(data));
}
export function zstdDecompress(buf, cb) {
  __zNeedCb(cb, "zstdDecompress");
  const data = __zBytes(buf, "zstdDecompress");
  __zAsync((d) => __zCall(() => __wjs_zlib_zstd_decompress(d)), [data], cb);
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
  BROTLI_OPERATION_PROCESS: 0, BROTLI_OPERATION_FLUSH: 1,
  BROTLI_OPERATION_FINISH: 2, BROTLI_OPERATION_EMIT_METADATA: 3,
  BROTLI_PARAM_MODE: 0, BROTLI_PARAM_QUALITY: 1, BROTLI_PARAM_LGWIN: 2,
  BROTLI_PARAM_LGBLOCK: 3, BROTLI_PARAM_DISABLE_LITERAL_CONTEXT_MODELING: 4,
  BROTLI_PARAM_SIZE_HINT: 5, BROTLI_PARAM_LARGE_WINDOW: 6,
  BROTLI_MODE_GENERIC: 0, BROTLI_MODE_TEXT: 1, BROTLI_MODE_FONT: 2,
  BROTLI_DEFAULT_QUALITY: 11, BROTLI_MIN_QUALITY: 0, BROTLI_MAX_QUALITY: 11,
  BROTLI_DECODE: 0, BROTLI_ENCODE: 1,
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
  constants, codes,
};
// 顶层非 BROTLI 别名（Node 遗留口径，非枚举）。
for (const [k, v] of Object.entries(constants)) {
  if (!k.startsWith("BROTLI")) Object.defineProperty(__api, k, { value: v, enumerable: false });
}
export default __api;
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zlib_level_maps() {
        assert_eq!(zlib_level(-1), flate2::Compression::default());
        assert_eq!(zlib_level(0), flate2::Compression::none());
        assert_eq!(zlib_level(6), flate2::Compression::new(6));
        assert_eq!(zlib_level(9), flate2::Compression::best());
        // 越界由 JS 侧先验；此处钳制不断言错
        assert_eq!(zlib_level(99), flate2::Compression::best());
    }

    #[test]
    fn zlib_roundtrip_vectors() {
        use std::io::{Read as _, Write as _};
        let data = b"hello winterjs zlib probe 0123456789".repeat(8);
        // gzip
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(&data).unwrap();
        let gz = e.finish().unwrap();
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(&gz[..]).read_to_end(&mut out).unwrap();
        assert_eq!(out, data);
        // zlib 包裹
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(&data).unwrap();
        let zw = e.finish().unwrap();
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(&zw[..]).read_to_end(&mut out).unwrap();
        assert_eq!(out, data);
        // raw
        let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(&data).unwrap();
        let raw = e.finish().unwrap();
        let mut out = Vec::new();
        flate2::read::DeflateDecoder::new(&raw[..]).read_to_end(&mut out).unwrap();
        assert_eq!(out, data);
        // brotli（quality 1 提速，单测秒级）
        let mut cout = Vec::new();
        {
            let mut w = brotli::CompressorWriter::new(&mut cout, 4096, 1, 22);
            w.write_all(&data).unwrap();
            w.flush().unwrap();
        }
        let mut dout = Vec::new();
        {
            let mut w = brotli::DecompressorWriter::new(&mut dout, 4096);
            w.write_all(&cout).unwrap();
            w.flush().unwrap();
        }
        assert_eq!(dout, data);
        // zstd（Fastest 唯一已实现档）
        let zc = ruzstd::encoding::compress_to_vec(
            &data[..],
            ruzstd::encoding::CompressionLevel::Fastest,
        );
        let mut zd = Vec::new();
        ruzstd::decoding::StreamingDecoder::new(&zc[..])
            .unwrap()
            .read_to_end(&mut zd)
            .unwrap();
        assert_eq!(zd, data);
    }
}
