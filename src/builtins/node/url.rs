//! `node:url`（Node `lib/url.js` 部分面，MIT；plan 9j：vite 顶层 import）。
//!
//! 忠实面（真机 node 26.8.2 逐项对过码与文案）：`URL`/`URLSearchParams`
//! （全局重导出）、`fileURLToPath`（string｜URL → posix 路径；类型错
//! `ERR_INVALID_ARG_TYPE`、串解不出 URL 即 `ERR_INVALID_URL`、非 file scheme
//! 即 `ERR_INVALID_URL_SCHEME`、host 非空非 localhost 即
//! `ERR_INVALID_FILE_URL_HOST`）、`pathToFileURL`（相对按 `process.cwd()` 解）。
//! 10a：legacy 面（`parse/format/resolve/resolveObject` + `Url` 类 +
//! `domainToASCII/domainToUnicode` + `urlToHttpOptions`，lib/url.js 口径移植）。
//!
//! 10f 对拍：parse/format/getHostname/parseHost/autoEscapeStr 全按 node 原文
//! 直译（首尾修剪扫描器/nonHost 主机扫描/fast-path 正则/三态警告前置的
//! ERR_INVALID_URL 主机校验），WHATWG `format(url, options)` 走 JS 剥离口径。
//! 偏差（记档）：win32 盘符只做 `/X:/` 前导剥离；resolveObject 非 WHATWG 近似。

/// 内嵌 ESM 源（§0.9 按域分块：`url_file.js` file-URL 双向 + `url_legacy.js`
/// legacy parse/format/Url，concat 字节恒等）。
pub const SOURCE: &str = concat!(
    include_str!("url_file.js"),
    include_str!("url_legacy.js"),
);
