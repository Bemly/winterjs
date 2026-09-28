//! 本体命名空间外壳（prelude 分域；拼接顺序见 mod.rs）。
//! `globalThis.Deno`（冻结只读）+ `globalThis.Bun` / `globalThis.WinterJS`（普通可写）。
//! 首片只放外壳 + 只读版本元信息（读 `process.versions`，缺席回落常量）；
//! `Deno.*` / `Bun.*` 全量兼容面按域逐片跟进，不在本片展开。
pub const NAMESPACE_JS: &str = r#"
{
  const __wjs_ns_version = () => {
    try {
      const v = globalThis.process && globalThis.process.versions;
      if (v && typeof v.winterjs === "string") return v.winterjs;
    } catch {}
    return "26.9.27";
  };
  if (globalThis.Deno === undefined) {
    const __wjs_deno_version = () => {
      const w = __wjs_ns_version();
      return { deno: w, v8: "13.6", typescript: "5.9" };
    };
    globalThis.Deno = {
      get version() { return __wjs_deno_version(); },
      get args() {
        try {
          const a = globalThis.process && globalThis.process.argv;
          return Array.isArray(a) ? a.slice(2) : [];
        } catch { return []; }
      },
      get pid() {
        try { return globalThis.process ? globalThis.process.pid : 0; }
        catch { return 0; }
      },
      get mainModule() {
        try { return globalThis.process ? String(globalThis.process.argv[1] || "") : ""; }
        catch { return ""; }
      },
    };
    try {
      Object.defineProperty(globalThis.Deno, Symbol.toStringTag, { value: "Deno" });
    } catch {}
    try { Object.freeze(globalThis.Deno); } catch {}
  }
  if (globalThis.Bun === undefined) {
    let __wjs_bun_version, __wjs_bun_revision;
    globalThis.Bun = {
      get version() { return __wjs_bun_version !== undefined ? __wjs_bun_version : __wjs_ns_version(); },
      set version(v) { __wjs_bun_version = String(v); },
      get revision() { return __wjs_bun_revision !== undefined ? __wjs_bun_revision : __wjs_ns_version(); },
      set revision(v) { __wjs_bun_revision = String(v); },
      get argv() {
        try {
          const a = globalThis.process && globalThis.process.argv;
          return Array.isArray(a) ? a.slice() : [];
        } catch { return []; }
      },
      get main() {
        try { return globalThis.process ? String(globalThis.process.argv[1] || "") : ""; }
        catch { return ""; }
      },
    };
    try {
      Object.defineProperty(globalThis.Bun, Symbol.toStringTag, { value: "Bun" });
    } catch {}
  }
  if (globalThis.WinterJS === undefined) {
    let __wjs_winterjs_version;
    globalThis.WinterJS = {
      get version() { return __wjs_winterjs_version !== undefined ? __wjs_winterjs_version : __wjs_ns_version(); },
      set version(v) { __wjs_winterjs_version = String(v); },
      get versions() {
        try {
          const v = globalThis.process && globalThis.process.versions;
          if (v && typeof v === "object") return { ...v };
        } catch {}
        return { winterjs: __wjs_ns_version() };
      },
    };
    try {
      Object.defineProperty(globalThis.WinterJS, Symbol.toStringTag, { value: "WinterJS" });
    } catch {}
  }
}
"#;
