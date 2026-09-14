//! ESM 运行时 glue：registry + HostLoadImportedModule hook + import.meta hook +
//! compile/link/evaluate 编排（Gecko153 新模块 API，见 plan.md Phase 2 路线）。
//!
//! 本模块属 mozjs 边界（AGENTS §6 允许 `unsafe` 的区域）。
//! 约定：hook 内自家错误一律暂存 `module_load_error` 并返回 false（静态加载由外层
//! 取出上报，保住行列信息；动态 import 额外 `report_error` 保住 rejection 原因）。

use std::collections::HashSet;
use std::ffi::CString;
use std::path::PathBuf;

use mozjs::context::JSContext;
use mozjs::conversions::ToJSValConvertible as _;
use mozjs::jsapi::{
    IsPromiseObject, JS_DefineProperty, JSObject, JSPROP_ENUMERATE, SetModuleLoadHook,
    SetModuleMetadataHook,
};
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;
use mozjs::rust::transform_str_to_source_text;
use mozjs::rust::wrappers2::{
    CompileModule1, FinishLoadingImportedModule, GetModuleRequestSpecifier, LoadRequestedModules,
};
use url::Url;

use crate::error::Error;
use crate::jsapi_glue::{raw_handle, report_error, value_to_string, wrap_cx};
use crate::loader::fetch::fetch;
use crate::loader::load_js;
use crate::loader::resolve::resolve;
use crate::loader::sourcemap::remap_location;
use crate::state;

// ── registry ─────────────────────────────────────────────────────────────

fn find_module(url: &str) -> Option<*mut JSObject> {
    state::with_rooted(|s| s.modules.iter().find(|m| m.url == url).map(|m| m.record.get()))
}

fn register_module(url: String, record: *mut JSObject) {
    state::with_rooted(|s| {
        if !s.modules.iter().any(|m| m.url == url) {
            s.modules.push(state::ModuleEntry { url, record: mozjs::jsapi::Heap::boxed(record) });
        }
    });
}

fn module_url(record: *mut JSObject) -> Option<String> {
    state::with_rooted(|s| {
        s.modules.iter().find(|m| m.record.get() == record).map(|m| m.url.clone())
    })
}

// ── 取回 + 转译 ──────────────────────────────────────────────────────────

/// URL 对应的本地解析路径（file: 真路径；data: 假名 `module.js`，按 JS 解析；
/// http(s): `remote-<blake3(url)>.<ext>` 假名（转译缓存键 + oxc SourceType 扩展名用，
/// 同一性仍按 URL 字符串，见 §4.12）。
fn module_path(url: &Url) -> Result<PathBuf, Error> {
    if url.scheme() == "file" {
        url.to_file_path().map_err(|_| Error::Other(format!("bad file URL: {url}")))
    } else if url.scheme() == "http" || url.scheme() == "https" {
        let ext = url
            .path()
            .rsplit('.')
            .next()
            .filter(|e| e.len() <= 5 && e.chars().all(|c| c.is_ascii_alphanumeric()))
            .unwrap_or("js");
        let hash = blake3::hash(url.as_str().as_bytes()).to_hex();
        Ok(PathBuf::from(format!("remote-{hash}.{ext}")))
    } else {
        Ok(PathBuf::from("module.js"))
    }
}

struct Prepared {
    original: String,
    js: String,
    imports: Vec<String>,
    is_module: bool,
    map: Option<String>,
}

