//! `node:events`（Node `lib/events.js` 全语义移植，MIT；bun-compat §4.2 三源对照落地）。
//!
//! 忠实移植：EventEmitter.init/emit（errorMonitor、captureRejections、kEmitting
//! 可变数组快照）、addListener/prepend(Once)Listener/once wrap、removeListener
//! （shapeMode 分支）、removeAllListeners、listeners/rawListeners/listenerCount/
//! eventNames、setMaxListeners/defaultMaxListeners（含 static 双形态）、
//! `events.once`（AbortSignal/error 语义）、`events.on` 异步迭代器（FixedQueue
//! 水位/pause/resume/close 事件）、`events.getEventListeners/getMaxListeners/
//! listenerCount`（EE + EventTarget 双形态）、EventEmitterAsyncResource（懒挂）、
//! enhanceStackTrace（identicalSequenceRange 折叠）。
//!
//! 偏差（bun-compat §4.1 口径，逐条记档）：
//! - primordials 解构还原为直接调用（无防篡改硬化）。
//! - `genericNodeError`/`ERR_UNHANDLED_ERROR` 的栈增强走
//!   `kEnhanceStackBeforeInspector` 符号预留（inspector 未做，enhanceStackTrace
//!   的 capture 用 `Error.captureStackTrace`（SM 支持，探针实测））。
//! - EventTarget 形态经 `node:internal/event_target` 鸭子类型（本仓 AbortSignal
//!   为极简实现）；`getEventListeners(target)` 对 Web EventTarget 返回空数组；
//!   `listenerCount(target)` 读原生侧表（10f：仅经帮助函数挂载的可见，用户直调
//!   `addEventListener` 不可见）。
//! - `process.emitWarning` 已补进 process prelude（warning 监听面 + stderr 打印）。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node lib/events.js (see module docs for deviations).
import { kEmptyObject, spliceOne } from 'node:internal/util';
import { inspect, identicalSequenceRange } from 'node:internal/util/inspect';
import errors from 'node:internal/errors';
const {
  AbortError,
  codes: {
    ERR_INVALID_ARG_TYPE: { HideStackFramesError: ERR_INVALID_ARG_TYPE },
    ERR_UNHANDLED_ERROR,
  },
  genericNodeError,
  kEnhanceStackBeforeInspector,
} = errors;
import {
  validateInteger,
  validateAbortSignal,
  validateBoolean,
  validateFunction,
  validateNumber,
  validateObject,
  validateString,
} from 'node:internal/validators';
import { addAbortListener, __etAdd, __etRemove, __etCount } from 'node:internal/events/abort_listener';
import FixedQueue from 'node:internal/fixed_queue';
import { kFirstEventParam } from 'node:internal/events/symbols';
import { kResistStopPropagation, isEventTarget } from 'node:internal/event_target';
import { AsyncResource } from 'node:async_hooks';

const kRejection = Symbol.for('nodejs.rejection');
// %AsyncIteratorPrototype%（primordials 同款算法）
const AsyncIteratorPrototype = Object.getPrototypeOf(
  Object.getPrototypeOf(async function* () {}).prototype);
const kCapture = Symbol('kCapture');
const kErrorMonitor = Symbol('events.errorMonitor');
const kShapeMode = Symbol('shapeMode');
const kEmitting = Symbol('events.emitting');
const kMaxEventTargetListeners = Symbol('events.maxEventTargetListeners');
const kMaxEventTargetListenersWarned = Symbol('events.maxEventTargetListenersWarned');
const kWatermarkData = Symbol.for('nodejs.watermarkData');

