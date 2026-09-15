//! `require()` CJS（plan Phase 4d-1/c-4x）：
//! - `require("node:X")`：ESM 子图求值（幂等标记）+ `default` 导出（缺省回 namespace）。
//! - `require("./rel")`：调用方定位（`describe_scripted_caller`）→ `.json` 解析 /
//!   `.cjs` 包装执行（`exports` 预注册，循环可见半成品；失败清场）/ ESM 报
//!   `ERR_REQUIRE_ESM` 用 `import`。
//! - `.js` 读最近 `package.json` 的 `type`（Node 口径）：`module` → 一律
//!   `ERR_REQUIRE_ESM`；其余（`commonjs`/缺省/无清单）强制 CJS（ESM 语法自然
//!   报 SyntaxError，不再走嗅探误判）。
//! - 调用传参用柯里化 `call_one` 链（§4.9：native 内禁 `Rooted<ValueArray>`）。
//! - 入口 `.cjs` 经 prelude `__wjs_require_main` 起（`run` 不打印其 exports）。

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::jsapi::JSObject;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;
use mozjs::rust::{evaluate_script, CompileOptionsWrapper};
use url::Url;

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::jsapi_glue::{
    call_one, get_prop_value, parse_json, report_error, value_to_string, wrap_cx, Frame,
};
use crate::loader::load_js;
use crate::loader::resolve::resolve_require;
use crate::state;

// ── CJS 具名导出静态发现（cjs-module-lexer 核心子集；零求值）──────────────
//
// require(esm)（Node ≥22.12）落地后，M5 的运行时键快照（求值取键）在
// CJS↔ESM 环上会无限递归：发现期求值 CJS → require ESM → 编译其子图 →
// 环上 CJS 再进发现期。正解同 Node cjs-module-lexer：静态词法为主（零求值、
// 循环天然安全），运行时快照降级为回退并加在飞护栏。
//
// 静态面（近似口径，偏差记本节）：`exports.NAME =` / `module.exports.NAME =`
// （标识符键）、`exports["NAME"] =`、`Object.defineProperty(exports, "NAME", …)`、
// `module.exports = require("<相对>")` 与 `__exportStar(require("<相对>"), exports)`
// 转出跟随（深度 ≤ 8，路径集合去重；多分支 if/else 取并集）。
// `__esModule` 互操作标记剔除。

