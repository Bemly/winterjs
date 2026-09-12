//! `node:vm`：同 Runtime 多 global 沙箱（9f-1，引擎深水）。
//!
//! 落法：每个 `createContext` 经 `JS_NewGlobalObject(SIMPLE_GLOBAL_CLASS)` 建独立
//! global（新 compartment，标准类走同一套懒 resolve，无需 prelude；job queue 是
//! per-context 共享，无需重装）。global 以 `Box<Heap>` 入 `state::vm_contexts`
//!（§4.40 定址），id 单调不复用；JS 侧 `FinalizationRegistry` 自动 release。
//!
//! 求值走 `evaluate_script`（自进目标 realm）+ 同步一轮 `RunJobs`（vm 内 promise
//! 当轮决议，更接近 Node 的 afterEvaluate 语义）；完成值对象跨 compartment 以
//! CCW 传递（透明访问，`instanceof` 等跨域判定以宿主为准，记档）。
//! 沙箱同步为快照式（create/run 前 sync-in，run 后 sync-out，非活绑定，见下）。
//!
//! 偏差记档：
//! - 沙箱非活绑定：`createContext` 后改 sandbox 属性需下次 run 前才 sync-in；
//!   sync-out 只回写"非标准初始键"（标准构造器名视为上下文自有，不污染 sandbox）。
//! - 错误只保 name/message（ Norton 式 `{name,message}` 包络重建同名 Error；
//!   抛非 Error 值退化为 `Error(String(v))`；identity/stack 不跨域）。
//! - `timeout`/`breakOnSigint` 只校验不执行（能正常结束的脚本行为一致；死循环
//!   与主脚本同等待遇——进程 hang，测试禁写此类用例）；`cachedData`/
//!   `produceCachedData` 接受忽略（无字节码缓存，每次 run 重解析）。
//! - `microtaskMode` 接受忽略（恒 afterEvaluate 等效：run 后同步排空一轮）。
//! - `importModuleDynamically`/模块系（SourceTextModule 等实验面）不支持；
//!   vm 内 `import()` 走主模块管线（`state::global` 指向主 global），行为未定义。
//! - `measureMemory` 恒 reject `ERR_CONTEXT_NOT_INITIALIZED`（实验警告照发）。

use mozjs::jsapi::{JSObject, RunJobs};
use mozjs::jsval::{BooleanValue, JSVal, UndefinedValue};
use mozjs::conversions::ToJSValConvertible as _;
use mozjs::realm::AutoRealm;
use mozjs::rooted;
use mozjs::rust::{CompileOptionsWrapper, RealmOptions, SIMPLE_GLOBAL_CLASS, transform_str_to_source_text};

use crate::jsapi_glue::{
    define_prop, exc_name_is, get_prop_value, report_error, value_to_string,
    wrap_cx, Frame,
};
use crate::state;

/// natives 抛错包络：`__wjs_vm_error:{name}\n{message}`（`%` 由 report 转义，
/// 换行分隔——message 内换行只影响尾部显示，JS 侧按首行取 name）。
/// JS 侧 `__vmCall` 剥包络重建同名 Error（§4.31 教训：不断言原文以外的形态）。
fn throw_vm(cx: &mut mozjs::context::JSContext, name: &str, message: &str) {
    let clean = message.replace('\0', "");
    report_error(cx, &format!("__wjs_vm_error:{name}\n{clean}"));
}

/// 字符串实参（缺省/非串 → TypeError 错，None）。
fn arg_string(
    cx: &mut mozjs::context::JSContext,
    frame: &Frame,
    i: u32,
    what: &str,
) -> Option<String> {
    if frame.argc() <= i {
        report_error(cx, &format!("TypeError: {what} needs an argument"));
        return None;
    }
    Some(value_to_string(cx, frame.arg(i)))
}

/// vm id 实参（字符串形态数字，约定见 §4.33）。
fn arg_id(cx: &mut mozjs::context::JSContext, frame: &Frame, i: u32) -> Option<u64> {
    let s = arg_string(cx, frame, i, "vm context")?;
    s.parse::<u64>().ok().or_else(|| {
        report_error(cx, "TypeError: vm context id must be a context id string");
        None
    })
}

/// 查注册表并重 root（调用方 realm 内使用；查无即 TypeError 错）。
/// 返回裸指针：调用方必须在同一作用域内 rooted 后用（无 GC 间隙）。
fn lookup_global(cx: &mut mozjs::context::JSContext, id: u64) -> Option<*mut JSObject> {
    match state::vm_global(id) {
        Some(g) if !g.is_null() => Some(g),
        _ => {
            report_error(cx, "ERR_INVALID_ARG_TYPE: contextifiedObject must be a vm.Context");
            None
        }
    }
}

