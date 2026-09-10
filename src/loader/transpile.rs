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
        });
    }
    let loaded = load_js_uncached(text, filename, path)?;
    super::cache::put(
        text,
        &ext,
        &super::cache::Cached {
            js: loaded.js.clone(),
            imports: loaded.imports.clone(),
            is_module: loaded.is_module,
            map: None,
        },
    );
    Ok(loaded)
}

fn load_js_uncached(text: &str, filename: &str, path: &Path) -> Result<LoadedSource, Error> {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(path)
        .map_err(|_| Error::Other(format!("unsupported module extension: {}", path.display())))?
        .with_module(true);
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

    let js = if is_ts_like(path) {
        let scoping = SemanticBuilder::new().build(&program).semantic.into_scoping();
        let options = TransformOptions::default();
        let tret = Transformer::new(&allocator, path, &options)
            .build_with_scoping(scoping, &mut program);
        if tret.diagnostics.has_errors() {
            return Err(first_diagnostic(text, filename, tret.diagnostics.into_vec()));
        }
        Codegen::new().build(&program).code
    } else {
        text.to_owned()
    };
    tracing::debug!(
        target: "winterjs::loader",
        filename,
        is_module,
        deps = imports.len(),
        js_bytes = js.len(),
        "source loaded"
    );
    Ok(LoadedSource { js, imports, is_module })
}
