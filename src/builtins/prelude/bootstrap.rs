//! 全局自举（global/performance/queueMicrotask/timers/模块钩子/napi  helper/DOMException）（prelude 分域；拼接顺序见 mod.rs）。
pub const BOOTSTRAP_JS: &str = r#"
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
  globalThis.__wjs2_timer_after = (ms) => {
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
  globalThis.__wjs2_timer_validate_cb = (cb) => {
    if (typeof cb !== "function") {
      const e = new TypeError(`The "callback" argument must be of type function. Received type ${typeof cb}`);
      e.code = "ERR_INVALID_ARG_TYPE";
      throw e;
    }
  };
  class Timeout {
    constructor(id) {
      this.__wjs2_id = id;
      this._destroyed = false;
      this._idleTimeout = 1;
      this._idleStart = 0;
      this._onTimeout = null;
      this._timerArgs = undefined;
      this._repeat = null;
      this.__wjs2_unrefed = false;
    }
    unref() { this.__wjs2_unrefed = true; __wjs2_timer_ref(this.__wjs2_id, false); return this; }
    ref() { this.__wjs2_unrefed = false; __wjs2_timer_ref(this.__wjs2_id, true); return this; }
    hasRef() { return !this.__wjs2_unrefed; }
    refresh() { __wjs2_timer_refresh(this.__wjs2_id); this._destroyed = false; return this; }
    close() { __clear(this); return this; }
    [Symbol.toPrimitive]() { return this.__wjs2_id; }
    [Symbol.dispose]() { __clear(this); }
  }
  class Immediate extends Timeout {}
  // ALS 快照挂载点：async_hooks 模块载入时安装 capture/restore；未载入则
  // 定时器回调无异步上下文（缺省口径）。
  const __alsRun = (snap, fn) =>
    snap !== undefined && globalThis.__wjs2_als_restore ? __wjs2_als_restore(snap, fn) : fn();
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
    const snap = globalThis.__wjs2_als_capture?.();
    const dom = globalThis.__wjs2_domain_capture?.();
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
    const id = interval ? __wjs2_setInterval(step, delay, []) : __wjs2_setTimeout(step, delay, []);
    self.__wjs2_id = id;
  };
  globalThis.setTimeout = function (cb, ms, ...rest) {
    __wjs2_timer_validate_cb(cb);
    const after = ms === undefined ? 1 : __wjs2_timer_after(ms);
    const self = new Timeout(0);
    self._idleTimeout = after;
    self._onTimeout = cb;
    self._timerArgs = rest;
    self._repeat = null;
    __arm(self, after, false);
    return self;
  };
  globalThis.setInterval = function (cb, ms, ...rest) {
    __wjs2_timer_validate_cb(cb);
    const after = ms === undefined ? 1 : __wjs2_timer_after(ms);
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
    if (id !== null && id !== undefined && typeof id === "object" && "__wjs2_id" in id) {
      id._destroyed = true;
    }
    __wjs2_clearTimeout(__wjs2_timer_id(id));
  };
  globalThis.clearTimeout = __clear;
  globalThis.clearInterval = __clear;
  globalThis.clearImmediate = __clear;
  // 10a：全局 setImmediate/clearImmediate（本仓无 macrotask 分层，setTimeout(0)
  // 近似——与 node:timers 同口径，check 阶段语义记档；clearImmediate 复用同表）。
  globalThis.setImmediate = function (cb, ...rest) {
    __wjs2_timer_validate_cb(cb);
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
globalThis.__wjs2_timer_id = (id) => {
  if (id === null || id === undefined) return 0;
  if (typeof id === "object") return id.__wjs2_id ?? 0;
  if (typeof id === "number") return Number.isFinite(id) && id >= 0 ? id : 0;
  if (typeof id === "string") { const n = Number(id); return Number.isFinite(n) && n >= 0 ? n : 0; }
  return 0;
};
// 未捕获异常分发（timer 等异步回调抛错时由 native 调）。
// __wjs2_uncaught_count 先探监听器数——为 0 时 native 保持 pending 原样走
// fatal 上报（错误信息/栈不经中转，不降级）；>0 时 native 取走异常经
// __wjs2_uncaught 逐个调用（Node 口径第二参 origin='uncaughtException'）。
globalThis.__wjs2_uncaught_count = () => {
  const p = globalThis.process;
  const ls = p && p.__wjs2_listeners ? p.__wjs2_listeners["uncaughtException"] : undefined;
  return ls ? ls.length : 0;
};
globalThis.__wjs2_uncaught = (err) => {
  const p = globalThis.process;
  // P2-process R7：capture 回调优先（setUncaughtExceptionCaptureCallback 面）——
  // 接住即吞（uncaughtException 监听不发、fatal 不走）；抛错冒泡由调用方按 fatal 收。
  const cap = p ? p.__wjs2_captureCb : undefined;
  if (typeof cap === "function") {
    cap(err);
    return true;
  }
  if (p && typeof p.__wjs2_emit === "function") {
    return p.__wjs2_emit("uncaughtException", err, "uncaughtException") > 0;
  }
  return false;
};
// 事件循环触发定时器 / structuredClone 枚举属性用的内部辅助
globalThis.__wjs2_call = (cb, args) => cb(...args);
// napi_call_function：recv 语义的参数展开（Function.prototype.apply）
globalThis.__wjs2_napi_call = (recv, fn, args) => fn.apply(recv, args);
// ESM 定制钩子注册表（module.registerHooks 写、import.meta.resolve 消费）。
// Node 口径：后注册者先跑（每个新钩子包住既有链，next = 链上已见部分）；
// 默认底座 = 本仓解析器（parentURL 显式 base 的 __wjs2_require_resolve_from）。
globalThis.__wjs2_module_hooks = [];
globalThis.__wjs2_module_resolve_chain = function (specifier, parentURL) {
  let chain = (spec, ctx) => ({ url: __wjs2_require_resolve_from(ctx.parentURL, spec) });
  for (const h of globalThis.__wjs2_module_hooks) {
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
globalThis.__wjs2_make_meta_resolve = function (url) {
  return function resolve(specifier) {
    return __wjs2_module_resolve_chain(specifier, url);
  };
};
// napi_new_instance：`new ctor(...args)` 全语义（new.target/prototype/异常传播）
globalThis.__wjs2_napi_new = (ctor, args) => new ctor(...args);
// napi_set_* 的非严格赋值面：JSAPI JS_SetProperty 是 strict 语义（对只读/
// 冻结属性抛 TypeError），Node 的 napi_set_property 走 v8 非严格 set（静默
// 无操作返回 ok）。sloppy 函数内的 `obj[key] = value` 与后者精确对齐。
globalThis.__wjs2_napi_set = (obj, key, value) => { obj[key] = value; };
// napi Buffer 形状：Uint8Array + Buffer.prototype（Node 实例同款）；
// is_buffer 判定（instanceof Buffer；Buffer 缺席恒 false）
globalThis.__wjs2_napi_bufferify = (u8) => {
  if (typeof Buffer !== "function") throw new TypeError("Buffer is not available");
  Object.setPrototypeOf(u8, Buffer.prototype);
  return u8;
};
globalThis.__wjs2_napi_is_buffer =
  (v) => typeof Buffer === "function" && v instanceof Buffer;
// napi_define_class/define_properties 的访问器定义（setter 传 undefined =
// Node getter-only 语义：sloppy 赋值静默、strict TypeError）
globalThis.__wjs2_napi_accessor =
  (obj, name, getter, setter, enumerable, configurable) =>
    Object.defineProperty(obj, name, { get: getter, set: setter, enumerable, configurable });
globalThis.__wjs2_entries = (v) => Object.entries(v);
"#;
