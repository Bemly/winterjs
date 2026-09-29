//! napi buffer 族（plan-napi M3）：arraybuffer / typedarray / dataview /
//! node Buffer 全面。
//!
//! - ArrayBuffer 走 `JS::NewArrayBuffer` / `JS::NewExternalArrayBuffer`。
//!   **数据指针稳定性（§4.75，vue-project 138/139 根因）**：Node/V8 契约是
//!   create_arraybuffer/buffer 交出的 data 指针终身稳定；SM 自建小 AB 用
//!   inline 存储 GC 可搬移——故 `napi_create_arraybuffer`/`napi_create_buffer`/
//!   `napi_create_buffer_copy` 一律走 `new_owned_ab`（Rust 分配 + external AB
//!   桥 + free 回收）。JS 侧建 AB 传给 addon 的指针仍只保回调存活期有效
//!   （跨回调须重取 `napi_get_arraybuffer_info`，偏差记档）。
//! - external 的 finalize 经 `JS::BufferContentsFreeFunc` 桥（Box 携带 env/
//!   cb/hint；本运行时单 GC 线程 + FOREGROUND 语义，free 恒主线程——SM 文档
//!   的"任意线程"在本运行时不会发生，记档）。
//! - typedarray 建/读全走 global 构造器（`__wjs2_napi_new`）——11 种含
//!   BigInt64/BigUint64 一把覆盖；napi 与 SM 的 Scalar::Type 序号不同
//!   （clamped 2↔8），显式映射表双向。
//! - Buffer = Uint8Array + Buffer.prototype（`__wjs2_napi_bufferify`），与
//!   Node 实例形状一致（instanceof Buffer ✓）。

use std::ffi::{c_void, CStr};
use std::ptr;

use mozjs::context::JSContext;
use mozjs::glue::GetUint8ArrayLengthAndData;
use mozjs::jsapi::{JSObject, JS_NewDataView, JS_InstanceOf, JS_IsTypedArrayObject};
use mozjs::jsval::{JSVal, ObjectValue};
use mozjs::rooted;

use crate::jsapi_glue::{call_one, call_two, get_prop_value, raw_handle};
use crate::napi::api::{cx_of, e, NAPI_GENERIC_FAILURE, NAPI_INVALID_ARG, NAPI_OK};
use crate::napi::sys::{self, napi_env, napi_status, napi_value, napi_typedarray_type};

// ── ArrayBuffer ──────────────────────────────────────────────────────────

/// # Safety
/// N-API 约定（vendored js_native_api.h:428）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_arraybuffer(
    env: napi_env,
    byte_length: usize,
    data: *mut *mut c_void,
    result: *mut napi_value,
) -> napi_status {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    let env_ref = unsafe { e(env) };
    // SAFETY：自有稳定存储（§4.75）；对象建后即 rooted，落槽前无 GC 点。
    unsafe {
        let Some((ptr, ab)) = new_owned_ab(&mut cx, byte_length) else {
            return NAPI_GENERIC_FAILURE;
        };
        rooted!(&in(cx) let ab_root: *mut JSObject = ab);
        if !data.is_null() {
            *data = ptr;
        }
        *result = env_ref.put(ObjectValue(ab_root.get()));
    }
    NAPI_OK
}

/// ArrayBuffer 数据指针 + 长度（`GetArrayBufferLengthAndData`，detached 返 0/空）。
///
/// # Safety
/// `ab` 为已验 ArrayBuffer 对象。
unsafe fn ab_data(ab: *mut JSObject) -> (usize, *mut u8) {
    // SAFETY：无 GC 点（nogc 令牌）；isShared 恒 false（SAB 面未开）。
    unsafe {
        let mut len = 0usize;
        let mut shared = false;
        let mut data: *mut u8 = ptr::null_mut();
        mozjs::jsapi::JS::GetArrayBufferLengthAndData(ab, &mut len, &mut shared, &mut data);
        (len, data)
    }
}

/// external finalize 桥（BufferContentsFreeFunc → node_api_basic_finalize）。
struct ExtBridge {
    env: napi_env,
    cb: sys::node_api_basic_finalize,
    hint: *mut c_void,
}

