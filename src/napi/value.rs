//! napi 值系统（plan-napi M1）：建值/读值/coerce/类型判定 + 错误对象族。
//! JSAPI 映射：coerce 走 `js::*Slow`（JS::ToNumber 的实现本体）；
//! 错误对象经 global 的 `Error`/`TypeError`/`RangeError` 构造（不引新机制）；
//! 数组建走 `JS::NewArrayObject1`（元素用 `JS_SetElement` 逐个落，天然 rooted）。

use std::ffi::{c_char, CStr};

use mozjs::jsapi::{JS_ClearPendingException, JS_GetPendingException, JS_IsExceptionPending, JS_NewPlainObject, JS_SetPendingException};
use mozjs::jsapi::JSObject;
use mozjs::jsval::{ObjectValue, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{call_one, get_prop_value, raw_handle, raw_handle_mut};
use crate::napi::api::{cx_of, e};
use crate::napi::sys;
use crate::napi::sys::{
    napi_env, napi_status, napi_value,
};

const NAPI_OK: napi_status = sys::napi_status_napi_ok;
const NAPI_INVALID_ARG: napi_status = sys::napi_status_napi_invalid_arg;

// ── 建值：对象/数组/字符串/symbol ────────────────────────────────────────

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_object(env: napi_env, result: *mut napi_value) -> napi_status {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    let env_ref = unsafe { e(env) };
    // SAFETY：对象先 rooted 再落槽（建值与落槽间可能 GC）。
    unsafe {
        let obj = JS_NewPlainObject(cx.raw_cx());
        if obj.is_null() {
            return sys::napi_status_napi_generic_failure;
        }
        rooted!(&in(cx) let root: *mut JSObject = obj);
        *result = env_ref.put(ObjectValue(root.get()));
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_array(env: napi_env, result: *mut napi_value) -> napi_status {
    // SAFETY：同 create_array_with_length（length=0 同一路径）。
    unsafe { napi_create_array_with_length(env, 0, result) }
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_array_with_length(
    env: napi_env,
    length: usize,
    result: *mut napi_value,
) -> napi_status {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    let env_ref = unsafe { e(env) };
    // SAFETY：同 create_object（rooted 后落槽）。
    unsafe {
        let obj = mozjs::jsapi::JS::NewArrayObject1(cx.raw_cx(), length);
        if obj.is_null() {
            return sys::napi_status_napi_generic_failure;
        }
        rooted!(&in(cx) let root: *mut JSObject = obj);
        *result = env_ref.put(ObjectValue(root.get()));
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_string_utf16(
    env: napi_env,
    str_: *const u16,
    length: usize,
    result: *mut napi_value,
) -> napi_status {
    if result.is_null() || str_.is_null() {
        return NAPI_INVALID_ARG;
    }
    // SAFETY：addon 保证长度语义内可读；JSString 建成即取 Value 落槽
    //（StringValue 为位转换，无 GC 点）。
    unsafe {
        let chars = if length == usize::MAX {
            let mut n = 0usize;
            while *str_.add(n) != 0 {
                n += 1;
            }
            n
        } else {
            length
        };
        let mut cx = cx_of(env);
        let s = mozjs::jsapi::JS_NewUCStringCopyN(cx.raw_cx(), str_, chars);
        if s.is_null() {
            return sys::napi_status_napi_generic_failure;
        }
        *result = e(env).put(mozjs::jsval::StringValue(&*s));
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（latin1 按字节无失真拓宽为 UCS2）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_string_latin1(
    env: napi_env,
    str_: *const c_char,
    length: usize,
    result: *mut napi_value,
) -> napi_status {
    if result.is_null() || str_.is_null() {
        return NAPI_INVALID_ARG;
    }
    // SAFETY：长度语义内可读。
    unsafe {
        let bytes = if length == usize::MAX {
            CStr::from_ptr(str_).to_bytes()
        } else {
            std::slice::from_raw_parts(str_.cast::<u8>(), length)
        };
        let wide: Vec<u16> = bytes.iter().map(|b| u16::from(*b)).collect();
        napi_create_string_utf16(env, wide.as_ptr(), wide.len(), result)
    }
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_symbol(
    env: napi_env,
    description: napi_value,
    result: *mut napi_value,
) -> napi_status {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    let env_ref = unsafe { e(env) };
    // SAFETY：描述串可空；Symbol 经 global Symbol() 构造（desc 非 string 时
    // napi 语义为传描述值 —— 真机只收 string，此处按 Symbol(desc) 直调）。
    unsafe {
        let desc_v = if description.is_null() {
            UndefinedValue()
        } else {
            env_ref.get(description)
        };
        rooted!(&in(cx) let desc_root = desc_v);
        let Some(sym_ctor) = get_prop_value(&mut cx, crate::state::global(), c"Symbol") else {
            return sys::napi_status_napi_generic_failure;
        };
        // SAFETY：global 上取到的构造器；this=global 的单参调用（call_one 前置）。
        let Some(sym_v) = call_one(&mut cx, crate::state::global(), sym_ctor, desc_root.get())
        else {
            return sys::napi_status_napi_generic_failure;
        };
        *result = env_ref.put(sym_v);
    }
    NAPI_OK
}

// ── 读值：字符串 utf16/latin1 ────────────────────────────────────────────

/// # Safety
/// N-API 约定（buf==NULL → result=长度；否则截断 + NUL，单位 = u16）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_value_string_utf16(
    env: napi_env,
    value: napi_value,
    buf: *mut u16,
    bufsize: usize,
    result: *mut usize,
) -> napi_status {
    if value.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(value) };
    if !v.is_string() {
        return sys::napi_status_napi_string_expected;
    }
    // SAFETY：AutoRequireNoGC 令牌为空结构；串在值槽位内存活。
    unsafe {
        let mut cx = cx_of(env);
        let s = v.to_string();
        let nogc = mozjs::jsapi::JS::AutoRequireNoGC { _address: 0 };
        let mut len = 0usize;
        let chars = mozjs::jsapi::JS_GetTwoByteStringCharsAndLength(
            cx.raw_cx(),
            &nogc,
            s,
            &mut len,
        );
        if chars.is_null() {
            return sys::napi_status_napi_generic_failure;
        }
        if buf.is_null() {
            if !result.is_null() {
                *result = len;
            }
            return NAPI_OK;
        }
        let n = bufsize.saturating_sub(1).min(len);
        std::ptr::copy_nonoverlapping(chars, buf, n);
        *buf.add(n) = 0;
        if !result.is_null() {
            *result = n;
        }
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（latin1 按字节读；单位 = 字节）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_value_string_latin1(
    env: napi_env,
    value: napi_value,
    buf: *mut c_char,
    bufsize: usize,
    result: *mut usize,
) -> napi_status {
    if value.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(value) };
    if !v.is_string() {
        return sys::napi_status_napi_string_expected;
    }
    // SAFETY：同 utf16（Latin1 通道）。
    unsafe {
        let mut cx = cx_of(env);
        let s = v.to_string();
        let nogc = mozjs::jsapi::JS::AutoRequireNoGC { _address: 0 };
        let mut len = 0usize;
        let chars = mozjs::jsapi::JS_GetLatin1StringCharsAndLength(
            cx.raw_cx(),
            &nogc,
            s,
            &mut len,
        );
        if chars.is_null() {
            return sys::napi_status_napi_generic_failure;
        }
        if buf.is_null() {
            if !result.is_null() {
                *result = len;
            }
            return NAPI_OK;
        }
        let n = bufsize.saturating_sub(1).min(len);
        std::ptr::copy_nonoverlapping(chars.cast::<c_char>(), buf, n);
        *buf.add(n) = 0;
        if !result.is_null() {
            *result = n;
        }
    }
    NAPI_OK
}

// ── coerce / 类型判定 / 等值 ─────────────────────────────────────────────

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_coerce_to_bool(
    env: napi_env,
    value: napi_value,
    result: *mut napi_value,
) -> napi_status {
    if value.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(value) };
    let cx = unsafe { cx_of(env) };
    let env_ref = unsafe { e(env) };
    // SAFETY：ToBoolean 是无副作用谓词（结果落 BooleanValue 槽位）。
    unsafe {
        rooted!(&in(cx) let v_root = v);
        *result = env_ref.put(mozjs::jsval::BooleanValue(mozjs::jsapi::js::ToBooleanSlow(
            raw_handle(v_root.as_ptr()),
        )));
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_coerce_to_number(
    env: napi_env,
    value: napi_value,
    result: *mut napi_value,
) -> napi_status {
    if value.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(value) };
    let mut cx = unsafe { cx_of(env) };
    let env_ref = unsafe { e(env) };
    // SAFETY：ToNumber 可抛（coerce 失败 → pending exception，返回 generic_failure，
    // addon 按契约查 pending）；v 为槽位值。
    unsafe {
        rooted!(&in(cx) let v_root = v);
        let mut d = 0f64;
        if !mozjs::jsapi::js::ToNumberSlow(cx.raw_cx(), raw_handle(v_root.as_ptr()), &mut d) {
            return sys::napi_status_napi_generic_failure;
        }
        *result = env_ref.put(mozjs::jsval::DoubleValue(d));
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_coerce_to_string(
    env: napi_env,
    value: napi_value,
    result: *mut napi_value,
) -> napi_status {
    if value.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(value) };
    let mut cx = unsafe { cx_of(env) };
    let env_ref = unsafe { e(env) };
    // SAFETY：ToString 可抛（语义同 coerce_to_number 注）。
    unsafe {
        rooted!(&in(cx) let v_root = v);
        let s = mozjs::jsapi::js::ToStringSlow(cx.raw_cx(), raw_handle(v_root.as_ptr()));
        if s.is_null() {
            return sys::napi_status_napi_generic_failure;
        }
        *result = env_ref.put(mozjs::jsval::StringValue(&*s));
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_coerce_to_object(
    env: napi_env,
    value: napi_value,
    result: *mut napi_value,
) -> napi_status {
    if value.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(value) };
    // 对象直返（js::ToObjectSlow 断言 !isObject——MOZ_ASSERT 实测炸，2026-09-13）。
    if v.is_object() {
        // SAFETY：result 为 addon 提供的合法出参。
        unsafe { *result = e(env).put(v) };
        return NAPI_OK;
    }
    let mut cx = unsafe { cx_of(env) };
    let env_ref = unsafe { e(env) };
    // SAFETY：ToObjectSlow 仅收原始值（可抛：null/undefined 报 TypeError）。
    unsafe {
        rooted!(&in(cx) let v_root = v);
        let obj = mozjs::jsapi::js::ToObjectSlow(cx.raw_cx(), raw_handle(v_root.as_ptr()), true);
        if obj.is_null() {
            return sys::napi_status_napi_generic_failure;
        }
        rooted!(&in(cx) let root: *mut JSObject = obj);
        *result = env_ref.put(ObjectValue(root.get()));
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_instanceof(
    env: napi_env,
    object: napi_value,
    constructor: napi_value,
    result: *mut bool,
) -> napi_status {
    if object.is_null() || constructor.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let env_ref = unsafe { e(env) };
    let obj_v = unsafe { env_ref.get(object) };
    let ctor_v = unsafe { env_ref.get(constructor) };
    if !ctor_v.is_object() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：JS_HasInstance 可抛；v/ctor 为槽位值。
    unsafe {
        rooted!(&in(cx) let obj_root = obj_v);
        rooted!(&in(cx) let ctor_root = ctor_v.to_object());
        let mut is = false;
        if !mozjs::jsapi::JS_HasInstance(
            cx.raw_cx(),
            raw_handle(&ctor_root.get()),
            raw_handle(obj_root.as_ptr()),
            &mut is,
        ) {
            return sys::napi_status_napi_generic_failure;
        }
        *result = is;
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_strict_equals(
    env: napi_env,
    lhs: napi_value,
    rhs: napi_value,
    result: *mut bool,
) -> napi_status {
    if lhs.is_null() || rhs.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let a = unsafe { e(env).get(lhs) };
    let b = unsafe { e(env).get(rhs) };
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：StrictlyEqual 无副作用谓词。
    unsafe {
        rooted!(&in(cx) let a_root = a);
        rooted!(&in(cx) let b_root = b);
        let mut eq = false;
        if !mozjs::jsapi::JS::StrictlyEqual(cx.raw_cx(), raw_handle(a_root.as_ptr()), raw_handle(b_root.as_ptr()), &mut eq) {
            return sys::napi_status_napi_generic_failure;
        }
        *result = eq;
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_is_array(
    env: napi_env,
    value: napi_value,
    result: *mut bool,
) -> napi_status {
    if value.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(value) };
    if !v.is_object() {
        // SAFETY：出参直写。
        unsafe { *result = false };
        return NAPI_OK;
    }
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：IsArrayObject 无副作用谓词（收 HandleObject）。
    unsafe {
        rooted!(&in(cx) let obj_root = v.to_object());
        let mut is = false;
        if !mozjs::jsapi::JS::IsArrayObject1(cx.raw_cx(), raw_handle(&obj_root.get()), &mut is) {
            return sys::napi_status_napi_generic_failure;
        }
        *result = is;
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_is_error(env: napi_env, value: napi_value, result: *mut bool) -> napi_status {
    unsafe {
    if value.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = e(env).get(value);
    if !v.is_object() {
        *result = false;
        return NAPI_OK;
    }
    // 偏差（记 plan-napi §4）：用 `instanceof Error` 近似（JS_IsErrorObject
    // 未入绑定；Error 子类/原型挂 Error.prototype 的对象同判真）。
    let mut cx = cx_of(env);
    // SAFETY：槽位值 + global Error 构造器（call 面同 create_error）。
    {
        rooted!(&in(cx) let v_root = v);
        let Some(err_ctor) = get_prop_value(&mut cx, crate::state::global(), c"Error") else {
            return sys::napi_status_napi_generic_failure;
        };
        rooted!(&in(cx) let ctor_root = err_ctor.to_object());
        let mut is = false;
        if !mozjs::jsapi::JS_HasInstance(cx.raw_cx(), raw_handle(&ctor_root.get()), raw_handle(v_root.as_ptr()), &mut is) {
            return sys::napi_status_napi_generic_failure;
        }
        *result = is;
    }
    NAPI_OK
    }
}

// ── 错误对象族（create + throw；经 global 构造器直调）───────────────────

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_error(
    env: napi_env,
    code: napi_value,
    msg: napi_value,
    result: *mut napi_value,
) -> napi_status {
    // SAFETY：同族建错误（create_error_impl 内部 rooted）。
    unsafe { create_error_impl(env, c"Error", code, msg, result) }
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_type_error(
    env: napi_env,
    code: napi_value,
    msg: napi_value,
    result: *mut napi_value,
) -> napi_status {
    // SAFETY：同族建错误。
    unsafe { create_error_impl(env, c"TypeError", code, msg, result) }
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_range_error(
    env: napi_env,
    code: napi_value,
    msg: napi_value,
    result: *mut napi_value,
) -> napi_status {
    // SAFETY：同族建错误。
    unsafe { create_error_impl(env, c"RangeError", code, msg, result) }
}

/// # Safety
/// `name` 为静态 CStr（c"Error" 等）；code/msg 为 napi_value（可空）。
unsafe fn create_error_impl(
    env: napi_env,
    name: &CStr,
    code: napi_value,
    msg: napi_value,
    result: *mut napi_value,
) -> napi_status {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let env_ref = unsafe { e(env) };
    let msg_v = if msg.is_null() {
        UndefinedValue()
    } else {
        unsafe { env_ref.get(msg) }
    };
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：global 构造器单参调用（call_one 前置；Error 系可无 new 构造）；
    // code 非空即挂 `code` 属性（N-API 语义）。
    unsafe {
        let Some(ctor) = get_prop_value(&mut cx, crate::state::global(), name) else {
            return sys::napi_status_napi_generic_failure;
        };
        rooted!(&in(cx) let msg_root = msg_v);
        let Some(err_v) = call_one(&mut cx, crate::state::global(), ctor, msg_root.get()) else {
            return sys::napi_status_napi_generic_failure;
        };
        rooted!(&in(cx) let err_root = err_v.to_object());
        if !code.is_null() {
            let code_v = env_ref.get(code);
            rooted!(&in(cx) let code_root = code_v);
            if !mozjs::jsapi::JS_SetProperty(
                cx.raw_cx(),
                raw_handle(&err_root.get()),
                c"code".as_ptr(),
                raw_handle(code_root.as_ptr()),
            ) {
                return sys::napi_status_napi_generic_failure;
            }
        }
        *result = env_ref.put(ObjectValue(err_root.get()));
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（throw 后回调返回 NULL → trampoline 传播）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_throw(env: napi_env, error: napi_value) -> napi_status {
    if error.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(error) };
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：槽位值；SetPendingException 为标准传播路径。
    unsafe {
        rooted!(&in(cx) let v_root = v);
        JS_SetPendingException(cx.raw_cx(), raw_handle(v_root.as_ptr()), mozjs::jsapi::JS::ExceptionStackBehavior::Capture);
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_throw_type_error(
    env: napi_env,
    _code: *const c_char,
    msg: *const c_char,
) -> napi_status {
    // SAFETY：throw_* 收 char*（js_native_api.h:403）；建 msg 值后复用 create+throw。
    unsafe {
        let mut msg_v: napi_value = std::ptr::null_mut();
        if crate::napi::api::napi_create_string_utf8(env, msg, usize::MAX, &mut msg_v) != sys::napi_status_napi_ok {
            return sys::napi_status_napi_generic_failure;
        }
        let mut out: napi_value = std::ptr::null_mut();
        let st = napi_create_type_error(env, std::ptr::null_mut(), msg_v, &mut out);
        if st != sys::napi_status_napi_ok || out.is_null() {
            return st;
        }
        napi_throw(env, out)
    }
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_throw_range_error(
    env: napi_env,
    _code: *const c_char,
    msg: *const c_char,
) -> napi_status {
    // SAFETY：同 throw_type_error。
    unsafe {
        let mut msg_v: napi_value = std::ptr::null_mut();
        if crate::napi::api::napi_create_string_utf8(env, msg, usize::MAX, &mut msg_v) != sys::napi_status_napi_ok {
            return sys::napi_status_napi_generic_failure;
        }
        let mut out: napi_value = std::ptr::null_mut();
        let st = napi_create_range_error(env, std::ptr::null_mut(), msg_v, &mut out);
        if st != sys::napi_status_napi_ok || out.is_null() {
            return st;
        }
        napi_throw(env, out)
    }
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_is_exception_pending(env: napi_env, result: *mut bool) -> napi_status {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：谓词。
    unsafe { *result = JS_IsExceptionPending(cx.raw_cx()) };
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_and_clear_last_exception(
    env: napi_env,
    result: *mut napi_value,
) -> napi_status {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    let env_ref = unsafe { e(env) };
    // SAFETY：标准 pending 消费路径（Get + Clear）。
    unsafe {
        if !JS_IsExceptionPending(cx.raw_cx()) {
            *result = env_ref.put(UndefinedValue());
            return NAPI_OK;
        }
        rooted!(&in(cx) let mut exc = UndefinedValue());
        if !JS_GetPendingException(cx.raw_cx(), raw_handle_mut(exc.as_ptr())) {
            JS_ClearPendingException(cx.raw_cx());
            return sys::napi_status_napi_generic_failure;
        }
        JS_ClearPendingException(cx.raw_cx());
        *result = env_ref.put(exc.get());
    }
    NAPI_OK
}