/// 建上下文：新 global（新 compartment）+ 入表，返回 id 字符串。
/// `__wjs_vm_create()` → id。
pub unsafe extern "C" fn vm_create(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let options = RealmOptions::default();
    rooted!(&in(cx) let global = unsafe {
        mozjs::rust::wrappers2::JS_NewGlobalObject(
            &mut cx,
            &SIMPLE_GLOBAL_CLASS,
            std::ptr::null_mut(),
            mozjs::jsapi::OnNewGlobalHookOption::FireOnNewGlobalHook,
            &*options,
        )
    });
    if global.is_null() {
        report_error(&mut cx, "OperationError: vm could not create a context");
        return false;
    }
    let id = state::vm_add(global.get());
    id.to_string().to_jsval(&mut cx, frame.rval_mut());
    true
}

/// 语法预检（`new Script`/`compileFunction` 构造期用，不执行）。
/// `__wjs_vm_compile(code, filename)` → undefined；失败抛包络 SyntaxError。
pub unsafe extern "C" fn vm_compile(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 vm_create
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let code = match arg_string(&mut cx, &frame, 0, "vm compile") {
        Some(s) => s,
        None => return false,
    };
    let filename = arg_string(&mut cx, &frame, 1, "vm compile").unwrap_or_default();
    let c_filename = std::ffi::CString::new(filename.as_str()).unwrap_or_else(|_| c"vm.js".to_owned());
    let options = CompileOptionsWrapper::new(&cx, c_filename, 1);
    let mut src = transform_str_to_source_text(&code);
    // SAFETY: cx 在 realm 内；options/src 存活到调用返回；空指针即语法失败
    let script = unsafe { mozjs::rust::wrappers2::Compile1(&mut cx, options.ptr, &mut src) };
    if script.is_null() {
        rooted!(&in(cx) let mut exc = UndefinedValue());
        let msg = match mozjs::rust::error_info_from_exception_stack(&mut cx, exc.handle_mut()) {
            Some(info) => info.message,
            None => "invalid script".to_string(),
        };
        throw_vm(&mut cx, "SyntaxError", &msg);
        return false;
    }
    frame.set_rval(UndefinedValue());
    true
}

/// 在指定上下文求值（`evaluate_script` 自进目标 realm；成功后同步排空一轮 microtask）。
/// `__wjs_vm_run(id, code, filename)` → completion；失败抛包络错。
pub unsafe extern "C" fn vm_run(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 vm_create
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    let code = match arg_string(&mut cx, &frame, 1, "vm run") {
        Some(s) => s,
        None => return false,
    };
    let filename = arg_string(&mut cx, &frame, 2, "vm run").unwrap_or_default();
    let ptr = match lookup_global(&mut cx, id) {
        Some(p) => p,
        None => return false,
    };
    rooted!(&in(cx) let global = ptr);
    rooted!(&in(cx) let mut rval = UndefinedValue());
    let filename = std::ffi::CString::new(filename.as_str()).unwrap_or_else(|_| c"vm.js".to_owned());
    let options = CompileOptionsWrapper::new(&cx, filename, 1);
    if mozjs::rust::evaluate_script(&mut cx, global.handle(), &code, rval.handle_mut(), options).is_err() {
        // §4.1：evaluate 返回后已出 realm，重进目标 realm 再读异常
        let mut realm = AutoRealm::new_from_handle(&mut cx, global.handle());
        rooted!(&in(&mut realm) let mut exc = UndefinedValue());
        let (name, message) = match mozjs::rust::error_info_from_exception_stack(&mut realm, exc.handle_mut()) {
            Some(info) => {
                let name = ["SyntaxError", "RangeError", "ReferenceError", "TypeError", "URIError", "EvalError"]
                    .into_iter()
                    .find(|n| exc_name_is(&mut realm, exc.get(), n))
                    .unwrap_or("Error");
                (name.to_string(), info.message)
            }
            None => ("Error".to_string(), value_to_string(&mut realm, exc.get())),
        };
        throw_vm(&mut realm, &name, &message);
        return false;
    }
    // Node afterEvaluate 等效：当轮排空 vm 内 promise 反应（同一 JobQueue）
    {
        let mut realm = AutoRealm::new_from_handle(&mut cx, global.handle());
        // SAFETY: realm 内排空内部 job queue（主循环同款）
        unsafe { RunJobs((&mut realm).raw_cx()) };
    }
    frame.set_rval(rval.get());
    true
}

