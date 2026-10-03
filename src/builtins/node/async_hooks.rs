//! `node:async_hooks`（plan2 §9a；`lib/async_hooks.js` 公开面 + ALS/AsyncResource 实做）。
//!
//! 口径（plan2 拍板：ALS/AsyncResource 实，其余 stub，Bun 同款）：
//! - `AsyncResource`：真类——asyncId 单调计数、triggerAsyncId、runInAsyncScope
//!   （作用域内 ALS 上下文可见）、bind、emitDestroy。
//! - `AsyncLocalStorage`：真类——getStore/run/enterWith/enter/exit/bind/snapshot/disable。
//! - 上下文模型：模块级 `Map<ALS, store>` 快照；AsyncResource 构造期拍照，
//!   runInAsyncScope 期间以拷贝生效（不改写父作用域，与 Node 传播语义同向）。
//! - 偏差（记档）：**跨 await/microtask 传播不支持**——事件循环 job 边界无
//!   JS 可挂的上下文钩子（引擎无 async_hooks 原语），run() 内同步链路完整；
//!   `createHook` 只验签名、钩子永不触发；`executionAsyncId/Resource`、
//!   `triggerAsyncId` 返回 runInAsyncScope 栈顶（顶层 1/根资源），非引擎真值。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Node lib/async_hooks.js surface; ALS/AsyncResource implemented (stub scope per plan2 9a).
// Deviations documented in module docs (no cross-await propagation; createHook inert).
import errors from 'node:internal/errors';
import { kEmptyObject } from 'node:internal/util';
import { validateBoolean, validateFunction, validateNumber, validateString } from 'node:internal/validators';
const {
  codes: { ERR_ASYNC_CALLBACK },
} = errors;

const async_id_symbol = Symbol('async_id_symbol');
const trigger_async_id_symbol = Symbol('trigger_async_id_symbol');

// ── 上下文模型 ────────────────────────────────────────────────────────────
let currentContext = new Map();          // Map<AsyncLocalStorage, store>
let nextAsyncId = 2;                     // 0 = invalid, 1 = 根
const ROOT_RESOURCE = { __proto__: null };
const idStack = [1];
const resourceStack = [ROOT_RESOURCE];

function newAsyncId() { return nextAsyncId++; }
// node 口径 symbols 表（immediate-error 套件 `symbols.async_id_symbol`）。
const symbols = {
  async_id_symbol,
  trigger_async_id_symbol,
};
function executionAsyncId() { return idStack[idStack.length - 1]; }
function executionAsyncResource() { return resourceStack[resourceStack.length - 1]; }
function triggerAsyncId() { return executionAsyncId(); }
function getDefaultTriggerAsyncId() { return idStack[idStack.length - 1]; }

// ── createHook（半 stub：签名校验 + enable 计数 + init 触发；before/after/
// destroy/promiseResolve 永不触发，跨 await 传播不支持，见头注）────────────
// R-stream：enable 计数供 `enabledHooksExist`（eos 三套件点名）；AsyncResource
// 构造期同步触发已 enable 钩子的 init（STREAM_END_OF_STREAM 上下文传播点名）。
const __enabledHooks = new Set();
class AsyncHook {
  constructor({ init, before, after, destroy, promiseResolve, trackPromises } = kEmptyObject) {
    if (init !== undefined && typeof init !== 'function') throw new ERR_ASYNC_CALLBACK('hook.init');
    if (before !== undefined && typeof before !== 'function') throw new ERR_ASYNC_CALLBACK('hook.before');
    if (after !== undefined && typeof after !== 'function') throw new ERR_ASYNC_CALLBACK('hook.after');
    if (destroy !== undefined && typeof destroy !== 'function') throw new ERR_ASYNC_CALLBACK('hook.destroy');
    if (promiseResolve !== undefined && typeof promiseResolve !== 'function') {
      throw new ERR_ASYNC_CALLBACK('hook.promiseResolve');
    }
    if (trackPromises !== undefined) validateBoolean(trackPromises, 'trackPromises');
    this.__wjs2_fns = { init, before, after, destroy, promiseResolve };
    this.__wjs2_on = false;
  }
  enable() {
    if (!this.__wjs2_on) { this.__wjs2_on = true; __enabledHooks.add(this); }
    return this;
  }
  disable() {
    if (this.__wjs2_on) { this.__wjs2_on = false; __enabledHooks.delete(this); }
    return this;
  }
}

