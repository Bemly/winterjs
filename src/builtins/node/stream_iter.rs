//! `node:stream/iter`（Node lib/stream/iter.js 逐字内嵌，MIT）。
/// 来源：nodejs/node `lib/stream/iter.js`（MIT 头见源内）逐字内嵌；require → 垫片映射，
/// primordials → node:internal/primordials。偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"import { primordials } from 'node:internal/primordials';
import * as __m0 from 'node:internal/util';
import * as __m1 from 'node:internal/streams/iter_types';
import * as __m2 from 'node:internal/streams/iter_push';
import * as __m3 from 'node:internal/streams/iter_duplex';
import * as __m4 from 'node:internal/streams/iter_from';
import * as __m5 from 'node:internal/streams/iter_pull';
import * as __m6 from 'node:internal/streams/iter_consumers';
import * as __m7 from 'node:internal/streams/iter_classic';
import * as __m8 from 'node:internal/streams/iter_broadcast';
import * as __m9 from 'node:internal/streams/iter_share';

// CJS require 垫片（静态 spec → 内建模块 default 导出；循环依赖经懒访问解环）
const require = (spec) => __requireMap(spec);
function __requireMap(spec) {
  switch (spec) {
    case 'internal/util': return __m0.default;
    case 'internal/streams/iter/types': return __m1.default;
    case 'internal/streams/iter/push': return __m2.default;
    case 'internal/streams/iter/duplex': return __m3.default;
    case 'internal/streams/iter/from': return __m4.default;
    case 'internal/streams/iter/pull': return __m5.default;
    case 'internal/streams/iter/consumers': return __m6.default;
    case 'internal/streams/iter/classic': return __m7.default;
    case 'internal/streams/iter/broadcast': return __m8.default;
    case 'internal/streams/iter/share': return __m9.default;
  default: throw new Error('unmapped internal require: ' + spec);
  }
}

const module = { exports: { __proto__: null } };

'use strict';

// Public entry point for the iterable streams API.
// Usage: require('stream/iter') or require('node:stream/iter')
// Requires: --experimental-stream-iter

const {
  ObjectFreeze,
} = primordials;

const { emitExperimentalWarning } = require('internal/util');
emitExperimentalWarning('stream/iter');

// Protocol symbols
const {
  toStreamable,
  toAsyncStreamable,
  broadcastProtocol,
  shareProtocol,
  shareSyncProtocol,
  drainableProtocol,
} = require('internal/streams/iter/types');

// Factories
const { push } = require('internal/streams/iter/push');
const { duplex } = require('internal/streams/iter/duplex');
const { from, fromSync } = require('internal/streams/iter/from');

// Pipelines
const {
  pull,
  pullSync,
  pipeTo,
  pipeToSync,
} = require('internal/streams/iter/pull');

// Consumers
const {
  bytes,
  bytesSync,
  text,
  textSync,
  arrayBuffer,
  arrayBufferSync,
  array,
  arraySync,
  tap,
  tapSync,
  merge,
  ondrain,
} = require('internal/streams/iter/consumers');

// Classic stream interop (Node.js-specific, not part of the spec)
const {
  fromReadable,
  fromWritable,
  toReadable,
  toReadableSync,
  toWritable,
} = require('internal/streams/iter/classic');

// Multi-consumer
const { broadcast, Broadcast } = require('internal/streams/iter/broadcast');
const {
  share,
  shareSync,
  Share,
  SyncShare,
} = require('internal/streams/iter/share');

/**
 * Stream namespace - unified access to all stream functions.
 * @example
 * const { Stream } = require('stream/iter');
 *
 * const { writer, readable } = Stream.push();
 * await writer.write("hello");
 * await writer.end();
 *
 * const output = Stream.pull(readable, transform1, transform2);
 * const data = await Stream.bytes(output);
 */
const Stream = ObjectFreeze({
  // Factories
  push,
  duplex,
  from,
  fromSync,

  // Pipelines
  pull,
  pullSync,

  // Pipe to destination
  pipeTo,
  pipeToSync,

  // Consumers (async)
  bytes,
  text,
  arrayBuffer,
  array,

  // Consumers (sync)
  bytesSync,
  textSync,
  arrayBufferSync,
  arraySync,

  // Combining
  merge,

  // Multi-consumer (push model)
  broadcast,

  // Multi-consumer (pull model)
  share,
  shareSync,

  // Utilities
  tap,
  tapSync,

  // Drain utility for event source integration
  ondrain,

  // Protocol symbols
  toStreamable,
  toAsyncStreamable,
  broadcastProtocol,
  shareProtocol,
  shareSyncProtocol,
  drainableProtocol,
});

module.exports = {
  // The Stream namespace
  Stream,

  // Also export everything individually for destructured imports

  // Protocol symbols
  toStreamable,
  toAsyncStreamable,
  broadcastProtocol,
  shareProtocol,
  shareSyncProtocol,
  drainableProtocol,

  // Factories
  push,
  duplex,
  from,
  fromSync,

  // Pipelines
  pull,
  pullSync,
  pipeTo,
  pipeToSync,

  // Consumers (async)
  bytes,
  text,
  arrayBuffer,
  array,

  // Consumers (sync)
  bytesSync,
  textSync,
  arrayBufferSync,
  arraySync,

  // Combining
  merge,

  // Multi-consumer
  broadcast,
  Broadcast,
  share,
  shareSync,
  Share,
  SyncShare,

  // Utilities
  tap,
  tapSync,
  ondrain,

  // Classic stream interop
  fromReadable,
  fromWritable,
  toReadable,
  toReadableSync,
  toWritable,
};

export { Stream, toStreamable, toAsyncStreamable, broadcastProtocol, shareProtocol, shareSyncProtocol, drainableProtocol, push, duplex, from, fromSync, pull, pullSync, pipeTo, pipeToSync, bytes, text, arrayBuffer, array, bytesSync, textSync, arrayBufferSync, arraySync, merge, broadcast, Broadcast, share, shareSync, Share, SyncShare, tap, tapSync, ondrain, fromReadable, fromWritable, toReadable, toReadableSync, toWritable };
export default module.exports;
"#;
