//! `bun:ffi`：Bun FFI 兼容层（libloading 之上，plan Phase 7-e6，§13 手写件）。
//!
//! 动态调用引擎（无 libffi 的纯 Rust 落法）：C ABI（SysV x86_64/arm64、Win64）
//! 按参数独立分类——整数/指针走 INTEGER 寄存器，float/double 走 SSE 寄存器。
//! `extern "C" fn(target, a0..an) -> R` 形状的中转 shim 对目标函数完全 ABI 透明
//! （编译器完成收/发两侧寄存器搬移，含栈参）；build.rs 按
//! (元数≤6, 双精度参数位置掩码, 返回类别) 生成全部 shim，运行时按签名查表
//! （`invoke`）。调用全同步，参数/返回经 JS 数值直转（无 JSON 桥，指针即数值）。
//!
//! 支持面：标量参数（整数系/布尔/指针/f64）+ 返回（整数系/f64/f32/void）；
//! `ptr()`：字符串→零结尾拷贝地址（Box::leak，进程退出回收）、TypedArray/
//! ArrayBuffer→数据裸地址（同步调用期间对象被调用帧保活；跨调用的生命周期
//! 归用户，Bun 同款风险文档化）、数值/null 直通。`CString` 读零结尾串；
//! `toBuffer/toArrayBuffer` 拷贝视图（偏差：Bun 是零拷贝 view，我们无 Buffer
//! 全局且外挂 buffer 释放回调复杂，拷贝安全优先）。不支持：结构体按值传参/
//! 返回、变参、f32 参数、元数 >6、`JSCallback`（均文档记录）。
//!
//! 生命周期：`dlopen` 后 Library 刻意泄漏（永不 dlclose，符号地址从此稳定）；
//! 符号地址/分配地址以 f64 进 JS（用户态地址 < 2^47，f64 精确表示）。

include!(concat!(env!("OUT_DIR"), "/ffi_shims.rs"));

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;
use mozjs::typedarray::Uint8;

use crate::jsapi_glue::{report_error, uint8_array, value_to_string, wrap_cx, Frame};

/// 整数系类型名（全部 INTEGER 类，usize 通道）。
const INT_TYPES: &[&str] = &[
    "u8", "i8", "u16", "i16", "u32", "i32", "u64", "i64", "usize", "isize", "int", "uint", "long",
    "ulong", "char", "bool", "ptr",
];

/// 类型名 → 参数类别：Some(true)=双精度，Some(false)=整数系；None=非法参数类型
/// （void/f32 不能做参数）。
fn arg_class(ty: &str) -> Option<bool> {
    if ty == "f64" {
        return Some(true);
    }
    if INT_TYPES.contains(&ty) {
        return Some(false);
    }
    None
}

/// 返回类别：0=I（含 void，调用侧丢弃），1=D，2=F。
fn ret_class(ty: &str) -> Option<u8> {
    match ty {
        "void" => Some(0),
        "f64" => Some(1),
        "f32" => Some(2),
        t if INT_TYPES.contains(&t) => Some(0),
        _ => None,
    }
}

/// UNSAFE-BOUNDARY: `__wjs_ffi_dlopen(path, namesJson)` → `{name: addr}` JSON。
/// 加载失败/符号缺失即报错（Library 泄漏保地址稳定）。
/// 前置：cx 在 realm 内；path 为合法动态库路径。
/// 覆盖：`phase7_ffi_dylib`、`phase7_ffi_errors`（加载/符号/类型错）。
pub unsafe extern "C" fn ffi_dlopen(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 || !frame.arg(0).is_string() || !frame.arg(1).is_string() {
        report_error(&mut cx, "TypeError: dlopen needs a path and a names JSON string");
        return false;
    }
    let path = value_to_string(&mut cx, frame.arg(0));
    let names_json = value_to_string(&mut cx, frame.arg(1));
    if let Err(msg) = crate::permissions::check_ffi() {
        report_error(&mut cx, &msg);
        return false;
    }
    let Ok(names) = serde_json::from_str::<Vec<String>>(&names_json) else {
        report_error(&mut cx, "TypeError: dlopen: bad names JSON");
        return false;
    };
    unsafe {
        let lib = match libloading::Library::new(&path) {
            Ok(l) => l,
            Err(e) => {
                report_error(&mut cx, &format!("Error: cannot open '{path}': {e}"));
                return false;
            }
        };
        let mut map = serde_json::Map::new();
        for name in &names {
            let sym: libloading::Symbol<*mut std::ffi::c_void> = match lib.get(name.as_bytes()) {
                Ok(s) => s,
                Err(e) => {
                    report_error(&mut cx, &format!("Error: cannot find symbol '{name}' in '{path}': {e}"));
                    return false;
                }
            };
            map.insert(name.clone(), serde_json::json!(*sym as usize));
        }
        std::mem::forget(lib); // 永不卸载（符号地址从此稳定；进程退出由 OS 回收）
        rooted!(&in(cx) let mut out = UndefinedValue());
        serde_json::Value::Object(map).to_string().to_jsval(&mut cx, out.handle_mut());
        frame.set_rval(out.get());
    }
    true
}

