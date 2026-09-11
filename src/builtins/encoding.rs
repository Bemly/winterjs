//! 编码：atob/btoa + TextEncoder/Decoder（`encoding_rs` + `base64`）。
//! 约定：复杂返回值走 JSON 桥（`encodeInto` 的 `{read,written}`）；
//! 仅 `te_encode` 需创建 Uint8Array（1 个 mozjs 边界 `unsafe`，见 §6 审计）。

use mozjs::context::JSContext;
use mozjs::conversions::ToJSValConvertible as _;
use mozjs::jsapi::JSObject;
use mozjs::jsval::{JSVal, ObjectValue, UndefinedValue};
use mozjs::rooted;
use mozjs::typedarray::{CreateWith, TypedArray, Uint8};

use crate::jsapi_glue::{report_error, value_to_string, view_bytes, wrap_cx, Frame};

fn arg_string(cx: &mut JSContext, frame: &Frame, i: u32, what: &str) -> Option<String> {
    if frame.argc() <= i {
        report_error(cx, &format!("TypeError: {what} requires an argument"));
        return None;
    }
    Some(value_to_string(cx, frame.arg(i)))
}

fn set_rval_string(cx: &mut JSContext, frame: &Frame, s: &str) {
    rooted!(&in(cx) let mut v = UndefinedValue());
    s.to_jsval(cx, v.handle_mut());
    frame.set_rval(v.get());
}

/// `__wjs_btoa(s)`：Latin-1 → base64；超界抛 InvalidCharacterError。
pub unsafe extern "C" fn btoa_encode(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(s) = arg_string(&mut cx, &frame, 0, "btoa") else {
        return false;
    };
    if s.chars().any(|c| c as u32 > 255) {
        report_error(&mut cx, "InvalidCharacterError: btoa input must be Latin-1");
        return false;
    }
    let bytes: Vec<u8> = s.chars().map(|c| c as u8).collect();
    let out = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
    set_rval_string(&mut cx, &frame, &out);
    true
}

/// `__wjs_atob(s)`：base64 → Latin-1 字符串；非法输入抛 InvalidCharacterError。
pub unsafe extern "C" fn atob_decode(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(s) = arg_string(&mut cx, &frame, 0, "atob") else {
        return false;
    };
    // forgiving-base64 前奏：删 ASCII 空白
    let clean: String = s.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    if clean.len() % 4 == 1 {
        report_error(&mut cx, "InvalidCharacterError: bad base64 length");
        return false;
    }
    match base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &clean) {
        Ok(bytes) => {
            let out: String = bytes.iter().map(|&b| b as char).collect();
            set_rval_string(&mut cx, &frame, &out);
            true
        }
        Err(_) => {
            report_error(&mut cx, "InvalidCharacterError: bad base64 input");
            false
        }
    }
}

/// `__wjs_te_encode(s)` → Uint8Array（UTF-8）。
pub unsafe extern "C" fn te_encode(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(s) = arg_string(&mut cx, &frame, 0, "TextEncoder.encode") else {
        return false;
    };
    rooted!(&in(cx) let mut obj: *mut JSObject = std::ptr::null_mut());
    // SAFETY: realm 内创建 Uint8Array；obj 为 rooted 出参；bytes 存活到调用返回
    let ok = unsafe {
        TypedArray::<Uint8, *mut JSObject>::create(&mut cx, CreateWith::Slice(s.as_bytes()), obj.handle_mut())
    };
    if ok.is_err() || obj.is_null() {
        report_error(&mut cx, "RangeError: cannot allocate Uint8Array");
        return false;
    }
    frame.set_rval(ObjectValue(obj.get()));
    true
}

