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
//! - `importModuleDynamically`/模块系：9i-1 落地 `Module` 基类 +
//!   `SourceTextModule`（零导入全链：compile/link/evaluate/namespace/status；
//!   带导入者 link 报 `ERR_VM_MODULE_LINK_FAILURE`，linker 切片后续）+
//!   `SyntheticModule`（纯 JS：evaluateCallback 回填导出）；
//!   vm 内 `import()` 仍走主模块管线，行为未定义（记档）。
//! - `measureMemory` 恒 reject `ERR_CONTEXT_NOT_INITIALIZED`（实验警告照发）。

use mozjs::jsapi::{JSObject, RunJobs};
use mozjs::jsval::{BooleanValue, JSVal, UndefinedValue};
use mozjs::conversions::ToJSValConvertible as _;
use mozjs::realm::AutoRealm;
use mozjs::rooted;
use mozjs::rust::{CompileOptionsWrapper, RealmOptions, SIMPLE_GLOBAL_CLASS, transform_str_to_source_text};

use crate::jsapi_glue::{
    define_prop, exc_name_is, get_prop_value, own_keys_json, report_error, same_value,
    value_to_string, wrap_cx, Frame,
};
use crate::state;

/// natives 抛错包络：`__wjs_vm_error:{name}\n{message}`（`%` 由 report 转义，
/// 换行分隔——message 内换行只影响尾部显示，JS 侧按首行取 name）。
/// JS 侧 `__vmCall` 剥包络重建同名 Error（§4.31 教训：不断言原文以外的形态）。
fn throw_vm(cx: &mut mozjs::context::JSContext, name: &str, message: &str) {
    let clean = message.replace('\0', "");
    report_error(cx, &format!("__wjs_vm_error:{name}\n{clean}"));
}

/// vm 求值路径的赋值类 TypeError 文案桥：SM 引擎文案 → node contextify 拦截器
/// 口径（真机该文案出自 node 的 global 属性拦截器，非引擎——套件按 node 文案
/// regex 断言；仅 vm_run 用，主域/主 global 求值不改写引擎文案）。
fn bridge_vm_assign_message(message: &str) -> String {
    // SM "assignment to undeclared variable z" → node "z is not defined"
    //（strict 隐式全局赋值的 ReferenceError，真机文案）。
    if let Some(key) = message.strip_prefix("assignment to undeclared variable ") {
        return format!("{key} is not defined");
    }
    let Some(rest) = message.strip_prefix('"') else {
        return message.to_string();
    };
    let Some((key, tail)) = rest.split_once("\" is ") else {
        return message.to_string();
    };
    match tail {
        "read-only" => {
            format!("Cannot assign to read only property '{key}' of object '[object Object]'")
        }
        "non-configurable and can't be redefined" => format!("Cannot redefine property: {key}"),
        _ => message.to_string(),
    }
}

