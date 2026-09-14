//! `node:path/win32`（Node `lib/path.js` win32 子路径，MIT；M5 vitest 牵引）。
//!
//! 本模块即 `node:path` 的 `win32` 对象本身；形态缺口随父模块记档
//! （见 `path_posix.rs`，同口径）。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Re-export of the win32 namespace from node:path (see module docs).
import { win32 } from 'node:path';
export default win32;
export const { sep, delimiter, normalize, join, resolve, dirname, basename, extname, isAbsolute, relative, parse, format } = win32;
"#;
