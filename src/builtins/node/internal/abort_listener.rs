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
    removeEventListener = () => {
      signal.removeEventListener('abort', listener);
    };
  }
  return {
    __proto__: null,
    [SymbolDispose]() {
      removeEventListener?.();
    },
  };
}

export { addAbortListener };
export default { addAbortListener };
"#;