/// 主 global 求值（`runInThisContext`；inspector_eval 同款嵌套求值）。
/// `__wjs_vm_run_this(code, filename)` → completion；失败抛包络错。
pub unsafe extern "C" fn vm_run_this(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 vm_create
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let code = match arg_string(&mut cx, &frame, 0, "vm run") {
        Some(s) => s,
        None => return false,
    };
    let filename = arg_string(&mut cx, &frame, 1, "vm run").unwrap_or_default();
    rooted!(&in(cx) let global = state::global());
    rooted!(&in(cx) let mut rval = UndefinedValue());
    let filename = std::ffi::CString::new(filename.as_str()).unwrap_or_else(|_| c"vm.js".to_owned());
    let options = CompileOptionsWrapper::new(&cx, filename, 1);
    if mozjs::rust::evaluate_script(&mut cx, global.handle(), &code, rval.handle_mut(), options).is_err() {
        let mut realm = AutoRealm::new_from_handle(&mut cx, global.handle());
        rooted!(&in(&mut realm) let mut exc = UndefinedValue());
        let (name, message) = match mozjs::rust::error_info_from_exception_stack(&mut realm, exc.handle_mut()) {
            Some(info) => {
                let name = ["SyntaxError", "RangeError", "ReferenceError", "TypeError", "URIError", "EvalError"]
                    .into_iter()
                    .find(|n| exc_name_is(&mut realm, exc.get(), n))
                    .unwrap_or("Error");
                (name.to_string(), info.message)
            }
            None => ("Error".to_string(), value_to_string(&mut realm, exc.get())),
        };
        throw_vm(&mut realm, &name, &message);
        return false;
    }
    {
        let mut realm = AutoRealm::new_from_handle(&mut cx, global.handle());
        // SAFETY: realm 内排空（主循环同款）
        unsafe { RunJobs((&mut realm).raw_cx()) };
    }
    frame.set_rval(rval.get());
    true
}

/// 编译函数（`compileFunction`；目标 realm 由 id 定，`""` 为主 global）。
/// `__wjs_vm_compile_fn(idStr, paramsCsv, code, filename)` → function。
pub unsafe extern "C" fn vm_compile_fn(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 vm_create
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id_s = arg_string(&mut cx, &frame, 0, "vm compileFunction").unwrap_or_default();
    let params = arg_string(&mut cx, &frame, 1, "vm compileFunction").unwrap_or_default();
    let code = match arg_string(&mut cx, &frame, 2, "vm compileFunction") {
        Some(s) => s,
        None => return false,
    };
    let filename = arg_string(&mut cx, &frame, 3, "vm compileFunction").unwrap_or_default();
    // 目标 global：空串即主 global（Node 默认 parsingContext=主上下文口径）。
    let ptr = if id_s.is_empty() {
        state::global()
    } else {
        let id = match id_s.parse::<u64>() {
            Ok(id) => id,
            Err(_) => {
                report_error(&mut cx, "ERR_INVALID_ARG_TYPE: options.parsingContext must be a Context");
                return false;
            }
        };
        match lookup_global(&mut cx, id) {
            Some(p) => p,
            None => return false,
        }
    };
    if ptr.is_null() {
        report_error(&mut cx, "OperationError: vm has no global");
        return false;
    }
    rooted!(&in(cx) let global = ptr);
    // 包成匿名函数表达式求值（`new Function` 口径：params 为形参，body 为函数体）。
    let wrapped = format!("(function anonymous({params}\n) {{\n{code}\n}})");
    rooted!(&in(cx) let mut rval = UndefinedValue());
    let filename = std::ffi::CString::new(filename.as_str()).unwrap_or_else(|_| c"vm.js".to_owned());
    let options = CompileOptionsWrapper::new(&cx, filename, 1);
    if mozjs::rust::evaluate_script(&mut cx, global.handle(), &wrapped, rval.handle_mut(), options).is_err() {
        let mut realm = AutoRealm::new_from_handle(&mut cx, global.handle());
        rooted!(&in(&mut realm) let mut exc = UndefinedValue());
        let (name, message) = match mozjs::rust::error_info_from_exception_stack(&mut realm, exc.handle_mut()) {
            Some(info) => ("SyntaxError".to_string(), info.message),
            None => ("SyntaxError".to_string(), "invalid function".to_string()),
        };
        throw_vm(&mut realm, &name, &message);
        return false;
    }
    if !rval.is_object() || unsafe { !mozjs::jsapi::IsFunctionObject(rval.to_object()) } {
        throw_vm(&mut cx, "Error", "vm did not produce a function");
        return false;
    }
    frame.set_rval(rval.get());
    true
}

