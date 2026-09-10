//! `require()` CJS（plan Phase 4d-1）：
//! - `require("node:X")`：ESM 子图求值（幂等标记）+ `default` 导出（缺省回 namespace）。
//! - `require("./rel")`：调用方定位（`describe_scripted_caller`）→ `.json` 解析 /
//!   `.cjs` 包装执行（`exports` 预注册，循环可见半成品；失败清场）/ ESM 报
//!   `ERR_REQUIRE_ESM` 用 `import`。
//! - 调用传参用柯里化 `call_one` 链（§4.9：native 内禁 `Rooted<ValueArray>`）。
//! - 入口 `.cjs` 经 prelude `__wjs_require_main` 起（`run` 不打印其 exports）。

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::jsapi::JSObject;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;
use mozjs::rust::{evaluate_script, CompileOptionsWrapper};
use url::Url;

use crate::error::Error;
use crate::jsapi_glue::{
    call_one, get_prop_value, parse_json, report_error, value_to_string, wrap_cx, Frame,
};
use crate::loader::load_js;
use crate::loader::resolve::resolve;
use crate::state;

/// 调用方 base（脚本文件名 → URL；eval/prelude 等非 URL 返回 None）。
fn caller_base(cx: &mozjs::context::JSContext) -> Option<Url> {
    let caller = mozjs::rust::describe_scripted_caller(cx).ok()?;
    tracing::debug!(target: "winterjs::require", caller = caller.filename, "scripted caller");
    let url = Url::parse(&caller.filename).ok()?;
    match url.scheme() {
        "file" | "node" => Some(url),
        _ => None,
    }
}

/// pending 异常 → 消息串（消费异常；无则兜底）。
fn pending_message(cx: &mut mozjs::context::JSContext) -> String {
    rooted!(&in(cx) let mut exc = UndefinedValue());
    match mozjs::rust::error_info_from_exception_stack(cx, exc.handle_mut()) {
        Some(info) => info.message,
        None => "uncaught exception".to_string(),
    }
}

/// `node:` ESM 求值 + `default` 导出（无 default 回 namespace 本体）。
fn require_esm_default(
    cx: &mut mozjs::context::JSContext,
    global: *mut JSObject,
    url: &Url,
) -> Result<JSVal, String> {
    use mozjs::rust::wrappers2::{GetModuleNamespace, ModuleEvaluate};
    let record = crate::modules::ensure_subgraph(cx, url).map_err(|e| e.to_string())?;
    if !state::module_evaluated(url.as_str()) {
        rooted!(&in(cx) let record_root: *mut JSObject = record);
        rooted!(&in(cx) let mut rval = UndefinedValue());
        // SAFETY: record 为有效 rooted 记录；realm 内同步求值
        if !unsafe { ModuleEvaluate(cx, record_root.handle(), rval.handle_mut()) } {
            let msg = crate::modules::module_error(cx, url.as_str()).to_string();
            return Err(msg);
        }
        state::set_module_evaluated(url.as_str().to_owned());
    }
    rooted!(&in(cx) let record_root: *mut JSObject = record);
    // SAFETY: record 有效；返回的 namespace 由记录保活（引擎内边）
    let ns = unsafe { GetModuleNamespace(cx, record_root.handle()) };
    if ns.is_null() {
        return Err(format!("cannot read namespace of '{}'", url.as_str()));
    }
    rooted!(&in(cx) let ns_root: *mut JSObject = ns);
    let _ = global;
    match get_prop_value(cx, ns_root.get(), c"default") {
        Some(v) if !v.is_undefined() => Ok(v),
        _ => Ok(mozjs::jsval::ObjectValue(ns_root.get())),
    }
}

