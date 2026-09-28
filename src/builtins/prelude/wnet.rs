//! B2 网络门面 JS 面：`WinterJS.tcp/udp/dns/tls`（Web 风 Promise）。
//!
//! 直驱 `__wjs_net/dgram/dns/tls` 原生（`__ev` 自有 sink，不套 `node:` 移植层）。
//! 形状：connect/bind/listen 皆 Promise；字节流与消息皆 AsyncIterator；
//! 写 fire-and-forget（背压不建模，文档记录）；错误带 `code`（`{code,msg}`
//! 载荷直取，不做 node 文案重塑）。
pub const WNET_JS: &str = r#"
{
  const __wnet_b64ToU8 = (b64) => {
    const s = atob(b64);
    const u8 = new Uint8Array(s.length);
    for (let i = 0; i < s.length; i++) u8[i] = s.charCodeAt(i);
    return u8;
  };
  const __wnet_toU8 = (v, what) => {
    if (typeof v === "string") return new TextEncoder().encode(v);
    if (v instanceof Uint8Array) return v;
    if (v instanceof ArrayBuffer) return new Uint8Array(v);
    if (ArrayBuffer.isView(v)) return new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
    throw new TypeError(`${what} must be string, Uint8Array, or ArrayBuffer`);
  };
  const __wnet_err = (payload, fallback) => {
    let code = "UNKNOWN", msg = fallback;
    try {
      const o = JSON.parse(payload || "{}");
      if (typeof o.code === "string") code = o.code;
      if (typeof o.msg === "string") msg = o.msg;
    } catch {}
    const e = new Error(msg);
    e.code = code;
    return e;
  };
  // 字节流端（connect/accept 共用；sink 纯闭包，dispatch 以 global 为 this 调无妨）。
  const __wnet_stream = () => {
    const st = {
      id: 0, queue: [], waiters: [], ended: false, closed: false,
      lastError: null, onConnect: null, onError: null,
    };
    const flush = () => {
      while (st.waiters.length && st.queue.length) {
        const w = st.waiters.shift();
        w(st.queue.shift());
      }
      if ((st.ended || st.closed) && st.waiters.length) {
        const ws = st.waiters.splice(0);
        for (const w of ws) w(null);
      }
    };
    const sock = {
      get closed() { return st.closed; },
      get error() { return st.lastError; },
      write(data) {
        const u8 = __wnet_toU8(data, "socket.write");
        __wjs_net_write(st.id, u8);
        return u8.length;
      },
      end() { try { __wjs_net_end(st.id); } catch {} },
      destroy() { try { __wjs_net_destroy(st.id); } catch {} st.closed = true; flush(); },
      async *[Symbol.asyncIterator]() {
        for (;;) {
          if (st.queue.length) yield st.queue.shift();
          else if (st.ended || st.closed) return;
          else {
            const chunk = await new Promise((r) => { st.waiters.push(r); });
            if (chunk === null) return;
            yield chunk;
          }
        }
      },
    };
    const sink = {
      __ev(kind, payload) {
        try {
          if (kind === "connect") {
            const cb = st.onConnect; st.onConnect = null;
            if (cb) cb();
          } else if (kind === "data") {
            st.queue.push(__wnet_b64ToU8(String(payload || "")));
            flush();
          } else if (kind === "end") {
            st.ended = true;
            flush();
          } else if (kind === "error") {
            const e = __wnet_err(payload, "socket error");
            st.lastError = e;
            const cb = st.onError; st.onError = null;
            if (cb) cb(e);
            st.ended = true;
            flush();
          } else if (kind === "close") {
            st.closed = true;
            flush();
          }
        } catch {}
      },
    };
    return { sock, sink, st };
  };
  const tcp = {
    connect(host, port) {
      if (typeof host !== "string" || host === "") throw new TypeError("tcp.connect requires a host");
      if (typeof port !== "number") throw new TypeError("tcp.connect requires a port");
      return new Promise((resolve, reject) => {
        const { sock, sink, st } = __wnet_stream();
        st.onConnect = () => resolve(sock);
        st.onError = (e) => reject(e);
        st.id = Number(__wjs_net_connect(host, port, sink));
      });
    },
    listen(port, host) {
      if (typeof port !== "number") throw new TypeError("tcp.listen requires a port");
      const h = host === undefined ? "0.0.0.0" : String(host);
      return new Promise((resolve, reject) => {
        const acceptQ = [], acceptW = [];
        let addr = null, ended = false, listenId = 0;
        const flushA = () => {
          while (acceptW.length && acceptQ.length) acceptW.shift()(acceptQ.shift());
          if (ended && acceptW.length) for (const w of acceptW.splice(0)) w(null);
        };
        const server = {
          address() { return addr; },
          close() { try { __wjs_net_destroy(listenId); } catch {} ended = true; flushA(); },
          async *[Symbol.asyncIterator]() {
            for (;;) {
              if (acceptQ.length) yield acceptQ.shift();
              else if (ended) return;
              else {
                const s = await new Promise((r) => acceptW.push(r));
                if (s === null) return;
                yield s;
              }
            }
          },
        };
        const sink = {
          __ev(kind, payload) {
            try {
              if (kind === "listening") {
                const o = JSON.parse(payload || "{}");
                addr = { address: o.addr, port: o.port };
                resolve(server);
              } else if (kind === "connection") {
                const o = JSON.parse(payload || "{}");
                const c = __wnet_stream();
                c.st.id = Number(o.connId);
                __wjs_net_attach(c.st.id, c.sink);
                acceptQ.push(c.sock);
                flushA();
              } else if (kind === "error") {
                reject(__wnet_err(payload, "listen error"));
              } else if (kind === "close") {
                ended = true;
                flushA();
              }
            } catch {}
          },
        };
        listenId = Number(__wjs_net_listen(port, h, sink));
      });
    },
  };
  const udp = {
    bind(port, host) {
      const p = port === undefined ? 0 : Number(port);
      const h = host === undefined ? "0.0.0.0" : String(host);
      if (!Number.isInteger(p) || p < 0) throw new TypeError("udp.bind requires a port");
      return new Promise((resolve, reject) => {
        const queue = [], waiters = [];
        let id = 0, addr = null, ended = false;
        const flush = () => {
          while (waiters.length && queue.length) waiters.shift()(queue.shift());
          if (ended && waiters.length) for (const w of waiters.splice(0)) w(null);
        };
        const sock = {
          address() { return addr; },
          get closed() { return ended; },
          send(data, port, host) {
            if (typeof port !== "number" || typeof host !== "string") {
              throw new TypeError("udp.send requires (data, port, host)");
            }
            const u8 = __wnet_toU8(data, "udp.send");
            __wjs_dgram_send(id, u8, `${host}:${port}`, 0);
            return u8.length;
          },
          close() { try { __wjs_net_destroy(id); } catch {} ended = true; flush(); },
          async *[Symbol.asyncIterator]() {
            for (;;) {
              if (queue.length) yield queue.shift();
              else if (ended) return;
              else {
                const m = await new Promise((r) => waiters.push(r));
                if (m === null) return;
                yield m;
              }
            }
          },
        };
        const sink = {
          __ev(kind, payload) {
            try {
              if (kind === "listening") {
                const o = JSON.parse(payload || "{}");
                addr = { address: o.addr, port: o.port };
                resolve(sock);
              } else if (kind === "message") {
                const o = JSON.parse(payload || "{}");
                queue.push({
                  data: __wnet_b64ToU8(String(o.data || "")),
                  remote: { address: o.address, port: o.port },
                });
                flush();
              } else if (kind === "error") {
                reject(__wnet_err(payload, "udp error"));
              } else if (kind === "close") {
                ended = true;
                flush();
              }
            } catch {}
          },
        };
        id = Number(__wjs_dgram_bind(p, h, sink, 0));
      });
    },
  };
  const dns = {
    lookup(host) {
      if (typeof host !== "string" || host === "") throw new TypeError("dns.lookup requires a hostname");
      return Promise.resolve().then(() => JSON.parse(__wjs_dns_lookup(host)));
    },
    resolve(host, type) {
      if (typeof host !== "string" || host === "") throw new TypeError("dns.resolve requires a hostname");
      const t = type === undefined ? "A" : String(type);
      return Promise.resolve().then(() => JSON.parse(__wjs_dns_query(t, host)));
    },
  };
  const tls = {
    connect(o) {
      if (!o || typeof o !== "object") throw new TypeError("tls.connect requires options");
      const host = String(o.host), port = Number(o.port);
      if (!host || !Number.isInteger(port)) throw new TypeError("tls.connect requires { host, port }");
      const wire = {};
      if (o.servername !== undefined) wire.servername = String(o.servername);
      if (o.rejectUnauthorized !== undefined) wire.rejectUnauthorized = !!o.rejectUnauthorized;
      return new Promise((resolve, reject) => {
        const { sock, sink, st } = __wnet_stream();
        st.onConnect = () => resolve(sock);
        st.onError = (e) => reject(e);
        st.id = Number(__wjs_tls_connect(host, port, JSON.stringify(wire), sink));
      });
    },
  };
  try {
    const W = globalThis.WinterJS;
    if (W && W.tcp === undefined) {
      W.tcp = tcp;
      W.udp = udp;
      W.dns = dns;
      W.tls = tls;
    }
  } catch {}
}
"#;
