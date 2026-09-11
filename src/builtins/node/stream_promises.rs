//! `node:stream/promises`（Node lib/stream/promises.js 逐字内嵌，MIT）。
/// 来源：nodejs/node `stream/promises.js`（MIT 头见源内）逐字内嵌；require → 垫片映射，
/// primordials → node:internal/primordials。偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"import { primordials } from 'node:internal/primordials';
import * as __m0 from 'node:internal/streams/end_of_stream';
import * as __m1 from 'node:internal/streams/pipeline';
import * as __m2 from 'node:internal/streams/utils';

// CJS require 垫片（静态 spec → 内建模块 default 导出；循环依赖经懒访问解环）
const require = (spec) => __requireMap(spec);
function __requireMap(spec) {
  switch (spec) {
    case 'internal/streams/end-of-stream': return __m0.default;
    case 'internal/streams/pipeline': return __m1.default;
    case 'internal/streams/utils': return __m2.default;
  default: throw new Error('unmapped internal require: ' + spec);
  }
}

const module = { exports: { __proto__: null } };

'use strict';

const {
  ArrayPrototypePop,
  Promise,
} = primordials;

const {
  isIterable,
  isNodeStream,
  isWebStream,
} = require('internal/streams/utils');

const { pipelineImpl: pl } = require('internal/streams/pipeline');
const { finished } = require('internal/streams/end-of-stream');

// (bare require('stream') dropped: ESM graph membership not needed)

function pipeline(...streams) {
  return new Promise((resolve, reject) => {
    let signal;
    let end;
    const lastArg = streams[streams.length - 1];
    if (lastArg && typeof lastArg === 'object' &&
        !isNodeStream(lastArg) && !isIterable(lastArg) && !isWebStream(lastArg)) {
      const options = ArrayPrototypePop(streams);
      signal = options.signal;
      end = options.end;
    }

    pl(streams, (err, value) => {
      if (err) {
        reject(err);
      } else {
        resolve(value);
      }
    }, { signal, end });
  });
}

module.exports = {
  finished,
  pipeline,
};

export { pipeline, finished };
export default module.exports;
"#;
