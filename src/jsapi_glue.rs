//! JSNative 调用帧与常用 JSAPI 便捷封装。
//! 本模块是项目的 unsafe 集中地（AGENTS §6）：除 hooks/jobqueue/state 的引擎协议
//! 代码外，所有裸 JSAPI 调用必须收敛到本模块的封装函数；每处 unsafe 注明前置条件，
//! 并用 `UNSAFE-BOUNDARY` 标签登记覆盖测试（黑盒重点）。

use std::ffi::{CStr, CString};

use mozjs::conversions::{ConversionResult, FromJSValConvertible as _, ToJSValConvertible as _};
use mozjs::context::JSContext;
use mozjs::gc::ValueArray;
use mozjs::jsapi::{HandleValueArray, JS_CallFunctionValue, JSObject};
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;
use mozjs::typedarray::{CreateWith, TypedArray, Uint8};

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
pub unsafe fn wrap_cx(cx_raw: *mut mozjs::jsapi::JSContext) -> JSContext { unsafe {
    JSContext::from_ptr(std::ptr::NonNull::new_unchecked(cx_raw))
}}

/// JSNative 调用帧布局（JSAPI 约定）：vp[0]=callee/返回值槽（复用），vp[1]=this，vp[2..]=实参。
/// 不变式由 `from_raw` 一次性确立，之后 `arg`/`set_rval` 均为 safe 访问器。
#[derive(Clone, Copy)]
pub struct Frame {
    vp: *mut JSVal,
    argc: u32,
}

impl Frame {
    /// # Safety
    /// `vp` 必须指向引擎提供的有效 JSNative 调用帧（至少 `2 + argc` 个槽位），
    /// 且该帧在 `Frame` 存活期内不被 GC 移动/回收（引擎回调期间恒成立）。
    pub unsafe fn from_raw(vp: *mut JSVal, argc: u32) -> Self {
        Frame { vp, argc }
    }

    pub fn argc(&self) -> u32 {
        self.argc
    }

    /// 越界时 debug 断言；调用方须用 `argc()` 守卫（既有调用点均已守卫）。
    pub fn arg(&self, i: u32) -> JSVal {
        debug_assert!(i < self.argc, "arg index out of range");
        // SAFETY: from_raw 的不变式 + 上方断言保证 `2 + i` 下标有效
        unsafe { *self.vp.add(2 + i as usize) }
    }

    /// 写入即设置返回值。
    pub fn set_rval(&self, v: JSVal) {
        // SAFETY: from_raw 的不变式保证 vp[0] 可写
        unsafe {
            *self.vp = v;
        }
    }

    /// 返回值槽句柄（`to_jsval` 等需 rust MutableHandle 的写入用；调用期内有效）。
    pub fn rval_mut(&self) -> mozjs::gc::MutableHandle<'_, JSVal> {
        // SAFETY: from_raw 的不变式保证 vp[0] 为已 root 的返回值槽（set_rval 同前置）
        unsafe { mozjs::gc::MutableHandle::from_marked_location(self.vp) }
    }
}