fn cjs_static_names(path: &Path, depth: usize, seen: &mut HashSet<PathBuf>) -> Vec<String> {
    if depth > 8 || !seen.insert(path.to_path_buf()) {
        return Vec::new();
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut names: Vec<String> = Vec::new();
    let mut push = |n: &str| {
        if n != "__esModule" && n != "default" && !names.iter().any(|x| x == n) {
            names.push(n.to_owned());
        }
    };
    // exports.NAME = … / module.exports.NAME = …（链式 `exports.a = exports.b =`
    // 全捕获；`=[^=]` 排除 ==/===；无行锚——`(0 && (exports.x = …))` 死代码形
    // 也是真导出名）。babel 的 `exports.version = exports.types = void 0` 落此。
    for caps in regex::Regex::new(r#"(?:module\.)?exports\.([A-Za-z_$][\w$]*)\s*=[^=]"#)
        .unwrap()
        .captures_iter(&text)
    {
        push(&caps[1]);
    }
    // exports["NAME"] = …
    for caps in regex::Regex::new(r#"(?:module\.)?exports\[["']([^"']+)["']\]\s*=[^=]"#)
        .unwrap()
        .captures_iter(&text)
    {
        push(&caps[1]);
    }
    // Object.defineProperty(exports|module.exports|(0, exports), "NAME", …)
    // babel 系 TS 编译产物的 `(0, exports)` 接收器同捕获。
    for caps in regex::Regex::new(
        r#"Object\.defineProperty\(\s*(?:\(0,\s*)?(?:module\.)?exports\s*,\s*["']([^"']+)["']"#,
    )
    .unwrap()
    .captures_iter(&text)
    {
        push(&caps[1]);
    }
    // require 绑定表（var/const/let X = require("spec")）：Object.keys(X).forEach
    // 动态转出的跟随依据。裸说明符经本仓 resolver 解析（真机 vue.cjs.js 的
    // Object.keys(runtimeDom).forEach 即此形，实测 173 键全出）。
    let mut bindings: Vec<(String, String)> = Vec::new();
    for caps in regex::Regex::new(
        r#"(?:var|const|let)\s+([A-Za-z_$][\w$]*)\s*=\s*require\(\s*['"]([^'"]+)['"]\s*\)"#,
    )
    .unwrap()
    .captures_iter(&text)
    {
        bindings.push((caps[1].to_owned(), caps[2].to_owned()));
    }
    // 整包转出跟随：module.exports = require("rel") / Object.keys(X|require(..)).forEach
    // / __exportStar(require("rel"), exports)。多分支取并集。
    let dir = path.parent().map(|d| d.to_path_buf()).unwrap_or_default();
    let mut follow_spec = |spec: &str, depth: usize, seen: &mut HashSet<PathBuf>| {
        if spec.starts_with("./") || spec.starts_with("../") {
            let resolved = cjs_follow_spec(&dir, spec);
            for name in cjs_static_names(&resolved, depth + 1, seen) {
                push(&name);
            }
            return;
        }
        // 裸说明符：base = 本文件 URL，走本仓 resolver（package exports 感知）。
        let Ok(base) = Url::from_file_path(path) else {
            return;
        };
        if let Ok(u) = resolve_require(spec, Some(&base)) {
            if let Ok(p) = u.to_file_path() {
                for name in cjs_static_names(&p, depth + 1, seen) {
                    push(&name);
                }
            }
        }
    };
    for caps in regex::Regex::new(r#"module\.exports\s*=\s*require\(\s*['"]([^'"]+)['"]\s*\)"#)
        .unwrap()
        .captures_iter(&text)
    {
        follow_spec(&caps[1], depth, seen);
    }
    for caps in regex::Regex::new(
        r#"Object\.keys\(\s*require\(\s*['"]([^'"]+)['"]\s*\)\s*\)\.forEach"#,
    )
    .unwrap()
    .captures_iter(&text)
    {
        follow_spec(&caps[1], depth, seen);
    }
    for caps in regex::Regex::new(r#"Object\.keys\(\s*([A-Za-z_$][\w$]*)\s*\)\.forEach"#)
        .unwrap()
        .captures_iter(&text)
    {
        if let Some((_, spec)) = bindings.iter().find(|(n, _)| n == &caps[1]) {
            follow_spec(spec, depth, seen);
        }
    }
    for caps in regex::Regex::new(
        r#"__exportStar\(\s*require\(\s*['"]([^'"]+)['"]\s*\)\s*,\s*(?:module\.)?exports\s*\)"#,
    )
    .unwrap()
    .captures_iter(&text)
    {
        let resolved = cjs_follow_spec(&dir, &caps[1]);
        for name in cjs_static_names(&resolved, depth + 1, seen) {
            push(&name);
        }
    }
    names
}

/// 转出跟随的相对说明符解析（仅 `./`/`../` 形；扩展名缺失探测 .js/.cjs/index 双形）。
fn cjs_follow_spec(dir: &Path, spec: &str) -> PathBuf {
    let rel = spec.trim_start_matches("./");
    if let Some(p) = dir.join(rel).canonicalize().ok() {
        return p;
    }
    for suffix in [".js", ".cjs", "/index.js", "/index.cjs"] {
        let cand = dir.join(format!("{rel}{suffix}"));
        if cand.is_file() {
            return cand;
        }
    }
    dir.join(rel)
}

// 发现期在飞护栏（递归环上重入即放弃，调用方退 default-only 垫片）。
thread_local! {
    static NAMES_INFLIGHT: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

/// 调用方 base（脚本文件名 → URL；eval/prelude 等非 URL 返回 None）。
/// 10f：入口经典脚本的 filename 是裸文件路径（非 file:// URL，见 runtime 入口
/// 求值），此处回落文件路径 → file URL，否则 CJS 入口的相对 require 全挂。
fn caller_base(cx: &mozjs::context::JSContext) -> Option<Url> {
    let caller = mozjs::rust::describe_scripted_caller(cx).ok()?;
    tracing::debug!(target: "winterjs::require", caller = caller.filename, "scripted caller");
    if let Ok(url) = Url::parse(&caller.filename) {
        return match url.scheme() {
            "file" | "node" => Some(url),
            _ => None,
        };
    }
    let path = std::path::Path::new(&caller.filename);
    if path.is_absolute() {
        return Url::from_file_path(path).ok();
    }
    None
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
    // §4.40/§4.68 教训的 require 版：链内每次 call_one 都可能 GC（curried 闭包
    // 分配 + wrapper 体嵌套 require），cur 与各实参一律全程 rooted、调用点现读
    // 现传——裸 JSVal 栈拷贝在 GC 搬移后即悬垂（css-tree 深图 worker 线程
    // SIGBUS 实锤：cur/arg 位型 0xFFF8/0x5800 垃圾，require 条件修复后 CJS
    // 图暴增才把它养出来）。
    rooted!(&in(cx) let mut cur = fn_v.get());
    rooted!(&in(cx) let exports_root = exports_v);
    rooted!(&in(cx) let require_root = require_v);
    rooted!(&in(cx) let module_val_root = module_v);
    rooted!(&in(cx) let mut s_v = UndefinedValue());
    filename_s.to_jsval(cx, s_v.handle_mut());
    rooted!(&in(cx) let mut d_v = UndefinedValue());
    dirname_s.to_jsval(cx, d_v.handle_mut());
    let mut call_arg = |label: &str, arg: JSVal| -> Result<JSVal, String> {
        if !cur.get().is_object() {
            state::cjs_remove(url.as_str());
            return Err(format!("cannot load '{}': {label} is not callable", url.as_str()));
        }
        match call_one(cx, global_root.get(), cur.get(), arg) {
            Some(v) => {
                cur.set(v);
                Ok(v)
            }
            None => {
                state::cjs_remove(url.as_str());
                Err(pending_message(cx))
            }
        }
    };
    call_arg("exports", exports_root.get())?;
    call_arg("require", require_root.get())?;
    call_arg("module", module_val_root.get())?;
    call_arg("__filename", s_v.get())?;
    call_arg("__dirname", d_v.get())?;
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

/// 最近 `package.json` 的 `type` 字段（`module`/`commonjs`/缺省）。
/// 纯 fs + 宽容 JSON：坏文件/无清单一律当缺省（`None`）；找到最近一份即停。
/// 纯函数（除 fs 外），单测覆盖判定表（用 tempfile 搭清单树）。
/// `pub(crate)`：入口 ESM 判定（`runtime::sniff_module`）复用同一口径。
pub(crate) fn nearest_pkg_type(path: &std::path::Path) -> Option<String> {
    let mut dir = path.parent();
    while let Some(d) = dir {
        let cand = d.join("package.json");
        if let Ok(text) = std::fs::read_to_string(&cand) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                return v.get("type").and_then(|t| t.as_str()).map(|s| s.to_string());
            }
            return None;
        }
        dir = d.parent();
    }
    None
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
    let url = resolve_require(spec, base.as_ref()).map_err(|e| {
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
            // napi（`.node` 二进制，plan-napi M0）：权限门控（复用 --allow-ffi）
            // → dlopen + register → exports（9j 的可读拒错退役）。
            if ext == "node" {
                return crate::napi::load(cx, global, spec, &path).map_err(Error::Other);
            }
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
                // require(esm)（Node ≥22.12，26 无条件）：type==module 的 .js
                // 或带模块语法的文件走同步求值返回 namespace；其余强制 CJS
                // （ESM 语法在包装执行期自然报 SyntaxError）。
                let type_module = ext == "js" && nearest_pkg_type(&path).as_deref() == Some("module");
                let loaded = load_js(&text, url.as_str(), &path).map_err(|e| {
                    Error::Other(format!("Cannot load '{spec}' ({e})"))
                })?;
                if type_module || loaded.is_module {
                    return crate::modules::require_esm(cx, &url);
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
        resolve_require(&spec, base.as_ref())
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
globalThis.__wjs_make_base_require = (base) => (id) => __wjs_require_from(base, String(id));
globalThis.__wjs_require_main = (url) => globalThis.require(String(url));
// 直挂原生（禁 JS 闭包包装）：describe_scripted_caller 的最内层帧须是调用方
// 文件——闭包帧（本 prelude）会盖掉它，相对 require.resolve 即丢 base
// （jsdom api.js 实测：caller=__wjs_node_prelude.js）。
globalThis.require.resolve = __wjs_require_resolve;
Object.defineProperty(globalThis.require, "main", {
  configurable: true,
  get() {
    const u = __wjs_require_main_url();
    if (u === undefined) return undefined;
    return { filename: u, id: u, paths: [] };
  },
});
"#;

/// 显式 base 解析（`createRequire(filename)` 用；`file:` URL 或路径，
/// 相对路径按 cwd 拼；非法 base 即错，不回落调用方）。
fn explicit_base(base_s: &str) -> Result<Url, String> {
    if let Ok(u) = Url::parse(base_s)
        && u.scheme() == "file"
    {
        return Ok(u);
    }
    let path = std::path::PathBuf::from(base_s);
    let abs = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map_err(|e| format!("cannot get cwd: {e}"))?
            .join(path)
    };
    Url::from_file_path(&abs).map_err(|_| format!("bad require base '{base_s}'"))
}

/// UNSAFE-BOUNDARY: `__wjs_require_from(base, id)` → `createRequire` 底座，
/// 显式 base 复用 `require_value`（调用方定位/JSON/CJS/ESM 口径与全局 `require`
/// 完全一致）；前置：两参皆字符串（非串即 TypeError，不读值）；
/// 覆盖：`tests/node.rs::phase9j_module_create_require`。
pub unsafe extern "C" fn require_from(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 || !frame.arg(0).is_string() || !frame.arg(1).is_string() {
        report_error(&mut cx, "TypeError: __wjs_require_from needs (base, id) strings");
        return false;
    }
    let base_s = value_to_string(&mut cx, frame.arg(0));
    let spec = value_to_string(&mut cx, frame.arg(1));
    let global = state::global();
    rooted!(&in(cx) let global_root: *mut JSObject = global);
    let base = match explicit_base(&base_s) {
        Ok(u) => u,
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: {e}"));
            return false;
        }
    };
    match require_value(&mut cx, global_root.get(), &spec, Some(base)) {
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

/// UNSAFE-BOUNDARY: `__wjs_require_resolve_from(base, id)` → 解析后 URL 串
/// （`createRequire().resolve` 用；同显式 base 规则）；
/// 前置同上；覆盖：`tests/node.rs::phase9j_module_create_require`。
pub unsafe extern "C" fn require_resolve_from(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 || !frame.arg(0).is_string() || !frame.arg(1).is_string() {
        report_error(&mut cx, "TypeError: __wjs_require_resolve_from needs (base, id) strings");
        return false;
    }
    let base_s = value_to_string(&mut cx, frame.arg(0));
    let spec = value_to_string(&mut cx, frame.arg(1));
    let base = match explicit_base(&base_s) {
        Ok(u) => u,
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: {e}"));
            return false;
        }
    };
    let url = if let Some(canonical) = crate::builtins::node::normalize_spec(&spec) {
        Url::parse(canonical).map_err(|e| Error::Other(format!("bad builtin URL: {e}")))
    } else {
        resolve_require(&spec, Some(&base))
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

/// UNSAFE-BOUNDARY: `__wjs_cjs_compile(module, code, filename)` → `module._compile`
/// 底座（vite loadConfigFromBundledFile：require.extensions 钩子把内存中的 CJS
/// 打包产物求值进给定 module 对象）。求值口径与 `require_cjs_file` 全同
/// （柯里化包装五连：exports/require/module/__filename/__dirname），差异：
/// module 由调用方传入、require 以 filename 为显式 base（prelude
/// `__wjs_make_base_require`）、不进 cjs 注册表（缓存语义由调用方
/// require.cache 承载）。前置：module 对象、code 串、filename 为绝对路径或
/// file: URL 串；覆盖：`tests/node.rs::phase9k_module_extensions_hook`。
pub unsafe extern "C" fn cjs_compile(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3
        || !frame.arg(0).is_object()
        || !frame.arg(1).is_string()
        || !frame.arg(2).is_string()
    {
        report_error(
            &mut cx,
            "TypeError: __wjs_cjs_compile needs (module object, code string, filename string)",
        );
        return false;
    }
    let code = value_to_string(&mut cx, frame.arg(1));
    let filename_s = value_to_string(&mut cx, frame.arg(2));
    let global = state::global();
    rooted!(&in(cx) let global_root: *mut JSObject = global);
    rooted!(&in(cx) let module_root: *mut JSObject = frame.arg(0).to_object());
    // filename → URL（file: 直 parse；路径串 canonicalize 对齐 §4.12，失败退原文）。
    let url = if let Ok(u) = Url::parse(&filename_s) {
        u
    } else {
        let p = std::path::PathBuf::from(&filename_s);
        let p = if p.is_absolute() {
            p
        } else {
            match std::env::current_dir() {
                Ok(d) => d.join(p),
                Err(e) => {
                    report_error(&mut cx, &format!("TypeError: cannot resolve '{filename_s}': {e}"));
                    return false;
                }
            }
        };
        let canon = p.canonicalize().unwrap_or(p);
        match Url::from_file_path(&canon) {
            Ok(u) => u,
            Err(_) => {
                report_error(&mut cx, &format!("TypeError: bad module filename '{filename_s}'"));
                return false;
            }
        }
    };
    // 柯里化包装（单参链，§4.9 合规；与 require_cjs_file 同形）。
    let wrapped = format!(
        "((exports) => (require) => (module) => (__filename) => (__dirname) => {{\n{code}\n}})"
    );
    let c_filename =
        std::ffi::CString::new(url.as_str()).unwrap_or_else(|_| c"module.js".into());
    let options = CompileOptionsWrapper::new(&mut cx, c_filename, 1);
    rooted!(&in(cx) let mut fn_v = UndefinedValue());
    let res = evaluate_script(&mut cx, global_root.handle(), wrapped.as_str(), fn_v.handle_mut(), options);
    if res.is_err() {
        let msg = pending_message(&mut cx);
        report_error(&mut cx, &msg);
        return false;
    }
    if !fn_v.is_object() {
        report_error(&mut cx, &format!("cannot compile '{}': wrapper failed", url.as_str()));
        return false;
    }
    let Some(exports_v) = get_prop_value(&mut cx, module_root.get(), c"exports") else {
        report_error(&mut cx, "TypeError: module.exports missing");
        return false;
    };
    rooted!(&in(cx) let exports_root = exports_v);
    let Some(make_req) = get_prop_value(&mut cx, global_root.get(), c"__wjs_make_base_require") else {
        report_error(&mut cx, "prelude helper __wjs_make_base_require missing");
        return false;
    };
    rooted!(&in(cx) let mut url_v = UndefinedValue());
    url.as_str().to_jsval(&mut cx, url_v.handle_mut());
    let Some(require_v) = call_one(&mut cx, global_root.get(), make_req, url_v.get()) else {
        let msg = pending_message(&mut cx);
        report_error(&mut cx, &msg);
        return false;
    };
    rooted!(&in(cx) let require_root = require_v);
    // __filename/__dirname（file: URL；其余退原文）。
    let (filename_str, dirname_str) = match url.to_file_path() {
        Ok(p) => {
            let dir = p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
            (p.to_string_lossy().into_owned(), dir)
        }
        Err(_) => (url.as_str().to_owned(), String::new()),
    };
    rooted!(&in(cx) let mut s_v = UndefinedValue());
    filename_str.to_jsval(&mut cx, s_v.handle_mut());
    rooted!(&in(cx) let mut d_v = UndefinedValue());
    dirname_str.to_jsval(&mut cx, d_v.handle_mut());
    // 五连单参调用（§4.9）。
    let mut cur = fn_v.get();
    let chain: [(&str, JSVal); 5] = [
        ("exports", exports_root.get()),
        ("require", require_root.get()),
        ("module", mozjs::jsval::ObjectValue(module_root.get())),
        ("__filename", s_v.get()),
        ("__dirname", d_v.get()),
    ];
    for (label, arg) in chain {
        if !cur.is_object() {
            report_error(&mut cx, &format!("cannot compile '{}': {label} step is not callable", url.as_str()));
            return false;
        }
        match call_one(&mut cx, global_root.get(), cur, arg) {
            Some(v) => cur = v,
            None => {
                let msg = pending_message(&mut cx);
                report_error(&mut cx, &msg);
                return false;
            }
        }
    }
    frame.set_rval(UndefinedValue());
    true
}

/// UNSAFE-BOUNDARY: `__wjs_builtin_modules()` → JSON 数组（`node:module` 的
/// `builtinModules`/`isBuiltin` 用；裸名 + `node:` 双形，与 `available()` 同源，
/// 天然不漂移）；前置：无参；覆盖：`tests/node.rs::phase9j_module_surface`。
pub unsafe extern "C" fn builtin_modules_json(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let mut names: Vec<&str> = Vec::new();
    for canonical in crate::builtins::node::available() {
        names.push(canonical.strip_prefix("node:").unwrap_or(canonical));
        names.push(canonical);
    }
    let json = serde_json::to_string(&names).unwrap_or_else(|_| "[]".into());
    rooted!(&in(cx) let mut v = UndefinedValue());
    json.to_jsval(&mut cx, v.handle_mut());
    frame.set_rval(v.get());
    true
}
/// CJS 互操作命名导出发现（M5 vitest 牵引，供 `modules.rs` 垫片静态列名）。
/// 编译期同步 require 一次取运行时键（`Object.keys` 实测；`__exportStar` 合并键
/// 天然在内，静态词法分析做不到这点）；注册表缓存使求值期复用零重跑。
/// best-effort：失败（循环中体/抛错体）即回空 → 调用方退 default-only 垫片，
/// 求值期 require 走原语义（抛错/半成品照旧；抛错体在发现期跑过一次、求值期
/// 再跑一次，双跑记档）。
/// 前置：cx 在 realm 内；`url` 为 file: CJS（调用方已判 `cjs_interop`）。
pub(crate) fn cjs_export_names(
    cx: &mut mozjs::context::JSContext,
    global: *mut JSObject,
    url: &Url,
) -> Vec<String> {
    // 静态优先（cjs-module-lexer 口径，零求值——require(esm) 循环安全）。
    if let Ok(path) = url.to_file_path() {
        let mut seen = HashSet::new();
        let names = cjs_static_names(&path, 0, &mut seen);
        if !names.is_empty() {
            return names;
        }
    }
    // 运行时键快照回退（__exportStar 合并键等动态形；在飞护栏防 CJS↔ESM 环
    // 递归——重入即放弃，调用方退 default-only 垫片）。
    let reentered = NAMES_INFLIGHT
        .with(|s| !s.borrow_mut().insert(url.as_str().to_owned()));
    if reentered {
        return Vec::new();
    }
    let out = cjs_export_names_runtime(cx, global, url);
    NAMES_INFLIGHT.with(|s| {
        s.borrow_mut().remove(url.as_str());
    });
    out
}

fn cjs_export_names_runtime(
    cx: &mut mozjs::context::JSContext,
    global: *mut JSObject,
    url: &Url,
) -> Vec<String> {
    let exports = match require_value(cx, global, url.as_str(), None) {
        Ok(v) if v.is_object() => v,
        _ => return Vec::new(),
    };
    rooted!(&in(cx) let exports_root = exports);
    rooted!(&in(cx) let global_root: *mut JSObject = global);
    // Object.keys(exports)（glue 直调，无新 JSAPI 面）。
    let keys_fn = match get_prop_value(cx, global_root.get(), c"Object")
        .filter(|o| o.is_object())
        .and_then(|o| get_prop_value(cx, o.to_object(), c"keys"))
    {
        Some(f) => f,
        _ => return Vec::new(),
    };
    let arr = match call_one(cx, global_root.get(), keys_fn, exports_root.get()) {
        Some(v) if v.is_object() => v,
        _ => return Vec::new(),
    };
    // JSON.stringify(keys) → Rust 侧解析（`parse_json` 另有落值用途，此处只要串）。
    let str_fn = match get_prop_value(cx, global_root.get(), c"JSON")
        .filter(|o| o.is_object())
        .and_then(|o| get_prop_value(cx, o.to_object(), c"stringify"))
    {
        Some(f) => f,
        _ => return Vec::new(),
    };
    let arr_rooted = arr;
    rooted!(&in(cx) let arr_root = arr_rooted);
    let json = match call_one(cx, global_root.get(), str_fn, arr_root.get()) {
        Some(v) => value_to_string(cx, v),
        _ => return Vec::new(),
    };
    serde_json::from_str::<Vec<String>>(&json).unwrap_or_default()
}
/// UNSAFE-BOUNDARY: `__wjs_require_cjs_by_url(url)` → CJS 互操作垫片底座
/// （`import` 命中 CJS 文件时合成 `export default` + 命名导出；复用
/// `require_value` 全口径：注册表命中则同值、CJS 循环见半成品；垫片求值期
/// 同步执行 CJS 体——编译期发现已跑过则缓存复用）。
/// 前置：单参为 file: URL 串（非法即抛错，不回落）；
/// 覆盖：`tests/node.rs::phase9j_cjs_interop_default`。
pub unsafe extern "C" fn require_cjs_by_url(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 || !frame.arg(0).is_string() {
        report_error(&mut cx, "TypeError: __wjs_require_cjs_by_url needs a file URL string");
        return false;
    }
    let spec = value_to_string(&mut cx, frame.arg(0));
    let global = state::global();
    rooted!(&in(cx) let global_root: *mut JSObject = global);
    // 绝对 file: URL 不依赖调用方 base（垫片求值点的调用方是垫片自身）。
    match require_value(&mut cx, global_root.get(), &spec, None) {
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

#[cfg(test)]
mod tests {
    use super::nearest_pkg_type;

    #[test]
    fn nearest_pkg_type_table() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub/deep");
        std::fs::create_dir_all(&sub).unwrap();
        let f = sub.join("a.js");
        // 无清单 → 缺省
        assert_eq!(nearest_pkg_type(&f), None);
        // 最近清单赢（子目录覆盖根）
        std::fs::write(dir.path().join("package.json"), r#"{"type":"commonjs"}"#).unwrap();
        std::fs::write(sub.join("package.json"), r#"{"type":"module"}"#).unwrap();
        assert_eq!(nearest_pkg_type(&f).as_deref(), Some("module"));
        assert_eq!(
            nearest_pkg_type(&dir.path().join("b.js")).as_deref(),
            Some("commonjs")
        );
        // 坏 JSON 当缺省且不再上找（最近清单即决）
        std::fs::write(sub.join("package.json"), r#"{"type": "#).unwrap();
        assert_eq!(nearest_pkg_type(&f), None);
    }
}