fn prepare(url: &Url) -> Result<Prepared, Error> {
    // node:/bun: 内建：内嵌源直给（仍走 load_js 统一转译/提 imports；源内禁 TS）。
    if url.scheme() == "node" || url.scheme() == "bun" {
        let table = if url.scheme() == "node" {
            (crate::builtins::node::source(url.as_str()), crate::builtins::node::available())
        } else {
            (crate::builtins::bun::source(url.as_str()), crate::builtins::bun::available())
        };
        let Some(text) = table.0 else {
            return Err(Error::Other(format!(
                "'{}' is not a builtin (available: {})",
                url.as_str(),
                table.1.join(", ")
            )));
        };
        let text = text.to_owned();
        let path = PathBuf::from(format!("{}.js", url.as_str().replace(':', "_")));
        let loaded = load_js(&text, url.as_str(), &path)?;
        tracing::debug!(target: "winterjs::modules", url = url.as_str(), "builtin module prepared");
        return Ok(Prepared { original: text, js: loaded.js, imports: loaded.imports, is_module: true, map: loaded.map });
    }
    let fetched = fetch(url)?;
    let path = module_path(url)?;
    let loaded = load_js(&fetched.text, url.as_str(), &path)?;
    Ok(Prepared { original: fetched.text, js: loaded.js, imports: loaded.imports, is_module: loaded.is_module, map: loaded.map })
}

// ── 编译（registry 命中直接返回；同时返回静态 imports 供子图遍历）─────────

fn compile_source(cx: &mut JSContext, filename: &str, js: &str) -> Result<*mut JSObject, Error> {
    let c_filename = CString::new(filename).unwrap_or_else(|_| c"module.js".into());
    let options = mozjs::rust::CompileOptionsWrapper::new(cx, c_filename, 1);
    let mut src = transform_str_to_source_text(js);
    // SAFETY: realm 内；options/src 存活到调用返回；null 表解析失败（pending 异常转定位错误）
    let record = unsafe { CompileModule1(cx, options.ptr, &mut src) };
    if record.is_null() {
        rooted!(&in(cx) let mut exc = UndefinedValue());
        // CompileModule 刚失败，pending exception 存在；消费并转为定位错误
        match mozjs::rust::error_info_from_exception_stack(cx, exc.handle_mut()) {
            Some(info) => Err(Error::script(filename, js, info.line.max(1), info.col, info.message)),
            None => Err(Error::Other(format!("failed to parse module {filename}"))),
        }
    } else {
        Ok(record)
    }
}

/// file: 依赖的 CJS 互操作判定（Node ≥22 detect-module 口径，plan 9j）：
/// `.cjs` 恒 CJS；`.js`/`.jsx` 仅当最近 type 非 module、无 ESM 语法、
/// 且能按经典脚本解析（TLA 专属文件经典解析失败，走 ESM，保 §4.17 入口重试
/// 与 TLA 导入不退化）时 CJS；其余（mjs/mts/ts/非 file）走原 ESM 路。
/// 入口经典路径（`sniff_module`）不动。
fn cjs_interop(url: &Url, is_module: bool, text: &str) -> bool {
    if url.scheme() != "file" {
        return false;
    }
    let Ok(path) = url.to_file_path() else {
        return false;
    };
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("cjs") => true,
        Some("js" | "jsx") => {
            !is_module
                && crate::builtins::node::require::nearest_pkg_type(&path).as_deref()
                    != Some("module")
                && parses_as_script(text, &path)
        }
        _ => false,
    }
}

/// 经典脚本目标试解析（oxc script/unambiguous goal）：TLA/import/export 任一
/// 即失败或升级为模块信号（`has_module_syntax`，oxc 无歧义 await 自动升级）。
/// 纯函数；只在上述歧义集上调用（ESM 已定文件不走这里，无额外开销）。
fn parses_as_script(text: &str, path: &std::path::Path) -> bool {
    use oxc::{allocator::Allocator, parser::Parser, span::SourceType};
    let allocator = Allocator::default();
    let Ok(source_type) = SourceType::from_path(path).map(|t| t.with_module(false)) else {
        return false;
    };
    let ret = Parser::new(&allocator, text, source_type).parse();
    !ret.fatal_error && !ret.diagnostics.has_errors() && !ret.module_record.has_module_syntax
}

