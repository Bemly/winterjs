//! `node:module`（Node `lib/module.js` 最小桥，MIT；plan 9j：解 vite `-r dev` 挡路）。
//!
//! 忠实面：`createRequire(filename)`（path｜file URL｜URL 对象 → 显式 base 的
//! require，`require.resolve` 同规则）/`createRequireFromPath`/`builtinModules`
//! （裸名 + `node:` 双形，与 `available()` 同源不漂移）/`isBuiltin`/
//! `Module.createRequire`/`Module.builtinModules`/`Module.syncBuiltinESMExports`
//! （无操作）/`Module.prototype.require`（以自身 filename 为 base）。
//!
//! 偏差（记档）：
//! - `require.cache` 为每 `createRequire` 独立 `{}`（全局 require 本就没有共享
//!   CJS 缓存表，`state::cjs_*` 是 URL 注册表，不等价，不硬套）。
//! - `require.extensions` 为 `{}`（转译/加载走 loader，不走扩展处理器）。
//! - `Module.register()` 抛 `ERR_METHOD_NOT_IMPLEMENTED`（ESM loader 定制不支持）。
//! - `stripTypeScriptTypes` 不导出（TS 由 loader 原生处理，无需剥离）。
//! - `runMain`/`_load`/`_resolveFilename` 等下划线内部件不导出。
//! - 错误对象沿全局 `require` 口径（消息串，无 `MODULE_NOT_FOUND` code；
//!   参数校验用 `ERR_INVALID_ARG_TYPE` 真码）。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node lib/module.js (minimal bridge; see module docs for deviations).
import errors from 'node:internal/errors';

const {
  codes: {
    ERR_INVALID_ARG_TYPE,
    ERR_METHOD_NOT_IMPLEMENTED,
  },
} = errors;

function normalizeBase(filename) {
  if (filename instanceof URL) filename = filename.href;
  if (typeof filename !== 'string') {
    throw new ERR_INVALID_ARG_TYPE('filename', ['string', 'URL'], filename);
  }
  return filename;
}

function makeRequire(base) {
  function require(id) {
    return __wjs_require_from(base, String(id));
  }
  require.resolve = function resolve(id) {
    return __wjs_require_resolve_from(base, String(id));
  };
  require.cache = {};
  require.extensions = {};
  require.main = globalThis.require?.main;
  return require;
}

export function createRequire(filename) {
  return makeRequire(normalizeBase(filename));
}

export function createRequireFromPath(path) {
  if (typeof path !== 'string') {
    throw new ERR_INVALID_ARG_TYPE('path', ['string'], path);
  }
  return makeRequire(path);
}

export const builtinModules = JSON.parse(__wjs_builtin_modules());

export function isBuiltin(moduleName) {
  return builtinModules.includes(String(moduleName));
}

export class Module {
  constructor(id = '', parent) {
    this.id = String(id);
    this.filename = String(id);
    this.paths = [];
    this.exports = {};
    this.parent = parent ?? null;
    this.loaded = false;
    this.children = [];
  }
  require(id) {
    return __wjs_require_from(this.filename, String(id));
  }
  static createRequire(filename) {
    return createRequire(filename);
  }
  static createRequireFromPath(path) {
    return createRequireFromPath(path);
  }
  static get builtinModules() {
    return builtinModules;
  }
  static syncBuiltinESMExports() {
    return undefined;
  }
  static register() {
    throw new ERR_METHOD_NOT_IMPLEMENTED('Module.register');
  }
}

export default {
  createRequire,
  createRequireFromPath,
  builtinModules,
  isBuiltin,
  Module,
  syncBuiltinESMExports: Module.syncBuiltinESMExports,
};
"#;