/// UNSAFE-BOUNDARY: `__wjs_ffi_ptr_str(s)` → 零结尾 UTF-8 拷贝地址（f64 数值）。
/// 前置：cx 在 realm 内；实参为字符串。拷贝 Box::leak（进程退出回收）。
/// 覆盖：`phase7_ffi_dylib`（字符串指针进 C）。
pub unsafe extern "C" fn ffi_ptr_str(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: ptr needs a value");
        return false;
    }
    let s = value_to_string(&mut cx, frame.arg(0));
    let mut bytes = s.into_bytes();
    bytes.push(0);
    let addr = Box::leak(bytes.into_boxed_slice()).as_ptr() as usize;
    frame.set_rval(mozjs::jsval::DoubleValue(addr as f64));
    true
}

/// UNSAFE-BOUNDARY: `__wjs_ffi_ptr_view(u8view)` → 数据裸地址（f64 数值）。
/// 前置：cx 在 realm 内；实参为非共享 Uint8Array（JS 侧已归一化）。
/// 地址稳定性：ArrayBuffer 数据 malloc'd 不随 GC 移动；同步调用期间调用帧保活。
/// 覆盖：`phase7_ffi_dylib`（指针写回可见性）。
pub unsafe extern "C" fn ffi_ptr_view(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 || !frame.arg(0).is_object() {
        report_error(&mut cx, "TypeError: ptr view needs a Uint8Array");
        return false;
    }
    let obj = frame.arg(0).to_object();
    let Ok(arr) = mozjs::typedarray::TypedArray::<Uint8, *mut mozjs::jsapi::JSObject>::from(obj)
    else {
        report_error(&mut cx, "TypeError: ptr view needs a Uint8Array");
        return false;
    };
    if arr.is_shared() {
        report_error(&mut cx, "TypeError: ptr does not accept SharedArrayBuffer views yet");
        return false;
    }
    let mut len = 0usize;
    let mut shared = false;
    let mut data: *mut u8 = std::ptr::null_mut();
    // SAFETY: obj 为有效 Uint8Array 反射体（is_object 已判定 + TypedArray::from 校验）
    unsafe { mozjs::glue::GetUint8ArrayLengthAndData(obj, &mut len, &mut shared, &mut data) };
    if shared {
        report_error(&mut cx, "TypeError: ptr does not accept SharedArrayBuffer views yet");
        return false;
    }
    let addr = data as usize;
    frame.set_rval(mozjs::jsval::DoubleValue(addr as f64));
    true
}