/// CJS 互操作垫片：同步 require 整包再导出（单 `export {}` 子句；具名
/// `export { tmp as "key" }` 全引号形，`import { "kebab-name" }` 双侧实测；
/// CJS 依赖在求值期懒解析，静态子图无需预编译；命名表由编译期
/// `cjs_export_names` 快照运行时键——`__exportStar` 合并键天然在内，
/// 快照后新增键/循环半成品缺键记档）。
/// 具名 `default` 即整包（真机实测：`import { default as d }` 得整包而非
/// `.default` 属性，故 `default` 键映射回整包，不单列）。
fn cjs_shim_js(url: &Url, names: &[String]) -> String {
    let mut js = String::from("const __wjs_cjs_exports = globalThis.__wjs_require_cjs_by_url(");
    js.push_str(&serde_json::to_string(url.as_str()).unwrap_or_else(|_| "\"\"".into()));
    js.push_str(");\n");
    let mut entries: Vec<String> = Vec::with_capacity(names.len() + 1);
    entries.push("__wjs_cjs_exports as default".to_owned());
    for (i, k) in names.iter().enumerate() {
        if k == "default" {
            continue;
        }
        js.push_str(&format!(
            "const __wjs_e_{i} = __wjs_cjs_exports[{}];\n",
            serde_json::to_string(k).unwrap_or_default()
        ));
        entries.push(format!(
            "__wjs_e_{i} as {}",
            serde_json::to_string(k).unwrap_or_default()
        ));
    }
    js.push_str("export { ");
    js.push_str(&entries.join(", "));
    js.push_str(" };\n");
    js
}

/// 取回 + 转译 + 编译 + 注册（无递归；调用方负责遍历）。
fn compile_url(cx: &mut JSContext, url: &Url) -> Result<(*mut JSObject, Vec<String>), Error> {
    if let Some(record) = find_module(url.as_str()) {
        return Ok((record, Vec::new()));
    }
    let prepared = prepare(url)?;
    if cjs_interop(url, prepared.is_module, &prepared.original) {
        // 命名导出发现（M5）：realm 内同步 require 取运行时键（best-effort；
        // 失败回 default-only，求值期 require 走原语义；双跑/半成品记档见
        // `cjs_export_names`）。
        let names = {
            let global = state::global();
            rooted!(&in(cx) let global_root: *mut JSObject = global);
            // SAFETY: global 由 global_root 保活；同步 require 的 realm 见 §4.1。
            let mut realm = mozjs::realm::AutoRealm::new_from_handle(cx, global_root.handle());
            crate::builtins::node::require::cjs_export_names(&mut realm, global_root.get(), url)
        };
        let js = cjs_shim_js(url, &names);
        let record = compile_source(cx, url.as_str(), &js)?;
        register_module(url.as_str().to_owned(), record);
        let Prepared { original, map, .. } = prepared;
        tracing::debug!(target: "winterjs::modules", url = url.as_str(), "cjs interop shim compiled");
        let debug = state::ModuleDebug { original, map };
        state::with_plain(|p| {
            p.module_debug.insert(url.as_str().to_owned(), debug);
        });
        return Ok((record, Vec::new()));
    }
    let record = compile_source(cx, url.as_str(), &prepared.js)?;
    register_module(url.as_str().to_owned(), record);
    let Prepared { original, js: _, imports, map, .. } = prepared;
    tracing::debug!(target: "winterjs::modules", url = url.as_str(), deps = imports.len(), "module compiled");
    let debug = state::ModuleDebug { original, map };
    state::with_plain(|p| {
        p.module_debug.insert(url.as_str().to_owned(), debug);
    });
    Ok((record, imports))
}

/// 入口编译（给 `runtime` 用；imports 不需要）。
pub fn compile_entry(cx: &mut JSContext, url: &Url) -> Result<*mut JSObject, Error> {
    compile_url(cx, url).map(|(r, _)| r)
}

// ── 报错回映射 ───────────────────────────────────────────────────────────

