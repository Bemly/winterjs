//! ESM loader 纯 Rust 层：resolve → fetch → 转译（无 JSAPI，可单元测）。
//! JS 侧编排（hook/registry/link/evaluate）见 `crate::modules`。

pub mod fetch;
pub mod resolve;
pub mod transpile;

pub use transpile::load_js;