function createHook(fns) {
  return new AsyncHook(fns);
}

// ── AsyncResource ─────────────────────────────────────────────────────────
class AsyncResource {
  constructor(type, options = undefined) {
    validateString(type, 'type');
    let triggerAsyncId_;
    let requireManualDestroy;
    if (options === undefined) {
      triggerAsyncId_ = getDefaultTriggerAsyncId();
      requireManualDestroy = false;
    } else if (typeof options !== 'object' || options === null) {
      throw new TypeError('options must be an object');
    } else {
      triggerAsyncId_ = options.triggerAsyncId ?? getDefaultTriggerAsyncId();
      requireManualDestroy = Boolean(options.requireManualDestroy);
    }
    validateNumber(triggerAsyncId_, 'options.triggerAsyncId');
    this[async_id_symbol] = newAsyncId();
    this[trigger_async_id_symbol] = triggerAsyncId_;
    this.__wjs2ManualDestroy = requireManualDestroy;
    this.__wjs2Destroyed = false;
    // 构造期 ALS 快照（跨作用域传播的唯一通道；跨 await 不支持，见头注）
    this.__wjs2Context = new Map(currentContext);
    // R-stream：同步触发已 enable 钩子的 init（node 口径 init(asyncId, type,
    // triggerAsyncId, resource)；用户回调走 Reflect.apply，抛错吞掉不中断构造）。
    for (const h of __enabledHooks) {
      const fn = h.__wjs2_fns && h.__wjs2_fns.init;
      if (typeof fn === 'function') {
        try { Reflect.apply(fn, h, [this[async_id_symbol], type, triggerAsyncId_, this]); } catch {}
      }
    }
  }

  asyncId() { return this[async_id_symbol]; }
  triggerAsyncId() { return this[trigger_async_id_symbol]; }

  runInAsyncScope(fn, thisArg = undefined, ...args) {
    validateFunction(fn, 'fn');
    idStack.push(this[async_id_symbol]);
    resourceStack.push(this);
    const prevContext = currentContext;
    currentContext = new Map(this.__wjs2Context);
    try {
      const ret = fn.apply(thisArg, args);
      return ret;
    } finally {
      currentContext = prevContext;
      resourceStack.pop();
      idStack.pop();
      if (!this.__wjs2ManualDestroy && !this.__wjs2Destroyed) {
        this.__wjs2Destroyed = true;
      }
    }
  }

  bind(fn, thisArg = this) {
    validateFunction(fn, 'fn');
    const resource = this;
    return function(...args) {
      return resource.runInAsyncScope(fn, thisArg, ...args);
    };
  }

  emitDestroy() {
    this.__wjs2Destroyed = true;
    return this;
  }

  static bind(fn, type, thisArg) {
    validateFunction(fn, 'fn');
    const resource = new AsyncResource(type ?? fn.name ?? 'anonymous');
    return resource.bind(fn, thisArg);
  }

  static emitDestroy(type) {
    validateString(type, 'type');
    new AsyncResource(type).emitDestroy();
  }
}

// ── AsyncLocalStorage ─────────────────────────────────────────────────────
class AsyncLocalStorage {
  #exitStack = [];

  disable() {
    this.#exitStack.length = 0;
  }

  getStore() {
    return currentContext.get(this);
  }

  run(store, callback, ...args) {
    validateFunction(callback, 'callback');
    const prevContext = currentContext;
    currentContext = new Map(prevContext);
    if (store !== undefined) currentContext.set(this, store);
    else currentContext.delete(this);
    try {
      return callback(...args);
    } finally {
      currentContext = prevContext;
    }
  }

  enter(store) {
    this.#exitStack.push(currentContext.get(this));
    if (store !== undefined) currentContext.set(this, store);
    else currentContext.delete(this);
  }

  // 10f：`enterWith` 为文档化主入口（`enter` 系遗留别名），语义同 enter——
  // 进入 store 直至被 run/exit 切换（run-stores-scope 套件门）。
  enterWith(store) {
    this.#exitStack.push(currentContext.get(this));
    if (store !== undefined) currentContext.set(this, store);
    else currentContext.delete(this);
  }

