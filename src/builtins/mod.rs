//! 内建注册与 JS prelude。
//! prelude 用 JS 实现需要 Promise/参数打包语义的薄壳（queueMicrotask、timers 包装、
//! `__wjs_call`/`__wjs_entries` 辅助），native 只做 Rust 侧的活。

pub mod clone;
pub mod console;
pub mod crypto;
pub mod encoding;
pub mod fetch;
pub mod timers;
pub mod url;

use std::ffi::CString;

use mozjs::context::JSContext;
use mozjs::jsapi::{JSObject, JSNative, JSPROP_ENUMERATE};
use mozjs::jsval::ObjectValue;
use mozjs::rooted;
use crate::jsapi_glue::raw_handle;

use crate::error::Error;
use crate::jsapi_glue::report_error;

/// 引擎启动时在全局对象上求值的一次性脚本（§1 路线 Phase 1）。
pub const PRELUDE: &str = r#"
globalThis.queueMicrotask = function (cb) {
  if (typeof cb !== "function") throw new TypeError("queueMicrotask: callback must be a function");
  // 与引擎内部 job queue 同一条微任务队列；回调抛错 → 未处理 rejection（由 runtime 上报）
  Promise.resolve().then(cb);
};
globalThis.setTimeout = function (cb, ms, ...rest) {
  if (typeof cb !== "function") throw new TypeError("setTimeout: callback must be a function");
  return __wjs_setTimeout(cb, Number(ms) || 0, rest);
};
globalThis.setInterval = function (cb, ms, ...rest) {
  if (typeof cb !== "function") throw new TypeError("setInterval: callback must be a function");
  return __wjs_setInterval(cb, Number(ms) || 0, rest);
};
globalThis.clearTimeout = function (id) { __wjs_clearTimeout(typeof id === "number" ? id : 0); };
globalThis.clearInterval = function (id) { __wjs_clearTimeout(typeof id === "number" ? id : 0); };
// 事件循环触发定时器 / structuredClone 枚举属性用的内部辅助
globalThis.__wjs_call = (cb, args) => cb(...args);
globalThis.__wjs_entries = (v) => Object.entries(v);
// ---- Phase 3a: URL / URLSearchParams / TextEncoder/Decoder / base64 / crypto ----
globalThis.btoa = (s) => __wjs_btoa(String(s));
globalThis.atob = (s) => __wjs_atob(String(s));
const __wjs_urlState = new WeakMap();
const __wjs_uspState = new WeakMap();
function __wjs_setHref(urlObj, newHref) {
  const st = __wjs_urlState.get(urlObj);
  st.href = newHref;
  if (st.usp) {
    const search = __wjs_url_get(newHref, "search");
    const q = search.startsWith("?") ? search.slice(1) : search;
    __wjs_uspState.get(st.usp).pairs = q === "" ? [] : JSON.parse(__wjs_usp_parse(q));
  }
}
function __wjs_pushSearch(urlObj) {
  const st = __wjs_urlState.get(urlObj);
  if (!st.usp) return;
  const q = __wjs_usp_serialize(JSON.stringify(__wjs_uspState.get(st.usp).pairs));
  st.href = __wjs_url_set(st.href, "search", q === "" ? "" : "?" + q);
}
function __wjs_uspFromUrl(urlObj) {
  const usp = new URLSearchParams("");
  __wjs_uspState.get(usp).parent = urlObj;
  const st = __wjs_urlState.get(urlObj);
  const search = __wjs_url_get(st.href, "search");
  const q = search.startsWith("?") ? search.slice(1) : search;
  __wjs_uspState.get(usp).pairs = q === "" ? [] : JSON.parse(__wjs_usp_parse(q));
  st.usp = usp;
  return usp;
}
function __wjs_uspTouch(usp) {
  const s = __wjs_uspState.get(usp);
  if (s.parent) __wjs_pushSearch(s.parent);
}
globalThis.URL = class URL {
  constructor(url, base) {
    const href = (base === undefined)
      ? __wjs_url_parse(String(url))
      : __wjs_url_parse(String(url), String(base));
    __wjs_urlState.set(this, { href, usp: null });
  }
  static canParse(url, base) {
    try {
      if (base === undefined) __wjs_url_parse(String(url));
      else __wjs_url_parse(String(url), String(base));
      return true;
    } catch { return false; }
  }
  get href() { return __wjs_urlState.get(this).href; }
  set href(v) { __wjs_setHref(this, __wjs_url_parse(String(v))); }
  get protocol() { return __wjs_url_get(this.href, "protocol"); }
  set protocol(v) { __wjs_setHref(this, __wjs_url_set(this.href, "protocol", String(v))); }
  get username() { return __wjs_url_get(this.href, "username"); }
  set username(v) { __wjs_setHref(this, __wjs_url_set(this.href, "username", String(v))); }
  get password() { return __wjs_url_get(this.href, "password"); }
  set password(v) { __wjs_setHref(this, __wjs_url_set(this.href, "password", String(v))); }
  get host() { return __wjs_url_get(this.href, "host"); }
  set host(v) { __wjs_setHref(this, __wjs_url_set(this.href, "host", String(v))); }
  get hostname() { return __wjs_url_get(this.href, "hostname"); }
  set hostname(v) { __wjs_setHref(this, __wjs_url_set(this.href, "hostname", String(v))); }
  get port() { return __wjs_url_get(this.href, "port"); }
  set port(v) { __wjs_setHref(this, __wjs_url_set(this.href, "port", String(v))); }
  get pathname() { return __wjs_url_get(this.href, "pathname"); }
  set pathname(v) { __wjs_setHref(this, __wjs_url_set(this.href, "pathname", String(v))); }
  get search() { return __wjs_url_get(this.href, "search"); }
  set search(v) { __wjs_setHref(this, __wjs_url_set(this.href, "search", String(v))); }
  get hash() { return __wjs_url_get(this.href, "hash"); }
  set hash(v) { __wjs_setHref(this, __wjs_url_set(this.href, "hash", String(v))); }
  get origin() { return __wjs_url_get(this.href, "origin"); }
  get searchParams() {
    const st = __wjs_urlState.get(this);
    if (!st.usp) return __wjs_uspFromUrl(this);
    return st.usp;
  }
  toString() { return this.href; }
  toJSON() { return this.href; }
};
globalThis.URLSearchParams = class URLSearchParams {
  constructor(init) {
    let pairs;
    if (init === undefined) pairs = [];
    else if (typeof init === "string") {
      const q = init.startsWith("?") ? init.slice(1) : init;
      pairs = q === "" ? [] : JSON.parse(__wjs_usp_parse(q));
    } else if (Array.isArray(init)) pairs = init.map((p) => [String(p[0]), String(p[1])]);
    else if (typeof init === "object" && init !== null) {
      pairs = Object.entries(init).map(([k, v]) => [String(k), String(v)]);
    } else throw new TypeError("URLSearchParams: unsupported init");
    __wjs_uspState.set(this, { pairs, parent: null });
  }
  get size() { return __wjs_uspState.get(this).pairs.length; }
  append(n, v) { __wjs_uspState.get(this).pairs.push([String(n), String(v)]); __wjs_uspTouch(this); }
  delete(n, v) {
    n = String(n);
    const s = __wjs_uspState.get(this);
    s.pairs = (v === undefined)
      ? s.pairs.filter((p) => p[0] !== n)
      : s.pairs.filter((p) => !(p[0] === n && p[1] === String(v)));
    __wjs_uspTouch(this);
  }
  get(n) { const p = __wjs_uspState.get(this).pairs.find((p) => p[0] === String(n)); return p ? p[1] : null; }
  getAll(n) { n = String(n); return __wjs_uspState.get(this).pairs.filter((p) => p[0] === n).map((p) => p[1]); }
  has(n, v) {
    n = String(n);
    const ps = __wjs_uspState.get(this).pairs;
    return (v === undefined)
      ? ps.some((p) => p[0] === n)
      : ps.some((p) => p[0] === n && p[1] === String(v));
  }
  set(n, v) {
    n = String(n); v = String(v);
    const s = __wjs_uspState.get(this);
    let found = false;
    s.pairs = s.pairs.filter((p) => {
      if (p[0] !== n) return true;
      if (!found) { p[1] = v; found = true; return true; }
      return false;
    });
    if (!found) s.pairs.push([n, v]);
    __wjs_uspTouch(this);
  }
  sort() {
    __wjs_uspState.get(this).pairs.sort((a, b) => a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0);
    __wjs_uspTouch(this);
  }
  toString() { return __wjs_usp_serialize(JSON.stringify(__wjs_uspState.get(this).pairs)); }
  *keys() { for (const [k] of __wjs_uspState.get(this).pairs) yield k; }
  *values() { for (const [, v] of __wjs_uspState.get(this).pairs) yield v; }
  *entries() { for (const p of __wjs_uspState.get(this).pairs) yield p; }
  [Symbol.iterator]() { return this.entries(); }
  forEach(cb, thisArg) { for (const [k, v] of __wjs_uspState.get(this).pairs) cb.call(thisArg, v, k, this); }
};
globalThis.TextEncoder = class TextEncoder {
  get encoding() { return "utf-8"; }
  encode(s) { return __wjs_te_encode(String(s === undefined ? "" : s)); }
  encodeInto(s, dest) { return JSON.parse(__wjs_te_encode_into(String(s), dest)); }
};
globalThis.TextDecoder = class TextDecoder {
  #label; #fatal; #ignoreBOM;
  constructor(label = "utf-8", options) {
    this.#label = __wjs_td_canonical(String(label));
    this.#fatal = !!(options && options.fatal);
    this.#ignoreBOM = !!(options && options.ignoreBOM);
  }
  get encoding() { return this.#label; }
  get fatal() { return this.#fatal; }
  get ignoreBOM() { return this.#ignoreBOM; }
  decode(input, options) {
    if (options && options.stream) throw new Error("TextDecoder streaming decode needs Phase 3b");
    let view = input;
    if (view === undefined) return __wjs_td_decode(this.#label, 0, 0, undefined);
    if (view instanceof ArrayBuffer) view = new Uint8Array(view);
    else if (typeof SharedArrayBuffer !== "undefined" && view instanceof SharedArrayBuffer) {
      throw new TypeError("TextDecoder.decode does not accept SharedArrayBuffer views yet");
    } else if (ArrayBuffer.isView(view) && !(view instanceof Uint8Array)) {
      view = new Uint8Array(view.buffer, view.byteOffset, view.byteLength);
    }
    return __wjs_td_decode(this.#label, this.#fatal ? 1 : 0, this.#ignoreBOM ? 1 : 0, view);
  }
};
globalThis.crypto = {
  getRandomValues(view) { __wjs_fill_random(view); return view; },
  randomUUID() { return __wjs_random_uuid(); },
  subtle: {
    async digest(algorithm, data) {
      const name = typeof algorithm === "string" ? algorithm : String(algorithm?.name ?? algorithm);
      let view = data;
      if (view instanceof ArrayBuffer) view = new Uint8Array(view);
      else if (ArrayBuffer.isView(view) && !(view instanceof Uint8Array)) {
        view = new Uint8Array(view.buffer, view.byteOffset, view.byteLength);
      }
      const out = __wjs_subtle_digest(name, view);
      return out.buffer;
    },
  },
};
// ---- Phase 3b: Headers / Request / Response / fetch ----
const __wjs_abortState = new WeakMap();
function __wjs_abortFire(signal, reason) {
  const st = __wjs_abortState.get(signal);
  if (!st || st.aborted) return;
  st.aborted = true;
  st.reason = reason === undefined ? new Error("AbortError: signal aborted") : reason;
}
globalThis.AbortSignal = class AbortSignal {
  constructor() { __wjs_abortState.set(this, { aborted: false, reason: undefined }); }
  get aborted() { return __wjs_abortState.get(this).aborted; }
  get reason() { return __wjs_abortState.get(this).reason; }
  throwIfAborted() {
    const st = __wjs_abortState.get(this);
    if (st.aborted) throw st.reason;
  }
  static abort(reason) {
    const s = new AbortSignal();
    __wjs_abortFire(s, reason);
    return s;
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
    url: s.url ?? "", bodyU8: s.bodyU8 ?? null, bodyUsed: false,
  });
}
function __wjs_takeBody(resp, what) {
  const st = __wjs_respState.get(resp);
  if (st.bodyUsed) throw new TypeError(`${what}: body already used`);
  st.bodyUsed = true;
  return st.bodyU8;
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
  async text() { const b = __wjs_takeBody(this, "Response.text"); return b ? new TextDecoder().decode(b) : ""; }
  async json() { return JSON.parse(await this.text()); }
  async arrayBuffer() { const b = __wjs_takeBody(this, "Response.arrayBuffer"); return b ? b.slice().buffer : new ArrayBuffer(0); }
  async bytes() { const b = __wjs_takeBody(this, "Response.bytes"); return b ? b.slice() : new Uint8Array(0); }
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
};
const __wjs_reqState = new WeakMap();
function __wjs_takeReqBody(req) {
  const st = __wjs_reqState.get(req);
  if (st.bodyUsed) throw new TypeError("Request body already used");
  st.bodyUsed = true;
  return st.bodyU8;
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
  async text() { const b = __wjs_takeReqBody(this); return b ? new TextDecoder().decode(b) : ""; }
  async json() { return JSON.parse(await this.text()); }
  async arrayBuffer() { const b = __wjs_takeReqBody(this); return b ? b.slice().buffer : new ArrayBuffer(0); }
};
globalThis.__wjs_make_response = (metaJson, bodyU8) => {
  const meta = JSON.parse(metaJson);
  const headers = new Headers();
  for (const [k, v] of meta.headers) headers.append(k, v);
  const resp = new Response(null);
  __wjs_respInit(resp, {
    status: meta.status, statusText: meta.statusText, headers,
    url: meta.url, bodyU8: bodyU8 ?? null,
  });
  return resp;
};
globalThis.__wjs_make_fetch_error = (msg) => new Error(String(msg));
globalThis.fetch = (input, init = {}) => {
  const req = new Request(input, init);
  const st = __wjs_reqState.get(req);
  if (st.signal && st.signal.aborted) {
    const reason = st.signal.reason !== undefined
      ? st.signal.reason
      : __wjs_make_fetch_error("AbortError: fetch aborted");
    return Promise.reject(reason);
  }
  const headersJson = JSON.stringify([...st.headers]);
  return new Promise((resolve, reject) => {
    __wjs_fetch_start(st.url, st.method, headersJson, st.bodyU8 ?? undefined, resolve, reject);
  });
};
"#;

/// 在 global 上定义全部 native（prelude 求值之前）。
pub fn define_all(cx: &mut JSContext, global: *mut JSObject) -> Result<(), Error> {
    // SAFETY: cx 处于 global 所属 realm（调用方持 AutoRealm）；raw 调用不触发 GC。
    unsafe {
        let rcx = cx.raw_cx();
        let timers: &[(&str, JSNative, u32)] = &[
            ("__wjs_setTimeout", Some(timers::set_timeout), 3),
            ("__wjs_setInterval", Some(timers::set_interval), 3),
            ("__wjs_clearTimeout", Some(timers::clear_timeout), 1),
        ];
        for (name, native, nargs) in timers {
            let cname = CString::new(*name).expect("no NUL");
            if mozjs::jsapi::JS_DefineFunction(rcx, raw_handle(&global), cname.as_ptr(), *native, *nargs, 0)
                .is_null()
            {
                report_error(cx, "failed to define builtin");
                return Err(Error::Other(format!("failed to define builtin {name}")));
            }
        }

        // Phase 3a: URL 解析 / form 编解码 / base64 / 编码器 / 随机数
        // （prelude 真类 + 薄壳；复杂值走 JSON 桥；见各模块文档）
        let web: &[(&str, JSNative, u32)] = &[
            ("__wjs_url_parse", Some(url::url_parse), 2),
            ("__wjs_url_get", Some(url::url_get), 2),
            ("__wjs_url_set", Some(url::url_set), 3),
            ("__wjs_usp_parse", Some(url::usp_parse), 1),
            ("__wjs_usp_serialize", Some(url::usp_serialize), 1),
            ("__wjs_btoa", Some(encoding::btoa_encode), 1),
            ("__wjs_atob", Some(encoding::atob_decode), 1),
            ("__wjs_te_encode", Some(encoding::te_encode), 1),
            ("__wjs_te_encode_into", Some(encoding::te_encode_into), 2),
            ("__wjs_td_canonical", Some(encoding::td_canonical), 1),
            ("__wjs_td_decode", Some(encoding::td_decode), 4),
            ("__wjs_fill_random", Some(crypto::fill_random), 1),
            ("__wjs_random_uuid", Some(crypto::random_uuid), 0),
            ("__wjs_subtle_digest", Some(crypto::subtle_digest), 2),
            ("__wjs_fetch_start", Some(fetch::fetch_start), 6),
        ];
        for (name, native, nargs) in web {
            let cname = CString::new(*name).expect("no NUL");
            if mozjs::jsapi::JS_DefineFunction(rcx, raw_handle(&global), cname.as_ptr(), *native, *nargs, 0)
                .is_null()
            {
                report_error(cx, "failed to define builtin");
                return Err(Error::Other(format!("failed to define builtin {name}")));
            }
        }

        // console 对象 + 方法
        let console = mozjs::jsapi::JS_NewPlainObject(rcx);
        if console.is_null() {
            return Err(Error::Other("failed to create console object".into()));
        }
        rooted!(in(rcx) let console_root: *mut JSObject = console);
        let methods: &[(&str, JSNative, u32)] = &[
            ("log", Some(console::log), 0),
            ("info", Some(console::info), 0),
            ("warn", Some(console::warn), 0),
            ("error", Some(console::error), 0),
            ("debug", Some(console::debug), 0),
            ("trace", Some(console::trace), 0),
            ("dir", Some(console::dir), 0),
            ("assert", Some(console::assert), 0),
            ("count", Some(console::count), 1),
            ("countReset", Some(console::count_reset), 1),
            ("time", Some(console::time), 1),
            ("timeLog", Some(console::time_log), 1),
            ("timeEnd", Some(console::time_end), 1),
            ("group", Some(console::group), 0),
            ("groupEnd", Some(console::group_end), 0),
            ("clear", Some(console::clear), 0),
        ];
        for (name, native, nargs) in methods {
            let cname = CString::new(*name).expect("no NUL");
            if mozjs::jsapi::JS_DefineFunction(
                rcx,
                raw_handle(console_root.as_ptr()),
                cname.as_ptr(),
                *native,
                *nargs,
                0,
            )
            .is_null()
            {
                return Err(Error::Other(format!("failed to define console.{name}")));
            }
        }
        rooted!(in(rcx) let console_val = ObjectValue(console));
        // SAFETY: 定义 console 属性（5 参简化形态）
        let ok = mozjs::jsapi::JS_DefineProperty(
            rcx,
            raw_handle(&global),
            c"console".as_ptr(),
            raw_handle(console_val.as_ptr()),
            JSPROP_ENUMERATE as u32,
        );
        if !ok {
            return Err(Error::Other("failed to define global console".into()));
        }

        // structuredClone
        let cname = c"structuredClone";
        let clone_native: JSNative = Some(clone::structured_clone);
        if mozjs::jsapi::JS_DefineFunction(
            rcx,
            raw_handle(&global),
            cname.as_ptr(),
            clone_native,
            1,
            0,
        )
        .is_null()
        {
            return Err(Error::Other("failed to define structuredClone".into()));
        }
    }
    Ok(())
}
