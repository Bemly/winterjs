//! B3 进程交互门面 JS 面：`WinterJS.command/terminal/repl`（Web 风 Promise）。
//!
//! `command` 直驱 `__wjs_spawn_*/child_*` 原生（target 自有 sink：
//! `__pushOut/__pushErr/onexit/onclose`）；`terminal/repl` 复用移植实现
//! （util 同款复用模式，用户侧无需 `node:` 前缀）。
pub const WPROC_JS: &str = r#"
{
  const __wproc_b64ToU8 = (b64) => {
    const s = atob(b64);
    const u8 = new Uint8Array(s.length);
    for (let i = 0; i < s.length; i++) u8[i] = s.charCodeAt(i);
    return u8;
  };
  const __wproc_u8ToB64 = (u8) => {
    let s = "";
    for (let i = 0; i < u8.length; i += 8192) s += String.fromCharCode.apply(null, u8.subarray(i, i + 8192));
    return btoa(s);
  };
  const __wproc_toU8 = (v, what) => {
    if (v instanceof Uint8Array) return v;
    if (typeof v === "string") return new TextEncoder().encode(v);
    if (v instanceof ArrayBuffer) return new Uint8Array(v);
    if (ArrayBuffer.isView(v)) return new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
    throw new TypeError(`${what} must be string, Uint8Array, or ArrayBuffer`);
  };
  const __wproc_opts = (o) => JSON.stringify({
    cwd: (o && o.cwd !== undefined) ? String(o.cwd) : null,
    env: (o && o.env !== undefined) ? o.env : null,
  });
  // spawn 通道（stdio 全 pipe；stdout/stderr 块收集 + AsyncIterator 双面）。
  const __wproc_spawn = (file, args, o) => {
    const outQ = [], errQ = [], outW = [], errW = [];
    let exitR = null, exited = null;
    const flushQ = (q, w) => {
      while (w.length && q.length) w.shift()(q.shift());
    };
    const target = {
      __pushOut(b64) { outQ.push(__wproc_b64ToU8(String(b64))); flushQ(outQ, outW); },
      __pushErr(b64) { errQ.push(__wproc_b64ToU8(String(b64))); flushQ(errQ, errW); },
      onexit() {},
      onclose(code, signal) {
        exited = { code: code ?? null, signal: signal ?? null };
        for (const w of outW.splice(0)) w(null);
        for (const w of errW.splice(0)) w(null);
        if (exitR) { const r = exitR; exitR = null; r(exited); }
      },
    };
    const id = Number(__wjs_spawn_start(String(file), JSON.stringify((args || []).map(String)),
      __wproc_opts(o), target, JSON.stringify(["pipe", "pipe", "pipe"])));
    const streamOf = (q, w) => ({
      async *[Symbol.asyncIterator]() {
        for (;;) {
          if (q.length) yield q.shift();
          else if (exited) return;
          else {
            const c = await new Promise((r) => w.push(r));
            if (c === null) return;
            yield c;
          }
        }
      },
    });
    return {
      id,
      get pid() { try { return Number(__wjs_child_pid(id)); } catch { return -1; } },
      stdin: {
        write(d) { return !!__wjs_child_stdin_write(id, __wproc_u8ToB64(__wproc_toU8(d, "stdin.write"))); },
        close() { try { __wjs_child_stdin_close(id); } catch {} },
      },
      stdout: streamOf(outQ, outW),
      stderr: streamOf(errQ, errW),
      wait() {
        if (exited) return Promise.resolve(exited);
        return new Promise((r) => { exitR = r; });
      },
      kill(sig) { try { return !!__wjs_child_kill(id, sig === undefined ? "SIGTERM" : String(sig)); } catch { return false; } },
    };
  };
  const command = {
    spawn(file, args, o) {
      if (typeof file !== "string" || file === "") throw new TypeError("command.spawn requires a file");
      return __wproc_spawn(file, args, o);
    },
    // run：输出全收集（execFile 同位语义；大输出走 spawn 流式，文档记录）。
    async run(file, args, o) {
      if (typeof file !== "string" || file === "") throw new TypeError("command.run requires a file");
      const h = __wproc_spawn(file, args, o);
      try {
        if (o && o.stdin !== undefined) {
          h.stdin.write(o.stdin);
          h.stdin.close();
        }
        const out = [], err = [];
        const pump = async (s, acc) => { for await (const c of s) acc.push(c); };
        await Promise.all([pump(h.stdout, out), pump(h.stderr, err), h.wait()]);
        const cat = (a) => {
          let n = 0;
          for (const c of a) n += c.length;
          const u = new Uint8Array(n);
          let off = 0;
          for (const c of a) { u.set(c, off); off += c.length; }
          return u;
        };
        const r = await h.wait();
        return { code: r.code, signal: r.signal, stdout: cat(out), stderr: cat(err) };
      } finally {
        try { h.kill(); } catch {}
      }
    },
  };
  const __wproc_req = (spec) => {
    try {
      const r = globalThis.require;
      if (typeof r !== "function") return null;
      return r(spec);
    } catch { return null; }
  };
  const terminal = {
    createInterface(...a) {
      const m = __wproc_req("node:readline");
      if (!m || typeof m.createInterface !== "function") throw new Error("WinterJS.terminal requires node:readline");
      return m.createInterface(...a);
    },
  };
  const wrepl = {
    start(...a) {
      const m = __wproc_req("node:repl");
      if (!m || typeof m.start !== "function") throw new Error("WinterJS.repl requires node:repl");
      return m.start(...a);
    },
  };
  try {
    const W = globalThis.WinterJS;
    if (W && W.command === undefined) {
      W.command = command;
      W.terminal = terminal;
      W.repl = wrepl;
    }
  } catch {}
}
"#;
