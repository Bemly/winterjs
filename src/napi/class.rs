//! napi 类与 wrap 面（plan-napi M2）：define_class / new_instance / wrap /
//! unwrap / remove_wrap / external + finalize。
//!
//! 设计（对齐引擎机制，2026-09-14 实测定稿）：
//! - 静态 `JSClass` 两枚，全 napi 类共享（类判定 = `JS_InstanceOf` 指针比对，
//!   不走类名）：`NAPI_INSTANCE_CLASS`（define_class 实例）与
//!   `NAPI_EXTERNAL_CLASS`（napi_create_external）。私有数据存 reserved slots
//!   0..3（data、finalize、hint、env，全 `PrivateValue`——double-tag，GC 不追，
//!   M1 gc-pressure 用例已验该语义；槽位未写 = undefined，`is_double` 即哨兵）。
//! - finalize：`JSCLASS_FOREGROUND_FINALIZE` 钉死主线程（不设则默认 background，
//!   helper 线程跑 finalize op，addon 回调碰 NapiEnv 即数据竞争）；op 内只读
//!   reserved slots（finalize 期唯一安全访问面），cb 槽非 double 即跳过（未
//!   wrap 实例/原型对象同款）。addon finalizer 在 GC sweep 内直接执行（Node
//!   同期语义：finalizer 期间禁大多数 napi_*），偏差记 plan-napi §4。
//! - wrap 仅限 NAPI_INSTANCE_CLASS 实例（rolldown 的 wrap 全走 define_class
//!   实例；external 的 slot0 已被外部数据占用，二者不混——Node 用 internal
//!   fields 分槽，我们单槽，偏差记档 M4）。任意对象 wrap 缺 GC 驱动的 finalize
//!   通道（绑定无 finalize observer），fail-fast 报错，不静默泄漏。
//! - new_instance / define_class 的访问器定义经 prelude helper（`__wjs_napi_new`
//!   / `__wjs_napi_accessor`），免变长 HandleValueArray 与 JSAPI 访问器雷区。

use std::ffi::{c_char, c_void, CString};
use std::ptr;

