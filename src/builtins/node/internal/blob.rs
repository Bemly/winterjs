//! `node:internal/blob`——Blob re-export（全局类在 prelude）+ isBlob 判定。
/// 源：nodejs/node（MIT）对应 internal 件的对位实现（Node 版走
/// internalBinding('blob') native；本仓 Blob 在 prelude 纯 JS）；偏差记档。
pub const SOURCE: &str = r#"const Blob = globalThis.Blob;
function isBlob(value) {
  return typeof Blob !== 'undefined' && value instanceof Blob;
}
export { Blob, isBlob };
export default { Blob, isBlob };
"#;
