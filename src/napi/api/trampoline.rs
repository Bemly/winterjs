//! napi trampoline：JS native → addon C 回调（reserved slots 携指针/data）。

use super::*;
use std::ffi::{c_void};
use crate::napi::sys::{napi_callback_info, napi_env, napi_value};
use crate::jsapi_glue::get_prop_value;
use crate::jsapi_glue::raw_handle;
use mozjs::jsapi::JS_IsExceptionPending;
use mozjs::jsapi::JS_NewObjectWithGivenProto;
use mozjs::jsapi::JS_NewPlainObject;
use mozjs::jsapi::JSContext as RawJSContext;
use mozjs::jsval::ObjectValue;
use mozjs::jsval::UndefinedValue;
use crate::napi::env::CbInfo;

use mozjs::context::JSContext;
use mozjs::jsapi::JSObject;
use mozjs::jsval::JSVal;
use mozjs::rooted;

// ── trampoline：JS native 函数 → addon C 回调 ────────────────────────────
// reserved slots：0 = addon 回调指针、1 = data。帧布局（jsapi_glue Frame 约定）：
// vp[0]=callee/rval 槽、vp[1]=this、vp[2..]=实参。
// 构造帧（SM native 构造约定，mozjs_sys 153 实证）：vp[1] 为 JS_IS_CONSTRUCTING
// 魔数（魔数不可被 JS 侧持有，is_magic 即构造判据），引擎在 vp[2+argc] 写
// new.target（CallArgs.h `newTarget = argv_[argc_]`）；this 由 native 自建——
// 取 new.target.prototype（非对象回落 Object.prototype 默认建）。`clasp` 非
// null 时实例带该类（define_class 路径，class.rs；wrap 面依赖 reserved slots）。

pub unsafe extern "C" fn napi_trampoline(
    cx_raw: *mut RawJSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY：引擎回调帧 + 会话 env（JS 线程）；整个函数体即边界（UNSAFE-BOUNDARY：
    // reserved slots 指针须由 napi_create_function/define_class 写入；覆盖 tests/napi.rs）。
    unsafe { napi_trampoline_frame(cx_raw, argc, vp, std::ptr::null()) }
}

