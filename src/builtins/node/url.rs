//! `node:url`（Node `lib/url.js` 部分面，MIT；plan 9j：vite 顶层 import）。
//!
//! 忠实面（真机 node 26.8.2 逐项对过码与文案）：`URL`/`URLSearchParams`
//! （全局重导出）、`fileURLToPath`（string｜URL → posix 路径；类型错
//! `ERR_INVALID_ARG_TYPE`、串解不出 URL 即 `ERR_INVALID_URL`、非 file scheme
//! 即 `ERR_INVALID_URL_SCHEME`、host 非空非 localhost 即
//! `ERR_INVALID_FILE_URL_HOST`）、`pathToFileURL`（相对按 `process.cwd()` 解）。
//!
//! 偏差（记档）：legacy `url.parse/format/resolve` 不导出（vite 内 follow-redirects
//! 走 `useNativeURL` 分支，恒为真）；`domainToASCII/domainToUnicode`、
//! `urlToHttpOptions`、`Url` legacy 类不导出；win32 盘符只做 `/X:/` 前导剥离。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node lib/url.js (partial face; see module docs for deviations).
import { resolve as resolvePath } from 'node:path';
import errors from 'node:internal/errors';

const {
  codes: {
    ERR_INVALID_ARG_TYPE,
    ERR_INVALID_FILE_URL_HOST,
    ERR_INVALID_URL,
    ERR_INVALID_URL_SCHEME,
  },
} = errors;

const URL_ = globalThis.URL;
const URLSearchParams_ = globalThis.URLSearchParams;

function fileURLToPath(path) {
  if (typeof path === 'string') {
    try {
      path = new URL_(path);
    } catch {
      throw new ERR_INVALID_URL(path);
    }
  } else if (!(path instanceof URL_)) {
    throw new ERR_INVALID_ARG_TYPE('path', ['string', 'URL'], path);
  }
  if (path.protocol !== 'file:') {
    throw new ERR_INVALID_URL_SCHEME('file');
  }
  if (path.host !== '' && path.host !== 'localhost') {
    throw new ERR_INVALID_FILE_URL_HOST(process.platform);
  }
  let pathname;
  try {
    pathname = decodeURIComponent(path.pathname);
  } catch {
    throw new ERR_INVALID_URL(path.href);
  }
  // win32 盘符前导剥离（/C:/x → C:/x；posix 原样，偏差记档）。
  if (/^\/[A-Za-z]:\//.test(pathname)) {
    pathname = pathname.slice(1);
  }
  return pathname;
}

function pathToFileURL(path) {
  if (typeof path !== 'string') {
    throw new ERR_INVALID_ARG_TYPE('path', ['string'], path);
  }
  const abs = resolvePath(process.cwd(), path);
  const encoded = abs.split('/').map((seg) => encodeURIComponent(seg)).join('/');
  return new URL_('file://' + encoded);
}

export { URL_ as URL, URLSearchParams_ as URLSearchParams, fileURLToPath, pathToFileURL };
export default { URL: URL_, URLSearchParams: URLSearchParams_, fileURLToPath, pathToFileURL };
"#;
