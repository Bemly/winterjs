//! `node:internal/test/mock`（Node `lib/internal/test_runner/mock/mock.js`
//! 移植，MIT；`module()`  loader 钩与 `timers` 另片，未移植）。
//!
//! 逐字点：MockFunctionContext 调用记录 `{arguments, error, result, stack,
//! target, this}` + `times` 到顶自 restore + `mockImplementationOnce` 下标门；
//! Proxy 壳（target 即原函数，name/length/descriptor 全透传；construct 经
//! `ReflectConstruct(impl, args, proxy)` 故原型归原函数）；method 经原型链找
//! 描述符但自有属性安装（原型链无损）；getter/setter 糖与互斥文案；property
//! 访问记录 + once 值 + writable 门；restore 全家与 tracker 级 restoreAll。
//!
//! 偏差：
//! - primordials 还原为直接调用（本仓既定口径）。
//! - 非 configurable 方法：引擎原生文案（`can't redefine non-configurable
//!   property`）与 V8（`Cannot redefine property: X`，套件正则钉住）不同——
//!   此处预检并抛 V8 文案桥（test-runner-mocking.js `method() fails if method
//!   cannot be redefined` 点名）。
//! - `restore()` 的方法分支按 `methodName !== undefined` 判（含 symbol；真机
//!   仅判 string，symbol 复原是其漏口——此处更正，套件不可见）。
//! - 私有 `#` 改普通字段（本仓 prelude 风格；实例一律 new，无 §4.23 问题）。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Copyright Node.js contributors. MIT.
// Port of node:internal/test_runner/mock/mock.js (MockTracker core only:
// no module() loader hooks, no timers — separate slices).
import { AbortError, codes } from "node:internal/errors";
import validators from "node:internal/validators";
import { addAbortListener } from "node:internal/events/abort_listener";
import * as nodeTimersPromises from "node:timers/promises";

const { validateAbortSignal, validateBoolean, validateFunction, validateInteger, validateNumber, validateObject, validateStringArray, validateUint32 } = validators;
const { ERR_INVALID_ARG_TYPE, ERR_INVALID_ARG_VALUE, ERR_INVALID_STATE } = codes;
// node internal/timers TIMEOUT_MAX（2**31 - 1）同值。
const TIMEOUT_MAX = 2147483647;

function kDefaultFunction() {}

function validateStringOrSymbol(value, name) {
  if (typeof value !== "string" && typeof value !== "symbol") {
    throw new ERR_INVALID_ARG_TYPE(name, ["string", "symbol"], value);
  }
}

function validateTimes(value, name) {
  if (value === Infinity) return;
  validateInteger(value, name, 1);
}

class MockFunctionContext {
  constructor(implementation, restore, times) {
    this._calls = [];
    this._mocks = new Map();
    this._implementation = implementation;
    this._restore = restore;
    this._times = times;
  }
  get calls() {
    return this._calls.slice();
  }
  callCount() {
    return this._calls.length;
  }
  mockImplementation(implementation) {
    validateFunction(implementation, "implementation");
    this._implementation = implementation;
  }
  mockImplementationOnce(implementation, onCall) {
    validateFunction(implementation, "implementation");
    const nextCall = this._calls.length;
    const call = onCall ?? nextCall;
    validateInteger(call, "onCall", nextCall);
    this._mocks.set(call, implementation);
  }
  restore() {
    const { descriptor, object, original, methodName } = this._restore;
    if (methodName !== undefined) {
      Object.defineProperty(object, methodName, { ...descriptor });
    } else {
      this._implementation = original;
    }
  }
  resetCalls() {
    this._calls = [];
  }
  trackCall(call) {
    this._calls.push(call);
  }
  nextImpl() {
    const nextCall = this._calls.length;
    const impl = this._mocks.has(nextCall) ? this._mocks.get(nextCall) : this._implementation;
    if (nextCall + 1 === this._times) {
      this.restore();
    }
    this._mocks.delete(nextCall);
    return impl;
  }
}

