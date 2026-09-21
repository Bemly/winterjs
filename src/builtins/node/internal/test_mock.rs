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
import { codes } from "node:internal/errors";
import validators from "node:internal/validators";

const { validateBoolean, validateFunction, validateInteger, validateObject } = validators;
const { ERR_INVALID_ARG_TYPE, ERR_INVALID_ARG_VALUE } = codes;

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

export { MockTracker };
export default { MockTracker };
"#;
