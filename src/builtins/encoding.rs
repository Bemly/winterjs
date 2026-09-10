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
    // 切片 a：非流式（stream:true 由 prelude 拦截，见 PRELUDE）。
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