class MockPropertyContext {
  constructor(object, propertyName, value, hasValue) {
    this._onceValues = new Map();
    this._accesses = [];
    this._object = object;
    this._propertyName = propertyName;
    this._originalValue = object[propertyName];
    this._value = hasValue ? value : this._originalValue;
    this._descriptor = Object.getOwnPropertyDescriptor(object, propertyName);
    if (!this._descriptor) {
      throw new ERR_INVALID_ARG_VALUE("propertyName", propertyName, "is not a property of the object");
    }
    const self = this;
    const { configurable, enumerable } = this._descriptor;
    Object.defineProperty(object, propertyName, {
      configurable,
      enumerable,
      get() {
        const nextValue = self._getAccessValue(self._value);
        self._accesses.push({ type: "get", value: nextValue, stack: new Error() });
        return nextValue;
      },
      set: (v) => self.mockImplementation(v),
    });
  }
  get accesses() {
    return this._accesses.slice();
  }
  accessCount() {
    return this._accesses.length;
  }
  mockImplementation(value) {
    if (!this._descriptor.writable) {
      throw new ERR_INVALID_ARG_VALUE("propertyName", this._propertyName, "cannot be set");
    }
    const nextValue = this._getAccessValue(value);
    this._accesses.push({ type: "set", value: nextValue, stack: new Error() });
    this._value = nextValue;
  }
  mockImplementationOnce(value, onAccess) {
    const nextAccess = this._accesses.length;
    const accessIndex = onAccess ?? nextAccess;
    validateInteger(accessIndex, "onAccess", nextAccess);
    this._onceValues.set(accessIndex, value);
  }
  resetAccesses() {
    this._accesses = [];
  }
  restore() {
    Object.defineProperty(this._object, this._propertyName, {
      ...this._descriptor,
      value: this._originalValue,
    });
  }
  _getAccessValue(value) {
    const accessIndex = this._accesses.length;
    let accessValue;
    if (this._onceValues.has(accessIndex)) {
      accessValue = this._onceValues.get(accessIndex);
      this._onceValues.delete(accessIndex);
    } else {
      accessValue = value;
    }
    return accessValue;
  }
}