/// UNSAFE-BOUNDARY: `__wjs_ffi_call(fnAddr, sigJson, ...args)` → 返回值。
/// 前置：cx 在 realm 内；fnAddr 为已加载库的真实 C 函数地址；sig 与实参数一致。
/// 覆盖：`phase7_ffi_dylib`（全类型矩阵）、`phase7_ffi_errors`（arity/类型错）。
pub unsafe extern "C" fn ffi_call(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 || !frame.arg(0).is_number() || !frame.arg(1).is_string() {
        report_error(&mut cx, "TypeError: FFI call needs an address and a signature");
        return false;
    }
    let target = frame.arg(0).to_number() as usize;
    let sig_json = value_to_string(&mut cx, frame.arg(1));
    let Ok(sig) = serde_json::from_str::<serde_json::Value>(&sig_json) else {
        report_error(&mut cx, "TypeError: FFI call: bad signature JSON");
        return false;
    };
    let Some(arg_types) = sig.get("a").and_then(|a| a.as_array()) else {
        report_error(&mut cx, "TypeError: FFI call: signature missing args");
        return false;
    };
    let Some(ret_ty) = sig.get("r").and_then(|r| r.as_str()) else {
        report_error(&mut cx, "TypeError: FFI call: signature missing returns");
        return false;
    };
    let arity = arg_types.len();
    if arity > 6 {
        report_error(&mut cx, "TypeError: FFI calls with more than 6 arguments are not supported yet");
        return false;
    }
    if frame.argc() < 2 + arity as u32 {
        report_error(&mut cx, "TypeError: FFI call: missing arguments");
        return false;
    }
    let Some(ret) = ret_class(ret_ty) else {
        report_error(&mut cx, &format!("TypeError: FFI call: bad return type '{ret_ty}'"));
        return false;
    };
    let mut iargs: Vec<u64> = Vec::with_capacity(arity);
    let mut dargs: Vec<f64> = Vec::with_capacity(arity);
    let mut mask: u64 = 0;
    for (i, ty_v) in arg_types.iter().enumerate() {
        let Some(ty) = ty_v.as_str() else {
            report_error(&mut cx, "TypeError: FFI call: bad arg type");
            return false;
        };
        let Some(is_d) = arg_class(ty) else {
            report_error(&mut cx, &format!("TypeError: FFI call: type '{ty}' is not a valid argument type (f32/void unsupported)"));
            return false;
        };
        let v = frame.arg(2 + i as u32);
        if is_d {
            if !v.is_number() {
                report_error(&mut cx, &format!("TypeError: FFI call: argument {i} must be a number (f64)"));
                return false;
            }
            dargs.push(v.to_number());
            mask |= 1 << i;
        } else {
            // 整数系：number（f64→i64 饱和转 u64 位型，负数保留补码）、null→0、bool→0/1
            let u = if v.is_number() {
                (v.to_number() as i64) as u64
            } else if v.is_null() {
                0
            } else if v.is_boolean() {
                if v.to_boolean() { 1 } else { 0 }
            } else {
                report_error(&mut cx, &format!("TypeError: FFI call: argument {i} must be a number or null ({ty})"));
                return false;
            };
            iargs.push(u);
        }
    }
    // SAFETY: invoke 前置条件由上文校验（真实地址 + 签名一致 + 实参数一致）
    let result = unsafe { invoke(arity, mask, ret, target, &iargs, &dargs) };
    match result {
        Err(()) => {
            report_error(&mut cx, "TypeError: FFI call: unsupported signature combination");
            false
        }
        Ok(FfiRet::I(u)) => {
            if ret_ty == "void" {
                frame.set_rval(UndefinedValue());
            } else {
                // 符号保留（i64 口径）；>2^53 有精度损失（文档记录）
                frame.set_rval(mozjs::jsval::DoubleValue((u as i64) as f64));
            }
            true
        }
        Ok(FfiRet::D(d)) => {
            frame.set_rval(mozjs::jsval::DoubleValue(d));
            true
        }
        Ok(FfiRet::F(f)) => {
            frame.set_rval(mozjs::jsval::DoubleValue(f as f64));
            true
        }
    }
}

/// UNSAFE-BOUNDARY: `__wjs_ffi_cstring(addr)` → 读零结尾 UTF-8 串。
/// 前置：cx 在 realm 内；addr 必须指向有效、以 NUL 结尾的内存（用户契约，
/// Bun 同款；悬垂即 UB —— 测试只读自家库返回的静态串/拷贝）。
/// 覆盖：`phase7_ffi_dylib`（C 返回 char*）。
pub unsafe extern "C" fn ffi_cstring(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 || !frame.arg(0).is_number() {
        report_error(&mut cx, "TypeError: CString needs an address");
        return false;
    }
    let addr = frame.arg(0).to_number() as usize;
    if addr == 0 {
        report_error(&mut cx, "RangeError: CString: null pointer");
        return false;
    }
    // SAFETY: addr 由调用方契约保证有效且零结尾（本测试只读静态串）
    let c = unsafe { std::ffi::CStr::from_ptr(addr as *const std::ffi::c_char) };
    let s = c.to_string_lossy().into_owned();
    rooted!(&in(cx) let mut out = UndefinedValue());
    s.to_jsval(&mut cx, out.handle_mut());
    frame.set_rval(out.get());
    true
}