/// CJS 包装执行（`load_js` 已转译；`module` 经 prelude 建；返回终态 `module.exports`）。
#[allow(clippy::too_many_lines)]
fn require_cjs_file(
    cx: &mut mozjs::context::JSContext,
    global: *mut JSObject,
    url: &Url,
    js: &str,
) -> Result<JSVal, String> {
    // 柯里化包装（单参链，§4.9 合规；`module`/`exports` 同一对象起）。
    let wrapped = format!(
        "((exports) => (require) => (module) => (__filename) => (__dirname) => {{\n{js}\n}})"
    );
    let c_filename =
        std::ffi::CString::new(url.as_str()).unwrap_or_else(|_| c"module.js".into());
    let options = CompileOptionsWrapper::new(cx, c_filename, 1);
    rooted!(&in(cx) let global_root: *mut JSObject = global);
    rooted!(&in(cx) let mut fn_v = UndefinedValue());
    let res = evaluate_script(cx, global_root.handle(), wrapped.as_str(), fn_v.handle_mut(), options);
    if res.is_err() {
        return Err(pending_message(cx));
    }
    if !fn_v.is_object() {
        return Err(format!("cannot load '{}': wrapper failed", url.as_str()));
    }
    // module 对象（prelude 建；`{exports: {}, id, filename, paths}`）。
    let Some(make_fn) = get_prop_value(cx, global_root.get(), c"__wjs_make_module") else {
        return Err("prelude helper __wjs_make_module missing".into());
    };
    rooted!(&in(cx) let mut url_v = UndefinedValue());
    url.as_str().to_jsval(cx, url_v.handle_mut());
    let Some(module_v) = call_one(cx, global_root.get(), make_fn, url_v.get()) else {
        return Err(pending_message(cx));
    };
    if !module_v.is_object() {
        return Err(format!("cannot load '{}': bad module object", url.as_str()));
    }
    rooted!(&in(cx) let module_root: *mut JSObject = module_v.to_object());
    let Some(exports_v) = get_prop_value(cx, module_root.get(), c"exports") else {
        return Err(pending_message(cx));
    };
    let Some(require_v) = get_prop_value(cx, global_root.get(), c"require") else {
        return Err("global require missing".into());
    };
    // 预注册（循环可见半成品；失败清场）。
    state::cjs_register(url.as_str().to_owned(), exports_v);
    // __filename/__dirname（file: URL；其余退原文）。
    let (filename_s, dirname_s) = match url.to_file_path() {
        Ok(p) => {
            let dir = p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
            (p.to_string_lossy().into_owned(), dir)
        }
        Err(_) => (url.as_str().to_owned(), String::new()),
    };
    // 五连单参调用（§4.9：不用 ValueArray；串先 rooted，避闭包借用冲突）。
    rooted!(&in(cx) let mut s_v = UndefinedValue());
    filename_s.to_jsval(cx, s_v.handle_mut());
    let filename_v = s_v.get();
    rooted!(&in(cx) let mut d_v = UndefinedValue());
    dirname_s.to_jsval(cx, d_v.handle_mut());
    let dirname_v = d_v.get();
    let mut cur = fn_v.get();
    let mut call_arg = |label: &str, arg: JSVal| -> Result<JSVal, String> {
        if !cur.is_object() {
            state::cjs_remove(url.as_str());
            return Err(format!("cannot load '{}': {label} is not callable", url.as_str()));
        }
        match call_one(cx, global_root.get(), cur, arg) {
            Some(v) => {
                cur = v;
                Ok(v)
            }
            None => {
                state::cjs_remove(url.as_str());
                Err(pending_message(cx))
            }
        }
    };
    call_arg("exports", exports_v)?;
    call_arg("require", require_v)?;
    call_arg("module", module_v)?;
    call_arg("__filename", filename_v)?;
    call_arg("__dirname", dirname_v)?;
    // 终态 exports（允许执行期重赋值 `module.exports = …`；先移除预注册再记终态）。
    match get_prop_value(cx, module_root.get(), c"exports") {
        Some(final_v) => {
            state::cjs_remove(url.as_str());
            state::cjs_register(url.as_str().to_owned(), final_v);
            Ok(final_v)
        }
        None => {
            state::cjs_remove(url.as_str());
            Err(pending_message(cx))
        }
    }
}

