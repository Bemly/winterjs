//! `node:diagnostics_channel`（Node `lib/diagnostics_channel.js` 语义移植，MIT）。
//!
//! 忠实面：channel/subscribe/unsubscribe/hasSubscribers（惰性激活原型切换）、
//! ActiveChannel.publish/bindStore/unbindStore/runStores（store transform）、
//! TracingChannel（start/end/asyncStart/asyncEnd/error 五通道 +
//! traceSync/tracePromise/traceCallback）、BoundedChannel、非 thenable 警告。
//!
//! 偏差（9a 口径）：
//! - `using`/DisposableStack 依赖改手动 try/finally + `[SymbolDispose]()` 直调
//!   （语义等价，避免转译层差异）；WeakRefMap/FinalizationRegistry → 普通 Map
//!   （具名通道有限集，无 GC 清理需求，记档）。
//! - `triggerUncaughtException` → `__dcUncaught`（nextTick 内探
//!   `uncaughtException` 监听：有则 emit，无则 throw 走本仓未捕获路径；
//!   裸 throw 经本仓 nextTick（microtask）会变 unhandled rejection，
//!   与真机 uncaughtException 对不上——10f 对拍修）。
//! - native 通道链接面（dc_binding.linkNativeChannel/notifyChannelActive）无——
//!   无 native dc 底座，`_index` 恒 undefined（分支保留）。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node lib/diagnostics_channel.js (see module docs for deviations).
import errors from 'node:internal/errors';
import { validateFunction } from 'node:internal/validators';
import types from 'node:internal/util/types';

const {
  codes: {
    ERR_INVALID_ARG_TYPE,
  },
} = errors;

const SymbolDispose = Symbol.dispose ?? Symbol.for('Symbol.dispose');

// 10f：triggerUncaughtException 转译（真机：有监听即 emit，无则 fatal；
// 本仓 nextTick 系 microtask，裸 throw 会变 unhandled rejection，故先探监听）
function __dcUncaught(err) {
  try {
    if (typeof process?.listenerCount === 'function' &&
        process.listenerCount('uncaughtException') > 0) {
      process.emit('uncaughtException', err);
      return;
    }
  } catch { /* 探监听失败即落空到 throw */ }
  throw err;
}

// 具名通道注册表（Node 为 WeakRefMap；具名通道有限集，Map 即可）
const channels = new Map();

function markActive(channel) {
  Object.setPrototypeOf(channel, ActiveChannel.prototype);
  channel._subscribers = [];
  channel._stores = new Map();
}

function maybeMarkInactive(channel) {
  if (!channel._subscribers.length && !channel._stores.size) {
    Object.setPrototypeOf(channel, Channel.prototype);
    channel._subscribers = undefined;
    channel._stores = undefined;
  }
}

// store 作用域：enter 集合收集，dispose 逆序恢复
function enterStores(activeChannel, data, exits) {
  if (activeChannel._stores) {
    for (const [store, transform] of activeChannel._stores.entries()) {
      let newContext = data;
      if (transform) {
        try {
          newContext = transform(data);
        } catch (err) {
          process.nextTick(() => { __dcUncaught(err); });
          continue;
        }
      }
      exits.push(store.__wjsWithScope(newContext));
    }
  }
}

function exitAll(exits) {
  while (exits.length > 0) {
    exits.pop()[SymbolDispose]();
  }
}

class ActiveChannel {
  subscribe(subscription) {
    validateFunction(subscription, 'subscription');
    this._subscribers = this._subscribers.slice();
    this._subscribers.push(subscription);
  }

  unsubscribe(subscription) {
    const index = this._subscribers.indexOf(subscription);
    if (index === -1) return false;

    const before = this._subscribers.slice(0, index);
    const after = this._subscribers.slice(index + 1);
    this._subscribers = before;
    this._subscribers.push(...after);

    maybeMarkInactive(this);
    return true;
  }

  bindStore(store, transform) {
    this._stores.set(store, transform);
  }

  unbindStore(store) {
    if (!this._stores.has(store)) return false;
    this._stores.delete(store);
    maybeMarkInactive(this);
    return true;
  }

  get hasSubscribers() {
    return true;
  }

  publish(data) {
    const subscribers = this._subscribers;
    for (let i = 0; i < (subscribers?.length || 0); i++) {
      try {
        const onMessage = subscribers[i];
        onMessage(data, this.name);
      } catch (err) {
        process.nextTick(() => { __dcUncaught(err); });
      }
    }
  }

  withStoreScope(data) {
    const exits = [];
    enterStores(this, data, exits);
    this.publish(data);
    return {
      [SymbolDispose]() {
        exitAll(exits);
      },
    };
  }

  runStores(data, fn, thisArg, ...args) {
    const scope = this.withStoreScope(data);
    try {
      return Reflect.apply(fn, thisArg, args);
    } finally {
      scope[SymbolDispose]();
    }
  }
}

class Channel {
  constructor(name) {
    this._subscribers = undefined;
    this._stores = undefined;
    this.name = name;
    this._index = undefined;
    channels.set(name, this);
  }

