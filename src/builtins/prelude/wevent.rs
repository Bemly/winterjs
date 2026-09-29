//! B6 事件杂项门面 JS 面：`WinterJS2.stream/diag/domain/trace/async/quic/crypto/ffi/serve`。
//!
//! `stream.pipeline` 纯 Web 流实现；`diag/domain/trace/async/quic` 复用移植实现
//!（util 同款复用模式）；`crypto/ffi` 同对象别名（ffi 门控随 `bun:ffi`）；
//! `serve` 复用 `node:http`（Deno.serve 同款桥接形）。`events` 由全局
//! `EventTarget` 覆盖，不另设面（文档记录）。
pub const WEVENT_JS: &str = r#"
{
  const __wev_req = (spec) => {
    try {
      const r = globalThis.require;
      if (typeof r !== "function") return null;
      return r(spec);
    } catch { return null; }
  };
  const __wev_need = (spec, face) => {
    const m = __wev_req(spec);
    if (!m) throw new Error(`${face} is unavailable in this build`);
    return m;
  };
  const stream = {
    // pipeline(readable, ...transforms, writable) → 转完 resolve（任一错即 reject）。
    async pipeline(...streams) {
      if (streams.length < 2) throw new TypeError("stream.pipeline requires readable + writable");
      const src = streams[0], dst = streams[streams.length - 1];
      if (!(src instanceof ReadableStream)) throw new TypeError("stream.pipeline first must be Readable");
      if (!(dst instanceof WritableStream)) throw new TypeError("stream.pipeline last must be Writable");
      let flow = src;
      for (let i = 1; i < streams.length - 1; i++) flow = flow.pipeThrough(streams[i]);
      await flow.pipeTo(dst);
    },
  };
  const serve = (o, handler) => {
    const http = __wev_need("node:http", "WinterJS2.serve");
    let opts = o || {}, h = handler;
    if (typeof opts === "function") { h = opts; opts = {}; }
    if (typeof h !== "function") throw new TypeError("serve requires a fetch handler");
    const port = Number(opts.port ?? 8000);
    const hostname = String(opts.hostname ?? opts.host ?? "127.0.0.1");
    const server = http.createServer(async (req, res) => {
      try {
        const chunks = [];
        for await (const c of req) chunks.push(c);
        const body = chunks.length ? Buffer.concat(chunks) : null;
        const url = "http://" + (req.headers.host || (hostname + ":" + port)) + req.url;
        const fwd = {};
        for (const [k, v] of Object.entries(req.headers)) {
          if (v !== undefined) fwd[k] = Array.isArray(v) ? v.join(", ") : String(v);
        }
        const frequest = new Request(url, { method: req.method, headers: fwd, body });
        const fresponse = await h(frequest);
        const out = {};
        fresponse.headers.forEach((v, k) => { out[k] = v; });
        res.writeHead(fresponse.status, out);
        res.end(Buffer.from(new Uint8Array(await fresponse.arrayBuffer())));
      } catch (e) {
        try { res.writeHead(500); res.end(String((e && e.message) || e)); } catch {}
      }
    });
    return new Promise((resolve, reject) => {
      server.on("error", reject);
      server.listen(port, hostname, () => {
        let realPort = port;
        try {
          const a = server.address();
          if (a && typeof a.port === "number") realPort = a.port;
        } catch {}
        try { if (typeof opts.onListen === "function") opts.onListen({ port: realPort, hostname }); } catch {}
        resolve({
          addr: { transport: "tcp", hostname, port: realPort },
          shutdown() { return new Promise((r) => server.close(() => r())); },
        });
      });
    });
  };
  // 预言求值期 process 尚不存在（4.230）：node:*/bun:* 别名一律惰性 getter。
  const __wev_lazy = (W, key, fn) => {
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
      if (W.stream === undefined) W.stream = stream;
      if (W.serve === undefined) W.serve = serve;
      __wev_lazy(W, "diagnostics", () => __wev_req("node:diagnostics_channel"));
      __wev_lazy(W, "domain", () => __wev_req("node:domain"));
      __wev_lazy(W, "trace", () => __wev_req("node:trace_events"));
      __wev_lazy(W, "AsyncLocalStorage", () => {
        const m = __wev_req("node:async_hooks");
        return (m && typeof m.AsyncLocalStorage === "function") ? m.AsyncLocalStorage : null;
      });
      __wev_lazy(W, "quic", () => __wev_req("node:quic"));
      if (W.crypto === undefined) W.crypto = globalThis.crypto || null;
      __wev_lazy(W, "ffi", () => __wev_req("bun:ffi"));
    }
  } catch {}
}
"#;
