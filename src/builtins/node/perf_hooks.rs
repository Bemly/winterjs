//! `node:perf_hooks`：性能计时面（纯 JS，零 native，9e-3）。
//! 时基：`__wjs_hrtime_ns`（单调纳秒，`process.hrtime` 同源）；`timeOrigin` 在
//! 模块求值时固化（每进程一次，`run_isolated` 语义下等价进程启动）。
//! 偏差记档：
//! - `monitorEventLoopDelay` 为简化采样器（`setInterval` 漂移法，无 C++ 直方图；
//!   统计量 min/max/mean/stddev/percentile/exceeds 口径一致，绝对值仅供参考）。
//! - `eventLoopUtilization` 未接事件循环内外计时：恒 `{ idle: 0, active: 累计,
//!   utilization: 1 }`（delta 形态正常计算差值）；`nodeTiming` 为形状桩（数值 0）。
//! - `PerformanceResourceTiming` 仅形状类（无资源加载埋点，不产生条目）。
//! - 全局 `performance` 在本模块求值期安装（vite 等顶层 import 即有；
//!   未 import 的会话徒手引用为 undefined，与 Node 全集口径的偏差）。

/// 内嵌 ESM 源（`node:perf_hooks`）。
pub const SOURCE: &str = r#"
const __t0 = BigInt(__wjs_hrtime_ns());
function __nowMs() {
  return Number(BigInt(__wjs_hrtime_ns()) - __t0) / 1e6;
}
const __origin = Date.now() - __nowMs();

class PerformanceEntry {
  constructor(name, entryType, startTime, duration, detail) {
    this.name = String(name);
    this.entryType = String(entryType);
    this.startTime = Number(startTime);
    this.duration = Number(duration);
    this.detail = detail ?? null;
  }
  toJSON() {
    return { name: this.name, entryType: this.entryType, startTime: this.startTime, duration: this.duration, detail: this.detail };
  }
}
class PerformanceMark extends PerformanceEntry {
  constructor(markName, options) {
    super(markName, "mark", __nowMs(), 0, options?.detail ?? null);
  }
}
class PerformanceMeasure extends PerformanceEntry {
  constructor(name, startTime, duration, detail) {
    super(name, "measure", startTime, duration, detail);
  }
}
class PerformanceResourceTiming extends PerformanceEntry {
  constructor(name, startTime, duration, detail) {
    super(name, "resource", startTime, duration, detail);
  }
}

const __entries = [];
const __observers = new Set();
let __flushQueued = false;
function __notify(type) {
  if (__flushQueued) return;
  __flushQueued = true;
  queueMicrotask(() => {
    __flushQueued = false;
    for (const o of [...__observers]) o.__dispatch();
  });
}

class PerformanceObserverEntryList {
  constructor(entries) {
    this.__entries = entries;
  }
  getEntries() { return [...this.__entries]; }
  getEntriesByName(name, type) {
    return this.__entries.filter((e) => e.name === name && (type === undefined || e.entryType === type));
  }
  getEntriesByType(type) {
    return this.__entries.filter((e) => e.entryType === type);
  }
}

