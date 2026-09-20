//! prelude part 00 (byte-exact slice; order matters, see prelude/mod.rs).
pub const PART_00: &str = r#"
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
"#;
