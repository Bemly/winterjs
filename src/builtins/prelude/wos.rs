//! B5 系统门面 JS 面：`WinterJS2.os/path/db/inspect/tty`。
//!
//! `os/db/inspect` 直驱原生（JSON 桥）；`path` 复用移植实现（util 同款复用模式）。
//! `tty` 纯面（isTTY 判定；raw 模式是 CLI 领地，文档记录）。
pub const WOS_JS: &str = r#"
{
  const __wos_json = (s) => {
    try { return JSON.parse(s); } catch { return s; }
  };
  const __wos_req = (spec) => {
    try {
      const r = globalThis.require;
      if (typeof r !== "function") return null;
      return r(spec);
    } catch { return null; }
  };
  const os = {
    platform() { return __wjs2_os_platform(); },
    arch() { return __wjs2_os_arch(); },
    info() { return __wos_json(__wjs2_os_info()); },
    cpus() { return __wos_json(__wjs2_os_cpus()); },
    mem() { return __wos_json(__wjs2_os_mem()); },
    net() { return __wos_json(__wjs2_os_net()); },
    user() { return __wos_json(__wjs2_os_user()); },
    uptime() { return Number(__wjs2_os_uptime()); },
    load() { return __wos_json(__wjs2_os_load()); },
    locale() { return __wjs2_os_locale(); },
  };
  const db = {
    // open(path) → 同步 turso 库（node:sqlite 同底座；:memory: 可用）。
    open(path) {
      if (typeof path !== "string" || path === "") throw new TypeError("db.open requires a path");
      const id = Number(__wjs2_nsqlite_open(path));
      const h = {
        exec(sql) { __wjs2_nsqlite_exec(id, String(sql)); },
        run(sql, params) {
          return __wos_json(__wjs2_nsqlite_run(id, String(sql), JSON.stringify(params ?? [])));
        },
        query(sql, params) {
          return __wos_json(__wjs2_nsqlite_rows(id, String(sql), JSON.stringify(params ?? [])));
        },
        close() { try { __wjs2_nsqlite_close(id); } catch {} },
      };
      return h;
    },
  };
  const inspect = {
    // 同线程嵌套求值（inspector 会话口径；抛错透传）。
    evaluate(code) {
      if (typeof code !== "string") throw new TypeError("inspect.evaluate requires code");
      return __wjs2_inspector_eval(code);
    },
  };
  const tty = {
    // 纯判定面（raw 模式归 CLI；写端尺寸走 process.stdout 既有面）。
    isTTY(s) {
      const t = s === undefined ? null : s;
      try {
        const p = globalThis.process;
        const o = t ?? (p && (p.stdout === undefined ? null : p.stdout));
        if (o && typeof o.isTTY === "boolean") return o.isTTY;
        if (o && typeof o.isTTY === "function") return !!o.isTTY();
      } catch {}
      return false;
    },
  };
  // 预言求值期 process 尚不存在（4.230）：node:* 别名惰性 getter。
  const __wos_lazy = (W, key, fn) => {
    let cache = null, failed = false;
    try {
      Object.defineProperty(W, key, {
        get() {
          if (cache !== null) return cache;
          if (failed) return undefined;
          try { cache = fn(); } catch { failed = true; return undefined; }
          return cache;
        },
        configurable: true, enumerable: true,
      });
    } catch {}
  };
  try {
    const W = globalThis.WinterJS2;
    if (W) {
      if (W.os === undefined) W.os = os;
      __wos_lazy(W, "path", () => __wos_req("node:path"));
      if (W.db === undefined) W.db = db;
      if (W.inspect === undefined) W.inspect = inspect;
      if (W.tty === undefined) W.tty = tty;
    }
  } catch {}
}
"#;