// EventEmitterAsyncResource（Node 懒挂因 bootstrap 顺序；此处直接定义，语义同）。
class EventEmitterReferencingAsyncResource extends AsyncResource {
  #eventEmitter;
  constructor(ee, type, options) {
    super(type, options);
    this.#eventEmitter = ee;
  }
  get eventEmitter() { return this.#eventEmitter; }
}

class EventEmitterAsyncResource extends EventEmitter {
  #asyncResource;
  constructor(options = undefined) {
    let name;
    if (typeof options === 'string') {
      name = options;
      options = undefined;
    } else {
      if (new.target === EventEmitterAsyncResource) {
        validateString(options?.name, 'options.name');
      }
      name = options?.name || new.target.name;
    }
    super(options);
    this.#asyncResource = new EventEmitterReferencingAsyncResource(this, name, options);
  }

  emit(event, ...args) {
    const asyncResource = this.#asyncResource;
    args.unshift(super.emit, this, event);
    return Reflect.apply(asyncResource.runInAsyncScope, asyncResource, args);
  }

  emitDestroy() { this.#asyncResource.emitDestroy(); }
  get asyncId() { return this.#asyncResource.asyncId(); }
  get triggerAsyncId() { return this.#asyncResource.triggerAsyncId(); }
  get asyncResource() { return this.#asyncResource; }
}

/**
 * Creates a new `EventEmitter` instance.
 */
function EventEmitter(opts) {
  EventEmitter.init.call(this, opts);
}

EventEmitter.EventEmitter = EventEmitter;
// Node module.exports 静态面（require('node:events').once 等直接可用）
EventEmitter.once = once;
EventEmitter.on = on;
EventEmitter.getEventListeners = getEventListeners;
EventEmitter.getMaxListeners = getMaxListeners;
EventEmitter.listenerCount = listenerCount;
EventEmitter.addAbortListener = addAbortListener;
EventEmitter.usingDomains = false;
EventEmitter.captureRejectionSymbol = kRejection;
Object.defineProperty(EventEmitter, 'captureRejections', {
  __proto__: null,
  get() { return EventEmitter.prototype[kCapture]; },
  set(value) {
    validateBoolean(value, 'EventEmitter.captureRejections');
    EventEmitter.prototype[kCapture] = value;
  },
  enumerable: true,
});

EventEmitter.EventEmitterAsyncResource = EventEmitterAsyncResource;
EventEmitter.errorMonitor = kErrorMonitor;
EventEmitter.kMaxEventTargetListeners = kMaxEventTargetListeners;
EventEmitter.kMaxEventTargetListenersWarned = kMaxEventTargetListenersWarned;

// The default for captureRejections is false
Object.defineProperty(EventEmitter.prototype, kCapture, {
  __proto__: null,
  value: false,
  writable: true,
  enumerable: false,
});

EventEmitter.prototype._events = undefined;
EventEmitter.prototype._eventsCount = 0;
EventEmitter.prototype._maxListeners = undefined;

// By default EventEmitters will print a warning if more than 10 listeners are
// added to it. This is a useful default which helps finding memory leaks.
let defaultMaxListeners = 10;

function checkListener(listener) {
  validateFunction(listener, 'listener');
}

Object.defineProperty(EventEmitter, 'defaultMaxListeners', {
  __proto__: null,
  enumerable: true,
  get() { return defaultMaxListeners; },
  set(arg) {
    validateNumber(arg, 'defaultMaxListeners', 0);
    defaultMaxListeners = arg;
  },
});

/**
 * Sets the max listeners (static form; EE 实例与 EventTarget 双支持)。
 */
EventEmitter.setMaxListeners =
  function(n = defaultMaxListeners, ...eventTargets) {
    validateNumber(n, 'setMaxListeners', 0);
    if (eventTargets.length === 0) {
      defaultMaxListeners = n;
    } else {
      for (let i = 0; i < eventTargets.length; i++) {
        const target = eventTargets[i];
        if (isEventTarget(target)) {
          target[kMaxEventTargetListeners] = n;
          target[kMaxEventTargetListenersWarned] = false;
        } else if (typeof target?.setMaxListeners === 'function') {
          target.setMaxListeners(n);
        } else {
          throw new ERR_INVALID_ARG_TYPE('eventTargets', ['EventEmitter', 'EventTarget'], target);
        }
      }
    }
  };

EventEmitter.init = function(opts) {
  if (this._events === undefined ||
      this._events === Object.getPrototypeOf(this)._events) {
    this._events = { __proto__: null };
    this._eventsCount = 0;
    this[kShapeMode] = false;
  } else {
    this[kShapeMode] = true;
  }

  this._maxListeners ||= undefined;

  if (opts?.captureRejections) {
    validateBoolean(opts.captureRejections, 'options.captureRejections');
    this[kCapture] = Boolean(opts.captureRejections);
  } else {
    this[kCapture] = EventEmitter.prototype[kCapture];
  }
};

function addCatch(that, promise, type, args) {
  if (!that[kCapture]) {
    return;
  }

  // Handle Promises/A+ spec, then could be a getter that throws on second use.
  try {
    const then = promise.then;
    if (typeof then === 'function') {
      then.call(promise, undefined, function(err) {
        // The callback is called with nextTick to avoid a follow-up
        // rejection from this promise.
        process.nextTick(emitUnhandledRejectionOrErr, that, err, type, args);
      });
    }
  } catch (err) {
    that.emit('error', err);
  }
}

function emitUnhandledRejectionOrErr(ee, err, type, args) {
  if (typeof ee[kRejection] === 'function') {
    ee[kRejection](err, type, ...args);
  } else {
    // We have to disable the capture rejections mechanism, otherwise
    // we might end up in an infinite loop.
    const prev = ee[kCapture];
    try {
      ee[kCapture] = false;
      ee.emit('error', err);
    } finally {
      ee[kCapture] = prev;
    }
  }
}

EventEmitter.prototype.setMaxListeners = function setMaxListeners(n) {
  validateNumber(n, 'setMaxListeners', 0);
  this._maxListeners = n;
  return this;
};

function _getMaxListeners(that) {
  if (that._maxListeners === undefined) return EventEmitter.defaultMaxListeners;
  return that._maxListeners;
}

EventEmitter.prototype.getMaxListeners = function getMaxListeners() {
  return _getMaxListeners(this);
};

function enhanceStackTrace(err, own) {
  let ctorInfo = '';
  try {
    const { name } = this.constructor;
    if (name !== 'EventEmitter') ctorInfo = ` on ${name} instance`;
  } catch {
    // Continue regardless of error.
  }
  const sep = `\nEmitted 'error' event${ctorInfo} at:\n`;

  const errStack = err.stack.split('\n').slice(1);
  const ownStack = own.stack.split('\n').slice(1);

  const { 0: len, 1: offset } = identicalSequenceRange(ownStack, errStack);
  if (len > 0) {
    ownStack.splice(offset + 1, len - 2, '    [... lines matching original stack trace ...]');
  }

  return err.stack + sep + ownStack.join('\n');
}

function getUnhandledErrorException(ee, args) {
  let er;
  if (args.length > 0) er = args[0];
  if (er instanceof Error) {
    try {
      const capture = {};
      // SpiderMonkey captureStackTrace 支持 (obj, constructorOpt)（探针实测）
      Error.captureStackTrace(capture, EventEmitter.prototype.emit);
      Object.defineProperty(er, kEnhanceStackBeforeInspector, {
        __proto__: null,
        value: enhanceStackTrace.bind(ee, er, capture),
        configurable: true,
      });
    } catch {
      // Continue regardless of error.
    }
    return er;
  }

  let stringifiedEr;
  try {
    stringifiedEr = inspect(er);
  } catch {
    stringifiedEr = er;
  }

  // At least give some kind of context to the user
  const err = new ERR_UNHANDLED_ERROR(stringifiedEr);
  err.context = er;
  return err;
}

/**
 * Synchronously calls each of the listeners registered for the event.
 */
EventEmitter.prototype.emit = function emit(type, ...args) {
  let doError = (type === 'error');

  const events = this._events;
  if (events !== undefined) {
    if (doError && events[kErrorMonitor] !== undefined)
      this.emit(kErrorMonitor, ...args);
    doError &&= events.error === undefined;
  } else if (!doError) {
    return false;
  }

  // If there is no 'error' event listener then throw.
  if (doError) {
    const er = getUnhandledErrorException(this, args);
    throw er; // Unhandled 'error' event
  }

  const handler = events[type];

  if (handler === undefined) return false;

  if (typeof handler === 'function') {
    const result = Reflect.apply(handler, this, args);
    if (result !== undefined && result !== null) {
      addCatch(this, result, type, args);
    }
  } else {
    handler[kEmitting]++;
    try {
      for (let i = 0; i < handler.length; ++i) {
        const result = Reflect.apply(handler[i], this, args);
        if (result !== undefined && result !== null) {
          addCatch(this, result, type, args);
        }
      }
    } finally {
      handler[kEmitting]--;
    }
  }

  return true;
};

function _addListener(target, type, listener, prepend) {
  let m;
  let events;
  let existing;

  checkListener(listener);

  events = target._events;
  if (events === undefined) {
    events = target._events = { __proto__: null };
    target._eventsCount = 0;
  } else {
    // To avoid recursion in the case that type === "newListener"! Before
    // adding it to the listeners, first emit "newListener".
    if (events.newListener !== undefined) {
      target.emit('newListener', type, listener.listener ?? listener);
      // Re-assign `events` because a newListener handler could have caused the
      // this._events to be assigned to a new object
      events = target._events;
    }
    existing = events[type];
  }

  if (existing === undefined) {
    // Optimize the case of one listener. Don't need the extra array object.
    events[type] = listener;
    ++target._eventsCount;
  } else {
    if (typeof existing === 'function') {
      // Adding the second element, need to change to array.
      existing = prepend ? [listener, existing] : [existing, listener];
      existing[kEmitting] = 0;
      events[type] = existing;
    } else {
      existing = ensureMutableListenerArray(events, type, existing);
      if (prepend) {
        existing.unshift(listener);
      } else {
        existing.push(listener);
      }
    }

    // Check for listener leak
    m = _getMaxListeners(target);
    if (m > 0 && existing.length > m && !existing.warned)
      warnMaxListenersExceeded(target, type, existing, m);
  }

  return target;
}

function warnMaxListenersExceeded(target, type, existing, m) {
  existing.warned = true;
  // No error code for this since it is a Warning
  const w = genericNodeError(
    `Possible EventEmitter memory leak detected. ${existing.length} ${String(type)} listeners ` +
    `added to ${inspect(target, { depth: -1 })}. MaxListeners is ${m}. Use emitter.setMaxListeners() to increase limit`,
    { name: 'MaxListenersExceededWarning', emitter: target, type: type, count: existing.length });
  process.emitWarning(w);
}

EventEmitter.prototype.addListener = function addListener(type, listener) {
  return _addListener(this, type, listener, false);
};

EventEmitter.prototype.on = EventEmitter.prototype.addListener;

EventEmitter.prototype.prependListener =
    function prependListener(type, listener) {
      return _addListener(this, type, listener, true);
    };

function _onceWrap(target, type, listener) {
  let fired = false;
  function wrapper(...args) {
    if (fired) return;
    fired = true;
    target.removeListener(type, wrapper);
    return Reflect.apply(listener, target, args);
  }
  wrapper.listener = listener;
  return wrapper;
}

EventEmitter.prototype.once = function once(type, listener) {
  checkListener(listener);
  this.on(type, _onceWrap(this, type, listener));
  return this;
};

EventEmitter.prototype.prependOnceListener =
    function prependOnceListener(type, listener) {
      checkListener(listener);
      this.prependListener(type, _onceWrap(this, type, listener));
      return this;
    };

EventEmitter.prototype.removeListener =
    function removeListener(type, listener) {
      checkListener(listener);

      const events = this._events;
      if (events === undefined) return this;

      let list = events[type];
      if (list === undefined) return this;

      if (list === listener || list.listener === listener) {
        this._eventsCount -= 1;

        if (this[kShapeMode]) {
          events[type] = undefined;
        } else if (this._eventsCount === 0) {
          this._events = { __proto__: null };
        } else {
          delete events[type];
        }

        if (events.removeListener !== undefined)
          this.emit('removeListener', type, list.listener || listener);
      } else if (typeof list !== 'function') {
        list = ensureMutableListenerArray(events, type, list);
        let position = -1;

        for (let i = list.length - 1; i >= 0; i--) {
          if (list[i] === listener || list[i].listener === listener) {
            position = i;
            break;
          }
        }

        if (position < 0) return this;

        if (position === 0) list.shift();
        else spliceOne(list, position);

        if (list.length === 1) events[type] = list[0];

        if (events.removeListener !== undefined)
          this.emit('removeListener', type, listener);
      }

      return this;
    };

EventEmitter.prototype.off = EventEmitter.prototype.removeListener;

EventEmitter.prototype.removeAllListeners =
    function removeAllListeners(type) {
      const events = this._events;
      if (events === undefined) return this;

      // Not listening for removeListener, no need to emit
      if (events.removeListener === undefined) {
        if (arguments.length === 0) {
          this._events = { __proto__: null };
          this._eventsCount = 0;
        } else if (events[type] !== undefined) {
          if (--this._eventsCount === 0) this._events = { __proto__: null };
          else delete events[type];
        }
        this[kShapeMode] = false;
        return this;
      }

      // Emit removeListener for all listeners on all events
      if (arguments.length === 0) {
        for (const key of Reflect.ownKeys(events)) {
          if (key === 'removeListener') continue;
          this.removeAllListeners(key);
        }
        this.removeAllListeners('removeListener');
        this._events = { __proto__: null };
        this._eventsCount = 0;
        this[kShapeMode] = false;
        return this;
      }

      const listeners = events[type];

      if (typeof listeners === 'function') {
        this.removeListener(type, listeners);
      } else if (listeners !== undefined) {
        // LIFO order
        for (let i = listeners.length - 1; i >= 0; i--) {
          this.removeListener(type, listeners[i]);
        }
      }

      return this;
    };

function _listeners(target, type, unwrap) {
  const events = target._events;

  if (events === undefined) return [];

  const evlistener = events[type];
  if (evlistener === undefined) return [];

  if (typeof evlistener === 'function')
    return unwrap ? [evlistener.listener || evlistener] : [evlistener];

  return unwrap ? unwrapListeners(evlistener) : arrayClone(evlistener);
}

EventEmitter.prototype.listeners = function listeners(type) {
  return _listeners(this, type, true);
};

EventEmitter.prototype.rawListeners = function rawListeners(type) {
  return _listeners(this, type, false);
};

EventEmitter.prototype.listenerCount = function listenerCount(type, listener) {
  const events = this._events;

  if (events !== undefined) {
    const evlistener = events[type];

    if (typeof evlistener === 'function') {
      if (listener != null) {
        return listener === evlistener || listener === evlistener.listener ? 1 : 0;
      }
      return 1;
    } else if (evlistener !== undefined) {
      if (listener != null) {
        let matching = 0;
        for (let i = 0, l = evlistener.length; i < l; i++) {
          if (evlistener[i] === listener || evlistener[i].listener === listener) {
            matching++;
          }
        }
        return matching;
      }
      return evlistener.length;
    }
  }

  return 0;
};

EventEmitter.prototype.eventNames = function eventNames() {
  if (this._eventsCount === 0) return [];
  const events = this._events;
  const names = [];
  for (const key of Reflect.ownKeys(events)) {
    // Removed listeners leave the key in place with an `undefined` value.
    if (events[key] !== undefined) names.push(key);
  }
  return names;
};

function arrayClone(arr) {
  switch (arr.length) {
    case 2: return [arr[0], arr[1]];
    case 3: return [arr[0], arr[1], arr[2]];
    case 4: return [arr[0], arr[1], arr[2], arr[3]];
    case 5: return [arr[0], arr[1], arr[2], arr[3], arr[4]];
    case 6: return [arr[0], arr[1], arr[2], arr[3], arr[4], arr[5]];
  }
  return arr.slice();
}

function cloneEventListenerArray(arr) {
  const copy = arrayClone(arr);
  copy[kEmitting] = 0;
  if (arr.warned) {
    copy.warned = true;
  }
  return copy;
}

function ensureMutableListenerArray(events, type, handler) {
  if (handler[kEmitting] > 0) {
    const copy = cloneEventListenerArray(handler);
    events[type] = copy;
    return copy;
  }
  return handler;
}

function unwrapListeners(arr) {
  const ret = arrayClone(arr);
  for (let i = 0; i < ret.length; ++i) {
    const orig = ret[i].listener;
    if (typeof orig === 'function') ret[i] = orig;
  }
  return ret;
}

/**
 * Returns a copy of the array of listeners for the event name
 * specified as `type` (EE + EventTarget 双形态)。
 */
function getEventListeners(emitterOrTarget, type) {
  // First check if EventEmitter
  if (typeof emitterOrTarget?.listeners === 'function') {
    return emitterOrTarget.listeners(type);
  }
  if (isEventTarget(emitterOrTarget)) {
    // 本仓 Web EventTarget 无 kEvents 存储（偏差见模块头注）。
    return [];
  }
  throw new ERR_INVALID_ARG_TYPE('emitter', ['EventEmitter', 'EventTarget'], emitterOrTarget);
}

/**
 * Returns the max listeners set (EE + EventTarget 双形态)。
 */
function getMaxListeners(emitterOrTarget) {
  if (typeof emitterOrTarget?.getMaxListeners === 'function') {
    return _getMaxListeners(emitterOrTarget);
  } else if (typeof emitterOrTarget?.[kMaxEventTargetListeners] === 'number') {
    return emitterOrTarget[kMaxEventTargetListeners];
  } else if (isEventTarget(emitterOrTarget)) {
    // 10f：未显式设置时 EventTarget 回默认，AbortSignal 回 0（真机口径）。
    if (typeof AbortSignal === 'function' && emitterOrTarget instanceof AbortSignal) return 0;
    return defaultMaxListeners;
  }

  throw new ERR_INVALID_ARG_TYPE('emitter', ['EventEmitter', 'EventTarget'], emitterOrTarget);
}

/**
 * Returns the number of registered listeners for `type` (EE + EventTarget 双形态)。
 */
function listenerCount(emitterOrTarget, type) {
  if (typeof emitterOrTarget.listenerCount === 'function') {
    return emitterOrTarget.listenerCount(type);
  }
  if (isEventTarget(emitterOrTarget)) {
    // 10f：原生 EventTarget 读侧表（经帮助函数挂载的监听可见；
    // 用户直调 addEventListener 不可见，记档）。
    return __etCount(emitterOrTarget, type);
  }
  throw new ERR_INVALID_ARG_TYPE('emitter', ['EventEmitter', 'EventTarget'], emitterOrTarget);
}

/**
 * Creates a `Promise` that is fulfilled when the emitter emits the given event.
 */
async function once(emitter, name, options = kEmptyObject) {
  validateObject(options, 'options');
  const { signal } = options;
  validateAbortSignal(signal, 'options.signal');
  if (signal?.aborted)
    throw new AbortError(undefined, { cause: signal.reason });
  return new Promise((resolve, reject) => {
    const errorListener = (err) => {
      emitter.removeListener(name, resolver);
      if (signal != null) {
        eventTargetAgnosticRemoveListener(signal, 'abort', abortListener);
      }
      reject(err);
    };
    const resolver = (...args) => {
      if (typeof emitter.removeListener === 'function') {
        emitter.removeListener('error', errorListener);
      }
      if (signal != null) {
        eventTargetAgnosticRemoveListener(signal, 'abort', abortListener);
      }
      resolve(args);
    };

    const opts = { __proto__: null, once: true, [kResistStopPropagation]: true };
    eventTargetAgnosticAddListener(emitter, name, resolver, opts);
    if (name !== 'error' && typeof emitter.once === 'function') {
      // EventTarget does not have `error` event semantics like Node
      // EventEmitters, we listen to `error` events only on EventEmitters.
      emitter.once('error', errorListener);
    }
    function abortListener() {
      // 10f：先自摘（原生 addEventListener 忽略 once 选项，侧表靠显式摘除；
      // 不摘则 abort 后 listenerCount 仍为 1，套件点名）。
      if (signal != null) {
        eventTargetAgnosticRemoveListener(signal, 'abort', abortListener);
      }
      eventTargetAgnosticRemoveListener(emitter, name, resolver);
      eventTargetAgnosticRemoveListener(emitter, 'error', errorListener);
      reject(new AbortError(undefined, { cause: signal?.reason }));
    }
    if (signal != null) {
      eventTargetAgnosticAddListener(
        signal, 'abort', abortListener, { __proto__: null, once: true, [kResistStopPropagation]: true });
    }
  });
}

function createIterResult(value, done) {
  return { done, value };
}

function eventTargetAgnosticRemoveListener(emitter, name, listener, flags) {
  if (typeof emitter.removeListener === 'function') {
    emitter.removeListener(name, listener);
  } else if (typeof emitter.removeEventListener === 'function') {
    emitter.removeEventListener(name, listener, flags);
    // 10f：原生 EventTarget 侧表同步摘除（`listenerCount` 可读）。
    __etRemove(emitter, name, listener);
  } else {
    throw new ERR_INVALID_ARG_TYPE('emitter', 'EventEmitter', emitter);
  }
}

function eventTargetAgnosticAddListener(emitter, name, listener, flags) {
  if (typeof emitter.on === 'function') {
    if (flags?.once) {
      emitter.once(name, listener);
    } else {
      emitter.on(name, listener);
    }
  } else if (typeof emitter.addEventListener === 'function') {
    emitter.addEventListener(name, listener, flags);
    // 10f：原生 EventTarget 侧表同步登记（`listenerCount` 可读）。
    __etAdd(emitter, name, listener);
  } else {
    throw new ERR_INVALID_ARG_TYPE('emitter', 'EventEmitter', emitter);
  }
}

/**
 * Returns an `AsyncIterator` that iterates `event` events.
 */
function on(emitter, event, options = kEmptyObject) {
  // Parameters validation
  validateObject(options, 'options');
  const signal = options.signal;
  validateAbortSignal(signal, 'options.signal');
  if (signal?.aborted)
    throw new AbortError(undefined, { cause: signal.reason });
  // Support both highWaterMark and highWatermark for backward compatibility
  const highWatermark = options.highWaterMark ?? options.highWatermark ?? 9007199254740991;
  validateInteger(highWatermark, 'options.highWaterMark', 1);
  // Support both lowWaterMark and lowWatermark for backward compatibility
  const lowWatermark = options.lowWaterMark ?? options.lowWatermark ?? 1;
  validateInteger(lowWatermark, 'options.lowWaterMark', 1);

  // Preparing controlling queues and variables
  const unconsumedEvents = new FixedQueue();
  const unconsumedPromises = new FixedQueue();
  let paused = false;
  let error = null;
  let finished = false;
  let size = 0;

  const iterator = Object.setPrototypeOf({
    next() {
      // First, we consume all unread events
      if (size) {
        const value = unconsumedEvents.shift();
        size--;
        if (paused && size < lowWatermark) {
          emitter.resume(); // Can not be finished yet
          paused = false;
        }
        return Promise.resolve(createIterResult(value, false));
      }

      // Then we error, if an error happened
      // This happens one time if at all, because after 'error'
      // we stop listening
      if (error) {
        const p = Promise.reject(error);
        // Only the first element errors
        error = null;
        return p;
      }

      // If the iterator is finished, resolve to done
      if (finished) return closeHandler();

      // Wait until an event happens
      return new Promise(function(resolve, reject) {
        unconsumedPromises.push({ resolve, reject });
      });
    },

    return() {
      return closeHandler();
    },

    throw(err) {
      if (!err || !(err instanceof Error)) {
        throw new ERR_INVALID_ARG_TYPE('EventEmitter.AsyncIterator', 'Error', err);
      }
      errorHandler(err);
    },
    [Symbol.asyncIterator]() {
      return this;
    },
    [kWatermarkData]: {
      get size() { return size; },
      get low() { return lowWatermark; },
      get high() { return highWatermark; },
      get isPaused() { return paused; },
    },
  }, AsyncIteratorPrototype);

  // Adding event handlers
  const { addEventListener, removeAll } = listenersController();
  addEventListener(emitter, event, options[kFirstEventParam] ? eventHandler : function(...args) {
    return eventHandler(args);
  });
  if (event !== 'error' && typeof emitter.on === 'function') {
    addEventListener(emitter, 'error', errorHandler);
  }
  const closeEvents = options?.close;
  if (closeEvents?.length) {
    for (let i = 0; i < closeEvents.length; i++) {
      addEventListener(emitter, closeEvents[i], closeHandler);
    }
  }

  const abortListenerDisposable = signal ? addAbortListener(signal, abortListener) : null;

  return iterator;

  function abortListener() {
    errorHandler(new AbortError(undefined, { cause: signal?.reason }));
  }

  function eventHandler(value) {
    if (unconsumedPromises.isEmpty()) {
      size++;
      if (!paused && size > highWatermark) {
        paused = true;
        emitter.pause();
      }
      unconsumedEvents.push(value);
    } else unconsumedPromises.shift().resolve(createIterResult(value, false));
  }

  function errorHandler(err) {
    if (unconsumedPromises.isEmpty()) error = err;
    else unconsumedPromises.shift().reject(err);

    closeHandler();
  }

  function closeHandler() {
    abortListenerDisposable?.[Symbol.dispose]?.();
    removeAll();
    finished = true;
    paused = false;
    const doneResult = createIterResult(undefined, true);
    while (!unconsumedPromises.isEmpty()) {
      unconsumedPromises.shift().resolve(doneResult);
    }

    return Promise.resolve(doneResult);
  }
}

function listenersController() {
  const listeners = [];

  return {
    addEventListener(emitter, event, handler, flags) {
      eventTargetAgnosticAddListener(emitter, event, handler, flags);
      listeners.push([emitter, event, handler, flags]);
    },
    removeAll() {
      while (listeners.length > 0) {
        Reflect.apply(eventTargetAgnosticRemoveListener, undefined, listeners.pop());
      }
    },
  };
}

export default EventEmitter;
// 10f：补齐真机具名导出（test-events-getmaxlisteners 点名；captureRejections/
// usingDomains 薄值，init 转调 EventEmitter.init）。
let captureRejections = false;
const usingDomains = false;
const setMaxListeners = EventEmitter.setMaxListeners;
const init = EventEmitter.init;
export { EventEmitter, EventEmitterAsyncResource, once, on, getEventListeners, getMaxListeners, setMaxListeners, defaultMaxListeners, captureRejections, init, usingDomains, listenerCount, addAbortListener, kErrorMonitor as errorMonitor, kRejection as captureRejectionSymbol, kFirstEventParam };
"#;
