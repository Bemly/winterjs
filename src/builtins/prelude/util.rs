//! 本体工具 JS 面：`WinterJS.util`（`format`/`inspect` 复用 `node:util`
//! 移植实现——纯 JS 移植代码即底座，Deno/Bun 命名空间同款复用模式；
//! 用户侧经 `WinterJS.*` 到达，无需 `node:` 前缀）。
pub const UTIL_JS: &str = r#"
{
  const __wjs_util_mod = () => {
    try {
      const r = globalThis.require;
      if (typeof r !== "function") return null;
      return r("node:util");
    } catch { return null; }
  };
  const util = {
    format(...args) {
      const m = __wjs_util_mod();
      if (!m || typeof m.format !== "function") throw new Error("WinterJS.util.format requires node:util");
      return m.format(...args);
    },
    inspect(v, o) {
      const m = __wjs_util_mod();
      if (!m || typeof m.inspect !== "function") throw new Error("WinterJS.util.inspect requires node:util");
      return o === undefined ? m.inspect(v) : m.inspect(v, o);
    },
  };
  try {
    const W = globalThis.WinterJS;
    if (W && W.util === undefined) W.util = util;
  } catch {}
}
"#;