/// 沙箱属性写入目标 global（sync-in 用；值可跨 compartment，引擎包 CCW）。
/// `__wjs_vm_set(id, key, value)` → undefined。
pub unsafe extern "C" fn vm_set(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 vm_create
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    let key = match arg_string(&mut cx, &frame, 1, "vm sandbox key") {
        Some(s) => s,
        None => return false,
    };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: vm sandbox set needs a value");
        return false;
    }
    let val = frame.arg(2);
    let ptr = match lookup_global(&mut cx, id) {
        Some(p) => p,
        None => return false,
    };
    rooted!(&in(cx) let global = ptr);
    let mut realm = AutoRealm::new_from_handle(&mut cx, global.handle());
    let Ok(c_key) = std::ffi::CString::new(key.as_str()) else {
        report_error(&mut realm, "TypeError: vm sandbox key must not contain NUL");
        return false;
    };
    if !define_prop(&mut realm, global.get(), &c_key, val) {
        throw_vm(&mut realm, "Error", "vm could not define sandbox property");
        return false;
    }
    frame.set_rval(UndefinedValue());
    true
}

/// 读目标 global 属性（sync-out/探针用；跨 compartment 值透明为 CCW）。
/// `__wjs_vm_get(id, key)` → value。
pub unsafe extern "C" fn vm_get(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 vm_create
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    let key = match arg_string(&mut cx, &frame, 1, "vm sandbox key") {
        Some(s) => s,
        None => return false,
    };
    let ptr = match lookup_global(&mut cx, id) {
        Some(p) => p,
        None => return false,
    };
    rooted!(&in(cx) let global = ptr);
    let mut realm = AutoRealm::new_from_handle(&mut cx, global.handle());
    let Ok(c_key) = std::ffi::CString::new(key.as_str()) else {
        report_error(&mut realm, "TypeError: vm sandbox key must not contain NUL");
        return false;
    };
    match get_prop_value(&mut realm, global.get(), &c_key) {
        Some(v) => frame.set_rval(v),
        None => frame.set_rval(UndefinedValue()),
    }
    true
}

/// 目标 global 自有可枚举键（sync-out 差集用；目标 realm 内求值，JSON 串回传）。
/// `__wjs_vm_keys(id)` → `'["a","b"]'`。
pub unsafe extern "C" fn vm_keys(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 vm_create
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    let ptr = match lookup_global(&mut cx, id) {
        Some(p) => p,
        None => return false,
    };
    rooted!(&in(cx) let global = ptr);
    rooted!(&in(cx) let mut rval = UndefinedValue());
    let filename = c"vm.js".to_owned();
    let options = CompileOptionsWrapper::new(&cx, filename, 1);
    let probe = "JSON.stringify(Object.keys(globalThis))";
    if mozjs::rust::evaluate_script(&mut cx, global.handle(), probe, rval.handle_mut(), options).is_err() {
        throw_vm(&mut cx, "Error", "vm could not enumerate context keys");
        return false;
    }
    frame.set_rval(rval.get());
    true
}

/// 摘除上下文（FinalizationRegistry/显式释放用；重复释放 false）。
/// `__wjs_vm_release(id)` → boolean。
pub unsafe extern "C" fn vm_release(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 vm_create
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    let _ = &mut cx;
    frame.set_rval(BooleanValue(state::vm_release(id)));
    true
}

/// 内嵌 ESM 源（`node:vm`）。
pub const SOURCE: &str = r#"
import { EventEmitter } from "node:events";

const __kCtx = "__wjs_vm_ctx_id";
const __kStd = "__wjs_vm_std_keys";

function __vmUnwrap(e) {
  const m = String((e && e.message) || e);
  const mm = m.match(/^__wjs_vm_error:([A-Za-z]+)\n([\s\S]*)$/);
  if (!mm) {
    const err = new Error(m);
    err.code = "ERR_VM_ERROR";
    throw err;
  }
  const [, name, message] = mm;
  const Ctor = globalThis[name] || Error;
  const err = new Ctor(message);
  throw err;
}
function __vmCall(fn) {
  try {
    return fn();
  } catch (e) {
    __vmUnwrap(e);
  }
}

