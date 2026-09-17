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
//! - Zip 实验面（上游 experimental）不做。
//! - 10a：`crc32` 落地（ISO-HDLC 自实现——`flate2::Crc` 不收 seed；同步纯函数）。
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

/// CRC-32/ISO-HDLC 表（const 生成；`flate2::Crc` 同算法但其 API 不收 seed，
/// 链式 `crc32(b, crc32(a))` 口径需自实现——真机值对拍钉住，见黑盒）。
const fn crc32_table_entry(i: u32) -> u32 {
    let mut crc = i;
    let mut k = 0;
    while k < 8 {
        crc = if crc & 1 == 1 {
            0xEDB8_8320 ^ (crc >> 1)
        } else {
            crc >> 1
        };
        k += 1;
    }
    crc
}
const fn crc32_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        t[i] = crc32_table_entry(i as u32);
        i += 1;
    }
    t
}
static CRC32_TABLE: [u32; 256] = crc32_table();

/// `__wjs_zlib_crc32(dataU8, seedU32)` → uint32（真机值对拍：
/// crc32("hello")=907060870，链式与空串口径同）。
/// JS 侧已验类型（ERR_INVALID_ARG_TYPE 原文），此处只做防御式取值。
pub unsafe extern "C" fn zlib_crc32(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(data) = arg_bytes(&mut cx, &frame, 0, "crc32") else {
        return false;
    };
    let seed = if frame.argc() > 1 && frame.arg(1).is_number() {
        frame.arg(1).to_number() as u32
    } else {
        0
    };
    // 链式：state = !seed（seed 为上一段终值时恰为其内部态）。
    let mut state = !seed;
    for &b in &data {
        state = CRC32_TABLE[((state ^ b as u32) & 0xFF) as usize] ^ (state >> 8);
    }
    frame.set_rval(mozjs::jsval::DoubleValue((!state) as f64));
    true
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
    match gunzip_multi(&data) {
        Ok(out) => set_rval_bytes(&mut cx, &frame, &out),
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// gzip 多成员连解（Node gunzip 多成员口径：flate2 `MultiGzDecoder` 连解；
/// 手写成员循环不可行——`GzDecoder` 内缓冲预读，`get_ref` 拿不到成员边界）。
fn gunzip_multi(data: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    let mut out = Vec::new();
    flate2::read::MultiGzDecoder::new(data)
        .read_to_end(&mut out)
        .map_err(|e| format!("Z_DATA_ERROR: {e}"))?;
    Ok(out)
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
    if let Ok(out) = gunzip_multi(&data) {
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
        // 禁显式 flush：flush 会先吐一个非终结同步块（空输入多 2 字节 framing，
        // 非空头尾亦与 one-shot 不一致）；drop 时的 FINISH 即完整终结，
        // 与 Node one-shot 逐字节一致（zero-byte 套件：空输入 1 字节）。
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
import errors from 'node:internal/errors';
import { kMaxLength as __bufKMaxLength } from 'node:buffer';

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
  const err = new Error(`${code}: ${rest}`.trim() || code);
  err.code = code;
  err.errno = __Z_ERRNO[code] ?? -1;
  throw err;
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
  const lv = __zLevel(opts, -1);
  __zFlush(opts, false);
  return __zCall(() => __wjs_zlib_deflate_lv(data, lv));
}
export function deflate(buf, opts, cb) {
  if (opts && opts.info && typeof cb === "function") { const C = Deflate; const eng = new C(opts); try { const r = deflateSync(buf, opts); cb(null, { buffer: r.buffer ?? r, engine: eng }); } catch (e) { cb(e); } return; }
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  __zNeedCb(cb, "deflate");
  const data = __zChecked(buf, "deflate");
  const lv = __zLevel(opts, -1);
  __zAsync((d, l) => __zCall(() => __wjs_zlib_deflate_lv(d, l)), [data, lv], cb);
}
function inflateSync__core(buf, opts) {
  const data = __zChecked(buf, "inflate");
  return __zCheckKMax(__zCall(() => __wjs_zlib_inflate(data)));
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
  return __wjs_zlib_crc32(bytes, value >>> 0);
}
export function inflate(buf, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  if (opts && opts.info) { const eng = new Inflate(opts); try { const r = inflateSync(buf, opts); cb(null, { buffer: r.buffer ?? r, engine: eng }); } catch (e) { cb(e); } return; }
  __zNeedCb(cb, "inflate");
  const data = __zChecked(buf, "inflate");
  __zAsync((d) => __zCheckKMax(__zCall(() => __wjs_zlib_inflate(d))), [data], cb);
}
function deflateRawSync__core(buf, opts) {
  const data = __zChecked(buf, "deflateRaw");
  const lv = __zLevel(opts, -1);
  __zFlush(opts, false);
  return __zCall(() => __wjs_zlib_deflate_raw(data, lv));
}
export function deflateRaw(buf, opts, cb) {
  if (opts && opts.info && typeof cb === "function") { const C = DeflateRaw; const eng = new C(opts); try { const r = deflateRawSync(buf, opts); cb(null, { buffer: r.buffer ?? r, engine: eng }); } catch (e) { cb(e); } return; }
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  __zNeedCb(cb, "deflateRaw");
  const data = __zChecked(buf, "deflateRaw");
  const lv = __zLevel(opts, -1);
  __zAsync((d, l) => __zCall(() => __wjs_zlib_deflate_raw(d, l)), [data, lv], cb);
}
function inflateRawSync__core(buf, opts) {
  const data = __zChecked(buf, "inflateRaw");
  return __zCheckKMax(__zCall(() => __wjs_zlib_inflate_raw(data)));
}
export function inflateRaw(buf, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  if (opts && opts.info) { const eng = new InflateRaw(opts); try { const r = inflateRawSync(buf, opts); cb(null, { buffer: r.buffer ?? r, engine: eng }); } catch (e) { cb(e); } return; }
  __zNeedCb(cb, "inflateRaw");
  const data = __zChecked(buf, "inflateRaw");
  __zAsync((d) => __zCheckKMax(__zCall(() => __wjs_zlib_inflate_raw(d))), [data], cb);
}
function gzipSync__core(buf, opts) {
  const data = __zChecked(buf, "gzip");
  const lv = __zLevel(opts, -1);
  __zFlush(opts, false);
  return __zCall(() => __wjs_zlib_gzip(data, lv));
}
export function gzip(buf, opts, cb) {
  if (opts && opts.info && typeof cb === "function") { const C = Gzip; const eng = new C(opts); try { const r = gzipSync(buf, opts); cb(null, { buffer: r.buffer ?? r, engine: eng }); } catch (e) { cb(e); } return; }
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  __zNeedCb(cb, "gzip");
  const data = __zChecked(buf, "gzip");
  const lv = __zLevel(opts, -1);
  __zAsync((d, l) => __zCall(() => __wjs_zlib_gzip(d, l)), [data, lv], cb);
}
function gunzipSync__core(buf, opts) {
  const data = __zChecked(buf, "gunzip");
  return __zCheckKMax(__zCall(() => __wjs_zlib_gunzip(data)));
}
export function gunzip(buf, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  if (opts && opts.info) { const eng = new Gunzip(opts); try { const r = gunzipSync(buf, opts); cb(null, { buffer: r.buffer ?? r, engine: eng }); } catch (e) { cb(e); } return; }
  __zNeedCb(cb, "gunzip");
  const data = __zChecked(buf, "gunzip");
  __zAsync((d) => __zCheckKMax(__zCall(() => __wjs_zlib_gunzip(d))), [data], cb);
}
function unzipSync__core(buf, opts) {
  const data = __zChecked(buf, "unzip");
  return __zCheckKMax(__zCall(() => __wjs_zlib_unzip(data)));
}
export function unzip(buf, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  if (opts && opts.info) { const eng = new Unzip(opts); try { const r = unzipSync(buf, opts); cb(null, { buffer: r.buffer ?? r, engine: eng }); } catch (e) { cb(e); } return; }
  __zNeedCb(cb, "unzip");
  const data = __zChecked(buf, "unzip");
  __zAsync((d) => __zCheckKMax(__zCall(() => __wjs_zlib_unzip(d))), [data], cb);
}
function brotliCompressSync__core(buf, opts) {
  const data = __zChecked(buf, "brotliCompress");
  const q = __zQuality(opts);
  __zFlush(opts, true);
  return __zCall(() => __wjs_zlib_brotli_compress(data, q));
}
export function brotliCompress(buf, opts, cb) {
  if (opts && opts.info && typeof cb === "function") { const C = BrotliCompress; const eng = new C(opts); try { const r = brotliCompressSync(buf, opts); cb(null, { buffer: r.buffer ?? r, engine: eng }); } catch (e) { cb(e); } return; }
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  __zNeedCb(cb, "brotliCompress");
  const data = __zChecked(buf, "brotliCompress");
  const q = __zQuality(opts);
  __zFlush(opts, true);
  __zAsync((d, x) => __zCall(() => __wjs_zlib_brotli_compress(d, x)), [data, q], cb);
}
function brotliDecompressSync__core(buf, opts) {
  const data = __zChecked(buf, "brotliDecompress");
  const maxOut = __zMaxOut(opts);
  const out = __zCall(() => __wjs_zlib_brotli_decompress(data));
  if (maxOut !== undefined && out.length > maxOut) throw new ERR_BUFFER_TOO_LARGE(maxOut);
  return __zCheckKMax(out);
}
export function brotliDecompress(buf, opts, cb) {
  if (opts && opts.info && typeof cb === "function") { const C = BrotliDecompress; const eng = new C(opts); try { const r = brotliDecompressSync(buf, opts); cb(null, { buffer: r.buffer ?? r, engine: eng }); } catch (e) { cb(e); } return; }
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  __zNeedCb(cb, "brotliDecompress");
  const data = __zChecked(buf, "brotliDecompress");
  const maxOut = __zMaxOut(opts);
  __zAsync((d, m) => {
    const out = __zCall(() => __wjs_zlib_brotli_decompress(d));
    if (m !== undefined && out.length > m) throw new ERR_BUFFER_TOO_LARGE(m);
    return __zCheckKMax(out);
  }, [data, maxOut], cb);
}
function zstdCompressSync__core(buf, opts) {
  const data = __zChecked(buf, "zstdCompress");
  return __zCall(() => __wjs_zlib_zstd_compress(data));
}
export function zstdCompress(buf, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  if (opts && opts.info) { const eng = new ZstdCompress(opts); try { const r = zstdCompressSync(buf, opts); cb(null, { buffer: r.buffer ?? r, engine: eng }); } catch (e) { cb(e); } return; }
  __zNeedCb(cb, "zstdCompress");
  const data = __zChecked(buf, "zstdCompress");
  __zAsync((d) => __zCall(() => __wjs_zlib_zstd_compress(d)), [data], cb);
}
function zstdDecompressSync__core(buf, opts) {
  const data = __zChecked(buf, "zstdDecompress");
  return __zCheckKMax(__zCall(() => __wjs_zlib_zstd_decompress(data)));
}
export function zstdDecompress(buf, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  if (opts && opts.info) { const eng = new ZstdDecompress(opts); try { const r = zstdDecompressSync(buf, opts); cb(null, { buffer: r.buffer ?? r, engine: eng }); } catch (e) { cb(e); } return; }
  __zNeedCb(cb, "zstdDecompress");
  const data = __zChecked(buf, "zstdDecompress");
  __zAsync((d) => __zCheckKMax(__zCall(() => __wjs_zlib_zstd_decompress(d))), [data], cb);
}
// 流式类（Node ZlibBase 口径的最小实现：Transform 子类，累积 input，
// _flush 时调同名 Sync 版一次产出；同步底层记档沿用 §7 头注）。
// 覆盖 12 类 + createXxx 工厂 + info 选项（{buffer, engine}）+ bytesWritten。
// 偏差记档：flush()/write 分段增量不做（整收）；dictionary/windowBits/memLevel/
// chunkSize/strategy 接受忽略（构造校验只做 failed-init 套件口径：chunkSize 范围）。
import { Transform } from "node:stream";
function __zStreamBase(opts, syncFn) {
  Transform.call(this);
  this.__chunks = [];
  this.__syncFn = syncFn;
  this.__opts = opts ?? {};
  this.bytesWritten = 0;
  // node Zlib 口径（destroy/close-after-error/reset-during-write 套件点名）：
  // _handle 开流非空、close/destroy 后置空；_closed 同步；_chunkSize/_outOffset
  // 内部计数（_processChunk 越界门）；__writeActive 标记分发中的写
  // （reset-during-write 套件：同 tick 内 reset 即抛）。
  const self = this;
  this._handle = {
    reset: () => {
      if (self.__writeActive) throw new Error("Cannot reset zlib stream while a write is in progress");
      self.__chunks = [];
    },
  };
  this._closed = false;
  this._chunkSize = (opts && Number.isInteger(opts.chunkSize)) ? opts.chunkSize : 16384;
  this._outOffset = 0;
  this.__writeActive = false;
}
Object.setPrototypeOf(__zStreamBase.prototype, Transform.prototype);
Object.setPrototypeOf(__zStreamBase, Transform);
__zStreamBase.prototype._transform = function (chunk, encoding, cb) {
  let u8;
  if (typeof chunk === "string") u8 = Buffer.from(chunk);
  else if (chunk instanceof ArrayBuffer) u8 = new Uint8Array(chunk);
  else if (ArrayBuffer.isView(chunk)) u8 = new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength);
  else u8 = chunk;
  this.__chunks.push(u8);
  this.bytesWritten += u8.length ?? 0;
  // 写分发标记（reset-during-write 套件）：microtask 清零——同 tick 内 reset 可见，
  // 下 tick 已落定不再抛（与真机"分发中"窗口对等）。
  this.__writeActive = true;
  queueMicrotask(() => { this.__writeActive = false; });
  cb();
};
__zStreamBase.prototype._flush = function (cb) {
  try {
    const out = this.__syncFn(Buffer.concat(this.__chunks), this.__opts);
    // Sync 返回 Buffer 本体（别取 .buffer——下游 write(ArrayBuffer) 会被 Writable 拒收）。
    this.push(out);
    cb();
  } catch (e) { cb(e); }
};
// flush(kind?, cb)：整收近似——把当前累积经 Sync 压出并 push（真增量语义偏离记档）。
// kind 逐族校验（flush-invalid-kind 套件，真机口径）：undefined/NaN/函数直通；
// 非 number → ARG_TYPE；zlib 族 {0,2,4} / brotli {0,1,2,3} / zstd {0,1,2} 之外 → OUT_OF_RANGE。
// close(cb)：置 _closed/空柄 + end（已销毁则只等 close）；cb 落 'close'。
// reset()：经 _handle.reset（分发中即抛，同上）。
// params(level, strategy)：校验并存回 _level/_strategy（deflate-constructors 套件口径）。
__zStreamBase.prototype.flush = function (kind, cb) {
  if (typeof kind === "function") { cb = kind; kind = undefined; }
  if (kind !== undefined && !(typeof kind === "number" && Number.isNaN(kind))) {
    if (typeof kind !== "number") {
      throw new ERR_INVALID_ARG_TYPE("flush", "number", kind);
    }
    const nm = this.__engineName || "";
    // 真机集（flush-invalid-kind 套件逐字）：zlib {Z_NO_FLUSH,Z_FINISH,Z_BLOCK}={0,4,5}，
    // brotli {PROCESS,FLUSH,FINISH,EMIT_METADATA}={0,1,2,3}，zstd {continue,flush,end}={0,1,2}。
    const valid = /brotli/i.test(nm) ? [0, 1, 2, 3] : (/zstd/i.test(nm) ? [0, 1, 2] : [0, 4, 5]);
    if (!valid.includes(kind)) {
      const err = new RangeError(`The value of "flush" is out of range. It must be one of ${valid.join(", ")}. Received ${kind}`);
      err.code = "ERR_OUT_OF_RANGE";
      throw err;
    }
  }
  try {
    if (this.__chunks.length > 0) {
      const out = this.__syncFn(Buffer.concat(this.__chunks), this.__opts);
      this.__chunks = [];
      this.push(out);
    }
    if (typeof cb === "function") cb();
  } catch (e) {
    if (typeof cb === "function") cb(e);
    else throw e;
  }
};
__zStreamBase.prototype._destroy = function (err, cb) {
  // 收尾旗与柄同 _destroy 点置位（与 Node 同步点一致：destroy() 后同步可见）。
  this._closed = true;
  this._handle = null;
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
// 真增量不做（整收 Sync 一次产出，flag 仅收不释）；_outOffset 越界即 RangeError。
__zStreamBase.prototype._processChunk = function (chunk, flag) {
  if (this._outOffset > this._chunkSize) {
    const err = new RangeError(`The value of "_outOffset" is out of range. It must be <= ${this._chunkSize}. Received ${this._outOffset}`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  return this.__syncFn(__zChecked(chunk), this.__opts);
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
function __zMakeClass(syncFn, check) {
  function C(opts) {
    // Node 口径：流类裸调返回新实例（DEP0184 deprecate 警告略）。
    if (!(this instanceof C)) return new C(opts);
    if (check) check(opts);
    __zStreamBase.call(this, opts, syncFn);
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
export const Deflate = __zMakeClass(deflateSync, (o) => __zCheckZlibOpts(o, 8, false));
export const Inflate = __zMakeClass(inflateSync, (o) => __zCheckZlibOpts(o, 8, true));
export const Gzip = __zMakeClass(gzipSync, (o) => __zCheckZlibOpts(o, 9, false));
export const Gunzip = __zMakeClass(gunzipSync, (o) => __zCheckZlibOpts(o, 8, true));
export const DeflateRaw = __zMakeClass(deflateRawSync, (o) => __zCheckZlibOpts(o, 8, false));
export const InflateRaw = __zMakeClass(inflateRawSync, (o) => __zCheckZlibOpts(o, 8, true));
export const Unzip = __zMakeClass(unzipSync, (o) => __zCheckZlibOpts(o, 8, true));
export const BrotliCompress = __zMakeClass(brotliCompressSync, (o) => { __zCheckChunkSize(o); __zCheckBrotliParams(o); });
export const BrotliDecompress = __zMakeClass(brotliDecompressSync, (o) => { __zCheckChunkSize(o); __zCheckBrotliParams(o); });
export const ZstdCompress = __zMakeClass(zstdCompressSync, __zCheckChunkSize);
export const ZstdDecompress = __zMakeClass(zstdDecompressSync, __zCheckChunkSize);
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
  BROTLI_DECODE: 0, BROTLI_ENCODE: 1,
  ZSTD_e_continue: 0, ZSTD_e_flush: 1, ZSTD_e_end: 2,
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
Object.defineProperty(__api, "codes", { writable: false });
// 顶层非 BROTLI 别名（Node 遗留口径，非枚举）。
for (const [k, v] of Object.entries(constants)) {
  if (!k.startsWith("BROTLI")) Object.defineProperty(__api, k, { value: v, enumerable: false });
}
Object.freeze(constants);
Object.freeze(codes);
export default __api;
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zlib_gunzip_multi_members() {
        use std::io::Write as _;
        let enc = |s: &[u8]| {
            let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(s).unwrap();
            e.finish().unwrap()
        };
        let data = [enc(b"abc"), enc(b"def")].concat();
        assert_eq!(gunzip_multi(&data).unwrap(), b"abcdef");
        assert_eq!(gunzip_multi(&enc(b"abc")).unwrap(), b"abc");
        // 空输入真机抛错（gunzipSync(Buffer.alloc(0)) → Z_BUF_ERROR unexpected EOF），
        // 不解出空串——7cb037a 入库时断言编码错（未跑全量）。
        assert!(gunzip_multi(&[]).is_err());
        assert!(gunzip_multi(b"garbage").is_err());
    }

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
