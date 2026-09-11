//! `node:internal/streams/lazy_transform`（Node lib/internal/streams/lazy_transform.js 逐字内嵌，MIT）。
/// 来源：nodejs/node `internal/streams/lazy_transform.js`（MIT 头见源内）逐字内嵌；require → 垫片映射，
/// primordials → node:internal/primordials。偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"import { primordials } from 'node:internal/primordials';
import * as __m0 from 'node:stream';

// CJS require 垫片（静态 spec → 内建模块 default 导出；循环依赖经懒访问解环）
const require = (spec) => __requireMap(spec);
function __requireMap(spec) {
  switch (spec) {
    case 'stream': return __m0.default;
  default: throw new Error('unmapped internal require: ' + spec);
  }
}

const module = { exports: { __proto__: null } };

// LazyTransform is a special type of Transform stream that is lazily loaded.
// This is used for performance with bi-API-ship: when two APIs are available
// for the stream, one conventional and one non-conventional.
'use strict';

const {
  ObjectDefineProperties,
  ObjectDefineProperty,
  ObjectSetPrototypeOf,
} = primordials;

const stream = require('stream');

module.exports = LazyTransform;

function LazyTransform(options) {
  this._options = options;
}
ObjectSetPrototypeOf(LazyTransform.prototype, stream.Transform.prototype);
ObjectSetPrototypeOf(LazyTransform, stream.Transform);

function makeGetter(name) {
  return function() {
    stream.Transform.call(this, this._options);
    this._writableState.decodeStrings = false;
    return this[name];
  };
}

function makeSetter(name) {
  return function(val) {
    ObjectDefineProperty(this, name, {
      __proto__: null,
      value: val,
      enumerable: true,
      configurable: true,
      writable: true,
    });
  };
}

ObjectDefineProperties(LazyTransform.prototype, {
  _readableState: {
    __proto__: null,
    get: makeGetter('_readableState'),
    set: makeSetter('_readableState'),
    configurable: true,
    enumerable: true,
  },
  _writableState: {
    __proto__: null,
    get: makeGetter('_writableState'),
    set: makeSetter('_writableState'),
    configurable: true,
    enumerable: true,
  },
});

export default module.exports;
"#;