  static [Symbol.hasInstance](instance) {
    if (instance === undefined || instance === null) {
      // V8 文案桥（`Object.getPrototypeOf(undefined)` SM 文案为小写 "can't
      // convert…"，套件 `tracing-channel-args-types` 按 V8 "Cannot convert
      // undefined or null to object" 正则断言；true-node 同款抛错位点）
      throw new TypeError('Cannot convert undefined or null to object');
    }
    const prototype = Object.getPrototypeOf(instance);
    return prototype === Channel.prototype ||
           prototype === ActiveChannel.prototype;
  }

  subscribe(subscription) {
    validateFunction(subscription, 'subscription');
    markActive(this);
    this.subscribe(subscription);
  }

  unsubscribe() {
    return false;
  }

  bindStore(store, transform) {
    markActive(this);
    this.bindStore(store, transform);
  }

  unbindStore() {
    return false;
  }

  get hasSubscribers() {
    return false;
  }

  publish() {}

  runStores(data, fn, thisArg, ...args) {
    return Reflect.apply(fn, thisArg, args);
  }

  withStoreScope() {
    return { [SymbolDispose]() {} };
  }
}

function channel(name) {
  const existing = channels.get(name);
  if (existing) return existing;

  if (typeof name !== 'string' && typeof name !== 'symbol') {
    throw new ERR_INVALID_ARG_TYPE('channel', ['string', 'symbol'], name);
  }

  return new Channel(name);
}

function subscribe(name, subscription) {
  return channel(name).subscribe(subscription);
}

function unsubscribe(name, subscription) {
  return channel(name).unsubscribe(subscription);
}

function hasSubscribers(name) {
  const existing = channels.get(name);
  if (!existing) return false;
  return existing.hasSubscribers;
}

const boundedEvents = ['start', 'end'];

function assertChannel(value, name) {
  if (!(value instanceof Channel)) {
    throw new ERR_INVALID_ARG_TYPE(name, ['Channel'], value);
  }
}

function emitNonThenableWarning(fn) {
  process.emitWarning(`tracePromise was called with the function '${fn.name || '<anonymous>'}', ` +
                      'which returned a non-thenable.');
}

function channelFromMap(nameOrChannels, name, className) {
  if (typeof nameOrChannels === 'string') {
    return channel(`tracing:${nameOrChannels}:${name}`);
  }
  if (typeof nameOrChannels === 'object' && nameOrChannels !== null) {
    const chan = nameOrChannels[name];
    assertChannel(chan, `nameOrChannels.${name}`);
    return chan;
  }
  throw new ERR_INVALID_ARG_TYPE('nameOrChannels', ['string', 'object', className], nameOrChannels);
}

class BoundedChannel {
  constructor(nameOrChannels) {
    for (let i = 0; i < boundedEvents.length; ++i) {
      const eventName = boundedEvents[i];
      Object.defineProperty(this, eventName, {
        __proto__: null,
        value: channelFromMap(nameOrChannels, eventName, 'BoundedChannel'),
      });
    }
  }

  get hasSubscribers() {
    return this.start?.hasSubscribers || this.end?.hasSubscribers;
  }

  subscribe(handlers) {
    for (let i = 0; i < boundedEvents.length; ++i) {
      const name = boundedEvents[i];
      if (!handlers[name]) continue;
      this[name]?.subscribe(handlers[name]);
    }
  }

  unsubscribe(handlers) {
    let done = true;
    for (let i = 0; i < boundedEvents.length; ++i) {
      const name = boundedEvents[i];
      if (!handlers[name]) continue;
      if (!this[name]?.unsubscribe(handlers[name])) {
        done = false;
      }
    }
    return done;
  }

  withScope(context = {}) {
    // start publish + stores enter；dispose 时 end publish + 恢复
    const bounded = this;
    const scope = { context, exits: undefined };
    if (bounded.start.hasSubscribers || (bounded.start._stores && bounded.start._stores.size)) {
      const exits = [];
      enterStores(bounded.start, context, exits);
      bounded.start.publish(context);
      scope.exits = exits;
    }
    return {
      [SymbolDispose]() {
        if (scope.exits === undefined) return;
        bounded.end.publish(scope.context);
        exitAll(scope.exits);
        scope.exits = undefined;
      },
    };
  }

  run(context, fn, thisArg, ...args) {
    context ??= {};
    const scope = this.withScope(context);
    try {
      return Reflect.apply(fn, thisArg, args);
    } finally {
      scope[SymbolDispose]();
    }
  }
}

class TracingChannel {
  #callWindow;
  #continuationWindow;

