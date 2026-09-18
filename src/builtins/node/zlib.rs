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

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::jsval::{JSVal, UndefinedValue};
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

// ===== 增量流引擎（10f-g：真流式编解码状态机，零新依赖）=====
// flate2 Compress/Decompress 分段 + 手工 gzip 成员机（trailing zeros/garbage/
// magic 语义逐条真机对拍）+ brotli crate 状态机（enc compress_stream / dec
// BrotliDecompressStream，字典双向）+ ruzstd 单帧一次性（能力记档）。
// 注册表 thread_local（JS 会话线程一份；JS 侧 close/destroy/end 必须 free；
// 线程生灭即回收，§4.24 隔离边界一致）。
use std::io::Read as _;
use brotli::SliceWrapperMut as _;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ZKind {
    ZlibDeflate = 0,
    RawDeflate = 1,
    GzipDeflate = 2,
    ZlibInflate = 3,
    RawInflate = 4,
    GzipInflate = 5,        // 多成员（Gunzip/Unzip）
    GzipInflateSingle = 6,  // 单成员（Web DecompressionStream 'gzip'）
    AutoInflate = 7,        // Unzip：1f8b → gzip 否则 zlib
    BrotliEnc = 8,
    BrotliDec = 9,
    ZstdEnc = 10,
    ZstdDec = 11,
}

/// 引擎错误（code = node 侧 err.code；msg = err.message）。
#[derive(Debug)]
struct ZErr {
    code: String,
    msg: String,
}
impl ZErr {
    fn new(code: &str, msg: &str) -> ZErr {
        ZErr { code: code.to_string(), msg: msg.to_string() }
    }
    fn data(msg: &str) -> ZErr {
        ZErr::new("Z_DATA_ERROR", msg)
    }
    fn buf(msg: &str) -> ZErr {
        // 真机口径：截断/空输入 = Z_BUF_ERROR "unexpected end of file"
        ZErr::new("Z_BUF_ERROR", msg)
    }
    fn junk() -> ZErr {
        ZErr::new(
            "ERR_TRAILING_JUNK_AFTER_STREAM_END",
            "Trailing junk found after the end of the compressed stream",
        )
    }
}

/// gzip 成员状态机。
struct GzMember {
    phase: u8, // 0=header 1=body 2=trailer 3=between
    hdr: Vec<u8>,
    zd: Option<flate2::Decompress>,
    crc: u32, // 成员输出 CRC 运行态（!-form 起点 0xFFFF_FFFF）
    outlen: u32,
    trailer: Vec<u8>,
}

/// brotli 编码器。
struct BrotliEnc {
    st: brotli::enc::encode::BrotliEncoderStateStruct<brotli::enc::StandardAlloc>,
}
/// brotli 解码器。
struct BrotliDec {
    st: brotli_decompressor::BrotliState<
        brotli::enc::StandardAlloc,
        brotli::enc::StandardAlloc,
        brotli::enc::StandardAlloc,
    >,
    done_flag: bool,
}

/// zstd 编码（ruzstd 只有一帧一次性：flush 边界分段出帧——偏离记档）。
struct ZstdEnc {
    buf: Vec<u8>,
    flushed: usize,
    pledged: Option<u64>,
    total_in: u64,
}
/// zstd 解码（只累积，flush/end 时对已收前缀一次性解帧）。
struct ZstdDec {
    carried: Vec<u8>,
    pos: usize,
}

struct ZEngine {
    kind: ZKind,
    level: i32,
    dict: Vec<u8>,
    zc: Option<flate2::Compress>,
    zd: Option<flate2::Decompress>,
    // gzip 编码
    gz_hdr_written: bool,
    gz_crc: u32,
    gz_isize: u32,
    // inflate 通用
    pending: Vec<u8>,
    pos: usize,
    hdr_checked: bool,
    first_byte: Option<u8>,
    // gzip 成员机
    gz: Option<GzMember>,
    // brotli
    be: Option<BrotliEnc>,
    bd: Option<BrotliDec>,
    // zstd
    ze: Option<ZstdEnc>,
    zd2: Option<ZstdDec>,
    reject: bool,
    done: bool,
    out: Vec<u8>,
}

impl ZEngine {
    fn new(kind: ZKind, level: i32, dict: &[u8], pledged: Option<u64>, reject: bool) -> ZEngine {
        let mut e = ZEngine {
            kind,
            level,
            dict: dict.to_vec(),
            zc: None,
            zd: None,
            gz_hdr_written: false,
            gz_crc: 0xFFFF_FFFF,
            gz_isize: 0,
            pending: Vec::new(),
            pos: 0,
            hdr_checked: false,
            first_byte: None,
            gz: None,
            be: None,
            bd: None,
            ze: None,
            zd2: None,
            reject,
            done: false,
            out: Vec::new(),
        };
        match kind {
            ZKind::BrotliEnc => {
                let mut st = brotli::enc::encode::BrotliEncoderStateStruct::new(
                    brotli::enc::StandardAlloc::default(),
                );
                st.set_parameter(
                    brotli::enc::encode::BrotliEncoderParameter::BROTLI_PARAM_QUALITY,
                    level.clamp(0, 11) as u32,
                );
                st.set_parameter(
                    brotli::enc::encode::BrotliEncoderParameter::BROTLI_PARAM_LGWIN,
                    22,
                );
                if !dict.is_empty() {
                    st.set_custom_dictionary(dict.len(), dict);
                }
                e.be = Some(BrotliEnc { st });
            }
            ZKind::BrotliDec => {
                let a8 = brotli::enc::StandardAlloc::default();
                let a32 = brotli::enc::StandardAlloc::default();
                let ahc = brotli::enc::StandardAlloc::default();
                let st = if dict.is_empty() {
                    brotli::BrotliState::new(a8, a32, ahc)
                } else {
                    let mut alloc = brotli::enc::StandardAlloc::default();
                    let mut mem =
                        <brotli::enc::StandardAlloc as brotli::Allocator<u8>>::alloc_cell(
                            &mut alloc,
                            dict.len(),
                        );
                    mem.slice_mut().copy_from_slice(dict);
                    brotli::BrotliState::new_with_custom_dictionary(a8, a32, ahc, mem)
                };
                e.bd = Some(BrotliDec { st, done_flag: false });
            }
            ZKind::ZstdEnc => {
                e.ze = Some(ZstdEnc { buf: Vec::new(), flushed: 0, pledged, total_in: 0 });
            }
            ZKind::ZstdDec => {
                e.zd2 = Some(ZstdDec { carried: Vec::new(), pos: 0 });
            }
            _ => {}
        }
        e
    }