/// `__wjs_te_encode_into(s, view)` → `{"read":utf16单位,"written":字节}` JSON。
pub unsafe extern "C" fn te_encode_into(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(s), view) = (
        arg_string(&mut cx, &frame, 0, "TextEncoder.encodeInto"),
        (frame.argc() > 1).then(|| frame.arg(1)),
    ) else {
        return false;
    };
    let Some(view) = view else {
        report_error(&mut cx, "TypeError: TextEncoder.encodeInto requires a destination view");
        return false;
    };
    let Some(mut bytes) = view_bytes(&mut cx, view, "TextEncoder.encodeInto") else {
        return false;
    };
    let cap = bytes.len();
    let mut read: usize = 0;
    let mut written: usize = 0;
    for c in s.chars() {
        let mut buf = [0u8; 4];
        let enc = c.encode_utf8(&mut buf);
        if written + enc.len() > cap {
            break;
        }
        bytes[written..written + enc.len()].copy_from_slice(enc.as_bytes());
        written += enc.len();
        read += c.len_utf16();
    }
    // 写回视图
    if !view.is_object() {
        report_error(&mut cx, "TypeError: TextEncoder.encodeInto requires a Uint8Array");
        return false;
    }
    // SAFETY: 上方 view_bytes 已校验为 Uint8Array
    let obj = view.to_object();
    if let Ok(mut arr) = TypedArray::<Uint8, *mut JSObject>::from(obj)
        && let Some(slot) = arr.as_mut_slice_safe(cx.no_gc_mut())
    {
        slot[..written].copy_from_slice(&bytes[..written]);
    }
    let json = format!(r#"{{"read":{read},"written":{written}}}"#);
    set_rval_string(&mut cx, &frame, &json);
    true
}

/// `__wjs_td_canonical(label)` → 规范编码名；未知抛 RangeError。
pub unsafe extern "C" fn td_canonical(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(label) = arg_string(&mut cx, &frame, 0, "TextDecoder") else {
        return false;
    };
    match encoding_rs::Encoding::for_label(label.trim().as_bytes()) {
        Some(enc) => {
            set_rval_string(&mut cx, &frame, enc.name());
            true
        }
        None => {
            report_error(&mut cx, &format!("RangeError: unknown encoding '{label}'"));
            false
        }
    }
}

/// `__wjs_td_decode(label, fatal, ignoreBOM, view)` → 解码字符串。
pub unsafe extern "C" fn td_decode(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(label), view) = (
        arg_string(&mut cx, &frame, 0, "TextDecoder.decode"),
        (frame.argc() > 3).then(|| frame.arg(3)),
    ) else {
        return false;
    };
    // fatal / ignoreBOM 走位置实参 1/2（prelude 传 0/1 数值）
    let flag = |i: u32| frame.argc() > i && {
        let v = frame.arg(i);
        v.is_number() && v.to_number() != 0.0 || v.is_boolean() && v.to_boolean()
    };
    let (fatal, ignore_bom) = (flag(1), flag(2));
    let Some(enc) = encoding_rs::Encoding::for_label(label.trim().as_bytes()) else {
        report_error(&mut cx, &format!("RangeError: unknown encoding '{label}'"));
        return false;
    };
    let Some(view) = view else {
        // 无输入 → 空串（规范行为）
        set_rval_string(&mut cx, &frame, "");
        return true;
    };
    // undefined/null 输入同样视为空（prelude 固定传 4 个实参）
    if view.is_undefined() || view.is_null() {
        set_rval_string(&mut cx, &frame, "");
        return true;
    }
    let Some(bytes) = view_bytes(&mut cx, view, "TextDecoder.decode") else {
        return false;
    };
    // 切片 a：非流式（stream:true 走 `__wjs_td_stream_*` 有状态解码器，下方）。
    let (text, had_errors) = if ignore_bom {
        let (t, e) = enc.decode_without_bom_handling(&bytes);
        (t, e)
    } else {
        let (t, _, e) = enc.decode(&bytes);
        (t, e)
    };
    if fatal && had_errors {
        report_error(&mut cx, &format!("TypeError: data is not valid {}", enc.name()));
        return false;
    }
    set_rval_string(&mut cx, &frame, &text);
    true
}

// ── TextDecoder 流式（有状态 Decoder；JS 侧按解码器实例持有 id）────────────
// 状态放线程本地堆（JS 线程独占，无 Send 需求；id 自增，last=1 自动回收，无泄漏路径）。
// encoding_rs 的 stateful 编码（ISO-2022-JP 等）只有增量 API 才对，prelude 切片不可行。
use std::cell::RefCell;
use std::collections::HashMap;