/// # Safety
/// 引擎在 ArrayBuffer 回收时调用（本运行时恒主线程，见模块头注）。
unsafe extern "C" fn ext_ab_free(contents: *mut c_void, user_data: *mut c_void) {
    // SAFETY：bridge 由 create_external_arraybuffer 以 Box::into_raw 交出。
    unsafe {
        let b = Box::from_raw(user_data as *mut ExtBridge);
        if let Some(f) = b.cb {
            // §4.78：GC sweep 内禁 addon 回调（改 env 表/写堆即腐坏 GC）——
            // 入队，`asyncwork::dispatch` 入口 / end_session 安全点执行。
            // data（contents）随 cb 交还 addon 释放（external 语义）。
            let env_ref = &mut *(b.env as *mut crate::napi::env::NapiEnv);
            env_ref.pending_finalizers.push(crate::napi::class::PendingFinalize {
                env: b.env,
                data: contents,
                finalize: Some(f),
                hint: b.hint,
            });
        }
    }
}

/// 自有稳定存储桥（`new_owned_ab` 专用；free 时按记录的 len 重建 layout）。
struct OwnedAb {
    len: usize,
}

/// # Safety
/// 引擎在 ArrayBuffer 回收时调用；contents 即 `new_owned_ab` 的分配基址。
unsafe extern "C" fn owned_ab_free(contents: *mut c_void, user_data: *mut c_void) {
    // SAFETY：bridge 由 new_owned_ab 以 Box::into_raw 交出；layout 与分配时同形。
    unsafe {
        let b = Box::from_raw(user_data as *mut OwnedAb);
        if !contents.is_null() {
            let layout = std::alloc::Layout::from_size_align(b.len.max(1), 16)
                .expect("owned ab layout");
            std::alloc::dealloc(contents as *mut u8, layout);
        }
    }
}

/// 建带**自有稳定存储**的 ArrayBuffer（Rust 分配 + external AB 桥，零填充）。
///
/// Node/V8 契约：`napi_create_arraybuffer/buffer` 交出的 data 指针**终身稳定**；
/// SM 自建 AB 对小长度用 inline 存储、GC 可搬移——napi-rs 按 Node 语义持指针
/// 跨 GC 读写即腐坏引擎堆（vue-project dev/transform 链 138/139 的根因，
/// AGENTS §4.75）。故 napi 建面一律走 external 稳定存储。
///
/// # Safety
/// `cx` 在 realm 内。成功返回 (稳定数据基址, AB 对象)；失败已回收分配。
unsafe fn new_owned_ab(cx: &mut JSContext, len: usize) -> Option<(*mut c_void, *mut JSObject)> {
    // SAFETY：分配/零填在 GC 外；对象建后即 rooted 于调用方（返回即交根）。
    unsafe {
        let layout = std::alloc::Layout::from_size_align(len.max(1), 16).ok()?;
        let ptr = std::alloc::alloc(layout);
        if ptr.is_null() {
            return None;
        }
        std::ptr::write_bytes(ptr, 0, len.max(1));
        let ab = mozjs::jsapi::glue::NewExternalArrayBuffer(
            cx.raw_cx(),
            len,
            ptr as *mut c_void,
            Some(owned_ab_free),
            Box::into_raw(Box::new(OwnedAb { len })) as *mut c_void,
        );
        if ab.is_null() {
            std::alloc::dealloc(ptr, layout);
            return None;
        }
        Some((ptr as *mut c_void, ab))
    }
}