class MockTracker {
  constructor() {
    this._mocks = [];
    this._timers = null;
  }
  get timers() {
    if (!this._timers) this._timers = new MockTimers();
    return this._timers;
  }
  fn(original = function () {}, implementation = original, options = {}) {
    if (original !== null && typeof original === "object") {
      options = original;
      original = function () {};
      implementation = original;
    } else if (implementation !== null && typeof implementation === "object") {
      options = implementation;
      implementation = original;
    }
    validateFunction(original, "original");
    validateFunction(implementation, "implementation");
    validateObject(options, "options");
    const { times = Infinity } = options;
    validateTimes(times, "options.times");
    const ctx = new MockFunctionContext(implementation, { original }, times);
    return this._setupMock(ctx, original);
  }
  method(objectOrFunction, methodName, implementation = kDefaultFunction, options = {}) {
    validateStringOrSymbol(methodName, "methodName");
    if (typeof objectOrFunction !== "function") {
      validateObject(objectOrFunction, "object");
    }
    if (implementation !== null && typeof implementation === "object") {
      options = implementation;
      implementation = kDefaultFunction;
    }
    validateFunction(implementation, "implementation");
    validateObject(options, "options");
    const { getter = false, setter = false, times = Infinity } = options;
    validateBoolean(getter, "options.getter");
    validateBoolean(setter, "options.setter");
    validateTimes(times, "options.times");
    if (setter && getter) {
      throw new ERR_INVALID_ARG_VALUE("options.setter", setter, "cannot be used with 'options.getter'");
    }
    const descriptor = findMethodOnPrototypeChain(objectOrFunction, methodName);
    let original;
    if (getter) {
      original = descriptor?.get;
    } else if (setter) {
      original = descriptor?.set;
    } else {
      original = descriptor?.value;
    }
    if (typeof original !== "function") {
      throw new ERR_INVALID_ARG_VALUE("methodName", original, "must be a method");
    }
    if (descriptor && descriptor.configurable === false) {
      throw new TypeError(`Cannot redefine property: ${String(methodName)}`);
    }
    const restore = { descriptor, object: objectOrFunction, methodName };
    const impl = implementation === kDefaultFunction ? original : implementation;
    const ctx = new MockFunctionContext(impl, restore, times);
    const mock = this._setupMock(ctx, original);
    const mockDescriptor = {
      configurable: descriptor.configurable,
      enumerable: descriptor.enumerable,
    };
    if (getter) {
      mockDescriptor.get = mock;
      mockDescriptor.set = descriptor.set;
    } else if (setter) {
      mockDescriptor.get = descriptor.get;
      mockDescriptor.set = mock;
    } else {
      mockDescriptor.writable = descriptor.writable;
      mockDescriptor.value = mock;
    }
    Object.defineProperty(objectOrFunction, methodName, mockDescriptor);
    return mock;
  }
  getter(object, methodName, implementation = kDefaultFunction, options = {}) {
    if (implementation !== null && typeof implementation === "object") {
      options = implementation;
      implementation = kDefaultFunction;
    } else {
      validateObject(options, "options");
    }
    const { getter = true } = options;
    if (getter === false) {
      throw new ERR_INVALID_ARG_VALUE("options.getter", getter, "cannot be false");
    }
    return this.method(object, methodName, implementation, { ...options, getter });
  }
  setter(object, methodName, implementation = kDefaultFunction, options = {}) {
    if (implementation !== null && typeof implementation === "object") {
      options = implementation;
      implementation = kDefaultFunction;
    } else {
      validateObject(options, "options");
    }
    const { setter = true } = options;
    if (setter === false) {
      throw new ERR_INVALID_ARG_VALUE("options.setter", setter, "cannot be false");
    }
    return this.method(object, methodName, implementation, { ...options, setter });
  }
  property(object, propertyName, ...rest) {
    validateObject(object, "object");
    validateStringOrSymbol(propertyName, "propertyName");
    const ctx = rest.length > 0
      ? new MockPropertyContext(object, propertyName, rest[0], true)
      : new MockPropertyContext(object, propertyName, undefined, false);
    this._mocks.push({ ctx, restore: (c) => c.restore() });
    const target = object;
    return new Proxy(target, {
      get(t, property, receiver) {
        if (property === "mock") return ctx;
        return Reflect.get(t, property, receiver);
      },
    });
  }
  reset() {
    this.restoreAll();
    try {
      if (this._timers) this._timers.reset();
    } catch {}
    this._mocks = [];
  }
  restoreAll() {
    for (const { ctx, restore } of this._mocks) {
      restore(ctx);
    }
  }
  _setupMock(ctx, fnToMatch) {
    const self = this;
    const mock = new Proxy(fnToMatch, {
      apply(_fn, thisArg, argList) {
        const fn = ctx.nextImpl();
        let result;
        let error;
        try {
          result = Reflect.apply(fn, thisArg, argList);
        } catch (err) {
          error = err;
          throw err;
        } finally {
          ctx.trackCall({ arguments: argList, error, result, stack: new Error(), target: undefined, this: thisArg });
        }
        return result;
      },
      construct(target, argList, newTarget) {
        const realTarget = ctx.nextImpl();
        let result;
        let error;
        try {
          result = Reflect.construct(realTarget, argList, newTarget);
        } catch (err) {
          error = err;
          throw err;
        } finally {
          ctx.trackCall({ arguments: argList, error, result, stack: new Error(), target, this: result });
        }
        return result;
      },
      get(target, property, receiver) {
        if (property === "mock") return ctx;
        return Reflect.get(target, property, receiver);
      },
    });
    this._mocks.push({ ctx, restore: (c) => c.restore() });
    return mock;
  }
}

function findMethodOnPrototypeChain(instance, methodName) {
  let host = instance;
  let descriptor;
  while (host !== null) {
    descriptor = Object.getOwnPropertyDescriptor(host, methodName);
    if (descriptor) break;
    host = Object.getPrototypeOf(host);
  }
  return descriptor;
}

