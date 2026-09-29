//! JSNative 调用帧与常用 JSAPI 便捷封装。
//! 本模块是项目的 unsafe 集中地（AGENTS §6）：除 hooks/jobqueue/state 的引擎协议
//! 代码外，所有裸 JSAPI 调用必须收敛到本模块的封装函数；每处 unsafe 注明前置条件，
//! 并用 `UNSAFE-BOUNDARY` 标签登记覆盖测试（黑盒重点）。

use std::ffi::{CStr, CString};

use mozjs::conversions::{ConversionResult, FromJSValConvertible as _, ToJSValConvertible as _};
use mozjs::context::JSContext;
use mozjs::gc::ValueArray;
use mozjs::jsapi::{HandleValueArray, JS_CallFunctionValue, JS_GetPendingException, JS_SetPendingException, JSObject};
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

/// UNSAFE-BOUNDARY: 写对象属性值（赋值语义；失败 false，pending 由调用方处理）。
/// 前置：cx 在 realm 内；obj 为有效对象；val 为 rooted 值。
/// 覆盖：`tests/node/vm.rs::phase10c_vm_rerun_with_new_globals`（经 vm sync-in 回落）。
pub fn set_prop_value(cx: &mut JSContext, obj: *mut JSObject, name: &CStr, val: JSVal) -> bool {
    rooted!(&in(cx) let v = val);
    // SAFETY: cx 为有效 wrapper；标记位置指针直拷；raw 调用不触发 GC
    unsafe {
        mozjs::jsapi::JS_SetProperty(
            cx.raw_cx(),
            raw_handle(&obj),
            name.as_ptr(),
            raw_handle(v.as_ptr()),
        )
    }
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

/// 读异常值的 `name` 属性（如 "SyntaxError"）。非对象/无 name 返回 None。
pub fn exc_name(cx: &mut JSContext, exc: JSVal) -> Option<String> {
    if !exc.is_object() {
        return None;
    }
    // SAFETY: is_object 已判定
    let obj = exc.to_object();
    get_prop_string(cx, obj, c"name")
}

/// 检查异常值的 `name` 属性（如 "SyntaxError"）。非对象/无 name 返回 false。
pub fn exc_name_is(cx: &mut JSContext, exc: JSVal, name: &str) -> bool {
    exc_name(cx, exc).as_deref() == Some(name)
}

/// 非对象异常值的原始值信封（10f worker 错误透传：throw 42/"boom"/7n/Symbol
/// 经 `__wjs2_prim:{json}` 跨线程还原，error-primitive 套件断同一性）。
/// 对象异常返回 None（走 `exc_name` 类名路径）。
/// UNSAFE-BOUNDARY：JS_TypeOfValue 只读；exc 须已 rooted（调用方 rooted! 槽位
/// `.get()` 传入）。覆盖测试：`tests/node/worker.rs` worker 错误形状。
pub fn exc_prim_marker(cx: &mut JSContext, exc: JSVal) -> Option<String> {
    if exc.is_object() {
        return None;
    }
    if exc.is_null() {
        return Some("__wjs2_prim:{\"t\":\"nil\"}".into());
    }
    if exc.is_undefined() {
        return Some("__wjs2_prim:{\"t\":\"undef\"}".into());
    }
    if exc.is_boolean() {
        return Some(format!("__wjs2_prim:{{\"t\":\"bool\",\"v\":{}}}", exc.to_boolean()));
    }
    if exc.is_int32() {
        return Some(format!("__wjs2_prim:{{\"t\":\"num\",\"v\":\"{}\"}}", exc.to_int32()));
    }
    if exc.is_double() {
        let n = exc.to_number();
        let v = if n.is_finite() { n.to_string() } else { "null".into() };
        return Some(format!("__wjs2_prim:{{\"t\":\"num\",\"v\":\"{v}\"}}"));
    }
    if exc.is_string() {
        let s = value_to_string(cx, exc);
        let json = serde_json::to_string(&s).unwrap_or_else(|_| "\"\"".into());
        return Some(format!("__wjs2_prim:{{\"t\":\"str\",\"v\":{json}}}"));
    }
    // BigInt/Symbol：jsval 谓词不覆盖，走引擎 TypeOf。
    rooted!(&in(cx) let exc_root = exc);
    // SAFETY: 只读调用；根槽位由 rooted! 保活
    let t = unsafe { mozjs::jsapi::JS_TypeOfValue(cx.raw_cx(), raw_handle(exc_root.as_ptr())) };
    use mozjs::jsapi::JSType;
    if t == JSType::JSTYPE_BIGINT {
        let s = value_to_string(cx, exc);
        return Some(format!("__wjs2_prim:{{\"t\":\"big\",\"v\":\"{s}\"}}"));
    }
    if t == JSType::JSTYPE_SYMBOL {
        // 描述经 prelude `__wjs2_symToString`（JS 的 toString 合法；注册 Symbol
        // 跨线程同一性靠 Symbol.for(key)）。
        let g = state::global();
        rooted!(&in(cx) let g_root: *mut JSObject = g);
        let s = get_prop_value(cx, g_root.get(), c"__wjs2_symToString")
            .and_then(|f| call_two(cx, g, f, exc_root.get(), UndefinedValue()))
            .filter(|r| r.is_string())
            .map(|r| value_to_string(cx, r))
            .unwrap_or_else(|| "Symbol()".into());
        let desc = s.strip_prefix("Symbol(").and_then(|r| r.strip_suffix(")")).unwrap_or("");
        let json = serde_json::to_string(desc).unwrap_or_else(|_| "\"\"".into());
        return Some(format!("__wjs2_prim:{{\"t\":\"sym\",\"v\":{json}}}"));
    }
    None
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
    match { let i = mozjs::rust::error_info_from_exception_stack(&mut realm, exc.handle_mut()); crate::jsapi_glue::fill_message(&mut realm, i, exc.get()) } {
        Some(info) => {
            let kind = exc_name(&mut realm, exc.get());
            let line = info.line.saturating_sub(state::line_adjust());
            Error::script_with_kind(filename, source, line, info.col, info.message, kind)
        }
        None => Error::Other("uncaught JS exception (no stack info)".into()),
    }
}

// ── 集中边界调用（UNSAFE-BOUNDARY 登记区）─────────────────────────────────
// 规则：业务模块禁直接调本节之外的裸 JSAPI；新增收敛函数必须带
// `UNSAFE-BOUNDARY` 标签（前置条件 + 覆盖测试名），供黑盒重点回归。

/// UNSAFE-BOUNDARY: 取走当前 pending exception（JS_GetPendingException 语义：
/// 成功取走即清除 pending）。前置：cx 在 realm 内；刚一次失败的 JSAPI 调用；
/// 取出的值必须 rooted 后再用于后续 JS 调用（§4.80 链式调用铁律）。
/// 覆盖：`tests/node/timers.rs::phase10f_timer_uncaught_routing`（有监听分发）
/// 与无监听 fatal 路径（探针保证不进本函数）。
pub fn take_pending_exception(cx: &mut JSContext) -> Option<JSVal> {
    rooted!(&in(cx) let mut val = UndefinedValue());
    // SAFETY: cx 有效；出参为 rooted 槽位
    let got = unsafe { JS_GetPendingException(cx.raw_cx(), raw_handle_mut(val.as_ptr())) };
    if got { Some(val.get()) } else { None }
}

/// 报错信息补全：引擎报告里 message 为空、而异常对象有自有 `message` 属性时回填
/// （node 口径 NodeError 为 `super()` 后 defineProperty 设 message，引擎内部
/// message 槽为空——修前入口报错恒 `Error: file:L:C: ` 空文案）。
/// UNSAFE-BOUNDARY：经 `get_prop_string` 只读；exc 须已 rooted（调用方槽位 `.get()`）。
/// 覆盖：`tests/node/require.rs::phase11_require_rethrows_original_exception`。
pub fn fill_message(
    cx: &mut JSContext,
    info: Option<mozjs::rust::ErrorInfo>,
    exc: JSVal,
) -> Option<mozjs::rust::ErrorInfo> {
    let mut info = info?;
    if info.message.is_empty() && exc.is_object() {
        if let Some(m) = get_prop_string(cx, exc.to_object(), c"message") {
            info.message = m;
        }
    }
    // D4：顺手记下异常栈，供 node 形渲染（`error::note_stack`，按 message 配对取走）。
    if exc.is_object() {
        if let Some(st) = get_prop_string(cx, exc.to_object(), c"stack") {
            let kind = get_prop_string(cx, exc.to_object(), c"name");
            crate::error::note_stack(&info.message, crate::state::remap_stack(&st), kind);
        }
    }
    Some(info)
}

/// UNSAFE-BOUNDARY: 是否有 pending exception（JS_IsExceptionPending 只读位，无副作用）。
/// 前置：cx 有效。覆盖：`tests/node/require.rs::phase11_require_rethrows_original_exception`。
pub fn exception_pending(cx: &mut JSContext) -> bool {
    // SAFETY: cx 有效；只读 pending 位
    unsafe { mozjs::jsapi::JS_IsExceptionPending(cx.raw_cx()) }
}

/// UNSAFE-BOUNDARY: 恢复 pending exception（JS_SetPendingException，Capture 栈）。
/// 前置：cx 在目标 realm 内；v 由调用方 rooted 后传入（§4.80）。
/// 覆盖：`tests/node/vm.rs` vm 对拍黑盒（原始异常透传，与 take 成对）。
pub fn set_pending_exception(cx: &mut JSContext, v: JSVal) {    rooted!(&in(cx) let vroot: JSVal = v);
    // SAFETY: cx 有效；入参为 rooted 槽位；Capture 保留异常栈语义
    unsafe {
        JS_SetPendingException(
            cx.raw_cx(),
            raw_handle(vroot.as_ptr()),
            mozjs::jsapi::JS::ExceptionStackBehavior::Capture,
        )
    };
}

/// UNSAFE-BOUNDARY: 查 Promise 结算状态（GetPromiseState + JS_GetPromiseResult，
/// 均只读；不推进 jobqueue——settle 由调用方经 RunJobs/pump 推进）。
/// 前置：cx 有效；`obj` 为 Promise 对象且由调用方 rooted（§4.40/§4.80）。
/// 返回：`Some(Some((fulfilled, value)))`=已结算（fulfilled?；value=结算值），
/// `Some(None)`=Pending，`None`=API 失败（pending 由调用方处理）。
/// 覆盖：`tests/repl.rs::repl_tla_await_resolves` / `repl_tla_await_rejects`。
pub fn promise_settled_value(cx: &mut JSContext, obj: *mut JSObject) -> Option<Option<(bool, JSVal)>> {
    rooted!(&in(cx) let obj_root = obj);
    // SAFETY: obj 为调用方 rooted 的 Promise 对象（只读 state）
    let state = unsafe { mozjs::jsapi::GetPromiseState(raw_handle(obj_root.as_ptr())) };
    match state {
        mozjs::jsapi::PromiseState::Pending => Some(None),
        mozjs::jsapi::PromiseState::Fulfilled | mozjs::jsapi::PromiseState::Rejected => {
            rooted!(&in(cx) let mut out = UndefinedValue());
            // SAFETY: 出参为 rooted 槽位
            unsafe {
                mozjs::glue::JS_GetPromiseResult(raw_handle(obj_root.as_ptr()), raw_handle_mut(out.as_ptr()));
            }
            Some(Some((
                state == mozjs::jsapi::PromiseState::Fulfilled,
                out.get(),
            )))
        }
    }
}

/// UNSAFE-BOUNDARY: 取 TLA 包装 promise 的结算对（读 `__wjs2_ok` + `v`/`e`，
/// 属性读不触发 GC，obj 内部 rooted，一次调用无 GC 间隙）。
/// 前置：cx 当前 realm 为对象所属 realm（主循环 global）；obj 为 Promise 结算值
/// 对象且调用方 rooted。返回 Some((resolved, value_or_reason))。
/// 覆盖：`tests/repl.rs::repl_tla_await_resolves` / `repl_tla_await_rejects`。
pub fn tla_pack_take(cx: &mut JSContext, obj: *mut JSObject) -> Option<(bool, JSVal)> {
    rooted!(&in(cx) let mut obj_root: *mut JSObject = obj);
    let optr = obj_root.get();
    let ok = get_prop_value(cx, optr, c"__wjs2_ok")?;
    let is_ok = ok.is_int32() && ok.to_int32() == 1;
    let field = if is_ok {
        get_prop_value(cx, optr, c"v")?
    } else {
        get_prop_value(cx, optr, c"e")?
    };
    Some((is_ok, field))
}

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

/// UNSAFE-BOUNDARY: 调双参函数 `fun(a, b)`（timer fire 经 `__wjs2_call(cb, args)`
/// 展开实参；native 内禁 `Rooted<ValueArray>`，§4.9）。
/// 前置：cx 在 realm 内；fun 为可调用；调用后 pending exception 由调用方处理。
/// 覆盖：`phase3_fetch_http_get`、`phase3_fetch_data_and_file`（经 fetch deliver）、
/// `tests/builtins.rs::phase10a_immediate_and_timeout_class`（经 timer fire）。
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

/// UNSAFE-BOUNDARY: 调三参函数 `fun(a, b, c)`（this=global；napi_call 的
/// prelude helper `__wjs2_napi_call(recv, fn, args)` 用）。
/// 前置：cx 在 realm 内；调用后 pending exception 由调用方处理。
/// 覆盖：`tests/napi.rs::phase_napi_m1_values`（经 napi_call_function）。
pub fn call_three(
    cx: &mut JSContext,
    global: *mut JSObject,
    fun: JSVal,
    a: JSVal,
    b: JSVal,
    c: JSVal,
) -> Option<JSVal> {
    rooted!(&in(cx) let fun_root = fun);
    rooted!(&in(cx) let argv = ValueArray::new([a, b, c]));
    rooted!(&in(cx) let mut rval = UndefinedValue());
    let args_array = HandleValueArray {
        length_: 3,
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

/// UNSAFE-BOUNDARY: 取对象全部自有键（含不可枚举字符串键；symbol 键以占位对象透传）。
/// 以 JSON 数组回传（字符串键为 JSON 串、symbol 键为 `{"__wjs2_symbol":true}` 占位；
/// 空对象回 `"[]"`）。占位无跨 realm 身份，调用方只做存在性/计数口径。
/// 前置：cx 在 obj 所属 realm 内；obj 为有效对象；调用后 pending 由调用方处理。
/// 覆盖：`tests/node/vm.rs::phase10f_vm_sync_all_keys`（经 vm sync-out/创建快照）。
pub fn own_keys_json(cx: &mut JSContext, obj: *mut JSObject) -> Option<String> {
    use mozjs::rust::IdVector;
    // SAFETY: realm 内；obj 有效；IdVector 为 rooted 槽（§4.40 定址纪律同源）
    let mut ids = IdVector::new(cx);
    // JSITER_OWNONLY | JSITER_HIDDEN | JSITER_SYMBOLS（自有 + 不可枚举 + symbol；
    // symbol 经 IdToValue 透传，主域侧按 opaque 占位处理）。
    const FLAGS: u32 = 0x8 | 0x10 | 0x20;
    let ok = unsafe {
        mozjs::rust::wrappers2::GetPropertyKeys(
            cx,
            mozjs::gc::Handle::from_marked_location(&obj),
            FLAGS,
            ids.handle_mut(),
        )
    };
    if !ok {
        return None;
    }
    let mut out = String::from("[");
    let mut first = true;
    rooted!(&in(cx) let mut v = UndefinedValue());
    for id in ids.iter() {
        // SAFETY: id 来自引擎枚举；v 为 rooted 出参（root.rs handle_mut 同款）。
        // symbol id 经 IdToValue 透传为 symbol 值，由调用方 JSON 侧按 opaque 占位。
        let ok = unsafe { mozjs::rust::wrappers2::JS_IdToValue(cx, *id, v.handle_mut()) };
        if !ok {
            return None;
        }
        if v.get().is_symbol() {
            if !first {
                out.push(',');
            }
            first = false;
            out.push_str("{\"__wjs2_symbol\":true}");
            continue;
        }
        let s = value_to_string(cx, v.get());
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str(&serde_json::Value::String(s).to_string());
    }
    out.push(']');
    Some(out)
}

/// UNSAFE-BOUNDARY: 跨 compartment SameValue 比较（`JS::SameValue` 语义：NaN 自等，
/// +0/-0 不等；CCW 参数由引擎自动解包比对底层身份）。
/// 前置：cx 在 realm 内；a/b 为 rooted 值（调用方 rooted 后传入 §4.80）。
/// 覆盖：`tests/node/vm.rs::phase10f_vm_sync_snapshot`（经 vm sync-out 快照比较）。
pub fn same_value(cx: &mut JSContext, a: JSVal, b: JSVal) -> Option<bool> {
    rooted!(&in(cx) let a_root = a);
    rooted!(&in(cx) let b_root = b);
    let mut same = false;
    // SAFETY: 谓词无副作用；a/b 为 rooted 槽（napi StrictlyEqual 同款 raw 形态）
    let ok = unsafe {
        mozjs::jsapi::JS::SameValue(
            cx.raw_cx(),
            raw_handle(a_root.as_ptr()),
            raw_handle(b_root.as_ptr()),
            &mut same,
        )
    };
    if ok { Some(same) } else { None }
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
