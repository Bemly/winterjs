//! `node:dns/promises`（Node `lib/dns/promises.js` 子路径；M5 vitest 牵引）。
//!
//! 本模块即 `node:dns` 的 `promises` 命名空间（lookup/resolve4/resolve6）；
//! 解析语义缺口随父模块记档。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Re-export of the promises namespace from node:dns (see module docs).
import { promises } from 'node:dns';
export default promises;
export const { lookup, resolve4, resolve6 } = promises;
"#;
