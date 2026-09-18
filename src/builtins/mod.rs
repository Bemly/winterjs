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
// Web/Node 全局 `performance` 最小子集（M5 vitest 牵引：tinybench 等徒手引用，
// 不 import 任何模块；`node:perf_hooks` 求值期以全功能对象覆盖，覆盖后与
// `perf_hooks.performance` 同一对象，真机口径）。
globalThis.performance = {
  now: () => Date.now() - globalThis.performance.timeOrigin,
  timeOrigin: Date.now(),
  toJSON() { return { timeOrigin: this.timeOrigin }; },
};
globalThis.queueMicrotask = function (cb) {
  if (typeof cb !== "function") throw new TypeError("queueMicrotask: callback must be a function");
  // 与引擎内部 job queue 同一条微任务队列；回调抛错 → 未处理 rejection（由 runtime 上报）
  Promise.resolve().then(cb);
};
// Node 口径 Timeout/Immediate 对象（10f 对拍定案）：真类 + unref 真语义
// （native 表 unrefed 位，事件循环 idle 判定忽略未 ref 项——keep-alive 归宿
// 与 node 一致）+ 触发期 this=Timeout 实例 + Symbol.dispose/close +
// Node lib/internal/timers.js 原文的 delay 钳制与三态警告。
// 类本体不出块作用域（真机 globalThis.Timeout === undefined，不污染全局）。
{
  let __warnedNegative = false;
  let __warnedNaN = false;
  // Node 原文直译：`!(after >= 1 && after <= TIMEOUT_MAX)` 一律钳 1（含
  // NaN/±Infinity/0/负数/溢出）；警告三态——溢出每次发、负数/NaN 每进程一次。
  globalThis.__wjs_timer_after = (ms) => {
    const after = ms * 1;
    if (!(after >= 1 && after <= 2147483647)) {
      const warn = (msg, name) => {
        try { globalThis.process?.emitWarning?.(msg, name); } catch {}
      };
      if (after > 2147483647) {
        warn(`${after} does not fit into a 32-bit signed integer.\nTimeout duration was set to 1.`, "TimeoutOverflowWarning");
      } else if (after < 0 && !__warnedNegative) {
        __warnedNegative = true;
        warn(`${after} is a negative number.\nTimeout duration was set to 1.`, "TimeoutNegativeWarning");
      } else if (Number.isNaN(after) && !__warnedNaN) {
        __warnedNaN = true;
        warn(`${after} is not a number.\nTimeout duration was set to 1.`, "TimeoutNaNWarning");
      }
      return 1;
    }
    return after;
  };
  // Node validateCallback 口径：TypeError + ERR_INVALID_ARG_TYPE 码
  // （套件按 {code,name} 匹配，§4.51 校验在包装外先抛）。
  globalThis.__wjs_timer_validate_cb = (cb) => {
    if (typeof cb !== "function") {
      const e = new TypeError(`The "callback" argument must be of type function. Received type ${typeof cb}`);
      e.code = "ERR_INVALID_ARG_TYPE";
      throw e;
    }
  };
  class Timeout {
    constructor(id) {
      this.__wjs_id = id;
      this._destroyed = false;
      this._idleTimeout = 1;
      this._idleStart = 0;
      this._onTimeout = null;
      this._timerArgs = undefined;
      this._repeat = null;
      this.__wjs_unrefed = false;
    }
    unref() { this.__wjs_unrefed = true; __wjs_timer_ref(this.__wjs_id, false); return this; }
    ref() { this.__wjs_unrefed = false; __wjs_timer_ref(this.__wjs_id, true); return this; }
    hasRef() { return !this.__wjs_unrefed; }
    refresh() { __wjs_timer_refresh(this.__wjs_id); this._destroyed = false; return this; }
    close() { __clear(this); return this; }
    [Symbol.toPrimitive]() { return this.__wjs_id; }
    [Symbol.dispose]() { __clear(this); }
  }
  class Immediate extends Timeout {}
  // ALS 快照挂载点：async_hooks 模块载入时安装 capture/restore；未载入则
  // 定时器回调无异步上下文（缺省口径）。
  const __alsRun = (snap, fn) =>
    snap !== undefined && globalThis.__wjs_als_restore ? __wjs_als_restore(snap, fn) : fn();
  // 域内回调执行（10f 对拍 immediate-queue-throw）：登记期捕获活域
  // （node AsyncContextFrame 同口径），回调抛错先路由域 error，无域再抛。
  const __runCb = (self, snap, dom) => {
    try {
      __alsRun(snap, () => Reflect.apply(self._onTimeout, self, self._timerArgs));
    } catch (err) {
      if (dom && typeof dom._emitError === "function") {
        dom._emitError(err);
      } else {
        throw err;
      }
    }
  };
  // 注册 + 触发闭包。触发语义对齐 node processTimers：
  // - 回调 this=Timeout 实例、实参 live 读 _timerArgs（套件 unenroll 直改）；
  // - Reflect.apply 直调内建（套件 user-call：回调自身 .call/.apply 可被
  //   猴子补丁成非函数，Instance 上的方法查找会炸）；
  // - 超时/immediate 触发即 _destroyed=true；
  // - interval 重排门走返回值（false = 不重排）：`_onTimeout` 失效或
  //   `_repeat` 清失或 `_idleTimeout === -1`（legacy unenroll 技法），
  //   native 侧见布尔 false 即不重排。
  const __arm = (self, delay, interval) => {
    const snap = globalThis.__wjs_als_capture?.();
    const dom = globalThis.__wjs_domain_capture?.();
    const step = interval
      ? () => {
          if (typeof self._onTimeout !== "function") { self._destroyed = true; return false; }
          __runCb(self, snap, dom);
          if (!self._repeat || self._idleTimeout === -1) { self._destroyed = true; return false; }
          return true;
        }
      : () => {
          if (typeof self._onTimeout !== "function") { self._destroyed = true; return false; }
          try { __runCb(self, snap, dom); } finally { self._destroyed = true; }
          return false;
        };
    const id = interval ? __wjs_setInterval(step, delay, []) : __wjs_setTimeout(step, delay, []);
    self.__wjs_id = id;
  };
  globalThis.setTimeout = function (cb, ms, ...rest) {
    __wjs_timer_validate_cb(cb);
    const after = ms === undefined ? 1 : __wjs_timer_after(ms);
    const self = new Timeout(0);
    self._idleTimeout = after;
    self._onTimeout = cb;
    self._timerArgs = rest;
    self._repeat = null;
    __arm(self, after, false);
    return self;
  };
  globalThis.setInterval = function (cb, ms, ...rest) {
    __wjs_timer_validate_cb(cb);
    const after = ms === undefined ? 1 : __wjs_timer_after(ms);
    const self = new Timeout(0);
    self._idleTimeout = after;
    self._onTimeout = cb;
    self._timerArgs = rest;
    self._repeat = after;
    __arm(self, after, true);
    return self;
  };
  // 三清同体（套件 api-refs：delete 全局后 clearInterval/clearImmediate
  // 不得二次解引用 globalThis.clearTimeout——那正是 131 报错的根）。
  const __clear = (id) => {
    if (id !== null && id !== undefined && typeof id === "object" && "__wjs_id" in id) {
      id._destroyed = true;
    }
    __wjs_clearTimeout(__wjs_timer_id(id));
  };
  globalThis.clearTimeout = __clear;
  globalThis.clearInterval = __clear;
  globalThis.clearImmediate = __clear;
  // 10a：全局 setImmediate/clearImmediate（本仓无 macrotask 分层，setTimeout(0)
  // 近似——与 node:timers 同口径，check 阶段语义记档；clearImmediate 复用同表）。
  globalThis.setImmediate = function (cb, ...rest) {
    __wjs_timer_validate_cb(cb);
    const self = new Immediate(0);
    self._idleTimeout = 0;
    self._onTimeout = cb;
    self._timerArgs = rest;
    self._repeat = null;
    __arm(self, 0, false);
    return self;
  };
  // clearImmediate 已并入上方 __clear 三清同体。
}
globalThis.__wjs_timer_id = (id) => {
  if (id === null || id === undefined) return 0;
  if (typeof id === "object") return id.__wjs_id ?? 0;
  if (typeof id === "number") return Number.isFinite(id) && id >= 0 ? id : 0;
  if (typeof id === "string") { const n = Number(id); return Number.isFinite(n) && n >= 0 ? n : 0; }
  return 0;
};
// 未捕获异常分发（timer 等异步回调抛错时由 native 调）。
// __wjs_uncaught_count 先探监听器数——为 0 时 native 保持 pending 原样走
// fatal 上报（错误信息/栈不经中转，不降级）；>0 时 native 取走异常经
// __wjs_uncaught 逐个调用（Node 口径第二参 origin='uncaughtException'）。
globalThis.__wjs_uncaught_count = () => {
  const p = globalThis.process;
  const ls = p && p.__wjs_listeners ? p.__wjs_listeners["uncaughtException"] : undefined;
  return ls ? ls.length : 0;
};
globalThis.__wjs_uncaught = (err) => {
  const p = globalThis.process;
  if (p && typeof p.__wjs_emit === "function") {
    return p.__wjs_emit("uncaughtException", err, "uncaughtException") > 0;
  }
  return false;
};
// 事件循环触发定时器 / structuredClone 枚举属性用的内部辅助
globalThis.__wjs_call = (cb, args) => cb(...args);
// napi_call_function：recv 语义的参数展开（Function.prototype.apply）
globalThis.__wjs_napi_call = (recv, fn, args) => fn.apply(recv, args);
// ESM 定制钩子注册表（module.registerHooks 写、import.meta.resolve 消费）。
// Node 口径：后注册者先跑（每个新钩子包住既有链，next = 链上已见部分）；
// 默认底座 = 本仓解析器（parentURL 显式 base 的 __wjs_require_resolve_from）。
globalThis.__wjs_module_hooks = [];
globalThis.__wjs_module_resolve_chain = function (specifier, parentURL) {
  let chain = (spec, ctx) => ({ url: __wjs_require_resolve_from(ctx.parentURL, spec) });
  for (const h of globalThis.__wjs_module_hooks) {
    if (h && typeof h.resolve === "function") {
      const next = chain;
      const hook = h.resolve;
      chain = (spec2, ctx2) => hook(spec2, ctx2, next);
    }
  }
  const out = chain(String(specifier), { parentURL });
  if (!out || typeof out.url !== "string") {
    throw new Error("module customization resolve hook must return { url: <string> }");
  }
  return out.url;
};
// DOMException（Web/Node 17+ 全局；jsdom 生态取此面）：Error 子类 + name +
// Web 规范常量族 + legacy code getter（name→旧码；真机 26.8.2 实测
// new DOMException("x","AbortError").code === 20）。
globalThis.DOMException = class DOMException extends Error {
  constructor(message = "", name = "Error") {
    super(String(message));
    this.name = String(name);
  }
  get code() {
    const legacy = {
      IndexSizeError: 1, HierarchyRequestError: 3, WrongDocumentError: 4,
      InvalidCharacterError: 5, NoModificationAllowedError: 7, NotFoundError: 8,
      NotSupportedError: 9, InUseAttributeError: 10, InvalidStateError: 11,
      SyntaxError: 12, InvalidModificationError: 13, NamespaceError: 14,
      InvalidAccessError: 15, TypeMismatchError: 17, SecurityError: 18,
      NetworkError: 19, AbortError: 20, URLMismatchError: 21,
      QuotaExceededError: 22, TimeoutError: 23, InvalidNodeTypeError: 24,
      DataCloneError: 25,
    };
    return legacy[this.name] ?? 0;
  }
  get [Symbol.toStringTag]() { return "DOMException"; }
};
{
  const codes = [
    ["INDEX_SIZE_ERR", 1], ["DOMSTRING_SIZE_ERR", 2], ["HIERARCHY_REQUEST_ERR", 3],
    ["WRONG_DOCUMENT_ERR", 4], ["INVALID_CHARACTER_ERR", 5], ["NO_DATA_ALLOWED_ERR", 6],
    ["NO_MODIFICATION_ALLOWED_ERR", 7], ["NOT_FOUND_ERR", 8], ["NOT_SUPPORTED_ERR", 9],
    ["INUSE_ATTRIBUTE_ERR", 10], ["INVALID_STATE_ERR", 11], ["SYNTAX_ERR", 12],
    ["INVALID_MODIFICATION_ERR", 13], ["NAMESPACE_ERR", 14], ["INVALID_ACCESS_ERR", 15],
    ["VALIDATION_ERR", 16], ["TYPE_MISMATCH_ERR", 17], ["SECURITY_ERR", 18],
    ["NETWORK_ERR", 19], ["ABORT_ERR", 20], ["URL_MISMATCH_ERR", 21],
    ["QUOTA_EXCEEDED_ERR", 22], ["TIMEOUT_ERR", 23], ["INVALID_NODE_TYPE_ERR", 24],
    ["DATA_CLONE_ERR", 25],
  ];
  for (const [name, code] of codes) {
    globalThis.DOMException[name] = code;
  }
}
// Node 15+ 全局 MessageChannel/MessagePort = worker_threads 同款类（getter 惰性
// require 保类同一性：global 与 module 导出同一对象，instanceof 不分叉）。
for (const name of ["MessageChannel", "MessagePort"]) {
  Object.defineProperty(globalThis, name, {
    configurable: true,
    get() { return globalThis.require("node:worker_threads")[name]; },
  });
}
// import.meta.resolve 的每模块闭包（modules.rs metadata_hook 以模块 URL 调用）
globalThis.__wjs_make_meta_resolve = function (url) {
  return function resolve(specifier) {
    return __wjs_module_resolve_chain(specifier, url);
  };
};
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
// ---- Buffer 全局（10f：node lib/buffer.js v26.8.2 + lib/internal/buffer.js 逐字移植，MIT）----
const __wjs_bufTAFill = Uint8Array.prototype.fill; // 原生 fill（Buffer.prototype.fill 会遮蔽，内部一律走它）
// native 片（slice/write 静态、_compare、indexOf 族）以纯 JS 重实现；
// pool 不做（直接分配，Buffer.poolSize 仅作值）；allocUnsafe 恒零填（无未初始化
// 内存暴露，记档）；kMaxLength/kStringMaxLength 取引擎实测边界（SM
// MaxStringLength=2^30-2；node 26 64-bit kMaxLength=MAX_SAFE_INTEGER）。
// 错误工厂自含（prelude 不能 import node:internal/errors；消息逐字对齐，
// 实现摘自 errors.rs 同源移植）。__wjs_bufDecode/Encode 为 string_decoder
// 依赖的全局 helper，原样保留。
const kMaxLength = 9007199254740991; // node 26 64-bit 实测 = MAX_SAFE_INTEGER
const kStringMaxLength = 1073741822; // SM MaxStringLength = 2^30 - 2（实测探针钉住，
                                     // 套件门：repeat(MAX+1) 须抛、repeat(MAX) 须过）
