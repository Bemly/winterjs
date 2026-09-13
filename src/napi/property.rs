//! napi 属性/元素面（plan-napi M1/M2）：named 走 char* JSAPI（glue 同款），
//! generic key 暂收 string（symbol key 记档 M3），元素走 u32 JSAPI，
//! `define_properties`/`define_class` 共用 `define_one`（value/method/getter/
//! setter + attrs 位映射）。

use std::ffi::{c_char, CStr, CString};

use mozjs::context::JSContext;
use mozjs::gc::ValueArray;
use mozjs::jsapi::{JSObject, JSPROP_ENUMERATE, JSPROP_PERMANENT, JSPROP_READONLY};
use mozjs::jsval::{JSVal, ObjectValue, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{call_one, get_prop_value, raw_handle, raw_handle_mut, value_to_string};
use crate::napi::api::{cx_of, e, napi_create_string_utf8, napi_set_named_property, set_via_helper};
use crate::napi::sys;
use crate::napi::sys::{napi_env, napi_status, napi_value};

const NAPI_OK: napi_status = sys::napi_status_napi_ok;
const NAPI_INVALID_ARG: napi_status = sys::napi_status_napi_invalid_arg;
const NAPI_GENERIC_FAILURE: napi_status = sys::napi_status_napi_generic_failure;

/// napi attrs → JSPROP 位映射。napi 位义（writable=1/enumerable=2/configurable=4，
/// "能力位"）与 JSPROP（ENUMERATE=1/READONLY=2/PERMANENT=4，"限制位"）完全不同
/// ——M1 直传是位撞位 bug（napi_writable 变可枚举、enumerable 变只读、
/// configurable 变永久，2026-09-14 实测修正）。
/// 口径 = Object.defineProperty 默认：默认全否，按位解禁。
/// napi_static（1024）不在此处理（define_class 拆分放置目标）。
pub(crate) fn attrs_to_js(attrs: u32) -> u32 {
    let mut f = (JSPROP_READONLY | JSPROP_PERMANENT) as u32;
    if attrs & sys::napi_property_attributes_napi_writable != 0 {
        f &= !JSPROP_READONLY as u32;
    }
    if attrs & sys::napi_property_attributes_napi_enumerable != 0 {
        f |= JSPROP_ENUMERATE as u32;
    }
    if attrs & sys::napi_property_attributes_napi_configurable != 0 {
        f &= !JSPROP_PERMANENT as u32;
    }
    f
}

/// 单 descriptor 定义（define_properties / define_class 共用；M2）。
/// 放置目标 `obj_v` 已验证为对象；napi_static 位由调用方拆分（此处忽略）。
/// 优先级：method → getter/setter 访问器 → value（Node 同序）。
///
/// # Safety
/// `env` 有效；`d` 指向 addon 提供的合法 descriptor（调用期有效）。
pub(crate) unsafe fn define_one(
    env: napi_env,
    cx: &mut JSContext,
    obj_v: JSVal,
    d: &sys::napi_property_descriptor,
) -> napi_status {
    unsafe {
        // 键名：utf8name 优先，回落 name 值（string；symbol 键记档 M3）。
        let name: CString = if !d.utf8name.is_null() {
            CString::from(CStr::from_ptr(d.utf8name))
        } else if !d.name.is_null() {
            let kv = e(env).get(d.name);
            if !kv.is_string() {
                return NAPI_INVALID_ARG;
            }
            let s = value_to_string(cx, kv);
            match CString::new(s.replace('\0', " ")) {
                Ok(c) => c,
                Err(_) => return NAPI_GENERIC_FAILURE,
            }
        } else {
            return NAPI_INVALID_ARG;
        };

        if let Some(cb) = d.method {
            // 方法：建 trampoline 函数对象后按 attrs 定义。
            let mut fnv: napi_value = std::ptr::null_mut();
            let st = crate::napi::api::napi_create_function(
                env,
                name.as_ptr(),
                usize::MAX,
                Some(cb),
                d.data,
                &mut fnv,
            );
            if st != NAPI_OK {
                return st;
            }
            let val = e(env).get(fnv);
            return define_value(cx, obj_v, &name, val, attrs_to_js(d.attributes));
        }
        if d.getter.is_some() || d.setter.is_some() {
            return define_accessor(env, cx, obj_v, &name, d);
        }
        // 数据属性（value 可空 = undefined）。
        let val = if d.value.is_null() {
            UndefinedValue()
        } else {
            e(env).get(d.value)
        };
        define_value(cx, obj_v, &name, val, attrs_to_js(d.attributes))
    }
}

/// 数据属性定义（JS_DefineProperty，attrs 已映射）。
///
/// # Safety
/// `env` 有效；name/value 语义同上。
unsafe fn define_value(
    cx: &mut JSContext,
    obj_v: JSVal,
    name: &CStr,
    val: JSVal,
    attrs: u32,
) -> napi_status {
    // SAFETY：对象与值先 rooted 再 define（define 期可能 GC）。
    unsafe {
        rooted!(&in(cx) let obj_root = obj_v.to_object());
        rooted!(&in(cx) let val_root = val);
        if !mozjs::jsapi::JS_DefineProperty(
            cx.raw_cx(),
            raw_handle(&obj_root.get()),
            name.as_ptr(),
            raw_handle(val_root.as_ptr()),
            attrs,
        ) {
            return NAPI_GENERIC_FAILURE;
        }
    }
    NAPI_OK
}

/// 访问器定义：getter/setter 建为 trampoline 函数对象，经 prelude
/// `__wjs_napi_accessor`（Object.defineProperty）落地——setter 传 undefined
/// 即 Node getter-only 语义；免 JSAPI 访问器定义面（getter/setter 旗帜位
/// 与 attrs 组合的断言雷区）。
///
/// # Safety
/// `env` 有效；`d` 语义同 define_one。
unsafe fn define_accessor(
    env: napi_env,
    cx: &mut JSContext,
    obj_v: JSVal,
    name: &CStr,
    d: &sys::napi_property_descriptor,
) -> napi_status {
    unsafe {
        let mut get_v: JSVal = UndefinedValue();
        if let Some(cb) = d.getter {
            let mut fnv: napi_value = std::ptr::null_mut();
            let st = crate::napi::api::napi_create_function(
                env,
                name.as_ptr(),
                usize::MAX,
                Some(cb),
                d.data,
                &mut fnv,
            );
            if st != NAPI_OK {
                return st;
            }
            get_v = e(env).get(fnv);
        }
        let mut set_v: JSVal = UndefinedValue();
        if let Some(cb) = d.setter {
            let mut fnv: napi_value = std::ptr::null_mut();
            let st = crate::napi::api::napi_create_function(
                env,
                name.as_ptr(),
                usize::MAX,
                Some(cb),
                d.data,
                &mut fnv,
            );
            if st != NAPI_OK {
                return st;
            }
            set_v = e(env).get(fnv);
        }
        // 实参：obj / name(string) / getter / setter / enumerable / configurable。
        let mut name_v: napi_value = std::ptr::null_mut();
        let st = napi_create_string_utf8(env, name.as_ptr(), usize::MAX, &mut name_v);
        if st != NAPI_OK {
            return st;
        }
        let attrs = d.attributes;
        let enum_v = mozjs::jsval::BooleanValue(
            attrs & sys::napi_property_attributes_napi_enumerable != 0,
        );
        let conf_v = mozjs::jsval::BooleanValue(
            attrs & sys::napi_property_attributes_napi_configurable != 0,
        );
        let Some(helper) = get_prop_value(cx, crate::state::global(), c"__wjs_napi_accessor")
        else {
            return NAPI_GENERIC_FAILURE;
        };
        let obj_arg = obj_v;
        let name_arg = e(env).get(name_v);
        rooted!(&in(cx) let argv = ValueArray::new([
            obj_arg, name_arg, get_v, set_v, enum_v, conf_v,
        ]));
        rooted!(&in(cx) let helper_root = helper);
        rooted!(&in(cx) let mut rval = UndefinedValue());
        let args = mozjs::jsapi::HandleValueArray::from(&argv);
        // addon 侧按契约查 pending exception（失败 = defineProperty 抛错）。
        if !mozjs::jsapi::JS_CallFunctionValue(
            cx.raw_cx(),
            raw_handle(&crate::state::global()),
            raw_handle(helper_root.as_ptr()),
            &args,
            raw_handle_mut(rval.as_ptr()),
        ) {
            return NAPI_GENERIC_FAILURE;
        }
        NAPI_OK
    }
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_named_property(
    env: napi_env,
    obj: napi_value,
    utf8name: *const c_char,
    result: *mut napi_value,
) -> napi_status {
    if obj.is_null() || result.is_null() || utf8name.is_null() {
        return NAPI_INVALID_ARG;
    }
    let obj_v = unsafe { e(env).get(obj) };
    if !obj_v.is_object() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    let env_ref = unsafe { e(env) };
    // SAFETY：JS_GetProperty 无 GC 点风险由引擎管；槽位读取。
    unsafe {
        rooted!(&in(cx) let obj_root = obj_v.to_object());
        rooted!(&in(cx) let mut v = UndefinedValue());
        if !mozjs::jsapi::JS_GetProperty(
            cx.raw_cx(),
            raw_handle(&obj_root.get()),
            utf8name,
            raw_handle_mut(v.as_ptr()),
        ) {
            return NAPI_GENERIC_FAILURE;
        }
        *result = env_ref.put(v.get());
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_has_named_property(
    env: napi_env,
    obj: napi_value,
    utf8name: *const c_char,
    result: *mut bool,
) -> napi_status {
    if obj.is_null() || result.is_null() || utf8name.is_null() {
        return NAPI_INVALID_ARG;
    }
    let obj_v = unsafe { e(env).get(obj) };
    if !obj_v.is_object() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：谓词。
    unsafe {
        rooted!(&in(cx) let obj_root = obj_v.to_object());
        let mut has = false;
        if !mozjs::jsapi::JS_HasProperty(cx.raw_cx(), raw_handle(&obj_root.get()), utf8name, &mut has) {
            return NAPI_GENERIC_FAILURE;
        }
        *result = has;
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_has_own_property(
    env: napi_env,
    obj: napi_value,
    key: napi_value,
    result: *mut bool,
) -> napi_status {
    if obj.is_null() || key.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let obj_v = unsafe { e(env).get(obj) };
    if !obj_v.is_object() {
        return NAPI_INVALID_ARG;
    }
    // SAFETY：键取串后走 char* 谓词（key 为 string；symbol 键 M2 记档）。
    unsafe {
        let name = match key_name(env, key) {
            Ok(n) => n,
            Err(st) => return st,
        };
        return has_own_property_str(env, obj, name.as_ptr(), result);
    }
}

/// char* 版谓词（key_name 路径复用）。
///
/// # Safety
/// `utf8name` 为合法 CStr。
unsafe fn has_own_property_str(
    env: napi_env,
    obj: napi_value,
    utf8name: *const c_char,
    result: *mut bool,
) -> napi_status {
    let obj_v = unsafe { e(env).get(obj) };
    if !obj_v.is_object() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：谓词。
    unsafe {
        rooted!(&in(cx) let obj_root = obj_v.to_object());
        let mut has = false;
        if !mozjs::jsapi::JS_HasOwnProperty(cx.raw_cx(), raw_handle(&obj_root.get()), utf8name, &mut has) {
            return NAPI_GENERIC_FAILURE;
        }
        *result = has;
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_delete_property(
    env: napi_env,
    obj: napi_value,
    key: napi_value,
    result: *mut bool,
) -> napi_status {
    if obj.is_null() || key.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let obj_v = unsafe { e(env).get(obj) };
    if !obj_v.is_object() {
        return NAPI_INVALID_ARG;
    }
    // SAFETY：键取串后走 char* 删除。
    unsafe {
        let name = match key_name(env, key) {
            Ok(n) => n,
            Err(st) => return st,
        };
        return delete_property_str(env, obj, name.as_ptr(), result);
    }
}

/// char* 版删除（key_name 路径复用）。
///
/// # Safety
/// `utf8name` 为合法 CStr。
unsafe fn delete_property_str(
    env: napi_env,
    obj: napi_value,
    utf8name: *const c_char,
    result: *mut bool,
) -> napi_status {
    let obj_v = unsafe { e(env).get(obj) };
    if !obj_v.is_object() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：DeleteProperty 经 ObjectOpResult 报告（code==OkCode 即成功）。
    unsafe {
        rooted!(&in(cx) let obj_root = obj_v.to_object());
        let mut op = mozjs::jsapi::JS::ObjectOpResult { code_: 1 };
        if !mozjs::jsapi::JS_DeleteProperty(cx.raw_cx(), raw_handle(&obj_root.get()), utf8name, &mut op) {
            return NAPI_GENERIC_FAILURE;
        }
        *result = op.code_ == mozjs::jsapi::JS::ObjectOpResult_SpecialCodes::OkCode as usize;
    }
    NAPI_OK
}

/// generic key（string 值键；symbol 键记档 M2——rolldown 名单以 named 为主）。
///
/// # Safety
/// N-API 约定。
unsafe fn key_name(env: napi_env, key: napi_value) -> Result<CString, napi_status> {
    let v = unsafe { e(env).get(key) };
    if !v.is_string() {
        return Err(NAPI_INVALID_ARG);
    }
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：key 已验字符串。
    let s = value_to_string(&mut cx, v);
    CString::new(s.replace('\0', " ")).map_err(|_| NAPI_GENERIC_FAILURE)
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_property(
    env: napi_env,
    obj: napi_value,
    key: napi_value,
    result: *mut napi_value,
) -> napi_status {
    if obj.is_null() || key.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    // SAFETY：键名取串后同 named 路径。
    unsafe {
        let name = match key_name(env, key) {
            Ok(n) => n,
            Err(st) => return st,
        };
        napi_get_named_property(env, obj, name.as_ptr(), result)
    }
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_set_property(
    env: napi_env,
    obj: napi_value,
    key: napi_value,
    value: napi_value,
) -> napi_status {
    if obj.is_null() || key.is_null() || value.is_null() {
        return NAPI_INVALID_ARG;
    }
    // SAFETY：同上。
    unsafe {
        let name = match key_name(env, key) {
            Ok(n) => n,
            Err(st) => return st,
        };
        napi_set_named_property(env, obj, name.as_ptr(), value)
    }
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_has_property(
    env: napi_env,
    obj: napi_value,
    key: napi_value,
    result: *mut bool,
) -> napi_status {
    if obj.is_null() || key.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    // SAFETY：同上。
    unsafe {
        let name = match key_name(env, key) {
            Ok(n) => n,
            Err(st) => return st,
        };
        napi_has_named_property(env, obj, name.as_ptr(), result)
    }
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_element(
    env: napi_env,
    obj: napi_value,
    index: u32,
    result: *mut napi_value,
) -> napi_status {
    if obj.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let obj_v = unsafe { e(env).get(obj) };
    if !obj_v.is_object() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    let env_ref = unsafe { e(env) };
    // SAFETY：元素槽位。
    unsafe {
        rooted!(&in(cx) let obj_root = obj_v.to_object());
        rooted!(&in(cx) let mut v = UndefinedValue());
        if !mozjs::jsapi::JS_GetElement(cx.raw_cx(), raw_handle(&obj_root.get()), index, raw_handle_mut(v.as_ptr())) {
            return NAPI_GENERIC_FAILURE;
        }
        *result = env_ref.put(v.get());
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_set_element(
    env: napi_env,
    obj: napi_value,
    index: u32,
    value: napi_value,
) -> napi_status {
    if obj.is_null() || value.is_null() {
        return NAPI_INVALID_ARG;
    }
    let obj_v = unsafe { e(env).get(obj) };
    let val = unsafe { e(env).get(value) };
    if !obj_v.is_object() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：非严格 set 语义经 sloppy helper（同 set_named_property，记档）。
    unsafe {
        return set_via_helper(&mut cx, obj_v, mozjs::jsval::UInt32Value(index), val);
    }
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_has_element(
    env: napi_env,
    obj: napi_value,
    index: u32,
    result: *mut bool,
) -> napi_status {
    if obj.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let obj_v = unsafe { e(env).get(obj) };
    if !obj_v.is_object() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：谓词。
    unsafe {
        rooted!(&in(cx) let obj_root = obj_v.to_object());
        let mut has = false;
        if !mozjs::jsapi::JS_HasElement(cx.raw_cx(), raw_handle(&obj_root.get()), index, &mut has) {
            return NAPI_GENERIC_FAILURE;
        }
        *result = has;
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_delete_element(
    env: napi_env,
    obj: napi_value,
    index: u32,
    result: *mut bool,
) -> napi_status {
    if obj.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let obj_v = unsafe { e(env).get(obj) };
    if !obj_v.is_object() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：DeleteElement 经 ObjectOpResult。
    unsafe {
        rooted!(&in(cx) let obj_root = obj_v.to_object());
        let mut op = mozjs::jsapi::JS::ObjectOpResult { code_: 1 };
        if !mozjs::jsapi::JS_DeleteElement(cx.raw_cx(), raw_handle(&obj_root.get()), index, &mut op) {
            return NAPI_GENERIC_FAILURE;
        }
        *result = op.code_ == mozjs::jsapi::JS::ObjectOpResult_SpecialCodes::OkCode as usize;
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（= `Object.keys`：自有可枚举 string 键）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_property_names(
    env: napi_env,
    obj: napi_value,
    result: *mut napi_value,
) -> napi_status {
    if obj.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let obj_v = unsafe { e(env).get(obj) };
    if !obj_v.is_object() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    let env_ref = unsafe { e(env) };
    // 实现路线：global `Object.keys` 直调（语义 = 自有可枚举 string 键，
    // 免 IdVector 机制；偏差记 plan-napi §4）。
    // SAFETY：global 方法单参调用（call_one 前置）。
    unsafe {
        rooted!(&in(cx) let obj_root = obj_v.to_object());
        let Some(obj_ctor) = get_prop_value(&mut cx, crate::state::global(), c"Object") else {
            return NAPI_GENERIC_FAILURE;
        };
        if !obj_ctor.is_object() {
            return NAPI_GENERIC_FAILURE;
        }
        rooted!(&in(cx) let obj_ctor_root = obj_ctor.to_object());
        let Some(keys_fn) = get_prop_value(&mut cx, obj_ctor_root.get(), c"keys") else {
            return NAPI_GENERIC_FAILURE;
        };
        rooted!(&in(cx) let arg = ObjectValue(obj_root.get()));
        let Some(arr_v) = call_one(&mut cx, crate::state::global(), keys_fn, arg.get()) else {
            return NAPI_GENERIC_FAILURE;
        };
        *result = env_ref.put(arr_v);
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_prototype(
    env: napi_env,
    obj: napi_value,
    result: *mut napi_value,
) -> napi_status {
    if obj.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let obj_v = unsafe { e(env).get(obj) };
    if !obj_v.is_object() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    let env_ref = unsafe { e(env) };
    // SAFETY：GetPrototype 谓词；proto null → JS null 值。
    unsafe {
        rooted!(&in(cx) let obj_root = obj_v.to_object());
        rooted!(&in(cx) let mut proto: *mut JSObject = std::ptr::null_mut());
        if !mozjs::jsapi::JS_GetPrototype(cx.raw_cx(), raw_handle(&obj_root.get()), raw_handle_mut(proto.as_ptr())) {
            return NAPI_GENERIC_FAILURE;
        }
        *result = env_ref.put(mozjs::jsval::ObjectOrNullValue(proto.get()));
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（数组 length；非数组按 length 属性语义直读，记档）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_array_length(
    env: napi_env,
    value: napi_value,
    result: *mut u32,
) -> napi_status {
    if value.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(value) };
    if !v.is_object() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：length 属性直读（glue get_prop_value）。
    unsafe {
        let Some(len_v) = get_prop_value(&mut cx, v.to_object(), c"length") else {
            return NAPI_GENERIC_FAILURE;
        };
        if !len_v.is_number() {
            return sys::napi_status_napi_arraybuffer_expected;
        }
        *result = len_v.to_number() as u32;
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（descriptor 数组：value/method/getter/setter + attrs；实现走
/// define_one，与 define_class 同一机制）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_define_properties(
    env: napi_env,
    obj: napi_value,
    property_count: usize,
    properties: *const sys::napi_property_descriptor,
) -> napi_status {
    if obj.is_null() || (property_count > 0 && properties.is_null()) {
        return NAPI_INVALID_ARG;
    }
    let obj_v = unsafe { e(env).get(obj) };
    if !obj_v.is_object() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：descriptor 数组由 addon 提供（napi 语义：调用期有效）；
    // define_one 内部先建函数对象（rooted）再 define。
    unsafe {
        for i in 0..property_count {
            let d = &*properties.add(i);
            let st = define_one(env, &mut cx, obj_v, d);
            if st != NAPI_OK {
                return st;
            }
        }
    }
    NAPI_OK
}
