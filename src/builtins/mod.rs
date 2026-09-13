//! 内建注册与 JS prelude。
//! prelude 用 JS 实现需要 Promise/参数打包语义的薄壳（queueMicrotask、timers 包装、
//! `__wjs_call`/`__wjs_entries` 辅助），native 只做 Rust 侧的活。

pub mod clone;
pub mod console;
pub mod crypto;
pub mod encoding;
pub mod fetch;
pub mod node;
pub mod timers;
pub mod url;
pub mod ws;
pub mod bun;

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
// Node 口径：`global` 为全局对象自引用别名（vite 等直引；9j 实测补齐）。
globalThis.global = globalThis;
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
// napi_call_function：recv 语义的参数展开（Function.prototype.apply）
globalThis.__wjs_napi_call = (recv, fn, args) => fn.apply(recv, args);
// napi_new_instance：`new ctor(...args)` 全语义（new.target/prototype/异常传播）
globalThis.__wjs_napi_new = (ctor, args) => new ctor(...args);
// napi_set_* 的非严格赋值面：JSAPI JS_SetProperty 是 strict 语义（对只读/
// 冻结属性抛 TypeError），Node 的 napi_set_property 走 v8 非严格 set（静默
// 无操作返回 ok）。sloppy 函数内的 `obj[key] = value` 与后者精确对齐。
globalThis.__wjs_napi_set = (obj, key, value) => { obj[key] = value; };
// napi Buffer 形状：Uint8Array + Buffer.prototype（Node 实例同款）；
// is_buffer 判定（instanceof Buffer；Buffer 缺席恒 false）
globalThis.__wjs_napi_bufferify = (u8) => {
  if (typeof Buffer !== "function") throw new TypeError("Buffer is not available");
  Object.setPrototypeOf(u8, Buffer.prototype);
  return u8;
};
globalThis.__wjs_napi_is_buffer =
  (v) => typeof Buffer === "function" && v instanceof Buffer;
// napi_define_class/define_properties 的访问器定义（setter 传 undefined =
// Node getter-only 语义：sloppy 赋值静默、strict TypeError）
globalThis.__wjs_napi_accessor =
  (obj, name, getter, setter, enumerable, configurable) =>
    Object.defineProperty(obj, name, { get: getter, set: setter, enumerable, configurable });
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
  #label; #fatal; #ignoreBOM; #streamId;
  constructor(label = "utf-8", options) {
    this.#label = __wjs_td_canonical(String(label));
    this.#fatal = !!(options && options.fatal);
    this.#ignoreBOM = !!(options && options.ignoreBOM);
    this.#streamId = undefined;
  }
  get encoding() { return this.#label; }
  get fatal() { return this.#fatal; }
  get ignoreBOM() { return this.#ignoreBOM; }
  decode(input, options) {
    let view = input;
    if (view === undefined) view = undefined;
    else if (view instanceof ArrayBuffer) view = new Uint8Array(view);
    else if (typeof SharedArrayBuffer !== "undefined" && view instanceof SharedArrayBuffer) {
      throw new TypeError("TextDecoder.decode does not accept SharedArrayBuffer views yet");
    } else if (ArrayBuffer.isView(view) && !(view instanceof Uint8Array)) {
      view = new Uint8Array(view.buffer, view.byteOffset, view.byteLength);
    }
    if (options && options.stream) {
      // 流式：有状态解码器攒截断序列（跨片多字节/stateful 编码正确）；
      // 中途 view 缺省视为空片（只推进状态，不收尾）。
      if (this.#streamId === undefined) {
        this.#streamId = __wjs_td_stream_open(this.#label, this.#fatal ? 1 : 0, this.#ignoreBOM ? 1 : 0);
      }
      return __wjs_td_stream_feed(this.#streamId, view, 0);
    }
    if (this.#streamId !== undefined) {
      // 非流式调用即收尾（含攒下的截断序列），id 自动回收
      const out = __wjs_td_stream_feed(this.#streamId, view, 1);
      this.#streamId = undefined;
      return out;
    }
    if (view === undefined) return __wjs_td_decode(this.#label, 0, 0, undefined);
    return __wjs_td_decode(this.#label, this.#fatal ? 1 : 0, this.#ignoreBOM ? 1 : 0, view);
  }
};
// ---- Buffer 全局（Node 子集；Uint8Array 子类，见头注口径）----
// 口径（文档记录）：from/alloc/concat/isBuffer/byteLength/toString(hex/base64/
// base64url/utf8/latin1/ascii/utf16le)；其余 TypedArray 行为全部继承；
// allocUnsafe 为零填（无未初始化内存暴露）；inspect 自定义；pool 概念无（直接分配）。
function __wjs_bufFromBytes(u8) {
  const b = new Buffer(u8.length);
  b.set(u8);
  return b;
}
function __wjs_bufDecode(str, enc) {
  enc = String(enc || "utf8").toLowerCase().replace(/[-_]/g, "");
  if (enc === "utf8" || enc === "utf-8") return new TextEncoder().encode(str);
  if (enc === "hex") {
    const s = String(str).replace(/\s+/g, "");
    if (s.length % 2 !== 0) throw new TypeError("Invalid hex string");
    const out = new Uint8Array(s.length / 2);
    for (let i = 0; i < out.length; i++) {
      const v = parseInt(s.slice(i * 2, i * 2 + 2), 16);
      if (Number.isNaN(v)) throw new TypeError("Invalid hex string");
      out[i] = v;
    }
    return out;
  }
  if (enc === "base64" || enc === "base64url") {
    let s = String(str).replace(/-/g, "+").replace(/_/g, "/");
    while (s.length % 4) s += "=";
    const bin = atob(s);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  }
  if (enc === "latin1" || enc === "binary") {
    const s = String(str);
    const out = new Uint8Array(s.length);
    for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i) & 255;
    return out;
  }
  if (enc === "ascii") {
    const s = String(str);
    const out = new Uint8Array(s.length);
    for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i) & 127;
    return out;
  }
  if (enc === "ucs2" || enc === "utf16le" || enc === "utf16") {
    const s = String(str);
    const out = new Uint8Array(s.length * 2);
    for (let i = 0; i < s.length; i++) {
      const c = s.charCodeAt(i);
      out[i * 2] = c & 255; out[i * 2 + 1] = (c >> 8) & 255;
    }
    return out;
  }
  throw new TypeError(`Unknown encoding: ${enc}`);
}
function __wjs_bufEncode(u8, enc) {
  enc = String(enc || "utf8").toLowerCase().replace(/[-_]/g, "");
  if (enc === "utf8" || enc === "utf-8") return new TextDecoder().decode(u8);
  if (enc === "hex") return [...u8].map((x) => x.toString(16).padStart(2, "0")).join("");
  if (enc === "base64") {
    let s = "";
    for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000));
    return btoa(s);
  }
  if (enc === "base64url") {
    let s = "";
    for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000));
    return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  }
  if (enc === "latin1" || enc === "binary") {
    let s = "";
    for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000));
    return s;
  }
  if (enc === "ascii") {
    let s = "";
    for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000));
    return s;
  }
  if (enc === "ucs2" || enc === "utf16le" || enc === "utf16") {
    let s = "";
    for (let i = 0; i + 1 < u8.length; i += 2) s += String.fromCharCode(u8[i] | (u8[i + 1] << 8));
    return s;
  }
  throw new TypeError(`Unknown encoding: ${enc}`);
}
globalThis.Buffer = class Buffer extends Uint8Array {
  static isBuffer(v) { return v instanceof Buffer; }
  static byteLength(s, enc) {
    if (typeof s === "string") return __wjs_bufDecode(s, enc).length;
    if (s instanceof ArrayBuffer) return s.byteLength;
    if (ArrayBuffer.isView(s)) return s.byteLength;
    throw new TypeError("byteLength: string or BufferSource required");
  }
  static from(v, enc) {
    if (typeof v === "string") return __wjs_bufFromBytes(__wjs_bufDecode(v, enc));
    if (Array.isArray(v)) return __wjs_bufFromBytes(new Uint8Array(v));
    if (v instanceof ArrayBuffer) return __wjs_bufFromBytes(new Uint8Array(v.slice(0)));
    if (ArrayBuffer.isView(v)) return __wjs_bufFromBytes(new Uint8Array(v.buffer, v.byteOffset, v.byteLength));
    throw new TypeError("Buffer.from: string/array/BufferSource required");
  }
  static alloc(size, fill, enc) {
    const n = Number(size);
    if (!Number.isInteger(n) || n < 0) throw new RangeError("Buffer.alloc: bad size");
    const b = new Buffer(n);
    if (fill !== undefined) {
      if (typeof fill === "string") {
        const pat = __wjs_bufDecode(fill, enc);
        if (pat.length) for (let i = 0; i < n; i++) b[i] = pat[i % pat.length];
      } else if (typeof fill === "number") {
        b.fill(fill & 255);
      } else if (fill instanceof Uint8Array || ArrayBuffer.isView(fill)) {
        const pat = new Uint8Array(fill.buffer, fill.byteOffset, fill.byteLength);
        if (pat.length) for (let i = 0; i < n; i++) b[i] = pat[i % pat.length];
      }
    }
    return b;
  }
  static allocUnsafe(size) {
    // 零填（无未初始化内存暴露，见头注）
    const n = Number(size);
    if (!Number.isInteger(n) || n < 0) throw new RangeError("Buffer.allocUnsafe: bad size");
    return new Buffer(n);
  }
  static allocUnsafeSlow(size) { return Buffer.allocUnsafe(size); }
  static concat(list, total) {
    if (!Array.isArray(list)) throw new TypeError("Buffer.concat: list must be an array");
    const parts = list.map((p) => {
      if (p instanceof Uint8Array) return p;
      if (ArrayBuffer.isView(p)) return new Uint8Array(p.buffer, p.byteOffset, p.byteLength);
      throw new TypeError("Buffer.concat: list must be Buffers");
    });
    const want = total === undefined ? parts.reduce((a, p) => a + p.length, 0) : Number(total);
    if (!Number.isInteger(want) || want < 0) throw new RangeError("Buffer.concat: bad totalLength");
    const out = new Buffer(Math.min(want, parts.reduce((a, p) => a + p.length, 0)));
    let off = 0;
    for (const p of parts) {
      if (off >= want) break;
      const n = Math.min(p.length, want - off);
      out.set(p.subarray(0, n), off);
      off += n;
    }
    return out;
  }
  static compare(a, b) {
    const x = Buffer.from(a), y = Buffer.from(b);
    const n = Math.min(x.length, y.length);
    for (let i = 0; i < n; i++) { if (x[i] !== y[i]) return x[i] < y[i] ? -1 : 1; }
    return x.length === y.length ? 0 : (x.length < y.length ? -1 : 1);
  }
  toString(enc, start, end) {
    const s = start === undefined ? 0 : Number(start);
    const e = end === undefined ? this.length : Number(end);
    return __wjs_bufEncode(this.subarray(s, e), enc);
  }
  subarray(begin, end) {
    // 保持 Buffer 类型（Node 口径），底层仍共享
    const v = super.subarray(begin, end);
    Object.setPrototypeOf(v, Buffer.prototype);
    return v;
  }
  slice(begin, end) { return this.subarray(begin, end); }
  write(str, offset, length, enc) {
    if (typeof offset === "string") { enc = offset; offset = 0; length = this.length; }
    else if (typeof length === "string") { enc = length; length = this.length; }
    offset = offset === undefined ? 0 : Number(offset);
    const src = __wjs_bufDecode(String(str), enc);
    const n = Math.min(src.length, length === undefined ? this.length - offset : Number(length), this.length - offset);
    if (offset < 0 || n < 0) throw new RangeError("Buffer.write: out of bounds");
    this.set(src.subarray(0, Math.max(0, n)), offset);
    return Math.max(0, n);
  }
  copy(target, tStart, sStart, sEnd) {
    if (!(target instanceof Uint8Array)) throw new TypeError("Buffer.copy: target must be a Buffer");
    tStart = tStart === undefined ? 0 : Number(tStart);
    sStart = sStart === undefined ? 0 : Number(sStart);
    sEnd = sEnd === undefined ? this.length : Number(sEnd);
    const n = Math.min(sEnd - sStart, target.length - tStart);
    if (n <= 0) return 0;
    target.set(this.subarray(sStart, sStart + n), tStart);
    return n;
  }
  equals(other) {
    const o = other instanceof Uint8Array ? other : Buffer.from(other);
    if (this.length !== o.length) return false;
    for (let i = 0; i < this.length; i++) if (this[i] !== o[i]) return false;
    return true;
  }
  compare(other) { return Buffer.compare(this, other); }
  toJSON() { return { type: "Buffer", data: [...this] }; }
};
const __wjs_keyState = new WeakMap();
function NotSupportedError_(what) { return new Error(`NotSupportedError: unsupported ${what}`); }
function __wjs_normHash(h) {
  const s = typeof h === "string" ? h : String(h?.name ?? "");
  const up = s.trim().toUpperCase();
  const map = { "SHA-1": "SHA-1", "SHA1": "SHA-1", "SHA-256": "SHA-256", "SHA256": "SHA-256", "SHA-384": "SHA-384", "SHA384": "SHA-384", "SHA-512": "SHA-512", "SHA512": "SHA-512" };
  if (!map[up]) throw new Error(`NotSupportedError: unsupported hash '${s}'`);
  return map[up];
}
function __wjs_makeKey(alg, material, usages, extractable, kind) {
  const k = Object.create(CryptoKey.prototype);
  __wjs_keyState.set(k, { alg, material, usages, extractable, kind: kind ?? "secret" });
  return k;
}
function __wjs_keyBytes(v) {
  if (v instanceof ArrayBuffer) return new Uint8Array(v);
  if (ArrayBuffer.isView(v)) return new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
  throw new TypeError("key data must be a BufferSource");
}
function __wjs_dataBytes(v) {
  if (typeof v === "string") return new TextEncoder().encode(v);
  return __wjs_keyBytes(v);
}
function __wjs_needUsage(st, op) {
  if (!st.usages.includes(op)) throw new Error(`InvalidAccessError: key cannot be used to ${op}`);
}
function __wjs_aesParams(algorithm) {
  const iv = __wjs_dataBytes(algorithm?.iv ?? new Uint8Array(0));
  if (iv.length !== 12) throw new Error("OperationError: AES-GCM iv must be 12 bytes");
  const aad = algorithm?.additionalData === undefined ? undefined : __wjs_dataBytes(algorithm.additionalData);
  const tagLength = algorithm?.tagLength === undefined ? 128 : Number(algorithm.tagLength);
  if (tagLength !== 128) throw new Error("NotSupportedError: only 128-bit AES-GCM tags for now");
  return { iv, aad };
}
function __wjs_b64urlEncode(u8) {
  let s = "";
  for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000));
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}
function __wjs_b64urlDecode(str) {
  str = String(str).replace(/-/g, "+").replace(/_/g, "/");
  while (str.length % 4) str += "=";
  const bin = atob(str);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}