/// ToString 语义取字符串。ToString 抛异常时清掉 pending exception 并给出占位串
/// （console.log 的参数转换不应让脚本爆炸，Phase 1 简化处理）。
pub fn value_to_string(cx: &mut JSContext, v: JSVal) -> String {
    rooted!(&in(cx) let val = v);
    match String::from_jsval(cx, val.handle(), ()) {
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

/// UNSAFE-BOUNDARY: 读对象属性值（含 undefined 值也 Some；API 失败 None，pending 由调用方处理）。
/// 前置：cx 在 realm 内；obj 为有效对象。
/// 覆盖：`phase4_require_cjs`、`phase4_require_json`（经 require 取 exports/default）。
pub fn get_prop_value(cx: &mut JSContext, obj: *mut JSObject, name: &CStr) -> Option<JSVal> {
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
    if ok { Some(v.get()) } else { None }
}

/// UNSAFE-BOUNDARY: `JSON.parse(text)`（失败 None，pending 由调用方处理）。
/// 前置：cx 在 realm 内；global 为有效全局。
/// 覆盖：`phase4_require_json`（经 require 读 `.json`）。
pub fn parse_json(cx: &mut JSContext, global: *mut JSObject, text: &str) -> Option<JSVal> {
    rooted!(&in(cx) let mut json_v = mozjs::jsval::UndefinedValue());
    // SAFETY: global 为有效 rooted 对象（调用方 rooted）；raw 调用不触发 GC
    let ok = unsafe {
        mozjs::jsapi::JS_GetProperty(
            cx.raw_cx(),
            raw_handle(&global),
            c"JSON".as_ptr(),
            raw_handle_mut(json_v.as_ptr()),
        )
    };
    if !ok || !json_v.is_object() {
        return None;
    }
    let json_obj = json_v.to_object();
    let parse = get_prop_value(cx, json_obj, c"parse")?;
    if !parse.is_object() {
        return None;
    }
    rooted!(&in(cx) let mut text_v = UndefinedValue());
    text.to_jsval(cx, text_v.handle_mut());
    call_one(cx, global, parse, text_v.get())
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
    let obj = exc.to_object();
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
    match mozjs::rust::error_info_from_exception_stack(&mut realm, exc.handle_mut()) {
        Some(info) => {
            let line = info.line.saturating_sub(state::line_adjust());
            Error::script(filename, source, line, info.col, info.message)
        }
        None => Error::Other("uncaught JS exception (no stack info)".into()),
    }
}

// ── 集中边界调用（UNSAFE-BOUNDARY 登记区）─────────────────────────────────
// 规则：业务模块禁直接调本节之外的裸 JSAPI；新增收敛函数必须带
// `UNSAFE-BOUNDARY` 标签（前置条件 + 覆盖测试名），供黑盒重点回归。

/// UNSAFE-BOUNDARY: 调单参函数 `fun(arg)`（this=global；返回 rval；失败 None）。
/// 前置：cx 在 realm 内；fun 为可调用；调用后 pending exception 由调用方处理。
/// 覆盖：`phase3_fetch_http_get`、`phase3_fetch_errors_are_rejections`、
/// `phase3_websocket_echo_and_close`（经 fetch/ws dispatch）。
pub fn call_one(
    cx: &mut JSContext,
    global: *mut JSObject,
    fun: JSVal,
    arg: JSVal,
) -> Option<JSVal> {
    rooted!(&in(cx) let fun_root = fun);
    rooted!(&in(cx) let arg_root = arg);
    rooted!(&in(cx) let mut rval = UndefinedValue());
    // SAFETY: 单实参直构（§4.9）；fun/arg 为有效 rooted 值；rval 为 rooted 出参
    let args = HandleValueArray::from(unsafe { raw_handle(arg_root.as_ptr()) });
    let ok = unsafe {
        JS_CallFunctionValue(
            cx.raw_cx(),
            raw_handle(&global),
            raw_handle(fun_root.as_ptr()),
            &args,
            raw_handle_mut(rval.as_ptr()),
        )
    };
    if ok { Some(rval.get()) } else { None }
}

/// UNSAFE-BOUNDARY: 调双参函数 `fun(a, b)`（fire_due 的 ValueArray 模式）。
/// 前置：cx 在 realm 内；仅事件循环上下文可用（native 内禁 `Rooted<ValueArray>`，§4.9）。
/// 覆盖：`phase3_fetch_http_get`、`phase3_fetch_data_and_file`（经 fetch deliver）。
pub fn call_two(
    cx: &mut JSContext,
    global: *mut JSObject,
    fun: JSVal,
    a: JSVal,
    b: JSVal,
) -> Option<JSVal> {
    rooted!(&in(cx) let fun_root = fun);
    rooted!(&in(cx) let argv = ValueArray::new([a, b]));
    rooted!(&in(cx) let mut rval = UndefinedValue());
    let args_array = HandleValueArray {
        length_: 2,
        // SAFETY: argv 为栈上 Rooted 槽，存活到调用返回，元素被 GC 追踪
        elements_: argv.as_ptr().cast(),
    };
    // SAFETY: cx/global/fun 均有效；rval 为 rooted 出参
    let ok = unsafe {
        JS_CallFunctionValue(
            cx.raw_cx(),
            raw_handle(&global),
            raw_handle(fun_root.as_ptr()),
            &args_array,
            raw_handle_mut(rval.as_ptr()),
        )
    };
    if ok { Some(rval.get()) } else { None }
}

/// UNSAFE-BOUNDARY: 由字节建 Uint8Array。
/// 前置：cx 在 realm 内；bytes 存活到调用返回。
/// 覆盖：`phase3_text_encoder_decoder`、`phase3_subtle_digest_vectors`、
/// `phase3_aes_gcm_roundtrip`、`phase3_fetch_data_and_file`、
/// `phase3_websocket_echo_and_close`（二进制消息）。
pub fn uint8_array(cx: &mut JSContext, bytes: &[u8]) -> Option<*mut JSObject> {
    rooted!(&in(cx) let mut obj: *mut JSObject = std::ptr::null_mut());
    // SAFETY: realm 内创建；obj 为 rooted 出参；bytes 存活到调用返回
    let ok = unsafe {
        TypedArray::<Uint8, *mut JSObject>::create(cx, CreateWith::Slice(bytes), obj.handle_mut())
    };
    if ok.is_err() || obj.is_null() {
        None
    } else {
        Some(obj.get())
    }
}

/// UNSAFE-BOUNDARY: Uint8Array 实参 → 字节拷贝（safe 读，无裸指针）。
/// 非 Uint8 视图/共享内存/detached 一律 TypeError（切片行为见各调用方文档）。
/// 覆盖：`phase3_text_decoder_fatal`、`phase3_crypto_random`（配额/类型错）、
/// `phase3_aes_gcm_roundtrip`、`phase3_hmac_sign_verify`、`phase3_fetch_http_post_echo`。
pub fn view_bytes(cx: &mut JSContext, v: JSVal, what: &str) -> Option<Vec<u8>> {
    if !v.is_object() {
        report_error(cx, &format!("TypeError: {what} requires a Uint8Array"));
        return None;
    }
    // SAFETY: is_object 已判定（to_object/from/as_slice_safe 均为 safe API）
    let obj = v.to_object();
    let Ok(arr) = TypedArray::<Uint8, *mut JSObject>::from(obj) else {
        report_error(cx, &format!("TypeError: {what} requires a Uint8Array"));
        return None;
    };
    if arr.is_shared() {
        report_error(cx, &format!("TypeError: {what} does not accept SharedArrayBuffer views yet"));
        return None;
    }
    match arr.as_slice_safe(cx.no_gc()) {
        Some(s) => Some(s.to_vec()),
        None => {
            report_error(cx, &format!("TypeError: {what} view is detached"));
            None
        }
    }
}

/// UNSAFE-BOUNDARY: 在对象上定义可枚举属性（值可跨 compartment，引擎自动包 CCW）。
/// 前置：cx 在 obj 所属 realm 内；obj 为有效对象；name 无 NUL。
/// 覆盖：`phase9f_vm_context_spawns_and_isolates`、`phase9f_vm_sandbox_sync`
/// （经 vm sync-in/out）。
pub fn define_prop(cx: &mut JSContext, obj: *mut JSObject, name: &CStr, val: JSVal) -> bool {
    rooted!(&in(cx) let v = val);
    // SAFETY: realm 内；obj 有效；name 无 NUL；v 为 rooted 值
    unsafe {
        mozjs::jsapi::JS_DefineProperty(
            cx.raw_cx(),
            raw_handle(&obj),
            name.as_ptr(),
            raw_handle(v.as_ptr()),
            mozjs::jsapi::JSPROP_ENUMERATE as u32,
        )
    }
}
