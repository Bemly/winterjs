//! `node:timers/promises`（Node lib/timers/promises.js 面，MIT；自写实现）。
//!
//! 忠实面：setTimeout/setImmediate/setInterval（Promise 形态，value 透传，
//! options.signal 中止 → AbortError）+ scheduler.yield/wait。setImmediate 以
//! setTimeout(0) 底座近似（本仓无 macrotask 分层，记档）；delay 类型校验宽松
//! （Node 口径：非数字回退默认，记档）。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
import errors from 'node:internal/errors';
const {
  AbortError,
  codes: {
    ERR_INVALID_ARG_TYPE: { HideStackFramesError: ERR_INVALID_ARG_TYPE },
  },
} = errors;

const kResistStopPropagation = Symbol.for('resistStopPropagation');

function validateAbortSignal(signal, name) {
  if (signal !== undefined &&
      (signal === null || typeof signal !== 'object' || !('aborted' in signal))) {
    throw new ERR_INVALID_ARG_TYPE(name, 'AbortSignal', signal);
  }
}

function checkOptions(options) {
  validateAbortSignal(options?.signal, 'options.signal');
  return options;
}

function normalizeDelay(after) {
  return (typeof after === 'number' && after >= 0) || (typeof after === 'bigint' && after >= 0)
    ? Number(after) : 0;
}

async function timersSetTimeout(after = 1, value, options = {}) {
  checkOptions(options);
  const signal = options.signal;
  if (signal?.aborted) {
    // 已中止路径同步拒绝（原移植稿的 primordials `PromiseReject` 未随行——
    // ReferenceError 反被 assert.rejects 吃成 mismatch，10f 现形）。
    return Promise.reject(new AbortError(undefined, { cause: signal.reason }));
  }
  const delay = normalizeDelay(after);
  let timerId, resolve_, reject_;
  const promise = new Promise((resolve, reject) => {
    resolve_ = resolve;
    reject_ = reject;
    timerId = setTimeout(() => resolve_(value), delay);
  });
  if (signal) {
    const onAbort = () => {
      clearTimeout(timerId);
      reject_(new AbortError(undefined, { cause: signal.reason }));
    };
    signal.addEventListener('abort', onAbort, { once: true, [kResistStopPropagation]: true });
  }
  return promise;
}

async function timersSetImmediate(value, options = {}) {
  checkOptions(options);
  // 本仓无 macrotask 分层：setImmediate ≈ setTimeout(0)（记档）
  return timersSetTimeout(0, value, options);
}

async function* intervalsSetInterval(delay, value, options) {
  checkOptions(options ?? {});
  let timerId = undefined;
  const signal = options?.signal;
  if (signal?.aborted) {
    throw new AbortError(undefined, { cause: signal.reason });
  }
  const onAbort = () => {
    if (timerId !== undefined) clearInterval(timerId);
    timerId = undefined;
  };
  signal?.addEventListener('abort', onAbort, { once: true, [kResistStopPropagation]: true });
  try {
    while (!signal?.aborted) {
      const fired = new Promise((resolve, reject) => {
        timerId = setTimeout(resolve, delay);
      });
      try {
        await fired;
      } catch (err) {
        if (err instanceof Error && err.name === 'AbortError') return;
        throw err;
      }
      if (signal?.aborted) return;
      yield value;
    }
  } finally {
    if (timerId !== undefined) clearInterval(timerId);
    signal?.removeEventListener('abort', onAbort);
  }
}

function setInterval(delay = 1, value, options = {}) {
  return intervalsSetInterval(delay, value, options);
}

class Scheduler {
  constructor() {
    throw new errors.codes.ERR_ILLEGAL_CONSTRUCTOR();
  }
  yield(value) {
    __validateSchedulerThis(this);
    return timersSetImmediate(value);
  }
  wait(delay, options) {
    __validateSchedulerThis(this);
    return timersSetTimeout(delay ?? 0, undefined, options);
  }
}
function __validateSchedulerThis(self) {
  if (!(self instanceof Scheduler)) {
    throw new errors.codes.ERR_INVALID_THIS('Scheduler');
  }
}
// 单例绕过构造器（node 同款：不可 new，但方法保留 Scheduler 牌 this 校验）。
const scheduler = Object.create(Scheduler.prototype, {
  [Symbol.toStringTag]: { value: 'Scheduler' },
});

export {
  timersSetTimeout as setTimeout,
  timersSetImmediate as setImmediate,
  setInterval,
  scheduler,
};
const __api = {
  setTimeout: timersSetTimeout,
  setImmediate: timersSetImmediate,
  setInterval,
  scheduler,
};
export default __api;
"#;