  exit(callback, ...args) {
    validateFunction(callback, 'callback');
    const prevContext = currentContext;
    currentContext = new Map(prevContext);
    currentContext.delete(this);
    try {
      return callback(...args);
    } finally {
      currentContext = prevContext;
      this.#exitStack.pop();
    }
  }

  bind(fn) {
    validateFunction(fn, 'fn');
    const als = this;
    return function(...args) {
      return als.run(als.getStore(), fn, ...args);
    };
  }

  snapshot() {
    // 拍照时点即捕获 store（Node 语义：snapshot 后续变更不影响）
    const als = this;
    const store = als.getStore();
    return function(cb, ...args) {
      validateFunction(cb, 'cb');
      return als.run(store, cb, ...args);
    };
  }

  // Node 私有面（diagnostics_channel RunStoresScope 用）：
  // 作用域内设置 store，dispose 时恢复原值。
  __wjs2WithScope(store) {
    const SymbolDispose = Symbol.dispose ?? Symbol.for('Symbol.dispose');
    const als = this;
    const prev = currentContext.get(als);
    if (store !== undefined) currentContext.set(als, store);
    else currentContext.delete(als);
    return {
      [SymbolDispose]() {
        if (prev !== undefined) currentContext.set(als, prev);
        else currentContext.delete(als);
      },
    };
  }

  static bind(fn, type, thisArg) {
    return AsyncResource.bind(fn, type, thisArg);
  }

  static snapshot() {
    const als = new AsyncLocalStorage();
    return als.snapshot();
  }
}
AsyncLocalStorage.AsyncLocalStorage = AsyncLocalStorage;
AsyncResource.AsyncResource = AsyncResource;

const asyncWrapProviders = Object.freeze({ __proto__: null });

// 10f timers 对拍：跨事件循环的异步上下文快照挂载点——定时器注册时 capture、
// 触发时 restore（套件 clearImmediate-als）。快照拷贝 Map 外层（ALS→store 映射），
// store 对象同一性保留（node 口径）；无活跃上下文时 capture 回 undefined，
// 挂载侧（prelude timers）据此走零开销直调。
globalThis.__wjs2_als_capture = () =>
  currentContext.size === 0 ? undefined : new Map(currentContext);
globalThis.__wjs2_als_restore = (snap, fn) => {
  const prev = currentContext;
  currentContext = snap;
  try { return fn(); } finally { currentContext = prev; }
};

// R-stream：eos 分支 + 套件直调。`internal/async_hooks` 经门面与本实例同源
// （见 `INTERNAL_ASYNC_HOOKS_FACADE_SOURCE`），故此处单实例状态即全局真相：
// hook 开集合非空或 ALS 上下文非空即真（default-path 无钩无 ALS 即假）。
// 注意：ALS 非空即真属近似（真机只看 hooks；本仓无 AsyncContextFrame 引擎
// 原语，ALS 测试靠此分支，记档）。
function enabledHooksExist() {
  return __enabledHooks.size > 0 || currentContext.size > 0;
}

export {
  AsyncLocalStorage,
  AsyncResource,
  createHook,
  enabledHooksExist,
  executionAsyncId,
  executionAsyncResource,
  triggerAsyncId,
  asyncWrapProviders,
  newAsyncId,
  symbols,
};
export default { AsyncLocalStorage, AsyncResource, createHook, enabledHooksExist, executionAsyncId, executionAsyncResource, triggerAsyncId, asyncWrapProviders, newAsyncId, symbols };
"#;

/// `node:internal/async_hooks` 门面（与 `node:async_hooks` 同实例状态）。
/// 背景：两 canonical 各自求值即两份模块级状态（ALS Map/enable 集）分叉——eos
/// 内部分支与套件直调读到空状态即假。门面只做重导出，状态锚定公开实例。
pub const INTERNAL_ASYNC_HOOKS_FACADE_SOURCE: &str = r#"
import { enabledHooksExist } from 'node:async_hooks';
export { enabledHooksExist };
export default { enabledHooksExist };
"#;