struct StreamDec {
    dec: encoding_rs::Decoder,
    fatal: bool,
    label: String,
}

thread_local! {
    static STREAM_DECODERS: RefCell<HashMap<u64, StreamDec>> = RefCell::new(HashMap::new());
    static STREAM_NEXT_ID: RefCell<u64> = RefCell::new(1);
}

/// 增量喂食（纯逻辑抽出，可单测）。两阶段：
/// 1) `last=false` 排空输入（截断序列由解码器内部缓存；零推进即停等下次 feed）；
/// 2) `last=true` 时空调一次，把缓存的截断落定为 FFFFD（或 fatal 抛）。
/// 背景（实测）：`decode([E2], last=false)` 回 `(InputEmpty, read=1)`——字节已进
/// 内部缓存；`decode([], last=true)` 才吐 `�`。单遍 `is_last` 算法必丢收尾（§4 记）。
fn stream_decode_chunk(
    dec: &mut encoding_rs::Decoder,
    mut src: &[u8],
    last: bool,
    fatal: bool,
    label: &str,
) -> Result<String, String> {
    let mut out = String::new();
    let mut extra = 0usize;
    // 阶段一：排空输入
    while !src.is_empty() {
        let need = dec
            .max_utf8_buffer_length(src.len())
            .unwrap_or_else(|| src.len().saturating_mul(3).saturating_add(32))
            .max(32)
            .saturating_add(extra);
        out.reserve(need);
        // `decode_to_string` 以 String 容量为输出上限且不重分配（reserve 已保证）。
        let (res, read, had_errors) = dec.decode_to_string(src, &mut out, false);
        src = &src[read..];
        if had_errors && fatal {
            return Err(format!("TypeError: data is not valid {label}"));
        }
        if read == 0 {
            match res {
                // 零推进 + 输入剩：OutputFull→加码重试（容量严格递增，必终止）；
                // InputEmpty→截断已缓存，等下次 feed（收尾走阶段二）。
                encoding_rs::CoderResult::OutputFull => {
                    extra = extra.saturating_add(need).max(64);
                    continue;
                }
                encoding_rs::CoderResult::InputEmpty => break,
            }
        }
    }
    // 阶段二：收尾落定
    if last {
        loop {
            out.reserve(32usize.saturating_add(extra));
            let (res, _, had_errors) = dec.decode_to_string(&[], &mut out, true);
            if had_errors && fatal {
                return Err(format!("TypeError: data is not valid {label}"));
            }
            match res {
                encoding_rs::CoderResult::InputEmpty => break,
                encoding_rs::CoderResult::OutputFull => {
                    extra = extra.saturating_add(64);
                }
            }
        }
    }
    Ok(out)
}

/// `__wjs_td_stream_open(label, fatalNum, ignoreBomNum)` → id（数值）。
pub unsafe extern "C" fn td_stream_open(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(label), fatal, ignore_bom) = (
        arg_string(&mut cx, &frame, 0, "TextDecoder"),
        frame.argc() > 1 && {
            let v = frame.arg(1);
            v.is_number() && v.to_number() != 0.0 || v.is_boolean() && v.to_boolean()
        },
        frame.argc() > 2 && {
            let v = frame.arg(2);
            v.is_number() && v.to_number() != 0.0 || v.is_boolean() && v.to_boolean()
        },
    ) else {
        return false;
    };
    let Some(enc) = encoding_rs::Encoding::for_label(label.trim().as_bytes()) else {
        report_error(&mut cx, &format!("RangeError: unknown encoding '{label}'"));
        return false;
    };
    // BOM 口径与一次性 `decode` 对齐（见 `td_decode`）。
    let dec = if ignore_bom {
        enc.new_decoder_without_bom_handling()
    } else {
        enc.new_decoder()
    };
    let id = STREAM_NEXT_ID.with(|n| {
        let mut n = n.borrow_mut();
        let id = *n;
        *n = n.wrapping_add(1).max(1);
        id
    });
    STREAM_DECODERS.with(|m| {
        m.borrow_mut().insert(id, StreamDec { dec, fatal, label: enc.name().into() });
    });
    rooted!(&in(cx) let mut v = UndefinedValue());
    (id as f64).to_jsval(&mut cx, v.handle_mut());
    frame.set_rval(v.get());
    true
}

