//! fetch 语义（Headers/Request/Response/body 快照）（prelude 分域；拼接顺序见 mod.rs）。
pub const HTTP_JS: &str = r#"
// ---- Phase 3b: Headers / Request / Response / fetch ----
// AbortSignal 重构到全局 EventTarget 基类（Node 同构：signal 即 EventTarget，
// abort 走 dispatchEvent；监听登记/移除/once/signal 选项全由基类承载）。
const __wjs_abortState = new WeakMap();
// Proxy 穿透键（mustNotMutateObjectDeep 包信号形）：Node 套件把 { signal }
//  deep-proxy 后再传入，WeakMap 的精确身份键即断裂（get→undefined→
//  TypeError）。状态同时挂 symbol 自有属性——读经 Proxy 转发仍命中目标本体；
//  写入只发生在构造/触发期（真实对象），永不穿过 Proxy 的 set 陷阱。
const __wjs_abortSym = Symbol("winterjs.abortState");
function __wjs_abortStateOf(signal) {
  return __wjs_abortState.get(signal) ?? signal[__wjs_abortSym];
}
function __wjs_abortFire(signal, reason) {
  const st = __wjs_abortStateOf(signal);
  if (!st || st.aborted) return;
  st.aborted = true;
  st.reason = reason === undefined
    ? new DOMException("This operation was aborted", "AbortError")
    : reason;
  const event = new Event("abort");
  // onabort 独立属性路径（Node 同为 getter/setter 而非 EventTarget on* 表）；
  // 沿既有口径吞错（abort 链失败不该炸用户回调）。
  if (typeof st.onabort === "function") {
    try { st.onabort.call(signal, event); } catch {}
  }
  signal.dispatchEvent(event);
}
globalThis.AbortSignal = class AbortSignal extends EventTarget {
  constructor() {
    super();
    const st = { aborted: false, reason: undefined, onabort: null };
    __wjs_abortState.set(this, st);
    this[__wjs_abortSym] = st;
  }
  get aborted() { return __wjs_abortStateOf(this).aborted; }
  get reason() { return __wjs_abortStateOf(this).reason; }
  get onabort() { return __wjs_abortStateOf(this).onabort; }
  set onabort(cb) { __wjs_abortStateOf(this).onabort = typeof cb === "function" ? cb : null; }
  throwIfAborted() {
    const st = __wjs_abortStateOf(this);
    if (st.aborted) throw st.reason;
  }
  static abort(reason) {
    const s = new AbortSignal();
    __wjs_abortFire(s, reason);
    return s;
  }
  static timeout(ms) {
    const c = new AbortController();
    const t = Number(ms);
    if (!Number.isFinite(t) || t < 0) throw new TypeError("AbortSignal.timeout needs a non-negative delay");
    setTimeout(() => c.abort(new DOMException("The operation was aborted due to timeout", "TimeoutError")), t);
    return c.signal;
  }
  static any(signals) {
    const list = [...(signals ?? [])];
    const c = new AbortController();
    for (const s of list) {
      if (!(s instanceof AbortSignal)) throw new TypeError("AbortSignal.any needs AbortSignals");
      if (s.aborted) { c.abort(s.reason); break; }
      s.addEventListener("abort", () => c.abort(s.reason), { once: true });
    }
    return c.signal;
  }
};
globalThis.AbortController = class AbortController {
  #signal;
  constructor() { this.#signal = new AbortSignal(); }
  get signal() { return this.#signal; }
  abort(reason) { __wjs_abortFire(this.#signal, reason); }
};
globalThis.Headers = class Headers {
  #pairs;
  constructor(init) {
    this.#pairs = [];
    if (init === undefined) return;
    if (init instanceof Headers) { for (const [k, v] of init) this.append(k, v); }
    else if (Array.isArray(init)) { for (const [k, v] of init) this.append(String(k), String(v)); }
    else if (typeof init === "object" && init !== null) {
      for (const [k, v] of Object.entries(init)) this.append(k, String(v));
    } else throw new TypeError("Headers: unsupported init");
  }
  static #norm(n) { return String(n).trim().toLowerCase(); }
  append(n, v) { this.#pairs.push([Headers.#norm(n), String(v).trim()]); }
  delete(n) { n = Headers.#norm(n); this.#pairs = this.#pairs.filter((p) => p[0] !== n); }
  get(n) {
    n = Headers.#norm(n);
    const vs = this.#pairs.filter((p) => p[0] === n).map((p) => p[1]);
    return vs.length ? vs.join(", ") : null;
  }
  getSetCookie() {
    return this.#pairs.filter((p) => p[0] === "set-cookie").map((p) => p[1]);
  }
  has(n) { n = Headers.#norm(n); return this.#pairs.some((p) => p[0] === n); }
  set(n, v) {
    n = Headers.#norm(n); v = String(v).trim();
    let found = false;
    this.#pairs = this.#pairs.filter((p) => {
      if (p[0] !== n) return true;
      if (!found) { p[1] = v; found = true; return true; }
      return false;
    });
    if (!found) this.#pairs.push([n, v]);
  }
  *keys() { for (const [k] of this.#sorted()) yield k; }
  *values() { for (const [, v] of this.#sorted()) yield v; }
  *entries() { for (const p of this.#sorted()) yield p; }
  [Symbol.iterator]() { return this.entries(); }
  forEach(cb, thisArg) { for (const [k, v] of this.#sorted()) cb.call(thisArg, v, k, this); }
  #sorted() { return [...this.#pairs].sort((a, b) => a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0); }
};
const __wjs_respState = new WeakMap();
function __wjs_respInit(resp, s) {
  __wjs_respState.set(resp, {
    status: s.status, statusText: s.statusText ?? "", headers: s.headers,
    url: s.url ?? "", bodyU8: s.bodyU8 ?? null, streamId: s.streamId ?? null, bodyUsed: false,
  });
}
function __wjs_takeBody(resp, what) {
  const st = __wjs_respState.get(resp);
  if (st.bodyUsed) throw new TypeError(`${what}: body already used`);
  st.bodyUsed = true;
  return st.bodyU8;
}
// 流式/快照统一建流（body getter 与 text 系共用；bodyUsed 由调用方维护）。
function __wjs_respStream(resp) {
  const st = __wjs_respState.get(resp);
  if (!st.bodyStream) {
    if (st.streamId !== null && st.streamId !== undefined) {
      const sid = st.streamId;
      st.bodyStream = new ReadableStream({
        pull(c) {
          return new Promise((resolve, reject) => {
            // 中止后 pull 直接拒绝（Rust 状态已摘，不再进 native）。
            if (__wjs_abortedFetch.has(sid)) {
              reject(new Error("AbortError: fetch aborted"));
              return;
            }
            __wjs_fetch_pull(sid,
              (chunk) => {
                if (chunk === null || chunk === undefined) {
                  try { c.close(); } catch {}
                  __wjs_fetchCleanup(sid);
                  resolve();
                  return;
                }
                try { c.enqueue(chunk); } catch (e) { reject(e); return; }
                resolve();
              },
              (e) => reject(e));
          });
        },
        cancel() { __wjs_fetchCleanup(sid); __wjs_fetch_abort(sid); },
      });
    } else {
      const bytes = st.bodyU8 ? st.bodyU8.slice() : new Uint8Array(0);
      st.bodyStream = new ReadableStream({
        start(c) { if (bytes.length) c.enqueue(bytes); c.close(); },
      });
    }
  }
  return st.bodyStream;
}
// 取全量字节（流式即读完流；快照即原路径；读即标记 disturbed）。
async function __wjs_respStreamBytes(resp, what) {
  const st = __wjs_respState.get(resp);
  if (st.streamId !== null && st.streamId !== undefined) {
    if (st.bodyUsed) throw new TypeError(`${what}: body already used`);
    st.bodyUsed = true;
    const chunks = [];
    let total = 0;
    for await (const c of __wjs_respStream(resp)) {
      const u8 = c instanceof Uint8Array ? c : new Uint8Array(c);
      chunks.push(u8);
      total += u8.length;
    }
    const out = new Uint8Array(total);
    let off = 0;
    for (const u8 of chunks) { out.set(u8, off); off += u8.length; }
    return out;
  }
  const b = __wjs_takeBody(resp, what);
  return b ? b.slice() : new Uint8Array(0);
}
function __wjs_normBody(body, what) {
  if (body === undefined || body === null) return null;
  if (typeof body === "string") return new TextEncoder().encode(body);
  if (body instanceof URLSearchParams) return new TextEncoder().encode(body.toString());
  if (body instanceof Uint8Array) return body.slice();
  if (body instanceof ArrayBuffer) return new Uint8Array(body.slice(0));
  throw new TypeError(`${what}: unsupported body type`);
}
function __wjs_fillHeaders(headers, init) {
  if (init === undefined) return;
  if (init instanceof Headers) { for (const [k, v] of init) headers.append(k, v); }
  else if (Array.isArray(init)) { for (const [k, v] of init) headers.append(String(k), String(v)); }
  else if (typeof init === "object" && init !== null) {
    for (const [k, v] of Object.entries(init)) headers.append(k, String(v));
  } else throw new TypeError("Headers: unsupported init");
}
globalThis.Response = class Response {
  constructor(body, init = {}) {
    const bytes = __wjs_normBody(body, "Response");
    const status = init.status === undefined ? 200 : Number(init.status);
    if (!Number.isInteger(status) || status < 200 || status > 599) {
      throw new RangeError("Response status must be 200-599");
    }
    const headers = new Headers();
    __wjs_fillHeaders(headers, init.headers);
    __wjs_respInit(this, {
      status, headers, url: "",
      statusText: init.statusText === undefined ? "" : String(init.statusText),
      bodyU8: bytes,
    });
  }
  get status() { return __wjs_respState.get(this).status; }
  get statusText() { return __wjs_respState.get(this).statusText; }
  get headers() { return __wjs_respState.get(this).headers; }
  get url() { return __wjs_respState.get(this).url; }
  get ok() { const s = this.status; return s >= 200 && s < 300; }
  get bodyUsed() { return __wjs_respState.get(this).bodyUsed; }
  get body() {
    const st = __wjs_respState.get(this);
    if (st.bodyUsed) return null;
    return __wjs_respStream(this);
  }
  async text() { return new TextDecoder().decode(await __wjs_respStreamBytes(this, "Response.text")); }
  async json() { return JSON.parse(await this.text()); }
  async arrayBuffer() { const b = await __wjs_respStreamBytes(this, "Response.arrayBuffer"); return b.slice().buffer; }
  async bytes() { return __wjs_respStreamBytes(this, "Response.bytes"); }
  static error() {
    const r = new Response(null);
    __wjs_respInit(r, { status: 0, statusText: "", headers: new Headers(), url: "", bodyU8: null });
    return r;
  }
  static redirect(url, status = 302) {
    if (![301, 302, 303, 307, 308].includes(status)) throw new RangeError("redirect status must be 301/302/303/307/308");
    const h = new Headers();
    h.set("location", String(url));
    return new Response(null, { status, headers: h });
  }
  // Response.json(data, init)：body 为 JSON 串，缺省 content-type 且 init 未给
  // 即补 application/json（真机 26.8.2 实测：init 显式 content-type 优先；
  // undefined/函数/不可序列化即 TypeError "Value is not JSON serializable"，
  // 用户 toJSON 抛错亦吞为该错；坏 status 走构造器 RangeError）。
  static json(data, init) {
    let text;
    try {
      text = JSON.stringify(data);
    } catch {
      throw new TypeError("Value is not JSON serializable");
    }
    if (text === undefined) throw new TypeError("Value is not JSON serializable");
    if (init === undefined || init === null) init = {};
    const headers = new Headers();
    __wjs_fillHeaders(headers, init.headers);
    if (!headers.has("content-type")) headers.set("content-type", "application/json");
    return new Response(text, { ...init, headers });
  }
};
const __wjs_reqState = new WeakMap();
function __wjs_takeReqBody(req) {
  const st = __wjs_reqState.get(req);
  if (st.bodyUsed) throw new TypeError("Request body already used");
  st.bodyUsed = true;
  return st.bodyU8;
}
// serve 请求体流（plan4 T1）：streamId 由 Rust Head 分发置入，chunk 经既有
// `__wjs_fetch_pull` 拉取（fetch 流机制复用，零新 native）；`__wjs_respStream`
// 同构（快照分支/流分支/读完标记三件照抄，`Response.body` 语义对等）。
function __wjs_reqStream(req) {
  const st = __wjs_reqState.get(req);
  if (!st.reqStream) {
    if (st.streamId !== null && st.streamId !== undefined) {
      const sid = st.streamId;
      st.reqStream = new ReadableStream({
        pull(c) {
          return new Promise((resolve, reject) => {
            __wjs_fetch_pull(sid,
              (chunk) => {
                if (chunk === null || chunk === undefined) {
                  try { c.close(); } catch {}
                  resolve();
                  return;
                }
                try { c.enqueue(chunk); } catch (e) { reject(e); return; }
                resolve();
              },
              (e) => reject(e));
          });
        },
        cancel() {},
      });
    } else {
      const bytes = st.bodyU8 ? st.bodyU8.slice() : new Uint8Array(0);
      st.reqStream = new ReadableStream({
        start(c) { if (bytes.length) c.enqueue(bytes); c.close(); },
      });
    }
  }
  return st.reqStream;
}
// 取全量字节（流式即读完流；快照即原路径；与 `__wjs_respStreamBytes` 同构）。
async function __wjs_reqStreamBytes(req, what) {
  const st = __wjs_reqState.get(req);
  if (st.streamId !== null && st.streamId !== undefined) {
    if (st.bodyUsed) throw new TypeError(`${what}: body already used`);
    st.bodyUsed = true;
    const chunks = [];
    let total = 0;
    for await (const c of __wjs_reqStream(req)) {
      const u8 = c instanceof Uint8Array ? c : new Uint8Array(c);
      chunks.push(u8);
      total += u8.length;
    }
    const out = new Uint8Array(total);
    let off = 0;
    for (const u8 of chunks) { out.set(u8, off); off += u8.length; }
    return out;
  }
  const b = __wjs_takeReqBody(req);
  return b ? b.slice() : new Uint8Array(0);
}
globalThis.Request = class Request {
  constructor(input, init = {}) {
    let url, method = "GET", headers = new Headers(), bodyU8 = null, signal = null;
    if (input instanceof Request) {
      const s = __wjs_reqState.get(input);
      url = s.url; method = s.method;
      for (const [k, v] of s.headers) headers.append(k, v);
      bodyU8 = s.bodyU8 ? s.bodyU8.slice() : null; signal = s.signal;
    } else if (typeof input === "string" || input instanceof URL) {
      url = String(input);
    } else throw new TypeError("Request: unsupported input");
    if (init.method !== undefined) method = String(init.method).toUpperCase();
    if (["CONNECT", "TRACE", "TRACK"].includes(method)) throw new TypeError(`Request: forbidden method ${method}`);
    if (init.headers !== undefined) { headers = new Headers(); __wjs_fillHeaders(headers, init.headers); }
    if (init.body !== undefined && init.body !== null) bodyU8 = __wjs_normBody(init.body, "Request");
    if ((method === "GET" || method === "HEAD") && bodyU8) {
      throw new TypeError("Request with GET/HEAD method cannot have body");
    }
    if (init.signal !== undefined && init.signal !== null) signal = init.signal;
    try { url = String(new URL(url)); } catch { throw new TypeError(`Request: Invalid URL: ${url}`); }
    __wjs_reqState.set(this, { url, method, headers, bodyU8, signal, bodyUsed: false });
  }
  get url() { return __wjs_reqState.get(this).url; }
  get method() { return __wjs_reqState.get(this).method; }
  get headers() { return __wjs_reqState.get(this).headers; }
  get signal() { return __wjs_reqState.get(this).signal; }
  get bodyUsed() { return __wjs_reqState.get(this).bodyUsed; }
  get body() {
    const st = __wjs_reqState.get(this);
    if (st.bodyUsed) return null;
    return __wjs_reqStream(this);
  }
  // clone()：url/方法/头（拷贝）/信号（fresh follower）/体（tee）全复制。
  // 真机 26.8.2 实测口径：bodyUsed 即 TypeError；clone().signal 永不 === 原
  // signal（无信号即 fresh 未 abort，有信号即跟随 abort）；快照体直接切片，
  // 流体（serve 请求）走源流 tee（已锁即抛原生错）。
  clone() {
    const st = __wjs_reqState.get(this);
    if (st.bodyUsed) throw new TypeError("Request.clone: body already used");
    let signal;
    if (st.signal !== null && st.signal !== undefined) {
      const c = new AbortController();
      signal = c.signal;
      const src = st.signal;
      if (src.aborted) {
        try { c.abort(src.reason); } catch {}
      } else {
        src.addEventListener("abort", () => { try { c.abort(src.reason); } catch {} }, { once: true });
      }
    } else {
      signal = new AbortController().signal;
    }
    const headers = new Headers();
    for (const [k, v] of st.headers) headers.append(k, v);
    const out = new Request(st.url, { method: st.method, headers, signal });
    const ost = __wjs_reqState.get(out);
    if (st.streamId !== null && st.streamId !== undefined) {
      const [b1, b2] = __wjs_reqStream(this).tee();
      st.reqStream = b1;
      ost.streamId = st.streamId;
      ost.serveId = st.serveId;
      ost.reqStream = b2;
    } else {
      ost.bodyU8 = st.bodyU8 ? st.bodyU8.slice() : null;
    }
    return out;
  }
  async text() { return new TextDecoder().decode(await __wjs_reqStreamBytes(this, "Request.text")); }
  async json() { return JSON.parse(await this.text()); }
  async arrayBuffer() { const b = await __wjs_reqStreamBytes(this, "Request.arrayBuffer"); return b.slice().buffer; }
};
globalThis.__wjs_make_response = (metaJson, bodyU8) => {
  const meta = JSON.parse(metaJson);
  const headers = new Headers();
  for (const [k, v] of meta.headers) headers.append(k, v);
  const resp = new Response(null);
  __wjs_respInit(resp, {
    status: meta.status, statusText: meta.statusText, headers,
    url: meta.url, bodyU8: bodyU8 ?? null,
    streamId: meta.streamId === undefined ? null : meta.streamId,
  });
  return resp;
};
globalThis.__wjs_make_fetch_error = (msg) => new Error(String(msg));
// 独立 serve 驱动（plan4 T1）：Rust dispatch 经 `__wjs_serve_on_head` 投递请求
//（id, metaJson, streamId；streamId 与请求 id 同号）。全异步：fetch 决议后排空
// 响应（头同步读 + 体流式推），失败一律 `__wjs_serve_fail` 落 500（未知 id
// 静默成功，见 serve_bridge）。
// T4：upgrade 请求（meta.upgrade）由 handler 内 `__wjs_serve_socket(req)` 工厂
// 配对、直接返回 socket 即接受；返回 Response 即 Decline 走普通管线
//（offer 期 fetch 与 HTTP 期 fetch 各跑一次，见 serve.rs 中间件注释）。
function __wjs_serve_make_request(url, method, headers, streamId, serveId) {
  const req = new Request(url, { method, headers });
  __wjs_reqState.get(req).streamId = streamId;
  __wjs_reqState.get(req).serveId = serveId;
  return req;
}
"#;
