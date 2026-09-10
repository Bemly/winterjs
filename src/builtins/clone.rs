//! structuredClone：serde_json 中转（Phase 1 支持 JSON 兼容值：原始值/纯对象/数组）。
//! 已知偏差（文档记录）：顶层 undefined → undefined（直通）；对象属性里的 undefined
//! 按 JSON 语义丢弃；Map/Set/Date 等非纯对象 Phase 1 不支持（DataCloneError）；
//! NaN/±Infinity → DataCloneError（JSON 无法表示）。

use mozjs::context::JSContext;
use mozjs::gc::ValueArray;
use mozjs::jsapi::{HandleValueArray, JS_GetElement, JS_ParseJSON, JSObject};
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{raw_handle, raw_handle_mut, wrap_cx};

use crate::jsapi_glue::{report_error, value_to_string, Frame};
use crate::state;

const MAX_DEPTH: usize = 100;

fn data_clone_error(cx: &mut JSContext, what: &str) -> bool {
    report_error(cx, &format!("DataCloneError: {what}"));
    false
}

/// JS 值 → serde_json::Value（仅 JSON 兼容子集）。
fn to_json(cx: &mut JSContext, v: JSVal, depth: usize) -> Result<serde_json::Value, String> {
    if depth > MAX_DEPTH {
        return Err("structure too deeply nested".into());
    }
    if v.is_null() {
        return Ok(serde_json::Value::Null);
    }
    if v.is_boolean() {
        return Ok(serde_json::Value::Bool(v.to_boolean()));
    }
    if v.is_int32() {
        return Ok(serde_json::json!(v.to_int32()));
    }
    if v.is_double() {
        let n = v.to_number();
        return serde_json::Number::from_f64(n)
            .map(serde_json::Value::Number)
            .ok_or_else(|| "NaN/Infinity cannot be cloned".into());
    }
    if v.is_string() {
        return Ok(serde_json::Value::String(value_to_string(cx, v)));
    }
    if v.is_object() {
        // SAFETY: is_object 已判定；to_object 返回引擎对象指针
        let obj = unsafe { v.to_object() };
        rooted!(&in(cx) let obj_root: *mut JSObject = obj);
        rooted!(&in(cx) let val_root = v);

        // 数组？（IsArrayObject 收 Handle<Value>）
        let mut is_array = false;
        // SAFETY: val_root 为有效 rooted 值；raw 调用不触发 GC
        let ok =
            unsafe { mozjs::jsapi::IsArrayObject(cx.raw_cx(), raw_handle(val_root.as_ptr()), &mut is_array) };
        if !ok {
            return Err("IsArrayObject failed".into());
        }

        if is_array {
            let mut len = 0u32;
            // SAFETY: 同上
            let ok = unsafe { mozjs::jsapi::GetArrayLength(cx.raw_cx(), raw_handle(obj_root.as_ptr()), &mut len) };
            if !ok {
                return Err("GetArrayLength failed".into());
            }
            let mut out = Vec::with_capacity(len as usize);
            for i in 0..len {
                rooted!(&in(cx) let mut elem = UndefinedValue());
                // SAFETY: i < len
                let ok = unsafe { JS_GetElement(cx.raw_cx(), raw_handle(obj_root.as_ptr()), i, raw_handle_mut(elem.as_ptr())) };
                if !ok {
                    return Err("GetElement failed".into());
                }
                if elem.is_undefined() {
                    out.push(serde_json::Value::Null);
                } else {
                    out.push(to_json(cx, elem.get(), depth + 1)?);
                }
            }
            return Ok(serde_json::Value::Array(out));
        }

        // 纯对象：Object.entries 经 prelude 辅助拿 [key, value] 数组（绕开 IdVector）
        let call_entries = state::with_rooted(|s| s.entries_fn.get());
        if call_entries.is_undefined() {
            return Err("__wjs_entries unavailable (prelude missing?)".into());
        }
        rooted!(&in(cx) let fun = call_entries);
        // 单实参：用 Handle<Value> 直构（val_root 已 rooted）；
        // 不用 Rooted<ValueArray>——见 AGENTS §4.9（其根注册在 native 内调用时 SEGV）
        let args = HandleValueArray::from(unsafe { raw_handle(val_root.as_ptr()) });
        rooted!(&in(cx) let mut pair_rval = UndefinedValue());
        // thisObj 必须是有效对象（null 传给 JS_CallFunctionValue 会 SEGV）——用 global
        let g = state::global();
        rooted!(&in(cx) let g_root: *mut JSObject = g);
        // SAFETY: fun 为有效可调用值；rval 为 rooted 出参
        let ok = unsafe {
            mozjs::jsapi::JS_CallFunctionValue(
                cx.raw_cx(),
                raw_handle(g_root.as_ptr()),
                raw_handle(fun.as_ptr()),
                &args,
                raw_handle_mut(pair_rval.as_ptr()),
            )
        };
        if !ok || !pair_rval.is_object() {
            return Err("entries enumeration failed".into());
        }
        let entries_obj = unsafe { pair_rval.to_object() };
        rooted!(&in(cx) let entries_root: *mut JSObject = entries_obj);
        let mut len = 0u32;
        // SAFETY: entries_root 有效
        let ok =
            unsafe { mozjs::jsapi::GetArrayLength(cx.raw_cx(), raw_handle(entries_root.as_ptr()), &mut len) };
        if !ok {
            return Err("entries length failed".into());
        }
        let mut map = serde_json::Map::new();
        for i in 0..len {
            rooted!(&in(cx) let mut pair = UndefinedValue());
            // SAFETY: i < len
            let ok = unsafe {
                JS_GetElement(cx.raw_cx(), raw_handle(entries_root.as_ptr()), i, raw_handle_mut(pair.as_ptr()))
            };
            if !ok || !pair.is_object() {
                return Err("entry pair failed".into());
            }
            let pair_obj = unsafe { pair.to_object() };
            rooted!(&in(cx) let pair_root: *mut JSObject = pair_obj);
            rooted!(&in(cx) let mut key_v = UndefinedValue());
            rooted!(&in(cx) let mut val_v = UndefinedValue());
            // SAFETY: 索引 0/1 恒在 pair 内
            unsafe {
                let ok0 = JS_GetElement(cx.raw_cx(), raw_handle(pair_root.as_ptr()), 0, raw_handle_mut(key_v.as_ptr()));
                let ok1 = JS_GetElement(cx.raw_cx(), raw_handle(pair_root.as_ptr()), 1, raw_handle_mut(val_v.as_ptr()));
                if !ok0 || !ok1 {
                    // 提前返回走错误路径
                    return Err("entry destructure failed".into());
                }
            }
            if !key_v.is_string() || val_v.is_undefined() {
                continue; // symbol 键/undefined 值按 JSON 语义跳过
            }
            let key = value_to_string(cx, key_v.get());
            let value = to_json(cx, val_v.get(), depth + 1)?;
            map.insert(key, value);
        }
        return Ok(serde_json::Value::Object(map));
    }
    if v.is_undefined() {
        return Err("undefined cannot be cloned".into());
    }
    Err("value type not supported (symbol/bigint/function/etc.)".into())
}

