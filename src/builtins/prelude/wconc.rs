//! B4 并发门面 JS 面：全局 `Worker`（Web 风）+ `WinterJS2.cluster/test/vm`。
//!
//! `Worker` 直驱 `__wjs2_worker_*` 原生（`__ev` 自有 sink；消息走 JSON 文 disciplined：
//! 发侧 `JSON.stringify`，收侧最小包络解码 + `JSON.parse`；与本仓 plain-data
//! structuredClone 口径一致）。`cluster/test` 复用移植实现（util 同款复用模式）。
//! `vm` 直驱 `__wjs2_vm_*`（completion 值透明回传）。
pub const WCONC_JS: &str = r#"
{
  // worker 消息包络解码（实测：parentPort.postMessage 经 __toWire 成
  // `{__wjs2_env, d}` 形；d 为 JSON 文即二次解析，否则原文直通。
  // 发侧一律 JSON 文 disciplined，与 plain-data structuredClone 口径一致）。
  const __wconc_unwrap = (payload) => {
    const msgErr = () => {
      const e = new Error("worker message is not valid JSON");
      e.name = "MessageError";
      return e;
    };
    let raw;
    try { raw = JSON.parse(String(payload)); }
    catch { throw msgErr(); }
    if (raw !== null && typeof raw === "object"
        && typeof raw.__wjs2_env === "string" && "d" in raw) {
      const d = raw.d;
      if (typeof d === "string") {
        try { return JSON.parse(d); }
        catch { return d; }
      }
      return d;
    }
    return raw;
  };
  class Worker {
    // new Worker(path | code, { eval?, name? })；worker 内经
    // `node:worker_threads` 的 parentPort 收发（发侧一律 JSON 文）。
    constructor(src, o) {
      if (typeof src !== "string" || src === "") throw new TypeError("Worker requires a file path or code string");
      const evalFlag = (o && o.eval === true) ? "1" : "";
      const name = (o && typeof o.name === "string") ? o.name : "";
      const ids = String(__wjs2_worker_spawn(src, evalFlag, "", name)).split(" ");
      this.__id = ids[0];
      this.__onmessage = null;
      this.__onmessageerror = null;
      this.__onerror = null;
      this.__done = false;
      this.__doneWaiters = [];
      const self = this;
      __wjs2_worker_attach(this.__id, {
        __ev(kind, payload) {
          try {
            if (kind === "message") {
              let data;
              try { data = __wconc_unwrap(payload); }
              catch (e) {
                if (self.__onmessageerror) { try { self.__onmessageerror({ data: undefined }); } catch {} }
                return;
              }
              if (self.__onmessage) { try { self.__onmessage({ data }); } catch {} }
            } else if (kind === "error") {
              let e;
              try { e = new Error(String(JSON.parse(String(payload)).message || payload)); }
              catch { e = new Error(String(payload)); }
              if (self.__onerror) { try { self.__onerror({ error: e }); } catch {} }
              self.__settleDone();
            } else {
              self.__settleDone();
            }
          } catch {}
        },
      });
    }
    __settleDone() {
      if (this.__done) return;
      this.__done = true;
      for (const w of this.__doneWaiters.splice(0)) { try { w(); } catch {} }
    }
    postMessage(data) {
      __wjs2_worker_post(this.__id, JSON.stringify(data));
    }
    terminate() {
      try { __wjs2_worker_terminate(this.__id); } catch {}
      this.__settleDone();
    }
    get onmessage() { return this.__onmessage; }
    set onmessage(fn) { this.__onmessage = (typeof fn === "function") ? fn : null; }
    get onmessageerror() { return this.__onmessageerror; }
    set onmessageerror(fn) { this.__onmessageerror = (typeof fn === "function") ? fn : null; }
    get onerror() { return this.__onerror; }
    set onerror(fn) { this.__onerror = (typeof fn === "function") ? fn : null; }
    get done() {
      if (this.__done) return Promise.resolve();
      return new Promise((r) => this.__doneWaiters.push(r));
    }
  }
  try {
    if (globalThis.Worker === undefined) globalThis.Worker = Worker;
  } catch {}
  const __wconc_req = (spec) => {
    try {
      const r = globalThis.require;
      if (typeof r !== "function") return null;
      return r(spec);
    } catch { return null; }
  };
  const __wconc_pick = (m, names) => {
    const o = {};
    for (const k of names) {
      try { if (m && typeof m[k] === "function") o[k] = m[k].bind(m); } catch {}
    }
    return o;
  };
  // 预言求值期 process 尚不存在（4.230）：node:* 别名一律惰性 getter，
  // 首次取用（用户代码期）才 require；失败即 undefined，不炸预言。
  const __wconc_lazy = (W, key, fn) => {
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
      __wconc_lazy(W, "cluster", () => __wconc_req("node:cluster"));
      __wconc_lazy(W, "test", () => __wconc_pick(__wconc_req("node:test"), ["test", "describe", "it"]));
      if (W.vm === undefined) {
        W.vm = {
          // run：上下文建值求值摘除全包（completion 透明；失败抛包络错）。
          run(code, o) {
            if (typeof code !== "string") throw new TypeError("vm.run requires code");
            const filename = (o && o.filename !== undefined) ? String(o.filename) : "wjs-vm.js";
            const id = Number(__wjs2_vm_create());
            try {
              return __wjs2_vm_run(id, code, filename);
            } finally {
              try { __wjs2_vm_release(id); } catch {}
            }
          },
        };
      }
    }
  } catch {}
}
"#;
