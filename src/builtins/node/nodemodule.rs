//! `node:module`（Node `lib/module.js` 最小桥，MIT；plan 9j：解 vite `-r dev` 挡路）。
//!
//! 忠实面：`createRequire(filename)`（path｜file URL｜URL 对象 → 显式 base 的
//! require，`require.resolve` 同规则）/`createRequireFromPath`/`builtinModules`
//! （裸名 + `node:` 双形，与 `available()` 同源不漂移）/`isBuiltin`/
//! `Module.createRequire`/`Module.builtinModules`/`Module.syncBuiltinESMExports`
//! （无操作）/`Module.prototype.require`（以自身 filename 为 base）。
//!
//! 偏差（记档）：
//! - `require.cache` 为每 `createRequire` 独立表（键 = `require.resolve` 的
//!   URL 串；仅 extensions 钩子路径消费，native 路径缓存仍走 loader 注册表）。
//! - `require.extensions`：空表起步、`createRequire` 系 require 按消费——命中
//!   钩子走 `module._compile(code, filename)`（native `__wjs_cjs_compile`，
//!   CJS 包装口径与 require 全同；require 以文件自身为 base），`.js` 兜底同
//!   vite `loaderExt` 口径。vite 配置打包链（loadConfigFromBundledFile）依赖。
//!   全局 `require` 不消费（转译/加载走 loader）。
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
import { fileURLToPath } from 'node:url';

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

function extnameOf(p) {
  const clean = String(p).replace(/[?#].*$/, "");
  const slash = Math.max(clean.lastIndexOf("/"), clean.lastIndexOf("\\"));
  const dot = clean.lastIndexOf(".");
  return dot > slash ? clean.slice(dot) : "";
}

function makeRequire(base) {
  function require(id) {
    const spec = String(id);
    const hooks = require.extensions;
    if (hooks && typeof hooks === "object" && !Array.isArray(hooks)) {
      let resolved = null;
      try { resolved = __wjs_require_resolve_from(base, spec); } catch { resolved = null; }
      if (resolved !== null && resolved.startsWith("file://")) {
        if (Object.prototype.hasOwnProperty.call(require.cache, resolved)) {
          return require.cache[resolved].exports;
        }
        const ext = extnameOf(resolved);
        const has = (k) => Object.prototype.hasOwnProperty.call(hooks, k);
        const hook = has(ext) ? hooks[ext] : has(".js") ? hooks[".js"] : null;
        if (typeof hook === "function") {
          const fsPath = fileURLToPath(resolved);
          const mod = {
            id: fsPath,
            filename: fsPath,
            paths: [],
            exports: {},
            loaded: false,
            children: [],
            parent: null,
          };
          mod._compile = function (code, filenameArg) {
            __wjs_cjs_compile(this, String(code), filenameArg != null ? String(filenameArg) : fsPath);
          };
          require.cache[resolved] = mod;
          hook(mod, fsPath);
          mod.loaded = true;
          return mod.exports;
        }
      }
    }
    return __wjs_require_from(base, spec);
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
