//! JSNative 调用帧与常用 JSAPI 便捷封装。
//! 本模块属 mozjs 边界（AGENTS §6 允许 unsafe 的区域）；每处 unsafe 注明前置条件。

use std::ffi::{CStr, CString};

use mozjs::conversions::{ConversionResult, FromJSValConvertible as _};
use mozjs::context::JSContext;
use mozjs::jsapi::JSObject;
use mozjs::jsval::JSVal;
use mozjs::rooted;

/// rust Handle 指针位置 → 裸 jsapi Handle（同一标记位置直拷）。
///
/// # Safety
/// 源 Rooted 须存活到 raw Handle 的使用结束（本项目的调用点都在同一作用域内）。
pub unsafe fn raw_handle<T>(src: *const T) -> mozjs::jsapi::Handle<T> {
    mozjs::jsapi::Handle {
        _phantom_0: std::marker::PhantomData,
        ptr: src,
    }
}

/// # Safety
/// 同 [`raw_handle`]。
pub unsafe fn raw_handle_mut<T>(src: *mut T) -> mozjs::jsapi::MutableHandle<T> {
    mozjs::jsapi::MutableHandle {
        _phantom_0: std::marker::PhantomData,
        ptr: src,
    }
}

use crate::error::Error;
use crate::state;

/// 引擎回调的 raw cx → wrapper（文档许可：回调提供的 RawJSContext 可安全构造 wrapper）。
///
/// # Safety
/// `cx_raw` 必须是引擎回调给出的有效指针，且仅在 JS 线程使用。
pub unsafe fn wrap_cx(cx_raw: *mut mozjs::jsapi::JSContext) -> JSContext {
    JSContext::from_ptr(std::ptr::NonNull::new_unchecked(cx_raw))
}

/// JSNative 调用帧布局（JSAPI 约定）：vp[0]=callee/返回值槽（复用），vp[1]=this，vp[2..]=实参。
#[derive(Clone, Copy)]
pub struct Frame {
    pub vp: *mut JSVal,
    pub argc: u32,
}

impl Frame {
    /// SAFETY: i < argc 且 vp 指向引擎提供的调用帧。
    pub unsafe fn arg(&self, i: u32) -> JSVal {
        debug_assert!(i < self.argc, "arg index out of range");
        *self.vp.add(2 + i as usize)
    }

    /// SAFETY: vp 指向引擎提供的调用帧（写入即设置返回值）。
    pub unsafe fn set_rval(&self, v: JSVal) {
        *self.vp = v;
    }
}

/// ToString 语义取字符串。ToString 抛异常时清掉 pending exception 并给出占位串
/// （console.log 的参数转换不应让脚本爆炸，Phase 1 简化处理）。
pub fn value_to_string(cx: &mut JSContext, v: JSVal) -> String {
    rooted!(&in(cx) let val = v);
    match unsafe { String::from_jsval(cx, val.handle(), ()) } {
        Ok(ConversionResult::Success(s)) => s,
        Ok(ConversionResult::Failure(_)) => "<unstringifiable>".into(),
        Err(_) => {
            // SAFETY: 仅在上一步确有 pending exception 时调用
            unsafe { mozjs::jsapi::JS_ClearPendingException(cx.raw_cx()) };
            "<error>".into()
        }
    }
}

/// 读字符串属性（不存在/非串返回 None；读属性本身不该抛）。
pub fn get_prop_string(
    cx: &mut JSContext,
    obj: *mut JSObject,
    name: &CStr,
) -> Option<String> {
    rooted!(&in(cx) let mut v = mozjs::jsval::UndefinedValue());
    // SAFETY: cx 为有效 wrapper；标记位置指针直拷；raw 调用不触发 GC
    let ok = unsafe {
        mozjs::jsapi::JS_GetProperty(
            cx.raw_cx(),
            raw_handle(&obj),
            name.as_ptr(),
            raw_handle_mut(v.as_ptr()),
        )
    };
    if ok && v.is_string() {
        Some(value_to_string(cx, v.get()))
    } else {
        None
    }
}

/// 读数值属性（u32 口径；不存在/非数返回 None）。
pub fn get_prop_u32(
    cx: &mut JSContext,
    obj: *mut JSObject,
    name: &CStr,
) -> Option<u32> {
    rooted!(&in(cx) let mut v = mozjs::jsval::UndefinedValue());
    // SAFETY: cx 为有效 wrapper；标记位置指针直拷；raw 调用不触发 GC
    let ok = unsafe {
        mozjs::jsapi::JS_GetProperty(
            cx.raw_cx(),
            raw_handle(&obj),
            name.as_ptr(),
            raw_handle_mut(v.as_ptr()),
        )
    };
    if ok && v.is_number() {
        Some(v.to_number() as u32)
    } else {
        None
    }
}

/// 向引擎上报错误（JS_ReportErrorASCII 是 printf 风格，需转义 %）。
pub fn report_error(cx: &mut JSContext, msg: &str) {
    let escaped = msg.replace('%', "%%");
    let Ok(c) = CString::new(escaped) else {
        return;
    };
    // SAFETY: c 活到调用返回；格式串不含未转义的 %。
    unsafe { mozjs::jsapi::JS_ReportErrorASCII(cx.raw_cx(), c.as_ptr()) };
}

/// 检查异常值的 `name` 属性（如 "SyntaxError"）。非对象/无 name 返回 false。
pub fn exc_name_is(cx: &mut JSContext, exc: JSVal, name: &str) -> bool {
    if !exc.is_object() {
        return false;
    }
    // SAFETY: is_object 已判定
    let obj = unsafe { exc.to_object() };
    match get_prop_string(cx, obj, c"name") {
        Some(n) => n == name,
        None => false,
    }
}

/// 把当前 pending exception 转成 Error::Script（错误路径统一入口）。
/// 前置条件：evaluate_script / JS_CallFunctionValue 刚返回 false。
pub fn pending_exception_error(
    cx: &mut JSContext,
    global: *mut JSObject,
    source: &str,
    filename: &str,
) -> Error {
    // §4.1：evaluate_script/回调返回后已不在 realm，必须重进再调 JSAPI。
    // SAFETY: global 由调用方的 rooted! 保活；Handle 仅指向其标记位置
    let mut realm = mozjs::realm::AutoRealm::new_from_handle(
        cx,
        unsafe { mozjs::gc::Handle::from_marked_location(&global) },
    );
    rooted!(&in(&mut realm) let mut exc = mozjs::jsval::UndefinedValue());
    match unsafe {
        mozjs::rust::error_info_from_exception_stack(&mut realm, exc.handle_mut())
    } {
        Some(info) => {
            let line = info.line.saturating_sub(state::line_adjust());
            Error::script(filename, source, line, info.col, info.message)
        }
        None => Error::Other("uncaught JS exception (no stack info)".into()),
    }
}