/// `new Uint8Array(ab)` 经全局构造器 + `bufferify`（external/buffer/copy 三面公用）。
///
/// # Safety
/// `env` 有效；`ab` 为已 rooted 的 ArrayBuffer 对象。
unsafe fn u8_over_ab(env: napi_env, ab: *mut JSObject) -> Option<JSVal> {
    // SAFETY：ctor/实参先 rooted 再调 `__wjs2_napi_new`（全语义构造）。
    unsafe {
        let mut cx = cx_of(env);
        rooted!(&in(cx) let ab_root: *mut JSObject = ab);
        let Some(u8ctor) = get_prop_value(&mut cx, crate::state::global(), c"Uint8Array") else {
            return None;
        };
        rooted!(&in(cx) let ctor_root = u8ctor);
        let args = mozjs::jsapi::JS::NewArrayObject1(cx.raw_cx(), 1);
        if args.is_null() {
            return None;
        }
        rooted!(&in(cx) let args_root = args);
        rooted!(&in(cx) let item = ObjectValue(ab_root.get()));
        if !mozjs::jsapi::JS_SetElement(
            cx.raw_cx(),
            raw_handle(&args_root.get()),
            0,
            raw_handle(item.as_ptr()),
        ) {
            return None;
        }
        let Some(helper) = get_prop_value(&mut cx, crate::state::global(), c"__wjs2_napi_new") else {
            return None;
        };
        let Some(r) = call_two(
            &mut cx,
            crate::state::global(),
            helper,
            ctor_root.get(),
            ObjectValue(args_root.get()),
        ) else {
            return None;
        };
        if !r.is_object() {
            return None;
        }
        bufferify(env, r.to_object())
    }
}