globalThis.CryptoKey = class CryptoKey {
  constructor() { throw new TypeError("Illegal constructor"); }
  get algorithm() { return { ...__wjs_keyState.get(this)?.alg }; }
  get extractable() { return !!__wjs_keyState.get(this)?.extractable; }
  get type() { return __wjs_keyState.get(this)?.kind ?? "secret"; }
  get usages() { return [...(__wjs_keyState.get(this)?.usages ?? [])]; }
};
function __wjs_normCurve(c) {
  const s = String(c ?? "").trim().toUpperCase().replace("_", "-");
  const map = { "P-256": "P-256", "P256": "P-256", "P-384": "P-384", "P384": "P-384", "P-521": "P-521", "P521": "P-521" };
  if (!map[s]) throw new Error(`NotSupportedError: unsupported curve '${c}' (P-256/384/521)`);
  return map[s];
}
function __wjs_rsaPubExp(v) {
  if (v === undefined) return 65537;
  if (v instanceof Uint8Array) {
    let n = 0;
    for (const b of v) n = n * 256 + b;
    return n;
  }
  return Number(v);
}
function __wjs_x_bits(algorithm, st, length) {
  const pubKey = algorithm?.public;
  const pst = __wjs_keyState.get(pubKey);
  if (!pst || pst.alg.name !== "X25519" || pst.kind === "private") {
    throw new TypeError("deriveBits: algorithm.public must be an X25519 public key");
  }
  const secret = __wjs_x_derive(st.material, pst.material);
  if (length === undefined || length === null) return secret.buffer;
  const bits = Number(length);
  if (!Number.isInteger(bits) || bits < 0 || bits > secret.length * 8 || bits % 8 !== 0) {
    throw new Error("OperationError: bad X25519 deriveBits length");
  }
  return secret.slice(0, bits / 8).buffer;
}
function __wjs_ecdh_bits(algorithm, st, length) {
  const pubKey = algorithm?.public;
  const pst = __wjs_keyState.get(pubKey);
  if (!pst || pst.alg.name !== "ECDH" || pst.kind === "private") {
    throw new TypeError("deriveBits: algorithm.public must be an ECDH public key");
  }
  if (pst.alg.namedCurve !== st.alg.namedCurve) throw new Error("InvalidAccessError: ECDH curves differ");
  const secret = __wjs_ecdh_derive(st.alg.namedCurve, st.material, pst.material);
  if (length === undefined || length === null) return secret.buffer;
  const bits = Number(length);
  if (!Number.isInteger(bits) || bits < 0 || bits > secret.length * 8 || bits % 8 !== 0) {
    throw new Error("OperationError: bad ECDH deriveBits length");
  }
  return secret.slice(0, bits / 8).buffer;
}
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
    async generateKey(alg, extractable, usages) {
      const name = typeof alg === "string" ? alg.toUpperCase() : String(alg?.name ?? "").toUpperCase();
      usages = [...(usages ?? [])].map(String);
      if (name === "AES-GCM") {
        const length = Number(alg?.length ?? 256);
        if (![128, 192, 256].includes(length)) throw new Error("NotSupportedError: AES-GCM length must be 128/192/256");
        const bytes = new Uint8Array(length / 8);
        crypto.getRandomValues(bytes);
        return __wjs_makeKey({ name: "AES-GCM", length }, bytes, usages, !!extractable);
      }
      if (name === "HMAC") {
        const hash = __wjs_normHash(alg?.hash);
        let length = alg?.length === undefined ? null : Number(alg.length);
        const outLen = { "SHA-1": 160, "SHA-256": 256, "SHA-384": 384, "SHA-512": 512 }[hash];
        if (length === null) length = outLen;
        if (!Number.isInteger(length) || length <= 0 || length > 1024 * 1024) {
          throw new Error("NotSupportedError: bad HMAC length");
        }
        const bytes = new Uint8Array(Math.ceil(length / 8));
        crypto.getRandomValues(bytes);
        return __wjs_makeKey({ name: "HMAC", hash, length }, bytes, usages, !!extractable);
      }
      if (name === "RSASSA-PKCS1-V1_5" || name === "RSA-OAEP" || name === "RSA-PSS") {
        const length = Number(alg?.modulusLength ?? 2048);
        if (![2048, 3072, 4096].includes(length)) throw new Error("NotSupportedError: RSA modulusLength must be 2048/3072/4096");
        const e = __wjs_rsaPubExp(alg?.publicExponent);
        if (!Number.isInteger(e) || e < 2 || e > 2 ** 33 - 1) throw new Error("DataError: bad RSA publicExponent");
        const hash = __wjs_normHash(alg?.hash ?? "SHA-256");
        const privDer = __wjs_rsa_generate(length, e);
        const pubDer = __wjs_rsa_public(privDer);
        const expBytes = (() => { const out = []; let n = e; do { out.unshift(n & 255); n = Math.floor(n / 256); } while (n > 0); return new Uint8Array(out); })();
        const keyAlg = { name, modulusLength: length, publicExponent: expBytes, hash };
        const mkPub = __wjs_makeKey(keyAlg, pubDer, usages, !!extractable, "public");
        const mkPriv = __wjs_makeKey(keyAlg, privDer, usages, !!extractable, "private");
        return { publicKey: mkPub, privateKey: mkPriv };
      }
      if (name === "ECDSA" || name === "ECDH") {
        const curve = __wjs_normCurve(alg?.namedCurve);
        const privDer = __wjs_ec_generate(curve);
        const pubDer = __wjs_ec_public(curve, privDer);
        const mkPub = __wjs_makeKey({ name, namedCurve: curve }, pubDer, usages, !!extractable, "public");
        const mkPriv = __wjs_makeKey({ name, namedCurve: curve }, privDer, usages, !!extractable, "private");
        return { publicKey: mkPub, privateKey: mkPriv };
      }
      if (name === "ED25519") {
        const seed = __wjs_ed_generate();
        const pub = __wjs_ed_public(seed);
        const mkPub = __wjs_makeKey({ name, namedCurve: "Ed25519" }, pub, usages, !!extractable, "public");
        const mkPriv = __wjs_makeKey({ name, namedCurve: "Ed25519" }, seed, usages, !!extractable, "private");
        return { publicKey: mkPub, privateKey: mkPriv };
      }
      if (name === "X25519") {
        const priv = __wjs_x_generate();
        const pub = __wjs_x_public(priv);
        const mkPub = __wjs_makeKey({ name, namedCurve: "X25519" }, pub, usages, !!extractable, "public");
        const mkPriv = __wjs_makeKey({ name, namedCurve: "X25519" }, priv, usages, !!extractable, "private");
        return { publicKey: mkPub, privateKey: mkPriv };
      }
      if (name === "ED448") {
        throw new Error(`NotSupportedError: generateKey ${name} needs follow-up`);
      }
      throw new Error(`NotSupportedError: generateKey ${name} needs Phase 3 c-4`);
    },
    async importKey(format, keyData, alg, extractable, usages) {
      const name = typeof alg === "string" ? alg.toUpperCase() : String(alg?.name ?? "").toUpperCase();
      usages = [...(usages ?? [])].map(String);
      const needHash = name === "HMAC" ? __wjs_normHash(alg?.hash) : undefined;
      if (name === "RSASSA-PKCS1-V1_5" || name === "RSA-OAEP" || name === "RSA-PSS") {
        const hash = __wjs_normHash(alg?.hash ?? "SHA-256");
        if (format === "jwk") {
          if (!keyData || keyData.kty !== "RSA" || typeof keyData.n !== "string" || typeof keyData.e !== "string") {
            throw new Error("DataError: bad RSA JWK (n/e)");
          }
          const n = __wjs_b64urlDecode(keyData.n), e = __wjs_b64urlDecode(keyData.e);
          const expBytes = e.slice();
          if (typeof keyData.d === "string") {
            const d = __wjs_b64urlDecode(keyData.d);
            const privDer = __wjs_rsa_import_priv(n, e, d);
            const keyAlg = { name, modulusLength: n.length * 8, publicExponent: expBytes, hash };
            return __wjs_makeKey(keyAlg, privDer, usages, !!extractable, "private");
          }
          const pubDer = __wjs_rsa_import_pub(n, e);
          const keyAlg = { name, modulusLength: n.length * 8, publicExponent: expBytes, hash };
          return __wjs_makeKey(keyAlg, pubDer, usages, !!extractable, "public");
        }
        if (format === "pkcs8") {
          const privDer = __wjs_keyBytes(keyData);
          // PKCS#8 自验证（解析失败即 DataError；公钥顺带导出供 algorithm）。
          const pubDer = __wjs_rsa_public(privDer);
          const parts = JSON.parse(__wjs_rsa_jwk(privDer, pubDer));
          const n = __wjs_b64urlDecode(parts.n), e = __wjs_b64urlDecode(parts.e);
          const keyAlg = { name, modulusLength: n.length * 8, publicExponent: e.slice(), hash };
          return __wjs_makeKey(keyAlg, privDer, usages, !!extractable, "private");
        }
        if (format === "spki") {
          const pubDer = __wjs_keyBytes(keyData);
          const parts = JSON.parse(__wjs_rsa_jwk_pub(pubDer));
          const n = __wjs_b64urlDecode(parts.n), e = __wjs_b64urlDecode(parts.e);
          const keyAlg = { name, modulusLength: n.length * 8, publicExponent: e.slice(), hash };
          return __wjs_makeKey(keyAlg, pubDer, usages, !!extractable, "public");
        }
        throw new Error(`NotSupportedError: importKey ${format} for RSA needs pkcs8/spki/jwk`);
      }
      if (name === "ECDSA" || name === "ECDH") {
        const curve = __wjs_normCurve(alg?.namedCurve);
        const keyAlg = { name, namedCurve: curve };
        if (format === "jwk") {
          if (!keyData || keyData.kty !== "EC" || keyData.crv !== curve
            || typeof keyData.x !== "string" || typeof keyData.y !== "string") {
            throw new Error("DataError: bad EC JWK (x/y/crv)");
          }
          const x = __wjs_b64urlDecode(keyData.x), y = __wjs_b64urlDecode(keyData.y);
          if (typeof keyData.d === "string") {
            const privDer = __wjs_ec_import_priv(curve, __wjs_b64urlDecode(keyData.d));
            // 公钥一致性：JWK 的 x/y 须与 d 对应（防混入）。
            const expect = __wjs_ec_public(curve, privDer);
            const got = __wjs_ec_import_pub(curve, x, y);
            const same = expect.length === got.length && expect.every((b, i) => b === got[i]);
            if (!same) throw new Error("DataError: EC JWK x/y does not match d");
            return __wjs_makeKey(keyAlg, privDer, usages, !!extractable, "private");
          }
          return __wjs_makeKey(keyAlg, __wjs_ec_import_pub(curve, x, y), usages, !!extractable, "public");
        }
        if (format === "pkcs8") {
          const privDer = __wjs_keyBytes(keyData);
          // PKCS#8 自验证（曲线错/损坏即 DataError）。
          __wjs_ec_public(curve, privDer);
          return __wjs_makeKey(keyAlg, privDer, usages, !!extractable, "private");
        }
        if (format === "spki") {
          const pubDer = __wjs_keyBytes(keyData);
          // SPKI 自验证（曲线错即 DataError）。
          __wjs_ec_jwk_pub(curve, pubDer);
          return __wjs_makeKey(keyAlg, pubDer, usages, !!extractable, "public");
        }
        if (format === "raw") {
          const v = __wjs_keyBytes(keyData);
          if (v.length < 1 || v[0] !== 0x04) throw new Error("DataError: EC raw public must be uncompressed (0x04‖x‖y)");
          const size = (v.length - 1) / 2;
          if (![32, 48, 66].includes(size) || 1 + 2 * size !== v.length) throw new Error("DataError: bad EC raw length");
          const x = v.slice(1, 1 + size), y = v.slice(1 + size);
          const c2 = size === 32 ? "P-256" : size === 48 ? "P-384" : "P-521";
          if (c2 !== curve) throw new Error("DataError: EC raw length does not match namedCurve");
          return __wjs_makeKey(keyAlg, __wjs_ec_import_pub(curve, x, y), usages, !!extractable, "public");
        }
        throw new Error(`NotSupportedError: importKey ${format} for EC needs jwk/pkcs8/spki/raw`);
      }
      if (name === "ED25519" || name === "X25519") {
        // JWK crv 用混合大小写（RFC 8037；内部 name 全大写，不外泄）。
        const crv = name === "ED25519" ? "Ed25519" : "X25519";
        const keyAlg = { name, namedCurve: crv };
        if (format === "jwk") {
          if (!keyData || keyData.kty !== "OKP" || keyData.crv !== crv
            || typeof keyData.x !== "string") {
            throw new Error("DataError: bad OKP JWK (kty/crv/x)");
          }
          const x = __wjs_b64urlDecode(keyData.x);
          if (x.length !== 32) throw new Error("DataError: bad OKP JWK (x length)");
          if (typeof keyData.d === "string") {
            const seed = __wjs_b64urlDecode(keyData.d);
            if (seed.length !== 32) throw new Error("DataError: bad OKP JWK (d length)");
            // 私钥一致性：JWK 的 x 须与 d 对应（防混入）。
            const expect = name === "ED25519" ? __wjs_ed_public(seed) : __wjs_x_public(seed);
            const same = expect.length === 32 && expect.every((b, i) => b === x[i]);
            if (!same) throw new Error("DataError: OKP JWK x does not match d");
            return __wjs_makeKey(keyAlg, seed, usages, !!extractable, "private");
          }
          return __wjs_makeKey(keyAlg, x, usages, !!extractable, "public");
        }
        if (format === "raw") {
          const v = __wjs_keyBytes(keyData);
          if (v.length !== 32) throw new Error("DataError: OKP raw key must be 32 bytes");
          return __wjs_makeKey(keyAlg, v, usages, !!extractable, "public");
        }
        if (format === "pkcs8") {
          const seed = __wjs_okp_seed_from_pkcs8(name, __wjs_keyBytes(keyData));
          return __wjs_makeKey(keyAlg, seed, usages, !!extractable, "private");
        }
        if (format === "spki") {
          const publ = __wjs_okp_pub_from_spki(name, __wjs_keyBytes(keyData));
          return __wjs_makeKey(keyAlg, publ, usages, !!extractable, "public");
        }
        throw new Error(`NotSupportedError: importKey ${format} for OKP needs jwk/raw/pkcs8/spki`);
      }
      let bytes;
      if (format === "raw") {
        bytes = __wjs_keyBytes(keyData);
      } else if (format === "jwk") {
        if (!keyData || keyData.kty !== "oct" || typeof keyData.k !== "string") {
          throw new Error("NotSupportedError: only oct JWK keys for now");
        }
        bytes = __wjs_b64urlDecode(keyData.k);
      } else throw new Error(`NotSupportedError: importKey ${format} needs Phase 3 c-4`);
      if (name === "AES-GCM") {
        if (![16, 24, 32].includes(bytes.length)) throw new TypeError("AES-GCM raw key must be 16/24/32 bytes");
        return __wjs_makeKey({ name, length: bytes.length * 8 }, bytes, usages, !!extractable);
      }
      if (name === "HMAC") {
        return __wjs_makeKey({ name, hash: needHash, length: bytes.length * 8 }, bytes, usages, !!extractable);
      }
      throw new Error(`NotSupportedError: importKey ${name} needs Phase 3 c-4`);
    },
    async exportKey(format, key) {
      const st = __wjs_keyState.get(key);
      if (!st) throw new TypeError("exportKey: not a CryptoKey");
      if (!st.extractable) throw new Error("InvalidAccessError: key is not extractable");
      const aname = st.alg.name;
      if (aname === "RSASSA-PKCS1-V1_5" || aname === "RSA-OAEP" || aname === "RSA-PSS") {
        const hash = st.alg.hash ?? "SHA-256";
        const isPriv = st.kind === "private";
        const privDer = isPriv ? st.material : null;
        const pubDer = isPriv ? __wjs_rsa_public(st.material) : st.material;
        if (format === "pkcs8") {
          if (!isPriv) throw new Error("InvalidAccessError: not a private key");
          return st.material.slice().buffer;
        }
        if (format === "spki") {
          return pubDer.slice().buffer;
        }
        if (format === "jwk") {
          const parts = isPriv
            ? JSON.parse(__wjs_rsa_jwk(privDer, pubDer))
            : JSON.parse(__wjs_rsa_jwk_pub(pubDer));
          const jwk = { kty: "RSA", n: parts.n, e: parts.e };
          if (isPriv) { jwk.d = parts.d; jwk.p = parts.p; jwk.q = parts.q; jwk.dp = parts.dp; jwk.dq = parts.dq; jwk.qi = parts.qi; }
          jwk.alg = aname === "RSASSA-PKCS1-V1_5"
            ? { "SHA-256": "RS256", "SHA-384": "RS384", "SHA-512": "RS512" }[hash] ?? "RS256"
            : aname === "RSA-PSS"
              ? { "SHA-256": "PS256", "SHA-384": "PS384", "SHA-512": "PS512" }[hash] ?? "PS256"
              : { "SHA-256": "RSA-OAEP", "SHA-384": "RSA-OAEP-384", "SHA-512": "RSA-OAEP-512" }[hash] ?? "RSA-OAEP";
          jwk.ext = true;
          return jwk;
        }
        throw new Error(`NotSupportedError: exportKey ${format} for RSA needs pkcs8/spki/jwk`);
      }
      if (aname === "ECDSA" || aname === "ECDH") {
        const curve = st.alg.namedCurve;
        const isPriv = st.kind === "private";
        const pubDer = isPriv ? __wjs_ec_public(curve, st.material) : st.material;
        if (format === "pkcs8") {
          if (!isPriv) throw new Error("InvalidAccessError: not a private key");
          return st.material.slice().buffer;
        }
        if (format === "spki") {
          return pubDer.slice().buffer;
        }
        if (format === "jwk") {
          const parts = isPriv
            ? JSON.parse(__wjs_ec_jwk(curve, st.material, pubDer))
            : JSON.parse(__wjs_ec_jwk_pub(curve, pubDer));
          const jwk = { kty: "EC", crv: curve, x: parts.x, y: parts.y };
          if (isPriv) jwk.d = parts.d;
          jwk.ext = true;
          return jwk;
        }
        if (format === "raw") {
          if (isPriv) throw new Error("InvalidAccessError: raw export needs a public key");
          const parts = JSON.parse(__wjs_ec_jwk_pub(curve, pubDer));
          const x = __wjs_b64urlDecode(parts.x), y = __wjs_b64urlDecode(parts.y);
          const out = new Uint8Array(1 + x.length + y.length);
          out[0] = 0x04; out.set(x, 1); out.set(y, 1 + x.length);
          return out.buffer;
        }
        throw new Error(`NotSupportedError: exportKey ${format} for EC needs pkcs8/spki/jwk/raw`);
      }
      if (aname === "ED25519" || aname === "X25519") {
        const isPriv = st.kind === "private";
        const pubBytes = isPriv
          ? (aname === "ED25519" ? __wjs_ed_public(st.material) : __wjs_x_public(st.material))
          : st.material;
        if (format === "pkcs8") {
          if (!isPriv) throw new Error("InvalidAccessError: not a private key");
          return __wjs_okp_pkcs8_from_seed(aname, st.material).buffer;
        }
        if (format === "spki") {
          return __wjs_okp_spki_from_pub(aname, pubBytes).buffer;
        }
        if (format === "jwk") {
          const jwk = { kty: "OKP", crv: aname === "ED25519" ? "Ed25519" : "X25519", x: __wjs_b64urlEncode(pubBytes) };
          if (isPriv) jwk.d = __wjs_b64urlEncode(st.material);
          // JWA 只给 Ed25519 定义了 "EdDSA"；X25519 无 alg（与 Node 一致，省略）。
          if (aname === "ED25519") jwk.alg = "EdDSA";
          jwk.ext = true;
          return jwk;
        }
        if (format === "raw") {
          if (isPriv) throw new Error("InvalidAccessError: raw export needs a public key");
          return pubBytes.slice().buffer;
        }
        throw new Error(`NotSupportedError: exportKey ${format} for OKP needs pkcs8/spki/jwk/raw`);
      }
      if (format === "raw") return st.material.slice().buffer;
      if (format === "jwk") {
        return { kty: "oct", k: __wjs_b64urlEncode(st.material), alg: st.alg.name === "AES-GCM" ? `A${st.alg.length}GCM` : `HS${st.alg.hash.split("-")[1]}`, ext: true };
      }
      throw new Error(`NotSupportedError: exportKey ${format} needs Phase 3 c-4`);
    },
    async encrypt(algorithm, key, data) {
      const st = __wjs_keyState.get(key);
      if (!st) throw new TypeError("encrypt: not a CryptoKey");
      __wjs_needUsage(st, "encrypt");
      if (st.alg.name === "AES-GCM") {
        const p = __wjs_aesParams(algorithm);
        const out = __wjs_aesgcm_encrypt(st.material, p.iv, p.aad, __wjs_dataBytes(data));
        return out.buffer;
      }
      if (st.alg.name === "RSA-OAEP") {
        if (st.kind !== "public") throw new TypeError("encrypt: not an RSA public key");
        const hash = __wjs_normHash(algorithm?.hash ?? st.alg.hash ?? "SHA-256");
        const label = algorithm?.label === undefined ? undefined : __wjs_dataBytes(algorithm.label);
        const out = __wjs_rsa_encrypt(hash, st.material, __wjs_dataBytes(data), label);
        return out.buffer;
      }
      throw new TypeError("encrypt: unsupported key");
    },
    async decrypt(algorithm, key, data) {
      const st = __wjs_keyState.get(key);
      if (!st) throw new TypeError("decrypt: not a CryptoKey");
      __wjs_needUsage(st, "decrypt");
      if (st.alg.name === "AES-GCM") {
        const p = __wjs_aesParams(algorithm);
        const out = __wjs_aesgcm_decrypt(st.material, p.iv, p.aad, __wjs_dataBytes(data));
        return out.buffer;
      }
      if (st.alg.name === "RSA-OAEP") {
        if (st.kind !== "private") throw new TypeError("decrypt: not an RSA private key");
        const hash = __wjs_normHash(algorithm?.hash ?? st.alg.hash ?? "SHA-256");
        const label = algorithm?.label === undefined ? undefined : __wjs_dataBytes(algorithm.label);
        const out = __wjs_rsa_decrypt(hash, st.material, __wjs_dataBytes(data), label);
        return out.buffer;
      }
      throw new TypeError("decrypt: unsupported key");
    },
    async sign(algorithm, key, data) {
      const st = __wjs_keyState.get(key);
      if (!st) throw new TypeError("sign: not a CryptoKey");
      __wjs_needUsage(st, "sign");
      if (st.alg.name === "HMAC") {
        const out = __wjs_hmac_sign(st.alg.hash, st.material, __wjs_dataBytes(data));
        return out.buffer;
      }
      if (st.alg.name === "RSASSA-PKCS1-V1_5") {
        if (st.kind !== "private") throw new TypeError("sign: not an RSA private key");
        const hash = __wjs_normHash(algorithm?.hash ?? st.alg.hash ?? "SHA-256");
        const out = __wjs_rsa_sign(hash, st.material, __wjs_dataBytes(data));
        return out.buffer;
      }
      if (st.alg.name === "RSA-PSS") {
        if (st.kind !== "private") throw new TypeError("sign: not an RSA private key");
        const hash = __wjs_normHash(algorithm?.hash ?? st.alg.hash ?? "SHA-256");
        // 缺省 saltLength = digest 长度（WebCrypto 口径）。
        const defSalt = { "SHA-256": 32, "SHA-384": 48, "SHA-512": 64 }[hash];
        const salt = algorithm?.saltLength === undefined ? defSalt : Number(algorithm.saltLength);
        if (!Number.isInteger(salt) || salt < 0) throw new Error("OperationError: bad RSA-PSS saltLength");
        const out = __wjs_pss_sign(hash, salt, st.material, __wjs_dataBytes(data));
        return out.buffer;
      }
      if (st.alg.name === "ED25519") {
        if (st.kind !== "private") throw new TypeError("sign: not an Ed25519 private key");
        const out = __wjs_ed_sign(st.material, __wjs_dataBytes(data));
        return out.buffer;
      }
      if (st.alg.name === "ECDSA") {
        if (st.kind !== "private") throw new TypeError("sign: not an EC private key");
        const hash = __wjs_normHash(algorithm?.hash ?? "SHA-256");
        const out = __wjs_ecdsa_sign(st.alg.namedCurve, hash, st.material, __wjs_dataBytes(data));
        return out.buffer;
      }
      throw new TypeError("sign: unsupported key");
    },
    async verify(algorithm, key, signature, data) {
      const st = __wjs_keyState.get(key);
      if (!st) throw new TypeError("verify: not a CryptoKey");
      __wjs_needUsage(st, "verify");
      if (st.alg.name === "HMAC") {
        return __wjs_hmac_verify(st.alg.hash, st.material, __wjs_dataBytes(signature), __wjs_dataBytes(data));
      }
      if (st.alg.name === "RSASSA-PKCS1-V1_5") {
        if (st.kind === "private") throw new TypeError("verify: not an RSA public key");
        const hash = __wjs_normHash(algorithm?.hash ?? st.alg.hash ?? "SHA-256");
        return __wjs_rsa_verify(hash, st.material, __wjs_dataBytes(signature), __wjs_dataBytes(data));
      }
      if (st.alg.name === "RSA-PSS") {
        if (st.kind === "private") throw new TypeError("verify: not an RSA public key");
        const hash = __wjs_normHash(algorithm?.hash ?? st.alg.hash ?? "SHA-256");
        const defSalt = { "SHA-256": 32, "SHA-384": 48, "SHA-512": 64 }[hash];
        const salt = algorithm?.saltLength === undefined ? defSalt : Number(algorithm.saltLength);
        if (!Number.isInteger(salt) || salt < 0) throw new Error("OperationError: bad RSA-PSS saltLength");
        return __wjs_pss_verify(hash, salt, st.material, __wjs_dataBytes(signature), __wjs_dataBytes(data));
      }
      if (st.alg.name === "ED25519") {
        if (st.kind === "private") throw new TypeError("verify: not an Ed25519 public key");
        return __wjs_ed_verify(st.material, __wjs_dataBytes(signature), __wjs_dataBytes(data));
      }
      if (st.alg.name === "ECDSA") {
        if (st.kind === "private") throw new TypeError("verify: not an EC public key");
        const hash = __wjs_normHash(algorithm?.hash ?? "SHA-256");
        return __wjs_ecdsa_verify(st.alg.namedCurve, hash, st.material, __wjs_dataBytes(signature), __wjs_dataBytes(data));
      }
      throw new TypeError("verify: unsupported key");
    },
    async deriveBits(algorithm, baseKey, length) {
      const st = __wjs_keyState.get(baseKey);
      if (!st || (st.alg.name !== "ECDH" && st.alg.name !== "X25519")) {
        throw new TypeError("deriveBits: not an ECDH/X25519 key");
      }
      if (st.kind !== "private") throw new TypeError("deriveBits: needs a private key");
      __wjs_needUsage(st, "deriveBits");
      if (st.alg.name === "X25519") return __wjs_x_bits(algorithm, st, length);
      return __wjs_ecdh_bits(algorithm, st, length);
    },
    async deriveKey(algorithm, baseKey, derivedKeyAlg, extractable, usages) {
      const st = __wjs_keyState.get(baseKey);
      if (!st || (st.alg.name !== "ECDH" && st.alg.name !== "X25519")) {
        throw new TypeError("deriveKey: not an ECDH/X25519 key");
      }
      if (st.kind !== "private") throw new TypeError("deriveKey: needs a private key");
      __wjs_needUsage(st, "deriveKey");
      const bitsOf = (length) => st.alg.name === "X25519"
        ? __wjs_x_bits(algorithm, st, length)
        : __wjs_ecdh_bits(algorithm, st, length);
      const dname = String(derivedKeyAlg?.name ?? "").toUpperCase();
      let bytes;
      if (dname === "AES-GCM") {
        const length = Number(derivedKeyAlg?.length ?? 256);
        if (![128, 192, 256].includes(length)) throw new Error("NotSupportedError: derived AES-GCM length must be 128/192/256");
        bytes = new Uint8Array(bitsOf(length));
        return __wjs_makeKey({ name: "AES-GCM", length }, bytes, [...(usages ?? [])].map(String), !!extractable);
      }
      if (dname === "HMAC") {
        const hash = __wjs_normHash(derivedKeyAlg?.hash);
        let length = derivedKeyAlg?.length === undefined ? null : Number(derivedKeyAlg.length);
        bytes = new Uint8Array(bitsOf(length));
        if (length === null) length = bytes.length * 8;
        return __wjs_makeKey({ name: "HMAC", hash, length }, bytes, [...(usages ?? [])].map(String), !!extractable);
      }
      throw new Error(`NotSupportedError: deriveKey to ${dname || "?"} needs follow-up`);
    },
  },
};
// ---- M5: 全局 Event / EventTarget / CustomEvent（Node 平坦派发口径）----
// Node 的 EventTarget 不实现捕获/冒泡 propagation path（官方文档明言）：
// capture 选项仅为 removeEventListener 匹配保留；listener 收函数或 {handleEvent}。
// 事件状态走共享 WeakMap（Event 与 EventTarget 跨类要读写字段，# 私有够不着；
// 与既有 __wjs_abortState 同风格，前缀避免污染全局面）。
const __wjs_eventState = new WeakMap();
const __wjs_etState = new WeakMap();
globalThis.Event = class Event {
  constructor(type, options = {}) {
    if (arguments.length === 0) throw new TypeError("Event requires at least 1 argument, but only 0 were passed");
    const o = options ?? {};
    __wjs_eventState.set(this, {
      type: String(type),
      bubbles: !!o.bubbles,
      cancelable: !!o.cancelable,
      composed: !!o.composed,
      defaultPrevented: false,
      stopped: false,
      immediate: false,
      dispatching: false,
      timeStamp: Date.now(),
      target: null,
      currentTarget: null,
    });
  }
  get type() { return __wjs_eventState.get(this).type; }
  get bubbles() { return __wjs_eventState.get(this).bubbles; }
  get cancelable() { return __wjs_eventState.get(this).cancelable; }
  get composed() { return __wjs_eventState.get(this).composed; }
  get timeStamp() { return __wjs_eventState.get(this).timeStamp; }
  get defaultPrevented() { return __wjs_eventState.get(this).defaultPrevented; }
  get target() { return __wjs_eventState.get(this).target; }
  get currentTarget() { return __wjs_eventState.get(this).currentTarget; }
  get srcElement() { return __wjs_eventState.get(this).target; }
  get isTrusted() { return false; }
  preventDefault() {
    const s = __wjs_eventState.get(this);
    if (s.cancelable) s.defaultPrevented = true;
  }
  stopPropagation() { __wjs_eventState.get(this).stopped = true; }
  stopImmediatePropagation() {
    const s = __wjs_eventState.get(this);
    s.stopped = true;
    s.immediate = true;
  }
};
globalThis.CustomEvent = class CustomEvent extends Event {
  #detail;
  constructor(type, options = {}) {
    super(type, options);
    this.#detail = (options ?? {}).detail ?? null;
  }
  get detail() { return this.#detail; }
};
globalThis.EventTarget = class EventTarget {
  constructor() {
    __wjs_etState.set(this, new Map());
  }
  addEventListener(type, listener, options = {}) {
    if (arguments.length < 2) throw new TypeError("addEventListener requires at least 2 arguments");
    if (typeof listener !== "function" && (typeof listener !== "object" || listener === null || typeof listener.handleEvent !== "function")) {
      throw new TypeError("addEventListener: listener must be a function or an object with handleEvent");
    }
    const o = typeof options === "boolean" ? { capture: options } : (options ?? {});
    if (o.signal?.aborted) return;
    const st = __wjs_etState.get(this);
    const key = String(type);
    const list = st.get(key) ?? [];
    if (list.some((e) => e.listener === listener && e.capture === !!o.capture)) return;
    const entry = { listener, once: !!o.once, capture: !!o.capture, signal: o.signal ?? null, removed: false };
    list.push(entry);
    st.set(key, list);
    if (o.signal) o.signal.addEventListener("abort", () => this.removeEventListener(key, listener, options), { once: true });
  }
  removeEventListener(type, listener, options = {}) {
    const o = typeof options === "boolean" ? { capture: options } : (options ?? {});
    const st = __wjs_etState.get(this);
    if (!st) return;
    const list = st.get(String(type));
    if (!list) return;
    const i = list.findIndex((e) => e.listener === listener && e.capture === !!o.capture && !e.removed);
    if (i >= 0) {
      list[i].removed = true;
      list.splice(i, 1);
    }
  }
  dispatchEvent(event) {
    if (!(event instanceof Event)) throw new TypeError("dispatchEvent requires an Event instance");
    const es = __wjs_eventState.get(event);
    if (es.dispatching) throw new Error("InvalidStateError: event is already being dispatched");
    const st = __wjs_etState.get(this);
    if (!st) throw new TypeError("dispatchEvent called on non-EventTarget");
    es.target = this;
    es.dispatching = true;
    const list = (st.get(es.type) ?? []).slice();
    try {
      for (const entry of list) {
        if (es.immediate || entry.removed) continue;
        if (entry.signal?.aborted) continue;
        if (entry.once) this.removeEventListener(es.type, entry.listener, { capture: entry.capture });
        es.currentTarget = this;
        if (typeof entry.listener === "function") {
          entry.listener.call(this, event);
        } else {
          entry.listener.handleEvent(event);
        }
      }
    } finally {
      es.dispatching = false;
      es.currentTarget = null;
    }
    return !(es.cancelable && es.defaultPrevented);
  }
};

// ---- Phase 3b: Headers / Request / Response / fetch ----
// AbortSignal 重构到全局 EventTarget 基类（Node 同构：signal 即 EventTarget，
// abort 走 dispatchEvent；监听登记/移除/once/signal 选项全由基类承载）。
const __wjs_abortState = new WeakMap();
function __wjs_abortFire(signal, reason) {
  const st = __wjs_abortState.get(signal);
  if (!st || st.aborted) return;
  st.aborted = true;
  st.reason = reason === undefined ? new Error("AbortError: signal aborted") : reason;
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
    __wjs_abortState.set(this, { aborted: false, reason: undefined, onabort: null });
  }
  get aborted() { return __wjs_abortState.get(this).aborted; }
  get reason() { return __wjs_abortState.get(this).reason; }
  get onabort() { return __wjs_abortState.get(this).onabort; }
  set onabort(cb) { __wjs_abortState.get(this).onabort = typeof cb === "function" ? cb : null; }
  throwIfAborted() {
    const st = __wjs_abortState.get(this);
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
    setTimeout(() => c.abort(new Error("TimeoutError: signal timed out")), t);
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
    streamId: meta.streamId === undefined ? null : meta.streamId,
  });
  return resp;
};
globalThis.__wjs_make_fetch_error = (msg) => new Error(String(msg));
globalThis.__wjs_make_ws_event = (kind, json, binU8, target) => {
  const meta = JSON.parse(json);
  if (kind === "open") return { type: "open", target, protocol: meta.protocol ?? "" };
  if (kind === "message-text") return { type: "message", target, data: meta.text };
  if (kind === "message-bin") return { type: "message", target, data: binU8.buffer };
  if (kind === "close") {
    return { type: "close", target, code: meta.code, reason: meta.reason, wasClean: !!meta.clean };
  }
  return { type: "error", target, message: meta.message };
};
const __wjs_wsObjs = new Map();
globalThis.__wjs_ws_emit = (id, prop, kind, json, binU8) => {
  const t = __wjs_wsObjs.get(id);
  if (!t) return;
  const st = __wjs_wskState.get(t);
  const event = globalThis.__wjs_make_ws_event(kind, json, binU8, t);
  if (prop === "onopen") {
    st.readyState = 1;
    if (event.protocol) st.protocol = event.protocol;
  }
  if (prop === "onclose") {
    st.readyState = 3;
    __wjs_wsObjs.delete(id);
  }
  const h = t[prop];
  if (typeof h === "function") h.call(t, event);
};
const __wjs_wskState = new WeakMap();
globalThis.WebSocket = class WebSocket {
  static CONNECTING = 0; static OPEN = 1; static CLOSING = 2; static CLOSED = 3;
  constructor(url, protocols) {
    let protos = [];
    if (protocols !== undefined) {
      protos = Array.isArray(protocols) ? protocols.map(String) : [String(protocols)];
    }
    const href = String(url instanceof URL ? url.href : url);
    __wjs_wskState.set(this, {
      url: href, protocol: "", readyState: 0, binaryType: "arraybuffer",
      bufferedAmount: 0, onopen: null, onmessage: null, onclose: null, onerror: null,
    });
    const id = __wjs_ws_connect(href, JSON.stringify(protos), this);
    __wjs_wskState.get(this).id = id;
    __wjs_wsObjs.set(id, this);
  }
  get url() { return __wjs_wskState.get(this).url; }
  get protocol() { return __wjs_wskState.get(this).protocol; }
  get readyState() { return __wjs_wskState.get(this).readyState; }
  get bufferedAmount() { return 0; }
  get binaryType() { return __wjs_wskState.get(this).binaryType; }
  set binaryType(v) {
    if (v !== "blob" && v !== "arraybuffer") throw new TypeError("binaryType must be 'blob' or 'arraybuffer'");
    __wjs_wskState.get(this).binaryType = v;
  }
  get onopen() { return __wjs_wskState.get(this).onopen; }
  set onopen(v) { __wjs_wskState.get(this).onopen = v; }
  get onmessage() { return __wjs_wskState.get(this).onmessage; }
  set onmessage(v) { __wjs_wskState.get(this).onmessage = v; }
  get onclose() { return __wjs_wskState.get(this).onclose; }
  set onclose(v) { __wjs_wskState.get(this).onclose = v; }
  get onerror() { return __wjs_wskState.get(this).onerror; }
  set onerror(v) { __wjs_wskState.get(this).onerror = v; }
  send(data) {
    const st = __wjs_wskState.get(this);
    if (st.readyState === 0) throw new Error("InvalidStateError: WebSocket is not open");
    if (st.readyState !== 1) return;
    if (typeof data === "string") __wjs_ws_send(st.id, 0, data);
    else if (data instanceof Uint8Array) __wjs_ws_send(st.id, 1, data);
    else if (data instanceof ArrayBuffer) __wjs_ws_send(st.id, 1, new Uint8Array(data));
    else if (ArrayBuffer.isView(data)) __wjs_ws_send(st.id, 1, new Uint8Array(data.buffer, data.byteOffset, data.byteLength));
    else throw new TypeError("WebSocket send: unsupported data type");
  }
  close(code = 1005, reason = "") {
    const st = __wjs_wskState.get(this);
    if (code !== 1005 && (!Number.isInteger(code) || code < 1000 || code > 4999 || [1004, 1005, 1006, 1015].includes(code))) {
      throw new Error("InvalidAccessError: bad WebSocket close code");
    }
    if (st.readyState === 3) return;
    st.readyState = 2;
    __wjs_ws_close(st.id, code, String(reason));
  }
};
// ---- Phase 3c-2: streams（纯 prelude 内存实现；默认 reader + BYOB）----
// BYOB 口径：`new ReadableStream({ type: "bytes", ... })` + `getReader({ mode: "byob" })`；
// `read(view)` 按 view 类型回同类前缀视图；`byobRequest.respond/respondWithNewView` 完整；
// 简化（文档记录）：done 时 value 为 undefined（非空视图）；respond 非元素对齐截断丢余量；
// 无 autoAllocateChunkSize；default reader 照常读字节流（Uint8Array 块）。
const __wjs_rsState = new WeakMap();
function __wjs_rsViewPrefix(r, n) {
  // 取 view 前 n 字节（元素对齐由调用方保证；DataView 按字节）。
  if (r.viewCtor === DataView) return new DataView(r.view.buffer, r.view.byteOffset, n);
  return new r.viewCtor(r.view.buffer, r.view.byteOffset, n / r.viewElem);
}
function __wjs_rsByobFill(st) {
  // 用 byteQ 填充排队的 BYOB 读；closed/出错同样结算
  while (st.byobReads.length) {
    const r = st.byobReads[0];
    try { new Uint8Array(r.view.buffer, 0, 0); }
    catch { st.byobReads.shift(); r.reject(new TypeError("BYOB view is detached")); continue; }
    if (st.error !== undefined) { st.byobReads.shift(); r.reject(st.error); continue; }
    if (st.byteLen === 0) {
      if (st.closed) { st.byobReads.shift(); r.resolve({ value: undefined, done: true }); continue; }
      break;
    }
    const n = Math.min(r.view.byteLength, st.byteLen);
    const take = n - (n % r.viewElem);
    if (take === 0) break;
    let off = take;
    for (const q of st.byteQ) {
      if (off === 0) break;
      const c = Math.min(q.length - q._off, off);
      new Uint8Array(r.view.buffer, r.view.byteOffset + (take - off), c).set(q.subarray(q._off, q._off + c));
      q._off += c; off -= c;
    }
    while (st.byteQ.length && st.byteQ[0]._off >= st.byteQ[0].length) st.byteQ.shift();
    st.byteLen -= take;
    st.byobReads.shift();
    r.resolve({ value: __wjs_rsViewPrefix(r, take), done: false });
  }
}
function __wjs_rsByobReq(st) {
  const r = st.byobReads[0];
  if (!r) return null;
  return {
    get view() { return r.view; },
    respond(n) {
      n = Number(n);
      if (!Number.isInteger(n) || n < 0 || n > r.view.byteLength) throw new RangeError("respond: bad byte count");
      if (st.byobReads[0] !== r || st.byobReq === null) throw new TypeError("respond: request is not active");
      st.byobReads.shift();
      st.byobReq = null;
      // 非元素对齐截断（余量丢弃，见头注）
      const take = n - (n % r.viewElem);
      r.resolve({ value: __wjs_rsViewPrefix(r, take), done: false });
      __wjs_rsPump(st);
    },
    respondWithNewView(v) {
      if (!ArrayBuffer.isView(v)) throw new TypeError("respondWithNewView needs a view");
      if (st.byobReads[0] !== r || st.byobReq === null) throw new TypeError("respondWithNewView: request is not active");
      r.view = v; r.viewCtor = v.constructor; r.viewElem = v.BYTES_PER_ELEMENT ?? 1;
    },
  };
}
function __wjs_rsByteToQueue(st) {
  // default reader 读字节流：整块搬运（有 _off 余量的半块留给 BYOB，不拆）
  while (st.byteQ.length && st.byteQ[0]._off === 0) {
    const q = st.byteQ.shift();
    st.byteLen -= q.length;
    st.queue.push(q);
  }
}
function __wjs_rsPull(st) {
  if (!st.reader || st.closed || st.error !== undefined || st.pulling) return;
  // pull 触发面（防微任务空转饿死事件循环，见 §4.27 追补）：
  // 只在新需求到达（read 推入等待）或有进展且需求还在（pump 尾）时调；
  // 无 pull 方法的源 + 挂起的读，eager 重拉即无限微任务链。
  st.pulling = true;
  st.pullProgress = false;
  // BYOB 读排队时带 byobRequest 进 pull（source 可直接写 view + respond）
  if (st.isBytes && st.byobReads.length && !st.byobReq) st.byobReq = __wjs_rsByobReq(st);
  try {
    const r = st.source.pull ? st.source.pull(st.controller) : undefined;
    Promise.resolve(r).then(() => { st.pulling = false; st.byobReq = null; __wjs_rsPump(st); }, (e) => {
      st.pulling = false; st.byobReq = null; __wjs_rsError(st, e);
    });
  } catch (e) { st.pulling = false; st.byobReq = null; __wjs_rsError(st, e); }
}
function __wjs_rsPump(st) {
  __wjs_rsByobFill(st);
  // default reader 读字节流：仅当有读等待（wantValue）才整块搬运；
  // closed 等待不搬，否则会饿死后来的 BYOB 读
  if (st.isBytes && st.pending.some((p) => p.wantValue)) __wjs_rsByteToQueue(st);
  while (st.pending.length && (st.queue.length || st.closed || st.error !== undefined)) {
    const { resolve, reject } = st.pending.shift();
    if (st.error !== undefined) { reject(st.error); continue; }
    if (st.queue.length) {
      const v = st.queue.shift();
      resolve({ value: v, done: false });
    } else { resolve({ value: undefined, done: true }); }
  }
  // pump 尾再拉：仅当需求还在且本轮有进展（enqueue/close/error 置 pullProgress）；
  // 干 pull（无进展）不再重拉——新需求到达时 read() 会拉。
  if (!st.closed && st.error === undefined && !st.pulling) {
    const demand = st.byobReads.length > 0 || st.pending.some((p) => p.wantValue);
    if (demand && st.pullProgress) { st.pullProgress = false; __wjs_rsPull(st); }
  }
}
function __wjs_rsError(st, e) {
  if (st.closed || st.error !== undefined) return;
  st.error = e;
  st.queue.length = 0;
  st.byteQ.length = 0; st.byteLen = 0;
  __wjs_rsByobFill(st);
  __wjs_rsPump(st);
}
function __wjs_rsController(stream, st) {
  if (st.isBytes) {
    return {
      get desiredSize() { return st.hwm - st.byteLen; },
      get byobRequest() { return st.byobReq; },
      enqueue(chunk) {
        if (st.closed || st.error !== undefined) throw new TypeError("stream is not readable");
        if (!ArrayBuffer.isView(chunk)) throw new TypeError("byte stream chunk must be a view");
        const v = new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength);
        v._off = 0;
        st.byteQ.push(v);
        st.byteLen += v.length;
        st.pullProgress = true;
        __wjs_rsPump(st);
      },
      close() {
        if (st.closed || st.error !== undefined) throw new TypeError("stream is not readable");
        st.closed = true;
        st.pullProgress = true;
        __wjs_rsPump(st);
      },
      error(e) { __wjs_rsError(st, e); },
    };
  }
  return {
    get desiredSize() { return st.hwm - st.queue.length; },
    enqueue(chunk) {
      if (st.closed || st.error !== undefined) throw new TypeError("stream is not readable");
      if (chunk === undefined) throw new TypeError("chunk must not be undefined");
      st.queue.push(chunk);
      st.pullProgress = true;
      __wjs_rsPump(st);
    },
    close() {
      if (st.closed || st.error !== undefined) throw new TypeError("stream is not readable");
      st.closed = true;
      st.pullProgress = true;
      __wjs_rsPump(st);
    },
    error(e) { __wjs_rsError(st, e); },
  };
}
globalThis.ReadableStream = class ReadableStream {
  constructor(underlyingSource = {}, strategy) {
    const hwm = strategy && strategy.highWaterMark !== undefined ? Number(strategy.highWaterMark) : 1;
    const utype = underlyingSource ? underlyingSource.type : undefined;
    if (utype !== undefined && utype !== "bytes") throw new TypeError("ReadableStream type must be 'bytes'");
    const st = {
      queue: [], pending: [], closed: false, error: undefined,
      reader: null, pulling: false, pullProgress: false, hwm: Number.isNaN(hwm) ? 1 : hwm,
      source: underlyingSource, controller: null,
      isBytes: utype === "bytes", byteQ: [], byteLen: 0, byobReads: [], byobReq: null,
    };
    st.controller = __wjs_rsController(this, st);
    __wjs_rsState.set(this, st);
    try {
      const r = underlyingSource.start ? underlyingSource.start(st.controller) : undefined;
      Promise.resolve(r).catch((e) => __wjs_rsError(st, e));
    } catch (e) { __wjs_rsError(st, e); }
  }
  get locked() { return !!__wjs_rsState.get(this).reader; }
  cancel(reason) {
    const st = __wjs_rsState.get(this);
    if (st.reader) throw new TypeError("stream is locked");
    st.queue.length = 0; st.closed = true;
    st.byteQ.length = 0; st.byteLen = 0;
    const c = st.source.cancel ? st.source.cancel(reason) : undefined;
    __wjs_rsPump(st);
    return Promise.resolve(c).then(() => undefined);
  }
  getReader(options) {
    const st = __wjs_rsState.get(this);
    if (st.reader) throw new TypeError("stream is locked");
    const mode = options ? options.mode : undefined;
    if (mode !== undefined && mode !== "byob") throw new TypeError(`Unknown reader mode '${mode}'`);
    const stream = this;
    if (mode === "byob") {
      if (!st.isBytes) throw new TypeError("getReader({ mode: 'byob' }) needs a byte stream");
      const reader = {
        get closed() {
          return new Promise((resolve, reject) => {
            if (st.error !== undefined) reject(st.error);
            else if (st.closed && !st.byteLen) resolve(undefined);
            else st.pending.push({ resolve: () => resolve(undefined), reject, wantValue: false });
          });
        },
        read(view) {
          return new Promise((resolve, reject) => {
            if (!ArrayBuffer.isView(view)) { reject(new TypeError("BYOB read needs a view")); return; }
            try { new Uint8Array(view.buffer, 0, 0); }
            catch { reject(new TypeError("BYOB view is detached")); return; }
            if (view.byteLength === 0) { reject(new TypeError("BYOB view must not be empty")); return; }
            if (st.error !== undefined) { reject(st.error); return; }
            st.byobReads.push({
              view, viewCtor: view.constructor, viewElem: view.BYTES_PER_ELEMENT ?? 1,
              resolve, reject,
            });
            __wjs_rsByobFill(st);
            __wjs_rsPull(st);
          });
        },
        releaseLock() { if (st.reader === reader) st.reader = null; },
        cancel(reason) {
          st.byteQ.length = 0; st.byteLen = 0; st.closed = true;
          const c = st.source.cancel ? st.source.cancel(reason) : undefined;
          if (st.reader === reader) st.reader = null;
          __wjs_rsPump(st);
          return Promise.resolve(c).then(() => undefined);
        },
      };
      st.reader = reader;
      return reader;
    }
    const reader = {
      get closed() {
        return new Promise((resolve, reject) => {
          if (st.error !== undefined) reject(st.error);
          else if (st.closed && !st.queue.length) resolve(undefined);
          else st.pending.push({ resolve: () => resolve(undefined), reject, wantValue: false });
        });
      },
      read() {
        return new Promise((resolve, reject) => {
          if (st.error !== undefined) { reject(st.error); return; }
          if (st.isBytes) __wjs_rsByteToQueue(st);
          if (st.queue.length) {
            const v = st.queue.shift();
            resolve({ value: v, done: false });
            __wjs_rsPull(st);
            return;
          }
          if (st.closed) { resolve({ value: undefined, done: true }); return; }
          st.pending.push({ resolve, reject, wantValue: true });
          __wjs_rsPull(st);
        });
      },
      releaseLock() { if (st.reader === reader) st.reader = null; },
      cancel(reason) {
        st.queue.length = 0; st.closed = true;
        st.byteQ.length = 0; st.byteLen = 0;
        const c = st.source.cancel ? st.source.cancel(reason) : undefined;
        if (st.reader === reader) st.reader = null;
        __wjs_rsPump(st);
        return Promise.resolve(c).then(() => undefined);
      },
    };
    st.reader = reader;
    return reader;
  }
  pipeThrough(t, options) {
    this.pipeTo(t.writable, options);
    return t.readable;
  }
  async pipeTo(dest, options = {}) {
    const preventClose = !!(options && options.preventClose);
    const reader = this.getReader();
    const writer = dest.getWriter();
    try {
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        await writer.write(value);
      }
      if (!preventClose) await writer.close();
    } finally {
      reader.releaseLock();
      writer.releaseLock();
    }
  }
  tee() {
    const st = __wjs_rsState.get(this);
    if (st.reader) throw new TypeError("stream is locked");
    // 简化 tee：顺序读源，两分支各收一份（引用共享；无背压，见文档）。
    const q1 = [], q2 = [];
    const mkBranch = (q) => new ReadableStream({
      pull(c) {
        if (q.length) { c.enqueue(q.shift()); return; }
        if (done) { c.close(); return; }
        if (failed !== undefined) { c.error(failed); return; }
        waiters.push(() => {
          if (q.length) { try { c.enqueue(q.shift()); } catch {} return; }
          if (done) { try { c.close(); } catch {} return; }
          if (failed !== undefined) { try { c.error(failed); } catch {} }
        });
      },
      cancel() {},
    });
    let done = false, failed;
    const waiters = [];
    const wake = () => { for (const w of waiters.splice(0)) w(); };
    const r1 = mkBranch(q1), r2 = mkBranch(q2);
    const src = this.getReader();
    st.reader = null;
    const loop = () => src.read().then(({ value, done: d }) => {
      if (d) { done = true; wake(); return; }
      q1.push(value); q2.push(value);
      wake();
      loop();
    }, (e) => { failed = e; wake(); });
    loop();
    return [r1, r2];
  }
  async *[Symbol.asyncIterator]() {
    const reader = this.getReader();
    try {
      for (;;) {
        const { value, done } = await reader.read();
        if (done) return;
        yield value;
      }
    } finally { reader.releaseLock(); }
  }
};
const __wjs_wsState = new WeakMap();
globalThis.WritableStream = class WritableStream {
  constructor(underlyingSink = {}, strategy) {
    const hwm = strategy && strategy.highWaterMark !== undefined ? Number(strategy.highWaterMark) : 1;
    const st = {
      queue: [], writing: false, closed: false, errored: false, error: undefined,
      writer: null, hwm: Number.isNaN(hwm) ? 1 : hwm, sink: underlyingSink,
      closeReq: null,
    };
    __wjs_wsState.set(this, st);
    const stream = this;
    st.controller = { error(e) { __wjs_wsError(stream, e); } };
    try {
      const r = underlyingSink.start ? underlyingSink.start(st.controller) : undefined;
      Promise.resolve(r).catch((e) => __wjs_wsError(this, e));
    } catch (e) { __wjs_wsError(this, e); }
  }
  get locked() { return !!__wjs_wsState.get(this).writer; }
  abort(reason) {
    const st = __wjs_wsState.get(this);
    if (st.writer) throw new TypeError("stream is locked");
    const a = st.sink.abort ? st.sink.abort(reason) : undefined;
    __wjs_wsError(this, reason);
    return Promise.resolve(a).then(() => undefined);
  }
  close() {
    const st = __wjs_wsState.get(this);
    if (st.writer) throw new TypeError("stream is locked");
    return __wjs_wsCloseReq(this);
  }
  getWriter() {
    const st = __wjs_wsState.get(this);
    if (st.writer) throw new TypeError("stream is locked");
    const stream = this;
    const writer = {
      get closed() {
        return new Promise((resolve, reject) => {
          if (st.errored) reject(st.error);
          else if (st.closed) resolve(undefined);
          else st.closeWaiters.push({ resolve, reject });
        });
      },
      get desiredSize() { return st.hwm - st.queue.length; },
      get ready() { return Promise.resolve(); },
      write(chunk) {
        if (chunk === undefined) return Promise.reject(new TypeError("chunk must not be undefined"));
        if (st.errored) return Promise.reject(st.error);
        if (st.closed) return Promise.reject(new TypeError("stream is closed"));
        return new Promise((resolve, reject) => {
          st.queue.push({ chunk, resolve, reject });
          __wjs_wsPump(stream);
        });
      },
      close() { return __wjs_wsCloseReq(stream); },
      abort(reason) {
        const a = st.sink.abort ? st.sink.abort(reason) : undefined;
        __wjs_wsError(stream, reason);
        return Promise.resolve(a).then(() => undefined);
      },
      releaseLock() { if (st.writer === writer) st.writer = null; },
    };
    st.closeWaiters = st.closeWaiters || [];
    st.writer = writer;
    return writer;
  }
};
function __wjs_wsError(stream, e) {
  const st = __wjs_wsState.get(stream);
  if (st.errored) return;
  st.errored = true;
  st.error = e;
  for (const q of st.queue.splice(0)) q.reject(e);
  if (st.closeReq) { const c = st.closeReq; st.closeReq = null; c.reject(e); }
  for (const w of (st.closeWaiters || []).splice(0)) w.reject(e);
}
function __wjs_wsCloseReq(stream) {
  const st = __wjs_wsState.get(stream);
  return new Promise((resolve, reject) => { st.closeReq = { resolve, reject }; __wjs_wsPump(stream); });
}
function __wjs_wsPump(stream) {
  const st = __wjs_wsState.get(stream);
  if (st.writing || st.errored) return;
  const item = st.queue.shift();
  if (!item) {
    if (st.closeReq && !st.writing) {
      const c = st.closeReq; st.closeReq = null;
      const done = () => { st.closed = true; c.resolve(undefined); for (const w of (st.closeWaiters || []).splice(0)) w.resolve(undefined); };
      try {
        Promise.resolve(st.sink.close ? st.sink.close() : undefined).then(done, (e) => { __wjs_wsError(stream, e); });
      } catch (e) { __wjs_wsError(stream, e); }
    }
    return;
  }
  st.writing = true;
  try {
    Promise.resolve(st.sink.write ? st.sink.write(item.chunk, st.controller) : undefined).then(
      () => { st.writing = false; item.resolve(undefined); __wjs_wsPump(stream); },
      (e) => { st.writing = false; item.reject(e); __wjs_wsError(stream, e); __wjs_wsPump(stream); },
    );
  } catch (e) { st.writing = false; item.reject(e); __wjs_wsError(stream, e); }
}
globalThis.TransformStream = class TransformStream {
  constructor(transformer = {}, writableStrategy, readableStrategy) {
    let rsCtrl;
    const readable = new ReadableStream({
      start(c) { rsCtrl = c; },
    }, readableStrategy);
    const writable = new WritableStream({
      write: (chunk, c) => transformer.transform
        ? transformer.transform(chunk, {
            enqueue: (out) => rsCtrl.enqueue(out),
            get desiredSize() { return rsCtrl.desiredSize; },
            terminate() { rsCtrl.close(); },
          })
        : rsCtrl.enqueue(chunk),
      close: () => {
        if (transformer.flush) {
          return Promise.resolve(transformer.flush({
            enqueue: (out) => rsCtrl.enqueue(out),
            get desiredSize() { return rsCtrl.desiredSize; },
            terminate() { rsCtrl.close(); },
          })).then(() => rsCtrl.close());
        }
        rsCtrl.close();
      },
      abort: (r) => rsCtrl.error(r),
    }, writableStrategy);
    try {
      const r = transformer.start ? transformer.start({
        enqueue: (out) => rsCtrl.enqueue(out),
        get desiredSize() { return rsCtrl.desiredSize; },
        terminate() { rsCtrl.close(); },
      }) : undefined;
      Promise.resolve(r).catch((e) => rsCtrl.error(e));
    } catch (e) { rsCtrl.error(e); }
    this.readable = readable;
    this.writable = writable;
  }
};
// ---- Blob（Web 全局；fetch/consumers/node:internal/blob 共用；9b）----
const __wjs_blobBytes = new WeakMap();
globalThis.Blob = class Blob {
  constructor(parts = [], options = {}) {
    const chunks = [];
    let size = 0;
    if (typeof parts === "string" || ArrayBuffer.isView(parts) || parts instanceof ArrayBuffer) {
      throw new TypeError("Blob parts must be an iterable");
    }
    for (const part of parts) {
      if (part instanceof Blob) {
        const u8 = __wjs_blobBytes.get(part);
        chunks.push(u8); size += u8.byteLength;
      } else if (typeof part === "string") {
        const u8 = new TextEncoder().encode(part);
        chunks.push(u8); size += u8.byteLength;
      } else if (ArrayBuffer.isView(part)) {
        chunks.push(new Uint8Array(part.buffer, part.byteOffset, part.byteLength));
        size += part.byteLength;
      } else if (part instanceof ArrayBuffer) {
        chunks.push(new Uint8Array(part)); size += part.byteLength;
      } else if (part == null) {
        // spec: null/undefined part 跳过
      } else {
        const u8 = new TextEncoder().encode(String(part));
        chunks.push(u8); size += u8.byteLength;
      }
    }
    const bytes = new Uint8Array(size);
    let off = 0;
    for (const c of chunks) { bytes.set(c, off); off += c.byteLength; }
    __wjs_blobBytes.set(this, bytes);
    const type = typeof options.type === "string" ? options.type : "";
    this.type = type.replace(/[^\x20-\x7E]/g, "").toLowerCase();
  }
  get size() { return __wjs_blobBytes.get(this).byteLength; }
  slice(start, end, contentType) {
    const b = __wjs_blobBytes.get(this);
    const s = start === undefined ? 0 : (start < 0 ? Math.max(b.byteLength + start, 0) : Math.min(start, b.byteLength));
    const e = end === undefined ? b.byteLength : (end < 0 ? Math.max(b.byteLength + end, 0) : Math.min(end, b.byteLength));
    const out = new Blob([], { type: contentType === undefined ? this.type : String(contentType) });
    __wjs_blobBytes.set(out, s < e ? b.slice(s, e) : new Uint8Array(0));
    return out;
  }
  arrayBuffer() {
    return Promise.resolve(__wjs_blobBytes.get(this).slice().buffer);
  }
  bytes() {
    return Promise.resolve(__wjs_blobBytes.get(this).slice());
  }
  text() {
    return Promise.resolve(new TextDecoder().decode(__wjs_blobBytes.get(this)));
  }
  stream() {
    const b = __wjs_blobBytes.get(this);
    return new ReadableStream({
      start(c) { c.enqueue(b.slice()); c.close(); },
    });
  }
  get [Symbol.toStringTag]() { return "Blob"; }
};
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
    // 监听留到流结束：head 结算只 resolve（流式 body 的 abort 还靠它）；
    // head 失败或 abort 触发或流终结时经 `__wjs_fetchCleanup` 摘除。
    let onAbort = null;
    const cleanup = () => {
      if (onAbort && st.signal) st.signal.removeEventListener("abort", onAbort);
      onAbort = null;
    };
    const id = __wjs_fetch_start(
      st.url, st.method, headersJson, st.bodyU8 ?? undefined,
      (v) => resolve(v),
      (e) => { if (id) __wjs_fetchCleanup(id); else cleanup(); reject(e); },
    );
    if (st.signal && id) {
      onAbort = () => {
        // Rust 侧取消任务 + 拒绝排队 pull（AbortError）；外层按原始 reason 拒绝。
        __wjs_abortedFetch.add(id);
        __wjs_fetch_abort(id);
        __wjs_fetchCleanup(id);
        reject(st.signal.reason);
      };
      st.signal.addEventListener("abort", onAbort);
      __wjs_fetchCleanups.set(id, cleanup);
    }
  });
};
// 已中止的流 id 集（pull 侧直接拒绝，不再进 Rust 状态）。
const __wjs_abortedFetch = new Set();
// 待摘的 abort 监听（流终结/取消时清理，长 signal 不堆积）。
const __wjs_fetchCleanups = new Map();
function __wjs_fetchCleanup(sid) {
  const fn = __wjs_fetchCleanups.get(sid);
  if (fn) {
    __wjs_fetchCleanups.delete(sid);
    try { fn(); } catch {}
  }
}
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
            ("__wjs_td_stream_open", Some(encoding::td_stream_open), 3),
            ("__wjs_td_stream_feed", Some(encoding::td_stream_feed), 3),
            ("__wjs_fill_random", Some(crypto::fill_random), 1),
            ("__wjs_random_uuid", Some(crypto::random_uuid), 0),
            ("__wjs_subtle_digest", Some(crypto::subtle_digest), 2),
            ("__wjs_aesgcm_encrypt", Some(crypto::aesgcm_encrypt), 4),
            ("__wjs_aesgcm_decrypt", Some(crypto::aesgcm_decrypt), 4),
            ("__wjs_hmac_sign", Some(crypto::hmac_sign), 3),
            ("__wjs_hmac_verify", Some(crypto::hmac_verify), 4),
            ("__wjs_rsa_generate", Some(crypto::rsa_generate), 2),
            ("__wjs_rsa_public", Some(crypto::rsa_public), 1),
            ("__wjs_rsa_sign", Some(crypto::rsa_sign), 3),
            ("__wjs_rsa_verify", Some(crypto::rsa_verify), 4),
            ("__wjs_rsa_encrypt", Some(crypto::rsa_encrypt), 4),
            ("__wjs_rsa_decrypt", Some(crypto::rsa_decrypt), 4),
            ("__wjs_rsa_jwk", Some(crypto::rsa_jwk), 2),
            ("__wjs_rsa_jwk_pub", Some(crypto::rsa_jwk_pub), 1),
            ("__wjs_rsa_import_priv", Some(crypto::rsa_import_priv), 3),
            ("__wjs_rsa_import_pub", Some(crypto::rsa_import_pub), 2),
            ("__wjs_ec_generate", Some(crypto::ec_generate), 1),
            ("__wjs_ec_public", Some(crypto::ec_public), 2),
            ("__wjs_ecdsa_sign", Some(crypto::ecdsa_sign), 4),
            ("__wjs_ecdsa_verify", Some(crypto::ecdsa_verify), 5),
            ("__wjs_ecdh_derive", Some(crypto::ecdh_derive), 3),
            ("__wjs_ec_jwk", Some(crypto::ec_jwk), 3),
            ("__wjs_ec_jwk_pub", Some(crypto::ec_jwk_pub), 2),
            ("__wjs_ec_import_priv", Some(crypto::ec_import_priv), 2),
            ("__wjs_ec_import_pub", Some(crypto::ec_import_pub), 3),
            // 9h-1：SPKI/PKCS#8 算法 OID 直判曲线（试解误判 secp256k1→P-256）
            ("__wjs_ec_guess_curve", Some(crypto::ec_guess_curve), 1),
            // 9i-3：X.509 证书验签（TBS 裸段 + 签名算法 OID 分发，复用验签底座）
            ("__wjs_x509_verify", Some(crypto::x509_verify), 3),
            // Phase c-4x：RSA-PSS / Ed25519 / X25519
            ("__wjs_pss_sign", Some(crypto::pss_sign), 4),
            ("__wjs_pss_verify", Some(crypto::pss_verify), 5),
            ("__wjs_ed_generate", Some(crypto::ed_generate), 0),
            ("__wjs_ed_public", Some(crypto::ed_public), 1),
            ("__wjs_ed_sign", Some(crypto::ed_sign), 2),
            ("__wjs_ed_verify", Some(crypto::ed_verify), 3),
            ("__wjs_x_generate", Some(crypto::x_generate), 0),
            ("__wjs_x_public", Some(crypto::x_public), 1),
            ("__wjs_x_derive", Some(crypto::x_derive), 2),
            ("__wjs_okp_pkcs8_from_seed", Some(crypto::okp_pkcs8_from_seed), 2),
            ("__wjs_okp_spki_from_pub", Some(crypto::okp_spki_from_pub), 2),
            ("__wjs_okp_seed_from_pkcs8", Some(crypto::okp_seed_from_pkcs8), 2),
            ("__wjs_okp_pub_from_spki", Some(crypto::okp_pub_from_spki), 2),
            ("__wjs_fetch_start", Some(fetch::fetch_start), 6),
            ("__wjs_fetch_abort", Some(fetch::fetch_abort), 1),
            ("__wjs_fetch_pull", Some(fetch::fetch_pull), 3),
            // Phase 4a: node:os / process（path 纯 JS，见 node/）
            ("__wjs_os_platform", Some(node::os::os_platform), 0),
            ("__wjs_os_arch", Some(node::os::os_arch), 0),
            ("__wjs_os_info", Some(node::os::os_info), 0),
            ("__wjs_os_cpus", Some(node::os::os_cpus), 0),
            ("__wjs_os_mem", Some(node::os::os_mem), 0),
            ("__wjs_os_net", Some(node::os::os_net), 0),
            ("__wjs_os_user", Some(node::os::os_user), 0),
            ("__wjs_os_uptime", Some(node::os::os_uptime), 0),
            ("__wjs_os_load", Some(node::os::os_load), 0),
            ("__wjs_os_locale", Some(node::os::os_locale), 0),
            ("__wjs_argv_json", Some(node::process_::argv_json), 0),
            ("__wjs_env_get", Some(node::process_::env_get), 1),
            ("__wjs_env_set", Some(node::process_::env_set), 2),
            ("__wjs_env_del", Some(node::process_::env_del), 1),
            ("__wjs_env_keys", Some(node::process_::env_keys), 0),
            ("__wjs_cwd", Some(node::process_::cwd), 0),
            ("__wjs_chdir", Some(node::process_::chdir), 1),
            ("__wjs_process_exit", Some(node::process_::process_exit), 1),
            ("__wjs_exit_code_get", Some(node::process_::exit_code_get), 0),
            ("__wjs_exit_code_set", Some(node::process_::exit_code_set), 1),
            ("__wjs_exec_path", Some(node::process_::exec_path), 0),
            ("__wjs_pid", Some(node::process_::pid), 0),
            ("__wjs_uptime", Some(node::process_::uptime), 0),
            ("__wjs_hrtime_ns", Some(node::process_::hrtime_ns), 0),
            ("__wjs_memory_usage", Some(node::process_::memory_usage), 0),
            ("__wjs_stdout_write", Some(node::process_::stdout_write), 1),
            ("__wjs_stderr_write", Some(node::process_::stderr_write), 1),
            ("__wjs_stdio_istty", Some(node::process_::stdio_istty), 1),
            // Phase 4b: node:fs
            ("__wjs_fs_read_file", Some(node::fs::fs_read_file), 1),
            ("__wjs_fs_write_file", Some(node::fs::fs_write_file), 3),
            ("__wjs_fs_append_file", Some(node::fs::fs_append_file), 2),
            ("__wjs_fs_stat", Some(node::fs::fs_stat), 2),
            ("__wjs_fs_mkdir", Some(node::fs::fs_mkdir), 2),
            ("__wjs_fs_rm", Some(node::fs::fs_rm), 3),
            ("__wjs_fs_readdir", Some(node::fs::fs_readdir), 2),
            ("__wjs_fs_rename", Some(node::fs::fs_rename), 2),
            ("__wjs_fs_copy_file", Some(node::fs::fs_copy_file), 2),
            ("__wjs_fs_exists", Some(node::fs::fs_exists), 1),
            ("__wjs_fs_unlink", Some(node::fs::fs_unlink), 1),
            ("__wjs_fs_rmdir", Some(node::fs::fs_rmdir), 2),
            ("__wjs_fs_realpath", Some(node::fs::fs_realpath), 1),
            ("__wjs_fs_mkdtemp", Some(node::fs::fs_mkdtemp), 1),
            // Phase 9c: fs 同步面增补（fd 系/link 系/时间戳/权限/access）
            ("__wjs_fs_read_link", Some(node::fs::fs_read_link), 1),
            ("__wjs_fs_link", Some(node::fs::fs_link), 2),
            ("__wjs_fs_symlink", Some(node::fs::fs_symlink), 2),
            ("__wjs_fs_truncate", Some(node::fs::fs_truncate), 2),
            ("__wjs_fs_utimes", Some(node::fs::fs_utimes), 3),
            ("__wjs_fs_chmod", Some(node::fs::fs_chmod), 2),
            ("__wjs_fs_access", Some(node::fs::fs_access), 2),
            ("__wjs_fs_open", Some(node::fs::fs_open), 2),
            ("__wjs_fs_close", Some(node::fs::fs_close), 1),
            ("__wjs_fs_read_fd", Some(node::fs::fs_read_fd), 3),
            ("__wjs_fs_write_fd", Some(node::fs::fs_write_fd), 3),
            ("__wjs_fs_ftruncate", Some(node::fs::fs_ftruncate), 2),
            ("__wjs_fs_fstat", Some(node::fs::fs_fstat), 1),
            ("__wjs_fs_fchmod", Some(node::fs::fs_fchmod), 2),
            ("__wjs_fs_futimes", Some(node::fs::fs_futimes), 3),
            ("__wjs_fs_fsync", Some(node::fs::fs_fsync), 2),
            ("__wjs_watch_start", Some(node::fs::watch_start), 4),
            ("__wjs_watch_close", Some(node::fs::watch_close), 1),
            // Phase 4c: child_process
            // Phase 9d: node:net + node:dns
            ("__wjs_net_connect", Some(node::net::net_connect), 3),
            ("__wjs_net_listen", Some(node::net::net_listen), 3),
            ("__wjs_net_attach", Some(node::net::net_attach), 2),
            ("__wjs_net_write", Some(node::net::net_write), 2),
            ("__wjs_net_end", Some(node::net::net_end), 1),
            ("__wjs_net_destroy", Some(node::net::net_destroy), 1),
            ("__wjs_dns_lookup", Some(node::dns::dns_lookup), 1),
            // Phase 9d-6: node:tls（握手底座；读写复用 net_* natives）
            ("__wjs_tls_connect", Some(node::tls::tls_connect), 4),
            ("__wjs_tls_listen", Some(node::tls::tls_listen), 4),
            // Phase 9d-7: node:http2（hyper 直引；关闭复用 __wjs_net_destroy）
            ("__wjs_h2_listen", Some(node::http2::h2_listen), 4),
            ("__wjs_h2_connect", Some(node::http2::h2_connect), 4),
            ("__wjs_h2_open", Some(node::http2::h2_open), 4),
            ("__wjs_h2_respond", Some(node::http2::h2_respond), 5),
            // Phase 9e-1a: node:crypto 增量 Hash（oneshot 复用全局 __wjs_*）
            ("__wjs_crypto_hash_new", Some(node::crypto::crypto_hash_new), 1),
            ("__wjs_crypto_hash_update", Some(node::crypto::crypto_hash_update), 2),
            ("__wjs_crypto_hash_digest", Some(node::crypto::crypto_hash_digest), 1),
            ("__wjs_crypto_hash_copy", Some(node::crypto::crypto_hash_copy), 1),
            // Phase 9e-1b: node:crypto 对称密码（CBC/CTR 流式 + ChaCha oneshot）
            ("__wjs_cipher_new", Some(node::crypto::cipher_new), 5),
            ("__wjs_cipher_update", Some(node::crypto::cipher_update), 2),
            ("__wjs_cipher_final", Some(node::crypto::cipher_final), 1),
            ("__wjs_cipher_chacha", Some(node::crypto::cipher_chacha), 6),
            // Phase 9e-1c: RSA v1.5 + DH/素性（签名/派生复用既有 natives）
            ("__wjs_rsa_encrypt_v15", Some(node::crypto::rsa_encrypt_v15), 2),
            ("__wjs_rsa_decrypt_v15", Some(node::crypto::rsa_decrypt_v15), 2),
            ("__wjs_dh_genkey", Some(node::crypto::dh_genkey), 3),
            ("__wjs_dh_secret", Some(node::crypto::dh_secret), 3),
            ("__wjs_prime_check", Some(node::crypto::prime_check), 2),
            ("__wjs_prime_gen", Some(node::crypto::prime_gen), 3),
            // Phase 9h-1: DSA（dsa 0.7 + hazmat；信封 JSON 桥）
            ("__wjs_dsa_generate", Some(crypto::dsa_generate), 2),
            ("__wjs_dsa_sign", Some(crypto::dsa_sign), 3),
            ("__wjs_dsa_verify", Some(crypto::dsa_verify), 4),
            ("__wjs_dsa_export", Some(crypto::dsa_export), 1),
            // Phase 9e-1c: RSA-SHA1 手工件（digest 0.10 版本面，§0.5 未批新行）
            ("__wjs_node_rsa_oaep", Some(node::crypto::node_rsa_oaep), 4),
            ("__wjs_node_rsa_v15_sign", Some(node::crypto::node_rsa_v15_sign), 3),
            ("__wjs_node_rsa_v15_verify", Some(node::crypto::node_rsa_v15_verify), 4),
            // Phase 9e-1d: KDF + X509（全员树内轮子）
            ("__wjs_kdf_pbkdf2", Some(node::crypto::kdf_pbkdf2), 5),
            ("__wjs_kdf_scrypt", Some(node::crypto::kdf_scrypt), 7),
            ("__wjs_kdf_hkdf", Some(node::crypto::kdf_hkdf), 5),
            ("__wjs_kdf_argon2", Some(node::crypto::kdf_argon2), 9),
            ("__wjs_x509_parse", Some(node::crypto::x509_parse), 1),
            // Phase 9i-7: X509 checkIssued（名字 DER + AKID/SKID + keyUsage）
            ("__wjs_x509_check_issued", Some(node::crypto::x509_check_issued), 2),
            // Phase 9i-4: ml-kem（FIPS 203；ml-kem crate，种子形 PKCS#8/SPKI/封装面）
            ("__wjs_mlkem_gen", Some(node::crypto::mlkem_gen), 1),
            ("__wjs_mlkem_seed_from_pkcs8", Some(node::crypto::mlkem_seed_from_pkcs8), 1),
            ("__wjs_mlkem_kind_from_spki", Some(node::crypto::mlkem_kind_from_spki), 1),
            ("__wjs_mlkem_encaps", Some(node::crypto::mlkem_encaps), 2),
            ("__wjs_mlkem_decaps", Some(node::crypto::mlkem_decaps), 2),
            // Phase 9i-6: ml-dsa（FIPS 204；纯签名，种子形 PKCS#8/SPKI/Sign-Verify）
            ("__wjs_mldsa_gen", Some(node::crypto::mldsa_gen), 1),
            ("__wjs_mldsa_seed_from_pkcs8", Some(node::crypto::mldsa_seed_from_pkcs8), 1),
            ("__wjs_mldsa_kind_from_spki", Some(node::crypto::mldsa_kind_from_spki), 1),
            ("__wjs_mldsa_public", Some(node::crypto::mldsa_public), 1),
            ("__wjs_mldsa_sign", Some(node::crypto::mldsa_sign), 2),
            ("__wjs_mldsa_verify", Some(node::crypto::mldsa_verify), 3),
            // Phase 9e-4: inspector 会话求值（同线程嵌套 evaluate_script）
            ("__wjs_inspector_eval", Some(node::inspector::inspector_eval), 1),
            // Phase 9f-1: node:vm（同 Runtime 多 global；id 字符串形态）
            ("__wjs_vm_create", Some(node::vm::vm_create), 0),
            ("__wjs_vm_compile", Some(node::vm::vm_compile), 2),
            ("__wjs_vm_run", Some(node::vm::vm_run), 3),
            ("__wjs_vm_run_this", Some(node::vm::vm_run_this), 2),
            ("__wjs_vm_compile_fn", Some(node::vm::vm_compile_fn), 4),
            ("__wjs_vm_set", Some(node::vm::vm_set), 3),
            ("__wjs_vm_get", Some(node::vm::vm_get), 2),
            ("__wjs_vm_keys", Some(node::vm::vm_keys), 1),
            ("__wjs_vm_release", Some(node::vm::vm_release), 1),
            // Phase 9i-1: vm 模块系（SourceText；Synthetic 纯 JS）
            ("__wjs_vm_compile_mod", Some(node::vm::vm_mod_compile), 3),
            ("__wjs_vm_link", Some(node::vm::vm_mod_link), 1),
            ("__wjs_vm_evaluate", Some(node::vm::vm_mod_evaluate), 1),
            ("__wjs_vm_mod_ns", Some(node::vm::vm_mod_ns), 1),
            ("__wjs_vm_mod_release", Some(node::vm::vm_mod_release), 1),
            ("__wjs_vm_mod_settled", Some(node::vm::vm_mod_settled), 1),
            ("__wjs_vm_mod_deps", Some(node::vm::vm_mod_deps), 1),
            // Phase 9f-2: worker 消息通道（端口对/投递/线程身份/环境数据）
            ("__wjs_port_pair", Some(node::worker::port_pair), 0),
            ("__wjs_port_attach", Some(node::worker::port_attach), 2),
            ("__wjs_port_post", Some(node::worker::port_post), 2),
            ("__wjs_port_close", Some(node::worker::port_close), 1),
            ("__wjs_port_unref", Some(node::worker::port_unref), 1),
            ("__wjs_port_ref", Some(node::worker::port_ref), 1),
            // Phase 9i-2: 端口迁移（offer/accept 经邀约槽 + 转发器）+ BroadcastChannel
            ("__wjs_port_offer", Some(node::worker::port_offer), 1),
            ("__wjs_port_accept", Some(node::worker::port_accept), 1),
            ("__wjs_port_withdraw", Some(node::worker::port_withdraw), 1),
            ("__wjs_port_detach", Some(node::worker::port_detach), 1),
            ("__wjs_bc_sub", Some(node::worker::bc_sub), 1),
            ("__wjs_bc_unsub", Some(node::worker::bc_unsub), 1),
            ("__wjs_bc_pub", Some(node::worker::bc_pub), 3),
            ("__wjs_bc_flags", Some(node::worker::bc_flags), 2),
            ("__wjs_bc_attach", Some(node::worker::bc_attach), 2),
            ("__wjs_worker_is_main", Some(node::worker::worker_is_main), 0),
            ("__wjs_worker_thread_id", Some(node::worker::worker_thread_id), 0),
            ("__wjs_worker_parent", Some(node::worker::worker_parent), 0),
            ("__wjs_worker_data", Some(node::worker::worker_data), 0),
            ("__wjs_worker_env_set", Some(node::worker::env_set), 2),
            ("__wjs_worker_env_get", Some(node::worker::env_get), 1),
            // Phase 9f-3: Worker（spawn/投递/终止/监听计数）
            ("__wjs_worker_spawn", Some(node::worker::worker_spawn), 3),
            ("__wjs_worker_attach", Some(node::worker::worker_attach), 2),
            ("__wjs_worker_post", Some(node::worker::worker_post), 2),
            ("__wjs_worker_terminate", Some(node::worker::worker_terminate), 1),
            ("__wjs_worker_set_ref", Some(node::worker::worker_set_ref), 2),
            ("__wjs_worker_tid", Some(node::worker::worker_tid), 1),
            ("__wjs_port_listen", Some(node::worker::port_listen), 1),
            ("__wjs_port_unlisten", Some(node::worker::port_unlisten), 1),
            ("__wjs_port_has_ref", Some(node::worker::port_has_ref), 1),
            // Phase 9g-1: node:quic（Endpoint/会话；流/数据报 9g-2）
            ("__wjs_quic_listen", Some(node::quic::quic_listen), 1),
            ("__wjs_quic_ep_addr", Some(node::quic::quic_ep_addr), 1),
            ("__wjs_quic_ep_close", Some(node::quic::quic_ep_close), 1),
            ("__wjs_quic_ep_attach", Some(node::quic::quic_ep_attach), 2),
            ("__wjs_quic_connect", Some(node::quic::quic_connect), 1),
            ("__wjs_quic_sess_attach", Some(node::quic::quic_sess_attach), 2),
            ("__wjs_quic_sess_info", Some(node::quic::quic_sess_info), 1),
            ("__wjs_quic_sess_stats", Some(node::quic::quic_sess_stats), 1),
            ("__wjs_quic_sess_close", Some(node::quic::quic_sess_close), 2),
            // Phase 9g-2: QUIC 流/数据报
            ("__wjs_quic_sess_open", Some(node::quic::quic_sess_open), 2),
            ("__wjs_quic_stream_attach", Some(node::quic::quic_stream_attach), 2),
            // Phase 9i-9: H3 分支（服务端 respond / 客户端 request）
            ("__wjs_quic_h3_respond", Some(node::quic::quic_h3_respond), 3),
            ("__wjs_quic_h3_request", Some(node::quic::quic_h3_request), 2),
            ("__wjs_quic_stream_write", Some(node::quic::quic_stream_write), 2),
            ("__wjs_quic_stream_finish", Some(node::quic::quic_stream_finish), 1),
            ("__wjs_quic_stream_reset", Some(node::quic::quic_stream_reset), 2),
            ("__wjs_quic_stream_stop", Some(node::quic::quic_stream_stop), 2),
            ("__wjs_quic_sess_send_dgram", Some(node::quic::quic_sess_send_dgram), 2),
            ("__wjs_quic_sess_max_dgram", Some(node::quic::quic_sess_max_dgram), 1),
            ("__wjs_dgram_bind", Some(node::dgram::dgram_bind), 3),
            ("__wjs_dgram_send", Some(node::dgram::dgram_send), 3),
            // Phase 9d-5: node:zlib（convenience 压缩面；流式类顺延）
            ("__wjs_zlib_deflate_lv", Some(node::zlib::zlib_deflate_lv), 2),
            ("__wjs_zlib_inflate", Some(node::zlib::zlib_inflate), 1),
            ("__wjs_zlib_deflate_raw", Some(node::zlib::zlib_deflate_raw), 2),
            ("__wjs_zlib_inflate_raw", Some(node::zlib::zlib_inflate_raw), 1),
            ("__wjs_zlib_gzip", Some(node::zlib::zlib_gzip), 2),
            ("__wjs_zlib_gunzip", Some(node::zlib::zlib_gunzip), 1),
            ("__wjs_zlib_unzip", Some(node::zlib::zlib_unzip), 1),
            ("__wjs_zlib_brotli_compress", Some(node::zlib::zlib_brotli_compress), 2),
            ("__wjs_zlib_brotli_decompress", Some(node::zlib::zlib_brotli_decompress), 1),
            ("__wjs_zlib_zstd_compress", Some(node::zlib::zlib_zstd_compress), 1),
            ("__wjs_zlib_zstd_decompress", Some(node::zlib::zlib_zstd_decompress), 1),
            ("__wjs_cp_exec", Some(node::child::cp_exec), 2),
            ("__wjs_cp_spawn", Some(node::child::cp_spawn), 3),
            // Phase 4d: 异步 spawn（c-4x 加 pipe：stdin 写/关 natives）
            ("__wjs_spawn_start", Some(node::child::spawn_start), 5),
            ("__wjs_child_kill", Some(node::child::child_kill), 2),
            ("__wjs_child_pid", Some(node::child::child_pid), 1),
            ("__wjs_child_stdin_write", Some(node::child::child_stdin_write), 2),
            ("__wjs_child_stdin_close", Some(node::child::child_stdin_close), 1),
            // Phase 4d: require（裸 native，直调保调用方定位；附属见 NODE_PRELUDE）
            ("require", Some(node::require::require_native), 1),
            ("__wjs_require_resolve", Some(node::require::require_resolve), 1),
            ("__wjs_require_main_url", Some(node::require::require_main_url), 0),
            // Phase 9j: node:module（createRequire 显式 base 底座 + 内建列表）
            ("__wjs_require_from", Some(node::require::require_from), 2),
            ("__wjs_require_resolve_from", Some(node::require::require_resolve_from), 2),
            ("__wjs_cjs_compile", Some(node::require::cjs_compile), 3),
            ("__wjs_builtin_modules", Some(node::require::builtin_modules_json), 0),
            // Phase 9j: CJS 互操作垫片（import 命中 CJS → export default）
            ("__wjs_require_cjs_by_url", Some(node::require::require_cjs_by_url), 1),
            ("__wjs_ws_connect", Some(ws::ws_connect), 3),
            ("__wjs_ws_send", Some(ws::ws_send), 3),
            ("__wjs_ws_close", Some(ws::ws_close), 3),
            // Phase 7-e4: bun:sqlite（同步语义，worker 线程见 bun/sqlite.rs）
            ("__wjs_sqlite_open", Some(bun::sqlite::sqlite_open), 1),
            ("__wjs_sqlite_exec", Some(bun::sqlite::sqlite_exec), 2),
            ("__wjs_sqlite_run", Some(bun::sqlite::sqlite_run), 4),
            ("__wjs_sqlite_rows", Some(bun::sqlite::sqlite_rows), 4),
            ("__wjs_sqlite_txn", Some(bun::sqlite::sqlite_txn), 1),
            ("__wjs_sqlite_close", Some(bun::sqlite::sqlite_close), 1),
            // Phase 7-e6: bun:ffi（动态调用引擎见 ffi.rs 头注；UNSAFE-BOUNDARY 密集区）
            ("__wjs_ffi_dlopen", Some(bun::ffi::ffi_dlopen), 2),
            ("__wjs_ffi_ptr_str", Some(bun::ffi::ffi_ptr_str), 1),
            ("__wjs_ffi_ptr_view", Some(bun::ffi::ffi_ptr_view), 1),
            ("__wjs_ffi_call", Some(bun::ffi::ffi_call), 2),
            ("__wjs_ffi_cstring", Some(bun::ffi::ffi_cstring), 1),
            ("__wjs_ffi_bytes", Some(bun::ffi::ffi_bytes), 2),
        ];
        // 重名 native 会静默覆盖（如 __wjs_env_* 曾被 worker 环境数据顶掉，
        // process.env 全坏——debug 期即炸，见 9f-2。局部表：每会话 define_all
        // 都跑一次，判重集必须局部（static 跨会话误报）。
        #[cfg(debug_assertions)]
        let mut seen_native: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for (name, native, nargs) in web {
            let cname = CString::new(*name).expect("no NUL");
            #[cfg(debug_assertions)]
            debug_assert!(
                seen_native.insert(*name),
                "duplicate builtin native name: {name}"
            );
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