  constructor(nameOrChannels) {
    if (typeof nameOrChannels === 'string') {
      this.#callWindow = new BoundedChannel(nameOrChannels);
      this.#continuationWindow = new BoundedChannel({
        start: channel(`tracing:${nameOrChannels}:asyncStart`),
        end: channel(`tracing:${nameOrChannels}:asyncEnd`),
      });
    } else if (typeof nameOrChannels === 'object') {
      this.#callWindow = new BoundedChannel({
        start: nameOrChannels.start,
        end: nameOrChannels.end,
      });
      this.#continuationWindow = new BoundedChannel({
        start: nameOrChannels.asyncStart,
        end: nameOrChannels.asyncEnd,
      });
    }

    Object.defineProperty(this, 'error', {
      __proto__: null,
      value: channelFromMap(nameOrChannels, 'error', 'TracingChannel'),
    });
  }

  get start() { return this.#callWindow.start; }
  get end() { return this.#callWindow.end; }
  get asyncStart() { return this.#continuationWindow.start; }
  get asyncEnd() { return this.#continuationWindow.end; }

  get hasSubscribers() {
    return this.#callWindow.hasSubscribers ||
      this.#continuationWindow.hasSubscribers ||
      this.error?.hasSubscribers;
  }

  subscribe(handlers) {
    if (handlers.start || handlers.end) {
      this.#callWindow.subscribe({ start: handlers.start, end: handlers.end });
    }
    if (handlers.asyncStart || handlers.asyncEnd) {
      this.#continuationWindow.subscribe({ start: handlers.asyncStart, end: handlers.asyncEnd });
    }
    if (handlers.error) {
      this.error.subscribe(handlers.error);
    }
  }

  unsubscribe(handlers) {
    let done = true;
    if (handlers.start || handlers.end) {
      if (!this.#callWindow.unsubscribe({ start: handlers.start, end: handlers.end })) {
        done = false;
      }
    }
    if (handlers.asyncStart || handlers.asyncEnd) {
      if (!this.#continuationWindow.unsubscribe({ start: handlers.asyncStart, end: handlers.asyncEnd })) {
        done = false;
      }
    }
    if (handlers.error) {
      if (!this.error.unsubscribe(handlers.error)) {
        done = false;
      }
    }
    return done;
  }

  traceSync(fn, context = undefined, thisArg, ...args) {
    if (!this.hasSubscribers) {
      return Reflect.apply(fn, thisArg, args);
    }

    if (context === undefined) {
      context = { __proto__: null };
    }

    const { error } = this;
    const scope = this.#callWindow.withScope(context);
    try {
      const result = Reflect.apply(fn, thisArg, args);
      context.result = result;
      return result;
    } catch (err) {
      context.error = err;
      error.publish(context);
      throw err;
    } finally {
      scope[SymbolDispose]();
    }
  }

  tracePromise(fn, context = undefined, thisArg, ...args) {
    if (!this.hasSubscribers) {
      const result = Reflect.apply(fn, thisArg, args);
      if (typeof result?.then !== 'function') {
        emitNonThenableWarning(fn);
      }
      return result;
    }

    if (context === undefined) {
      context = { __proto__: null };
    }

    const { error } = this;
    const continuationWindow = this.#continuationWindow;

    function onReject(err) {
      context.error = err;
      error.publish(context);
      const scope = continuationWindow.withScope(context);
      scope[SymbolDispose]();
    }

    function onRejectWithRethrow(err) {
      onReject(err);
      throw err;
    }

    function onResolve(result) {
      context.result = result;
      const scope = continuationWindow.withScope(context);
      scope[SymbolDispose]();
      return result;
    }

    const scope = this.#callWindow.withScope(context);
    try {
      const result = Reflect.apply(fn, thisArg, args);
      if (typeof result?.then !== 'function') {
        emitNonThenableWarning(fn);
        context.result = result;
        return result;
      }
      if (types.isPromise(result) && Object.getPrototypeOf(result) === Promise.prototype) {
        return result.then(onResolve, onRejectWithRethrow);
      }
      result.then(onResolve, onReject);
      return result;
    } catch (err) {
      context.error = err;
      error.publish(context);
      throw err;
    } finally {
      scope[SymbolDispose]();
    }
  }

  traceCallback(fn, position = -1, context = {}, thisArg, ...args) {
    if (!this.hasSubscribers) {
      return Reflect.apply(fn, thisArg, args);
    }

    const { error } = this;
    const continuationWindow = this.#continuationWindow;

    function wrappedCallback(err, res) {
      if (err) {
        context.error = err;
        error.publish(context);
      } else {
        context.result = res;
      }
      const scope = continuationWindow.withScope(context);
      try {
        return Reflect.apply(callback, this, arguments);
      } finally {
        scope[SymbolDispose]();
      }
    }

    const callback = args.at(position);
    validateFunction(callback, 'callback');
    args.splice(position, 1, wrappedCallback);

    const scope = this.#callWindow.withScope(context);
    try {
      return Reflect.apply(fn, thisArg, args);
    } catch (err) {
      context.error = err;
      error.publish(context);
      throw err;
    } finally {
      scope[SymbolDispose]();
    }
  }
}

function tracingChannel(nameOrChannels) {
  return new TracingChannel(nameOrChannels);
}

function boundedChannel(nameOrChannels) {
  return new BoundedChannel(nameOrChannels);
}

export {
  channel,
  hasSubscribers,
  subscribe,
  tracingChannel,
  unsubscribe,
  boundedChannel,
  Channel,
  BoundedChannel,
};
export default { channel, hasSubscribers, subscribe, tracingChannel, unsubscribe, boundedChannel, Channel, BoundedChannel };
"#;