// ---- MockTimers（B2，node mock_timers.js 移植） ----
//
// 逐字点：enable 校验（now NaN/类型/负值三门 + apis 白名单）/tick 按
// (runAt, id) 发射 + interval 重排 + 回调内自清跳过/setTime 只拨钟不发射/
// reset 复原全部补丁 + 测试结束经 tracker.reset() 全量复原。
//
// 偏差（引擎边界，套件不覆盖）：
// - `node:timers` / `node:timers/promises` 的具名函数补丁跳过——ESM 命名空间
//   冻结不可写（`setTimeout is read-only`），只补全局 + scheduler 对象 +
//   Date + AbortSignal.timeout。两 timers 套件仅用全局与 scheduler，无碍。
// - 优先队列用插入排序小数组（量级极小，与堆同序）。
const SUPPORTED_APIS = ["setTimeout", "setInterval", "setImmediate", "Date", "scheduler.wait", "AbortSignal.timeout"];
const kInitialEpoch = 0;
const kImmediateDelay = -1;
// abort_listener 同款回退键（引擎无 Symbol.dispose 时一致）。
const __disposeKey = Symbol.dispose ?? Symbol.for("Symbol.dispose");

class TimerQueue {
  constructor() {
    this.items = [];
  }
  peek() {
    return this.items.length > 0 ? this.items[0] : undefined;
  }
  peekBottom() {
    return this.items.length > 0 ? this.items[this.items.length - 1] : undefined;
  }
  insert(t) {
    let lo = 0, hi = this.items.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      const m = this.items[mid];
      if (m.runAt < t.runAt || (m.runAt === t.runAt && m.id < t.id)) lo = mid + 1;
      else hi = mid;
    }
    this.items.splice(lo, 0, t);
  }
  shift() {
    return this.items.shift();
  }
  remove(t) {
    const i = this.items.indexOf(t);
    if (i >= 0) this.items.splice(i, 1);
  }
  clear() {
    this.items = [];
  }
}

class MockTimeout {
  constructor(mock, id, callback, runAt, interval, args) {
    this._mock = mock;
    this.id = id;
    this.callback = callback;
    this.runAt = runAt;
    this.interval = interval;
    this.args = args;
    this.queued = true;
  }
  hasRef() {
    return true;
  }
  ref() {
    return this;
  }
  unref() {
    return this;
  }
  refresh() {
    return this;
  }
  close() {
    this._mock._clearTimer(this);
    return this;
  }
}