use mozjs::context::JSContext;
use mozjs::glue::JS_GetReservedSlot;
use mozjs::jsapi::{
    JS_NewObjectWithGivenProto, JSCLASS_FOREGROUND_FINALIZE, JSCLASS_RESERVED_SLOTS_SHIFT,
    JSClass, JSClassOps, JSContext as RawJSContext, JSObject, JS_SetReservedSlot,
    JSPROP_PERMANENT,
};
use mozjs::jsval::{JSVal, ObjectValue, PrivateValue, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{call_two, get_prop_value, raw_handle};
use crate::napi::api::{cstr_of_len, cx_of, e, napi_trampoline_frame, object_prototype};
use crate::napi::property::define_one;
use crate::napi::sys;
use crate::napi::sys::{napi_env, napi_status, napi_value};

const NAPI_OK: napi_status = sys::napi_status_napi_ok;
const NAPI_INVALID_ARG: napi_status = sys::napi_status_napi_invalid_arg;
const NAPI_GENERIC_FAILURE: napi_status = sys::napi_status_napi_generic_failure;

// reserved slot 布局（instance/external 同款）。
const SLOT_DATA: u32 = 0;
const SLOT_FINALIZE: u32 = 1;
const SLOT_HINT: u32 = 2;
const SLOT_ENV: u32 = 3;
const SLOT_COUNT: u32 = 4;

static NAPI_CLASS_OPS: JSClassOps = JSClassOps {
    addProperty: None,
    delProperty: None,
    enumerate: None,
    newEnumerate: None,
    resolve: None,
    mayResolve: None,
    finalize: Some(napi_class_finalize),
    call: None,
    construct: None,
    trace: None,
};

/// define_class 实例类（wrap 面；全 napi 类共享同一静态类）。
pub(crate) static NAPI_INSTANCE_CLASS: JSClass = JSClass {
    name: c"napi-instance".as_ptr(),
    flags: JSCLASS_FOREGROUND_FINALIZE | (SLOT_COUNT << JSCLASS_RESERVED_SLOTS_SHIFT),
    cOps: &NAPI_CLASS_OPS,
    spec: ptr::null(),
    ext: ptr::null(),
    oOps: ptr::null(),
};

/// napi_create_external 产物类（typeof = external）。
pub(crate) static NAPI_EXTERNAL_CLASS: JSClass = JSClass {
    name: c"napi-external".as_ptr(),
    flags: JSCLASS_FOREGROUND_FINALIZE | (SLOT_COUNT << JSCLASS_RESERVED_SLOTS_SHIFT),
    cOps: &NAPI_CLASS_OPS,
    spec: ptr::null(),
    ext: ptr::null(),
    oOps: ptr::null(),
};

/// 类判定谓词（`JS_InstanceOf` args=null：纯指针比对，不匹配不置 pending）。
pub(crate) fn object_is_class(
    cx: &mut JSContext,
    obj: *mut JSObject,
    clasp: *const JSClass,
) -> bool {
    if obj.is_null() {
        return false;
    }
    // SAFETY：谓词无 GC 点；obj 为栈上 rooted 值（调用方保证存活期）。
    unsafe {
        rooted!(&in(cx) let obj_root: *mut JSObject = obj);
        mozjs::jsapi::JS_InstanceOf(
            cx.raw_cx(),
            raw_handle(&obj_root.get()),
            clasp,
            ptr::null_mut(),
        )
    }
}

/// 共享 finalize op（GC sweep 期执行；主线程——FOREGROUND_FINALIZE 钉死）。
///
/// # Safety
/// 仅引擎 GC 调用（JSFinalizeOp 协议）；只读 reserved slots（finalize 期
/// 唯一安全访问面），禁一切 JSAPI（addon finalizer 同约束，Node 同口径）。
unsafe extern "C" fn napi_class_finalize(_gcx: *mut mozjs::jsapi::JS::GCContext, obj: *mut JSObject) {
    // SAFETY：引擎 finalize 协议（见 # Safety）；slots 全 PrivateValue 语义。
    unsafe {
        let mut data = UndefinedValue();
        let mut cb = UndefinedValue();
        let mut hint = UndefinedValue();
        let mut env_v = UndefinedValue();
        JS_GetReservedSlot(obj, SLOT_DATA, &mut data);
        JS_GetReservedSlot(obj, SLOT_FINALIZE, &mut cb);
        JS_GetReservedSlot(obj, SLOT_HINT, &mut hint);
        JS_GetReservedSlot(obj, SLOT_ENV, &mut env_v);
        // 未 wrap/未挂 finalizer 的实例与原型对象：cb 槽为 undefined，跳过。
        if !cb.is_double() || !env_v.is_double() {
            return;
        }
        let env = env_v.to_private() as napi_env;
        let data_ptr = if data.is_double() {
            data.to_private() as *mut c_void
        } else {
            ptr::null_mut()
        };
        let hint_ptr = if hint.is_double() {
            hint.to_private() as *mut c_void
        } else {
            ptr::null_mut()
        };
        // SAFETY：cb 由 napi_wrap/napi_create_external 写入（addon 侧
        // node_api_basic_finalize 签名，vendored 头实测 = napi_finalize 同形）。
        let f: unsafe extern "C" fn(napi_env, *mut c_void, *mut c_void) =
            std::mem::transmute(cb.to_private());
        f(env, data_ptr, hint_ptr);
    }
}

/// define_class 构造器 trampoline：与普通 trampoline 同帧，仅实例类不同。
///
/// # Safety
/// 仅引擎回调帧内调用（同 napi_trampoline 前置）。
pub unsafe extern "C" fn napi_ctor_trampoline(
    cx_raw: *mut RawJSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY：引擎回调帧（同 napi_trampoline；UNSAFE-BOUNDARY，覆盖 m2 黑盒）。
    unsafe { napi_trampoline_frame(cx_raw, argc, vp, &NAPI_INSTANCE_CLASS) }
}

/// # Safety
/// N-API 约定（vendored js_native_api.h:312；static 属性挂构造器函数对象，
/// 其余挂原型——Node 同款放置）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_define_class(
    env: napi_env,
    utf8name: *const c_char,
    length: usize,
    constructor: sys::napi_callback,
    data: *mut c_void,
    property_count: usize,
    properties: *const sys::napi_property_descriptor,
    result: *mut napi_value,
) -> napi_status {
    if result.is_null() || constructor.is_none() {
        return NAPI_INVALID_ARG;
    }
    let name = match unsafe { cstr_of_len(utf8name, length) } {
        Ok(s) if !s.is_empty() => s,
        Ok(_) => "__napianonymous".to_string(),
        Err(st) => return st,
    };
    let Ok(cname) = CString::new(name) else {
        return NAPI_GENERIC_FAILURE;
    };
    let mut cx = unsafe { cx_of(env) };
    let env_ref = unsafe { e(env) };
    // SAFETY：原型与函数先 rooted 再接线（建值与落槽间可能 GC）。
    unsafe {
        // 原型：实例同款类 + Object.prototype（instanceof/方法查找全走原型链）。
        let Some(base) = object_prototype(&mut cx) else {
            return NAPI_GENERIC_FAILURE;
        };
        rooted!(&in(cx) let base_root: *mut JSObject = base.to_object());
        let proto = JS_NewObjectWithGivenProto(
            cx.raw_cx(),
            &NAPI_INSTANCE_CLASS,
            raw_handle(&base_root.get()),
        );
        if proto.is_null() {
            return NAPI_GENERIC_FAILURE;
        }
        rooted!(&in(cx) let proto_root: *mut JSObject = proto);
        let fun = mozjs::jsapi::js::NewFunctionWithReserved(
            cx.raw_cx(),
            Some(napi_ctor_trampoline),
            0,
            mozjs::jsapi::JSFUN_CONSTRUCTOR,
            cname.as_ptr(),
        );
        if fun.is_null() {
            return NAPI_GENERIC_FAILURE;
        }
        let fobj = mozjs::jsapi::JS_GetFunctionObject(fun);
        rooted!(&in(cx) let froot: *mut JSObject = fobj);
        mozjs::jsapi::js::SetFunctionNativeReserved(
            froot.get(),
            0,
            &PrivateValue(constructor.unwrap() as *const c_void),
        );
        mozjs::jsapi::js::SetFunctionNativeReserved(froot.get(), 1, &PrivateValue(data));
        // ctor.prototype = proto（JS 口径 {writable, 非 enum, 非 config} = PERMANENT）。
        rooted!(&in(cx) let proto_val = ObjectValue(proto_root.get()));
        if !mozjs::jsapi::JS_DefineProperty(
            cx.raw_cx(),
            raw_handle(&froot.get()),
            c"prototype".as_ptr(),
            raw_handle(proto_val.as_ptr()),
            JSPROP_PERMANENT as u32,
        ) {
            return NAPI_GENERIC_FAILURE;
        }
        // 属性放置：napi_static → 函数对象；其余 → 原型（Node 同款）。
        for i in 0..property_count {
            let d = &*properties.add(i);
            let target_v = if d.attributes & sys::napi_property_attributes_napi_static != 0 {
                ObjectValue(froot.get())
            } else {
                ObjectValue(proto_root.get())
            };
            let st = define_one(env, &mut cx, target_v, d);
            if st != NAPI_OK {
                return st;
            }
        }
        *result = env_ref.put(ObjectValue(froot.get()));
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（vendored js_native_api.h:287；argv 为本 env 槽位指针数组）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_new_instance(
    env: napi_env,
    constructor: napi_value,
    argc: usize,
    argv: *const napi_value,
    result: *mut napi_value,
) -> napi_status {
    if constructor.is_null() || result.is_null() || (argc > 0 && argv.is_null()) {
        return NAPI_INVALID_ARG;
    }
    let ctor_v = unsafe { e(env).get(constructor) };
    let mut cx = unsafe { cx_of(env) };
    let env_ref = unsafe { e(env) };
    // SAFETY：函数与数组先 rooted 再调用；实参槽位 traced。
    unsafe {
        if !ctor_v.is_object() || !mozjs::jsapi::JS_ObjectIsFunction(ctor_v.to_object()) {
            return sys::napi_status_napi_function_expected;
        }
        rooted!(&in(cx) let ctor_root = ctor_v);
        let arr = mozjs::jsapi::JS::NewArrayObject1(cx.raw_cx(), argc);
        if arr.is_null() {
            return NAPI_GENERIC_FAILURE;
        }
        rooted!(&in(cx) let arr_root = arr);
        for i in 0..argc {
            let val = env_ref.get(*argv.add(i));
            rooted!(&in(cx) let val_root = val);
            if !mozjs::jsapi::JS_SetElement(
                cx.raw_cx(),
                raw_handle(&arr_root.get()),
                i as u32,
                raw_handle(val_root.as_ptr()),
            ) {
                return NAPI_GENERIC_FAILURE;
            }
        }
        let Some(helper) = get_prop_value(&mut cx, crate::state::global(), c"__wjs_napi_new")
        else {
            return NAPI_GENERIC_FAILURE;
        };
        // `new ctor(...args)` 全语义（异常 → pending，addon 按契约查）。
        let Some(r) = call_two(
            &mut cx,
            crate::state::global(),
            helper,
            ctor_root.get(),
            ObjectValue(arr_root.get()),
        ) else {
            return NAPI_GENERIC_FAILURE;
        };
        *result = env_ref.put(r);
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（vendored js_native_api.h:323）。`result`（napi_ref）M3 未接——
/// 非 null 即 fail-fast（记 plan-napi §4；rolldown 不取 ref）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_wrap(
    env: napi_env,
    js_object: napi_value,
    native_object: *mut c_void,
    finalize_cb: sys::node_api_basic_finalize,
    finalize_hint: *mut c_void,
    result: *mut sys::napi_ref,
) -> napi_status {
    if js_object.is_null() {
        return NAPI_INVALID_ARG;
    }
    let obj_v = unsafe { e(env).get(js_object) };
    if !obj_v.is_object() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：类判定 + reserved slots 写（PrivateValue，GC 不追）。
    unsafe {
        let obj = obj_v.to_object();
        if !object_is_class(&mut cx, obj, &NAPI_INSTANCE_CLASS) {
            let env_ref = e(env);
            env_ref.set_last_error(
                "napi_wrap requires a define_class instance (arbitrary objects need GC-driven finalize, planned M4)",
            );
            return NAPI_INVALID_ARG;
        }
        let mut existing = UndefinedValue();
        // data 槽被占用即已 wrap（wrap 必写 data；finalize 可能是 NULL）
        JS_GetReservedSlot(obj, SLOT_DATA, &mut existing);
        if existing.is_double() {
            let env_ref = e(env);
            env_ref.set_last_error("object is already wrapped");
            return NAPI_GENERIC_FAILURE;
        }
        JS_SetReservedSlot(obj, SLOT_DATA, &PrivateValue(native_object));
        if let Some(f) = finalize_cb {
            JS_SetReservedSlot(obj, SLOT_FINALIZE, &PrivateValue(f as *const c_void));
            JS_SetReservedSlot(obj, SLOT_HINT, &PrivateValue(finalize_hint));
            JS_SetReservedSlot(obj, SLOT_ENV, &PrivateValue(env as *const c_void));
        }
        if !result.is_null() {
            // napi_ref 出参（Node 口径：初始计数 0 的引用，指向被 wrap 对象；
            // napi-rs 3 的 ctor 恒传此参做 Reference 簿记）。
            let rec = Box::new(crate::napi::refcount::RefRec {
                value: mozjs::jsapi::Heap::boxed(obj_v),
                refcount: 0,
            });
            *result = &*rec as *const crate::napi::refcount::RefRec
                as *const c_void as sys::napi_ref;
            e(env).refs.push(rec);
        }
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（vendored js_native_api.h:329）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_unwrap(
    env: napi_env,
    js_object: napi_value,
    result: *mut *mut c_void,
) -> napi_status {
    if js_object.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let obj_v = unsafe { e(env).get(js_object) };
    if !obj_v.is_object() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：类判定 + reserved slots 读。
    unsafe {
        let obj = obj_v.to_object();
        if !object_is_class(&mut cx, obj, &NAPI_INSTANCE_CLASS) {
            let env_ref = e(env);
            env_ref.set_last_error("object is not a define_class instance");
            return NAPI_INVALID_ARG;
        }
        let mut data = UndefinedValue();
        JS_GetReservedSlot(obj, SLOT_DATA, &mut data);
        if !data.is_double() {
            let env_ref = e(env);
            env_ref.set_last_error("object is not wrapped");
            return NAPI_INVALID_ARG;
        }
        *result = data.to_private() as *mut c_void;
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（vendored js_native_api.h:332；摘除后 finalize 不再触发，
/// 所有权归还 addon——Node 同语义）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_remove_wrap(
    env: napi_env,
    js_object: napi_value,
    result: *mut *mut c_void,
) -> napi_status {
    if js_object.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let obj_v = unsafe { e(env).get(js_object) };
    if !obj_v.is_object() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：类判定 + reserved slots 读写（摘除 = 四槽全清，finalize 链断）。
    unsafe {
        let obj = obj_v.to_object();
        if !object_is_class(&mut cx, obj, &NAPI_INSTANCE_CLASS) {
            let env_ref = e(env);
            env_ref.set_last_error("object is not a define_class instance");
            return NAPI_INVALID_ARG;
        }
        let mut data = UndefinedValue();
        JS_GetReservedSlot(obj, SLOT_DATA, &mut data);
        if !data.is_double() {
            let env_ref = e(env);
            env_ref.set_last_error("object is not wrapped");
            return NAPI_INVALID_ARG;
        }
        let undef = UndefinedValue();
        for slot in [SLOT_DATA, SLOT_FINALIZE, SLOT_HINT, SLOT_ENV] {
            JS_SetReservedSlot(obj, slot, &undef);
        }
        *result = data.to_private() as *mut c_void;
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（vendored js_native_api.h:336；data 可空 = null external）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_external(
    env: napi_env,
    data: *mut c_void,
    finalize_cb: sys::node_api_basic_finalize,
    finalize_hint: *mut c_void,
    result: *mut napi_value,
) -> napi_status {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    let env_ref = unsafe { e(env) };
    // SAFETY：对象先 rooted 再落槽；slots 全 PrivateValue（GC 不追）。
    unsafe {
        let Some(base) = object_prototype(&mut cx) else {
            return NAPI_GENERIC_FAILURE;
        };
        rooted!(&in(cx) let base_root: *mut JSObject = base.to_object());
        let obj = JS_NewObjectWithGivenProto(
            cx.raw_cx(),
            &NAPI_EXTERNAL_CLASS,
            raw_handle(&base_root.get()),
        );
        if obj.is_null() {
            return NAPI_GENERIC_FAILURE;
        }
        rooted!(&in(cx) let obj_root: *mut JSObject = obj);
        JS_SetReservedSlot(obj_root.get(), SLOT_DATA, &PrivateValue(data));
        if let Some(f) = finalize_cb {
            JS_SetReservedSlot(obj_root.get(), SLOT_FINALIZE, &PrivateValue(f as *const c_void));
            JS_SetReservedSlot(obj_root.get(), SLOT_HINT, &PrivateValue(finalize_hint));
            JS_SetReservedSlot(obj_root.get(), SLOT_ENV, &PrivateValue(env as *const c_void));
        }
        *result = env_ref.put(ObjectValue(obj_root.get()));
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（vendored js_native_api.h:341）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_value_external(
    env: napi_env,
    value: napi_value,
    result: *mut *mut c_void,
) -> napi_status {
    if value.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(value) };
    if !v.is_object() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：类判定 + reserved slots 读。
    unsafe {
        let obj = v.to_object();
        if !object_is_class(&mut cx, obj, &NAPI_EXTERNAL_CLASS) {
            return NAPI_INVALID_ARG;
        }
        let mut data = UndefinedValue();
        JS_GetReservedSlot(obj, SLOT_DATA, &mut data);
        if !data.is_double() {
            return NAPI_INVALID_ARG;
        }
        *result = data.to_private() as *mut c_void;
    }
    NAPI_OK
}
