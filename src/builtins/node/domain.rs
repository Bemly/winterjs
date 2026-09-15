//! `node:domain`：遗留薄面（10e，Bun 🟡对等）。
//!
//! 纯 JS（零 native）：create/run/bind/intercept + add/remove/enter/exit/active
//! + 同步错误路由（`run` 内同步抛错 → domain `error` 事件；无监听则重抛）。
//! 偏差记档：异步回调（timer/IO）抛错不路由——真机靠 async_hooks 隐式上下文，
//! 本仓 `async_hooks` 为 stub 口径（Bun 同款），无处挂载；`bind` 包裹后被
//! 同步调用时路由正常，跨事件循环调用不路由。`dispose` 不存在（真机同款无此方法）。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
import { EventEmitter } from "node:events";

let __stack = [];
// 真机具名导出 `active`（活绑定，进出同步刷新）。
export let active = null;
function __refreshActive() {
  active = __stack.length === 0 ? null : __stack[__stack.length - 1];
}

export class Domain extends EventEmitter {
  constructor() {
    super();
    this.members = [];
  }
  _emitError(err) {
    // node _errorHandler 口径（lib/domain.js）：错误挂 domain/domainThrown 标记；
    // 先弹掉栈顶相邻的自身（处理器运行在 active=栈上更高一位或 undefined 下）；
    // 处理完清栈归 null（domainUncaughtExceptionClear：两轮事件循环间不留活域，
    // 套件 reset-process-domain-on-throw 分别断言 undefined 与 null 两态）。
    try { err.domain = this; err.domainThrown = true; } catch {}
    while (__stack[__stack.length - 1] === this) __stack.pop();
    active = __stack.length === 0 ? undefined : __stack[__stack.length - 1];
    let caught = false;
    if (this.listenerCount("error") > 0) {
      caught = this.emit("error", err);
    } else {
      throw err;
    }
    __stack.length = 0;
    active = null;
    return caught;
  }
  run(fn, ...args) {
    if (typeof fn !== "function") {
      const err = new TypeError("domain.run: callback must be a function");
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    this.enter();
    try {
      const r = fn(...args);
      this.exit();
      return r;
    } catch (e) {
      this.exit();
      this._emitError(e);
      return undefined;
    }
  }
  bind(fn) {
    if (typeof fn !== "function") {
      const err = new TypeError("domain.bind: callback must be a function");
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    const self = this;
    return function (...args) {
      self.enter();
      try {
        const r = fn(...args);
        self.exit();
        return r;
      } catch (e) {
        self.exit();
        self._emitError(e);
        return undefined;
      }
    };
  }
  intercept(fn) {
    if (typeof fn !== "function") {
      const err = new TypeError("domain.intercept: callback must be a function");
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    const self = this;
    return function (err, ...args) {
      if (err !== null && err !== undefined) {
        self._emitError(err);
        return undefined;
      }
      self.enter();
      try {
        const r = fn(...args);
        self.exit();
        return r;
      } catch (e) {
        self.exit();
        self._emitError(e);
        return undefined;
      }
    };
  }
  add(emitter) {
    if (emitter === null || (typeof emitter !== "object" && typeof emitter !== "function")) {
      const err = new TypeError("domain.add: emitter must be an object");
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    if (typeof emitter.on !== "function" || typeof emitter.removeListener !== "function") {
      const err = new TypeError("domain.add: emitter must be an EventEmitter");
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    if (!this.members.includes(emitter)) {
      this.members.push(emitter);
      let handlers = __addedHandlers.get(emitter);
      if (handlers === undefined) {
        handlers = [];
        __addedHandlers.set(emitter, handlers);
      }
      const self = this;
      const onError = function (err) { self._emitError(err); };
      handlers.push([this, onError]);
      emitter.on("error", onError);
    }
    return emitter;
  }
  remove(emitter) {
    const i = this.members.indexOf(emitter);
    if (i !== -1) this.members.splice(i, 1);
    const handlers = __addedHandlers.get(emitter);
    if (handlers !== undefined) {
      for (let j = handlers.length - 1; j >= 0; j--) {
        if (handlers[j][0] === this) {
          try { emitter.removeListener("error", handlers[j][1]); } catch {}
          handlers.splice(j, 1);
        }
      }
      if (handlers.length === 0) __addedHandlers.delete(emitter);
    }
    return emitter;
  }
  enter() {
    __stack.push(this);
    __refreshActive();
  }
  exit() {
    const i = __stack.lastIndexOf(this);
    if (i !== -1) __stack.splice(i, 1);
    __refreshActive();
  }
}

const __addedHandlers = new WeakMap();

export function create() {
  return new Domain();
}
export function createDomain() {
  return new Domain();
}
export function getActive() {
  return active;
}
// node lib/domain.js 原文口径：本模块载入即接管 process.domain（读/写活绑定）。
// 初始 null（模块载入前 undefined）；exit 到空栈为 undefined——两个"无域"态
// 有区别，套件 reset-process-domain-on-throw 分别断言。
Object.defineProperty(globalThis.process, "domain", {
  enumerable: true,
  configurable: true,
  get() { return active; },
  set(v) { active = v; },
});
// 定时器面（prelude timers）的域捕获点：登记期取活域，回调抛错先路由域。
globalThis.__wjs_domain_capture = () => active;
const __api = {
  Domain, create, createDomain,
  get active() { return active; },
};
export default __api;
"#;