class MockTimers {
  constructor() {
    this._timersInContext = [];
    this._isEnabled = false;
    this._currentTimer = 1;
    this._now = kInitialEpoch;
    this._queue = new TimerQueue();
    this._saved = {};
  }
  _createTimer(isInterval, callback, delay, ...args) {
    if (delay > TIMEOUT_MAX) delay = 1;
    const timer = new MockTimeout(this, this._currentTimer++, callback, this._now + delay, isInterval ? delay : undefined, args);
    this._queue.insert(timer);
    return timer;
  }
  _clearTimer(timer) {
    if (!timer) return;
    this._queue.remove(timer);
    timer.queued = false;
    timer.interval = undefined;
  }
  _fakeSetTimeout(callback, delay, ...args) {
    return this._createTimer(false, callback, delay, ...args);
  }
  _fakeClearTimeout(timer) {
    this._clearTimer(timer);
  }
  _fakeSetInterval(callback, delay, ...args) {
    return this._createTimer(true, callback, delay, ...args);
  }
  _fakeSetImmediate(callback, ...args) {
    return this._createTimer(false, callback, kImmediateDelay, ...args);
  }
  _fakeSchedulerWait(delay, options) {
    return this._setTimeoutPromisified(delay, undefined, options);
  }
  async _setTimeoutPromisified(ms, result, options) {
    if (options?.signal) {
      validateAbortSignal(options.signal, "options.signal");
      if (options.signal.aborted) {
        throw new AbortError(undefined, { cause: options.signal.reason });
      }
    }
    let resolvePromise, rejectPromise;
    const promise = new Promise((resolve, reject) => {
      resolvePromise = resolve;
      rejectPromise = reject;
    });
    let abortListener;
    if (options?.signal) {
      abortListener = addAbortListener(options.signal, () => {
        rejectPromise(new AbortError(undefined, { cause: options.signal.reason }));
      });
    }
    const timer = this._createTimer(false, () => resolvePromise(result), ms);
    try {
      await promise;
      return result;
    } finally {
      try { abortListener?.[__disposeKey]?.(); } catch {}
      this._clearTimer(timer);
    }
  }
  _createDate(NativeDateConstructor) {
    if (NativeDateConstructor.isMock) {
      throw new ERR_INVALID_STATE("Date is already being mocked!");
    }
    const mock = this;
    function MockDate(year, month, date, hours, minutes, seconds, ms) {
      if (!new.target) {
        return String(new NativeDateConstructor(mock._now));
      }
      switch (arguments.length) {
        case 0: return new NativeDateConstructor(mock._now);
        case 1: return new NativeDateConstructor(year);
        case 2: return new NativeDateConstructor(year, month);
        case 3: return new NativeDateConstructor(year, month, date);
        case 4: return new NativeDateConstructor(year, month, date, hours);
        case 5: return new NativeDateConstructor(year, month, date, hours, minutes);
        case 6: return new NativeDateConstructor(year, month, date, hours, minutes, seconds);
        default: return new NativeDateConstructor(year, month, date, hours, minutes, seconds, ms);
      }
    }
    MockDate.now = function now() {
      return mock._now;
    };
    MockDate.toString = function toString() {
      // 真机可观测串（V8 单行；SM 原生多行，套件逐字钉住）。
      return "function Date() { [native code] }";
    };
    Object.defineProperties(MockDate, {
      isMock: { enumerable: true, configurable: false, writable: false, value: true },
    });
    MockDate.prototype = NativeDateConstructor.prototype;
    MockDate.parse = NativeDateConstructor.parse;
    MockDate.UTC = NativeDateConstructor.UTC;
    return MockDate;
  }
  _patchGlobal(name, fake) {
    if (!this._saved[name]) {
      this._saved[name] = Object.getOwnPropertyDescriptor(globalThis, name);
    }
    globalThis[name] = fake;
  }
  _restoreGlobal(name) {
    const desc = this._saved[name];
    if (desc) {
      Object.defineProperty(globalThis, name, desc);
      delete this._saved[name];
    }
  }
  _toggle(activate) {
    const self = this;
    const toFake = {
      "setTimeout"() {
        self._patchGlobal("setTimeout", (...a) => self._fakeSetTimeout(...a));
        self._patchGlobal("clearTimeout", (t) => self._fakeClearTimeout(t));
      },
      "setInterval"() {
        self._patchGlobal("setInterval", (...a) => self._fakeSetInterval(...a));
        self._patchGlobal("clearInterval", (t) => self._fakeClearTimeout(t));
      },
      "setImmediate"() {
        self._patchGlobal("setImmediate", (...a) => self._fakeSetImmediate(...a));
        self._patchGlobal("clearImmediate", (t) => self._fakeClearTimeout(t));
      },
      "scheduler.wait"() {
        const sched = nodeTimersPromises.scheduler;
        if (!self._saved["scheduler.wait"]) {
          self._saved["scheduler.wait"] = Object.hasOwn(sched, "wait")
            ? Object.getOwnPropertyDescriptor(sched, "wait")
            : "absent";
        }
        sched.wait = (...a) => self._fakeSchedulerWait(...a);
      },
      "Date"() {
        if (!self._saved["Date"]) {
          self._saved["Date"] = Object.getOwnPropertyDescriptor(globalThis, "Date");
        }
        globalThis.Date = self._createDate(self._saved["Date"].value);
      },
      "AbortSignal.timeout"() {
        if (!self._saved["AbortSignal.timeout"]) {
          self._saved["AbortSignal.timeout"] = Object.getOwnPropertyDescriptor(AbortSignal, "timeout");
        }
        Object.defineProperty(AbortSignal, "timeout", {
          configurable: true,
          writable: true,
          value(delay) {
            validateUint32(delay, "delay", false);
            const controller = new AbortController();
            self._createTimer(false, () => controller.abort(), delay);
            return controller.signal;
          },
        });
      },
    };
    const toReal = {
      "setTimeout"() { self._restoreGlobal("setTimeout"); self._restoreGlobal("clearTimeout"); },
      "setInterval"() { self._restoreGlobal("setInterval"); self._restoreGlobal("clearInterval"); },
      "setImmediate"() { self._restoreGlobal("setImmediate"); self._restoreGlobal("clearImmediate"); },
      "scheduler.wait"() {
        const saved = self._saved["scheduler.wait"];
        if (saved === "absent") {
          delete nodeTimersPromises.scheduler.wait;
        } else if (saved) {
          Object.defineProperty(nodeTimersPromises.scheduler, "wait", saved);
        }
        delete self._saved["scheduler.wait"];
      },
      "Date"() { self._restoreGlobal("Date"); },
      "AbortSignal.timeout"() {
        const saved = self._saved["AbortSignal.timeout"];
        if (saved) {
          Object.defineProperty(AbortSignal, "timeout", saved);
          delete self._saved["AbortSignal.timeout"];
        }
      },
    };
    const target = activate ? toFake : toReal;
    for (const api of this._timersInContext) target[api]();
    this._isEnabled = activate;
  }
  _assertEnabled() {
    if (!this._isEnabled) {
      throw new ERR_INVALID_STATE("You should enable MockTimers first by calling the .enable function");
    }
  }
  _assertTimeArg(time) {
    if (time < 0) {
      throw new ERR_INVALID_ARG_VALUE("time", "positive integer", time);
    }
  }
  _isValidDateWithGetTime(maybeDate) {
    try {
      maybeDate.getTime();
      return true;
    } catch {
      return false;
    }
  }
  tick(time = 1) {
    this._assertEnabled();
    this._assertTimeArg(time);
    this._now += time;
    let timer = this._queue.peek();
    while (timer) {
      if (timer.runAt > this._now) break;
      Reflect.apply(timer.callback, undefined, timer.args);
      const after = this._queue.peek();
      if (after && after.id === timer.id) {
        this._queue.shift();
        timer.queued = false;
      }
      if (timer.interval !== undefined) {
        timer.runAt += timer.interval;
        this._queue.insert(timer);
      }
      timer = this._queue.peek();
    }
  }
  enable(options = {}) {
    const internalOptions = { ...options };
    if (this._isEnabled) {
      throw new ERR_INVALID_STATE("MockTimers is already enabled!");
    }
    if (Number.isNaN(internalOptions.now)) {
      throw new ERR_INVALID_ARG_VALUE("now", internalOptions.now, `epoch must be a positive integer received ${internalOptions.now}`);
    }
    internalOptions.now ||= 0;
    internalOptions.apis ||= SUPPORTED_APIS;
    validateStringArray(internalOptions.apis, "options.apis");
    for (const api of internalOptions.apis) {
      if (!SUPPORTED_APIS.includes(api)) {
        throw new ERR_INVALID_ARG_VALUE("options.apis", api, `option ${api} is not supported`);
      }
    }
    this._timersInContext = internalOptions.apis;
    if (this._isValidDateWithGetTime(internalOptions.now)) {
      this._now = internalOptions.now.getTime();
    } else if (validateNumber(internalOptions.now, "initialTime") === undefined) {
      this._assertTimeArg(internalOptions.now);
      this._now = internalOptions.now;
    }
    this._toggle(true);
  }
  setTime(time = kInitialEpoch) {
    validateNumber(time, "time");
    this._assertTimeArg(time);
    this._assertEnabled();
    this._now = time;
  }
  reset() {
    if (!this._isEnabled) return;
    this._toggle(false);
    this._timersInContext = [];
    this._now = kInitialEpoch;
    this._queue.clear();
  }
  runAll() {
    this._assertEnabled();
    const longest = this._queue.peekBottom();
    if (!longest) return;
    this.tick(longest.runAt - this._now);
  }
}

export { MockTracker, MockTimers };
export default { MockTracker, MockTimers };
"#;
