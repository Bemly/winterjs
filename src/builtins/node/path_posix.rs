//! `node:path/posix`（Node `lib/path.js` posix 子路径，MIT；M5 vitest 牵引）。
//!
//! 本模块即 `node:path` 的 `posix` 对象本身（真机 `require('path').posix ===
//! require('path/posix')` 同一对象口径）；形态缺口（嵌套 `posix`/`win32` 自指、
//! `matchesGlob`/`toNamespacedPath`/`_makeLong`）随父模块记档，不另欠。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Re-export of the posix namespace from node:path (see module docs).
import { posix } from 'node:path';
export default posix;
export const { sep, delimiter, normalize, join, resolve, dirname, basename, extname, isAbsolute, relative, parse, format } = posix;
"#;