/// # Safety
/// N-API 约定（vendored js_native_api.h:434）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_external_arraybuffer(
    env: napi_env,
    external_data: *mut c_void,
    byte_length: usize,
    finalize_cb: sys::node_api_basic_finalize,
    finalize_hint: *mut c_void,
    result: *mut napi_value,
) -> napi_status {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    let env_ref = unsafe { e(env) };
    // SAFETY：对象先 rooted 再落槽；contents 所有权转给引擎（成功路径）。
    unsafe {
        let bridge = Box::new(ExtBridge {
            env,
            cb: finalize_cb,
            hint: finalize_hint,
        });
        let ab = mozjs::jsapi::glue::NewExternalArrayBuffer(
            cx.raw_cx(),
            byte_length,
            external_data,
            Some(ext_ab_free),
            Box::into_raw(bridge) as *mut c_void,
        );
        if ab.is_null() {
            return NAPI_GENERIC_FAILURE;
        }
        rooted!(&in(cx) let ab_root: *mut JSObject = ab);
        *result = env_ref.put(ObjectValue(ab_root.get()));
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（vendored js_native_api.h:451；data/byte_length 均可空出参）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_arraybuffer_info(
    env: napi_env,
    arraybuffer: napi_value,
    data: *mut *mut c_void,
    byte_length: *mut usize,
) -> napi_status {
    if arraybuffer.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(arraybuffer) };
    if !v.is_object() {
        return NAPI_INVALID_ARG;
    }
    // SAFETY：谓词 + 数据读取（无 GC 点）。
    unsafe {
        let obj = v.to_object();
        if !mozjs::jsapi::JS::IsArrayBufferObject(obj) {
            return sys::napi_status_napi_arraybuffer_expected;
        }
        let (len, ptr) = ab_data(obj);
        if !data.is_null() {
            *data = ptr as *mut c_void;
        }
        if !byte_length.is_null() {
            *byte_length = len;
        }
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_is_arraybuffer(
    env: napi_env,
    value: napi_value,
    result: *mut bool,
) -> napi_status {
    if value.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(value) };
    // SAFETY：谓词。
    unsafe {
        *result = v.is_object() && mozjs::jsapi::JS::IsArrayBufferObject(v.to_object());
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（vendored js_native_api.h:502 附近；Node-API 10 detach）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_detach_arraybuffer(
    env: napi_env,
    arraybuffer: napi_value,
) -> napi_status {
    if arraybuffer.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(arraybuffer) };
    if !v.is_object() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：detach 失败经 pending 报告。
    unsafe {
        rooted!(&in(cx) let ab_root: *mut JSObject = v.to_object());
        if !mozjs::jsapi::JS::DetachArrayBuffer(cx.raw_cx(), raw_handle(&ab_root.get())) {
            return NAPI_GENERIC_FAILURE;
        }
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_is_detached_arraybuffer(
    env: napi_env,
    value: napi_value,
    result: *mut bool,
) -> napi_status {
    if value.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(value) };
    if !v.is_object() {
        return NAPI_INVALID_ARG;
    }
    // SAFETY：谓词。
    unsafe {
        let obj = v.to_object();
        *result = mozjs::jsapi::JS::IsArrayBufferObject(obj)
            && !mozjs::jsapi::JS::ArrayBufferHasData(obj);
    }
    NAPI_OK
}

// ── TypedArray ───────────────────────────────────────────────────────────

/// napi typedarray 类型 → global 构造器名（`__wjs2_napi_new` 建）。
fn ta_ctor_name(t: napi_typedarray_type) -> Option<&'static CStr> {
    Some(match t {
        sys::napi_typedarray_type_napi_int8_array => c"Int8Array",
        sys::napi_typedarray_type_napi_uint8_array => c"Uint8Array",
        sys::napi_typedarray_type_napi_uint8_clamped_array => c"Uint8ClampedArray",
        sys::napi_typedarray_type_napi_int16_array => c"Int16Array",
        sys::napi_typedarray_type_napi_uint16_array => c"Uint16Array",
        sys::napi_typedarray_type_napi_int32_array => c"Int32Array",
        sys::napi_typedarray_type_napi_uint32_array => c"Uint32Array",
        sys::napi_typedarray_type_napi_float32_array => c"Float32Array",
        sys::napi_typedarray_type_napi_float64_array => c"Float64Array",
        sys::napi_typedarray_type_napi_bigint64_array => c"BigInt64Array",
        sys::napi_typedarray_type_napi_biguint64_array => c"BigUint64Array",
        _ => return None,
    })
}

/// SM `Scalar::Type` → napi 类型（**序号不同**：clamped 8↔2，显式表）。
fn sm_type_to_napi(t: mozjs::jsapi::JS::Scalar::Type) -> Option<napi_typedarray_type> {
    use mozjs::jsapi::JS::Scalar::Type as S;
    Some(match t {
        S::Int8 => sys::napi_typedarray_type_napi_int8_array,
        S::Uint8 => sys::napi_typedarray_type_napi_uint8_array,
        S::Uint8Clamped => sys::napi_typedarray_type_napi_uint8_clamped_array,
        S::Int16 => sys::napi_typedarray_type_napi_int16_array,
        S::Uint16 => sys::napi_typedarray_type_napi_uint16_array,
        S::Int32 => sys::napi_typedarray_type_napi_int32_array,
        S::Uint32 => sys::napi_typedarray_type_napi_uint32_array,
        S::Float32 => sys::napi_typedarray_type_napi_float32_array,
        S::Float64 => sys::napi_typedarray_type_napi_float64_array,
        S::BigInt64 => sys::napi_typedarray_type_napi_bigint64_array,
        S::BigUint64 => sys::napi_typedarray_type_napi_biguint64_array,
        _ => return None,
    })
}

/// # Safety
/// N-API 约定（vendored js_native_api.h:457；经 global 构造器 `new Kind(ab,
/// byteOffset, length)`，BigInt64 系同路径）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_typedarray(
    env: napi_env,
    type_: napi_typedarray_type,
    length: usize,
    arraybuffer: napi_value,
    byte_offset: usize,
    result: *mut napi_value,
) -> napi_status {
    if arraybuffer.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(ctor_name) = ta_ctor_name(type_) else {
        return NAPI_INVALID_ARG;
    };
    let ab_v = unsafe { e(env).get(arraybuffer) };
    if !ab_v.is_object() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    let env_ref = unsafe { e(env) };
    // SAFETY：构造器/实参数组先 rooted 再调用（__wjs2_napi_new = new 全语义）。
    unsafe {
        if !mozjs::jsapi::JS::IsArrayBufferObject(ab_v.to_object()) {
            return sys::napi_status_napi_arraybuffer_expected;
        }
        let Some(ctor) = get_prop_value(&mut cx, crate::state::global(), ctor_name) else {
            return NAPI_GENERIC_FAILURE;
        };
        rooted!(&in(cx) let ctor_root = ctor);
        let ab_obj = mozjs::jsapi::JS::NewArrayObject1(cx.raw_cx(), 3);
        if ab_obj.is_null() {
            return NAPI_GENERIC_FAILURE;
        }
        rooted!(&in(cx) let args_root = ab_obj);
        for (i, v) in [
            ab_v,
            mozjs::jsval::UInt32Value(byte_offset as u32),
            mozjs::jsval::UInt32Value(length as u32),
        ]
        .into_iter()
        .enumerate()
        {
            rooted!(&in(cx) let item_root = v);
            if !mozjs::jsapi::JS_SetElement(
                cx.raw_cx(),
                raw_handle(&args_root.get()),
                i as u32,
                raw_handle(item_root.as_ptr()),
            ) {
                return NAPI_GENERIC_FAILURE;
            }
        }
        let Some(helper) = get_prop_value(&mut cx, crate::state::global(), c"__wjs2_napi_new")
        else {
            return NAPI_GENERIC_FAILURE;
        };
        let Some(r) = call_two(
            &mut cx,
            crate::state::global(),
            helper,
            ctor_root.get(),
            ObjectValue(args_root.get()),
        ) else {
            return NAPI_GENERIC_FAILURE;
        };
        if !r.is_object() {
            return NAPI_GENERIC_FAILURE;
        }
        *result = env_ref.put(r);
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（vendored js_native_api.h:464；出参均可空）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_typedarray_info(
    env: napi_env,
    typedarray: napi_value,
    type_: *mut napi_typedarray_type,
    length: *mut usize,
    data: *mut *mut c_void,
    arraybuffer: *mut napi_value,
    byte_offset: *mut usize,
) -> napi_status {
    if typedarray.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(typedarray) };
    if !v.is_object() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：谓词 + 无 GC 点读面。
    unsafe {
        let obj = v.to_object();
        if !JS_IsTypedArrayObject(obj) {
            return NAPI_INVALID_ARG;
        }
        if !type_.is_null() {
            let Some(t) = sm_type_to_napi(mozjs::jsapi::JS_GetArrayBufferViewType(obj)) else {
                return NAPI_GENERIC_FAILURE;
            };
            *type_ = t;
        }
        if !length.is_null() {
            *length = mozjs::jsapi::JS_GetTypedArrayLength(obj);
        }
        if !byte_offset.is_null() {
            *byte_offset = mozjs::jsapi::JS_GetTypedArrayByteOffset(obj);
        }
        if !data.is_null() || !arraybuffer.is_null() {
            rooted!(&in(cx) let view_root: *mut JSObject = obj);
            let mut shared = false;
            // 入参 = 视图对象，返回 = 底层 ArrayBuffer（视图不能传空）
            let ab_obj = mozjs::jsapi::JS_GetArrayBufferViewBuffer(
                cx.raw_cx(),
                raw_handle(&view_root.get()),
                &mut shared,
            );
            if !arraybuffer.is_null() {
                *arraybuffer = e(env).put(ObjectValue(ab_obj));
            }
            if !data.is_null() {
                let (_, base) = ab_data(ab_obj);
                *data = base.add(mozjs::jsapi::JS_GetTypedArrayByteOffset(obj)) as *mut c_void;
            }
        }
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_is_typedarray(
    env: napi_env,
    value: napi_value,
    result: *mut bool,
) -> napi_status {
    if value.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(value) };
    // SAFETY：谓词。
    unsafe {
        *result = v.is_object() && JS_IsTypedArrayObject(v.to_object());
    }
    NAPI_OK
}

// ── DataView ─────────────────────────────────────────────────────────────

/// # Safety
/// N-API 约定（vendored js_native_api.h:472）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_dataview(
    env: napi_env,
    length: usize,
    arraybuffer: napi_value,
    byte_offset: usize,
    result: *mut napi_value,
) -> napi_status {
    if arraybuffer.is_null() || result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let ab_v = unsafe { e(env).get(arraybuffer) };
    if !ab_v.is_object() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    let env_ref = unsafe { e(env) };
    // SAFETY：JS_NewDataView（入参越界经 pending 报错）；对象 rooted 后落槽。
    unsafe {
        if !mozjs::jsapi::JS::IsArrayBufferObject(ab_v.to_object()) {
            return sys::napi_status_napi_arraybuffer_expected;
        }
        rooted!(&in(cx) let ab_root: *mut JSObject = ab_v.to_object());
        let dv = JS_NewDataView(
            cx.raw_cx(),
            raw_handle(&ab_root.get()),
            byte_offset,
            length,
        );
        if dv.is_null() {
            return NAPI_GENERIC_FAILURE;
        }
        rooted!(&in(cx) let dv_root: *mut JSObject = dv);
        *result = env_ref.put(ObjectValue(dv_root.get()));
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（vendored js_native_api.h:481；出参均可空）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_dataview_info(
    env: napi_env,
    dataview: napi_value,
    data_length: *mut usize,
    data: *mut *mut c_void,
    arraybuffer: *mut napi_value,
    byte_offset: *mut usize,
) -> napi_status {
    if dataview.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(dataview) };
    if !v.is_object() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    // SAFETY：谓词（DataView 类指针比对）+ 无 GC 点读面。
    unsafe {
        let obj = v.to_object();
        if !is_dataview(&mut cx, obj) {
            return NAPI_INVALID_ARG;
        }
        if !data_length.is_null() {
            *data_length = mozjs::jsapi::JS_GetArrayBufferViewByteLength(obj);
        }
        if !byte_offset.is_null() {
            *byte_offset = mozjs::jsapi::JS_GetArrayBufferViewByteOffset(obj);
        }
        if !data.is_null() || !arraybuffer.is_null() {
            rooted!(&in(cx) let view_root: *mut JSObject = obj);
            let mut shared = false;
            // 入参 = 视图对象，返回 = 底层 ArrayBuffer（视图不能传空）
            let ab_obj = mozjs::jsapi::JS_GetArrayBufferViewBuffer(
                cx.raw_cx(),
                raw_handle(&view_root.get()),
                &mut shared,
            );
            if !arraybuffer.is_null() {
                *arraybuffer = e(env).put(ObjectValue(ab_obj));
            }
            if !data.is_null() {
                let (_, base) = ab_data(ab_obj);
                *data = base.add(mozjs::jsapi::JS_GetArrayBufferViewByteOffset(obj)) as *mut c_void;
            }
        }
    }
    NAPI_OK
}

/// DataView 判定（`JS::DataView::FixedLengthClassPtr` 指针比对）。
fn is_dataview(cx: &mut JSContext, obj: *mut JSObject) -> bool {
    // SAFETY：谓词（args=null：不匹配不置 pending）。
    unsafe {
        rooted!(&in(cx) let obj_root: *mut JSObject = obj);
        JS_InstanceOf(
            cx.raw_cx(),
            raw_handle(&obj_root.get()),
            mozjs::jsapi::JS::DataView_FixedLengthClassPtr,
            ptr::null_mut(),
        )
    }
}

/// # Safety
/// N-API 约定。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_is_dataview(
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
    // SAFETY：谓词。
    unsafe {
        *result = is_dataview(&mut cx, v.to_object());
    }
    NAPI_OK
}

// ── node Buffer ──────────────────────────────────────────────────────────

/// Uint8Array 挂 Buffer.prototype（Node 实例形状）。
///
/// # Safety
/// `env`/`obj` 语义同 napi 面；失败（无 Buffer 全局）返 None。
unsafe fn bufferify(env: napi_env, obj: *mut JSObject) -> Option<JSVal> {
    // SAFETY：helper 调用（call_one 前置；异常经 pending）。
    unsafe {
        let mut cx = cx_of(env);
        let Some(helper) = get_prop_value(&mut cx, crate::state::global(), c"__wjs2_napi_bufferify")
        else {
            return None;
        };
        rooted!(&in(cx) let obj_root: *mut JSObject = obj);
        call_one(&mut cx, crate::state::global(), helper, ObjectValue(obj_root.get()))
    }
}

/// Buffer 实例的数据指针 + 字节长（u8 视图读面）。
///
/// # Safety
/// `obj` 为 Uint8Array。
unsafe fn u8_data(obj: *mut JSObject) -> (usize, *mut u8) {
    // SAFETY：无 GC 点（nogc 语义同 AB 读面）。
    unsafe {
        let mut len = 0usize;
        let mut shared = false;
        let mut data: *mut u8 = ptr::null_mut();
        GetUint8ArrayLengthAndData(obj, &mut len, &mut shared, &mut data);
        (len, data)
    }
}

/// # Safety
/// N-API 约定（vendored node_api.h:118）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_buffer(
    env: napi_env,
    length: usize,
    data: *mut *mut c_void,
    result: *mut napi_value,
) -> napi_status {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    let env_ref = unsafe { e(env) };
    // SAFETY：自有稳定存储（§4.75）；对象先 rooted 再 bufferify/落槽。
    unsafe {
        let Some((ptr, ab)) = new_owned_ab(&mut cx, length) else {
            return NAPI_GENERIC_FAILURE;
        };
        rooted!(&in(cx) let ab_root: *mut JSObject = ab);
        let Some(bv) = u8_over_ab(env, ab_root.get()) else {
            return NAPI_GENERIC_FAILURE;
        };
        if !data.is_null() {
            *data = ptr;
        }
        *result = env_ref.put(bv);
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（vendored node_api.h:142）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_buffer_copy(
    env: napi_env,
    length: usize,
    data: *const c_void,
    result_data: *mut *mut c_void,
    result: *mut napi_value,
) -> napi_status {
    if result.is_null() || (length > 0 && data.is_null()) {
        return NAPI_INVALID_ARG;
    }
    let mut cx = unsafe { cx_of(env) };
    let env_ref = unsafe { e(env) };
    // SAFETY：同 create_buffer；拷贝写自有稳定存储（无 GC 点）。
    unsafe {
        let Some((ptr, ab)) = new_owned_ab(&mut cx, length) else {
            return NAPI_GENERIC_FAILURE;
        };
        rooted!(&in(cx) let ab_root: *mut JSObject = ab);
        if !result_data.is_null() {
            *result_data = ptr;
        }
        std::ptr::copy_nonoverlapping(data as *const u8, ptr as *mut u8, length);
        let Some(bv) = u8_over_ab(env, ab_root.get()) else {
            return NAPI_GENERIC_FAILURE;
        };
        *result = env_ref.put(bv);
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（vendored node_api.h:124；external arraybuffer + Buffer 形状）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_create_external_buffer(
    env: napi_env,
    length: usize,
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
    // SAFETY：external AB 先建（bridge 所有权转引擎），再 bufferify 落槽。
    unsafe {
        let bridge = Box::new(ExtBridge {
            env,
            cb: finalize_cb,
            hint: finalize_hint,
        });
        let ab = mozjs::jsapi::glue::NewExternalArrayBuffer(
            cx.raw_cx(),
            length,
            data,
            Some(ext_ab_free),
            Box::into_raw(bridge) as *mut c_void,
        );
        if ab.is_null() {
            return NAPI_GENERIC_FAILURE;
        }
        rooted!(&in(cx) let ab_root: *mut JSObject = ab);
        // new Uint8Array(ab) 经构造器（全语义，含越界抛错）。
        let Some(bv) = u8_over_ab(env, ab_root.get()) else {
            return NAPI_GENERIC_FAILURE;
        };
        *result = env_ref.put(bv);
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（vendored node_api.h:147）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_is_buffer(
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
    // SAFETY：prelude helper（instanceof Buffer；异常不可能，返回值恒布尔）。
    unsafe {
        let Some(helper) = get_prop_value(&mut cx, crate::state::global(), c"__wjs2_napi_is_buffer")
        else {
            return NAPI_GENERIC_FAILURE;
        };
        rooted!(&in(cx) let v_root = v);
        let Some(r) = call_one(&mut cx, crate::state::global(), helper, v_root.get()) else {
            return NAPI_GENERIC_FAILURE;
        };
        *result = r.is_boolean() && r.to_boolean();
    }
    NAPI_OK
}

/// # Safety
/// N-API 约定（vendored node_api.h:150；data/length 均可空）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_get_buffer_info(
    env: napi_env,
    value: napi_value,
    data: *mut *mut c_void,
    length: *mut usize,
) -> napi_status {
    if value.is_null() {
        return NAPI_INVALID_ARG;
    }
    let v = unsafe { e(env).get(value) };
    if !v.is_object() {
        return NAPI_INVALID_ARG;
    }
    // SAFETY：u8 视图读面（无 GC 点）。
    unsafe {
        let obj = v.to_object();
        if !JS_IsTypedArrayObject(obj) {
            return NAPI_INVALID_ARG;
        }
        let (len, ptr) = u8_data(obj);
        if !data.is_null() {
            *data = ptr as *mut c_void;
        }
        if !length.is_null() {
            *length = len;
        }
    }
    NAPI_OK
}