(() => {
function __wjs_bufFormatList(array, type = 'and') {
  switch (array.length) {
    case 0: return '';
    case 1: return `${array[0]}`;
    case 2: return `${array[0]} ${type} ${array[1]}`;
    case 3: return `${array[0]}, ${array[1]}, ${type} ${array[2]}`;
    default: return `${array.slice(0, -1).join(', ')}, ${type} ${array[array.length - 1]}`;
  }
}
// node util.inspect 最小替身（错误消息 Received 兜底；depth=-1 不展开嵌套）
function __wjs_bufInspect(value, depth = -1) {
  if (value === null) return 'null';
  if (value === undefined) return 'undefined';
  const t = typeof value;
  if (t === 'string') {
    if (value.length > 28) value = value.slice(0, 25) + '...';
    return `'${value}'`;
  }
  if (t === 'number' || t === 'boolean' || t === 'bigint' || t === 'symbol') return String(value);
  if (t === 'function') return value.name ? `[Function: ${value.name}]` : '[Function (anonymous)]';
  if (t !== 'object') return String(value);
  const ctor = value.constructor?.name;
  if (Array.isArray(value)) {
    if (depth === -1) return `[Array(${value.length})]`;
    const items = value.slice(0, 7).map((v) => __wjs_bufInspect(v, depth - 1));
    if (value.length > 7) items.push(`... ${value.length - 7} more item${value.length - 7 > 1 ? 's' : ''}`);
    return `[ ${items.join(', ')} ]`;
  }
  if (value instanceof Error) {
    return ctor === 'Error' ? (value.stack || String(value)).split('\n')[0] : `${ctor || 'Error'}: ${value.message}`;
  }
  if (value instanceof Date) return isNaN(value.getTime()) ? 'Invalid Date' : value.toISOString();
  const keys = Object.keys(value);
  if (depth === -1) {
    if (keys.length > 0) return '[Object]';
    if (Object.getPrototypeOf(value) === null) return '[Object: null prototype] {}';
    return ctor === 'Object' || ctor === undefined ? '{}' : `${ctor} {}`;
  }
  const proto = Object.getPrototypeOf(value);
  const head = proto === null ? '[Object: null prototype] ' : (ctor === 'Object' || ctor === undefined ? '' : `${ctor} `);
  if (keys.length === 0) return `${head}{}`;
  const parts = keys.slice(0, 7).map((k) => `${k}: ${__wjs_bufInspect(value[k], depth - 1)}`);
  if (keys.length > 7) parts.push(`... ${keys.length - 7} more item${keys.length - 7 > 1 ? 's' : ''}`);
  return `${head}{ ${parts.join(', ')} }`;
}
function __wjs_bufSpecificType(value) {
  if (value === null) return 'null';
  if (value === undefined) return 'undefined';
  const type = typeof value;
  switch (type) {
    case 'bigint': return `type bigint (${value}n)`;
    case 'number':
      if (value === 0) {
        return 1 / value === -Infinity ? 'type number (-0)' : 'type number (0)';
      } else if (value !== value) {
        return 'type number (NaN)';
      } else if (value === Infinity) {
        return 'type number (Infinity)';
      } else if (value === -Infinity) {
        return 'type number (-Infinity)';
      }
      return `type number (${value})`;
    case 'boolean': return value ? 'type boolean (true)' : 'type boolean (false)';
    case 'symbol': return `type symbol (${String(value)})`;
    case 'function': return `function ${value.name}`;
    case 'object': {
      const name = value.constructor?.name;
      if (typeof name === 'string' && name !== '') return `an instance of ${name}`;
      return `${__wjs_bufInspect(value)}`;
    }
    case 'string':
      if (value.length > 28) value = `${value.slice(0, 25)}...`;
      if (value.indexOf("'") === -1) return `type string ('${value}')`;
      return `type string (${JSON.stringify(value)})`;
    default: {
      let inspected = __wjs_bufInspect(value, 0);
      if (inspected.length > 28) inspected = `${inspected.slice(0, 25)}...`;
      return `type ${type} (${inspected})`;
    }
  }
}
// ERR_INVALID_ARG_TYPE（errors.rs 同源；prelude 自含）
// 跨 realm ArrayBuffer 品牌检查（vm.runInNewContext 产物 instanceof 不可靠；
// Object.prototype.toString tag 全 realm 稳定）
function __wjs_bufIsAnyAB(v) {
  if (v instanceof ArrayBuffer) return true;
  if (typeof SharedArrayBuffer !== 'undefined' && v instanceof SharedArrayBuffer) return true;
  const tag = Object.prototype.toString.call(v);
  return tag === '[object ArrayBuffer]' || tag === '[object SharedArrayBuffer]';
}
// detached 容错视图（detached 视空；isAscii/isUtf8 套件口径）
function __wjs_bufAsU8(v) {
  try {
    if (v instanceof ArrayBuffer || Object.prototype.toString.call(v) === '[object ArrayBuffer]') return new Uint8Array(v);
    if (ArrayBuffer.isView(v)) return new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
  } catch {
    return new Uint8Array(0);
  }
  return null;
}
const __wjs_bufKTypes = ['string', 'function', 'number', 'object', 'Function', 'Object', 'boolean', 'bigint', 'symbol'];
const __wjs_bufClassRegExp = /^[A-Z][a-zA-Z0-9]*$/;
function __wjs_bufArgTypeErr(name, expected, actual) {
  if (!Array.isArray(expected)) expected = [expected];
  let msg = 'The ';
  if (name.endsWith(' argument')) {
    msg += `${name} `;
  } else {
    msg += `"${name}" ${name.includes('.') ? 'property' : 'argument'} `;
  }
  msg += 'must be ';
  const types = [];
  const instances = [];
  const other = [];
  for (const value of expected) {
    if (__wjs_bufKTypes.includes(value)) types.push(value.toLowerCase());
    else if (__wjs_bufClassRegExp.test(value)) instances.push(value);
    else other.push(value);
  }
  if (instances.length > 0) {
    const pos = types.indexOf('object');
    if (pos !== -1) {
      types.splice(pos, 1);
      instances.push('Object');
    }
  }
  if (types.length > 0) {
    msg += `${types.length > 1 ? 'one of type' : 'of type'} ${__wjs_bufFormatList(types, 'or')}`;
    if (instances.length > 0 || other.length > 0) msg += ' or ';
  }
  if (instances.length > 0) {
    msg += `an instance of ${__wjs_bufFormatList(instances, 'or')}`;
    if (other.length > 0) msg += ' or ';
  }
  if (other.length > 0) {
    if (other.length > 1) {
      msg += `one of ${__wjs_bufFormatList(other, 'or')}`;
    } else {
      if (other[0].toLowerCase() !== other[0]) msg += 'an ';
      msg += `${other[0]}`;
    }
  }
  msg += `. Received ${__wjs_bufSpecificType(actual)}`;
  const e = new TypeError(msg);
  e.code = 'ERR_INVALID_ARG_TYPE';
  return e;
}
function __wjs_bufNumSep(val) {
  let res = '';
  let i = val.length;
  const start = val[0] === '-' ? 1 : 0;
  for (; i >= start + 4; i -= 3) res = `_${val.slice(i - 3, i)}${res}`;
  return `${val.slice(0, i)}${res}`;
}
function __wjs_bufRangeErr(str, range, input, replaceDefault = false) {
  let msg = replaceDefault ? str : `The value of "${str}" is out of range.`;
  let received;
  if (Number.isInteger(input) && Math.abs(input) > 2 ** 32) {
    received = __wjs_bufNumSep(String(input));
  } else if (typeof input === 'bigint') {
    received = String(input);
    if (input > 2n ** 32n || input < -(2n ** 32n)) received = __wjs_bufNumSep(received);
    received += 'n';
  } else {
    received = __wjs_bufInspect(input);
  }
  msg += ` It must be ${range}. Received ${received}`;
  const e = new RangeError(msg);
  e.code = 'ERR_OUT_OF_RANGE';
  return e;
}
function __wjs_bufOobErr(name = undefined) {
  const msg = name ? `"${name}" is outside of buffer bounds`
                   : 'Attempt to access memory outside buffer bounds';
  const e = new RangeError(msg);
  e.code = 'ERR_BUFFER_OUT_OF_BOUNDS';
  return e;
}
function __wjs_bufArgValueErr(name, value, reason = 'is invalid') {
  let inspected = __wjs_bufInspect(value);
  if (inspected.length > 128) inspected = `${inspected.slice(0, 128)}...`;
  const type = name.includes('.') ? 'property' : 'argument';
  const e = new TypeError(`The ${type} '${name}' ${reason}. Received ${inspected}`);
  e.code = 'ERR_INVALID_ARG_VALUE';
  return e;
}
function __wjs_bufEncErr(encoding) {
  const e = new TypeError(`Unknown encoding: ${encoding}`);
  e.code = 'ERR_UNKNOWN_ENCODING';
  return e;
}
function __wjs_bufSizeErr(bits) {
  const e = new RangeError(`Buffer size must be a multiple of ${bits}`);
  e.code = 'ERR_INVALID_BUFFER_SIZE';
  return e;
}
function __wjs_bufMissingArgsErr(...args) {
  let msg = 'The ';
  const wrapped = args.map((a) => Array.isArray(a) ? a.map((x) => `"${x}"`).join(' or ') : `"${a}"`);
  msg += `${__wjs_bufFormatList(wrapped)} argument${args.length > 1 ? 's' : ''} must be specified`;
  const e = new TypeError(msg);
  e.code = 'ERR_MISSING_ARGS';
  return e;
}
// validators（validators.js 原文口径）
function __wjs_bufValidateNumber(value, name, min = undefined, max) {
  if (typeof value !== 'number') throw __wjs_bufArgTypeErr(name, 'number', value);
  if ((min != null && value < min) || (max != null && value > max) ||
      ((min != null || max != null) && Number.isNaN(value))) {
    throw __wjs_bufRangeErr(
      name,
      `${min != null ? `>= ${min}` : ''}${min != null && max != null ? ' && ' : ''}${max != null ? `<= ${max}` : ''}`,
      value);
  }
}
function __wjs_bufValidateInteger(value, name, min = -Number.MAX_SAFE_INTEGER, max = Number.MAX_SAFE_INTEGER) {
  if (typeof value !== 'number') throw __wjs_bufArgTypeErr(name, 'number', value);
  if (!Number.isInteger(value)) throw __wjs_bufRangeErr(name, 'an integer', value);
  if (value < min || value > max) throw __wjs_bufRangeErr(name, `>= ${min} && <= ${max}`, value);
}
function __wjs_bufValidateString(value, name) {
  if (typeof value !== 'string') throw __wjs_bufArgTypeErr(name, 'string', value);
}
function __wjs_bufValidateArray(value, name, minLength = 0) {
  if (!Array.isArray(value)) throw __wjs_bufArgTypeErr(name, 'Array', value);
  if (value.length < minLength) {
    throw __wjs_bufArgValueErr(name, value, `must have a length of at least ${minLength}`);
  }
}
function __wjs_bufValidateBuffer(buffer, name = 'buffer') {
  if (!ArrayBuffer.isView(buffer)) {
    throw __wjs_bufArgTypeErr(name, ['Buffer', 'TypedArray', 'DataView'], buffer);
  }
}
// normalizeEncoding（internal/util.js 原文）
function __wjs_bufNormalizeEncoding(enc) {
  if (enc == null || enc === 'utf8' || enc === 'utf-8') return 'utf8';
  return __wjs_bufSlowCases(enc);
}
function __wjs_bufSlowCases(enc) {
  switch (enc.length) {
    case 4:
      if (enc === 'UTF8') return 'utf8';
      if (enc === 'ucs2' || enc === 'UCS2') return 'utf16le';
      enc = enc.toLowerCase();
      if (enc === 'utf8') return 'utf8';
      if (enc === 'ucs2') return 'utf16le';
      break;
    case 3:
      if (enc === 'hex' || enc === 'HEX' || enc.toLowerCase() === 'hex') return 'hex';
      break;
    case 5:
      if (enc === 'ascii') return 'ascii';
      if (enc === 'ucs-2') return 'utf16le';
      if (enc === 'UTF-8') return 'utf8';
      if (enc === 'ASCII') return 'ascii';
      if (enc === 'UCS-2') return 'utf16le';
      enc = enc.toLowerCase();
      if (enc === 'utf-8') return 'utf8';
      if (enc === 'ascii') return 'ascii';
      if (enc === 'ucs-2') return 'utf16le';
      break;
    case 6:
      if (enc === 'base64') return 'base64';
      if (enc === 'latin1' || enc === 'binary') return 'latin1';
      if (enc === 'BASE64') return 'base64';
      if (enc === 'LATIN1' || enc === 'BINARY') return 'latin1';
      enc = enc.toLowerCase();
      if (enc === 'base64') return 'base64';
      if (enc === 'latin1' || enc === 'binary') return 'latin1';
      break;
    case 7:
      if (enc === 'utf16le' || enc === 'UTF16LE' || enc.toLowerCase() === 'utf16le') return 'utf16le';
      break;
    case 8:
      if (enc === 'utf-16le' || enc === 'UTF-16LE' || enc.toLowerCase() === 'utf-16le') return 'utf16le';
      break;
    case 9:
      if (enc === 'base64url' || enc === 'BASE64URL' || enc.toLowerCase() === 'base64url') return 'base64url';
      break;
    default:
      if (enc === '') return 'utf8';
  }
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
    // node 实测：ascii 写/解码不掩码（读侧 __wjs_bufEncode 掩 0x7F）
    const s = String(str);
    const out = new Uint8Array(s.length);
    for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i) & 255;
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
    // 10f：ascii 解码掩 0x7F（真机口径；旧实现与 latin1 同形漏掩，套件 fuzz 点名）。
    let s = "";
    for (let i = 0; i < u8.length; i += 0x8000) {
      const part = u8.subarray(i, i + 0x8000);
      const masked = new Uint8Array(part.length);
      for (let j = 0; j < part.length; j++) masked[j] = part[j] & 127;
      s += String.fromCharCode(...masked);
    }
    return s;
  }
  if (enc === "ucs2" || enc === "utf16le" || enc === "utf16") {
    let s = "";
    for (let i = 0; i + 1 < u8.length; i += 2) s += String.fromCharCode(u8[i] | (u8[i + 1] << 8));
    return s;
  }
  throw new TypeError(`Unknown encoding: ${enc}`);
}
// ---- encoding write/slice 静态实现（node C++ binding 的 JS 重实现）----
function __wjs_bufUtf8WriteStatic(buf, string, offset, length) {
  const bytes = new TextEncoder().encode(string);
  let k = Math.min(bytes.length, length);
  // 截断必须落在字符边界（下一字节须为 lead byte，套件 "split char" 门）
  while (k > 0 && k < bytes.length && (bytes[k] & 0xC0) === 0x80) k--;
  if (k > 0) buf.set(bytes.subarray(0, k), offset);
  return k;
}
function __wjs_bufAsciiWriteStatic(buf, string, offset, length) {
  // node 实测：ascii 写不掩码（'über' → [0xFC]）；只有读/slice 掩 0x7F
  let n = 0;
  const L = string.length;
  for (; n < length && n < L; n++) buf[offset + n] = string.charCodeAt(n) & 0xFF;
  return n;
}
function __wjs_bufLatin1WriteStatic(buf, string, offset, length) {
  let n = 0;
  const L = string.length;
  for (; n < length && n < L; n++) buf[offset + n] = string.charCodeAt(n) & 0xFF;
  return n;
}
function __wjs_bufUcs2WriteStatic(buf, string, offset, length) {
  let n = 0;
  const L = string.length;
  for (let i = 0; i < L && n + 1 < length; i++) {
    const c = string.charCodeAt(i);
    buf[offset + n] = c & 255;
    buf[offset + n + 1] = (c >> 8) & 255;
    n += 2;
  }
  return n;
}
function __wjs_bufHexVal(c) {
  if (c >= 48 && c <= 57) return c - 48;
  if (c >= 97 && c <= 102) return c - 87;
  if (c >= 65 && c <= 70) return c - 55;
  return -1;
}
function __wjs_bufHexWriteStatic(buf, string, offset, length) {
  let n = 0;
  for (let i = 0; i + 1 < string.length && n < length; i += 2) {
    const a = __wjs_bufHexVal(string.charCodeAt(i));
    const b = __wjs_bufHexVal(string.charCodeAt(i + 1));
    if (a === -1 || b === -1) break;
    buf[offset + n++] = a * 16 + b;
  }
  return n;
}
const __wjs_bufB64Std = new Int8Array(128).fill(-1);
{
  const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  for (let i = 0; i < chars.length; i++) __wjs_bufB64Std[chars.charCodeAt(i)] = i;
}
const __wjs_bufB64Url = new Int8Array(128).fill(-1);
{
  const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';
  for (let i = 0; i < chars.length; i++) __wjs_bufB64Url[chars.charCodeAt(i)] = i;
}
const __wjs_bufB64Lenient = new Int8Array(128).fill(-1);
{
  const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/-_';
  for (let i = 0; i < chars.length; i++) {
    const c = chars.charCodeAt(i);
    if (c === 0x2B || c === 0x2D) __wjs_bufB64Lenient[c] = 62;      // '+' '-' → 62
    else if (c === 0x2F || c === 0x5F) __wjs_bufB64Lenient[c] = 63; // '/' '_' → 63
    else __wjs_bufB64Lenient[c] = i % 64;
  }
}
function __wjs_bufB64WriteStatic(buf, string, offset, length, url) {
  // node simdutf 口径：解码双字母表均收（'base64' 也接受 -_，反之亦然）；
  // 只有输出（slice）按 flag 选字母表/是否补 padding
  const dec = __wjs_bufB64Lenient;
  let n = 0;
  let carry = -1; // 组内进度：-1 组头；0..2 = 已攒字节数
  let group = 0;
  for (let i = 0; i < string.length && n < length; i++) {
    const c = string.charCodeAt(i);
    if (c === 0x3D) break; // '=' 终止
    if (c >= 128) continue; // 非 ASCII 忽略（node simdutf 忽略无效字符）
    const v = dec[c];
    if (v === -1) continue;
    group = (group << 6) | v;
    carry++;
    if (carry === 3) {
      buf[offset + n++] = (group >> 16) & 255;
      if (n < length) buf[offset + n++] = (group >> 8) & 255;
      if (n < length) buf[offset + n++] = group & 255;
      carry = -1;
      group = 0;
    }
  }
  if (carry === 1 && n < length) buf[offset + n++] = (group >> 4) & 255;
  else if (carry === 2 && n < length) {
    buf[offset + n++] = (group >> 10) & 255;
    if (n < length) buf[offset + n++] = (group >> 2) & 255;
  }
  return n;
}
function __wjs_bufB64Slice(u8, start, end, url) {
  const chars = url
    ? 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_'
    : 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  let s = '';
  for (let i = start; i < end; i += 3) {
    const b0 = u8[i];
    const b1 = i + 1 < end ? u8[i + 1] : 0;
    const b2 = i + 2 < end ? u8[i + 2] : 0;
    const n = Math.min(3, end - i);
    s += chars[b0 >> 2];
    s += chars[((b0 & 3) << 4) | (b1 >> 4)];
    if (n > 1) s += chars[((b1 & 15) << 2) | (b2 >> 6)]; else if (!url) s += '=';
    if (n > 2) s += chars[b2 & 63]; else if (!url) s += '=';
  }
  return s;
}
function __wjs_bufB64ByteLength(str, bytes) {
  if (str.charCodeAt(bytes - 1) === 0x3D) bytes--;
  if (bytes > 1 && str.charCodeAt(bytes - 1) === 0x3D) bytes--;
  return (bytes * 3) >>> 2;
}
// ---- 定长整数/浮点读写（lib/internal/buffer.js 逐字；错误消息真机口径）----
function __wjs_bufCheckBounds(buf, offset, byteLength) {
  __wjs_bufValidateNumber(offset, 'offset');
  if (buf[offset] === undefined || buf[offset + byteLength] === undefined)
    __wjs_bufBoundsError(offset, buf.length - (byteLength + 1));
}
function __wjs_bufCheckInt(value, min, max, buf, offset, byteLength) {
  if (value > max || value < min) {
    const n = typeof min === 'bigint' ? 'n' : '';
    let range;
    if (byteLength > 3) {
      if (min === 0 || min === 0n) {
        range = `>= 0${n} and < 2${n} ** ${(byteLength + 1) * 8}${n}`;
      } else {
        range = `>= -(2${n} ** ${(byteLength + 1) * 8 - 1}${n}) and < 2${n} ** ${(byteLength + 1) * 8 - 1}${n}`;
      }
    } else {
      range = `>= ${min}${n} and <= ${max}${n}`;
    }
    throw __wjs_bufRangeErr('value', range, value);
  }
  __wjs_bufCheckBounds(buf, offset, byteLength);
}
function __wjs_bufBoundsError(value, length, type) {
  if (Math.floor(value) !== value) {
    __wjs_bufValidateNumber(value, type);
    throw __wjs_bufRangeErr(type || 'offset', 'an integer', value);
  }
  if (length < 0)
    throw __wjs_bufOobErr();
  throw __wjs_bufRangeErr(type || 'offset', `>= ${type ? 1 : 0} and <= ${length}`, value);
}
function __wjs_bufReadBigUInt64LE(offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = this[offset];
  const last = this[offset + 7];
  if (first === undefined || last === undefined)
    __wjs_bufBoundsError(offset, this.length - 8);
  const lo = first + this[++offset] * 2 ** 8 + this[++offset] * 2 ** 16 + this[++offset] * 2 ** 24;
  const hi = this[++offset] + this[++offset] * 2 ** 8 + this[++offset] * 2 ** 16 + last * 2 ** 24;
  return BigInt(lo) + (BigInt(hi) << 32n);
}
function __wjs_bufReadBigUInt64BE(offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = this[offset];
  const last = this[offset + 7];
  if (first === undefined || last === undefined)
    __wjs_bufBoundsError(offset, this.length - 8);
  const hi = first * 2 ** 24 + this[++offset] * 2 ** 16 + this[++offset] * 2 ** 8 + this[++offset];
  const lo = this[++offset] * 2 ** 24 + this[++offset] * 2 ** 16 + this[++offset] * 2 ** 8 + last;
  return (BigInt(hi) << 32n) + BigInt(lo);
}
function __wjs_bufReadBigInt64LE(offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = this[offset];
  const last = this[offset + 7];
  if (first === undefined || last === undefined)
    __wjs_bufBoundsError(offset, this.length - 8);
  const val = this[offset + 4] +
    this[offset + 5] * 2 ** 8 +
    this[offset + 6] * 2 ** 16 +
    (last << 24); // Overflow
  return (BigInt(val) << 32n) +
    BigInt(first +
    this[++offset] * 2 ** 8 +
    this[++offset] * 2 ** 16 +
    this[++offset] * 2 ** 24);
}
function __wjs_bufReadBigInt64BE(offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = this[offset];
  const last = this[offset + 7];
  if (first === undefined || last === undefined)
    __wjs_bufBoundsError(offset, this.length - 8);
  const val = (first << 24) + // Overflow
    this[++offset] * 2 ** 16 +
    this[++offset] * 2 ** 8 +
    this[++offset];
  return (BigInt(val) << 32n) +
    BigInt(this[++offset] * 2 ** 24 +
    this[++offset] * 2 ** 16 +
    this[++offset] * 2 ** 8 +
    last);
}
function __wjs_bufReadUIntLE(offset, byteLength) {
  if (offset === undefined) throw __wjs_bufArgTypeErr('offset', 'number', offset);
  if (byteLength === 6) return __wjs_bufReadUInt48LE(this, offset);
  if (byteLength === 5) return __wjs_bufReadUInt40LE(this, offset);
  if (byteLength === 3) return __wjs_bufReadUInt24LE(this, offset);
  if (byteLength === 4) return __wjs_bufReadUInt32LE(this, offset);
  if (byteLength === 2) return __wjs_bufReadUInt16LE(this, offset);
  if (byteLength === 1) return __wjs_bufReadUInt8(this, offset);
  __wjs_bufBoundsError(byteLength, 6, 'byteLength');
}
function __wjs_bufReadUInt48LE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 5];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 6);
  return first + buf[++offset] * 2 ** 8 + buf[++offset] * 2 ** 16 + buf[++offset] * 2 ** 24 +
    (buf[++offset] + last * 2 ** 8) * 2 ** 32;
}
function __wjs_bufReadUInt40LE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 4];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 5);
  return first + buf[++offset] * 2 ** 8 + buf[++offset] * 2 ** 16 + buf[++offset] * 2 ** 24 + last * 2 ** 32;
}
function __wjs_bufReadUInt32LE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 3];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 4);
  return first + buf[++offset] * 2 ** 8 + buf[++offset] * 2 ** 16 + last * 2 ** 24;
}
function __wjs_bufReadUInt24LE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 2];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 3);
  return first + buf[++offset] * 2 ** 8 + last * 2 ** 16;
}
function __wjs_bufReadUInt16LE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 1];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 2);
  return first + last * 2 ** 8;
}
function __wjs_bufReadUInt8(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const val = buf[offset];
  if (val === undefined) __wjs_bufBoundsError(offset, buf.length - 1);
  return val;
}
function __wjs_bufReadUIntBE(offset, byteLength) {
  if (offset === undefined) throw __wjs_bufArgTypeErr('offset', 'number', offset);
  if (byteLength === 6) return __wjs_bufReadUInt48BE(this, offset);
  if (byteLength === 5) return __wjs_bufReadUInt40BE(this, offset);
  if (byteLength === 3) return __wjs_bufReadUInt24BE(this, offset);
  if (byteLength === 4) return __wjs_bufReadUInt32BE(this, offset);
  if (byteLength === 2) return __wjs_bufReadUInt16BE(this, offset);
  if (byteLength === 1) return __wjs_bufReadUInt8(this, offset);
  __wjs_bufBoundsError(byteLength, 6, 'byteLength');
}
function __wjs_bufReadUInt48BE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 5];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 6);
  return (first * 2 ** 8 + buf[++offset]) * 2 ** 32 + buf[++offset] * 2 ** 24 +
    buf[++offset] * 2 ** 16 + buf[++offset] * 2 ** 8 + last;
}
function __wjs_bufReadUInt40BE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 4];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 5);
  return first * 2 ** 32 + buf[++offset] * 2 ** 24 + buf[++offset] * 2 ** 16 + buf[++offset] * 2 ** 8 + last;
}
function __wjs_bufReadUInt32BE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 3];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 4);
  return first * 2 ** 24 + buf[++offset] * 2 ** 16 + buf[++offset] * 2 ** 8 + last;
}
function __wjs_bufReadUInt24BE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 2];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 3);
  return first * 2 ** 16 + buf[++offset] * 2 ** 8 + last;
}
function __wjs_bufReadUInt16BE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 1];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 2);
  return first * 2 ** 8 + last;
}
function __wjs_bufReadIntLE(offset, byteLength) {
  if (offset === undefined) throw __wjs_bufArgTypeErr('offset', 'number', offset);
  if (byteLength === 6) return __wjs_bufReadInt48LE(this, offset);
  if (byteLength === 5) return __wjs_bufReadInt40LE(this, offset);
  if (byteLength === 3) return __wjs_bufReadInt24LE(this, offset);
  if (byteLength === 4) return __wjs_bufReadInt32LE(this, offset);
  if (byteLength === 2) return __wjs_bufReadInt16LE(this, offset);
  if (byteLength === 1) return __wjs_bufReadInt8(this, offset);
  __wjs_bufBoundsError(byteLength, 6, 'byteLength');
}
function __wjs_bufReadInt48LE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 5];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 6);
  const val = buf[offset + 4] + last * 2 ** 8;
  return (val | (val & 2 ** 15) * 0x1fffe) * 2 ** 32 + first + buf[++offset] * 2 ** 8 +
    buf[++offset] * 2 ** 16 + buf[++offset] * 2 ** 24;
}
function __wjs_bufReadInt40LE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 4];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 5);
  return (last | (last & 2 ** 7) * 0x1fffffe) * 2 ** 32 + first + buf[++offset] * 2 ** 8 +
    buf[++offset] * 2 ** 16 + buf[++offset] * 2 ** 24;
}
function __wjs_bufReadInt32LE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 3];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 4);
  return first + buf[++offset] * 2 ** 8 + buf[++offset] * 2 ** 16 + (last << 24);
}
function __wjs_bufReadInt24LE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 2];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 3);
  const val = first + buf[++offset] * 2 ** 8 + last * 2 ** 16;
  return val | (val & 2 ** 23) * 0x1fe;
}
function __wjs_bufReadInt16LE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 1];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 2);
  const val = first + last * 2 ** 8;
  return val | (val & 2 ** 15) * 0x1fffe;
}
function __wjs_bufReadInt8(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const val = buf[offset];
  if (val === undefined) __wjs_bufBoundsError(offset, buf.length - 1);
  return val | (val & 2 ** 7) * 0x1fffffe;
}
function __wjs_bufReadIntBE(offset, byteLength) {
  if (offset === undefined) throw __wjs_bufArgTypeErr('offset', 'number', offset);
  if (byteLength === 6) return __wjs_bufReadInt48BE(this, offset);
  if (byteLength === 5) return __wjs_bufReadInt40BE(this, offset);
  if (byteLength === 3) return __wjs_bufReadInt24BE(this, offset);
  if (byteLength === 4) return __wjs_bufReadInt32BE(this, offset);
  if (byteLength === 2) return __wjs_bufReadInt16BE(this, offset);
  if (byteLength === 1) return __wjs_bufReadInt8(this, offset);
  __wjs_bufBoundsError(byteLength, 6, 'byteLength');
}
function __wjs_bufReadInt48BE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 5];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 6);
  const val = buf[++offset] + first * 2 ** 8;
  return (val | (val & 2 ** 15) * 0x1fffe) * 2 ** 32 + buf[++offset] * 2 ** 24 +
    buf[++offset] * 2 ** 16 + buf[++offset] * 2 ** 8 + last;
}
function __wjs_bufReadInt40BE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 4];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 5);
  return (first | (first & 2 ** 7) * 0x1fffffe) * 2 ** 32 + buf[++offset] * 2 ** 24 +
    buf[++offset] * 2 ** 16 + buf[++offset] * 2 ** 8 + last;
}
function __wjs_bufReadInt32BE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 3];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 4);
  return (first << 24) + buf[++offset] * 2 ** 16 + buf[++offset] * 2 ** 8 + last;
}
function __wjs_bufReadInt24BE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 2];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 3);
  const val = first * 2 ** 16 + buf[++offset] * 2 ** 8 + last;
  return val | (val & 2 ** 23) * 0x1fe;
}
function __wjs_bufReadInt16BE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 1];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 2);
  const val = first * 2 ** 8 + last;
  return val | (val & 2 ** 15) * 0x1fffe;
}
// 浮点（float32Array 转换板，原文同款）
const __wjs_bufF32 = new Float32Array(1);
const __wjs_bufU8F32 = new Uint8Array(__wjs_bufF32.buffer);
const __wjs_bufF64 = new Float64Array(1);
const __wjs_bufU8F64 = new Uint8Array(__wjs_bufF64.buffer);
__wjs_bufF32[0] = -1;
function __wjs_bufReadFloatLE(offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = this[offset];
  const last = this[offset + 3];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, this.length - 4);
  __wjs_bufU8F32[0] = first;
  __wjs_bufU8F32[1] = this[++offset];
  __wjs_bufU8F32[2] = this[++offset];
  __wjs_bufU8F32[3] = last;
  return __wjs_bufF32[0];
}
function __wjs_bufReadFloatBE(offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = this[offset];
  const last = this[offset + 3];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, this.length - 4);
  __wjs_bufU8F32[3] = first;
  __wjs_bufU8F32[2] = this[++offset];
  __wjs_bufU8F32[1] = this[++offset];
  __wjs_bufU8F32[0] = last;
  return __wjs_bufF32[0];
}
function __wjs_bufReadDoubleLE(offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = this[offset];
  const last = this[offset + 7];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, this.length - 8);
  __wjs_bufU8F64[0] = first;
  __wjs_bufU8F64[1] = this[++offset];
  __wjs_bufU8F64[2] = this[++offset];
  __wjs_bufU8F64[3] = this[++offset];
  __wjs_bufU8F64[4] = this[++offset];
  __wjs_bufU8F64[5] = this[++offset];
  __wjs_bufU8F64[6] = this[++offset];
  __wjs_bufU8F64[7] = last;
  return __wjs_bufF64[0];
}
function __wjs_bufReadDoubleBE(offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = this[offset];
  const last = this[offset + 7];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, this.length - 8);
  __wjs_bufU8F64[7] = first;
  __wjs_bufU8F64[6] = this[++offset];
  __wjs_bufU8F64[5] = this[++offset];
  __wjs_bufU8F64[4] = this[++offset];
  __wjs_bufU8F64[3] = this[++offset];
  __wjs_bufU8F64[2] = this[++offset];
  __wjs_bufU8F64[1] = this[++offset];
  __wjs_bufU8F64[0] = last;
  return __wjs_bufF64[0];
}
function __wjs_bufWriteBigU64LE(buf, value, offset, min, max) {
  __wjs_bufCheckInt(value, min, max, buf, offset, 7);
  let lo = Number(value & 0xffffffffn);
  buf[offset++] = lo;
  lo = lo >> 8;
  buf[offset++] = lo;
  lo = lo >> 8;
  buf[offset++] = lo;
  lo = lo >> 8;
  buf[offset++] = lo;
  let hi = Number(value >> 32n & 0xffffffffn);
  buf[offset++] = hi;
  hi = hi >> 8;
  buf[offset++] = hi;
  hi = hi >> 8;
  buf[offset++] = hi;
  hi = hi >> 8;
  buf[offset++] = hi;
  return offset;
}
function __wjs_bufWriteBigU64BE(buf, value, offset, min, max) {
  __wjs_bufCheckInt(value, min, max, buf, offset, 7);
  let lo = Number(value & 0xffffffffn);
  buf[offset + 7] = lo;
  lo = lo >> 8;
  buf[offset + 6] = lo;
  lo = lo >> 8;
  buf[offset + 5] = lo;
  lo = lo >> 8;
  buf[offset + 4] = lo;
  let hi = Number(value >> 32n & 0xffffffffn);
  buf[offset + 3] = hi;
  hi = hi >> 8;
  buf[offset + 2] = hi;
  hi = hi >> 8;
  buf[offset + 1] = hi;
  hi = hi >> 8;
  buf[offset] = hi;
  return offset + 8;
}
function __wjs_bufWriteUIntLE(value, offset, byteLength) {
  if (byteLength === 6) return __wjs_bufWriteU48LE(this, value, offset, 0, 0xffffffffffff);
  if (byteLength === 5) return __wjs_bufWriteU40LE(this, value, offset, 0, 0xffffffffff);
  if (byteLength === 3) return __wjs_bufWriteU24LE(this, value, offset, 0, 0xffffff);
  if (byteLength === 4) return __wjs_bufWriteU32LE(this, value, offset, 0, 0xffffffff);
  if (byteLength === 2) return __wjs_bufWriteU16LE(this, value, offset, 0, 0xffff);
  if (byteLength === 1) return __wjs_bufWriteU8(this, value, offset, 0, 0xff);
  __wjs_bufBoundsError(byteLength, 6, 'byteLength');
}
function __wjs_bufWriteU48LE(buf, value, offset, min, max) {
  value = +value;
  __wjs_bufCheckInt(value, min, max, buf, offset, 5);
  const newVal = Math.floor(value * 2 ** -32);
  buf[offset++] = value;
  value = value >>> 8;
  buf[offset++] = value;
  value = value >>> 8;
  buf[offset++] = value;
  value = value >>> 8;
  buf[offset++] = value;
  buf[offset++] = newVal;
  buf[offset++] = (newVal >>> 8);
  return offset;
}
function __wjs_bufWriteU40LE(buf, value, offset, min, max) {
  value = +value;
  __wjs_bufCheckInt(value, min, max, buf, offset, 4);
  const newVal = value;
  buf[offset++] = value;
  value = value >>> 8;
  buf[offset++] = value;
  value = value >>> 8;
  buf[offset++] = value;
  value = value >>> 8;
  buf[offset++] = value;
  buf[offset++] = Math.floor(newVal * 2 ** -32);
  return offset;
}
function __wjs_bufWriteU32LE(buf, value, offset, min, max) {
  value = +value;
  __wjs_bufCheckInt(value, min, max, buf, offset, 3);
  buf[offset++] = value;
  value = value >>> 8;
  buf[offset++] = value;
  value = value >>> 8;
  buf[offset++] = value;
  value = value >>> 8;
  buf[offset++] = value;
  return offset;
}
function __wjs_bufWriteU24LE(buf, value, offset, min, max) {
  value = +value;
  __wjs_bufCheckInt(value, min, max, buf, offset, 2);
  buf[offset++] = value;
  value = value >>> 8;
  buf[offset++] = value;
  value = value >>> 8;
  buf[offset++] = value;
  return offset;
}
function __wjs_bufWriteU16LE(buf, value, offset, min, max) {
  value = +value;
  __wjs_bufCheckInt(value, min, max, buf, offset, 1);
  buf[offset++] = value;
  buf[offset++] = (value >>> 8);
  return offset;
}
function __wjs_bufWriteU8(buf, value, offset, min, max) {
  value = +value;
  __wjs_bufValidateNumber(offset, 'offset');
  if (value > max || value < min) {
    throw __wjs_bufRangeErr('value', `>= ${min} and <= ${max}`, value);
  }
  if (buf[offset] === undefined) __wjs_bufBoundsError(offset, buf.length - 1);
  buf[offset] = value;
  return offset + 1;
}
function __wjs_bufWriteUIntBE(value, offset, byteLength) {
  if (byteLength === 6) return __wjs_bufWriteU48BE(this, value, offset, 0, 0xffffffffffff);
  if (byteLength === 5) return __wjs_bufWriteU40BE(this, value, offset, 0, 0xffffffffff);
  if (byteLength === 3) return __wjs_bufWriteU24BE(this, value, offset, 0, 0xffffff);
  if (byteLength === 4) return __wjs_bufWriteU32BE(this, value, offset, 0, 0xffffffff);
  if (byteLength === 2) return __wjs_bufWriteU16BE(this, value, offset, 0, 0xffff);
  if (byteLength === 1) return __wjs_bufWriteU8(this, value, offset, 0, 0xff);
  __wjs_bufBoundsError(byteLength, 6, 'byteLength');
}
function __wjs_bufWriteU48BE(buf, value, offset, min, max) {
  value = +value;
  __wjs_bufCheckInt(value, min, max, buf, offset, 5);
  const newVal = Math.floor(value * 2 ** -32);
  buf[offset++] = (newVal >>> 8);
  buf[offset++] = newVal;
  buf[offset + 3] = value;
  value = value >>> 8;
  buf[offset + 2] = value;
  value = value >>> 8;
  buf[offset + 1] = value;
  value = value >>> 8;
  buf[offset] = value;
  return offset + 4;
}
function __wjs_bufWriteU40BE(buf, value, offset, min, max) {
  value = +value;
  __wjs_bufCheckInt(value, min, max, buf, offset, 4);
  buf[offset++] = Math.floor(value * 2 ** -32);
  buf[offset + 3] = value;
  value = value >>> 8;
  buf[offset + 2] = value;
  value = value >>> 8;
  buf[offset + 1] = value;
  value = value >>> 8;
  buf[offset] = value;
  return offset + 4;
}
function __wjs_bufWriteU32BE(buf, value, offset, min, max) {
  value = +value;
  __wjs_bufCheckInt(value, min, max, buf, offset, 3);
  buf[offset + 3] = value;
  value = value >>> 8;
  buf[offset + 2] = value;
  value = value >>> 8;
  buf[offset + 1] = value;
  value = value >>> 8;
  buf[offset] = value;
  return offset + 4;
}
function __wjs_bufWriteU24BE(buf, value, offset, min, max) {
  value = +value;
  __wjs_bufCheckInt(value, min, max, buf, offset, 2);
  buf[offset + 2] = value;
  value = value >>> 8;
  buf[offset + 1] = value;
  value = value >>> 8;
  buf[offset] = value;
  return offset + 3;
}
function __wjs_bufWriteU16BE(buf, value, offset, min, max) {
  value = +value;
  __wjs_bufCheckInt(value, min, max, buf, offset, 1);
  buf[offset++] = (value >>> 8);
  buf[offset++] = value;
  return offset;
}
function __wjs_bufWriteFloatLE(val, offset = 0) {
  val = +val;
  __wjs_bufCheckBounds(this, offset, 3);
  __wjs_bufF32[0] = val;
  this[offset++] = __wjs_bufU8F32[0];
  this[offset++] = __wjs_bufU8F32[1];
  this[offset++] = __wjs_bufU8F32[2];
  this[offset++] = __wjs_bufU8F32[3];
  return offset;
}
function __wjs_bufWriteFloatBE(val, offset = 0) {
  val = +val;
  __wjs_bufCheckBounds(this, offset, 3);
  __wjs_bufF32[0] = val;
  this[offset++] = __wjs_bufU8F32[3];
  this[offset++] = __wjs_bufU8F32[2];
  this[offset++] = __wjs_bufU8F32[1];
  this[offset++] = __wjs_bufU8F32[0];
  return offset;
}
function __wjs_bufWriteDoubleLE(val, offset = 0) {
  val = +val;
  __wjs_bufCheckBounds(this, offset, 7);
  __wjs_bufF64[0] = val;
  this[offset++] = __wjs_bufU8F64[0];
  this[offset++] = __wjs_bufU8F64[1];
  this[offset++] = __wjs_bufU8F64[2];
  this[offset++] = __wjs_bufU8F64[3];
  this[offset++] = __wjs_bufU8F64[4];
  this[offset++] = __wjs_bufU8F64[5];
  this[offset++] = __wjs_bufU8F64[6];
  this[offset++] = __wjs_bufU8F64[7];
  return offset;
}
function __wjs_bufWriteDoubleBE(val, offset = 0) {
  val = +val;
  __wjs_bufCheckBounds(this, offset, 7);
  __wjs_bufF64[0] = val;
  this[offset++] = __wjs_bufU8F64[7];
  this[offset++] = __wjs_bufU8F64[6];
  this[offset++] = __wjs_bufU8F64[5];
  this[offset++] = __wjs_bufU8F64[4];
  this[offset++] = __wjs_bufU8F64[3];
  this[offset++] = __wjs_bufU8F64[2];
  this[offset++] = __wjs_bufU8F64[1];
  this[offset++] = __wjs_bufU8F64[0];
  return offset;
}
// write 静态包装（internal/buffer.js utf8Write 等的越界校验口径）
function __wjs_bufUtf8Write(buf, string, offset = 0, length) {
  offset = Number(offset);
  if (offset < 0 || offset > buf.byteLength) throw __wjs_bufOobErr('offset');
  if (length === undefined) length = buf.byteLength - offset;
  else length = Number(length);
  if (length < 0 || length > buf.byteLength - offset) throw __wjs_bufOobErr('length');
  return __wjs_bufUtf8WriteStatic(buf, string, offset, length);
}
function __wjs_bufAsciiWrite(buf, string, offset = 0, length) {
  offset = Number(offset);
  if (offset < 0 || offset > buf.byteLength) throw __wjs_bufOobErr('offset');
  if (length === undefined) length = buf.byteLength - offset;
  else length = Number(length);
  if (length < 0 || length > buf.byteLength - offset) throw __wjs_bufOobErr('length');
  return __wjs_bufAsciiWriteStatic(buf, string, offset, length);
}
function __wjs_bufLatin1Write(buf, string, offset = 0, length) {
  offset = Number(offset);
  if (offset < 0 || offset > buf.byteLength) throw __wjs_bufOobErr('offset');
  if (length === undefined) length = buf.byteLength - offset;
  else length = Number(length);
  if (length < 0 || length > buf.byteLength - offset) throw __wjs_bufOobErr('length');
  return __wjs_bufLatin1WriteStatic(buf, string, offset, length);
}
function __wjs_bufUcs2Write(buf, string, offset = 0, length) {
  offset = Number(offset);
  if (offset < 0 || offset > buf.byteLength) throw __wjs_bufOobErr('offset');
  if (length === undefined) length = buf.byteLength - offset;
  else length = Number(length);
  if (length < 0 || length > buf.byteLength - offset) throw __wjs_bufOobErr('length');
  return __wjs_bufUcs2WriteStatic(buf, string, offset, length);
}
function __wjs_bufHexWrite(buf, string, offset = 0, length) {
  offset = Number(offset);
  if (offset < 0 || offset > buf.byteLength) throw __wjs_bufOobErr('offset');
  if (length === undefined) length = buf.byteLength - offset;
  else length = Number(length);
  if (length < 0 || length > buf.byteLength - offset) throw __wjs_bufOobErr('length');
  return __wjs_bufHexWriteStatic(buf, string, offset, length);
}
function __wjs_bufBase64Write(buf, string, offset = 0, length) {
  offset = Number(offset);
  if (offset < 0 || offset > buf.byteLength) throw __wjs_bufOobErr('offset');
  if (length === undefined) length = buf.byteLength - offset;
  else length = Number(length);
  if (length < 0 || length > buf.byteLength - offset) throw __wjs_bufOobErr('length');
  return __wjs_bufB64WriteStatic(buf, string, offset, length, false);
}
function __wjs_bufBase64urlWrite(buf, string, offset = 0, length) {
  offset = Number(offset);
  if (offset < 0 || offset > buf.byteLength) throw __wjs_bufOobErr('offset');
  if (length === undefined) length = buf.byteLength - offset;
  else length = Number(length);
  if (length < 0 || length > buf.byteLength - offset) throw __wjs_bufOobErr('length');
  return __wjs_bufB64WriteStatic(buf, string, offset, length, true);
}
// slice 静态（utf8 用 TextDecoder；ascii 掩 0x7F，10f 真机口径）
function __wjs_bufUtf8Slice(u8, start, end) {
  // 预检先于 decode（utf8 串长 ≤ 字节数；>1GB decode 会撞 SM 崩溃面，
  // 不给引擎机会）——node kStringMaxLength 字节等同口径的保守近似
  if (end - start > kStringMaxLength) {
    const e = new Error(`Cannot create a string longer than ${kStringMaxLength} characters`);
    e.code = 'ERR_STRING_TOO_LONG';
    throw e;
  }
  const s = new TextDecoder().decode(u8.subarray(start, end));
  if (s.length > kStringMaxLength) {
    // node 引擎级字串上限在 SM 更高，按 node 口径在 slice 层模拟
    // （E('ERR_STRING_TOO_LONG', 'Cannot create a string longer than %s characters', Error)）
    const e = new Error(`Cannot create a string longer than ${kStringMaxLength} characters`);
    e.code = 'ERR_STRING_TOO_LONG';
    throw e;
  }
  return s;
}
function __wjs_bufAsciiSlice(u8, start, end) {
  let s = '';
  for (let i = start; i < end; i++) s += String.fromCharCode(u8[i] & 0x7F);
  return s;
}
function __wjs_bufLatin1Slice(u8, start, end) {
  let s = '';
  for (let i = start; i < end; i++) s += String.fromCharCode(u8[i]);
  return s;
}
function __wjs_bufUcs2Slice(u8, start, end) {
  let s = '';
  for (let i = start; i + 1 < end; i += 2) s += String.fromCharCode(u8[i] | (u8[i + 1] << 8));
  return s;
}
function __wjs_bufHexSlice(u8, start, end) {
  let s = '';
  for (let i = start; i < end; i++) s += u8[i].toString(16).padStart(2, '0');
  return s;
}
// indexOf 族（C++ SearchString 的 JS 重实现）
function __wjs_bufIndexOfNumber(buf, val, byteOffset, dir, end) {
  val = val >>> 0;
  val = val & 0xFF;
  const len = buf.length;
  const limit = Math.min(end === undefined ? len : end, len);
  if (dir) {
    // 负偏移 = 距尾偏移（node SearchString 口径；越界收敛到 0）
    if (byteOffset < 0) byteOffset = Math.max(len + byteOffset, 0);
    if (byteOffset >= limit) return -1;
    for (let i = byteOffset; i < limit; i++) {
      if (buf[i] === val) return i;
    }
    return -1;
  }
  if (byteOffset < 0) {
    byteOffset = len + byteOffset;
    if (byteOffset < 0) return -1;
  }
  let i = Math.min(byteOffset, limit - 1);
  for (; i >= 0; i--) {
    if (buf[i] === val) return i;
  }
  return -1;
}
function __wjs_bufIndexOfBytes(buf, needle, byteOffset, dir, end, align = 1) {
  const len = buf.length;
  const nlen = needle.length;
  if (nlen === 0) {
    // 空 needle 钳到搜索上限 end（套件 "clamp to search_end" 门）
    const lim0 = Math.min(end === undefined ? len : end, len);
    return Math.min(Math.max(byteOffset, 0), lim0);
  }
  const limit = Math.min(end === undefined ? len : end, len);
  if (dir) {
    let i = byteOffset < 0 ? Math.max(len + byteOffset, 0) : Math.max(byteOffset, 0);
    // utf16le 对齐搜索（node C++ 口径：只在偶字节偏移命中，套件 allChars 门）
    if (align === 2 && i % 2 !== 0) i++;
    for (; i <= limit - nlen; i += align) {
      let ok = true;
      for (let j = 0; j < nlen; j++) {
        if (buf[i + j] !== needle[j]) { ok = false; break; }
      }
      if (ok) return i;
    }
    return -1;
  }
  if (byteOffset < 0) {
    byteOffset = len + byteOffset;
    if (byteOffset < 0) return -1;
  }
  let i = Math.min(byteOffset, limit - nlen);
  if (align === 2 && i % 2 !== 0) i--;
  for (; i >= 0; i -= align) {
    let ok = true;
    for (let j = 0; j < nlen; j++) {
      if (buf[i + j] !== needle[j]) { ok = false; break; }
    }
    if (ok) return i;
  }
  return -1;
}
function __wjs_bufIndexOfString(buf, val, byteOffset, enc, dir, end) {
  const ops = __wjs_bufEncodingOps[enc];
  return __wjs_bufIndexOfBytes(buf, __wjs_bufEncodeStr(val, ops), byteOffset, dir, end,
                               ops.encoding === 'utf16le' ? 2 : 1);
}
function __wjs_bufEncodeStr(str, ops) {
  const tmp = new __wjs_bufFastBuffer(ops.byteLength(str) || 1);
  const actual = ops.write(tmp, str, 0, tmp.length);
  return tmp.subarray(0, actual);
}
// ---- encodingOps（lib/buffer.js 原文结构）----
function __wjs_bufByteLengthUtf8(string) {
  return new TextEncoder().encode(string).length;
}
const __wjs_bufEncodingOps = {
  utf8: {
    encoding: 'utf8',
    byteLength: __wjs_bufByteLengthUtf8,
    write: __wjs_bufUtf8Write,
    slice: __wjs_bufUtf8Slice,
    indexOf: (buf, val, byteOffset, dir, end) =>
      __wjs_bufIndexOfString(buf, val, byteOffset, 'utf8', dir, end),
  },
  ucs2: {
    encoding: 'utf16le',
    byteLength: (string) => string.length * 2,
    write: __wjs_bufUcs2Write,
    slice: __wjs_bufUcs2Slice,
    indexOf: (buf, val, byteOffset, dir, end) =>
      __wjs_bufIndexOfString(buf, val, byteOffset, 'utf16le', dir, end),
  },
  utf16le: {
    encoding: 'utf16le',
    byteLength: (string) => string.length * 2,
    write: __wjs_bufUcs2Write,
    slice: __wjs_bufUcs2Slice,
    indexOf: (buf, val, byteOffset, dir, end) =>
      __wjs_bufIndexOfString(buf, val, byteOffset, 'utf16le', dir, end),
  },
  latin1: {
    encoding: 'latin1',
    byteLength: (string) => string.length,
    write: __wjs_bufLatin1Write,
    slice: __wjs_bufLatin1Slice,
    indexOf: (buf, val, byteOffset, dir, end) =>
      __wjs_bufIndexOfString(buf, val, byteOffset, 'latin1', dir, end),
  },
  ascii: {
    encoding: 'ascii',
    byteLength: (string) => string.length,
    write: __wjs_bufAsciiWrite,
    slice: __wjs_bufAsciiSlice,
    indexOf: (buf, val, byteOffset, dir, end) =>
      __wjs_bufIndexOfBytes(buf, __wjs_bufEncodeStr(val, __wjs_bufEncodingOps.ascii), byteOffset, dir, end),
  },
  base64: {
    encoding: 'base64',
    byteLength: (string) => __wjs_bufB64ByteLength(string, string.length),
    write: __wjs_bufBase64Write,
    slice: (u8, start, end) => __wjs_bufB64Slice(u8, start, end, false),
    indexOf: (buf, val, byteOffset, dir, end) =>
      __wjs_bufIndexOfBytes(buf, __wjs_bufEncodeStr(val, __wjs_bufEncodingOps.base64), byteOffset, dir, end),
  },
  base64url: {
    encoding: 'base64url',
    byteLength: (string) => __wjs_bufB64ByteLength(string, string.length),
    write: __wjs_bufBase64urlWrite,
    slice: (u8, start, end) => __wjs_bufB64Slice(u8, start, end, true),
    indexOf: (buf, val, byteOffset, dir, end) =>
      __wjs_bufIndexOfBytes(buf, __wjs_bufEncodeStr(val, __wjs_bufEncodingOps.base64url), byteOffset, dir, end),
  },
  hex: {
    encoding: 'hex',
    byteLength: (string) => string.length >>> 1,
    write: __wjs_bufHexWrite,
    slice: __wjs_bufHexSlice,
    indexOf: (buf, val, byteOffset, dir, end) =>
      __wjs_bufIndexOfBytes(buf, __wjs_bufEncodeStr(val, __wjs_bufEncodingOps.hex), byteOffset, dir, end),
  },
};
function __wjs_bufGetEncodingOps(encoding) {
  encoding += '';
  switch (encoding.length) {
    case 4:
      if (encoding === 'utf8') return __wjs_bufEncodingOps.utf8;
      if (encoding === 'ucs2') return __wjs_bufEncodingOps.ucs2;
      encoding = encoding.toLowerCase();
      if (encoding === 'utf8') return __wjs_bufEncodingOps.utf8;
      if (encoding === 'ucs2') return __wjs_bufEncodingOps.ucs2;
      break;
    case 5:
      if (encoding === 'utf-8') return __wjs_bufEncodingOps.utf8;
      if (encoding === 'ascii') return __wjs_bufEncodingOps.ascii;
      if (encoding === 'ucs-2') return __wjs_bufEncodingOps.ucs2;
      encoding = encoding.toLowerCase();
      if (encoding === 'utf-8') return __wjs_bufEncodingOps.utf8;
      if (encoding === 'ascii') return __wjs_bufEncodingOps.ascii;
      if (encoding === 'ucs-2') return __wjs_bufEncodingOps.ucs2;
      break;
    case 7:
      if (encoding === 'utf16le' || encoding.toLowerCase() === 'utf16le')
        return __wjs_bufEncodingOps.utf16le;
      break;
    case 8:
      if (encoding === 'utf-16le' || encoding.toLowerCase() === 'utf-16le')
        return __wjs_bufEncodingOps.utf16le;
      break;
    case 6:
      if (encoding === 'latin1' || encoding === 'binary') return __wjs_bufEncodingOps.latin1;
      if (encoding === 'base64') return __wjs_bufEncodingOps.base64;
      encoding = encoding.toLowerCase();
      if (encoding === 'latin1' || encoding === 'binary') return __wjs_bufEncodingOps.latin1;
      if (encoding === 'base64') return __wjs_bufEncodingOps.base64;
      break;
    case 3:
      if (encoding === 'hex' || encoding.toLowerCase() === 'hex') return __wjs_bufEncodingOps.hex;
      break;
    case 9:
      if (encoding === 'base64url' || encoding.toLowerCase() === 'base64url')
        return __wjs_bufEncodingOps.base64url;
      break;
  }
}
// ---- Buffer 类本体（lib/buffer.js 原文）----
class __wjs_bufFastBuffer extends Uint8Array {}
let __wjs_bufWarned = false;
function __wjs_bufShowFlaggedDeprecation() {
  if (__wjs_bufWarned) return;
  // isInsideNodeModules(3) 的栈走查近似（SM 栈格式；DEP0169 同法）
  const saved = Error.stackTraceLimit;
  Error.stackTraceLimit = 5;
  const stack = new Error().stack || '';
  Error.stackTraceLimit = saved;
  const frames = stack.split('\n').slice(1, 5);
  if (frames.some((f) => f.includes('node_modules'))) return;
  __wjs_bufWarned = true;
  try {
    process.emitWarning(
      'Buffer() is deprecated due to security and usability issues. ' +
      'Please use the Buffer.alloc(), Buffer.allocUnsafe(), or Buffer.from() ' +
      'methods instead.', 'DeprecationWarning', 'DEP0005');
  } catch { }
}
function Buffer(arg, encodingOrOffset, length) {
  __wjs_bufShowFlaggedDeprecation();
  if (typeof arg === 'number') {
    if (typeof encodingOrOffset === 'string') {
      throw __wjs_bufArgTypeErr('string', 'string', arg);
    }
    return Buffer.alloc(arg);
  }
  return Buffer.from(arg, encodingOrOffset, length);
}
Object.defineProperty(Buffer, Symbol.species, {
  enumerable: false,
  configurable: true,
  get() { return __wjs_bufFastBuffer; },
});
Object.setPrototypeOf(Buffer, Uint8Array);
Buffer.prototype = __wjs_bufFastBuffer.prototype;
Buffer.prototype.constructor = Buffer;
Buffer.poolSize = 64 * 1024;
// 10f：小串池化（Node lib/buffer.js fromStringFast 口径：< poolSize/2 走池，
// 8 字节对齐，满即新池；`a.buffer === b.buffer` 套件门）。池 AB 进
// `__wjs_bufPooled`（WeakSet，全局暴露供 worker  transfer 拒收），
// `ArrayBuffer.prototype.transfer` 对池内 AB 抛 TypeError（真机同款不可转移）。
let __wjs_bufPoolAB = new ArrayBuffer(Buffer.poolSize);
const __wjs_bufPooled = new WeakSet([__wjs_bufPoolAB]);
globalThis.__wjs_bufPooled = __wjs_bufPooled;
let __wjs_bufPoolOffset = 0;
function __wjs_bufPoolAlign() {
  if (__wjs_bufPoolOffset & 0x7) __wjs_bufPoolOffset = (__wjs_bufPoolOffset + 7) & ~7;
}
{
  const __wjs_bufOrigTransfer = ArrayBuffer.prototype.transfer;
  Object.defineProperty(ArrayBuffer.prototype, 'transfer', {
    value: function() {
      if (__wjs_bufPooled.has(this)) {
        throw new TypeError('Cannot transfer a pooled Buffer ArrayBuffer');
      }
      return __wjs_bufOrigTransfer.call(this);
    },
    writable: true, configurable: true,
  });
}
function __wjs_bufToInteger(n, defaultVal) {
  n = +n;
  if (!Number.isNaN(n) && n >= Number.MIN_SAFE_INTEGER && n <= Number.MAX_SAFE_INTEGER) {
    return ((n % 1) === 0 ? n : Math.floor(n));
  }
  return defaultVal;
}
function __wjs_bufCopyImpl(source, target, targetStart, sourceStart, sourceEnd) {
  if (!ArrayBuffer.isView(source))
    throw __wjs_bufArgTypeErr('source', ['Buffer', 'Uint8Array'], source);
  if (!ArrayBuffer.isView(target))
    throw __wjs_bufArgTypeErr('target', ['Buffer', 'Uint8Array'], target);
  if (targetStart === undefined) {
    targetStart = 0;
  } else {
    targetStart = Number.isInteger(targetStart) ? targetStart : __wjs_bufToInteger(targetStart, 0);
    if (targetStart < 0) throw __wjs_bufRangeErr('targetStart', '>= 0', targetStart);
  }
  if (sourceStart === undefined) {
    sourceStart = 0;
  } else {
    sourceStart = Number.isInteger(sourceStart) ? sourceStart : __wjs_bufToInteger(sourceStart, 0);
    if (sourceStart < 0 || sourceStart > source.byteLength)
      throw __wjs_bufRangeErr('sourceStart', `>= 0 && <= ${source.byteLength}`, sourceStart);
  }
  if (sourceEnd === undefined) {
    sourceEnd = source.byteLength;
  } else {
    sourceEnd = Number.isInteger(sourceEnd) ? sourceEnd : __wjs_bufToInteger(sourceEnd, 0);
    if (sourceEnd < 0) throw __wjs_bufRangeErr('sourceEnd', '>= 0', sourceEnd);
  }
  if (targetStart >= target.byteLength || sourceStart >= sourceEnd)
    return 0;
  return __wjs_bufCopyActual(source, target, targetStart, sourceStart, sourceEnd);
}
function __wjs_bufCopyActual(source, target, targetStart, sourceStart, sourceEnd) {
  if (sourceEnd - sourceStart > target.byteLength - targetStart)
    sourceEnd = sourceStart + target.byteLength - targetStart;
  let nb = sourceEnd - sourceStart;
  const sourceLen = source.byteLength - sourceStart;
  if (nb > sourceLen) nb = sourceLen;
  if (nb <= 0) return 0;
  // 字节级拷贝（node _copy memmove 口径；目标非 u8 时 targetStart 按元素索引
  // 换算字节偏移，如 Uint16Array——test-buffer-copy "packed into 16-bit" 门）
  const elemSize = target.length ? (target.byteLength / target.length) : 1;
  const byteStart = targetStart * elemSize;
  const room = target.byteLength - byteStart;
  if (nb > room) nb = room;
  if (nb <= 0) return 0;
  const dst = new Uint8Array(target.buffer, target.byteOffset + byteStart, nb);
  const src = new Uint8Array(source.buffer, source.byteOffset + sourceStart, nb);
  dst.set(src); // 同 buffer 时 set 按 memmove 语义
  return nb;
}
Buffer.from = function from(value, encodingOrOffset, length) {
  if (typeof value === 'string')
    return __wjs_bufFromString(value, encodingOrOffset);
  if (typeof value === 'object' && value !== null) {
    if (__wjs_bufIsAnyAB(value)) {
      // 10f：伪 AB（原型链伪造、无内部槽，如 `Object.setPrototypeOf(AB, ArrayBuffer)`）
      // 须拒为 ERR_INVALID_ARG_TYPE（V8 IsArrayBuffer 品牌检查口径）；直接进
      // FromArrayBuffer 会在引擎内抛 incompatible 文案，断言对不上。
      let branded = true;
      try { void value.byteLength; } catch { branded = false; }
      if (branded)
        return __wjs_bufFromArrayBuffer(value, encodingOrOffset, length);
      // 落空到尾部统一 invalid-arg（`an instance of AB`，__wjs_bufSpecificType 口径）
    } else {
      const valueOf = value.valueOf && value.valueOf();
      if (valueOf != null && valueOf !== value &&
          (typeof valueOf === 'string' || typeof valueOf === 'object')) {
        return from(valueOf, encodingOrOffset, length);
      }
      const b = __wjs_bufFromObject(value);
      if (b) return b;
      if (typeof value[Symbol.toPrimitive] === 'function') {
        const primitive = value[Symbol.toPrimitive]('string');
        if (typeof primitive === 'string') {
          return __wjs_bufFromString(primitive, encodingOrOffset);
        }
      }
    }
  }
  throw __wjs_bufArgTypeErr(
    'first argument',
    ['string', 'Buffer', 'ArrayBuffer', 'Array', 'Array-like Object'],
    value,
  );
};
Buffer.copyBytesFrom = function copyBytesFrom(view, offset, length) {
  if (!ArrayBuffer.isView(view) || view instanceof DataView) {
    throw __wjs_bufArgTypeErr('view', ['TypedArray'], view);
  }
  const viewLength = view.length;
  if (viewLength === 0) return new __wjs_bufFastBuffer();
  let start = 0;
  let end = viewLength;
  if (offset !== undefined) {
    __wjs_bufValidateInteger(offset, 'offset', 0);
    if (offset >= viewLength) return new __wjs_bufFastBuffer();
    start = offset;
  }
  if (length !== undefined) {
    __wjs_bufValidateInteger(length, 'length', 0);
    end = Math.min(start + length, viewLength);
  }
  if (end <= start) return new __wjs_bufFastBuffer();
  const viewByteLength = view.byteLength;
  const elementSize = viewByteLength / viewLength;
  const srcByteOffset = view.byteOffset + start * elementSize;
  const srcByteLength = (end - start) * elementSize;
  return __wjs_bufFromArrayLike(new Uint8Array(view.buffer, srcByteOffset, srcByteLength));
};
const __wjs_bufOf = (...items) => {
  const len = items.length;
  const newObj = new __wjs_bufFastBuffer(len);
  for (let k = 0; k < len; k++) newObj[k] = items[k];
  return newObj;
};
Buffer.of = __wjs_bufOf;
Buffer.alloc = function alloc(size, fill, encoding) {
  __wjs_bufValidateNumber(size, 'size', 0, kMaxLength);
  if (fill !== undefined && fill !== 0 && size > 0) {
    const buf = new __wjs_bufFastBuffer(size);
    return __wjs_bufFill(buf, fill, 0, buf.length, encoding);
  }
  return new __wjs_bufFastBuffer(size);
};
Buffer.allocUnsafe = function allocUnsafe(size) {
  // alignment 形参不做（无 O_DIRECT 场景；真机 26 有，记档）
  __wjs_bufValidateNumber(size, 'size', 0, kMaxLength);
  return size <= 0 ? new __wjs_bufFastBuffer() : new __wjs_bufFastBuffer(size);
};
Buffer.allocUnsafeSlow = function allocUnsafeSlow(size) {
  __wjs_bufValidateNumber(size, 'size', 0, kMaxLength);
  return size <= 0 ? new __wjs_bufFastBuffer() : new __wjs_bufFastBuffer(size);
};
function __wjs_bufFromStringFast(string, ops) {
  const length = ops.byteLength(string);
  // 池路径（小串共享池 AB；actual 按写入实长推进，与 Node fromStringFast 同口径）
  if (length > 0 && length < (Buffer.poolSize >>> 1)) {
    __wjs_bufPoolAlign();
    if (length > __wjs_bufPoolAB.byteLength - __wjs_bufPoolOffset) {
      __wjs_bufPoolAB = new ArrayBuffer(Buffer.poolSize);
      __wjs_bufPooled.add(__wjs_bufPoolAB);
      __wjs_bufPoolOffset = 0;
    }
    const scratch = new Uint8Array(__wjs_bufPoolAB);
    const actual = ops.write(scratch, string, __wjs_bufPoolOffset, length);
    const b = new __wjs_bufFastBuffer(__wjs_bufPoolAB, __wjs_bufPoolOffset, actual);
    __wjs_bufPoolOffset += actual;
    return b;
  }
  const buf = Buffer.allocUnsafeSlow(length);
  const actual = ops.write(buf, string, 0, length);
  return actual < length ? new __wjs_bufFastBuffer(buf.buffer, 0, actual) : buf;
}
function __wjs_bufFromString(string, encoding) {
  let ops;
  if (!encoding || encoding === 'utf8' || typeof encoding !== 'string') {
    ops = __wjs_bufEncodingOps.utf8;
  } else {
    ops = __wjs_bufGetEncodingOps(encoding);
    if (ops === undefined) throw __wjs_bufEncErr(encoding);
  }
  return string.length === 0 ? new __wjs_bufFastBuffer() : __wjs_bufFromStringFast(string, ops);
}
function __wjs_bufFromArrayBuffer(obj, byteOffset, length) {
  if (byteOffset === undefined) {
    byteOffset = 0;
  } else {
    byteOffset = +byteOffset;
    if (Number.isNaN(byteOffset)) byteOffset = 0;
  }
  const maxLength = obj.byteLength - byteOffset;
  if (maxLength < 0) throw __wjs_bufOobErr('offset');
  if (length !== undefined) {
    length = +length;
    if (length > 0) {
      if (length > maxLength) throw __wjs_bufOobErr('length');
    } else {
      length = 0;
    }
  }
  return new __wjs_bufFastBuffer(obj, byteOffset, length);
}
function __wjs_bufFromArrayLike(obj) {
  const { length } = obj;
  if (length <= 0) return new __wjs_bufFastBuffer();
  return new __wjs_bufFastBuffer(obj);
}
function __wjs_bufFromObject(obj) {
  if (obj.length !== undefined || (obj.buffer != null && __wjs_bufIsAnyAB(obj.buffer))) {
    if (typeof obj.length !== 'number') {
      return new __wjs_bufFastBuffer();
    }
    return __wjs_bufFromArrayLike(obj);
  }
  if (obj.type === 'Buffer' && Array.isArray(obj.data)) {
    return __wjs_bufFromArrayLike(obj.data);
  }
}
Buffer.isBuffer = function isBuffer(b) {
  return b instanceof Buffer;
};
Buffer.compare = function compare(buf1, buf2) {
  if (!__wjs_bufIsU8(buf1)) throw __wjs_bufArgTypeErr('buf1', ['Buffer', 'Uint8Array'], buf1);
  if (!__wjs_bufIsU8(buf2)) throw __wjs_bufArgTypeErr('buf2', ['Buffer', 'Uint8Array'], buf2);
  if (buf1 === buf2) return 0;
  return __wjs_bufCompare(buf1, buf2);
};
function __wjs_bufIsU8(v) { return v instanceof Uint8Array; }
function __wjs_bufCompare(a, b) {
  const n = Math.min(a.length, b.length);
  for (let i = 0; i < n; i++) {
    if (a[i] !== b[i]) return a[i] < b[i] ? -1 : 1;
  }
  return a.length === b.length ? 0 : (a.length < b.length ? -1 : 1);
}
Buffer.isEncoding = function isEncoding(encoding) {
  return typeof encoding === 'string' && encoding.length !== 0 &&
         __wjs_bufNormalizeEncoding(encoding) !== undefined;
};
Buffer.concat = function concat(list, length) {
  __wjs_bufValidateArray(list, 'list');
  if (list.length === 0) return new __wjs_bufFastBuffer();
  if (length === undefined) {
    length = 0;
    for (let i = 0; i < list.length; i++) {
      const buf = list[i];
      if (!__wjs_bufIsU8(buf)) {
        throw __wjs_bufArgTypeErr(`list[${i}]`, ['Buffer', 'Uint8Array'], buf);
      }
      length += buf.byteLength;
    }
    const buffer = length <= 0 ? new __wjs_bufFastBuffer() : new __wjs_bufFastBuffer(length);
    let pos = 0;
    for (let i = 0; i < list.length; i++) {
      const buf = list[i];
      buffer.set(buf, pos);
      pos += buf.byteLength;
    }
    return buffer;
  }
  __wjs_bufValidateInteger(length, 'length', 0);
  for (let i = 0; i < list.length; i++) {
    if (!__wjs_bufIsU8(list[i])) {
      throw __wjs_bufArgTypeErr(`list[${i}]`, ['Buffer', 'Uint8Array'], list[i]);
    }
  }
  const buffer = length <= 0 ? new __wjs_bufFastBuffer() : new __wjs_bufFastBuffer(length);
  let pos = 0;
  for (let i = 0; i < list.length; i++) {
    const buf = list[i];
    const bufLength = buf.byteLength;
    if (pos + bufLength > length) {
      buffer.set(buf.subarray(0, length - pos), pos);
      pos = length;
      break;
    }
    buffer.set(buf, pos);
    pos += bufLength;
  }
  if (pos < length) {
    __wjs_bufTAFill.call(buffer, 0, pos, length);
  }
  return buffer;
};
function __wjs_bufByteLengthUtf8(string) { return new TextEncoder().encode(string).length; }
function __wjs_bufByteLength(string, encoding) {
  if (typeof string !== 'string') {
    if (ArrayBuffer.isView(string) || __wjs_bufIsAnyAB(string)) {
      try {
        return string.byteLength;
      } catch {
        return 0; // detached 视空
      }
    }
    throw __wjs_bufArgTypeErr('string', ['string', 'Buffer', 'ArrayBuffer'], string);
  }
  const len = string.length;
  if (len === 0) return 0;
  if (!encoding || encoding === 'utf8') {
    return __wjs_bufByteLengthUtf8(string);
  }
  if (encoding === 'ascii') {
    return len;
  }
  const ops = __wjs_bufGetEncodingOps(encoding);
  if (ops === undefined) {
    return __wjs_bufByteLengthUtf8(string);
  }
  return ops.byteLength(string);
}
Buffer.byteLength = __wjs_bufByteLength;
Buffer.prototype.copy = function copy(target, targetStart, sourceStart, sourceEnd) {
  return __wjs_bufCopyImpl(this, target, targetStart, sourceStart, sourceEnd);
};
Buffer.prototype.toString = function toString(encoding, start, end) {
  if (arguments.length === 0) {
    return __wjs_bufUtf8Slice(this, 0, this.length);
  }
  const bufferLength = this.length;
  if (start <= 0) start = 0;
  else if (start >= bufferLength) return '';
  else start = Math.trunc(start) || 0;
  if (end === undefined || end > bufferLength) end = bufferLength;
  else end = Math.trunc(end) || 0;
  if (end <= start) return '';
  if (encoding === undefined) return __wjs_bufUtf8Slice(this, start, end);
  const ops = __wjs_bufGetEncodingOps(encoding);
  if (ops === undefined) throw __wjs_bufEncErr(encoding);
  return ops.slice(this, start, end);
};
Buffer.prototype.equals = function equals(otherBuffer) {
  if (!__wjs_bufIsU8(otherBuffer)) {
    throw __wjs_bufArgTypeErr('otherBuffer', ['Buffer', 'Uint8Array'], otherBuffer);
  }
  if (this === otherBuffer) return true;
  const len = this.byteLength;
  if (len !== otherBuffer.byteLength) return false;
  return len === 0 || __wjs_bufCompare(this, otherBuffer) === 0;
};
let INSPECT_MAX_BYTES = 50;
const __wjs_bufCustomInspect = Symbol.for('nodejs.util.inspect.custom');
Buffer.prototype[__wjs_bufCustomInspect] = function inspect(recurseTimes, ctx) {
  const max = INSPECT_MAX_BYTES;
  const actualMax = Math.min(max, this.length);
  const remaining = this.length - max;
  let str = __wjs_bufHexSlice(this, 0, actualMax).replace(/(.{2})/g, '$1 ').trim();
  if (remaining > 0) str += ` ... ${remaining} more byte${remaining > 1 ? 's' : ''}`;
  // Inspect special properties as well, if possible（lib/buffer.js extras 段）。
  if (ctx && typeof globalThis.__wjs_inspect === 'function') {
    let extras = false;
    const obj = { };
    Object.keys(this).forEach((key) => {
      if (/^\d+$/.test(key)) return;
      extras = true;
      obj[key] = this[key];
    });
    if (extras) {
      if (this.length !== 0) str += ', ';
      str += Object.keys(obj)
        .map((key) => `${key}: ${globalThis.__wjs_inspect(obj[key], { ...ctx, breakLength: Infinity, compact: true })}`)
        .join(', ');
    }
  }
  let constructorName = 'Buffer';
  try {
    const { constructor } = this;
    if (typeof constructor === 'function' &&
        Object.prototype.hasOwnProperty.call(constructor, 'name')) {
      constructorName = constructor.name;
    }
  } catch { }
  return `<${constructorName} ${str}>`;
};
Buffer.prototype.inspect = Buffer.prototype[__wjs_bufCustomInspect];
function __wjs_bufCompareOffset(source, target, targetStart, sourceStart, targetEnd, sourceEnd) {
  const tlen = targetEnd - targetStart;
  const slen = sourceEnd - sourceStart;
  const n = Math.min(tlen, slen);
  for (let i = 0; i < n; i++) {
    const a = source[sourceStart + i];
    const b = target[targetStart + i];
    if (a !== b) return a < b ? -1 : 1;
  }
  return slen === tlen ? 0 : (slen < tlen ? -1 : 1);
}
Buffer.prototype.compare = function compare(target, targetStart, targetEnd, sourceStart, sourceEnd) {
  if (!__wjs_bufIsU8(target)) {
    throw __wjs_bufArgTypeErr('target', ['Buffer', 'Uint8Array'], target);
  }
  if (arguments.length === 1) return __wjs_bufCompare(this, target);
  if (targetStart === undefined) targetStart = 0;
  else __wjs_bufValidateOffset(targetStart, 'targetStart');
  if (targetEnd === undefined) targetEnd = target.length;
  else __wjs_bufValidateOffset(targetEnd, 'targetEnd', 0, target.length);
  if (sourceStart === undefined) sourceStart = 0;
  else __wjs_bufValidateOffset(sourceStart, 'sourceStart');
  if (sourceEnd === undefined) sourceEnd = this.length;
  else __wjs_bufValidateOffset(sourceEnd, 'sourceEnd', 0, this.length);
  if (sourceStart >= sourceEnd) return (targetStart >= targetEnd ? 0 : -1);
  if (targetStart >= targetEnd) return 1;
  return __wjs_bufCompareOffset(this, target, targetStart, sourceStart, targetEnd, sourceEnd);
};
function __wjs_bufBidirectionalIndexOf(buffer, val, byteOffset, end, encoding, dir) {
  __wjs_bufValidateBuffer(buffer);
  if (typeof byteOffset === 'string') {
    encoding = byteOffset;
    byteOffset = undefined;
  } else if (byteOffset > 0x7fffffff) {
    byteOffset = 0x7fffffff;
  } else if (byteOffset < -0x80000000) {
    byteOffset = -0x80000000;
  }
  byteOffset = +byteOffset;
  if (Number.isNaN(byteOffset)) {
    byteOffset = dir ? 0 : (buffer.length || buffer.byteLength);
  }
  dir = !!dir;
  if (typeof val === 'number') {
    return __wjs_bufIndexOfNumber(buffer, val >>> 0, byteOffset, dir, end);
  }
  let ops;
  if (encoding === undefined) ops = __wjs_bufEncodingOps.utf8;
  else ops = __wjs_bufGetEncodingOps(encoding);
  if (typeof val === 'string') {
    if (ops === undefined) throw __wjs_bufEncErr(encoding);
    return ops.indexOf(buffer, val, byteOffset, dir, end);
  }
  if (__wjs_bufIsU8(val)) {
    // node indexOfBuffer：needle 按给定 encoding 重编码（'ucs2' 把 'f' 编成
    // [0x66,0x00] 两字节——奇尾字节补零，非丢弃）
    if (ops !== undefined && ops.encoding === 'utf16le') {
      const out = new Uint8Array(val.length + (val.length % 2));
      for (let i = 0; i < val.length; i++) out[i] = val[i];
      return __wjs_bufIndexOfBytes(buffer, out, byteOffset, dir, end, 2);
    }
    if (ops !== undefined && ops.encoding !== 'utf8') {
      const reencoded = __wjs_bufEncodeStr(ops.slice(val, 0, val.length), ops);
      return __wjs_bufIndexOfBytes(buffer, reencoded, byteOffset, dir, end);
    }
    return __wjs_bufIndexOfBytes(buffer, val, byteOffset, dir, end);
  }
  throw __wjs_bufArgTypeErr('value', ['number', 'string', 'Buffer', 'Uint8Array'], val);
}
Buffer.prototype.indexOf = function indexOf(val, offset, end, encoding) {
  if (typeof end === 'string') {
    encoding = end;
    end = this.length;
  } else if (end === undefined) {
    end = this.length;
  }
  return __wjs_bufBidirectionalIndexOf(this, val, offset, end, encoding, true);
};
Buffer.prototype.lastIndexOf = function lastIndexOf(val, offset, end, encoding) {
  if (typeof end === 'string') {
    encoding = end;
    end = this.length;
  } else if (end === undefined) {
    end = this.length;
  }
  return __wjs_bufBidirectionalIndexOf(this, val, offset, end, encoding, false);
};
Buffer.prototype.includes = function includes(val, offset, end, encoding) {
  if (typeof end === 'string') {
    encoding = end;
    end = this.length;
  } else if (end === undefined) {
    end = this.length;
  }
  return __wjs_bufBidirectionalIndexOf(this, val, offset, end, encoding, true) !== -1;
};
function __wjs_bufValidateOffset(value, name, min = 0, max = kMaxLength) {
  __wjs_bufValidateInteger(value, name, min, max);
}
function __wjs_bufFill(buf, value, offset, end, encoding) {
  if (value === undefined) value = 0; // node fill() 无参零填（bindingFill undefined 口径）
  if (typeof value === 'string') {
    if (offset === undefined || typeof offset === 'string') {
      encoding = offset;
      offset = 0;
      end = buf.length;
    } else if (typeof end === 'string') {
      encoding = end;
      end = buf.length;
    }
    const normalizedEncoding = __wjs_bufNormalizeEncoding(encoding);
    if (normalizedEncoding === undefined) {
      __wjs_bufValidateString(encoding, 'encoding');
      throw __wjs_bufEncErr(encoding);
    }
    if (value.length === 0) {
      value = 0;
    } else if (value.length === 1) {
      if (normalizedEncoding === 'utf8' || normalizedEncoding === 'ascii') {
        const code = value.charCodeAt(0);
        if (code < 128) value = code;
      } else if (normalizedEncoding === 'latin1') {
        value = value.charCodeAt(0);
      }
    }
  } else {
    encoding = undefined;
  }
  if (offset === undefined) {
    offset = 0;
    end = buf.length;
  } else {
    __wjs_bufValidateOffset(offset, 'offset');
    if (end === undefined) {
      end = buf.length;
    } else {
      __wjs_bufValidateOffset(end, 'end', 0, buf.length);
    }
    if (offset >= end) return buf;
  }
  if (typeof value === 'number') {
    const byteLen = buf.byteLength;
    const fillLength = end - offset;
    if (offset > end || fillLength + offset > byteLen) throw __wjs_bufOobErr();
    __wjs_bufTAFill.call(buf, value, offset, end);
  } else {
    const res = __wjs_bufBindingFill(buf, value, offset, end, encoding);
    if (res < 0) {
      if (res === -1) throw __wjs_bufArgValueErr('value', value);
      throw __wjs_bufOobErr();
    }
  }
  return buf;
}
function __wjs_bufBindingFill(buf, value, offset, end, encoding) {
  let bytes;
  if (typeof value === 'string') {
    const ops = __wjs_bufGetEncodingOps(encoding === undefined ? 'utf8' : encoding);
    if (ops === undefined) return -1;
    const tmp = new __wjs_bufFastBuffer(ops.byteLength(value) || 1);
    const actual = ops.write(tmp, value, 0, tmp.length);
    bytes = tmp.subarray(0, actual);
  } else if (__wjs_bufIsU8(value)) {
    if (value.length === 0) return -1;
    bytes = value;
  } else {
    return -1;
  }
  if (bytes.length === 0) return -1;
  const room = end - offset;
  if (room < bytes.length) bytes = bytes.subarray(0, room);
  buf.set(bytes, offset);
  for (let i = offset + bytes.length; i < end; i += bytes.length) {
    const n = Math.min(bytes.length, end - i);
    buf.set(bytes.subarray(0, n), i);
  }
  return 0;
}
Buffer.prototype.fill = function fill(value, offset, end, encoding) {
  return __wjs_bufFill(this, value, offset, end, encoding);
};
Buffer.prototype.write = function write(string, offset, length, encoding) {
  const bufferLength = this.length;
  if (offset === undefined) {
    return __wjs_bufUtf8Write(this, string, 0, bufferLength);
  }
  if (length === undefined && typeof offset === 'string') {
    encoding = offset;
    length = bufferLength;
    offset = 0;
  } else {
    __wjs_bufValidateOffset(offset, 'offset', 0, bufferLength);
    const remaining = bufferLength - offset;
    if (length === undefined) {
      length = remaining;
    } else if (typeof length === 'string') {
      encoding = length;
      length = remaining;
    } else {
      __wjs_bufValidateOffset(length, 'length', 0, bufferLength);
      if (length > remaining) length = remaining;
    }
  }
  if (!encoding || encoding === 'utf8') return __wjs_bufUtf8Write(this, string, offset, length);
  if (encoding === 'ascii') return __wjs_bufAsciiWrite(this, string, offset, length);
  const ops = __wjs_bufGetEncodingOps(encoding);
  if (ops === undefined) throw __wjs_bufEncErr(encoding);
  return ops.write(this, string, offset, length);
};
Buffer.prototype.toJSON = function toJSON() {
  const bufferLength = this.length;
  if (bufferLength > 0) {
    const data = new Array(bufferLength);
    for (let i = 0; i < bufferLength; ++i) data[i] = this[i];
    return { type: 'Buffer', data };
  }
  return { type: 'Buffer', data: [] };
};
function __wjs_bufAdjustOffset(offset, length) {
  offset = Math.trunc(offset);
  if (offset === 0) return 0;
  if (offset < 0) {
    offset += length;
    return offset > 0 ? offset : 0;
  }
  if (offset < length) return offset;
  return Number.isNaN(offset) ? 0 : length;
}
Buffer.prototype.subarray = function subarray(start, end) {
  const srcLength = this.length;
  start = __wjs_bufAdjustOffset(start, srcLength);
  end = end !== undefined ? __wjs_bufAdjustOffset(end, srcLength) : srcLength;
  const newLength = end > start ? end - start : 0;
  return new __wjs_bufFastBuffer(this.buffer, this.byteOffset + start, newLength);
};
Buffer.prototype.slice = function slice(start, end) {
  return this.subarray(start, end);
};
function __wjs_bufSwap(b, n, m) {
  const i = b[n];
  b[n] = b[m];
  b[m] = i;
}
Buffer.prototype.swap16 = function swap16() {
  const len = this.length;
  if (len % 2 !== 0) throw __wjs_bufSizeErr('16-bits');
  for (let i = 0; i < len; i += 2) __wjs_bufSwap(this, i, i + 1);
  return this;
};
Buffer.prototype.swap32 = function swap32() {
  const len = this.length;
  if (len % 4 !== 0) throw __wjs_bufSizeErr('32-bits');
  for (let i = 0; i < len; i += 4) {
    __wjs_bufSwap(this, i, i + 3);
    __wjs_bufSwap(this, i + 1, i + 2);
  }
  return this;
};
Buffer.prototype.swap64 = function swap64() {
  const len = this.length;
  if (len % 8 !== 0) throw __wjs_bufSizeErr('64-bits');
  for (let i = 0; i < len; i += 8) {
    __wjs_bufSwap(this, i, i + 7);
    __wjs_bufSwap(this, i + 1, i + 6);
    __wjs_bufSwap(this, i + 2, i + 5);
    __wjs_bufSwap(this, i + 3, i + 4);
  }
  return this;
};
Buffer.prototype.toLocaleString = Buffer.prototype.toString;
// parent/offset getter（lib/buffer.js 原文；prototype 上，非自有属性）
Object.defineProperty(Buffer.prototype, 'parent', {
  enumerable: true,
  get() {
    if (!(this instanceof Buffer)) return undefined;
    return this.buffer;
  },
});
Object.defineProperty(Buffer.prototype, 'offset', {
  enumerable: true,
  get() {
    if (!(this instanceof Buffer)) return undefined;
    return this.byteOffset;
  },
});
// read/write 原型方法挂载（addBufferPrototypeMethods 原文结构）
Buffer.prototype.readBigUInt64LE = __wjs_bufReadBigUInt64LE;
Buffer.prototype.readBigUInt64BE = __wjs_bufReadBigUInt64BE;
Buffer.prototype.readBigUint64LE = __wjs_bufReadBigUInt64LE;
Buffer.prototype.readBigUint64BE = __wjs_bufReadBigUInt64BE;
Buffer.prototype.readBigInt64LE = __wjs_bufReadBigInt64LE;
Buffer.prototype.readBigInt64BE = __wjs_bufReadBigInt64BE;
Buffer.prototype.writeBigUInt64LE = function (value, offset = 0) {
  return __wjs_bufWriteBigU64LE(this, value, offset, 0n, 0xffffffffffffffffn);
};
Buffer.prototype.writeBigUInt64BE = function (value, offset = 0) {
  return __wjs_bufWriteBigU64BE(this, value, offset, 0n, 0xffffffffffffffffn);
};
Buffer.prototype.writeBigUint64LE = Buffer.prototype.writeBigUInt64LE;
Buffer.prototype.writeBigUint64BE = Buffer.prototype.writeBigUInt64BE;
Buffer.prototype.writeBigInt64LE = function (value, offset = 0) {
  return __wjs_bufWriteBigU64LE(this, value, offset, -0x8000000000000000n, 0x7fffffffffffffffn);
};
Buffer.prototype.writeBigInt64BE = function (value, offset = 0) {
  return __wjs_bufWriteBigU64BE(this, value, offset, -0x8000000000000000n, 0x7fffffffffffffffn);
};
Buffer.prototype.readUIntLE = __wjs_bufReadUIntLE;
Buffer.prototype.readUInt32LE = function (offset) { return __wjs_bufReadUInt32LE(this, offset); };
Buffer.prototype.readUInt16LE = function (offset) { return __wjs_bufReadUInt16LE(this, offset); };
Buffer.prototype.readUInt8 = function (offset) { return __wjs_bufReadUInt8(this, offset); };
Buffer.prototype.readUIntBE = __wjs_bufReadUIntBE;
Buffer.prototype.readUInt32BE = function (offset) { return __wjs_bufReadUInt32BE(this, offset); };
Buffer.prototype.readUInt16BE = function (offset) { return __wjs_bufReadUInt16BE(this, offset); };
Buffer.prototype.readUintLE = __wjs_bufReadUIntLE;
Buffer.prototype.readUint32LE = Buffer.prototype.readUInt32LE;
Buffer.prototype.readUint16LE = Buffer.prototype.readUInt16LE;
Buffer.prototype.readUint8 = Buffer.prototype.readUInt8;
Buffer.prototype.readUintBE = __wjs_bufReadUIntBE;
Buffer.prototype.readUint32BE = Buffer.prototype.readUInt32BE;
Buffer.prototype.readUint16BE = Buffer.prototype.readUInt16BE;
Buffer.prototype.readIntLE = __wjs_bufReadIntLE;
Buffer.prototype.readInt32LE = function (offset) { return __wjs_bufReadInt32LE(this, offset); };
Buffer.prototype.readInt16LE = function (offset) { return __wjs_bufReadInt16LE(this, offset); };
Buffer.prototype.readInt8 = function (offset) { return __wjs_bufReadInt8(this, offset); };
Buffer.prototype.readIntBE = __wjs_bufReadIntBE;
Buffer.prototype.readInt32BE = function (offset) { return __wjs_bufReadInt32BE(this, offset); };
Buffer.prototype.readInt16BE = function (offset) { return __wjs_bufReadInt16BE(this, offset); };
Buffer.prototype.writeUIntLE = __wjs_bufWriteUIntLE;
Buffer.prototype.writeUInt32LE = function (value, offset = 0) { return __wjs_bufWriteU32LE(this, value, offset, 0, 0xffffffff); };
Buffer.prototype.writeUInt16LE = function (value, offset = 0) { return __wjs_bufWriteU16LE(this, value, offset, 0, 0xffff); };
Buffer.prototype.writeUInt8 = function (value, offset = 0) { return __wjs_bufWriteU8(this, value, offset, 0, 0xff); };
Buffer.prototype.writeUIntBE = __wjs_bufWriteUIntBE;
Buffer.prototype.writeUInt32BE = function (value, offset = 0) { return __wjs_bufWriteU32BE(this, value, offset, 0, 0xffffffff); };
Buffer.prototype.writeUInt16BE = function (value, offset = 0) { return __wjs_bufWriteU16BE(this, value, offset, 0, 0xffff); };
Buffer.prototype.writeUintLE = __wjs_bufWriteUIntLE;
Buffer.prototype.writeUint32LE = Buffer.prototype.writeUInt32LE;
Buffer.prototype.writeUint16LE = Buffer.prototype.writeUInt16LE;
Buffer.prototype.writeUint8 = Buffer.prototype.writeUInt8;
Buffer.prototype.writeUintBE = __wjs_bufWriteUIntBE;
Buffer.prototype.writeUint32BE = Buffer.prototype.writeUInt32BE;
Buffer.prototype.writeUint16BE = Buffer.prototype.writeUInt16BE;
Buffer.prototype.writeIntLE = __wjs_bufWriteIntLE;
function __wjs_bufWriteIntBE(value, offset, byteLength) {
  if (byteLength === 6) return __wjs_bufWriteU48BE(this, value, offset, -0x800000000000, 0x7fffffffffff);
  if (byteLength === 5) return __wjs_bufWriteU40BE(this, value, offset, -0x8000000000, 0x7fffffffff);
  if (byteLength === 3) return __wjs_bufWriteU24BE(this, value, offset, -0x800000, 0x7fffff);
  if (byteLength === 4) return __wjs_bufWriteU32BE(this, value, offset, -0x80000000, 0x7fffffff);
  if (byteLength === 2) return __wjs_bufWriteU16BE(this, value, offset, -0x8000, 0x7fff);
  if (byteLength === 1) return __wjs_bufWriteU8(this, value, offset, -0x80, 0x7f);
  __wjs_bufBoundsError(byteLength, 6, 'byteLength');
}
function __wjs_bufWriteIntLE(value, offset, byteLength) {
  if (byteLength === 6) return __wjs_bufWriteU48LE(this, value, offset, -0x800000000000, 0x7fffffffffff);
  if (byteLength === 5) return __wjs_bufWriteU40LE(this, value, offset, -0x8000000000, 0x7fffffffff);
  if (byteLength === 3) return __wjs_bufWriteU24LE(this, value, offset, -0x800000, 0x7fffff);
  if (byteLength === 4) return __wjs_bufWriteU32LE(this, value, offset, -0x80000000, 0x7fffffff);
  if (byteLength === 2) return __wjs_bufWriteU16LE(this, value, offset, -0x8000, 0x7fff);
  if (byteLength === 1) return __wjs_bufWriteU8(this, value, offset, -0x80, 0x7f);
  __wjs_bufBoundsError(byteLength, 6, 'byteLength');
}
Buffer.prototype.writeInt32LE = function (value, offset = 0) { return __wjs_bufWriteU32LE(this, value, offset, -0x80000000, 0x7fffffff); };
Buffer.prototype.writeInt16LE = function (value, offset = 0) { return __wjs_bufWriteU16LE(this, value, offset, -0x8000, 0x7fff); };
Buffer.prototype.writeInt8 = function (value, offset = 0) { return __wjs_bufWriteU8(this, value, offset, -0x80, 0x7f); };
Buffer.prototype.writeIntBE = __wjs_bufWriteIntBE;
Buffer.prototype.writeInt32BE = function (value, offset = 0) { return __wjs_bufWriteU32BE(this, value, offset, -0x80000000, 0x7fffffff); };
Buffer.prototype.writeInt16BE = function (value, offset = 0) { return __wjs_bufWriteU16BE(this, value, offset, -0x8000, 0x7fff); };
Buffer.prototype.readFloatLE = __wjs_bufReadFloatLE;
Buffer.prototype.readFloatBE = __wjs_bufReadFloatBE;
Buffer.prototype.readDoubleLE = __wjs_bufReadDoubleLE;
Buffer.prototype.readDoubleBE = __wjs_bufReadDoubleBE;
Buffer.prototype.writeFloatLE = __wjs_bufWriteFloatLE;
Buffer.prototype.writeFloatBE = __wjs_bufWriteFloatBE;
Buffer.prototype.writeDoubleLE = __wjs_bufWriteDoubleLE;
Buffer.prototype.writeDoubleBE = __wjs_bufWriteDoubleBE;
Buffer.prototype.asciiWrite = function (string, offset, length) { return __wjs_bufAsciiWrite(this, string, offset, length); };
Buffer.prototype.base64Write = function (string, offset, length) { return __wjs_bufBase64Write(this, string, offset, length); };
Buffer.prototype.base64urlWrite = function (string, offset, length) { return __wjs_bufBase64urlWrite(this, string, offset, length); };
Buffer.prototype.latin1Write = function (string, offset, length) { return __wjs_bufLatin1Write(this, string, offset, length); };
Buffer.prototype.hexWrite = function (string, offset, length) { return __wjs_bufHexWrite(this, string, offset, length); };
Buffer.prototype.ucs2Write = function (string, offset, length) { return __wjs_bufUcs2Write(this, string, offset, length); };
Buffer.prototype.utf8Write = function (string, offset, length) { return __wjs_bufUtf8Write(this, string, offset, length); };
Buffer.prototype.asciiSlice = function (start, end) { return __wjs_bufAsciiSlice(this, start, end); };
Buffer.prototype.base64Slice = function (start, end) { return __wjs_bufB64Slice(this, start, end, false); };
Buffer.prototype.base64urlSlice = function (start, end) { return __wjs_bufB64Slice(this, start, end, true); };
Buffer.prototype.latin1Slice = function (start, end) { return __wjs_bufLatin1Slice(this, start, end); };
Buffer.prototype.hexSlice = function (start, end) { return __wjs_bufHexSlice(this, start, end); };
Buffer.prototype.ucs2Slice = function (start, end) { return __wjs_bufUcs2Slice(this, start, end); };
Buffer.prototype.utf8Slice = function (start, end) { return __wjs_bufUtf8Slice(this, start, end); };
globalThis.__wjs_bufApi = {
  get INSPECT_MAX_BYTES() { return INSPECT_MAX_BYTES; },
  set INSPECT_MAX_BYTES(v) {
    __wjs_bufValidateNumber(v, 'INSPECT_MAX_BYTES', 0);
    INSPECT_MAX_BYTES = v;
  },
  kMaxLength,
  kStringMaxLength,
  isUtf8(input) {
    if ((ArrayBuffer.isView(input) && !(input instanceof DataView)) || __wjs_bufIsAnyAB(input)) {
      const u8 = __wjs_bufAsU8(input) ?? new Uint8Array(0);
      try {
        new TextDecoder('utf-8', { fatal: true }).decode(u8);
        return true;
      } catch {
        return false;
      }
    }
    throw __wjs_bufArgTypeErr('input', ['ArrayBuffer', 'Buffer', 'TypedArray'], input);
  },
  isAscii(input) {
    if ((ArrayBuffer.isView(input) && !(input instanceof DataView)) || __wjs_bufIsAnyAB(input)) {
      const u8 = __wjs_bufAsU8(input) ?? new Uint8Array(0);
      for (let i = 0; i < u8.length; i++) {
        if (u8[i] > 0x7f) return false;
      }
      return true;
    }
    throw __wjs_bufArgTypeErr('input', ['ArrayBuffer', 'Buffer', 'TypedArray'], input);
  },
  btoa(input) {
    if (arguments.length === 0) throw __wjs_bufMissingArgsErr('input');
    return globalThis.btoa(`${input}`);
  },
  atob(input) {
    if (arguments.length === 0) throw __wjs_bufMissingArgsErr('input');
    return globalThis.atob(`${input}`);
  },
  transcode(source, fromEncoding, toEncoding) {
    if (!__wjs_bufIsU8(source)) {
      throw __wjs_bufArgTypeErr('source', ['Buffer', 'Uint8Array'], source);
    }
    if (source.length === 0) return new __wjs_bufFastBuffer();
    fromEncoding = __wjs_bufNormalizeEncoding(fromEncoding) || fromEncoding;
    toEncoding = __wjs_bufNormalizeEncoding(toEncoding) || toEncoding;
    const fromOps = __wjs_bufGetEncodingOps(fromEncoding);
    const toOps = __wjs_bufGetEncodingOps(toEncoding);
    if (fromOps === undefined || toOps === undefined) {
      const e = new RangeError(`Unable to transcode Buffer [U_UNKNOWN_ENCODING]`);
      e.code = 'ERR_UNKNOWN_ENCODING';
      e.errno = -1;
      throw e;
    }
    const decoded = fromOps.slice(source, 0, source.length);
    return __wjs_bufFromStringFast(decoded, toOps);
  },
};

