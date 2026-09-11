//! `node:trace_events`（Node `lib/trace_events.js` 语义移植，MIT）。
//! 偏差：`CategorySet` 原生面无——enabled 类别集合维护在 JS 侧
//! （`getEnabledCategories` 返回所有已启用 Tracing 的类别并集）；
//! `customInspectSymbol` 用 `nodejs.util.inspect.custom`。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node lib/trace_events.js (JS-side category tracking; see module docs).
import errors from 'node:internal/errors';
import { validateObject, validateStringArray } from 'node:internal/validators';
import { format } from 'node:internal/util/inspect';

const {
  codes: {
    ERR_TRACE_EVENTS_CATEGORY_REQUIRED,
  },
} = errors;

const customInspectSymbol = Symbol.for('nodejs.util.inspect.custom');
const enabledTracingObjects = new Set();
const categoriesByObject = new WeakMap(); // 类外读 #categories 的桥
const kMaxTracingCount = 10;

class Tracing {
  #categories;
  #enabled = false;

  constructor(categories) {
    this.#categories = categories;
    categoriesByObject.set(this, categories);
  }

  enable() {
    if (!this.#enabled) {
      this.#enabled = true;
      enabledTracingObjects.add(this);
      if (enabledTracingObjects.size > kMaxTracingCount) {
        process.emitWarning(
          'Possible trace_events memory leak detected. There are more than ' +
          `${kMaxTracingCount} enabled Tracing objects.`,
        );
      }
    }
  }

  disable() {
    if (this.#enabled) {
      this.#enabled = false;
      enabledTracingObjects.delete(this);
    }
  }

  get enabled() {
    return this.#enabled;
  }

  get categories() {
    return this.#categories.join(',');
  }

  [customInspectSymbol](depth, opts) {
    if (typeof depth === 'number' && depth < 0) return this;
    const obj = { enabled: this.enabled, categories: this.categories };
    return `Tracing ${format(obj)}`;
  }
}

function createTracing(options) {
  validateObject(options, 'options');
  validateStringArray(options.categories, 'options.categories');

  if (options.categories.length <= 0)
    throw new ERR_TRACE_EVENTS_CATEGORY_REQUIRED();

  return new Tracing(options.categories);
}

function getEnabledCategories() {
  const all = new Set();
  for (const tracing of enabledTracingObjects) {
    for (const c of categoriesByObject.get(tracing)) all.add(c);
  }
  return [...all].join(',');
}

export { createTracing, getEnabledCategories };
export default { createTracing, getEnabledCategories };
"#;
