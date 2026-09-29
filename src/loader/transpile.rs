//! oxc 解析 + TS 转译 + 静态 import 提取（纯 Rust，无 JSAPI）。
//!
//! - `.ts/.mts/.cts/.tsx`：转译后喂引擎（specifier 保持原样，解析走自家 hook，无需重写后缀）。
//! - `.js/.mjs`：原样过，只解析（提 imports + 嗅探 ESM）。
//! - 解析失败 → `Error::script`（首个诊断，行列由 span 换算，miette 渲染）。

use std::path::Path;

use oxc::{
    allocator::Allocator,
    codegen::Codegen,
    parser::Parser,
    semantic::SemanticBuilder,
    span::SourceType,
    transformer::{TransformOptions, Transformer},
};

use crate::error::Error;

pub struct LoadedSource {
    /// 可直接喂给 CompileModule 的 JS。
    pub js: String,
    /// 静态 import/export-from 的 specifier（type-only 已过滤）。
    pub imports: Vec<String>,
    /// 是否走模块求值（import/export、`import.meta`、动态 `import()` 任一）。
    pub is_module: bool,
    /// TS 转译的 sourcemap JSON（JS 源为 None；报错回映射用）。
    pub map: Option<String>,
}

impl std::fmt::Debug for LoadedSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedSource")
            .field("js_bytes", &self.js.len())
            .field("imports", &self.imports)
            .field("is_module", &self.is_module)
            .finish()
    }
}

fn is_ts_like(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("ts") | Some("mts") | Some("cts") | Some("tsx")
    )
}

