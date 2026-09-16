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
function executionAsyncId() { return idStack[idStack.length - 1]; }
function executionAsyncResource() { return resourceStack[resourceStack.length - 1]; }
function triggerAsyncId() { return executionAsyncId(); }
function getDefaultTriggerAsyncId() { return idStack[idStack.length - 1]; }

// ── createHook（stub：验签名，钩子永不触发）─────────────────────────────
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
  }
  enable() { return this; }
  disable() { return this; }
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
    this.__wjsManualDestroy = requireManualDestroy;
    this.__wjsDestroyed = false;
    // 构造期 ALS 快照（跨作用域传播的唯一通道；跨 await 不支持，见头注）
    this.__wjsContext = new Map(currentContext);
  }

  asyncId() { return this[async_id_symbol]; }
  triggerAsyncId() { return this[trigger_async_id_symbol]; }

  runInAsyncScope(fn, thisArg = undefined, ...args) {
    validateFunction(fn, 'fn');
    idStack.push(this[async_id_symbol]);
    resourceStack.push(this);
    const prevContext = currentContext;
    currentContext = new Map(this.__wjsContext);
    try {
      const ret = fn.apply(thisArg, args);
      return ret;
    } finally {
      currentContext = prevContext;
      resourceStack.pop();
      idStack.pop();
      if (!this.__wjsManualDestroy && !this.__wjsDestroyed) {
        this.__wjsDestroyed = true;
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
    this.__wjsDestroyed = true;
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
  __wjsWithScope(store) {
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
globalThis.__wjs_als_capture = () =>
  currentContext.size === 0 ? undefined : new Map(currentContext);
globalThis.__wjs_als_restore = (snap, fn) => {
  const prev = currentContext;
  currentContext = snap;
  try { return fn(); } finally { currentContext = prev; }
};

export {
  AsyncLocalStorage,
  AsyncResource,
  createHook,
  executionAsyncId,
  executionAsyncResource,
  triggerAsyncId,
  asyncWrapProviders,
};
export default { AsyncLocalStorage, AsyncResource, createHook, executionAsyncId, executionAsyncResource, triggerAsyncId, asyncWrapProviders };
"#;