/// vm 运行期错误的信封消息：文案桥 + 位置标记（`__wjs_vm_stk:{json}`）。
/// node displayErrors 口径——vm 错误的 err.stack 以 `filename:line` 前缀开头
/// （checkErr 类 `startsWith(filename)` 校验点名）；JS 侧 __vmUnwrap 剥标记
/// 重建栈。栈内帧格式仍是引擎口径（SM `@` vs V8 `at`，记档偏离）。
fn vm_stk_envelope(filename: &str, code: &str, info: &mozjs::rust::ErrorInfo) -> String {
    let line = info.line.max(1);
    let col = info.col.max(1);
    let srcline = code
        .split('\n')
        .nth((line as usize).saturating_sub(1))
        .unwrap_or("");
    let stk = serde_json::json!({ "f": filename, "l": line, "c": col, "s": srcline }).to_string();
    format!("{}\n__wjs_vm_stk:{stk}", bridge_vm_assign_message(&info.message))
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
    let mut options = RealmOptions::default();
    // Atomics/SharedArrayBuffer 与主域同开关（jsdom 等生态取此面）。
    options.creationOptions_.sharedMemoryAndAtomics_ = true;
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

/// `__wjs_vm_global(id)` → 该 context 的 global 对象本体（DONT_CONTEXTIFY 用：
/// jsdom 29 拿它当 window 直装 DOM 全局，写入即落 vm global）。
/// UNSAFE-BOUNDARY: id 为 vm 表有效 id；出参经 rooted（覆盖 tests/node/vm.rs）。
pub unsafe extern "C" fn vm_global(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_string(&mut cx, &frame, 0, "vm global") {
        Some(s) => match s.parse::<u64>() {
            Ok(v) => v,
            Err(_) => {
                report_error(&mut cx, "ERR_INVALID_ARG_TYPE: vm global id must be a module id string");
                return false;
            }
        },
        None => return false,
    };
    let Some(g) = (match state::vm_global(id) {
        Some(g) if !g.is_null() => Some(g),
        _ => None,
    }) else {
        report_error(&mut cx, "ERR_INVALID_ARG_TYPE: contextifiedObject must be a vm.Context");
        return false;
    };
    rooted!(&in(cx) let g_root: *mut JSObject = g);
    frame.set_rval(mozjs::jsval::ObjectValue(g_root.get()));
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
    let filename_c =
        std::ffi::CString::new(filename.as_str()).unwrap_or_else(|_| c"vm.js".to_owned());
    let options = CompileOptionsWrapper::new(&cx, filename_c, 1);
    if mozjs::rust::evaluate_script(&mut cx, global.handle(), &code, rval.handle_mut(), options).is_err() {
        // §4.1：evaluate 返回后已出 realm，重进目标 realm 再读异常
        let mut realm = AutoRealm::new_from_handle(&mut cx, global.handle());
        // 先取原始异常对象暂存（take 即清 pending），再恢复 pending 供 error_info
        // 读 name/message/位置（读完再清）——__vmCall 优先取原物（保 vm realm
        // 身份/原型/栈，跨域 instanceof 点名），信封重建只作兜底。
        if let Some(v) = crate::jsapi_glue::take_pending_exception(&mut realm) {
            rooted!(&in(&mut realm) let orig_root: JSVal = v);
            state::with_rooted(|s| s.vm_last_error.set(orig_root.get()));
            crate::jsapi_glue::set_pending_exception(&mut realm, orig_root.get());
        }
        rooted!(&in(&mut realm) let mut exc = UndefinedValue());
        let (name, message) = match mozjs::rust::error_info_from_exception_stack(&mut realm, exc.handle_mut()) {
            Some(info) => {
                let name = ["SyntaxError", "RangeError", "ReferenceError", "TypeError", "URIError", "EvalError"]
                    .into_iter()
                    .find(|n| exc_name_is(&mut realm, exc.get(), n))
                    .unwrap_or("Error");
                (name.to_string(), vm_stk_envelope(&filename, &code, &info))
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
    let filename_c =
        std::ffi::CString::new(filename.as_str()).unwrap_or_else(|_| c"vm.js".to_owned());
    let options = CompileOptionsWrapper::new(&cx, filename_c, 1);
    if mozjs::rust::evaluate_script(&mut cx, global.handle(), &code, rval.handle_mut(), options).is_err() {
        let mut realm = AutoRealm::new_from_handle(&mut cx, global.handle());
        // 同 vm_run：取原物暂存 + 恢复 pending。
        if let Some(v) = crate::jsapi_glue::take_pending_exception(&mut realm) {
            rooted!(&in(&mut realm) let orig_root: JSVal = v);
            state::with_rooted(|s| s.vm_last_error.set(orig_root.get()));
            crate::jsapi_glue::set_pending_exception(&mut realm, orig_root.get());
        }
        rooted!(&in(&mut realm) let mut exc = UndefinedValue());
        let (name, message) = match mozjs::rust::error_info_from_exception_stack(&mut realm, exc.handle_mut()) {
            Some(info) => {
                let name = ["SyntaxError", "RangeError", "ReferenceError", "TypeError", "URIError", "EvalError"]
                    .into_iter()
                    .find(|n| exc_name_is(&mut realm, exc.get(), n))
                    .unwrap_or("Error");
                (name.to_string(), vm_stk_envelope(&filename, &code, &info))
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
    rooted!(&in(cx) let mut rval = UndefinedValue());
    let filename_c =
        std::ffi::CString::new(filename.as_str()).unwrap_or_else(|_| c"vm.js".to_owned());
    let options = CompileOptionsWrapper::new(&cx, filename_c, 1);
    // 体级语法预检（声明位包络）：compileFunction 的体必须能独立作为函数体解析。
    // 表达式包络 `(function(){ … })` 会被体首 `});` 提前闭合吸收（测试
    // `});\n(function(){…})();\n(function() {` 套件点名）——真机 V8 以声明位
    // 包络编译，未闭合括号悬到 EOF 即错；声明位同构，预检失败即 SyntaxError。
    // 预检产物弃置，实编译仍走表达式包络（toString === `function (p) {\n…\n}`）。
    let wrapped_chk = format!("function __wjs_vm_body_chk({params}) {{\n{code}\n}}");
    {
        let mut src_chk = transform_str_to_source_text(&wrapped_chk);
        // SAFETY: cx 在 realm 内；options/src 存活到调用返回；空指针即语法失败
        let chk = unsafe { mozjs::rust::wrappers2::Compile1(&mut cx, options.ptr, &mut src_chk) };
        if chk.is_null() {
            rooted!(&in(cx) let mut exc = UndefinedValue());
            let msg = match mozjs::rust::error_info_from_exception_stack(&mut cx, exc.handle_mut()) {
                Some(info) => info.message,
                None => "invalid function".to_string(),
            };
            throw_vm(&mut cx, "SyntaxError", &msg);
            return false;
        }
    }
    // 包成匿名函数表达式求值（toString 对真机：`function (p) {\n…\n}`——
    // 无 "anonymous" 名、params 后无换行，真机 compileFunction fn.name === ""）。
    let wrapped = format!("(function ({params}) {{\n{code}\n}})");
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
/// 语义：`define` 优先（可枚举数据描述符）；已有属性重定义失败回落赋值；
/// 双失败（目标只读无 setter）即跳过——真机以目标描述符为准（`inherited_properties`
/// 只读继承、`preserves-property` 等），源端不同步，不抛（10c-3 回落的静默形）。
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
        // 已有属性的重定义在跨 compartment 值（sync-out 又 sync-in 的函数等）
        // 下失败：回落赋值语义（更新值、保留既有描述符；双失败（如只读无 setter）
        // 即跳过——真机口径以目标描述符为准，源端只读不同步，见 vm_set 头注）。
        let _ = crate::jsapi_glue::set_prop_value(&mut realm, global.get(), &c_key, val);
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

/// 目标 global 全部自有字符串键（sync-out/创建快照用；含不可枚举，JSON 数组回传）。
/// `__wjs_vm_keys_all(id)` → `'["a","b"]'`。
/// UNSAFE-BOUNDARY: 前置同 `vm_keys`；`GetPropertyKeys` 失败 None（覆盖测试同 `vm_keys`）。
pub unsafe extern "C" fn vm_keys_all(
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
    let json = {
        let mut realm = AutoRealm::new_from_handle(&mut cx, global.handle());
        own_keys_json(&mut realm, global.get()).unwrap_or_default()
    };
    if json.is_empty() {
        report_error(&mut cx, "OperationError: vm could not enumerate context keys");
        return false;
    }
    json.to_jsval(&mut cx, frame.rval_mut());
    true
}

/// 目标 global 自有键计数快照（仅调试/探针用；`{"strings": [...], "symbols": n}` JSON 回传，
/// symbol 只计数——跨 realm 无字符串身份，存在性由计数断言）。
/// `__wjs_vm_keys_count(id)` → `'{"strings":[...],"symbols":0}'`。
/// UNSAFE-BOUNDARY: 前置同 `vm_keys`；枚举经 `own_keys_json` 同源（覆盖测试 `phase10f_vm_sync_all_keys`）。
pub unsafe extern "C" fn vm_keys_count(
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
    let json = {
        let mut realm = AutoRealm::new_from_handle(&mut cx, global.handle());
        match own_keys_json(&mut realm, global.get()) {
            Some(json) => {
                let v: serde_json::Value =
                    serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
                let (strings, symbols) = match v {
                    serde_json::Value::Array(items) => {
                        let mut strings = Vec::new();
                        let mut symbols = 0usize;
                        for it in items {
                            match it {
                                serde_json::Value::String(s) => strings.push(s),
                                _ => symbols += 1,
                            }
                        }
                        (strings, symbols)
                    }
                    _ => (Vec::new(), 0usize),
                };
                serde_json::json!({ "strings": strings, "symbols": symbols }).to_string()
            }
            None => String::new(),
        }
    };
    if json.is_empty() {
        report_error(&mut cx, "OperationError: vm could not enumerate context keys");
        return false;
    }
    json.to_jsval(&mut cx, frame.rval_mut());
    true
}

/// 跨 compartment SameValue 比较（sync-out 快照比较用；`__wjs_vm_same(a, b)` → boolean）。
/// UNSAFE-BOUNDARY: 前置——cx 在 realm 内；a/b 由 Frame rooted 后传入（§4.80）。
/// 覆盖：`tests/node/vm.rs::phase10f_vm_sync_snapshot`。
pub unsafe extern "C" fn vm_same(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 vm_create；a/b 由 Frame rooted 后传入（§4.80）
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: vm SameValue needs two arguments");
        return false;
    }
    match same_value(&mut cx, frame.arg(0), frame.arg(1)) {
        Some(same) => {
            same.to_jsval(&mut cx, frame.rval_mut());
            true
        }
        None => {
            report_error(&mut cx, "OperationError: vm SameValue comparison failed");
            false
        }
    }
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

/// 取 vm_run/vm_run_this 暂存的原始异常对象并清槽：
/// `__wjs_vm_take_error()` → value（无则 undefined）。
/// UNSAFE-BOUNDARY: 槽值经 RootedState.vm_last_error（Heap，trace 覆盖）保活；
/// JS 单线程专用；读后即清（信封/原物一一对应）。
/// 覆盖：`tests/node/vm.rs` vm 对拍黑盒（跨域 SyntaxError instanceof）。
pub unsafe extern "C" fn vm_take_error(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 vm_create
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let _ = &mut cx;
    let mut out = UndefinedValue();
    state::with_rooted(|s| {
        out = s.vm_last_error.get();
        s.vm_last_error.set(UndefinedValue());
    });
    frame.set_rval(out);
    true
}

// ── 9i-1 模块系（SourceTextModule；SyntheticModule 纯 JS，见 SOURCE）─────────
//
// 落法：`compile` 在目标 compartment 内 `load_js` 转译（含 TS）+ `CompileModule1`
// （filename=identifier，使 `referrer_base` 天然分流）；`link` 走
// `load_dependencies` + `ModuleLink`（零导入恒过；`has_imports` 者 v1 拒绝——
// 进程级 load hook 按主 global/主注册表工作，vm 记录带入即 compartment 错配，
// linker 切片后续做）；`evaluate` 走 `ModuleEvaluate` + 一轮 `RunJobs`
// （afterEvaluate 等效）；namespace 经 `GetModuleNamespace` 以 CCW 回主域。
// 记录以 `Box<Heap>` 入 `state::vm_mods`（§4.40 定址），状态位防重复 link/evaluate。

/// 编译模块：`__wjs_vm_compile_mod(ctxId, identifier, code)` → modId 字符串。
/// 失败抛包络 SyntaxError（转译错/编译错，含行列信息）。
pub unsafe extern "C" fn vm_mod_compile(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    use mozjs::rust::wrappers2::CompileModule1;
    // SAFETY: 同 vm_create
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let ctx = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    let identifier = match arg_string(&mut cx, &frame, 1, "vm SourceTextModule") {
        Some(s) => s,
        None => return false,
    };
    let code = match arg_string(&mut cx, &frame, 2, "vm SourceTextModule") {
        Some(s) => s,
        None => return false,
    };
    let ptr = match lookup_global(&mut cx, ctx) {
        Some(p) => p,
        None => return false,
    };
    rooted!(&in(cx) let global = ptr);
    // 转译（TS 免费；ext 从 identifier 点后缀取，无点按 js）。
    let ext = identifier.rsplit('.').next().filter(|e| {
        identifier.contains('.')
            && e.len() <= 5
            && !e.is_empty()
            && e.chars().all(|c| c.is_ascii_alphanumeric())
    });
    let fake_path = std::path::PathBuf::from(format!("vm_mod.{}", ext.unwrap_or("js")));
    let loaded = match crate::loader::load_js(&code, &identifier, &fake_path) {
        Ok(l) => l,
        Err(e) => {
            throw_vm(&mut cx, "SyntaxError", &e.to_string());
            return false;
        }
    };
    let has_imports = !loaded.imports.is_empty();
    let c_filename =
        std::ffi::CString::new(identifier.as_str()).unwrap_or_else(|_| c"vm_module.js".to_owned());
    let mut realm = AutoRealm::new_from_handle(&mut cx, global.handle());
    let options = CompileOptionsWrapper::new(&realm, c_filename, 1);
    let mut src = transform_str_to_source_text(&loaded.js);
    // SAFETY: realm 内；options/src 存活到调用返回；null 即编译失败
    let record = unsafe { CompileModule1(&mut realm, options.ptr, &mut src) };
    if record.is_null() {
        rooted!(&in(&mut realm) let mut exc = UndefinedValue());
        let msg = match mozjs::rust::error_info_from_exception_stack(&mut realm, exc.handle_mut()) {
            Some(info) => format!("{}:{}:{}: {}", identifier, info.line.max(1), info.col.max(1), info.message),
            None => format!("{identifier}: invalid module"),
        };
        throw_vm(&mut realm, "SyntaxError", &msg);
        return false;
    }
    let id = state::vm_mod_add(ctx, identifier, record, has_imports, loaded.imports);
    id.to_string().to_jsval(&mut realm, frame.rval_mut());
    true
}

/// 取静态依赖表：`__wjs_vm_mod_deps(modId)` → JSON 数组串。
pub unsafe extern "C" fn vm_mod_deps(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 vm_create
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let s = match arg_string(&mut cx, &frame, 0, "vm Module") {
        Some(s) => s,
        None => return false,
    };
    let Ok(id) = s.parse::<u64>() else {
        report_error(&mut cx, "ERR_INVALID_ARG_TYPE: vm Module id must be a module id string");
        return false;
    };
    let Some(json) = state::vm_mod_deps_json(id) else {
        report_error(&mut cx, "ERR_VM_MODULE_NOT_FOUND: vm Module has been released");
        return false;
    };
    json.to_jsval(&mut cx, frame.rval_mut());
    true
}

/// 链接模块：`__wjs_vm_link(modId)` → undefined。
/// 零导入恒过；带导入 v1 报 `ERR_VM_MODULE_LINK_FAILURE`（linker 切片后续）。
pub unsafe extern "C" fn vm_mod_link(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    use mozjs::rust::wrappers2::ModuleLink;
    // SAFETY: 同 vm_create
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let s = match arg_string(&mut cx, &frame, 0, "vm Module") {
        Some(s) => s,
        None => return false,
    };
    let id = match s.parse::<u64>() {
        Ok(id) => id,
        Err(_) => {
            report_error(&mut cx, "ERR_INVALID_ARG_TYPE: vm Module id must be a module id string");
            return false;
        }
    };
    let Some((record, ctx, has_imports, _linked, _evaluated)) = state::vm_mod_get(id) else {
        report_error(&mut cx, "ERR_VM_MODULE_NOT_FOUND: vm Module has been released");
        return false;
    };
    if has_imports {
        let ident = state::vm_mod_identifier(id).unwrap_or_default();
        report_error(
            &mut cx,
            &format!(
                "ERR_VM_MODULE_LINK_FAILURE: module '{ident}' has imports but no linker was provided (v1: zero-import only)"
            ),
        );
        return false;
    }
    let Some(ptr) = state::vm_global(ctx) else {
        report_error(&mut cx, "ERR_VM_MODULE_NOT_FOUND: vm context has been released");
        return false;
    };
    rooted!(&in(cx) let global = ptr);
    rooted!(&in(cx) let record_root: *mut JSObject = record);
    let mut realm = AutoRealm::new_from_handle(&mut cx, global.handle());
    if let Err(e) = crate::modules::load_dependencies(&mut realm, record_root.get()) {
        throw_vm(&mut realm, "Error", &e.to_string());
        return false;
    }
    // SAFETY: record 有效 rooted；加载态已就绪，realm 内同步 link
    if !unsafe { ModuleLink(&mut realm, record_root.handle()) } {
        let msg = crate::modules::module_error(&mut realm, "vm_module").to_string();
        throw_vm(&mut realm, "Error", &msg);
        return false;
    }
    state::vm_mod_set_linked(id);
    frame.set_rval(UndefinedValue());
    true
}

/// 求值模块：`__wjs_vm_evaluate(modId)` → completion（promise 照常回调用方）。
/// 未 link 即报 `ERR_VM_MODULE_STATUS`（Node 口径：先 link 后 evaluate）。
pub unsafe extern "C" fn vm_mod_evaluate(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    use mozjs::rust::wrappers2::ModuleEvaluate;
    // SAFETY: 同 vm_create
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let s = match arg_string(&mut cx, &frame, 0, "vm Module") {
        Some(s) => s,
        None => return false,
    };
    let id = match s.parse::<u64>() {
        Ok(id) => id,
        Err(_) => {
            report_error(&mut cx, "ERR_INVALID_ARG_TYPE: vm Module id must be a module id string");
            return false;
        }
    };
    let Some((record, ctx, _has_imports, linked, _evaluated)) = state::vm_mod_get(id) else {
        report_error(&mut cx, "ERR_VM_MODULE_NOT_FOUND: vm Module has been released");
        return false;
    };
    if !linked {
        report_error(&mut cx, "ERR_VM_MODULE_STATUS: module must be linked before evaluate");
        return false;
    }
    let Some(ptr) = state::vm_global(ctx) else {
        report_error(&mut cx, "ERR_VM_MODULE_NOT_FOUND: vm context has been released");
        return false;
    };
    rooted!(&in(cx) let global = ptr);
    rooted!(&in(cx) let record_root: *mut JSObject = record);
    rooted!(&in(cx) let mut rval = UndefinedValue());
    let mut realm = AutoRealm::new_from_handle(&mut cx, global.handle());
    // SAFETY: record 有效 rooted；realm 内同步求值
    if !unsafe { ModuleEvaluate(&mut realm, record_root.handle(), rval.handle_mut()) } {
        let msg = crate::modules::module_error(&mut realm, "vm_module").to_string();
        throw_vm(&mut realm, "Error", &msg);
        return false;
    }
    // Node afterEvaluate 等效：当轮排空 vm 内 promise 反应（同一 JobQueue）
    {
        // SAFETY: realm 内排空内部 job queue（主循环同款）
        unsafe { RunJobs(realm.raw_cx()) };
    }
    // 跨域求值恒异步（见 §4.57）：rval 为 promise 时结算未定，不置 evaluated 位，
    // 由 JS 壳在 promise 落定后经 `__wjs_vm_mod_settled` 补记；同步完成值才即置。
    let is_promise = if rval.is_object() {
        rooted!(&in(&mut realm) let rval_obj: *mut JSObject = rval.to_object());
        // SAFETY: rval_obj 为有效 rooted 对象
        unsafe { mozjs::jsapi::IsPromiseObject(crate::jsapi_glue::raw_handle(rval_obj.as_ptr())) }
    } else {
        false
    };
    if !is_promise {
        state::vm_mod_set_evaluated(id);
    }
    frame.set_rval(rval.get());
    true
}

/// 取模块 namespace：`__wjs_vm_mod_ns(modId)` → namespace 对象（CCW 回主域）。
pub unsafe extern "C" fn vm_mod_ns(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    use mozjs::rust::wrappers2::GetModuleNamespace;
    // SAFETY: 同 vm_create
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let s = match arg_string(&mut cx, &frame, 0, "vm Module") {
        Some(s) => s,
        None => return false,
    };
    let id = match s.parse::<u64>() {
        Ok(id) => id,
        Err(_) => {
            report_error(&mut cx, "ERR_INVALID_ARG_TYPE: vm Module id must be a module id string");
            return false;
        }
    };
    let Some((record, ctx, _has_imports, _linked, evaluated)) = state::vm_mod_get(id) else {
        report_error(&mut cx, "ERR_VM_MODULE_NOT_FOUND: vm Module has been released");
        return false;
    };
    if !evaluated {
        report_error(&mut cx, "ERR_VM_MODULE_STATUS: module must be evaluated before reading namespace");
        return false;
    }
    let Some(ptr) = state::vm_global(ctx) else {
        report_error(&mut cx, "ERR_VM_MODULE_NOT_FOUND: vm context has been released");
        return false;
    };
    rooted!(&in(cx) let global = ptr);
    rooted!(&in(cx) let record_root: *mut JSObject = record);
    let mut realm = AutoRealm::new_from_handle(&mut cx, global.handle());
    // SAFETY: record 有效；返回的 namespace 由记录保活（引擎内边）
    let ns = unsafe { GetModuleNamespace(&mut realm, record_root.handle()) };
    if ns.is_null() {
        throw_vm(&mut realm, "Error", "vm could not read module namespace");
        return false;
    }
    rooted!(&in(&mut realm) let ns_root: *mut JSObject = ns);
    frame.set_rval(mozjs::jsval::ObjectValue(ns_root.get()));
    true
}

/// 摘除模块：`__wjs_vm_mod_release(modId)` → boolean。
pub unsafe extern "C" fn vm_mod_release(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 vm_create
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let s = match arg_string(&mut cx, &frame, 0, "vm Module") {
        Some(s) => s,
        None => return false,
    };
    let Ok(id) = s.parse::<u64>() else {
        report_error(&mut cx, "ERR_INVALID_ARG_TYPE: vm Module id must be a module id string");
        return false;
    };
    let _ = &mut cx;
    frame.set_rval(BooleanValue(state::vm_mod_release(id)));
    true
}

/// 异步落定补记：`__wjs_vm_mod_settled(modId)` → undefined。
/// 跨域求值恒异步（§4.57），promise 路径的 evaluated 位由 JS 壳在落定后补记。
pub unsafe extern "C" fn vm_mod_settled(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 vm_create
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let s = match arg_string(&mut cx, &frame, 0, "vm Module") {
        Some(s) => s,
        None => return false,
    };
    let Ok(id) = s.parse::<u64>() else {
        report_error(&mut cx, "ERR_INVALID_ARG_TYPE: vm Module id must be a module id string");
        return false;
    };
    state::vm_mod_set_evaluated(id);
    frame.set_rval(UndefinedValue());
    true
}

/// 内嵌 ESM 源（`node:vm`）。
pub const SOURCE: &str = r#"
import { EventEmitter } from "node:events";

// 簿记（ctx id / 快照）一律挂 WeakMap，不落沙箱自有键——真机 contextify 不给
// 沙箱加任何键（ownkeys/ownpropertynames/ownpropertysymbols 三件 + Proxy
// definer-interception 的 trap 计数都点名"沙箱键集不变"）。
const __vmBookkeeping = new WeakMap();
function __vmStdKeys(obj) {
  let rec = __vmBookkeeping.get(obj);
  if (!rec) {
    rec = { id: undefined, std: null, init: new Map() };
    __vmBookkeeping.set(obj, rec);
  }
  return rec;
}
function __vmCtxId(obj) {
  const rec = __vmBookkeeping.get(obj);
  return rec ? rec.id : undefined;
}

function __vmUnwrap(e) {
  const m = String((e && e.message) || e);
  const mm = m.match(/^__wjs_vm_error:([A-Za-z]+)\n([\s\S]*)$/);
  if (!mm) {
    const err = new Error(m);
    err.code = "ERR_VM_ERROR";
    throw err;
  }
  const [, name, body] = mm;
  // 位置标记（native vm_stk_envelope 附带）：剥出后重建 node displayErrors
  // 形态的栈——`filename:line\n源行\ncaret\n\nName: message\n    at f:l:c`。
  // checkErr 类 `err.stack.startsWith(filename)` 校验靠首行；帧格式引擎口径记档。
  let message = body;
  let stk = null;
  const sm = body.match(/^([\s\S]*)\n__wjs_vm_stk:(\{.*\})$/);
  if (sm) {
    message = sm[1];
    try { stk = JSON.parse(sm[2]); } catch { stk = null; }
  }
  const Ctor = globalThis[name] || Error;
  const err = new Ctor(message);
  if (stk && stk.f) {
    const caret = " ".repeat(Math.max(0, (stk.c | 0) - 1)) + "^";
    err.stack = `${stk.f}:${stk.l}\n${stk.s ?? ""}\n${caret}\n\n${name}: ${message}\n    at ${stk.f}:${stk.l}:${stk.c}`;
  }
  throw err;
}
function __vmCall(fn) {
  try {
    return fn();
  } catch (e) {
    // native 暂存的原始异常对象优先（保 vm realm 身份/原型/栈——跨域
    // `instanceof vmCtx.SyntaxError` 与栈断言点名）；信封重建只作兜底。
    let orig;
    try { orig = __wjs_vm_take_error(); } catch { orig = undefined; }
    if (orig !== undefined && orig !== null) {
      // 赋值类 TypeError 文案桥（native bridge_vm_assign_message 同源规则——
      // 原物透传绕过了 native 侧桥，按 node contextify 拦截器口径补齐）。
      if (orig && typeof orig === "object") {
        try {
          const m = orig.message;
          if (typeof m === "string") {
            const am = m.match(/^assignment to undeclared variable (\S+)$/);
            if (am) {
              orig.message = `${am[1]} is not defined`;
            } else {
              const bm = m.match(/^"([^"]+)" is (read-only|non-configurable and can't be redefined)$/);
              if (bm) {
                orig.message = bm[2] === "read-only"
                  ? `Cannot assign to read only property '${bm[1]}' of object '[object Object]'`
                  : `Cannot redefine property: ${bm[1]}`;
              }
            }
          }
        } catch { /* 保留原文案 */ }
        // 信封带位置标记时给原物栈补 node displayErrors 前缀
        //（`f:l\n源行\ncaret\n\n` + 原栈首行 Name: message 同构拼接）。
        const m = String((e && e.message) || e);
        // 组序：1=name、2=message、3=json 栈标记（与 __vmUnwrap 的双组序不同！）
        const sm = m.match(/^__wjs_vm_error:([A-Za-z]+)\n([\s\S]*)\n__wjs_vm_stk:(\{.*\})$/);
        if (sm) {
          try {
            const stk = JSON.parse(sm[3]);
            if (stk && stk.f) {
              const caret = " ".repeat(Math.max(0, (stk.c | 0) - 1)) + "^";
              orig.stack = `${stk.f}:${stk.l}\n${stk.s ?? ""}\n${caret}\n\n${orig.stack}`;
            }
          } catch { /* 保留原栈 */ }
        }
      }
      throw orig;
    }
    __vmUnwrap(e);
  }
}

let __vmFinal = null;
let __vmModFinal = null;
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
function __vmModAutoRelease(obj, id) {
  try {
    if (typeof FinalizationRegistry === "undefined") return;
    if (!__vmModFinal) {
      __vmModFinal = new FinalizationRegistry((held) => {
        try { __wjs_vm_mod_release(String(held)); } catch { /* 会话收尾期忽略 */ }
      });
    }
    __vmModFinal.register(obj, id);
  } catch { /* 无注册表即会话级存活，记档 */ }
}

function __validateCtx(obj) {
  if ((typeof obj !== "object" && typeof obj !== "function") || obj === null) {
    const err = new TypeError(`The "contextifiedObject" argument must be of type object. Received ${obj === null ? "null" : typeof obj}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const id = __vmCtxId(obj);
  if (typeof id !== "string") {
    // 真机 26 冠词即 "an vm.Context"（怪癖逐字）+ Received 实例描述
    const err = new TypeError(`The "contextifiedObject" argument must be an vm.Context. Received ${__recv(obj)}`);
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
  return typeof __vmCtxId(obj) === "string";
}

function __vmSnapshot(id, obj) {
  // 创建期快照：目标 global 全部自有字符串键集合 + 各键 SameValue 基线。
  // 值经 CCW 取回主域；跨 compartment 同底层恒同一引用，SameValue 可比（§4.57）。
  // 键枚举用 GetPropertyKeys（OWNONLY+HIDDEN），不可枚举的 defineProperty 产物同样覆盖。
  const rec = __vmStdKeys(obj);
  let std = [];
  try {
    std = JSON.parse(__vmCall(() => __wjs_vm_keys_all(id)));
  } catch {
    std = JSON.parse(__vmCall(() => __wjs_vm_keys(id)));
  }
  rec.std = new Set(std);
  rec.init = new Map();
  for (const k of std) {
    rec.init.set(k, __vmCall(() => __wjs_vm_get(id, k)));
  }
}
// symbol 键通道：键（symbol 本体作值）与完整描述符经字符串暂存位过域，
// 目标域内 defineProperty 落定（defineProperty 不触发访问器，无 mustCall 污染；
// 描述符对象过域照常工作，p38 实证）。
const __kTmpKey = "__wjs_vm_tmp_key";
const __kTmpDesc = "__wjs_vm_tmp_desc";
function __vmStageAndDefine(id, key, dd) {
  __vmCall(() => __wjs_vm_set(id, __kTmpKey, key));
  __vmCall(() => __wjs_vm_set(id, __kTmpDesc, dd));
  __vmCall(() => __wjs_vm_run(id,
    `Object.defineProperty(globalThis, globalThis[${JSON.stringify(__kTmpKey)}], globalThis[${JSON.stringify(__kTmpDesc)}]); delete globalThis[${JSON.stringify(__kTmpKey)}]; delete globalThis[${JSON.stringify(__kTmpDesc)}];`,
    "vm-sync-in.js"));
}
// 描述符读取（可抛）：SM proxy 不变量文案 → V8 口径（套件按 trap 名子串
// 断言；仅限本模块 sync 路径的窄桥，不改引擎全局文案）。
function __vmDescOrThrow(obj, k) {
  try {
    return Object.getOwnPropertyDescriptor(obj, k);
  } catch (e) {
    if (e && typeof e.message === "string") {
      const m = e.message.match(/^proxy can't report a non-existent property '"(.*)"' as non-configurable$/);
      if (m) {
        const err = new TypeError(`'getOwnPropertyDescriptor' on proxy: trap reported non-configurability for property '${m[1]}' which is either non-existent or configurable in the proxy target`);
        throw err;
      }
    }
    throw e;
  }
}
function __syncIn(id, obj) {
  // 主域侧全部自有字符串键（含不可枚举；与目标 global 的 HIDDEN 枚举口径对齐）。
  // 纯 Object.keys 会漏沙箱不可枚举种子（defineProperty value 形），vm 内即 undefined。
  // 描述符携带：数据描述符传 d.value（值拷贝，此时求值）；访问器传描述符对象本身——
  // 经 __wjs_vm_set 过 CCW 后在目标域内 Object.defineProperty 落定，getter/setter
  // 身份由引擎 CCW 透明代理（真机"访问器活绑定"同效：主域改 getter 实现即 vm 内可见；
  // 实证见 p38：描述符对象过沙箱属性中转后 define 照常工作）。
  // 只写目标缺席键（目标已有——首轮 define 产物或标准内建——即跳过）：
  // define+赋值双失败（目标只读）由 vm_set 静默跳过，但预检可省一次跨域调用；
  // 更重要的是预检以目标描述符为准，不以源值为准。
  // 注意：__wjs_vm_keys 只回可枚举键；不可枚举目标键不在此集——
  // 该分支漏检时 vm_set 的静默跳过是最后一道防线（本行注释钉住两层关系）。
  // 【10f 修订】预检取消：真机 contextify 的拦截器让 sandbox 自有键**遮蔽**
  // vm realm 内建（harmony-symbols/proxies：sandbox {Symbol} 后 vm 内读
  // Symbol 得主域 Symbol）——源键必须每次重落。目标已有的只读/不可配置键
  // 由 vm_set 的 define 失败→赋值失败→静默跳过兜底（global-non-writable
  // 的 vm 侧只读 x 不被沙箱可写值覆盖）。
  const keys = Object.getOwnPropertyNames(obj);
  for (const k of keys) {
    // 主域 globalThis 自指（globalThis/global）永不同步——值是主域 global 本体，
    // define 进目标即把目标 globalThis 换成主域 CCW，后续一切 set 全写错域
    //（BQ1：set globalThis 后 probe 即 undefined，而 set global 无事）。
    // 真机口径：目标自有 globalThis 恒为自身（AL1 mc-exists 同源），无需同步。
    if (k === "globalThis" || k === "global") continue;
    // 沙箱自指键（ctx.window = ctx）：真机沙箱即 global proxy，this/window 恒同
    // 身份——目标域内挂 vm global 本体（CCW 缓存保证 thisVal === windowVal），
    // 并记账 selfRefs 让 syncOut 不回写（回写会把 sandbox.window 换成 CCW）。
    let selfRef = false;
    try { selfRef = obj[k] === obj; } catch { selfRef = false; }
    if (selfRef) {
      try {
        const g = __vmCall(() => __wjs_vm_global(id));
        __vmCall(() => __wjs_vm_set(id, k, g));
        const rec0 = __vmStdKeys(obj);
        (rec0.selfRefs ??= new Set()).add(k);
      } catch { /* 跳过该键 */ }
      continue;
    }
    // globalThis 等宿主对象有"名无描述符"键（getOwnPropertyNames 列得出、
    // getOwnPropertyDescriptor 回 undefined）：无描述符即按值语义走 obj[k]。
    // 描述符读取错误必须容错——真机 contextify 不在 sync 期查询属性描述符
    //（proxy-failure-CP：trap 恒抛的 sandbox 照常 create/run）；set 期的
    // 不变量错误由 syncOut 的传播路径负责（set-property-proxy）。
    let d = null;
    try { d = __vmDescOrThrow(obj, k); } catch { d = null; }
    if (d && ("get" in d || "set" in d)) {
      // 键形态以"有无可调用 get/set"为准，不以 key 存在为准——
      // 宿主懒访问器（MessageChannel 等）可能是 {get: fn, set: undefined}，
      // set: undefined 必须剔除，否则目标域 defineProperty 读到
      // set: undefined 即判"描述符非对象"（AB3 实证；真机侧同键是数据描述符）。
      const hasGet = "get" in d && typeof d.get === "function";
      const hasSet = "set" in d && typeof d.set === "function";
      if (!hasGet && !hasSet) {
        // 伪访问器（get/set 皆不可调用）：按值语义走 obj[k]（此时求值）。
        try { __vmCall(() => __wjs_vm_set(id, k, obj[k])); } catch { /* 跳过该键 */ }
        continue;
      }
      // 真访问器：描述符对象暂存 + 目标域内 defineProperty 落定后删暂存。
      // 不删则数据暂存遮蔽访问器（setter 永不触发，p45 实证）。
      const t = `__wjs_vm_tmp_${k}`;
      const dd = { enumerable: false, configurable: true };
      if (hasGet) dd.get = d.get;
      if (hasSet) dd.set = d.set;
      dd.enumerable = !!d.enumerable;
      dd.configurable = !!d.configurable;
      __vmCall(() => __wjs_vm_set(id, t, dd));
      __vmCall(() => __wjs_vm_run(id, `Object.defineProperty(globalThis, ${JSON.stringify(k)}, globalThis[${JSON.stringify(t)}]); delete globalThis[${JSON.stringify(t)}]`, "vm-sync-in.js"));
    } else if (d && "value" in d) {
      if (d.writable === true && d.enumerable === true && d.configurable === true) {
        // 默认属性快路径（define_prop 即 {w,e,c}=true，无损失）。
        __vmCall(() => __wjs_vm_set(id, k, d.value));
      } else {
        // 非默认属性（nonWritableProp 等）走描述符 staging：真机按源描述符落定，
        // vm 侧 writable:false 不可写/不可枚举都要保形（global-setter descriptor10）。
        const dd = { value: d.value, writable: !!d.writable, enumerable: !!d.enumerable, configurable: !!d.configurable };
        try { __vmStageAndDefine(id, k, dd); } catch { /* 跳过该键 */ }
      }
    }
    else if (d) __vmCall(() => __wjs_vm_set(id, k, obj[k]));
    else {
      // 无描述符键：读值失败即跳过（globalThis 宿主键），不中断整表。
      try { __vmCall(() => __wjs_vm_set(id, k, obj[k])); } catch { /* 跳过该键 */ }
    }
  }
  // symbol 键同步（真机 contextify 转发 symbol 面；ownkeys/ownpropertysymbols/
  // global-setter 的 symbol 描述符都点名）。不可重定义等失败跳过该键，不中断。
  const syms = Object.getOwnPropertySymbols(obj);
  for (const s of syms) {
    let d = null;
    try { d = Object.getOwnPropertyDescriptor(obj, s); } catch { d = null; }
    if (!d) continue;
    const dd = { enumerable: !!d.enumerable, configurable: !!d.configurable };
    const hasGet = "get" in d && typeof d.get === "function";
    const hasSet = "set" in d && typeof d.set === "function";
    if (hasGet || hasSet) {
      if (hasGet) dd.get = d.get;
      if (hasSet) dd.set = d.set;
    } else {
      dd.value = d.value;
      dd.writable = !!d.writable;
    }
    try {
      __vmStageAndDefine(id, s, dd);
    } catch { /* 跳过该键 */ }
  }
}
function __syncOut(id, obj) {
  const rec = __vmBookkeeping.get(obj);
  const std = rec ? rec.std : null;
  const init = rec ? rec.init : null;
  const snap = JSON.parse(__vmCall(() => __wjs_vm_keys_all(id)));
  for (const entry of snap) {
    // symbol 占位无跨 realm 身份：只维护存在性（ownkeys 计数口径），不做值同步。
    if (entry !== null && typeof entry === "object") continue;
    const k = entry;
    // global 自有只读常量（undefined/NaN/Infinity，非枚举、不可写、值恒同）：永不同步。
    // 旧 keys（仅可枚举）路径从未见过它们；keys_all 含 HIDDEN 后必须显式跳过，
    // 否则 DONT_CONTEXTIFY（obj 即 vm global 本体）写只读属性直接抛。
    //（簿记键自 symbol 化起不再进字符串快照，无需再跳过。）
    if (k === "undefined" || k === "NaN" || k === "Infinity") continue;
    // syncIn 记账的自指键（window 等）：不回写（值是 vm global 本体，
    // 回写会把 sandbox 侧同键换成 CCW，破坏沙箱自指身份）。
    if (rec && rec.selfRefs && rec.selfRefs.has(k)) continue;
    // 源端访问器键：不读不写——syncIn 已装同款访问器，syncOut 再读/写即各多触发
    // 一次 getter/setter（global-setter 的 mustCall 精确计数口径）；值面归源端管。
    // 描述符查询语义：trap 自身抛的异常容错（proxy-failure-CP：不意外查询属性）；
    // 引擎不变量 TypeError 照真机传播（set-property-proxy：trap 返回 {} 报
    // non-configurability）。
    let d = null;
    try {
      d = __vmDescOrThrow(obj, k);
    } catch (e) {
      if (e && e.name === "TypeError") throw e;
      d = null;
    }
    if (d && ("get" in d || "set" in d)) continue;
    const cur = __vmCall(() => __wjs_vm_get(id, k));
    if (std !== null && std.has(k)) {
      // 快照内键：仅当与创建快照发生 SameValue 变化时回写（this.Symbol = Symbol 等）；
      // 未改即跳过，防标准构造器污染沙箱。SameValue 经引擎比较（NaN 自等，±0 区分）。
      const before = init.get(k);
      if (__vmCall(() => __wjs_vm_same(cur, before))) continue;
    }
    // 目标描述符优先：主域侧已有同名只读数据（源端 defineProperty 默认不可写不可配置，
    // 首轮 sync-in 的 define 产物即如此）则赋值抛——只读数据即跳过。
    // 可写数据才赋值（保留既有描述符，10c-3 回落语义）；目标缺席（全新键）直接挂载。
    if (d && d.writable === false) continue;
    try {
      obj[k] = cur;
    } catch {
      // 主域 getter-only（无 setter）赋值抛：真机静默不写（VV），此处同效跳过。
      // 有 setter 但 setter 内抛则会误吞——setter 抛的用例另案（套件无此形，记档）。
    }
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
  if (typeof v !== "number") {
    const err = new TypeError(`The "options.${what}" property must be of type number. Received ${__recv(v)}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (!Number.isInteger(v) || v < 0 || v > 4294967295) {
    const err = new RangeError(`The "options.${what}" property must be an integer in the range 0 to 4294967295. Received ${String(v)}`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  return v;
}
// timeout：真机 validateUint32(…, positive=true)——0/负数/NaN 也 RangeError。
function __normTimeout(v, what) {
  if (v === undefined) return undefined;
  if (typeof v !== "number") {
    const err = new TypeError(`The "options.${what}" property must be of type number. Received ${__recv(v)}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (!Number.isInteger(v) || v <= 0 || v > 4294967295) {
    const err = new RangeError(`The "options.${what}" property must be an integer in the range 1 to 4294967295. Received ${String(v)}`);
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
// ERR_INVALID_ARG_TYPE 的 Received 描述（node 真机口径：null → "null"、
// 原始值 → "type T (值)"、字符串带引号）。
function __recv(v) {
  if (v === null) return "null";
  if (typeof v === "string") return `type string ('${v}')`;
  if (typeof v === "function") return "type function";
  if (typeof v === "object") {
    if (Array.isArray(v)) return "an instance of Array";
    return "an instance of Object";
  }
  return `type ${typeof v} (${String(v)})`;
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
  __normTimeout(options.timeout, "timeout");
  __normBool(options.displayErrors, "displayErrors");
  __normBool(options.breakOnSigint, "breakOnSigint");
  return { filename };
}

export function createContext(contextObject = {}, options = {}) {
  // Node 24+ DONT_CONTEXTIFY（真机实测语义）：新建独立 context，返回其 global
  // 对象本体——不等于主 globalThis、写入不穿透主域、runInContext("this")===返回值。
  // jsdom 29（vitest jsdom 环境）拿它当 window 直装 DOM 全局。
  // 注意：新 global 只有 SpiderMonkey 标准内建（Object/Array/Symbol 等），
  // 无 winterjs 主域扩展（process/console/Buffer 等）——真机 vanilla 口径（§4.90 同源）。
  if (contextObject === __dontCtx) {
    const id = __vmCall(() => __wjs_vm_create());
    const g = __vmCall(() => __wjs_vm_global(id));
    __vmStdKeys(g).id = id;
    return g;
  }
  if (contextObject !== null && (typeof contextObject !== "object" && typeof contextObject !== "function")) {
    const err = new TypeError(`The "contextObject" argument must be of type object. Received type ${typeof contextObject}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (options === null || ((typeof options !== "object" && typeof options !== "function"))) {
    const err = new TypeError(`The "options" argument must be of type object. Received ${__recv(options)}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  // name/origin 字符串校验（真机 ERR_INVALID_ARG_TYPE 文案逐字，basic 套件点名）。
  for (const k of ["name", "origin"]) {
    if (options[k] !== undefined && typeof options[k] !== "string") {
      const err = new TypeError(`The "options.${k}" property must be of type string. Received ${__recv(options[k])}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
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
  __vmStdKeys(contextObject).id = id;
  __vmSnapshot(id, contextObject);
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
  options = options ?? {};
  // contextName/contextOrigin（createContext name/origin 的 run 侧别名）字符串校验。
  for (const k of ["contextName", "contextOrigin"]) {
    if (options[k] !== undefined && typeof options[k] !== "string") {
      const err = new TypeError(`The "options.${k}" property must be of type string. Received ${__recv(options[k])}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
  }
  if (contextObject === undefined) return runInContext(code, createContext({}, options ?? {}), options);
  const ctx = createContext(contextObject, options ?? {});
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
      const err = new TypeError(`The "options" argument must be of type object. Received ${__recv(options)}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    this.__code = code;
    this.__filename = __normStr(options.filename, "filename", "evalmachine.<anonymous>");
    void __normUint(options.lineOffset, "lineOffset");
    void __normUint(options.columnOffset, "columnOffset");
    void __normTimeout(options.timeout, "timeout");
    void __normBool(options.displayErrors, "displayErrors");
    void __normBool(options.breakOnSigint, "breakOnSigint");
    void __normBool(options.produceCachedData, "produceCachedData");
    // cachedData 类型门（真机 validateBufferish：Buffer/TypedArray/DataView）；
    // 字节码本体接受忽略（无缓存引擎，记档），类型不对仍按真机拒。
    if (options.cachedData !== undefined &&
        !(typeof ArrayBuffer.isView === "function" && ArrayBuffer.isView(options.cachedData)) &&
        !(options.cachedData instanceof ArrayBuffer)) {
      const err = new TypeError('The "options.cachedData" property must be one of Buffer, TypedArray, or DataView');
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    __vmCall(() => __wjs_vm_compile(code, this.__filename));
  }
  // Script 方法层 options 只收 object/function/undefined（真机 assertErrors：
  // 'bad'/42/null 即 TypeError）。
  __normOpts(options) {
    if (options !== undefined &&
        (options === null || (typeof options !== "object" && typeof options !== "function"))) {
      const err = new TypeError(`The "options" argument must be of type object. Received ${__recv(options)}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    return options ?? {};
  }
  runInContext(contextifiedObject, options) {
    return runInContext(this.__code, contextifiedObject, { ...this.__normOpts(options), filename: this.__filename });
  }
  // 真机形态：`this.runInContext(...)` 的成员访问先于实参求值——this 非对象时
  // 即报 `this.runInContext is not a function`（new-script-new-context 末块
  // `.call('hello')` 点名）；options 形状由 createContext 侧校验。
  runInNewContext(contextObject, options) {
    return this.runInContext(createContext(contextObject, options), options);
  }
  runInThisContext(options) {
    return runInThisContext(this.__code, { ...this.__normOpts(options), filename: this.__filename });
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
  // undefined → 默认 []；null/非数组 → 抛（真机 null 不给 ?? 吞掉）。
  const extOpt = options.contextExtensions;
  if (extOpt !== undefined && !Array.isArray(extOpt)) {
    const err = new TypeError(`The "options.contextExtensions" property must be an instance of Array. Received ${__recv(extOpt)}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const exts = extOpt ?? [];
  for (let ei = 0; ei < exts.length; ei++) {
    const ext = exts[ei];
    if (ext === null || (typeof ext !== "object" && typeof ext !== "function")) {
      const err = new TypeError(`The "options.contextExtensions[${ei}]" property must be of type object. Received type ${typeof ext} (${String(ext)})`);
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

// ── 9i-1 模块系（真机口径：node --experimental-vm-modules 实测，见模块头注）──
// SourceTextModule：Rust 侧编译/link/evaluate（零导入全链；带导入 link 即
// ERR_VM_MODULE_LINK_FAILURE，linker 切片后续）；SyntheticModule 纯 JS。

let __vmModSeq = 0;

function __modErr(code, message) {
  const err = new Error(message);
  err.code = code;
  throw err;
}

export class Module {
  link(linker) {
    if (typeof linker !== "function") {
      __modErr("ERR_INVALID_ARG_TYPE", `The "linker" argument must be of type function. Received ${linker === null ? "null" : typeof linker}`);
    }
    return this.__doLink(linker);
  }
  evaluate() {
    return this.__doEvaluate();
  }
}

export class SourceTextModule extends Module {
  constructor(code, options = {}) {
    super();
    if (typeof code !== "string") {
      __modErr("ERR_INVALID_ARG_TYPE", `The "code" argument must be of type string. Received type ${typeof code}`);
    }
    if (options === null || (typeof options !== "object" && typeof options !== "function")) {
      __modErr("ERR_INVALID_ARG_TYPE", `The "options" argument must be of type object. Received ${options === null ? "null" : typeof options}`);
    }
    const identifier = __normStr(options.identifier, "identifier", `vm:module(${__vmModSeq++})`);
    let ctxId;
    if (options.context !== undefined) {
      ctxId = __validateCtx(options.context);
      this.__context = options.context;
    } else {
      ctxId = __vmCall(() => __wjs_vm_create());
      const holder = {};
      __vmStdKeys(holder).id = ctxId;
      __vmSnapshot(ctxId, holder);
      __vmAutoRelease(holder, ctxId);
      this.__context = undefined;
    }
    this.__ctxId = ctxId;
    this.__identifier = identifier;
    this.__status = "unlinked";
    this.__error = null;
    this.__ns = undefined;
    // importModuleDynamically/initializeImportMeta 接受忽略（v1 未接线，记档）。
    this.__id = __vmCall(() => __wjs_vm_compile_mod(ctxId, identifier, code));
    __vmModAutoRelease(this, this.__id);
  }
  get status() { return this.__status; }
  get identifier() { return this.__identifier; }
  get context() { return this.__context; }
  get dependencySpecifiers() {
    return JSON.parse(__vmCall(() => __wjs_vm_mod_deps(this.__id)));
  }
  get namespace() {
    if (this.__status !== "evaluated") {
      __modErr("ERR_VM_MODULE_STATUS", "Module status must be evaluated");
    }
    return this.__ns;
  }
  get error() {
    if (this.__status !== "errored") {
      __modErr("ERR_VM_MODULE_STATUS", "Module status must be errored");
    }
    return this.__error;
  }
  __doLink(linker) {
    if (this.__status !== "unlinked") {
      return Promise.reject(Object.assign(new Error("Module status must be unlinked"), { code: "ERR_VM_MODULE_STATUS" }));
    }
    void linker;
    return Promise.resolve().then(() => {
      __vmCall(() => __wjs_vm_link(this.__id));
      this.__status = "linked";
    });
  }
  __doEvaluate() {
    if (this.__status !== "linked" && this.__status !== "evaluated" && this.__status !== "errored") {
      return Promise.reject(Object.assign(new Error("Module status must be linked"), { code: "ERR_VM_MODULE_STATUS" }));
    }
    if (this.__status === "evaluated") return Promise.resolve(undefined);
    return Promise.resolve().then(() => {
      // native 同步抛错（参数/link 前置）即失败落定；完成值可能是跨域 promise
      // （§4.57：`instanceof Promise` 跨 compartment 恒 false，必须按 thenable 认领，
      // 否则落定被丢弃、报错变 unhandled rejection）。
      const r = __vmCall(() => __wjs_vm_evaluate(this.__id));
      const done = () => {
        __vmCall(() => __wjs_vm_mod_settled(this.__id));
        this.__status = "evaluated";
        this.__ns = __vmCall(() => __wjs_vm_mod_ns(this.__id));
        return undefined;
      };
      const failed = (e) => {
        this.__status = "errored";
        this.__error = e;
        throw e;
      };
      if (r !== null && (typeof r === "object" || typeof r === "function") && typeof r.then === "function") {
        return r.then(done, failed);
      }
      try {
        return done();
      } catch (e) {
        return failed(e);
      }
    });
  }
}

export class SyntheticModule extends Module {
  constructor(exportNames, evaluateCallback, options = {}) {
    super();
    if (!Array.isArray(exportNames)) {
      __modErr("ERR_INVALID_ARG_TYPE", `The "exportNames" argument must be of type array. Received type ${typeof exportNames}`);
    }
    if (typeof evaluateCallback !== "function") {
      __modErr("ERR_INVALID_ARG_TYPE", `The "evaluateCallback" argument must be of type function. Received type ${typeof evaluateCallback}`);
    }
    if (options === null || (typeof options !== "object" && typeof options !== "function")) {
      __modErr("ERR_INVALID_ARG_TYPE", `The "options" argument must be of type object. Received ${options === null ? "null" : typeof options}`);
    }
    this.__exports = {};
    for (const n of exportNames) this.__exports[String(n)] = undefined;
    this.__cb = evaluateCallback;
    this.__identifier = __normStr(options.identifier, "identifier", `vm:module(${__vmModSeq++})`);
    this.__status = "linked";
    this.__error = null;
    this.__ns = undefined;
  }
  get status() { return this.__status; }
  get identifier() { return this.__identifier; }
  get dependencySpecifiers() { return undefined; }
  get namespace() {
    if (this.__status !== "evaluated") {
      __modErr("ERR_VM_MODULE_STATUS", "Module status must be evaluated");
    }
    return this.__ns;
  }
  get error() {
    if (this.__status !== "errored") {
      __modErr("ERR_VM_MODULE_STATUS", "Module status must be errored");
    }
    return this.__error;
  }
  setExport(name, value) {
    if (this.__status === "evaluated") {
      __modErr("ERR_VM_MODULE_STATUS", "Module status must not be evaluated");
    }
    this.__exports[String(name)] = value;
  }
  // Synthetic 的 linker 可选（真机口径：无参 link 即过）。
  link(linker) {
    if (linker !== undefined) return super.link(linker);
    return this.__doLink(undefined);
  }
  __doLink(linker) {
    void linker;
    // Synthetic 出生即 linked；重复 link 照真机保持 linked（无操作成功）。
    return Promise.resolve(undefined);
  }
  __doEvaluate() {
    if (this.__status === "evaluated") return Promise.resolve(undefined);
    return Promise.resolve().then(() => {
      try {
        this.__cb(this.__exports);
      } catch (e) {
        this.__status = "errored";
        this.__error = e;
        throw e;
      }
      this.__status = "evaluated";
      this.__ns = Object.freeze({ ...this.__exports });
      return undefined;
    });
  }
}

const __api = {
  Script, createContext, createScript, runInContext, runInNewContext,
  runInThisContext, isContext, compileFunction, measureMemory, constants,
  Module, SourceTextModule, SyntheticModule,
};
export default __api;
"#;