/// `__wjs_td_stream_feed(idNum, viewU8?, lastNum)` → 字符串片；last=1 自动回收 id。
pub unsafe extern "C" fn td_stream_feed(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 || !frame.arg(0).is_number() {
        report_error(&mut cx, "TypeError: streaming decode needs a decoder");
        return false;
    }
    let id = frame.arg(0).to_number() as u64;
    let last = frame.argc() > 2 && {
        let v = frame.arg(2);
        v.is_number() && v.to_number() != 0.0 || v.is_boolean() && v.to_boolean()
    };
    let bytes: Vec<u8> = if frame.argc() > 1 {
        let v = frame.arg(1);
        if v.is_undefined() || v.is_null() {
            Vec::new()
        } else {
            match view_bytes(&mut cx, v, "TextDecoder.decode") {
                Some(b) => b,
                None => return false,
            }
        }
    } else {
        Vec::new()
    };
    // 状态取出→喂食→（非收尾写回；收尾/失败丢弃）。闭包内不调 JS，避免借用纠缠。
    enum Outcome {
        Text(String),
        Gone,
        Fatal(String),
    }
    let outcome = STREAM_DECODERS.with(|m| {
        let mut m = m.borrow_mut();
        let Some(st) = m.get_mut(&id) else {
            return Outcome::Gone;
        };
        let label = st.label.clone();
        match stream_decode_chunk(&mut st.dec, &bytes, last, st.fatal, &label) {
            Ok(t) => {
                if last {
                    m.remove(&id);
                }
                Outcome::Text(t)
            }
            Err(e) => {
                m.remove(&id);
                Outcome::Fatal(e)
            }
        }
    });
    match outcome {
        Outcome::Text(t) => {
            set_rval_string(&mut cx, &frame, &t);
            true
        }
        Outcome::Gone => {
            report_error(&mut cx, "OperationError: bad streaming decoder");
            false
        }
        Outcome::Fatal(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::stream_decode_chunk;

    #[test]
    fn split_multibyte_survives_stream() {
        // "€" = E2 82 AC 切两半：首片输出空（不提前 FFFD），尾片补全
        let mut dec = encoding_rs::UTF_8.new_decoder();
        assert_eq!(stream_decode_chunk(&mut dec, &[0xE2], false, false, "UTF-8").unwrap(), "");
        assert_eq!(
            stream_decode_chunk(&mut dec, &[0x82, 0xAC], true, false, "UTF-8").unwrap(),
            "€"
        );
    }

    #[test]
    fn truncation_at_end_replaces() {
        // 收尾时截断序列按规范变 FFFD（一次性 decode 同口径）
        let mut dec = encoding_rs::UTF_8.new_decoder();
        assert_eq!(stream_decode_chunk(&mut dec, &[0xE2], true, false, "UTF-8").unwrap(), "�");
    }

    #[test]
    fn fatal_rejects_garbage() {
        let mut dec = encoding_rs::UTF_8.new_decoder();
        assert!(stream_decode_chunk(&mut dec, &[0xFF], true, true, "UTF-8").is_err());
        let mut dec = encoding_rs::UTF_8.new_decoder();
        assert_eq!(stream_decode_chunk(&mut dec, &[0xFF], true, false, "UTF-8").unwrap(), "�");
    }

    #[test]
    fn stateful_encoding_streams() {
        // GBK "你" = C4 E3：跨片增量解码（一次性切片做不到）
        let mut dec = encoding_rs::GBK.new_decoder();
        assert_eq!(stream_decode_chunk(&mut dec, &[0xC4], false, false, "GBK").unwrap(), "");
        assert_eq!(
            stream_decode_chunk(&mut dec, &[0xE3], true, false, "GBK").unwrap(),
            "你"
        );
    }
}
