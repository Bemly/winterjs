//! `node:internal/registry`——CJS 循环依赖的调用期解析表（9b）。
//! 被懒 require 的模块（如 duplex⇄duplexify 类继承环）在求值末尾自注册，
//! 调用方运行时从表取 default 导出（此时图已求值完毕）。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
const registry = new Map();
export default registry;
export { registry };
"#;