/// SAFETY: 由引擎以有效调用帧调用；arg0 为待克隆值。
pub unsafe extern "C" fn structured_clone(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = wrap_cx(cx_raw);
    let frame = Frame { vp, argc };
    if argc < 1 {
        report_error(&mut cx, "TypeError: structuredClone requires an argument");
        return false;
    }
    let input = frame.arg(0);

    // 顶层 undefined 直通（规范行为；JSON 无法表示）
    if input.is_undefined() {
        frame.set_rval(UndefinedValue());
        return true;
    }

    let json = match to_json(&mut cx, input, 0) {
        Ok(j) => j,
        Err(why) => return data_clone_error(&mut cx, &why),
    };
    let text = match serde_json::to_string(&json) {
        Ok(t) => t,
        Err(e) => return data_clone_error(&mut cx, &format!("serialization failed: {e}")),
    };
    let utf16: Vec<u16> = text.encode_utf16().collect();

    rooted!(&in(cx) let mut out = UndefinedValue());
    // SAFETY: utf16 存活到调用返回；out 为 rooted 出参
    let ok = unsafe {
        JS_ParseJSON(cx.raw_cx(), utf16.as_ptr(), utf16.len() as u32, raw_handle_mut(out.as_ptr()))
    };
    if !ok {
        return false; // ParseJSON 已置异常
    }
    frame.set_rval(out.get());
    true
}