/// UNSAFE-BOUNDARY: `__wjs_ffi_bytes(addr, len)` → [addr, addr+len) 的拷贝
/// （Uint8Array）。前置：cx 在 realm 内；[addr, addr+len) 须为有效可读内存。
/// 偏差：拷贝而非零拷贝 view（模块头注）。覆盖：`phase7_ffi_dylib`。
pub unsafe extern "C" fn ffi_bytes(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 || !frame.arg(0).is_number() || !frame.arg(1).is_number() {
        report_error(&mut cx, "TypeError: toBuffer/toArrayBuffer needs an address and a byteLength");
        return false;
    }
    let addr = frame.arg(0).to_number() as usize;
    let len = frame.arg(1).to_number() as usize;
    if addr == 0 {
        report_error(&mut cx, "RangeError: null pointer");
        return false;
    }
    // SAFETY: [addr, addr+len) 由调用方契约保证有效可读
    let bytes = unsafe { std::slice::from_raw_parts(addr as *const u8, len) }.to_vec();
    let Some(obj) = uint8_array(&mut cx, &bytes) else {
        report_error(&mut cx, "RangeError: cannot allocate FFI view");
        return false;
    };
    rooted!(&in(cx) let out = mozjs::jsval::ObjectValue(obj));
    frame.set_rval(out.get());
    true
}

