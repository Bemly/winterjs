//! `node:assert/strict`（Node `lib/assert/strict.js` 子路径；M5 vitest 牵引）。
//!
//! 本模块即 `node:assert` 的 `strict` 命名空间（真机 `require('assert').strict
//! === require('assert/strict')` 同一对象口径）；断言语义缺口随父模块记档。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Re-export of the strict namespace from node:assert (see module docs).
import { strict } from 'node:assert';
export default strict;
export const { ok, equal, deepEqual, fail, ifError, throws, rejects, doesNotThrow, doesNotReject, match, doesNotMatch } = strict;
"#;