// Uint8Array 构造失败文案桥（V8 "Invalid typed array length: N" 口径；
// SM 抛自有文案，套件按 V8 插值断言；newTarget 必须透传，否则 TypedArray
// 子类化（`class X extends Uint8Array`）全灭为基类原型——10f buffer 实测）。
(() => {
  const U8 = globalThis.Uint8Array;
  globalThis.Uint8Array = new Proxy(U8, {
    construct(target, args, newTarget) {
      try {
        return Reflect.construct(target, args, newTarget);
      } catch (e) {
        throw new RangeError(`Invalid typed array length: ${args[0]}`);
      }
    },
  });
})();

// String.prototype.repeat 的 RangeError 文案桥（V8 口径："Invalid string length"/
// "Invalid count value: N"；SM 文案不同，套件正则按 V8 断言）
(() => {
  const rep = String.prototype.repeat;
  Object.defineProperty(String.prototype, 'repeat', {
    value: function (count) {
      if (typeof count === 'number' && count < 0) {
        throw new RangeError(`Invalid count value: ${count}`);
      }
      try {
        return rep.call(this, count);
      } catch (e) {
        throw e instanceof RangeError ? new RangeError('Invalid string length') : e;
      }
    },
    writable: true,
    configurable: true,
    enumerable: false,
  });
})();
globalThis.__wjs_bufDecode = __wjs_bufDecode;
globalThis.__wjs_bufEncode = __wjs_bufEncode;
globalThis.Buffer = Buffer;
})();
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
// undici webidl 口径的值回显（MessageEvent 校验文案；真机逐形实测）：
// instanceOf 消息 = `"` + inspect(v, {quotes:'double'}) + `"`（"str" 形串自带
// 双引号故现 `""str""`；数字/容器仅外包一对）；not-iterable 用裸 inspect。
// 覆盖套件点名的形状（标量/空容器/数组/类实例），完整 inspect 面在 util。
const __wjs_insp = (v) => {
  if (v === null) return "null";
  if (v === undefined) return "undefined";
  const t = typeof v;
  if (t === "string") {
    const body = v.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
    return `"${body}"`;
  }
  if (t === "number" || t === "boolean" || t === "bigint") return String(v);
  if (t === "symbol") return v.toString();
  if (t === "function") return `[Function: ${v.name || "(anonymous)"}]`;
  if (Array.isArray(v)) return `[ ${v.map((x) => __wjs_insp(x)).join(", ")} ]`;
  const n = v.constructor && v.constructor.name && v.constructor.name !== "Object" ? v.constructor.name : null;
  const keys = Object.keys(v);
  const body = keys.length === 0 ? "" : ` ${keys.map((k) => `${k}: ${__wjs_insp(v[k])}`).join(", ")} `;
  return n ? `${n} {${body}}` : `{${body}}`;
};
const __wjs_inspQuoted = (v) => `"${__wjs_insp(v)}"`;
// Rust 侧取 symbol 描述（ToString 对 symbol 抛 TypeError；JS 侧 toString 合法）。
globalThis.__wjs_symToString = (v) => (typeof v === "symbol") ? v.toString() : null;
// 10f：全局 MessageEvent（node 26 主/worker 线程均全局；message-port/
// message-event 套件逐项对拍）。source/ports 须 MessagePort 实例——品牌经
// worker 模块求值期登记的 `__wjs_MessagePort` 隐藏槽判定（主线程无全局
// MessagePort；求值前无从有端口，非 null source 即 TypeError 正确）。
globalThis.MessageEvent = class MessageEvent extends Event {
  #data; #origin; #lastEventId; #source; #ports;
  constructor(type, init = {}) {
    if (arguments.length === 0) throw new TypeError("MessageEvent requires at least 1 argument, but only 0 were passed");
    super(type, init);
    const o = init ?? {};
    this.#data = o.data ?? null;
    this.#origin = String(o.origin ?? "");
    this.#lastEventId = String(o.lastEventId ?? "");
    const src = o.source ?? null;
    if (src !== null) {
      const M = globalThis.__wjs_MessagePort;
      if (!M || !(src instanceof M)) {
        throw new TypeError(`MessageEvent constructor: Expected eventInitDict.source (${__wjs_inspQuoted(src)}) to be an instance of MessagePort.`);
      }
    }
    this.#source = src;
    let ports = o.ports;
    if (ports !== undefined && ports !== null) {
      if (typeof ports[Symbol.iterator] !== "function") {
        throw new TypeError(`MessageEvent constructor: eventInitDict.ports (${__wjs_insp(ports)}) is not iterable.`);
      }
      const list = [...ports];
      for (let i = 0; i < list.length; i++) {
        const M2 = globalThis.__wjs_MessagePort;
        if (!M2 || !(list[i] instanceof M2)) {
          throw new TypeError(`MessageEvent constructor: Expected eventInitDict.ports[${i}] (${__wjs_inspQuoted(list[i])}) to be an instance of MessagePort.`);
        }
      }
      this.#ports = list;
    } else {
      this.#ports = [];
    }
    // 内部派发目标（`__wjsTarget` 不属 WebIDL 字典面，仅宿主 port 桥使用）。
    const tgt = o.__wjsTarget;
    if (tgt) __wjs_eventState.get(this).target = tgt;
  }
  get data() { return this.#data; }
  get origin() { return this.#origin; }
  get lastEventId() { return this.#lastEventId; }
  get source() { return this.#source; }
  get ports() { return this.#ports; }
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
// ---- QueuingStrategy 双类（Web 全局；WHATWG streams。真机 26 口径：highWaterMark
// 是原型 getter 非自有键、size 是可枚举 accessor 且全实例共享同一函数、构造器
// ARG_TYPE 文案 + highWaterMark 缺失 ERR_MISSING_OPTION；size 对 undefined/null
// 抛 TypeError、其余回 chunk.byteLength（原始值/普通对象 → undefined）；10f）----
const __wjs_qsState = new WeakMap();
function __wjs_qsArg(init) {
  if (init === null) return "null";
  if (typeof init === "string") return `type string ('${init}')`;
  if (typeof init === "number") return `type number (${init})`;
  if (typeof init === "function") return "type function";
  return `type ${typeof init}`;
}
const __wjs_qsSizeBL = (chunk) => {
  if (chunk === undefined || chunk === null) throw new TypeError("chunk must not be undefined or null");
  return chunk.byteLength;
};
const __wjs_qsSizeCount = () => 1;
globalThis.ByteLengthQueuingStrategy = class ByteLengthQueuingStrategy {
  constructor(init) {
    if (init === null || (typeof init !== "object" && typeof init !== "function")) {
      const err = new TypeError(`The "init" argument must be of type object. Received ${__wjs_qsArg(init)}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    if (init.highWaterMark === undefined) {
      const err = new TypeError("init.highWaterMark is required");
      err.code = "ERR_MISSING_OPTION";
      throw err;
    }
    __wjs_qsState.set(this, init.highWaterMark);
  }
  get highWaterMark() { return __wjs_qsState.get(this); }
  get size() { return __wjs_qsSizeBL; }
};
globalThis.CountQueuingStrategy = class CountQueuingStrategy {
  constructor(init) {
    if (init === null || (typeof init !== "object" && typeof init !== "function")) {
      const err = new TypeError(`The "init" argument must be of type object. Received ${__wjs_qsArg(init)}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    if (init.highWaterMark === undefined) {
      const err = new TypeError("init.highWaterMark is required");
      err.code = "ERR_MISSING_OPTION";
      throw err;
    }
    __wjs_qsState.set(this, init.highWaterMark);
  }
  get highWaterMark() { return __wjs_qsState.get(this); }
  get size() { return __wjs_qsSizeCount; }
};
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
// File（Web/Node 20+ 全局；jsdom/vitest 生态取此面）：Blob 子类 + name/lastModified。
// 状态复用 __wjs_blobBytes（WeakMap 随原型链命中，§4.23 纪律）。
globalThis.File = class File extends Blob {
  constructor(parts = [], name, options = {}) {
    if (arguments.length < 2 || name === undefined) {
      throw new TypeError("File constructor: name is required");
    }
    super(parts, options);
    this.name = String(name);
    this.lastModified =
      typeof options.lastModified === "number" ? options.lastModified : Date.now();
  }
  get [Symbol.toStringTag]() { return "File"; }
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
            ("__wjs_timer_ref", Some(timers::timer_ref), 2),
            ("__wjs_timer_refresh", Some(timers::timer_refresh), 1),
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
            // 10f crypto五轮：压缩/混合 SEC1 点导入（轮子内解压+上曲线校验）
            ("__wjs_ec_import_compressed", Some(crypto::ec_import_compressed), 2),
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
            // 10e：Ed448（ed448-goldilocks 特批钉版；RFC 8032 纯签名）
            ("__wjs_ed448_generate", Some(crypto::ed448_generate), 0),
            ("__wjs_ed448_public", Some(crypto::ed448_public), 1),
            ("__wjs_ed448_sign", Some(crypto::ed448_sign), 2),
            ("__wjs_ed448_verify", Some(crypto::ed448_verify), 3),
            ("__wjs_x_generate", Some(crypto::x_generate), 0),
            ("__wjs_x_public", Some(crypto::x_public), 1),
            ("__wjs_x_derive", Some(crypto::x_derive), 2),
            ("__wjs_x448_generate", Some(crypto::x448_generate), 0),
            ("__wjs_x448_public", Some(crypto::x448_public), 1),
            ("__wjs_x448_derive", Some(crypto::x448_derive), 2),
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
            // 10f os 对拍：machine/uname/priority
            ("__wjs_os_machine", Some(node::os::os_machine), 0),
            ("__wjs_os_uname", Some(node::os::os_uname), 0),
            ("__wjs_os_prio_get", Some(node::os::os_prio_get), 1),
            ("__wjs_os_prio_set", Some(node::os::os_prio_set), 2),
            ("__wjs_argv_json", Some(node::process_::argv_json), 0),
            ("__wjs_next_tick", Some(node::process_::next_tick_queue), 2),
            ("__wjs_process_getuid", Some(node::process_::getuid), 0),
            ("__wjs_process_getgid", Some(node::process_::getgid), 0),
            ("__wjs_process_geteuid", Some(node::process_::geteuid), 0),
            ("__wjs_process_getegid", Some(node::process_::getegid), 0),
            ("__wjs_process_getgroups", Some(node::process_::getgroups), 0),
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
            // 10f：process.umask（unix 真改，test/common 前置）
            ("__wjs_umask", Some(node::process_::umask), 1),
            ("__wjs_uptime", Some(node::process_::uptime), 0),
            ("__wjs_hrtime_ns", Some(node::process_::hrtime_ns), 0),
            ("__wjs_memory_usage", Some(node::process_::memory_usage), 0),
            ("__wjs_stdout_write", Some(node::process_::stdout_write), 1),
            ("__wjs_stderr_write", Some(node::process_::stderr_write), 1),
            ("__wjs_stdio_istty", Some(node::process_::stdio_istty), 1),
            // 10c-1: node:tty（winsize/setRawMode；unix-only 实现，其余平台回落）
            ("__wjs_tty_winsize", Some(node::tty::tty_winsize), 1),
            ("__wjs_tty_set_raw_mode", Some(node::tty::tty_set_raw_mode), 2),
            // Phase 4b: node:fs
            ("__wjs_fs_read_file", Some(node::fs::fs_read_file), 1),
            ("__wjs_fs_write_file", Some(node::fs::fs_write_file), 3),
            ("__wjs_fs_append_file", Some(node::fs::fs_append_file), 2),
            ("__wjs_fs_stat", Some(node::fs::fs_stat), 2),
            // M5 vitest 牵引：statfs（unix 经 nix statvfs，既有直引轮子）
            ("__wjs_fs_statfs", Some(node::fs::fs_statfs), 1),
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
            ("__wjs_fs_chown", Some(node::fs::fs_chown), 3),
            ("__wjs_fs_fchown", Some(node::fs::fs_fchown), 3),
            ("__wjs_fs_futimes", Some(node::fs::fs_futimes), 3),
            ("__wjs_fs_fsync", Some(node::fs::fs_fsync), 2),
            ("__wjs_watch_start", Some(node::fs::watch_start), 4),
            ("__wjs_watch_close", Some(node::fs::watch_close), 1),
            // Phase 4c: child_process
            // Phase 9d: node:net + node:dns
            ("__wjs_net_connect", Some(node::net::net_connect), 5),
            ("__wjs_net_bind", Some(node::net::net_bind), 3),
            ("__wjs_net_unhold", Some(node::net::net_unhold), 1),
            ("__wjs_net_fd", Some(node::net::net_fd), 1),
            ("__wjs_net_isip", Some(node::net::net_isip), 1),
            ("__wjs_net_listen", Some(node::net::net_listen), 3),
            ("__wjs_net_attach", Some(node::net::net_attach), 2),
            ("__wjs_net_write", Some(node::net::net_write), 2),
            ("__wjs_net_end", Some(node::net::net_end), 1),
            ("__wjs_net_destroy", Some(node::net::net_destroy), 1),
            // 10a：ref 真计数（net/dgram 共用）
            ("__wjs_net_ref", Some(node::net::net_ref), 1),
            ("__wjs_net_unref", Some(node::net::net_unref), 1),
            ("__wjs_dns_lookup", Some(node::dns::dns_lookup), 1),
            // 10d：dns 深件（hickory 全套；lookup 维持 std）
            ("__wjs_dns_query", Some(node::dns::dns_query), 2),
            // 10f：Resolver 定制查询（投递/轮询/遗忘，见 dns.rs job 表）
            ("__wjs_dns_job_start", Some(node::dns::dns_job_start), 6),
            ("__wjs_dns_job_poll", Some(node::dns::dns_job_poll), 1),
            ("__wjs_dns_job_forget", Some(node::dns::dns_job_forget), 1),
            ("__wjs_dns_servers_get", Some(node::dns::dns_servers_get), 0),
            ("__wjs_dns_servers_set", Some(node::dns::dns_servers_set), 1),
            ("__wjs_dns_order_get", Some(node::dns::dns_order_get), 0),
            ("__wjs_dns_order_set", Some(node::dns::dns_order_set), 1),
            // Phase 9d-6: node:tls（握手底座；读写复用 net_* natives）
            ("__wjs_tls_connect", Some(node::tls::tls_connect), 4),
            ("__wjs_tls_listen", Some(node::tls::tls_listen), 4),
            // Phase 9d-7: node:http2（hyper 直引；关闭复用 __wjs_net_destroy）
            // 10f 流式化：头/体/收尾/RST 分离（ChanBody 增量应答）
            ("__wjs_h2_listen", Some(node::http2::h2_listen), 4),
            ("__wjs_h2_connect", Some(node::http2::h2_connect), 4),
            ("__wjs_h2_open", Some(node::http2::h2_open), 4),
            ("__wjs_h2_open_trailers", Some(node::http2::h2_open_trailers), 3),
            ("__wjs_h2_respond", Some(node::http2::h2_respond), 4),
            ("__wjs_h2_data", Some(node::http2::h2_data), 3),
            ("__wjs_h2_end", Some(node::http2::h2_end), 3),
            ("__wjs_h2_reset", Some(node::http2::h2_reset), 3),
            // Phase 9e-1a: node:crypto 增量 Hash（oneshot 复用全局 __wjs_*）
            ("__wjs_crypto_hash_new", Some(node::crypto::crypto_hash_new), 1),
            ("__wjs_crypto_hash_update", Some(node::crypto::crypto_hash_update), 2),
            ("__wjs_crypto_hash_digest", Some(node::crypto::crypto_hash_digest), 1),
            ("__wjs_crypto_hash_copy", Some(node::crypto::crypto_hash_copy), 1),
            ("__wjs_crypto_hash_set_len", Some(node::crypto::crypto_hash_set_len), 2),
            // Phase 9e-1b: node:crypto 对称密码（CBC/CTR 流式 + ChaCha oneshot）
            ("__wjs_cipher_new", Some(node::crypto::cipher_new), 5),
            ("__wjs_cipher_update", Some(node::crypto::cipher_update), 2),
            ("__wjs_cipher_final", Some(node::crypto::cipher_final), 1),
            ("__wjs_cipher_chacha", Some(node::crypto::cipher_chacha), 6),
            // 10e: AES-CCM oneshot（ccm 0.6 直引）
            ("__wjs_ccm_crypt", Some(node::crypto::ccm_crypt_native), 7),
            // 10e: GCM 任意 iv（12B 走 crate，其余 J0 手工；WebCrypto 共用面不动）
            ("__wjs_gcm_anyiv", Some(node::crypto::gcm_anyiv), 5),
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
            ("__wjs_node_rsa_oaep_flip", Some(node::crypto::node_rsa_oaep_flip), 4),
            ("__wjs_rsa_v15_flip", Some(node::crypto::rsa_v15_flip), 3),
            ("__wjs_rsa_raw", Some(node::crypto::rsa_raw), 3),
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
            ("__wjs_vm_global", Some(node::vm::vm_global), 1),
            ("__wjs_vm_run", Some(node::vm::vm_run), 3),
            ("__wjs_vm_run_this", Some(node::vm::vm_run_this), 2),
            ("__wjs_vm_compile_fn", Some(node::vm::vm_compile_fn), 4),
            ("__wjs_vm_set", Some(node::vm::vm_set), 3),
            ("__wjs_vm_get", Some(node::vm::vm_get), 2),
            ("__wjs_vm_keys", Some(node::vm::vm_keys), 1),
            ("__wjs_vm_keys_all", Some(node::vm::vm_keys_all), 1),
            ("__wjs_vm_keys_count", Some(node::vm::vm_keys_count), 1),
            ("__wjs_vm_same", Some(node::vm::vm_same), 2),
            ("__wjs_vm_release", Some(node::vm::vm_release), 1),
            ("__wjs_vm_take_error", Some(node::vm::vm_take_error), 0),
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
            ("__wjs_port_try_recv", Some(node::worker::port_try_recv), 1),
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
            ("__wjs_worker_name", Some(node::worker::worker_name), 0),
            ("__wjs_worker_is_fork", Some(node::worker::worker_is_fork), 0),
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
            // 10a：组播/广播/TTL/connect（JSON 单 native；id+op 包）
            ("__wjs_dgram_sockopt", Some(node::dgram::dgram_sockopt), 2),
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
            // 10a：crc32（ISO-HDLC 自实现；flate2::Crc 不收 seed）
            ("__wjs_zlib_crc32", Some(node::zlib::zlib_crc32), 2),
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
            // 10d：node:sqlite（turso 底座；DatabaseSync/StatementSync）
            ("__wjs_nsqlite_open", Some(node::sqlite::nsqlite_open), 1),
            ("__wjs_nsqlite_exec", Some(node::sqlite::nsqlite_exec), 2),
            ("__wjs_nsqlite_run", Some(node::sqlite::nsqlite_run), 4),
            ("__wjs_nsqlite_rows", Some(node::sqlite::nsqlite_rows), 4),
            ("__wjs_nsqlite_cols", Some(node::sqlite::nsqlite_cols), 2),
            ("__wjs_nsqlite_close", Some(node::sqlite::nsqlite_close), 1),
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