/// spec → 值（node:/file: 分流；调用方 base 定位相对路径）。
fn require_value(
    cx: &mut mozjs::context::JSContext,
    global: *mut JSObject,
    spec: &str,
    base: Option<Url>,
) -> Result<JSVal, Error> {
    // 内建（含 `fs` 裸名）优先，与 ESM 一致。
    if let Some(canonical) = crate::builtins::node::normalize_spec(spec) {
        let url = Url::parse(canonical)
            .map_err(|e| Error::Other(format!("bad builtin URL: {e}")))?;
        return require_esm_default(cx, global, &url).map_err(Error::Other);
    }
    let url = resolve(spec, base.as_ref()).map_err(|e| {
        Error::Other(format!("Cannot find module '{spec}' ({e})"))
    })?;
    match url.scheme() {
        "node" => require_esm_default(cx, global, &url).map_err(Error::Other),
        "file" => {
            if let Some(hit) = state::cjs_find(url.as_str()) {
                return Ok(hit);
            }
            let path = url
                .to_file_path()
                .map_err(|_| Error::Other(format!("Cannot find module '{spec}'")))?;
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext == "json" {
                let text = std::fs::read_to_string(&path).map_err(|e| {
                    Error::Other(format!("Cannot find module '{spec}' ({})", e))
                })?;
                match parse_json(cx, global, &text) {
                    Some(v) => {
                        state::cjs_register(url.as_str().to_owned(), v);
                        Ok(v)
                    }
                    None => Err(Error::Other(format!("'{spec}' is not valid JSON"))),
                }
            } else {
                let text = std::fs::read_to_string(&path).map_err(|e| {
                    Error::Other(format!("Cannot find module '{spec}' ({e})"))
                })?;
                let loaded = load_js(&text, url.as_str(), &path).map_err(|e| {
                    Error::Other(format!("Cannot load '{spec}' ({e})"))
                })?;
                if loaded.is_module {
                    return Err(Error::Other(format!(
                        "require() of ES Module '{spec}' is not supported; use import instead"
                    )));
                }
                require_cjs_file(cx, global, &url, &loaded.js).map_err(Error::Other)
            }
        }
        s => Err(Error::Other(format!("Cannot require '{spec}' (scheme '{s}:')"))),
    }
}

/// `require(id)` 全局函数（裸 native：调用方定位要求直调，禁 prelude 包装）。
pub unsafe extern "C" fn require_native(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 || !frame.arg(0).is_string() {
        report_error(&mut cx, "TypeError: require needs a module id string");
        return false;
    }
    let spec = value_to_string(&mut cx, frame.arg(0));
    let global = state::global();
    rooted!(&in(cx) let global_root: *mut JSObject = global);
    let base = caller_base(&cx);
    match require_value(&mut cx, global_root.get(), &spec, base) {
        Ok(v) => {
            rooted!(&in(cx) let v_root = v);
            frame.set_rval(v_root.get());
            true
        }
        Err(e) => {
            report_error(&mut cx, &e.to_string());
            false
        }
    }
}

/// `__wjs_require_resolve(id)` → 解析后 URL 串（`require.resolve` 用；同调用方规则）。
pub unsafe extern "C" fn require_resolve(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 || !frame.arg(0).is_string() {
        report_error(&mut cx, "TypeError: require.resolve needs a module id string");
        return false;
    }
    let spec = value_to_string(&mut cx, frame.arg(0));
    let base = caller_base(&cx);
    let url = if let Some(canonical) = crate::builtins::node::normalize_spec(&spec) {
        Url::parse(canonical).map_err(|e| Error::Other(format!("bad builtin URL: {e}")))
    } else {
        resolve(&spec, base.as_ref())
    };
    match url {
        Ok(u) => {
            rooted!(&in(cx) let mut v = UndefinedValue());
            u.as_str().to_jsval(&mut cx, v.handle_mut());
            frame.set_rval(v.get());
            true
        }
        Err(e) => {
            report_error(&mut cx, &format!("Cannot find module '{spec}' ({e})"));
            false
        }
    }
}

/// `require` 附属 prelude（`NODE_PRELUDE` 尾部拼装；`require` 本体为裸 native）。
pub const REQUIRE_PRELUDE: &str = r#"
globalThis.__wjs_make_module = (filename) => ({
  exports: {},
  id: String(filename),
  filename: String(filename),
  paths: [],
});
globalThis.__wjs_require_main = (url) => globalThis.require(String(url));
globalThis.require.resolve = (id) => __wjs_require_resolve(String(id));
Object.defineProperty(globalThis.require, "main", {
  configurable: true,
  get() {
    const u = __wjs_require_main_url();
    if (u === undefined) return undefined;
    return { filename: u, id: u, paths: [] };
  },
});
"#;

/// `__wjs_require_main_url()` → 主模块 URL 串｜undefined（prelude 包成对象）。
pub unsafe extern "C" fn require_main_url(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    match state::main_module() {
        Some(url) => {
            rooted!(&in(cx) let mut v = UndefinedValue());
            url.to_jsval(&mut cx, v.handle_mut());
            frame.set_rval(v.get());
            true
        }
        None => {
            frame.set_rval(UndefinedValue());
            true
        }
    }
}
