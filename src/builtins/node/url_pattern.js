// `node:url` URLPattern 面（plan3 §5 专项）：别名 prelude 全局真类
// （`url_file.js` 同款 `URL_` 别名口径；本文件置 concat 首位，default
// 对象字面量求值时绑定已就绪，无 TDZ）。
const URLPattern_ = globalThis.URLPattern;