/// 模块求值错误：pending 转定位错误，按异常文件名找回源文件，TS 经 sourcemap 回映射。
/// 前置条件：刚失败且 pending exception 存在（link/evaluate/定时器回调失败点）。
pub fn module_error(cx: &mut JSContext, fallback_url: &str) -> Error {
    rooted!(&in(cx) let mut exc = UndefinedValue());
    let Some(info) = mozjs::rust::error_info_from_exception_stack(cx, exc.handle_mut()) else {
        return Error::Other("uncaught module exception (no stack info)".into());
    };
    let filename = if info.filename.is_empty() {
        fallback_url.to_owned()
    } else {
        info.filename.clone()
    };
    let debug = state::with_plain(|p| p.module_debug.get(&filename).cloned());
    match debug {
        Some(d) => {
            let (line, col) =
                remap_location(d.map.as_deref(), info.line.max(1), info.col.max(1));
            Error::script(&filename, &d.original, line, col, info.message)
        }
        None => Error::script(
            &filename,
            fallback_url,
            info.line.max(1),
            info.col.max(1),
            info.message,
        ),
    }
}

// ── hooks ────────────────────────────────────────────────────────────────

/// referrer 脚本的文件名 → base URL（CompileOptions 写入的即模块 URL 字符串）。
/// 私有值通道（SetModule/ScriptPrivate）在 153 下收不到 GC 字符串，改走文件名（实测 §4.11）。
fn referrer_base(script: *mut mozjs::jsapi::JSScript) -> Option<Url> {
    if script.is_null() {
        return None;
    }
    // SAFETY: hook 调用期内 referrer 脚本存活；返回的借用指针立即拷贝成 String
    let name = unsafe {
        let c = mozjs::jsapi::JS_GetScriptFilename(script);
        if c.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr(c).to_string_lossy().into_owned()
    };
    Url::parse(&name).ok()
}

/// payload 是否 Promise（是 → 动态 import；否 → 静态加载态）。
fn payload_is_promise(cx: &mut JSContext, payload: JSVal) -> bool {
    if !payload.is_object() {
        return false;
    }
    // SAFETY: is_object 已判定
    let obj = payload.to_object();
    rooted!(&in(cx) let obj_root: *mut JSObject = obj);
    // SAFETY: obj_root 为有效 rooted 对象
    unsafe { IsPromiseObject(raw_handle(obj_root.as_ptr())) }
}

/// 失败：暂存友好错误；动态场景额外 report（保住 rejection 原因）；一律返回 false。
fn fail_load(cx: &mut JSContext, err: Error, dynamic: bool) -> bool {
    tracing::debug!(target: "winterjs::modules", dynamic, "module load failed: {err}");
    if dynamic {
        report_error(cx, &err.to_string());
    }
    state::with_plain(|p| p.module_load_error = Some(err));
    false
}

/// 子图全编译 + 引擎加载 + link（动态 import 用：引擎不遍历后代，host 负责整图；
/// 注意 ModuleLink 要求先走完加载态（直接 link 报 `unexpected status: New`），
/// 故内嵌一次 `load_dependencies`（边经 hook 从注册表命中，循环由引擎处理）。
/// `require(node:)` 复用同一入口（pub(crate)）。
pub(crate) fn ensure_subgraph(cx: &mut JSContext, root: &Url) -> Result<*mut JSObject, Error> {
    use mozjs::rust::wrappers2::ModuleLink;

    let mut seen: HashSet<String> = HashSet::new();
    let mut stack = vec![root.clone()];
    let mut root_record = None;
    while let Some(url) = stack.pop() {
        if !seen.insert(url.as_str().to_owned()) {
            continue;
        }
        let (record, imports) = compile_url(cx, &url)?;
        if root_record.is_none() {
            root_record = Some(record);
        }
        for spec in imports {
            stack.push(resolve(&spec, Some(&url))?);
        }
    }
    let Some(root_record) = root_record else {
        return Err(Error::Other("empty module subgraph".into()));
    };
    load_dependencies(cx, root_record)?;
    rooted!(&in(cx) let root_obj: *mut JSObject = root_record);
    // SAFETY: root 为有效 rooted 记录；加载态已就绪，realm 内同步 link
    if !unsafe { ModuleLink(cx, root_obj.handle()) } {
        rooted!(&in(cx) let mut exc = UndefinedValue());
        // link 失败的 pending 异常消费为定位错误
        match mozjs::rust::error_info_from_exception_stack(cx, exc.handle_mut()) {
            Some(info) => {
                return Err(Error::script(
                    root.as_str(),
                    "",
                    info.line.max(1),
                    info.col,
                    info.message,
                ));
            }
            None => return Err(Error::Other(format!("failed to link {}", root.as_str()))),
        }
    }
    Ok(root_record)
}

