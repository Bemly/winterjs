//! `node:stream`（Node lib/stream.js 逐字内嵌，MIT）。
/// 来源：nodejs/node `stream.js`（MIT 头见源内）逐字内嵌；require → 垫片映射，
/// primordials → node:internal/primordials。偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"import { primordials } from 'node:internal/primordials';
import * as __m0 from 'node:internal/streams/duplex';
import * as __m1 from 'node:internal/buffer';
import * as __m2 from 'node:internal/errors';
import * as __m3 from 'node:internal/streams/add_abort_signal';
import * as __m4 from 'node:internal/streams/compose';
import * as __m5 from 'node:internal/streams/destroy';
import * as __m6 from 'node:internal/streams/duplexpair';
import * as __m7 from 'node:internal/streams/end_of_stream';
import * as __m8 from 'node:internal/streams/legacy';
import * as __m9 from 'node:internal/streams/operators';
import * as __m10 from 'node:internal/streams/passthrough';
import * as __m11 from 'node:internal/streams/pipeline';
import * as __m12 from 'node:internal/streams/readable';
import * as __m13 from 'node:internal/streams/state';
import * as __m14 from 'node:internal/streams/transform';
import * as __m15 from 'node:internal/streams/utils';
import * as __m16 from 'node:internal/streams/writable';
import * as __m17 from 'node:internal/util';
import * as __m18 from 'node:internal/util/types';
import * as __m19 from 'node:stream/promises';
import * as __m20 from 'node:internal/streams/duplexify';

// CJS require 垫片（静态 spec → 内建模块 default 导出；循环依赖经懒访问解环）
const require = (spec) => __requireMap(spec);
function __requireMap(spec) {
  switch (spec) {
    case 'internal/streams/duplex': return __m0.default;
    case 'internal/buffer': return __m1.default;
    case 'internal/errors': return __m2.default;
    case 'internal/streams/add-abort-signal': return __m3.default;
    case 'internal/streams/compose': return __m4.default;
    case 'internal/streams/destroy': return __m5.default;
    case 'internal/streams/duplexpair': return __m6.default;
    case 'internal/streams/end-of-stream': return __m7.default;
    case 'internal/streams/legacy': return __m8.default;
    case 'internal/streams/operators': return __m9.default;
    case 'internal/streams/passthrough': return __m10.default;
    case 'internal/streams/pipeline': return __m11.default;
    case 'internal/streams/readable': return __m12.default;
    case 'internal/streams/state': return __m13.default;
    case 'internal/streams/transform': return __m14.default;
    case 'internal/streams/utils': return __m15.default;
    case 'internal/streams/writable': return __m16.default;
    case 'internal/util': return __m17.default;
    case 'internal/util/types': return __m18.default;
    case 'stream/promises': return __m19.default;
    case 'internal/streams/duplexify': return __m20.default;
  default: throw new Error('unmapped internal require: ' + spec);
  }
}

const module = { exports: { __proto__: null } };

// Copyright Joyent, Inc. and other Node contributors.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to permit
// persons to whom the Software is furnished to do so, subject to the
// following conditions:
//
// The above copyright notice and this permission notice shall be included
// in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN
// NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
// DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
// OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
// USE OR OTHER DEALINGS IN THE SOFTWARE.

'use strict';

const {
  ObjectDefineProperty,
  ObjectKeys,
  ReflectApply,
} = primordials;

const {
  promisify: { custom: customPromisify },
} = require('internal/util');

const {
  streamReturningOperators,
  promiseReturningOperators,
} = require('internal/streams/operators');

const {
  codes: {
    ERR_ILLEGAL_CONSTRUCTOR,
  },
} = require('internal/errors');
const compose = require('internal/streams/compose');
const { setDefaultHighWaterMark, getDefaultHighWaterMark } = require('internal/streams/state');
const { pipeline } = require('internal/streams/pipeline');
const { destroyer } = require('internal/streams/destroy');
const { eos } = require('internal/streams/end-of-stream');
const internalBuffer = require('internal/buffer');

const promises = require('stream/promises');
const utils = require('internal/streams/utils');
const { isArrayBufferView, isUint8Array } = require('internal/util/types');

