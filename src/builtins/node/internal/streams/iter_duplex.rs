//! `node:internal/streams/iter_duplex`（Node internal/streams/iter/duplex.js 逐字内嵌，MIT）。
/// 来源：nodejs/node `internal/streams/iter/duplex.js`（MIT 头见源内）逐字内嵌；require → 垫片映射，
/// primordials → node:internal/primordials。偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"import { primordials } from 'node:internal/primordials';
import * as __m0 from 'node:internal/streams/iter_push';
import * as __m1 from 'node:internal/validators';

// CJS require 垫片（静态 spec → 内建模块 default 导出；循环依赖经懒访问解环）
const require = (spec) => __requireMap(spec);
function __requireMap(spec) {
  switch (spec) {
    case 'internal/streams/iter/push': return __m0.default;
    case 'internal/validators': return __m1.default;
  default: throw new Error('unmapped internal require: ' + spec);
  }
}

const module = { exports: { __proto__: null } };

'use strict';

// New Streams API - Duplex Channel
//
// Creates a pair of connected channels where data written to one
// channel's writer appears in the other channel's readable.

const {
  SymbolAsyncDispose,
  SymbolAsyncIterator,
} = primordials;

const {
  push,
} = require('internal/streams/iter/push');
const {
  validateAbortSignal,
  validateObject,
} = require('internal/validators');

/**
 * Create a pair of connected duplex channels for bidirectional communication.
 * @param {{ budget?: number, backpressure?: string, signal?: AbortSignal,
 *           a?: object, b?: object }} [options]
 * @returns {[DuplexChannel, DuplexChannel]}
 */
function duplex(options = { __proto__: null }) {
  validateObject(options, 'options');
  const { budget, backpressure, signal, a, b } = options;
  if (a !== undefined) {
    validateObject(a, 'options.a');
  }
  if (b !== undefined) {
    validateObject(b, 'options.b');
  }
  if (signal !== undefined) {
    validateAbortSignal(signal, 'options.signal');
  }

  // Channel A writes to B's readable (A->B direction).
  // Signal is NOT passed to push() -- we handle abort via close() below.
  const { writer: aWriter, readable: bReadable } = push({
    budget: a?.budget ?? budget,
    backpressure: a?.backpressure ?? backpressure,
  });

  // Channel B writes to A's readable (B->A direction)
  const { writer: bWriter, readable: aReadable } = push({
    budget: b?.budget ?? budget,
    backpressure: b?.backpressure ?? backpressure,
  });

  let aClosed = false;
  let bClosed = false;
  // Track active iterators so close() can call .return() on them
  let aReadableIterator = null;
  let bReadableIterator = null;

  const channelA = {
    __proto__: null,
    get writer() { return aWriter; },
    // Wrap readable to track the iterator for cleanup on close()
    get readable() {
      return {
        __proto__: null,
        [SymbolAsyncIterator]() {
          const iter = aReadable[SymbolAsyncIterator]();
          aReadableIterator = iter;
          return iter;
        },
      };
    },
    async close() {
      if (aClosed) return;
      aClosed = true;
      // End the writer (signals end-of-stream to B's readable)
      aWriter.endSync();
      // Stop iteration of this channel's readable
      if (aReadableIterator?.return) {
        await aReadableIterator.return();
        aReadableIterator = null;
      }
    },
    [SymbolAsyncDispose]() {
      return this.close();
    },
  };

  const channelB = {
    __proto__: null,
    get writer() { return bWriter; },
    get readable() {
      return {
        __proto__: null,
        [SymbolAsyncIterator]() {
          const iter = bReadable[SymbolAsyncIterator]();
          bReadableIterator = iter;
          return iter;
        },
      };
    },
    async close() {
      if (bClosed) return;
      bClosed = true;
      bWriter.endSync();
      if (bReadableIterator?.return) {
        await bReadableIterator.return();
        bReadableIterator = null;
      }
    },
    [SymbolAsyncDispose]() {
      return this.close();
    },
  };

  // Signal handler: fail both writers with the abort reason so consumers
  // see the error. This is an error-path shutdown, not a clean close.
  if (signal) {
    const abortBoth = () => {
      const reason = signal.reason;
      aWriter.fail(reason);
      bWriter.fail(reason);
    };
    if (signal.aborted) {
      abortBoth();
    } else {
      signal.addEventListener('abort', abortBoth,
                              { __proto__: null, once: true });
    }
  }

  return [channelA, channelB];
}

module.exports = {
  duplex,
};

export { duplex };
export default module.exports;
"#;
