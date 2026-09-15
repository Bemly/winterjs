//! `node:internal/events/abort_listener`（Node 近逐字移植，MIT）。
//!
//! 适配：本仓 AbortSignal 极简（addEventListener 忽略 options，见 builtins/mod.rs），
//! `{ once: true, [kResistStopPropagation]: true }` 传入无效但无害（once 语义由
//! 调用方 cleanup 兜底）；`SymbolDispose` 缺 Symbol.dispose 时用注册表回退。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node:internal/events/abort_listener.
import errors from 'node:internal/errors';
import { validateAbortSignal, validateFunction } from 'node:internal/validators';
const {
  codes: { ERR_INVALID_ARG_TYPE: { HideStackFramesError: ERR_INVALID_ARG_TYPE } },
} = errors;

const SymbolDispose = Symbol.dispose ?? Symbol.for('Symbol.dispose');

function addAbortListener(signal, listener) {
  if (signal === undefined) {
    throw new ERR_INVALID_ARG_TYPE('signal', 'AbortSignal', signal);
  }
  validateAbortSignal(signal, 'signal');
  validateFunction(listener, 'listener');

  let removeEventListener;
  if (signal.aborted) {
    queueMicrotask(() => listener());
  } else {
    // 本仓 AbortSignal 忽略 options（极简实现）；once 语义由 dispose 清理兜底。
    signal.addEventListener('abort', listener, { once: true });
    __etAdd(signal, 'abort', listener);
    removeEventListener = () => {
      signal.removeEventListener('abort', listener);
      __etRemove(signal, 'abort', listener);
    };
  }
  return {
    __proto__: null,
    [SymbolDispose]() {
      removeEventListener?.();
    },
  };
}

// 原生 EventTarget 监听侧表（10f）：引擎 EventTarget 无 JS 可见监听表，
// `events.listenerCount(target)` 读此表。只记录经本模块/上层帮助函数挂载的
// 监听（用户直调 addEventListener 不可见，记档）。
const __etListeners = new WeakMap();
function __etAdd(target, type, listener) {
  let byType = __etListeners.get(target);
  if (byType === undefined) {
    byType = new Map();
    __etListeners.set(target, byType);
  }
  let set = byType.get(type);
  if (set === undefined) {
    set = new Set();
    byType.set(type, set);
  }
  set.add(listener);
}
function __etRemove(target, type, listener) {
  const byType = __etListeners.get(target);
  if (byType === undefined) return;
  const set = byType.get(type);
  if (set === undefined) return;
  set.delete(listener);
  if (set.size === 0) byType.delete(type);
}
function __etCount(target, type) {
  const byType = __etListeners.get(target);
  if (byType === undefined) return 0;
  const set = byType.get(type);
  return set === undefined ? 0 : set.size;
}

export { addAbortListener, __etAdd, __etRemove, __etCount };
export default { addAbortListener };
"#;