/// trampoline 共享帧（plain create_function 与 define_class 构造器两路）。
///
/// # Safety
/// 仅引擎回调帧内调用（cx_raw/vp 有效；env 会话单例）。
pub(crate) unsafe fn napi_trampoline_frame(
    cx_raw: *mut RawJSContext,
    argc: u32,
    vp: *mut JSVal,
    clasp: *const mozjs::jsapi::JSClass,
) -> bool {
    // SAFETY：引擎回调帧 + 会话 env + reserved slots 前置（UNSAFE-BOUNDARY，
    // 覆盖 tests/napi.rs 全量黑盒）。
    unsafe {
    let Some(env) = crate::state::napi_env_ptr() else {
        return false;
    };
    let callee = *vp;
    if !callee.is_object() {
        return false;
    }
    let fobj = callee.to_object();
    let cb_ptr = (*mozjs::jsapi::js::GetFunctionNativeReserved(fobj, 0)).to_private() as usize;
    let data = (*mozjs::jsapi::js::GetFunctionNativeReserved(fobj, 1)).to_private();
    if cb_ptr == 0 {
        return false;
    }
    // SAFETY：指针由 napi_create_function/define_class 写入（addon 回调，签名按 N-API 头）。
    let cb: unsafe extern "C" fn(napi_env, napi_callback_info) -> napi_value =
        std::mem::transmute(cb_ptr);

    let mut cx = JSContext::from_ptr(NonNull::new_unchecked(cx_raw));
    let env_ref = e(env as napi_env);
    let constructing = (*vp.add(1)).is_magic();
    let new_target_v = if constructing {
        // 引擎写入的 new.target（构造帧保证存在且为 constructor 对象）。
        *vp.add(2 + argc as usize)
    } else {
        UndefinedValue()
    };
    // 构造帧自建 this（N-API 语义：cbinfo this_arg 即新实例；引擎要求构造返回
    // 对象——addon 返回非对象时回落 this，见下方 rval 语义）。
    let this_v: JSVal = if constructing {
        let proto = if new_target_v.is_object() {
            get_prop_value(&mut cx, new_target_v.to_object(), c"prototype")
                .filter(|p| p.is_object())
        } else {
            None
        };
        let obj = match proto {
            Some(p) => {
                rooted!(&in(cx) let proto_root: *mut JSObject = p.to_object());
                // clasp null = plain object（NewPlainObjectWithProto，jsapi.cpp 实证）；
                // 非 null = define_class 实例（带 wrap reserved slots，class.rs）。
                JS_NewObjectWithGivenProto(cx.raw_cx(), clasp, raw_handle(&proto_root.get()))
            }
            None => {
                if clasp.is_null() {
                    JS_NewPlainObject(cx.raw_cx())
                } else {
                    // JS 语义：prototype 非对象回落 %Object.prototype%。
                    let Some(base) = object_prototype(&mut cx) else {
                        return false;
                    };
                    rooted!(&in(cx) let base_root: *mut JSObject = base.to_object());
                    JS_NewObjectWithGivenProto(cx.raw_cx(), clasp, raw_handle(&base_root.get()))
                }
            }
        };
        if obj.is_null() {
            return false;
        }
        ObjectValue(obj)
    } else {
        *vp.add(1)
    };

    // 回调槽位基线：argv/this/new.target 与回调内建值都在其上，返回时截断
    //（Node 契约：napi_value 仅回调存活期有效；同时是 finalize 链的前提——
    // 槽位是 GC 根，不截断则 external/wrap 对象永不可达死态，finalizer 永不
    // 跑（§4.77 实测：pin-all = external 内存无底洞泄漏）。addon 跨窗持有
    // 必须走 ref（§4.76 wrap-ref 出参）。
    let mark = env_ref.slots.len();
    // escape 回收基线（本回调产物按此水位截断；N-API handle scope 契约）。
    let escape_base = env_ref.escape_slots.len();
    // 实参/this/new.target 拷入 env 槽位（回调期间有效；Node 同语义）。
    let mut argv = Vec::with_capacity(argc as usize);
    for i in 0..argc {
        argv.push(env_ref.put(*vp.add(2 + i as usize)));
    }
    let this = env_ref.put(this_v);
    let nt = env_ref.put(new_target_v);
    let info = CbInfo {
        argc,
        argv,
        this,
        data: data as *mut c_void,
        new_target: nt,
    };
    let r = cb(env as napi_env, &info as *const CbInfo as napi_callback_info);
    // SAFETY：info 借用的槽位都在 env arena 内（traced），cb 返回后仅指针作废。
    let env_ref = e(env as napi_env);
    // pending 优先：addon 抛错即传播（无论返回值形态；引擎要求成功返回时无 pending）。
    if JS_IsExceptionPending(cx_raw) {
        env_ref.slots.truncate(mark);
        crate::napi::scope::escape_truncate_to(env as napi_env, escape_base);
        return false;
    }
    if constructing {
        // 构造返回语义：返回对象即 new 结果，否则回落自建 this（V8/SM 同款）。
        let rv = if !r.is_null() { env_ref.get(r) } else { UndefinedValue() };
        *vp = if rv.is_object() { rv } else { this_v };
    } else if !r.is_null() {
        *vp = env_ref.get(r);
    } else {
        *vp = UndefinedValue();
    }
    // 返回值已拷入帧槽（vp），回调产物槽位全量回收（escaped 池单独回收）。
    env_ref.slots.truncate(mark);
    crate::napi::scope::escape_truncate_to(env as napi_env, escape_base);
    true
    }
}

/// `Object.prototype` 值（define_class 实例回落 proto 用）。
pub(crate) unsafe fn object_prototype(cx: &mut JSContext) -> Option<JSVal> {
    // SAFETY：global 对象属性读取（get_prop_value 前置）。
    {
        let ctor = get_prop_value(cx, crate::state::global(), c"Object")?;
        if !ctor.is_object() {
            return None;
        }
        rooted!(&in(cx) let ctor_root = ctor.to_object());
        let proto = get_prop_value(cx, ctor_root.get(), c"prototype")?;
        proto.is_object().then_some(proto)
    }
}