    fn unconsumed(&self) -> usize {
        match self.kind {
            ZKind::ZstdDec => {
                self.zd2.as_ref().map(|z| z.carried.len() - z.pos).unwrap_or(0)
            }
            _ => self.pending.len() - self.pos,
        }
    }

    /// 压缩侧喂入（flag：node 侧族内 flush kind）。
    fn deflate_feed(&mut self, input: &[u8], flag: u32) -> Result<(), ZErr> {
        if self.done {
            return Ok(()); // 终结后静默吞掉（真机：write-after-end 回调照常触发）
        }
        let flush = match flag {
            1 => flate2::FlushCompress::Partial,
            2 => flate2::FlushCompress::Sync,
            3 | 5 => flate2::FlushCompress::Full, // Z_BLOCK≈Full（miniz 无 Block，记档）
            4 => flate2::FlushCompress::Finish,
            _ => flate2::FlushCompress::None,
        };
        let gzip = self.kind == ZKind::GzipDeflate;
        if gzip && !self.gz_hdr_written {
            self.out.extend_from_slice(&gzip_header(self.level));
            self.gz_hdr_written = true;
        }
        let zlib_wrap = self.kind == ZKind::ZlibDeflate;
        let zc = self
            .zc
            .get_or_insert_with(|| flate2::Compress::new(zlib_level(self.level), zlib_wrap));
        let mut src = input;
        loop {
            let tin = zc.total_in();
            let ob = self.out.len();
            let status = zc
                .compress_vec(src, &mut self.out, flush)
                .map_err(|e| ZErr::new("Z_STREAM_ERROR", &e.to_string()))?;
            let consumed = (zc.total_in() - tin) as usize;
            if gzip {
                for &b in &self.out[ob..] {
                    self.gz_crc =
                        CRC32_TABLE[((self.gz_crc ^ b as u32) & 0xFF) as usize] ^ (self.gz_crc >> 8);
                }
                self.gz_isize = self.gz_isize.wrapping_add(consumed as u32);
            }
            src = &src[consumed..];
            match status {
                flate2::Status::StreamEnd => {
                    self.done = true;
                    break;
                }
                _ => {
                    if src.is_empty() {
                        break;
                    }
                }
            }
        }
        if gzip && self.done {
            let crc = !self.gz_crc;
            self.out.extend_from_slice(&crc.to_le_bytes());
            self.out.extend_from_slice(&self.gz_isize.to_le_bytes());
        }
        Ok(())
    }

    /// 解压侧喂入（先入 pending，再按族泵出）。
    fn inflate_feed(&mut self, input: &[u8], flag: u32) -> Result<(), ZErr> {
        if self.kind == ZKind::ZstdDec {
            let z = self.zd2.as_mut().unwrap();
            z.carried.extend_from_slice(input);
            if flag == 1 || flag == 2 {
                return self.zstd_decode_pump(flag == 2);
            }
            return Ok(());
        }
        self.pending.extend_from_slice(input);
        if self.first_byte.is_none() && self.pending.len() > self.pos {
            self.first_byte = Some(self.pending[self.pos]);
        }
        match self.kind {
            ZKind::ZlibInflate | ZKind::RawInflate => self.plain_inflate_pump(flag),
            ZKind::GzipInflate | ZKind::GzipInflateSingle | ZKind::AutoInflate => {
                self.gz_inflate_pump(flag)
            }
            _ => Ok(()),
        }
    }

