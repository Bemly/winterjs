//! `node:internal/streams/state`（Node lib/internal/streams/state.js 逐字内嵌，MIT）。
/// 来源：nodejs/node `internal/streams/state.js`（MIT 头见源内）逐字内嵌；require → 垫片映射，
/// primordials → node:internal/primordials。偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"import { primordials } from 'node:internal/primordials';
import * as __m0 from 'node:internal/errors';
import * as __m1 from 'node:internal/validators';

// CJS require 垫片（静态 spec → 内建模块 default 导出；循环依赖经懒访问解环）
const require = (spec) => __requireMap(spec);
function __requireMap(spec) {
  switch (spec) {
    case 'internal/errors': return __m0.default;
    case 'internal/validators': return __m1.default;
  default: throw new Error('unmapped internal require: ' + spec);
  }
}

const module = { exports: { __proto__: null } };

'use strict';

const {
  MathFloor,
  NumberIsInteger,
} = primordials;
const { validateInteger } = require('internal/validators');

const { ERR_INVALID_ARG_VALUE } = require('internal/errors').codes;

// TODO (fix): For some reason Windows CI fails with bigger hwm.
let defaultHighWaterMarkBytes = process.platform === 'win32' ? 16 * 1024 : 64 * 1024;
let defaultHighWaterMarkObjectMode = 16;

function highWaterMarkFrom(options, isDuplex, duplexKey) {
  return options.highWaterMark != null ? options.highWaterMark :
    isDuplex ? options[duplexKey] : null;
}

function getDefaultHighWaterMark(objectMode) {
  return objectMode ? defaultHighWaterMarkObjectMode : defaultHighWaterMarkBytes;
}

function setDefaultHighWaterMark(objectMode, value) {
  validateInteger(value, 'value', 0);
  if (objectMode) {
    defaultHighWaterMarkObjectMode = value;
  } else {
    defaultHighWaterMarkBytes = value;
  }
}

function getHighWaterMark(state, options, duplexKey, isDuplex) {
  const hwm = highWaterMarkFrom(options, isDuplex, duplexKey);
  if (hwm != null) {
    if (!NumberIsInteger(hwm) || hwm < 0) {
      const name = isDuplex ? `options.${duplexKey}` : 'options.highWaterMark';
      throw new ERR_INVALID_ARG_VALUE(name, hwm);
    }
    return MathFloor(hwm);
  }

  // Default value
  return getDefaultHighWaterMark(state.objectMode);
}

module.exports = {
  getHighWaterMark,
  getDefaultHighWaterMark,
  setDefaultHighWaterMark,
};

export default module.exports;
"#;
