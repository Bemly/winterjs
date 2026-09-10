//! loader 热路径基线（直接调 oxc/oxc_resolver；glue 层薄，见 `src/loader/`）。
//!
//! 跑：`cargo bench`（基线存 `target/criterion`，不入库；退化即红）。

use std::path::PathBuf;

use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;
use oxc::{
    allocator::Allocator,
    codegen::Codegen,
    parser::Parser,
    semantic::SemanticBuilder,
    span::SourceType,
    transformer::{TransformOptions, Transformer},
};

fn fixture_ts() -> String {
    let dir = env!("CARGO_MANIFEST_DIR");
    std::fs::read_to_string(format!("{dir}/tests/fixtures/loader_sample.ts")).unwrap()
}

/// 镜像 `transpile.rs` 的 TS 管线（parse → semantic → transform → codegen）。
fn bench_transpile(c: &mut Criterion) {
    let ts = fixture_ts();
    let path = PathBuf::from("loader_sample.ts");
    c.bench_function("loader/transpile_ts", |b| {
        b.iter(|| {
            let allocator = Allocator::default();
            let source_type = SourceType::from_path(&path).unwrap().with_module(true);
            let ret = Parser::new(&allocator, black_box(&ts), source_type).parse();
            assert!(!ret.fatal_error);
            let mut program = ret.program;
            let scoping = SemanticBuilder::new().build(&program).semantic.into_scoping();
            let options = TransformOptions::default();
            let tret = Transformer::new(&allocator, &path, &options)
                .build_with_scoping(scoping, &mut program);
            assert!(!tret.diagnostics.has_errors());
            let js = Codegen::new().build(&program).code;
            black_box(js);
        });
    });
}

/// resolver 裸导入（temp 小包；含一次 tsconfig 自动发现）。
fn bench_resolve(c: &mut Criterion) {
    let dir = std::env::temp_dir().join("winterjs-bench-resolve");
    std::fs::create_dir_all(dir.join("node_modules/pkg")).unwrap();
    std::fs::write(dir.join("node_modules/pkg/package.json"), "{\"main\":\"index.js\"}").unwrap();
    std::fs::write(dir.join("node_modules/pkg/index.js"), "export default 1;\n").unwrap();
    std::fs::write(dir.join("app.js"), "import x from \"pkg\";\n").unwrap();
    let app = dir.join("app.js");
    let resolver = oxc_resolver::Resolver::new(oxc_resolver::ResolveOptions {
        extensions: [".ts", ".js", ".mjs"].iter().map(|s| s.to_string()).collect(),
        condition_names: vec!["node".into(), "import".into()],
        ..oxc_resolver::ResolveOptions::default()
    });
    c.bench_function("loader/resolve_bare", |b| {
        b.iter(|| {
            let r = resolver.resolve_file(black_box(&app), black_box("pkg")).unwrap();
            black_box(r.full_path());
        });
    });
}

criterion_group!(benches, bench_transpile, bench_resolve);
criterion_main!(benches);
