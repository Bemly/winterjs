//! `node:internal/streams/add_abort_signal`（Node lib/internal/streams/add-abort-signal.js 逐字内嵌，MIT）。
/// 来源：nodejs/node `internal/streams/add-abort-signal.js`（MIT 头见源内）逐字内嵌；require → 垫片映射，
/// primordials → node:internal/primordials。偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"import { primordials } from 'node:internal/primordials';
import * as __m0 from 'node:internal/errors';
import * as __m1 from 'node:internal/events/abort_listener';
import * as __m2 from 'node:internal/streams/end_of_stream';
import * as __m3 from 'node:internal/streams/utils';

// CJS require 垫片（静态 spec → 内建模块 default 导出；循环依赖经懒访问解环）
const require = (spec) => __requireMap(spec);
function __requireMap(spec) {
  switch (spec) {
    case 'internal/errors': return __m0.default;
    case 'internal/events/abort_listener': return __m1.default;
    case 'internal/streams/end-of-stream': return __m2.default;
    case 'internal/streams/utils': return __m3.default;
  default: throw new Error('unmapped internal require: ' + spec);
  }
}

const module = { exports: { __proto__: null } };

'use strict';

const {
  SymbolDispose,
} = primordials;

const {
  AbortError,
  codes: {
    ERR_INVALID_ARG_TYPE,
  },
} = require('internal/errors');

const {
  isNodeStream,
  isWebStream,
  kControllerErrorFunction,
} = require('internal/streams/utils');

const { eos } = require('internal/streams/end-of-stream');
let addAbortListener;

// This method is inlined here for readable-stream
// It also does not allow for signal to not exist on the stream
// https://github.com/nodejs/node/pull/36061#discussion_r533718029
const validateAbortSignal = (signal, name) => {
  if (typeof signal !== 'object' ||
       !('aborted' in signal)) {
    throw new ERR_INVALID_ARG_TYPE(name, 'AbortSignal', signal);
  }
};

module.exports.addAbortSignal = function addAbortSignal(signal, stream) {
  validateAbortSignal(signal, 'signal');
  if (!isNodeStream(stream) && !isWebStream(stream)) {
    throw new ERR_INVALID_ARG_TYPE('stream', ['ReadableStream', 'WritableStream', 'Stream'], stream);
  }
  return module.exports.addAbortSignalNoValidate(signal, stream);
};

module.exports.addAbortSignalNoValidate = function(signal, stream) {
  if (typeof signal !== 'object' || !('aborted' in signal)) {
    return stream;
  }
  const onAbort = isNodeStream(stream) ?
    () => {
      stream.destroy(new AbortError(undefined, { cause: signal.reason }));
    } :
    () => {
      stream[kControllerErrorFunction](new AbortError(undefined, { cause: signal.reason }));
    };
  if (signal.aborted) {
    onAbort();
  } else {
    addAbortListener ??= require('internal/events/abort_listener').addAbortListener;
    const disposable = addAbortListener(signal, onAbort);
    eos(stream, disposable[SymbolDispose]);
  }
  return stream;
};

export default module.exports;
"#;