/// 内嵌 ESM 源（`suffix` 走 process.platform；符号声明归一化在 JS 侧完成）。
pub const SOURCE: &str = r#"
// ---- Phase 7-e6: bun:ffi ----
const FFIType = Object.freeze({
  char: "char", u8: "u8", i8: "i8", u16: "u16", i16: "i16", u32: "u32", i32: "i32",
  u64: "u64", i64: "i64", usize: "usize", isize: "isize", int: "int", uint: "uint",
  long: "long", ulong: "ulong", bool: "bool", ptr: "ptr", f32: "f32", f64: "f64", void: "void",
});
const suffix = process.platform === "win32" ? ".dll" : process.platform === "darwin" ? ".dylib" : ".so";
function __wjs_normType(t) {
  if (typeof t !== "string" || !(t in FFIType)) {
    throw new TypeError(`dlopen: unknown FFIType '${String(t)}'`);
  }
  return t;
}
function __wjs_normSig(decl) {
  if (typeof decl === "string") return { a: [], r: __wjs_normType(decl) };
  if (decl && typeof decl === "object") {
    if (decl.args !== undefined && !Array.isArray(decl.args)) throw new TypeError("dlopen: symbol args must be an array of FFIType");
    const a = (decl.args ?? []).map(__wjs_normType);
    for (const t of a) {
      if (t === "void") throw new TypeError("dlopen: void is not a valid argument type");
      if (t === "f32") throw new TypeError("dlopen: f32 arguments are not supported yet (use f64)");
    }
    return { a, r: __wjs_normType(decl.returns ?? "void") };
  }
  throw new TypeError("dlopen: symbol declaration must be an FFIType or {args, returns}");
}
function dlopen(path, symbolsTable, opts) {
  if (typeof path !== "string" || path === "") throw new TypeError("dlopen: path must be a non-empty string");
  if (symbolsTable === null || typeof symbolsTable !== "object") {
    throw new TypeError("dlopen: symbols table must be an object");
  }
  const names = Object.keys(symbolsTable);
  // 先校验全部声明再加载（否则 native dlsym 的报错会盖过声明错误）
  const sigs = names.map((name) => __wjs_normSig(symbolsTable[name]));
  const addrs = JSON.parse(__wjs_ffi_dlopen(path, JSON.stringify(names)));
  const symbols = {};
  for (const [i, name] of names.entries()) {
    const sig = sigs[i];
    const sigJson = JSON.stringify(sig);
    const addr = addrs[name];
    symbols[name] = function (...args) {
      if (args.length !== sig.a.length) {
        throw new TypeError(`FFI call '${name}': expected ${sig.a.length} argument(s), got ${args.length}`);
      }
      return __wjs_ffi_call(addr, sigJson, ...args);
    };
  }
  return { symbols };
}
function ptr(v) {
  if (typeof v === "string") return __wjs_ffi_ptr_str(v);
  if (v instanceof ArrayBuffer) return __wjs_ffi_ptr_view(new Uint8Array(v));
  if (ArrayBuffer.isView(v)) {
    return __wjs_ffi_ptr_view(v instanceof Uint8Array ? v : new Uint8Array(v.buffer, v.byteOffset, v.byteLength));
  }
  if (typeof v === "number") {
    if (!Number.isFinite(v) || v < 0) throw new TypeError("ptr: bad address");
    return v;
  }
  if (v === null) return 0;
  throw new TypeError(`ptr: unsupported value type '${typeof v}'`);
}
class CString {
  #s;
  constructor(address) {
    if (typeof address !== "number") throw new TypeError("CString: address must be a number");
    this.ptr = address;
    this.#s = __wjs_ffi_cstring(address);
    this.length = this.#s.length;
  }
  toString() { return this.#s; }
  valueOf() { return this.#s; }
}
function toArrayBuffer(address, byteLength) {
  if (typeof byteLength !== "number" || byteLength < 0) throw new TypeError("toArrayBuffer: byteLength required");
  return __wjs_ffi_bytes(address, byteLength).buffer;
}
function toBuffer(address, byteLength) {
  if (typeof byteLength !== "number" || byteLength < 0) throw new TypeError("toBuffer: byteLength required");
  return __wjs_ffi_bytes(address, byteLength);
}
export { dlopen, FFIType, suffix, ptr, CString, toArrayBuffer, toBuffer };
"#;

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn t_add(a: i32, b: i32) -> i32 {
        a + b
    }
    extern "C" fn t_mix(a: i32, b: f64) -> f64 {
        a as f64 + b
    }
    extern "C" fn t_f32(x: f64) -> f32 {
        (x as f32) * 2.0
    }
    extern "C" fn t_zero() -> i64 {
        77
    }
    extern "C" fn t_id(a: i64) -> i64 {
        a
    }

    #[test]
    fn invoke_shims_roundtrip() {
        unsafe {
            // 全整数参 + 整数返
            match invoke(2, 0b00, 0, t_add as *const () as usize, &[3, 4], &[]) {
                Ok(FfiRet::I(v)) => assert_eq!(v, 7),
                other => panic!("bad: {other:?}"),
            }
            // 混合类别：a0 整数 a1 双精度（mask 0b10）+ 浮点返
            match invoke(2, 0b10, 1, t_mix as *const () as usize, &[2], &[0.5]) {
                Ok(FfiRet::D(v)) => assert!((v - 2.5).abs() < 1e-12),
                other => panic!("bad: {other:?}"),
            }
            // f32 返回（参数是 f64 → mask 0b01）
            match invoke(1, 0b01, 2, t_f32 as *const () as usize, &[], &[21.0]) {
                Ok(FfiRet::F(v)) => assert_eq!(v, 42.0f32),
                other => panic!("bad: {other:?}"),
            }
            // 零参
            match invoke(0, 0, 0, t_zero as *const () as usize, &[], &[]) {
                Ok(FfiRet::I(v)) => assert_eq!(v, 77),
                other => panic!("bad: {other:?}"),
            }
            // 负数按补码走（i64 通道），返回侧保号
            let neg = -12345i64 as u64;
            match invoke(1, 0b00, 0, t_id as *const () as usize, &[neg], &[]) {
                Ok(FfiRet::I(v)) => assert_eq!((v as i64) as f64, -12345.0),
                other => panic!("bad: {other:?}"),
            }
            // 未知组合（元数 >6）报错
            assert!(invoke(7, 0, 0, t_zero as *const () as usize, &[], &[]).is_err());
        }
    }

    #[test]
    fn classify_tables() {
        for t in ["u8", "i32", "u64", "isize", "int", "long", "bool", "char", "ptr"] {
            assert_eq!(arg_class(t), Some(false), "{t}");
            assert_eq!(ret_class(t), Some(0), "{t}");
        }
        assert_eq!(arg_class("f64"), Some(true));
        assert_eq!(ret_class("f64"), Some(1));
        assert_eq!(ret_class("f32"), Some(2));
        assert_eq!(ret_class("void"), Some(0));
        // 非法：f32/void 不能做参数；未知类型两边都非法
        assert_eq!(arg_class("f32"), None);
        assert_eq!(arg_class("void"), None);
        assert_eq!(arg_class("nope"), None);
        assert_eq!(ret_class("nope"), None);
    }
}