/// SAFETY: 由引擎以有效参数回调（HostLoadImportedModule 约定）。
unsafe extern "C" fn load_hook(
    cx_raw: *mut mozjs::jsapi::JSContext,
    referrer: mozjs::jsapi::Handle<*mut mozjs::jsapi::JSScript>,
    module_request: mozjs::jsapi::Handle<*mut JSObject>,
    _host_defined: mozjs::jsapi::Handle<JSVal>,
    payload: mozjs::jsapi::Handle<JSVal>,
    _line: u32,
    _col: mozjs::jsapi::ColumnNumberOneOrigin,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效
    let mut cx = unsafe { wrap_cx(cx_raw) };
    // hook 触发时不保证在 realm 内，先重进 global（§4.1 铁律）
    let global = state::global();
    rooted!(&in(cx) let global_root: *mut JSObject = global);
    let mut realm = mozjs::realm::AutoRealm::new_from_handle(&mut cx, global_root.handle());
    let cx: &mut JSContext = &mut realm;

    // SAFETY: hook 参数 Handle 在调用期内有效；转成带生命期的 rust Handle
    let spec = unsafe {
        let req = mozjs::gc::Handle::from_marked_location(&*module_request.ptr);
        let jsstr = GetModuleRequestSpecifier(cx, req);
        if jsstr.is_null() {
            return fail_load(cx, Error::Other("cannot read import specifier".into()), false);
        }
        mozjs::conversions::jsstr_to_string(cx, std::ptr::NonNull::new_unchecked(jsstr))
    };
    // SAFETY: 同上；payload 只读出（是否 Promise 决定动/静路径）
    let payload_val = unsafe { *payload.ptr };
    let dynamic = payload_is_promise(cx, payload_val);
    // SAFETY: referrer Handle 在调用期内有效
    let base = unsafe { referrer_base(*referrer.ptr) };
    tracing::debug!(target: "winterjs::modules", spec, dynamic, "load hook");

    let url = match resolve(&spec, base.as_ref()) {
        Ok(u) => u,
        Err(e) => return fail_load(cx, e, dynamic),
    };
    // 同一 (referrer, specifier) 必须同结果：注册表按 URL 去重天然满足。
    let result = if dynamic {
        ensure_subgraph(cx, &url)
    } else {
        compile_url(cx, &url).map(|(r, _)| r)
    };
    let record = match result {
        Ok(r) => r,
        Err(e) => return fail_load(cx, e, dynamic),
    };
    rooted!(&in(cx) let record_root: *mut JSObject = record);
    // SAFETY: 槽位转换（from_marked_location）+ safe wrapper 调用
    unsafe {
        let referrer_h = mozjs::gc::Handle::from_marked_location(&*referrer.ptr);
        let request = mozjs::gc::Handle::from_marked_location(&*module_request.ptr);
        let payload_h = mozjs::gc::Handle::from_marked_location(&*payload.ptr);
        FinishLoadingImportedModule(cx, referrer_h, request, payload_h, record_root.handle(), dynamic)
    }
}