const Stream = module.exports = require('internal/streams/legacy').Stream;

Stream.isDestroyed = utils.isDestroyed;
Stream.isDisturbed = utils.isDisturbed;
Stream.isErrored = utils.isErrored;
Stream.isReadable = utils.isReadable;
Stream.isWritable = utils.isWritable;

Stream.Readable = require('internal/streams/readable');
const streamKeys = ObjectKeys(streamReturningOperators);
for (let i = 0; i < streamKeys.length; i++) {
  const key = streamKeys[i];
  const op = streamReturningOperators[key];
  function fn(...args) {
    if (new.target) {
      throw new ERR_ILLEGAL_CONSTRUCTOR();
    }
    return Stream.Readable.from(ReflectApply(op, this, args));
  }
  ObjectDefineProperty(fn, 'name', { __proto__: null, value: op.name });
  ObjectDefineProperty(fn, 'length', { __proto__: null, value: op.length });
  ObjectDefineProperty(Stream.Readable.prototype, key, {
    __proto__: null,
    value: fn,
    enumerable: false,
    configurable: true,
    writable: true,
  });
}
const promiseKeys = ObjectKeys(promiseReturningOperators);
for (let i = 0; i < promiseKeys.length; i++) {
  const key = promiseKeys[i];
  const op = promiseReturningOperators[key];
  function fn(...args) {
    if (new.target) {
      throw new ERR_ILLEGAL_CONSTRUCTOR();
    }
    return ReflectApply(op, this, args);
  }
  ObjectDefineProperty(fn, 'name', { __proto__: null, value: op.name });
  ObjectDefineProperty(fn, 'length', { __proto__: null, value: op.length });
  ObjectDefineProperty(Stream.Readable.prototype, key, {
    __proto__: null,
    value: fn,
    enumerable: false,
    configurable: true,
    writable: true,
  });
}
Stream.Writable = require('internal/streams/writable');
Stream.Duplex = require('internal/streams/duplex');
Stream.Transform = require('internal/streams/transform');
Stream.PassThrough = require('internal/streams/passthrough');
Stream.duplexPair = require('internal/streams/duplexpair');
Stream.pipeline = pipeline;
const { addAbortSignal } = require('internal/streams/add-abort-signal');
Stream.addAbortSignal = addAbortSignal;
Stream.finished = eos;
Stream.destroy = destroyer;
Stream.compose = compose;
Stream.setDefaultHighWaterMark = setDefaultHighWaterMark;
Stream.getDefaultHighWaterMark = getDefaultHighWaterMark;

ObjectDefineProperty(Stream, 'promises', {
  __proto__: null,
  configurable: true,
  enumerable: true,
  get() {
    return promises;
  },
});

ObjectDefineProperty(pipeline, customPromisify, {
  __proto__: null,
  enumerable: true,
  get() {
    return promises.pipeline;
  },
});

ObjectDefineProperty(eos, customPromisify, {
  __proto__: null,
  enumerable: true,
  get() {
    return promises.finished;
  },
});

// Backwards-compat with node 0.4.x
Stream.Stream = Stream;

Stream._isArrayBufferView = isArrayBufferView;
Stream._isUint8Array = isUint8Array;
Stream._uint8ArrayToBuffer = function _uint8ArrayToBuffer(chunk) {
  return new internalBuffer.FastBuffer(chunk.buffer,
                                       chunk.byteOffset,
                                       chunk.byteLength);
};

// duplexify 与 duplex 是真类继承环（duplexify 顶层 `extends Duplex`），CJS 侧
// 靠"from 首调时才 require"避开；ESM 静态图里在此（全家桶求值完成后）拉进
// 图求值 + 自注册 registry，此后 Duplex.from 运行时经 registry 命中。
require('internal/streams/duplexify');


export { pipeline, compose, addAbortSignal, setDefaultHighWaterMark, getDefaultHighWaterMark, promises };
export { eos as finished, destroyer as destroy };
export const Readable = module.exports.Readable;
export const Writable = module.exports.Writable;
export const Duplex = module.exports.Duplex;
export const Transform = module.exports.Transform;
export const PassThrough = module.exports.PassThrough;
export const duplexPair = module.exports.duplexPair;
export default module.exports;
"#;
