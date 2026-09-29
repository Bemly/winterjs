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
pub(crate) fn zlib_level(level: i32) -> flate2::Compression {
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
pub(crate) static CRC32_TABLE: [u32; 256] = crc32_table();

/// `__wjs2_zlib_crc32(dataU8, seedU32)` → uint32（真机值对拍：
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

/// `__wjs2_zlib_deflate_lv(dataU8, level)`（level -1=默认；JS 侧已验 -1..9）。
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

/// `__wjs2_zlib_inflate(dataU8)`（zlib 包裹）。
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

/// `__wjs2_zlib_deflate_raw(dataU8, level)`（裸 deflate）。
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

/// `__wjs2_zlib_inflate_raw(dataU8)`（裸 deflate 解码）。
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

/// `__wjs2_zlib_gzip(dataU8, level)`。
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

/// `__wjs2_zlib_gunzip(dataU8)`。
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

/// `__wjs2_zlib_unzip(dataU8)`（gzip 优先、zlib 兜底；Node Unzip 自动识别口径）。
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

use super::zlib_engine::{ZEngine, ZErr, ZKind};


pub(crate) fn brotli_dec_err(st: &brotli_decompressor::BrotliState<
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
pub(crate) fn gzip_header(level: i32) -> [u8; 10] {
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

/// `__wjs2_zlib_stream_new(kind, level, dict|null, pledged|-1, reject01)` → id。
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

/// `__wjs2_zlib_stream_feed(id, dataU8, flag)` → JSON `{"c":n,"d":bool[,"code","msg"]}`。
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

/// `__wjs2_zlib_stream_out(id)` → Uint8Array（排空引擎累计输出）。
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

/// `__wjs2_zlib_stream_free(id)`。
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

/// `__wjs2_zlib_stream_reset(id)`。
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

/// 内嵌 ESM 源（`node:zlib`；§0.9 按域分块：`zlib.js` 全量，concat 字节恒等）。
pub const SOURCE: &str = include_str!("zlib.js");

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
        let data = b"hello winterjs2 zlib probe 0123456789".repeat(8);
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

#[cfg(test)]
mod zdbg {
    use super::*;
    #[test]
    fn zlib_dict_stream_roundtrip_chunks() {
        // zlib 族流式字典（FDICT 被动 NEED_DICT 恢复路径；dictionary 套件流式形）。
        let dict = b"hello world, this is a dictionary test";
        let data = b"A line of data\n".repeat(20);
        let comp = {
            let mut c = ZEngine::new(ZKind::ZlibDeflate, -1, dict, None, false);
            c.feed(&data, 0).unwrap();
            c.feed(&[], 4).unwrap();
            c.take_out()
        };
        let mut d = ZEngine::new(ZKind::ZlibInflate, -1, dict, None, false);
        let half = comp.len() / 2;
        d.feed(&comp[..half], 0).unwrap();
        d.feed(&comp[half..], 0).unwrap();
        d.feed(&[], 4).unwrap();
        assert_eq!(d.take_out(), data);
        // reset 后重用（引擎重建，字典保活）。
        let mut c2 = ZEngine::new(ZKind::ZlibDeflate, -1, dict, None, false);
        c2.feed(&data, 0).unwrap();
        c2.feed(&[], 4).unwrap();
        c2.reset();
        c2.feed(&data, 0).unwrap();
        c2.feed(&[], 4).unwrap();
        let comp2 = c2.take_out();
        let mut d2 = ZEngine::new(ZKind::ZlibInflate, -1, dict, None, false);
        d2.feed(&comp2, 0).unwrap();
        d2.feed(&[], 4).unwrap();
        assert_eq!(d2.take_out(), data);
    }

    #[test]
    fn raw_dict_stream_proactive_set_dictionary() {
        // G9-3a 回归：raw 流无 FDICT 头，字典必须在首次 inflate 前主动设
        // （zlib 语义）；被动 NEED_DICT 恢复对 raw 留 Mode::Bad，后续喂入
        // 报 "repeated call with bad state"（dictionary 套件 raw/rawreset 修前必挂）。
        let dict = b"lorem ipsum dolor sit amet consectetur adipiscing elit 0123456789";
        let data = b"HTTP/1.1 200 Ok\r\nServer: node.js\r\nContent-Length: 0\r\n\r\n".repeat(8);
        let comp = {
            let mut c = ZEngine::new(ZKind::RawDeflate, -1, dict, None, false);
            c.feed(&data, 0).unwrap();
            c.feed(&[], 4).unwrap();
            c.take_out()
        };
        let mut d = ZEngine::new(ZKind::RawInflate, -1, dict, None, false);
        let half = comp.len() / 2;
        d.feed(&comp[..half], 0).unwrap();
        d.feed(&comp[half..], 0).unwrap();
        d.feed(&[], 4).unwrap();
        assert_eq!(d.take_out(), data);
    }

    #[test]
    fn zstd_pledged_mismatch_engine_error() {
        // pledged 终检（zstd_enc_feed）：mismatch → ZSTD_error_srcSize_wrong；
        // match → 正常出帧。pledged 套件 err.errno=72 断言由 JS constants 覆盖。
        let mut e = ZEngine::new(ZKind::ZstdEnc, -1, &[], Some(9), false);
        let err = e.feed(b"xxxxxxxxxx", 2).unwrap_err();
        assert_eq!(err.code, "ZSTD_error_srcSize_wrong");
        assert_eq!(err.msg, "Src size is incorrect");
        let mut e2 = ZEngine::new(ZKind::ZstdEnc, -1, &[], Some(10), false);
        e2.feed(b"xxxxxxxxxx", 2).unwrap();
        assert!(!e2.take_out().is_empty());
        // 空输入 pledged 0 → 正常（testCases {0,0} 形）。
        let mut e3 = ZEngine::new(ZKind::ZstdEnc, -1, &[], Some(0), false);
        e3.feed(&[], 2).unwrap();
    }

    #[test]
    fn zlib_stream_inflate_slices_and_finish() {
        // 回归（zip-property 丢尾）：RawDeflate 全量压缩 → RawInflate 逐片喂
        // → finish 泵到 StreamEnd（deflate_feed 的 finish 循环曾 src 空即退，
        // 丢 flate2 内部缓冲尾段 2304B）。
        let data: Vec<u8> = (0..(256 * 1024)).map(|i| (i as u32).wrapping_mul(2654435761).wrapping_shr(16) as u8).collect();
        let comp = {
            let mut c = ZEngine::new(ZKind::RawDeflate, -1, &[], None, false);
            c.feed(&data, 4).unwrap();
            c.take_out()
        };
        let mut d = ZEngine::new(ZKind::RawInflate, -1, &[], None, false);
        let step = 4096;
        let mut i = 0;
        while i < comp.len() {
            let end = (i + step).min(comp.len());
            let (_, dn) = d.feed(&comp[i..end], 0).unwrap();
            if dn { println!("done at slice {i}"); }
            i = end;
        }
        let r3 = d.feed(&[], 4).unwrap();
        assert!(r3.1, "finish 必须 StreamEnd");
        assert_eq!(d.take_out().len(), data.len());
    }
}