/// SAFETY: 由引擎以有效参数回调（ModuleMetadataHook 约定）。
unsafe extern "C" fn metadata_hook(
    cx_raw: *mut mozjs::jsapi::JSContext,
    module_record: mozjs::jsapi::Handle<*mut JSObject>,
    meta_object: mozjs::jsapi::Handle<*mut JSObject>,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效
    let mut cx = unsafe { wrap_cx(cx_raw) };
    // SAFETY: 参数 Handle 在调用期内有效
    let (record, meta) = unsafe { (*module_record.ptr, *meta_object.ptr) };
    let global = state::global();
    rooted!(&in(cx) let global_root: *mut JSObject = global);
    let mut realm = mozjs::realm::AutoRealm::new_from_handle(&mut cx, global_root.handle());
    let cx: &mut JSContext = &mut realm;
    let Some(url) = module_url(record) else {
        return true; // 未知模块：import.meta 保持空对象
    };
    rooted!(&in(cx) let mut v = UndefinedValue());
    url.to_jsval(cx, v.handle_mut());
    // SAFETY: cx/meta/v 均有效；c"url" 无 NUL
    unsafe {
        JS_DefineProperty(
            cx.raw_cx(),
            raw_handle(&meta),
            c"url".as_ptr(),
            raw_handle(v.as_ptr()),
            JSPROP_ENUMERATE as u32,
        )
    }
}

/// 安装模块 hooks（`install` 同款进程级一次性语义；操作对象是 rt，realm 内外皆可）。
pub fn install_hooks(rt: &mozjs::rust::Runtime) {
    // SAFETY: rt 存活；hook 为 'static fn，状态走 TLS
    unsafe {
        let raw = rt.rt();
        SetModuleLoadHook(raw, Some(load_hook));
        SetModuleMetadataHook(raw, Some(metadata_hook));
    }
    tracing::debug!(target: "winterjs::modules", "module hooks installed");
}

// ── LoadRequestedModules 回调 ────────────────────────────────────────────

/// 成功置旗（无状态可记；失败侧走 stash）。
unsafe extern "C" fn load_resolved(
    _cx: *mut mozjs::jsapi::JSContext,
    _v: mozjs::jsapi::Handle<JSVal>,
) -> bool {
    true
}

/// 失败：引擎侧的值转字符串暂存（hook 侧 stash 优先，带行列）。
unsafe extern "C" fn load_rejected(
    cx_raw: *mut mozjs::jsapi::JSContext,
    _host_defined: mozjs::jsapi::Handle<JSVal>,
    error: mozjs::jsapi::Handle<JSVal>,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效
    let mut cx = unsafe { wrap_cx(cx_raw) };
    // SAFETY: 参数 Handle 在调用期内有效
    let val = unsafe { *error.ptr };
    let msg = value_to_string(&mut cx, val);
    state::with_plain(|p| {
        if p.module_load_error.is_none() {
            p.module_load_error = Some(Error::Other(format!("module load failed: {msg}")));
        }
    });
    // SAFETY: rejected 时引擎一般已置异常；清场避免污染后续
    unsafe { mozjs::jsapi::JS_ClearPendingException(cx.raw_cx()) };
    true
}

/// `LoadRequestedModules` 同步加载入口的依赖图；失败取 stash（hook 已暂存友好错误）。
pub fn load_dependencies(cx: &mut JSContext, entry: *mut JSObject) -> Result<(), Error> {
    rooted!(&in(cx) let entry_root: *mut JSObject = entry);
    rooted!(&in(cx) let undef = UndefinedValue());
    // LoadRequestedModules 为引擎调用（unsafe）；Handle 由 rooted 守卫直接给
    let ok = unsafe {
        LoadRequestedModules(
            cx,
            entry_root.handle(),
            undef.handle(),
            Some(load_resolved),
            Some(load_rejected),
        )
    };
    if !ok {
        // hook 返回 false 的路径：rejected 回调已暂存；防御性清场
        // SAFETY: 仅清 pending，不读值
        unsafe { mozjs::jsapi::JS_ClearPendingException(cx.raw_cx()) };
    }
    if let Some(err) = state::with_plain(|p| p.module_load_error.take()) {
        return Err(err);
    }
    if !ok {
        return Err(Error::Other("module dependencies failed to load".into()));
    }
    Ok(())
}