class PerformanceObserver {
  constructor(callback) {
    if (typeof callback !== "function") {
      const err = new TypeError("PerformanceObserver requires a callback");
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    this.__cb = callback;
    this.__types = null;
    this.__buffered = false;
    this.__taken = [];
    this.__last = -1;
  }
  static get supportedEntryTypes() {
    return ["mark", "measure", "function"];
  }
  observe(options) {
    const types = options?.entryTypes ?? (options?.type ? [options.type] : null);
    if (!types) {
      const err = new TypeError("observe needs entryTypes");
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    this.__types = [...types];
    this.__buffered = !!options?.buffered;
    __observers.add(this);
    if (this.__buffered) {
      this.__last = -1;
      this.__dispatch();
    } else {
      // 非 buffered：只收 observe 之后的新条目（Node 同款）
      this.__last = __entries.length - 1;
    }
  }
  disconnect() {
    __observers.delete(this);
  }
  takeRecords() {
    const out = this.__taken;
    this.__taken = [];
    return out;
  }
  __dispatch() {
    if (!__observers.has(this)) return;
    const last = this.__last ?? -1;
    for (let i = last + 1; i < __entries.length; i++) {
      if (this.__types.includes(__entries[i].entryType)) this.__taken.push(__entries[i]);
    }
    this.__last = __entries.length - 1;
    this.__drain();
  }
  __drain() {
    if (this.__taken.length === 0) return;
    const list = new PerformanceObserverEntryList(this.__taken);
    this.__taken = [];
    this.__cb(list, this);
  }
}

function __record(entry) {
  __entries.push(entry);
  __notify(entry.entryType);
  return entry;
}

const __performanceMethods = {
  now: () => __nowMs(),
  get timeOrigin() { return __origin; },
  toJSON() {
    return { timeOrigin: __origin, nodeTiming: this.nodeTiming.toJSON() };
  },
  mark(markName, options) {
    return __record(new PerformanceMark(markName, options));
  },
  measure(name, startOrOptions, endMark) {
    let start = 0;
    let detail = null;
    if (typeof startOrOptions === "object" && startOrOptions !== null) {
      detail = startOrOptions.detail ?? null;
      start = startOrOptions.start ?? 0;
      const end = startOrOptions.end ?? __nowMs();
      const duration = startOrOptions.duration ?? (end - start);
      return __record(new PerformanceMeasure(name, start, duration, detail));
    }
    if (typeof startOrOptions === "string") {
      const found = __entries.filter((e) => e.name === startOrOptions).pop();
      start = found ? found.startTime : 0;
    } else if (typeof startOrOptions === "number") {
      start = startOrOptions;
    }
    let end = __nowMs();
    if (typeof endMark === "string") {
      const found = __entries.filter((e) => e.name === endMark).pop();
      end = found ? found.startTime : end;
    }
    return __record(new PerformanceMeasure(name, start, end - start, detail));
  },
  getEntries() { return [...__entries]; },
  getEntriesByName(name, type) {
    return __entries.filter((e) => e.name === name && (type === undefined || e.entryType === type));
  },
  getEntriesByType(type) {
    return __entries.filter((e) => e.entryType === type);
  },
  clearMarks(name) {
    for (let i = __entries.length - 1; i >= 0; i--) {
      if (__entries[i].entryType === "mark" && (name === undefined || __entries[i].name === name)) {
        __entries.splice(i, 1);
      }
    }
  },
  clearMeasures(name) {
    for (let i = __entries.length - 1; i >= 0; i--) {
      if (__entries[i].entryType === "measure" && (name === undefined || __entries[i].name === name)) {
        __entries.splice(i, 1);
      }
    }
  },
  clearResourceTimings() {
    for (let i = __entries.length - 1; i >= 0; i--) {
      if (__entries[i].entryType === "resource") __entries.splice(i, 1);
    }
  },
  eventLoopUtilization(util1, util2) {
    const active = __nowMs();
    if (util1 !== undefined && util2 !== undefined) {
      const idle = (util2.idle ?? 0) - (util1.idle ?? 0);
      const act = (util2.active ?? 0) - (util1.active ?? 0);
      return { idle, active: act, utilization: act <= 0 ? 0 : (act - idle) / act };
    }
    return { idle: 0, active, utilization: 1 };
  },
  get nodeTiming() {
    return {
      name: "node", entryType: "node", startTime: 0, duration: __nowMs(),
      nodeStart: 0, v8Start: 0, bootstrapComplete: 0, environment: 0,
      loopStart: 0, loopExit: -1, idleTime: 0,
      toJSON() {
        return { ...this };
      },
    };
  },
};
class Performance {}
const performance = __performanceMethods;
Object.setPrototypeOf(performance, Performance.prototype);
// Node 全局 performance（vite loadEnv 等徒手引用，不 import 本模块也能拿到——
// 本模块求值期装全局；偏差：未 import 过 perf_hooks 的会话无此全局，记档 M5）。
globalThis.performance = performance;

function timerify(fn, options) {
  if (typeof fn !== "function") {
    const err = new TypeError("timerify needs a function");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const name = typeof options === "string" ? options : (options?.histogram?.name ?? fn.name);
  function wrapped(...args) {
    const start = __nowMs();
    try {
      return fn.apply(this, args);
    } finally {
      __record(new PerformanceEntry(name || "anonymous", "function", start, __nowMs() - start, null));
    }
  }
  Object.defineProperty(wrapped, "name", { value: fn.name, configurable: true });
  return wrapped;
}

class Histogram {
  constructor() {
    this.__values = [];
    this.__exceeds = 0;
  }
  get count() { return this.__values.length; }
  get countBigInt() { return BigInt(this.__values.length); }
  // 空表哨兵（真机口径）：min=INT64_MAX，max=0，mean/stddev=NaN
  get min() { return this.__values.length === 0 ? 9223372036854776000 : Math.min(...this.__values); }
  get max() { return this.__values.length === 0 ? 0 : Math.max(...this.__values); }
  get mean() {
    if (this.__values.length === 0) return NaN;
    return this.__values.reduce((a, b) => a + b, 0) / this.__values.length;
  }
  get stddev() {
    if (this.__values.length === 0) return NaN;
    const m = this.mean;
    return Math.sqrt(this.__values.reduce((a, b) => a + (b - m) ** 2, 0) / this.__values.length);
  }
  get exceeds() { return this.__exceeds; }
  get minBigInt() { return this.__values.length === 0 ? null : BigInt(Math.floor(this.min)); }
  get maxBigInt() { return this.__values.length === 0 ? null : BigInt(Math.floor(this.max)); }
  get meanBigInt() { return this.__values.length === 0 ? null : BigInt(Math.floor(this.mean)); }
  get stddevBigInt() { return this.__values.length === 0 ? null : BigInt(Math.floor(this.stddev)); }
  get exceedsBigInt() { return BigInt(this.__exceeds); }
  record(val) {
    const v = Number(val);
    if (!Number.isFinite(v)) {
      const err = new TypeError("histogram value must be finite");
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    this.__values.push(v);
  }
  recordDelta() {}
  add(other) {
    for (const v of other.__values) this.__values.push(v);
  }
  reset() {
    this.__values = [];
    this.__exceeds = 0;
  }
  percentile(p) {
    if (this.__values.length === 0) return 0;
    const sorted = [...this.__values].sort((a, b) => a - b);
    const rank = Math.ceil((Number(p) / 100) * sorted.length);
    return sorted[Math.min(Math.max(rank - 1, 0), sorted.length - 1)];
  }
  percentileBigInt(p) {
    const v = this.percentile(p);
    return Number.isNaN(v) ? null : BigInt(Math.floor(v));
  }
  get percentiles() {
    const out = new Map();
    for (const p of [1, 25, 50, 75, 99]) out.set(p, this.percentile(p));
    return out;
  }
  get percentilesBigInt() {
    const out = new Map();
    for (const [k, v] of this.percentiles) out.set(k, Number.isNaN(v) ? null : BigInt(Math.floor(v)));
    return out;
  }
}
function createHistogram(options) {
  return new Histogram();
}
function importHistogram(record) {
  const h = new Histogram();
  if (record && typeof record === "object") {
    for (const k of ["min", "max", "mean", "stddev"]) {
      if (record[k] !== undefined) h[`__${k}`] = Number(record[k]);
    }
  }
  return h;
}

class IntervalHistogram extends Histogram {
  constructor(options) {
    super();
    this.__resolution = options?.resolution ?? 10;
    this.__timer = null;
    this.__expected = 0;
  }
  enable() {
    if (this.__timer !== null) return true;
    this.__expected = Date.now() + this.__resolution;
    this.__timer = setInterval(() => {
      const now = Date.now();
      const drift = Math.max(0, now - this.__expected);
      this.__expected = now + this.__resolution;
      this.record(drift);
      if (drift > this.__resolution) this.__exceeds++;
    }, this.__resolution);
    if (typeof this.__timer === "object" && this.__timer.unref) this.__timer.unref();
    return true;
  }
  disable() {
    if (this.__timer === null) return false;
    clearInterval(this.__timer);
    this.__timer = null;
    return true;
  }
}
function monitorEventLoopDelay(options) {
  return new IntervalHistogram(options);
}
function eventLoopUtilization(...args) {
  return performance.eventLoopUtilization(...args);
}

const __api = {
  Performance, PerformanceEntry, PerformanceMark, PerformanceMeasure,
  PerformanceObserver, PerformanceObserverEntryList, PerformanceResourceTiming,
  monitorEventLoopDelay, eventLoopUtilization, timerify,
  createHistogram, importHistogram, performance,
  constants: {
    NODE_PERFORMANCE_GC_MAJOR: 4, NODE_PERFORMANCE_GC_MINOR: 1,
    NODE_PERFORMANCE_GC_MINOR_MARK_SWEEP: 2, NODE_PERFORMANCE_GC_INCREMENTAL: 8,
    NODE_PERFORMANCE_GC_WEAKCB: 16, NODE_PERFORMANCE_GC_FLAGS_NO: 0,
    NODE_PERFORMANCE_GC_FLAGS_CONSTRUCT_RETAINED: 2,
    NODE_PERFORMANCE_GC_FLAGS_FORCED: 4,
    NODE_PERFORMANCE_GC_FLAGS_SYNCHRONOUS_PHANTOM_PROCESSING: 8,
    NODE_PERFORMANCE_GC_FLAGS_ALL_AVAILABLE_GARBAGE: 16,
    NODE_PERFORMANCE_GC_FLAGS_ALL_EXTERNAL_MEMORY: 32,
    NODE_PERFORMANCE_GC_FLAGS_SCHEDULE_IDLE: 64,
  },
};
export default __api;
export {
  Performance, PerformanceEntry, PerformanceMark, PerformanceMeasure,
  PerformanceObserver, PerformanceObserverEntryList, PerformanceResourceTiming,
  monitorEventLoopDelay, eventLoopUtilization, timerify,
  createHistogram, importHistogram, performance,
};
export const constants = {
  NODE_PERFORMANCE_GC_MAJOR: 4, NODE_PERFORMANCE_GC_MINOR: 1,
  NODE_PERFORMANCE_GC_MINOR_MARK_SWEEP: 2, NODE_PERFORMANCE_GC_INCREMENTAL: 8,
  NODE_PERFORMANCE_GC_WEAKCB: 16, NODE_PERFORMANCE_GC_FLAGS_NO: 0,
  NODE_PERFORMANCE_GC_FLAGS_CONSTRUCT_RETAINED: 2,
  NODE_PERFORMANCE_GC_FLAGS_FORCED: 4,
  NODE_PERFORMANCE_GC_FLAGS_SYNCHRONOUS_PHANTOM_PROCESSING: 8,
  NODE_PERFORMANCE_GC_FLAGS_ALL_AVAILABLE_GARBAGE: 16,
  NODE_PERFORMANCE_GC_FLAGS_ALL_EXTERNAL_MEMORY: 32,
  NODE_PERFORMANCE_GC_FLAGS_SCHEDULE_IDLE: 64,
};
"#;