let __vmFinal = null;
function __vmAutoRelease(obj, id) {
  try {
    if (typeof FinalizationRegistry === "undefined") return;
    if (!__vmFinal) {
      __vmFinal = new FinalizationRegistry((held) => {
        try { __wjs_vm_release(String(held)); } catch { /* 会话收尾期忽略 */ }
      });
    }
    __vmFinal.register(obj, id);
  } catch { /* 无注册表即会话级存活，记档 */ }
}

function __validateCtx(obj) {
  if ((typeof obj !== "object" && typeof obj !== "function") || obj === null) {
    const err = new TypeError(`The "contextifiedObject" argument must be of type object. Received ${obj === null ? "null" : typeof obj}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const id = obj[__kCtx];
  if (typeof id !== "string") {
    const err = new TypeError("The \"contextifiedObject\" argument must be a vm.Context");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  return id;
}
export function isContext(obj) {
  if ((typeof obj !== "object" && typeof obj !== "function") || obj === null) {
    const err = new TypeError(`The "object" argument must be of type object. Received ${obj === null ? "null" : typeof obj}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  return typeof obj[__kCtx] === "string";
}

function __syncIn(id, obj) {
  for (const k of Object.keys(obj)) {
    if (k === __kCtx || k === __kStd) continue;
    __vmCall(() => __wjs_vm_set(id, k, obj[k]));
  }
}
function __syncOut(id, obj) {
  const std = new Set(obj[__kStd] || []);
  const keys = JSON.parse(__vmCall(() => __wjs_vm_keys(id)));
  for (const k of keys) {
    if (std.has(k)) continue;
    obj[k] = __vmCall(() => __wjs_vm_get(id, k));
  }
}

function __normStr(v, what, dflt) {
  if (v === undefined) return dflt;
  if (typeof v !== "string") {
    const err = new TypeError(`The "options.${what}" property must be of type string. Received type ${typeof v}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  return v;
}
function __normUint(v, what) {
  if (v === undefined) return undefined;
  if (typeof v !== "number" || !Number.isInteger(v) || v < 0 || v > 4294967295) {
    const err = new TypeError(`The "options.${what}" property must be an integer in range. Received ${String(v)}`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  return v;
}
function __normBool(v, what) {
  if (v === undefined) return undefined;
  if (typeof v !== "boolean") {
    const err = new TypeError(`The "options.${what}" property must be of type boolean. Received type ${typeof v}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  return v;
}

function __runArgs(contextifiedObject, options) {
  if (typeof options === "string") options = { filename: options };
  options = options ?? {};
  if ((typeof options !== "object" && typeof options !== "function") || options === null) {
    const err = new TypeError(`The "options" argument must be of type object. Received ${options === null ? "null" : typeof options}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const filename = __normStr(options.filename, "filename", "evalmachine.<anonymous>");
  __normUint(options.timeout, "timeout");
  __normBool(options.displayErrors, "displayErrors");
  __normBool(options.breakOnSigint, "breakOnSigint");
  return { filename };
}

export function createContext(contextObject = {}, options = {}) {
  if (contextObject !== null && (typeof contextObject !== "object" && typeof contextObject !== "function")) {
    const err = new TypeError(`The "contextObject" argument must be of type object. Received type ${typeof contextObject}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (options === null || ((typeof options !== "object" && typeof options !== "function"))) {
    const err = new TypeError(`The "options" argument must be of type object. Received ${options === null ? "null" : typeof options}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (isContext(contextObject)) return contextObject;
  const name = typeof options.name === "string" ? options.name : undefined;
  void name;
  if (options.microtaskMode !== undefined && options.microtaskMode !== "afterEvaluate") {
    const err = new TypeError(`The "options.microtaskMode" property must be one of 'afterEvaluate'. Received '${String(options.microtaskMode)}'`);
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
  const id = __vmCall(() => __wjs_vm_create());
  const std = JSON.parse(__vmCall(() => __wjs_vm_keys(id)));
  Object.defineProperty(contextObject, __kCtx, { value: id, enumerable: false, writable: false, configurable: true });
  Object.defineProperty(contextObject, __kStd, { value: std, enumerable: false, writable: false, configurable: true });
  if (contextObject !== null && contextObject !== undefined) __syncIn(id, contextObject);
  __vmAutoRelease(contextObject, id);
  return contextObject;
}

export function runInContext(code, contextifiedObject, options) {
  const id = __validateCtx(contextifiedObject);
  const { filename } = __runArgs(contextifiedObject, options);
  __syncIn(id, contextifiedObject);
  const r = __vmCall(() => __wjs_vm_run(id, String(code), filename));
  __syncOut(id, contextifiedObject);
  return r;
}

export function runInNewContext(code, contextObject, options) {
  if (typeof options === "string") options = { filename: options };
  const ctx = createContext(contextObject ?? {}, options ?? {});
  return runInContext(code, ctx, options);
}

export function runInThisContext(code, options) {
  const { filename } = __runArgs(null, options);
  return __vmCall(() => __wjs_vm_run_this(String(code), filename));
}

export class Script {
  constructor(code, options = {}) {
    code = String(code);
    if (typeof options === "string") options = { filename: options };
    if (options === null || (typeof options !== "object" && typeof options !== "function")) {
      const err = new TypeError(`The "options" argument must be of type object. Received ${options === null ? "null" : typeof options}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    this.__code = code;
    this.__filename = __normStr(options.filename, "filename", "evalmachine.<anonymous>");
    void __normUint(options.timeout, "timeout");
    void __normBool(options.displayErrors, "displayErrors");
    void __normBool(options.breakOnSigint, "breakOnSigint");
    __vmCall(() => __wjs_vm_compile(code, this.__filename));
  }
  runInContext(contextifiedObject, options) {
    return runInContext(this.__code, contextifiedObject, { ...(options ?? {}), filename: this.__filename });
  }
  runInNewContext(contextObject, options) {
    return runInNewContext(this.__code, contextObject, { ...(options ?? {}), filename: this.__filename });
  }
  runInThisContext(options) {
    return runInThisContext(this.__code, { ...(options ?? {}), filename: this.__filename });
  }
  createCachedData() {
    return Buffer.alloc(0);
  }
  get cachedDataRejected() { return undefined; }
  get cachedDataProduced() { return false; }
  get sourceMapURL() { return undefined; }
}

export function createScript(code, options) {
  return new Script(code, options);
}

export function compileFunction(code, params, options = {}) {
  if (typeof code !== "string") {
    const err = new TypeError(`The "code" argument must be of type string. Received type ${typeof code}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (options === null || (typeof options !== "object" && typeof options !== "function")) {
    const err = new TypeError(`The "options" argument must be of type object. Received ${options === null ? "null" : typeof options}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (params !== undefined) {
    if (!Array.isArray(params)) {
      const err = new TypeError(`The "params" argument must be of type string array. Received type ${typeof params}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    for (const p of params) {
      if (typeof p !== "string") {
        const err = new TypeError("The \"params\" array must only contain strings.");
        err.code = "ERR_INVALID_ARG_TYPE";
        throw err;
      }
    }
  }
  const filename = __normStr(options.filename, "filename", "");
  let ctxId = "";
  if (options.parsingContext !== undefined) {
    ctxId = __validateCtx(options.parsingContext);
  }
  const exts = options.contextExtensions ?? [];
  if (!Array.isArray(exts)) {
    const err = new TypeError(`The "options.contextExtensions" property must be an array.`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  for (const ext of exts) {
    if (ext === null || (typeof ext !== "object" && typeof ext !== "function")) {
      const err = new TypeError(`The "options.contextExtensions" array must only contain objects.`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
  }
  const paramsCsv = (params ?? []).join(",");
  const fn = __vmCall(() => __wjs_vm_compile_fn(ctxId, paramsCsv, code, filename));
  if (ctxId === "") {
    for (const ext of exts) Object.assign(globalThis, ext);
  } else {
    for (const ext of exts) __syncIn(ctxId, ext);
  }
  return fn;
}

export function measureMemory(options = {}) {
  if (typeof process !== "undefined" && typeof process.emitWarning === "function") {
    process.emitWarning("vm.measureMemory is experimental", "ExperimentalWarning");
  }
  void options;
  const err = new Error("vm.measureMemory requires a context with memory measurement support");
  err.code = "ERR_CONTEXT_NOT_INITIALIZED";
  return Promise.reject(err);
}

const __useMainLoader = Symbol("USE_MAIN_CONTEXT_DEFAULT_LOADER");
const __dontCtx = Symbol("DONT_CONTEXTIFY");
export const constants = Object.freeze({
  USE_MAIN_CONTEXT_DEFAULT_LOADER: __useMainLoader,
  DONT_CONTEXTIFY: __dontCtx,
});

const __api = {
  Script, createContext, createScript, runInContext, runInNewContext,
  runInThisContext, isContext, compileFunction, measureMemory, constants,
};
export default __api;
"#;