    /// zlib/raw 解压泵。
    fn plain_inflate_pump(&mut self, flag: u32) -> Result<(), ZErr> {
        if self.done {
            return Ok(());
        }
        let zlib = self.kind == ZKind::ZlibInflate;
        let finish = flag == 4;
        let tolerate = flag == 2; // finishFlush=Z_SYNC_FLUSH：截断容忍（truncated 套件）
        if zlib && !self.hdr_checked {
            let avail = self.pending.len() - self.pos;
            if avail < 2 {
                if finish {
                    if tolerate {
                        self.done = true;
                        return Ok(());
                    }
                    return Err(ZErr::buf("unexpected end of file"));
                }
                return Ok(());
            }
            let b0 = self.pending[self.pos];
            let b1 = self.pending[self.pos + 1];
            let cm = b0 & 0x0f;
            let cinfo = b0 >> 4;
            if cm != 8 || cinfo > 7 || ((b0 as u16) * 256 + b1 as u16) % 31 != 0 {
                return Err(ZErr::data("incorrect header check"));
            }
            self.hdr_checked = true;
        }
        if !self.hdr_checked && !zlib {
            self.hdr_checked = true;
        }
        let zd = self.zd.get_or_insert_with(|| flate2::Decompress::new(zlib));
        let mut err: Option<flate2::DecompressError> = None;
        let mut ended = false;
        loop {
            let src = &self.pending[self.pos..];
            let tin = zd.total_in();
            let flush =
                if finish { flate2::FlushDecompress::Finish } else { flate2::FlushDecompress::None };
            match zd.decompress_vec(src, &mut self.out, flush) {
                Ok(status) => {
                    self.pos += (zd.total_in() - tin) as usize;
                    match status {
                        flate2::Status::StreamEnd => {
                            ended = true;
                            break;
                        }
                        _ => {
                            if self.pos >= self.pending.len() {
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    self.pos += (zd.total_in() - tin) as usize;
                    err = Some(e);
                    break;
                }
            }
        }
        if let Some(e) = err {
            return Err(self.classify_inflate_err(&e));
        }
        if ended {
            self.done = true;
            if self.reject && self.unconsumed() > 0 {
                return Err(ZErr::junk());
            }
        } else if finish {
            if tolerate {
                self.done = true; // 部分输出即收尾（真机 finishFlush 口径）
            } else {
                return Err(ZErr::buf("unexpected end of file"));
            }
        }
        self.compact();
        Ok(())
    }

    fn classify_inflate_err(&self, e: &flate2::DecompressError) -> ZErr {
        // 首块 BTYPE==3（保留块型）→ 真机 "invalid block type"（raw garbage 口径）
        if let Some(b0) = self.first_byte {
            if (b0 >> 1) & 3 == 3 {
                return ZErr::data("invalid block type");
            }
        }
        ZErr::data(&e.to_string())
    }

    /// gzip 多成员/单成员解压泵（trailing zeros/garbage/magic 语义真机对拍：
    /// 全零尾 → 忽略；乱码 → "incorrect header check"；假 gzip 头 →
    /// "unknown compression method"；截断 → Z_BUF_ERROR "unexpected end of file"；
    /// rejectGarbageAfterEnd → ERR_TRAILING_JUNK_AFTER_STREAM_END）。
    fn gz_inflate_pump(&mut self, flag: u32) -> Result<(), ZErr> {
        let finish = flag == 4;
        let tolerate = flag == 2;
        let single = self.kind == ZKind::GzipInflateSingle;
        if self.gz.is_none() {
            self.gz = Some(GzMember {
                phase: 0,
                hdr: Vec::new(),
                zd: None,
                crc: 0xFFFF_FFFF,
                outlen: 0,
                trailer: Vec::new(),
            });
        }
        // Auto 分流：1f8b → gzip；否则 zlib（一次性决定；真机 unzipSync(garbage) → zlib 路径）
        if self.kind == ZKind::AutoInflate {
            let avail = self.pending.len() - self.pos;
            if avail == 0 {
                if finish {
                    if tolerate {
                        self.done = true;
                        return Ok(());
                    }
                    return Err(ZErr::buf("unexpected end of file"));
                }
                return Ok(());
            }
            let is_gz = (avail >= 2
                && self.pending[self.pos] == 0x1f
                && self.pending[self.pos + 1] == 0x8b)
                || (avail == 1 && self.pending[self.pos] == 0x1f);
            if !is_gz {
                self.kind = ZKind::ZlibInflate;
                return self.plain_inflate_pump(flag);
            }
            self.kind = ZKind::GzipInflate;
        }
        let trunc = |tolerate: bool, s: &mut Self| -> Result<(), ZErr> {
            if tolerate {
                s.done = true;
                return Ok(());
            }
            Err(ZErr::buf("unexpected end of file"))
        };
        loop {
            let phase = self.gz.as_ref().unwrap().phase;
            match phase {
                0 => {
                    // header：固定 10B + 可选段（FEXTRA/FNAME/FCOMMENT/FHCRC）
                    let mut need: usize = 10;
                    let flg0;
                    {
                        let hdr = &self.gz.as_ref().unwrap().hdr;
                        flg0 = if hdr.len() >= 4 { Some(hdr[3]) } else { None };
                    }
                    if let Some(flg) = flg0 {
                        if flg & 4 != 0 {
                            // FEXTRA
                            if self.gz.as_ref().unwrap().hdr.len() >= 12 {
                                let h = &self.gz.as_ref().unwrap().hdr;
                                need = 12 + u16::from_le_bytes([h[10], h[11]]) as usize;
                            } else {
                                need = 12;
                            }
                        }
                    }
                    // 收集到 need 字节（除 FNAME/FCOMMENT/FHCRC 后段）
                    while self.gz.as_ref().unwrap().hdr.len() < need && self.pos < self.pending.len() {
                        let b = self.pending[self.pos];
                        self.pos += 1;
                        self.gz.as_mut().unwrap().hdr.push(b);
                    }
                    let hdr = &self.gz.as_ref().unwrap().hdr;
                    if hdr.len() < 2 {
                        if finish {
                            return trunc(tolerate, self);
                        }
                        return Ok(());
                    }
                    if hdr[0] != 0x1f || hdr[1] != 0x8b {
                        return Err(ZErr::data("incorrect header check"));
                    }
                    if hdr.len() < 3 {
                        if finish {
                            return trunc(tolerate, self);
                        }
                        return Ok(());
                    }
                    if hdr[2] != 8 {
                        return Err(ZErr::data("unknown compression method"));
                    }
                    if hdr.len() < 10 {
                        if finish {
                            return trunc(tolerate, self);
                        }
                        return Ok(());
                    }
                    let flg = hdr[3];
                    let extra = flg & 4 != 0;
                    if extra && hdr.len() < 12 {
                        if finish {
                            return trunc(tolerate, self);
                        }
                        return Ok(());
                    }
                    if extra && hdr.len() < 12 + u16::from_le_bytes([hdr[10], hdr[11]]) as usize {
                        if finish {
                            return trunc(tolerate, self);
                        }
                        return Ok(());
                    }
                    // FNAME(8)/FCOMMENT(16)：收到 NUL 为止
                    for sec in [(flg & 8 != 0), (flg & 16 != 0)] {
                        if !sec {
                            continue;
                        }
                        loop {
                            if self.pos >= self.pending.len() {
                                if finish {
                                    return trunc(tolerate, self);
                                }
                                return Ok(());
                            }
                            let b = self.pending[self.pos];
                            self.pos += 1;
                            self.gz.as_mut().unwrap().hdr.push(b);
                            if b == 0 {
                                break;
                            }
                        }
                    }
                    // FHCRC(2)
                    if flg & 2 != 0 {
                        let mut got = 0;
                        while got < 2 {
                            if self.pos >= self.pending.len() {
                                if finish {
                                    return trunc(tolerate, self);
                                }
                                return Ok(());
                            }
                            let b = self.pending[self.pos];
                            self.pos += 1;
                            self.gz.as_mut().unwrap().hdr.push(b);
                            got += 1;
                        }
                        // FHCRC 校验：crc16 over header（真机未测，记档不校验）
                    }
                    let m = self.gz.as_mut().unwrap();
                    m.phase = 1;
                    m.zd = Some(flate2::Decompress::new(false));
                }
                1 => {
                    // body：raw inflate
                    let mut ended = false;
                    let mut err: Option<flate2::DecompressError> = None;
                    {
                        let m = self.gz.as_mut().unwrap();
                        let zd = m.zd.as_mut().unwrap();
                        let mut produced: Vec<Vec<u8>> = Vec::new();
                        loop {
                            let src = &self.pending[self.pos..];
                            let tin = zd.total_in();
                            let mut ob: Vec<u8> = Vec::new();
                            match zd.decompress_vec(src, &mut ob, flate2::FlushDecompress::None)
                            {
                                Ok(status) => {
                                    let consumed = (zd.total_in() - tin) as usize;
                                    self.pos += consumed;
                                    if !ob.is_empty() {
                                        produced.push(ob);
                                    }
                                    match status {
                                        flate2::Status::StreamEnd => {
                                            ended = true;
                                            break;
                                        }
                                        _ => {
                                            if self.pos >= self.pending.len() {
                                                break;
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    self.pos += (zd.total_in() - tin) as usize;
                                    err = Some(e);
                                    break;
                                }
                            }
                        }
                        let m = self.gz.as_mut().unwrap();
                        for chunk in &produced {
                            for &b in chunk.iter() {
                                m.crc = CRC32_TABLE[((m.crc ^ b as u32) & 0xFF) as usize]
                                    ^ (m.crc >> 8);
                            }
                            m.outlen = m.outlen.wrapping_add(chunk.len() as u32);
                        }
                        for chunk in produced.drain(..) {
                            self.out.extend_from_slice(&chunk);
                        }
                    }
                    if let Some(e) = err {
                        return Err(self.classify_inflate_err(&e));
                    }
                    if ended {
                        self.gz.as_mut().unwrap().phase = 2;
                    } else if finish {
                        return trunc(tolerate, self);
                    } else {
                        return Ok(());
                    }
                }
                2 => {
                    // trailer：crc32 + isize 各 4B
                    while self.gz.as_ref().unwrap().trailer.len() < 8
                        && self.pos < self.pending.len()
                    {
                        let b = self.pending[self.pos];
                        self.pos += 1;
                        self.gz.as_mut().unwrap().trailer.push(b);
                    }
                    if self.gz.as_ref().unwrap().trailer.len() < 8 {
                        if finish {
                            return trunc(tolerate, self);
                        }
                        return Ok(());
                    }
                    let t = self.gz.as_ref().unwrap().trailer.clone();
                    let m = self.gz.as_ref().unwrap();
                    let crc_expect = u32::from_le_bytes([t[0], t[1], t[2], t[3]]);
                    let isize_expect = u32::from_le_bytes([t[4], t[5], t[6], t[7]]);
                    if crc_expect != !m.crc {
                        return Err(ZErr::data("incorrect data check"));
                    }
                    if isize_expect != m.outlen {
                        return Err(ZErr::data("incorrect length check"));
                    }
                    let m = self.gz.as_mut().unwrap();
                    m.phase = 3;
                    m.hdr.clear();
                    m.trailer.clear();
                    m.zd = None;
                    m.crc = 0xFFFF_FFFF;
                    m.outlen = 0;
                }
                _ => {
                    // between：成员完结后决定去向
                    let avail = self.pending.len() - self.pos;
                    if avail == 0 {
                        if finish || single {
                            self.done = true;
                        }
                        return Ok(());
                    }
                    if self.reject {
                        return Err(ZErr::junk());
                    }
                    // trailing 全零 → 忽略（reject 已在上方报 junk，真机口径）
                    if self.pending[self.pos..].iter().all(|&b| b == 0) {
                        self.done = true;
                        return Ok(());
                    }
                    if single {
                        // 单成员（Web）：任何剩余输入即 junk → TypeError（JS 侧映射）
                        return Err(ZErr::junk());
                    }
                    self.gz.as_mut().unwrap().phase = 0;
                }
            }
        }
    }

    fn compact(&mut self) {
        if self.pos > 0 && self.pos == self.pending.len() {
            self.pending.clear();
            self.pos = 0;
        } else if self.pos > (1 << 20) {
            self.pending.drain(..self.pos);
            self.pos = 0;
        }
    }

    /// brotli 编码泵。
    fn brotli_enc_feed(&mut self, input: &[u8], flag: u32) -> Result<(), ZErr> {
        let be = self.be.as_mut().unwrap();
        if flag == 2 && be.st.is_finished() {
            return Ok(()); // 终结后吞掉
        }
        let op = match flag {
            1 => brotli::enc::encode::BrotliEncoderOperation::BROTLI_OPERATION_FLUSH,
            2 => brotli::enc::encode::BrotliEncoderOperation::BROTLI_OPERATION_FINISH,
            3 => brotli::enc::encode::BrotliEncoderOperation::BROTLI_OPERATION_EMIT_METADATA,
            _ => brotli::enc::encode::BrotliEncoderOperation::BROTLI_OPERATION_PROCESS,
        };
        let mut avail_in = input.len();
        let mut in_off = 0usize;
        loop {
            let mut outbuf = [0u8; 16384];
            let mut avail_out = outbuf.len();
            let mut out_off = 0usize;
            let mut total_out: Option<usize> = None;
            let ok = be.st.compress_stream(
                op,
                &mut avail_in,
                input,
                &mut in_off,
                &mut avail_out,
                &mut outbuf,
                &mut out_off,
                &mut total_out,
                &mut |_, _, _, _| {},
            );
            if !ok {
                return Err(ZErr::new("Z_DATA_ERROR", "Compression failed"));
            }
            self.out.extend_from_slice(&outbuf[..out_off]);
            if avail_in == 0 && !be.st.has_more_output() {
                break;
            }
        }
        if flag == 2 && be.st.is_finished() {
            self.done = true;
        }
        Ok(())
    }

    /// brotli 解码泵。
    fn brotli_dec_feed(&mut self, input: &[u8]) -> Result<(), ZErr> {
        let bd = self.bd.as_mut().unwrap();
        if bd.done_flag {
            return Ok(());
        }
        let mut avail_in = input.len();
        let mut in_off = 0usize;
        let mut total_out: usize = 0;
        loop {
            let mut outbuf = [0u8; 16384];
            let mut avail_out = outbuf.len();
            let mut out_off = 0usize;
            let res = brotli::BrotliDecompressStream(
                &mut avail_in,
                &mut in_off,
                input,
                &mut avail_out,
                &mut out_off,
                &mut outbuf,
                &mut total_out,
                &mut bd.st,
            );
            self.out.extend_from_slice(&outbuf[..out_off]);
            match res {
                brotli::BrotliResult::ResultSuccess => {
                    if brotli_decompressor::BrotliDecoderIsFinished(&bd.st) {
                        bd.done_flag = true;
                        self.done = true;
                        if self.reject && avail_in > 0 {
                            return Err(ZErr::junk());
                        }
                        break;
                    }
                    if avail_in == 0 {
                        break;
                    }
                }
                brotli::BrotliResult::NeedsMoreInput => {
                    if brotli_decompressor::BrotliDecoderIsFinished(&bd.st) {
                        bd.done_flag = true;
                        self.done = true;
                        if self.reject && avail_in > 0 {
                            return Err(ZErr::junk());
                        }
                    }
                    break;
                }
                brotli::BrotliResult::NeedsMoreOutput => {
                    continue;
                }
                brotli::BrotliResult::ResultFailure => {
                    return Err(brotli_dec_err(&bd.st));
                }
            }
        }
        Ok(())
    }

    /// zstd 编码泵（flush 边界分段出帧；pledged 终检）。
    fn zstd_enc_feed(&mut self, input: &[u8], flag: u32) -> Result<(), ZErr> {
        let ze = self.ze.as_mut().unwrap();
        ze.buf.extend_from_slice(input);
        ze.total_in += input.len() as u64;
        let end = flag == 2;
        if flag == 1 || end {
            if ze.buf.len() > ze.flushed {
                let seg = ze.buf[ze.flushed..].to_vec();
                ze.flushed = ze.buf.len();
                let out = ruzstd::encoding::compress_to_vec(
                    &seg[..],
                    ruzstd::encoding::CompressionLevel::Fastest,
                );
                self.out.extend_from_slice(&out);
            } else if end && ze.total_in == 0 {
                const EMPTY: &[u8] = &[];
                let out = ruzstd::encoding::compress_to_vec(
                    EMPTY,
                    ruzstd::encoding::CompressionLevel::Fastest,
                );
                self.out.extend_from_slice(&out);
            }
        }
        if end {
            let ze = self.ze.as_ref().unwrap();
            if let Some(p) = ze.pledged {
                if p != ze.total_in {
                    return Err(ZErr::new("ZSTD_error_srcSize_wrong", "Src size is incorrect"));
                }
            }
            self.done = true;
        }
        Ok(())
    }

    /// zstd 解码泵（一次性解 pos 前缀首帧；轮子无增量 API，记档）。
    fn zstd_decode_pump(&mut self, tolerate_missing: bool) -> Result<(), ZErr> {
        let z = self.zd2.as_mut().unwrap();
        if z.pos >= z.carried.len() {
            return Ok(());
        }
        let mut cur = std::io::Cursor::new(&z.carried[z.pos..]);
        let mut fd = ruzstd::decoding::FrameDecoder::new();
        // 注：ruzstd::decoding::{FrameDecoder, BlockDecodingStrategy}；FrameDecoder 自带 io::Read
        if let Err(_e) = fd.init(&mut cur) {
            let avail = z.carried.len() - z.pos;
            let magic_ok = avail >= 4
                && z.carried[z.pos] == 0x28
                && z.carried[z.pos + 1] == 0xB5
                && z.carried[z.pos + 2] == 0x2F
                && z.carried[z.pos + 3] == 0xFD;
            return Err(if avail < 4 || magic_ok {
                if tolerate_missing {
                    self.done = true;
                    return Ok(());
                }
                ZErr::buf("unexpected end of file")
            } else {
                ZErr::new("ZSTD_error_prefix_unknown", "Unknown frame descriptor")
            });
        }
        loop {
            if fd.is_finished() {
                break;
            }
            match fd.decode_blocks(&mut cur, ruzstd::decoding::BlockDecodingStrategy::UptoBytes(1 << 20)) {
                Ok(_) => {
                    let mut buf = [0u8; 65536];
                    loop {
                        match fd.read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => self.out.extend_from_slice(&buf[..n]),
                            Err(_) => break,
                        }
                    }
                    let exhausted = cur.position() as usize >= z.carried.len() - z.pos;
                    if exhausted && !fd.is_finished() && fd.can_collect() == 0 {
                        if tolerate_missing {
                            self.done = true;
                            return Ok(());
                        }
                        return Err(ZErr::buf("unexpected end of file"));
                    }
                }
                Err(_e) => {
                    let exhausted = cur.position() as usize >= z.carried.len() - z.pos;
                    if exhausted {
                        if tolerate_missing {
                            self.done = true;
                            return Ok(());
                        }
                        return Err(ZErr::buf("unexpected end of file"));
                    }
                    return Err(ZErr::new("ZSTD_error_??", "corrupt zstd frame"));
                }
            }
        }
        let mut buf = [0u8; 65536];
        loop {
            match fd.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => self.out.extend_from_slice(&buf[..n]),
                Err(_) => break,
            }
        }
        let consumed = cur.position() as usize;
        z.pos += consumed;
        self.done = true;
        if self.reject && z.carried.len() > z.pos {
            return Err(ZErr::junk());
        }
        Ok(())
    }

    /// 统一入口：返回 (unconsumed, done)。
    fn feed(&mut self, input: &[u8], flag: u32) -> Result<(usize, bool), ZErr> {
        let r = match self.kind {
            ZKind::ZlibDeflate | ZKind::RawDeflate | ZKind::GzipDeflate => {
                self.deflate_feed(input, flag)
            }
            ZKind::BrotliEnc => self.brotli_enc_feed(input, flag),
            ZKind::BrotliDec => self.brotli_dec_feed(input),
            ZKind::ZstdEnc => self.zstd_enc_feed(input, flag),
            _ => self.inflate_feed(input, flag),
        };
        r?;
        Ok((self.unconsumed(), self.done))
    }

    fn take_out(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.out)
    }

    fn reset(&mut self) {
        let kind = self.kind;
        let level = self.level;
        let reject = self.reject;
        let dict = std::mem::take(&mut self.dict);
        let pledged = self.ze.as_ref().and_then(|z| z.pledged);
        *self = ZEngine::new(kind, level, &dict, pledged, reject);
    }
}

fn brotli_dec_err(st: &brotli_decompressor::BrotliState<
    brotli::enc::StandardAlloc,
    brotli::enc::StandardAlloc,
    brotli::enc::StandardAlloc,
>) -> ZErr {
    // node 口径：ERR__<去掉 BROTLI_DECODER_ 前缀的枚举名>，message "Decompression failed"
    let name = format!("{:?}", st.error_code);
    let stripped = name.strip_prefix("BROTLI_DECODER_").unwrap_or(&name);
    ZErr::new(&format!("ERR__{}", stripped), "Decompression failed")
}

/// gzip 流头（与 flate2 GzEncoder 逐字节一致，单测钉住）。
fn gzip_header(level: i32) -> [u8; 10] {
    let xfl: u8 = if level >= 9 {
        2
    } else if level >= 0 && level <= 1 {
        4
    } else {
        0
    };
    [0x1f, 0x8b, 8, 0, 0, 0, 0, 0, xfl, 255]
}

thread_local! {
    static ZSTREAMS: std::cell::RefCell<std::collections::HashMap<u32, ZEngine>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
    static ZSTREAM_NEXT: std::cell::Cell<u32> = std::cell::Cell::new(1);
}

/// `__wjs_zlib_stream_new(kind, level, dict|null, pledged|-1, reject01)` → id。
pub unsafe extern "C" fn zlib_stream_new(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let num = |i: u32| -> f64 {
        if frame.argc() > i && frame.arg(i).is_number() {
            frame.arg(i).to_number()
        } else {
            f64::NAN
        }
    };
    let kind_v = num(0);
    if !(0.0..=11.0).contains(&kind_v) {
        report_error(&mut cx, "RangeError: invalid zlib stream kind");
        return false;
    }
    let kind = match kind_v as u32 {
        0 => ZKind::ZlibDeflate,
        1 => ZKind::RawDeflate,
        2 => ZKind::GzipDeflate,
        3 => ZKind::ZlibInflate,
        4 => ZKind::RawInflate,
        5 => ZKind::GzipInflate,
        6 => ZKind::GzipInflateSingle,
        7 => ZKind::AutoInflate,
        8 => ZKind::BrotliEnc,
        9 => ZKind::BrotliDec,
        10 => ZKind::ZstdEnc,
        _ => ZKind::ZstdDec,
    };
    let level = num(1) as i32;
    let dict: Vec<u8> = if frame.argc() > 2 && !frame.arg(2).is_null_or_undefined() {
        match view_bytes(&mut cx, frame.arg(2), "dictionary") {
            Some(v) => v,
            None => return false,
        }
    } else {
        Vec::new()
    };
    let pv = num(3);
    let pledged = if pv.is_finite() && pv >= 0.0 { Some(pv as u64) } else { None };
    let reject = frame.argc() > 4 && frame.arg(4).is_number() && frame.arg(4).to_number() != 0.0;
    let id = ZSTREAM_NEXT.with(|n| {
        let v = n.get();
        n.set(v.wrapping_add(1));
        v
    });
    ZSTREAMS.with(|s| {
        s.borrow_mut().insert(id, ZEngine::new(kind, level, &dict, pledged, reject));
    });
    frame.set_rval(mozjs::jsval::DoubleValue(id as f64));
    true
}

/// `__wjs_zlib_stream_feed(id, dataU8, flag)` → JSON `{"c":n,"d":bool[,"code","msg"]}`。
pub unsafe extern "C" fn zlib_stream_feed(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = if frame.argc() > 0 && frame.arg(0).is_number() {
        frame.arg(0).to_number() as u32
    } else {
        0
    };
    let data: Vec<u8> = if frame.argc() > 1 && !frame.arg(1).is_null_or_undefined() {
        match view_bytes(&mut cx, frame.arg(1), "stream data") {
            Some(v) => v,
            None => return false,
        }
    } else {
        Vec::new()
    };
    let flag = if frame.argc() > 2 && frame.arg(2).is_number() {
        frame.arg(2).to_number() as u32
    } else {
        0
    };
    let r = ZSTREAMS.with(|s| {
        let mut map = s.borrow_mut();
        match map.get_mut(&id) {
            Some(e) => e.feed(&data, flag),
            None => Err(ZErr::new("Z_STREAM_ERROR", "unknown stream id")),
        }
    });
    let json = match r {
        Ok((c, d)) => format!(r#"{{"c":{c},"d":{d}}}"#),
        Err(e) => format!(
            r#"{{"c":0,"d":false,"code":{},"msg":{}}}"#,
            json_str(&e.code),
            json_str(&e.msg)
        ),
    };
    rooted!(&in(cx) let mut v = UndefinedValue());
    json.to_jsval(&mut cx, v.handle_mut());
    frame.set_rval(v.get());
    true
}

/// `__wjs_zlib_stream_out(id)` → Uint8Array（排空引擎累计输出）。
pub unsafe extern "C" fn zlib_stream_out(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = if frame.argc() > 0 && frame.arg(0).is_number() {
        frame.arg(0).to_number() as u32
    } else {
        0
    };
    let out = ZSTREAMS.with(|s| {
        let mut map = s.borrow_mut();
        match map.get_mut(&id) {
            Some(e) => e.take_out(),
            None => Vec::new(),
        }
    });
    set_rval_bytes(&mut cx, &frame, &out)
}

/// `__wjs_zlib_stream_free(id)`。
pub unsafe extern "C" fn zlib_stream_free(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let _ = cx_raw; // N-API 签名固定；free/reset 纯 Rust 表操作无 JSAPI
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = if frame.argc() > 0 && frame.arg(0).is_number() {
        frame.arg(0).to_number() as u32
    } else {
        0
    };
    ZSTREAMS.with(|s| {
        s.borrow_mut().remove(&id);
    });
    frame.set_rval(mozjs::jsval::UndefinedValue());
    true
}

/// `__wjs_zlib_stream_reset(id)`。
pub unsafe extern "C" fn zlib_stream_reset(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let _ = cx_raw; // N-API 签名固定；free/reset 纯 Rust 表操作无 JSAPI
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = if frame.argc() > 0 && frame.arg(0).is_number() {
        frame.arg(0).to_number() as u32
    } else {
        0
    };
    ZSTREAMS.with(|s| {
        if let Some(e) = s.borrow_mut().get_mut(&id) {
            e.reset();
        }
    });
    frame.set_rval(mozjs::jsval::UndefinedValue());
    true
}

/// JSON 字符串字面量（引擎错误进 feed 的 JSON 载荷）。
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// 内嵌 ESM 源（`node:zlib`）。
pub const SOURCE: &str = r#"
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
  const maxOut = __zMaxOut(opts);
  const out = __zCall(() => __wjs_zlib_inflate_raw(data));
  if (maxOut !== undefined && out.length > maxOut) throw new ERR_BUFFER_TOO_LARGE(maxOut);
  return __zCheckKMax(out);
}
export function inflateRaw(buf, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  if (opts && opts.info) { const eng = new InflateRaw(opts); try { const r = inflateRawSync(buf, opts); cb(null, { buffer: r.buffer ?? r, engine: eng }); } catch (e) { cb(e); } return; }
  __zNeedCb(cb, "inflateRaw");
  const data = __zChecked(buf, "inflateRaw");
  const maxOut = __zMaxOut(opts);
  __zAsync((d, m) => {
    const out = __zCall(() => __wjs_zlib_inflate_raw(d));
    if (m !== undefined && out.length > m) throw new ERR_BUFFER_TOO_LARGE(m);
    return __zCheckKMax(out);
  }, [data, maxOut], cb);
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
  const maxOut = __zMaxOut(opts);
  const out = __zCall(() => __wjs_zlib_zstd_decompress(data));
  if (maxOut !== undefined && out.length > maxOut) throw new ERR_BUFFER_TOO_LARGE(maxOut);
  return __zCheckKMax(out);
}
export function zstdDecompress(buf, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  if (opts && opts.info) { const eng = new ZstdDecompress(opts); try { const r = zstdDecompressSync(buf, opts); cb(null, { buffer: r.buffer ?? r, engine: eng }); } catch (e) { cb(e); } return; }
  __zNeedCb(cb, "zstdDecompress");
  const data = __zChecked(buf, "zstdDecompress");
  const maxOut = __zMaxOut(opts);
  __zAsync((d, m) => {
    const out = __zCall(() => __wjs_zlib_zstd_decompress(d));
    if (m !== undefined && out.length > m) throw new ERR_BUFFER_TOO_LARGE(m);
    return __zCheckKMax(out);
  }, [data, maxOut], cb);
}
// 流式类（Node ZlibBase 口径的最小实现：Transform 子类，累积 input，
// _flush 时调同名 Sync 版一次产出；同步底层记档沿用 §7 头注）。
// 覆盖 12 类 + createXxx 工厂 + info 选项（{buffer, engine}）+ bytesWritten。
// 偏差记档：flush()/write 分段增量不做（整收）；dictionary/windowBits/memLevel/
// chunkSize/strategy 接受忽略（构造校验只做 failed-init 套件口径：chunkSize 范围）。
import { Transform } from "node:stream";
function __zStreamBase(opts, syncFn) {
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
"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// 真机 26.8.2 对拍（probe-flush.js）：level-0 deflate
    /// write+flush(Z_NO_FLUSH) → 7801；flush()(Z_FULL_FLUSH) → 存储块+数据+空存储块。
    #[test]
    fn zlib_stream_flush_framing_vectors() {
        use ZKind::*;
        let chunk: Vec<u8> = vec![
            0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, 0x4a, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x01,
            0x00, 0x48,
        ];
        let mut e = ZEngine::new(ZlibDeflate, 0, &[], None, false);
        e.feed(&chunk, 0).unwrap();
        assert_eq!(e.take_out(), vec![0x78, 0x01], "NO_FLUSH 后只出 zlib 头");
        e.feed(&[], 3).unwrap();
        let mut want = vec![0x00, 0x10, 0x00, 0xef, 0xff];
        want.extend_from_slice(&chunk);
        want.extend_from_slice(&[0x00, 0x00, 0x00, 0xff, 0xff]);
        assert_eq!(e.take_out(), want, "Z_FULL_FLUSH = 存储块+数据+空存储块");
    }

    /// 流式 gzip 帧与自家 GzEncoder（one-shot 真机对齐基线）逐字节一致。
    #[test]
    fn zlib_stream_gzip_matches_gzencoder() {
        use std::io::Write as _;
        use ZKind::*;
        for level in [0i32, 1, 6, 9] {
            let data = b"stream gzip framing probe 0123456789".repeat(3);
            let mut e = ZEngine::new(GzipDeflate, level, &[], None, false);
            e.feed(&data[..10], 0).unwrap();
            e.feed(&data[10..], 4).unwrap();
            let streamed = e.take_out();
            let mut enc = flate2::write::GzEncoder::new(Vec::new(), zlib_level(level));
            enc.write_all(&data).unwrap();
            let oneshot = enc.finish().unwrap();
            assert_eq!(streamed, oneshot, "level {level}");
            // 自家 gunzip 成员机回解
            let mut d = ZEngine::new(GzipInflate, -1, &[], None, false);
            d.feed(&streamed, 4).unwrap();
            assert!(d.done);
            assert_eq!(d.take_out(), data);
        }
    }

    /// 增量引擎族回归：deflate/inflate/raw/gzip 双向 + brotli + zstd + 截断语义。
    #[test]
    fn zlib_stream_engine_vectors() {
        use ZKind::*;
        let data = b"engine roundtrip vector -- the quick brown fox".repeat(20);
        // zlib 包裹
        let mut c = ZEngine::new(ZlibDeflate, 6, &[], None, false);
        c.feed(&data, 0).unwrap();
        c.feed(&[], 4).unwrap();
        let z = c.take_out();
        assert!(c.done);
        let mut d = ZEngine::new(ZlibInflate, -1, &[], None, false);
        d.feed(&z, 0).unwrap();
        d.feed(&[], 4).unwrap();
        assert!(d.done);
        assert_eq!(d.take_out(), data);
        // raw 截断 → Z_BUF_ERROR unexpected end of file（真机口径）
        let mut raw = ZEngine::new(RawDeflate, 6, &[], None, false);
        raw.feed(&data, 0).unwrap();
        raw.feed(&[], 4).unwrap();
        let r = raw.take_out();
        let mut di = ZEngine::new(RawInflate, -1, &[], None, false);
        let e = di.feed(&r[..r.len() / 2], 4).err().expect("截断应报错");
        assert_eq!((e.code.as_str(), e.msg.as_str()), ("Z_BUF_ERROR", "unexpected end of file"));
        // finishFlush=Sync 容忍截断（部分输出）
        let mut dt = ZEngine::new(RawInflate, -1, &[], None, false);
        dt.feed(&r[..r.len() / 2], 2).unwrap();
        assert!(dt.done);
        let partial = dt.take_out();
        assert!(!partial.is_empty() && partial.len() < data.len());
        assert_eq!(&data[..partial.len()], &partial[..]);
        // brotli 双向
        let mut be = ZEngine::new(BrotliEnc, 5, &[], None, false);
        be.feed(&data, 0).unwrap();
        be.feed(&[], 2).unwrap();
        let b = be.take_out();
        assert!(be.done);
        let mut bd = ZEngine::new(BrotliDec, -1, &[], None, false);
        bd.feed(&b, 0).unwrap();
        assert!(bd.done);
        assert_eq!(bd.take_out(), data);
        // brotli 字典双向
        let dict = b"engine dictionary shared context lorem ipsum";
        let mut bed = ZEngine::new(BrotliEnc, 5, dict, None, false);
        bed.feed(dict, 0).unwrap();
        bed.feed(&[], 2).unwrap();
        let bd2 = bed.take_out();
        let mut bdd = ZEngine::new(BrotliDec, -1, dict, None, false);
        bdd.feed(&bd2, 0).unwrap();
        assert_eq!(bdd.take_out(), dict.to_vec());
        // brotli 无字典解带字典流 → ERR__ 错误（真机 ERR__ERROR_FORMAT_DICTIONARY 形）
        let mut bdn = ZEngine::new(BrotliDec, -1, &[], None, false);
        let err = bdn.feed(&bd2, 0).err().expect("无字典应失败");
        assert!(err.code.starts_with("ERR__"), "code: {}", err.code);
        assert_eq!(err.msg, "Decompression failed");
        // zstd 双向
        let mut ze = ZEngine::new(ZstdEnc, -1, &[], None, false);
        ze.feed(&data, 0).unwrap();
        ze.feed(&[], 2).unwrap();
        let zs = ze.take_out();
        assert!(ze.done);
        let mut zd = ZEngine::new(ZstdDec, -1, &[], None, false);
        zd.feed(&zs, 2).unwrap();
        assert_eq!(zd.take_out(), data);
        // zstd 截断 → unexpected end of file；tolerate → 部分输出
        let mut zt = ZEngine::new(ZstdDec, -1, &[], None, false);
        let trunc = &zs[..zs.len() / 2];
        zt.zd2.as_mut().unwrap().carried.extend_from_slice(trunc);
        let e = zt.zstd_decode_pump(false).err().expect("截断应报错");
        assert_eq!((e.code.as_str(), e.msg.as_str()), ("Z_BUF_ERROR", "unexpected end of file"));
        let mut zt2 = ZEngine::new(ZstdDec, -1, &[], None, false);
        zt2.zd2.as_mut().unwrap().carried.extend_from_slice(trunc);
        zt2.zstd_decode_pump(true).unwrap();
        assert!(zt2.done);
        // zstd 前缀错误（真机 ZSTD_error_prefix_unknown / Unknown frame descriptor）
        let mut zg = ZEngine::new(ZstdDec, -1, &[], None, false);
        zg.zd2.as_mut().unwrap().carried.extend_from_slice(b"garbage data here");
        let e = zg.zstd_decode_pump(false).err().expect("垃圾应报错");
        assert_eq!((e.code.as_str(), e.msg.as_str()), ("ZSTD_error_prefix_unknown", "Unknown frame descriptor"));
        // gunzip 成员机：多成员 + 尾零 + 尾垃圾 + reject
        let mut g1 = ZEngine::new(GzipDeflate, 6, &[], None, false);
        g1.feed(b"abc", 4).unwrap();
        let ga = g1.take_out();
        let mut g2 = ZEngine::new(GzipDeflate, 6, &[], None, false);
        g2.feed(b"def", 4).unwrap();
        let gb = g2.take_out();
        let mut gm = ZEngine::new(GzipInflate, -1, &[], None, false);
        let concat: Vec<u8> = [ga.as_slice(), gb.as_slice(), &[0u8; 10][..]].concat();
        gm.feed(&concat, 4).unwrap();
        assert!(gm.done);
        assert_eq!(gm.take_out(), b"abcdef");
        // 1f 8b ff ff 尾垃圾 → unknown compression method
        let mut gmg = ZEngine::new(GzipInflate, -1, &[], None, false);
        let concat2: Vec<u8> = [ga.as_slice(), &[0x1f, 0x8b, 0xff, 0xff][..]].concat();
        let e = gmg.feed(&concat2, 4).err().unwrap();
        assert_eq!((e.code.as_str(), e.msg.as_str()), ("Z_DATA_ERROR", "unknown compression method"));
        // 非魔数乱码尾 → incorrect header check
        let mut gmg2 = ZEngine::new(GzipInflate, -1, &[], None, false);
        let concat3: Vec<u8> = [ga.as_slice(), &[1u8, 2, 3][..]].concat();
        let e = gmg2.feed(&concat3, 4).err().unwrap();
        assert_eq!((e.code.as_str(), e.msg.as_str()), ("Z_DATA_ERROR", "incorrect header check"));
        // reject：第二成员也算 junk
        let mut gmr = ZEngine::new(GzipInflate, -1, &[], None, true);
        let concat4: Vec<u8> = [ga.as_slice(), gb.as_slice()].concat();
        let e = gmr.feed(&concat4, 4).err().unwrap();
        assert_eq!(e.code, "ERR_TRAILING_JUNK_AFTER_STREAM_END");
        // 空输入 inflate → Z_BUF_ERROR unexpected end of file（真机口径）
        let mut ei = ZEngine::new(ZlibInflate, -1, &[], None, false);
        let e = ei.feed(&[], 4).err().unwrap();
        assert_eq!((e.code.as_str(), e.msg.as_str()), ("Z_BUF_ERROR", "unexpected end of file"));
    }

    #[test]
    fn zz_scratch_isolate_hang() {
        // 直接探 flate2（zlib_rs 后端）compress_vec 各 flush 的行为
        use flate2::{Compress, FlushCompress, Status};
        let data = b"engine roundtrip vector -- the quick brown fox".repeat(20);
        for (name, flush) in [("None", FlushCompress::None), ("Sync", FlushCompress::Sync), ("Full", FlushCompress::Full), ("Finish", FlushCompress::Finish)] {
            let mut c = Compress::new(flate2::Compression::new(6), true);
            let mut out = Vec::new();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut iters = 0;
                let mut src = &data[..];
                loop {
                    iters += 1;
                    if iters > 50 { panic!("{name}: no progress after 50 iters"); }
                    let tin = c.total_in();
                    let ob = out.len();
                    let st = c.compress_vec(src, &mut out, flush).unwrap();
                    let consumed = (c.total_in() - tin) as usize;
                    eprintln!("ZZ {name} iter{iters} st={st:?} consumed={consumed} produced={}", out.len() - ob);
                    src = &src[consumed..];
                    match st {
                        Status::StreamEnd => break,
                        _ => { if src.is_empty() { break; } }
                    }
                }
            }));
            if let Err(e) = r {
                eprintln!("ZZ {name} LOOP: {e:?}");
            }
        }
    }

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
