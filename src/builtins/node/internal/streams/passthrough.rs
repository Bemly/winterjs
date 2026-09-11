//! `node:internal/streams/passthrough`（Node lib/internal/streams/passthrough.js 逐字内嵌，MIT）。
/// 来源：nodejs/node `internal/streams/passthrough.js`（MIT 头见源内）逐字内嵌；require → 垫片映射，
/// primordials → node:internal/primordials。偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"import { primordials } from 'node:internal/primordials';
import * as __m0 from 'node:internal/streams/transform';

// CJS require 垫片（静态 spec → 内建模块 default 导出；循环依赖经懒访问解环）
const require = (spec) => __requireMap(spec);
function __requireMap(spec) {
  switch (spec) {
    case 'internal/streams/transform': return __m0.default;
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

// a passthrough stream.
// basically just the most minimal sort of Transform stream.
// Every written chunk gets output as-is.

'use strict';

const {
  ObjectSetPrototypeOf,
} = primordials;

module.exports = PassThrough;

let Transform;
function __ensureTransform() { if (Transform === undefined) Transform = require('internal/streams/transform'); return Transform; }
function __wirePassThrough() { const T = __ensureTransform(); ObjectSetPrototypeOf(PassThrough.prototype, T.prototype); ObjectSetPrototypeOf(PassThrough, T); }

function PassThrough(options) {
  __wirePassThrough();
  if (!(this instanceof PassThrough))
    return new PassThrough(options);

  __ensureTransform().call(this, options);
}

PassThrough.prototype._transform = function(chunk, encoding, cb) {
  cb(null, chunk);
};

export default module.exports;
"#;