fn offset_to_line_col(text: &str, offset: usize) -> (u32, u32) {
    let offset = offset.min(text.len());
    let mut line = 1u32;
    let mut col = 1u32;
    for b in text.as_bytes().iter().take(offset) {
        if *b == b'\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

fn syntax_error(text: &str, filename: &str, message: String, offset: Option<usize>) -> Error {
    let (line, col) = offset.map(|o| offset_to_line_col(text, o)).unwrap_or((1, 1));
    Error::script(filename, text, line, col, message)
}

fn first_diagnostic(text: &str, filename: &str, diags: Vec<oxc::diagnostics::OxcDiagnostic>) -> Error {
    let d = &diags[0];
    let mut message = d.message.to_string();
    if let Some(help) = d.help.as_deref() {
        message.push_str("; ");
        message.push_str(help);
    }
    let offset = d.labels.first().map(|l| l.span().start as usize);
    syntax_error(text, filename, message, offset)
}

/// 解析 +（TS 系）转译。`filename` 只用于报错展示（取 URL 字符串）。
pub fn load_js(text: &str, filename: &str, path: &Path) -> Result<LoadedSource, Error> {
    load_js_goal(text, filename, path, false)
}

/// CJS goal 转译：node 的 CJS 恒函数包装执行，顶层 `return`/`arguments` 合法
/// （`SourceType::with_commonjs` 口径）；`load_js` 的 module goal 对此类文件
/// 必报 "return not in function"。CJS/ESM 分类由调用方 `cjs_interop` 定，
/// 缓存与 `load_js` 同键（同一文本分类确定，只走单一 goal）。
pub fn load_cjs_js(text: &str, filename: &str, path: &Path) -> Result<LoadedSource, Error> {
    load_js_goal(text, filename, path, true)
}

fn load_js_goal(text: &str, filename: &str, path: &Path, cjs_goal: bool) -> Result<LoadedSource, Error> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if let Some(hit) = super::cache::get(text, &ext) {
        return Ok(LoadedSource {
            js: hit.js,
            imports: hit.imports,
            is_module: hit.is_module,
            map: hit.map,
        });
    }
    let loaded = load_js_uncached(text, filename, path, cjs_goal)?;
    super::cache::put(
        text,
        &ext,
        &super::cache::Cached {
            js: loaded.js.clone(),
            imports: loaded.imports.clone(),
            is_module: loaded.is_module,
            map: loaded.map.clone(),
        },
    );
    Ok(loaded)
}

fn load_js_uncached(
    text: &str,
    filename: &str,
    path: &Path,
    cjs_goal: bool,
) -> Result<LoadedSource, Error> {
    let allocator = Allocator::default();
    // 无扩展名入口（`type: module` 包的 extensionless bin）按 `.js` 解析；
    // 模块/经典由调用方定（此处只定语法方言）。CJS goal 保 from_path 的
    // 方言（.jsx JSX、.ts TS）只把 kind 钉成 CommonJS（顶层 return 合法）。
    let source_type = SourceType::from_path(path)
        .or_else(|_| {
            if path.extension().is_none() {
                SourceType::from_path("module.js")
            } else {
                SourceType::from_path(path)
            }
        })
        .map_err(|_| Error::Other(format!("unsupported module extension: {}", path.display())))?;
    let source_type = if cjs_goal {
        source_type.with_commonjs(true)
    } else {
        source_type.with_module(true)
    };
    let ret = Parser::new(&allocator, text, source_type).parse();
    if ret.fatal_error {
        let diags = ret.diagnostics.into_vec();
        if diags.is_empty() {
            return Err(Error::Other(format!("failed to parse {filename}")));
        }
        return Err(first_diagnostic(text, filename, diags));
    }
    if ret.diagnostics.has_errors() {
        return Err(first_diagnostic(text, filename, ret.diagnostics.into_vec()));
    }
    let mut program = ret.program;

    // 静态依赖 + ESM 判定直接取解析器建好的模块记录（import.meta/动态 import 也算模块信号）。
    let mut imports = Vec::new();
    for (spec, reqs) in ret.module_record.requested_modules.iter() {
        // 全为 type-only 的请求会被擦除，不跟进抓取（否则 `import type 'x'` 误报缺失）
        if reqs.iter().all(|r| r.is_type) {
            continue;
        }
        imports.push(spec.to_string());
    }
    let is_module = ret.module_record.has_module_syntax
        || !ret.module_record.import_metas.is_empty()
        || !ret.module_record.dynamic_imports.is_empty();

    let (js, map) = if is_ts_like(path) {
        let scoping = SemanticBuilder::new().build(&program).semantic.into_scoping();
        let options = TransformOptions::default();
        let tret = Transformer::new(&allocator, path, &options)
            .build_with_scoping(scoping, &mut program);
        if tret.diagnostics.has_errors() {
            return Err(first_diagnostic(text, filename, tret.diagnostics.into_vec()));
        }
        let ret = Codegen::new()
            .with_options(oxc::codegen::CodegenOptions {
                source_map_path: Some(path.to_path_buf()),
                ..Default::default()
            })
            .build(&program);
        (ret.code, ret.map.map(|m| m.to_json_string()))
    } else {
        (text.to_owned(), None)
    };
    tracing::debug!(
        target: "winterjs2::loader",
        filename,
        is_module,
        deps = imports.len(),
        js_bytes = js.len(),
        has_map = map.is_some(),
        "source loaded"
    );
    Ok(LoadedSource { js, imports, is_module, map })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let dir = env!("CARGO_MANIFEST_DIR");
        std::fs::read_to_string(format!("{dir}/tests/fixtures/{name}")).unwrap()
    }

    #[test]
    fn snap_ts_transpile_output() {
        let ts = fixture("loader_sample.ts");
        let loaded = load_js_uncached(&ts, "loader_sample.ts", Path::new("loader_sample.ts"), false).unwrap();
        assert!(loaded.is_module);
        insta::assert_snapshot!(loaded.js);
    }

    #[test]
    fn snap_ts_import_extraction() {
        let ts = fixture("loader_sample.ts");
        let loaded = load_js_uncached(&ts, "loader_sample.ts", Path::new("loader_sample.ts"), false).unwrap();
        // type-only 整包请求被过滤（config.js/types.js 不在内），其余保留
        insta::assert_debug_snapshot!(loaded.imports);
    }

    #[test]
    fn snap_js_passthrough_meta_and_dynamic() {
        let js = "console.log(import.meta.url);\nconst m = await import(\"./lazy.js\");\n";
        let loaded = load_js_uncached(js, "m.js", Path::new("m.js"), false).unwrap();
        assert!(loaded.is_module);
        assert_eq!(loaded.js, js);
        insta::assert_debug_snapshot!(loaded.imports);
    }

    #[test]
    fn syntax_error_has_location() {
        let err = load_js_uncached("const = 1;\n", "bad.js", Path::new("bad.js"), false).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("bad.js:1:7"), "location: {msg}");
    }
}
