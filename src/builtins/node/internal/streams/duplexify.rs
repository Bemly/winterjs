//! `node:internal/streams/duplexify`（Node lib/internal/streams/duplexify.js 逐字内嵌，MIT）。
/// 来源：nodejs/node `internal/streams/duplexify.js`（MIT 头见源内）逐字内嵌；require → 垫片映射，
/// primordials → node:internal/primordials。偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"import { primordials } from 'node:internal/primordials';
import * as __m0 from 'node:internal/abort_controller';
import * as __m1 from 'node:internal/blob';
import * as __m2 from 'node:internal/errors';
import * as __m3 from 'node:internal/streams/destroy';
import * as __m4 from 'node:internal/streams/end_of_stream';
import * as __m5 from 'node:internal/streams/from';
import * as __m6 from 'node:internal/streams/readable';
import * as __m7 from 'node:internal/streams/utils';
import * as __m8 from 'node:internal/streams/writable';
import __reg from 'node:internal/registry';

// CJS require 垫片（静态 spec → 内建模块 default 导出；循环依赖经懒访问解环）
const require = (spec) => __requireMap(spec);
function __requireMap(spec) {
  switch (spec) {
    case 'internal/abort_controller': return __m0.default;
    case 'internal/blob': return __m1.default;
    case 'internal/errors': return __m2.default;
    case 'internal/streams/destroy': return __m3.default;
    case 'internal/streams/end-of-stream': return __m4.default;
    case 'internal/streams/from': return __m5.default;
    case 'internal/streams/readable': return __m6.default;
    case 'internal/streams/utils': return __m7.default;
    case 'internal/streams/writable': return __m8.default;
    case 'internal/streams/duplex': { const v = __reg.get('node:internal/streams/duplex'); if (!v) throw new Error('lazy module not evaluated yet: internal/streams/duplex'); return v; }
  default: throw new Error('unmapped internal require: ' + spec);
  }
}

const module = { exports: { __proto__: null } };

'use strict';

const {
  FunctionPrototypeCall,
  PromiseWithResolvers,
} = primordials;

const {
  isReadable,
  isWritable,
  isIterable,
  isNodeStream,
  isReadableNodeStream,
  isWritableNodeStream,
  isDuplexNodeStream,
  isReadableStream,
  isWritableStream,
} = require('internal/streams/utils');
const { eos } = require('internal/streams/end-of-stream');
const {
  AbortError,
  codes: {
    ERR_INVALID_ARG_TYPE,
    ERR_INVALID_RETURN_VALUE,
  },
} = require('internal/errors');
const { destroyer } = require('internal/streams/destroy');
const Duplex = require('internal/streams/duplex');
const Readable = require('internal/streams/readable');
const Writable = require('internal/streams/writable');
const from = require('internal/streams/from');

const {
  isBlob,
} = require('internal/blob');
const { AbortController } = require('internal/abort_controller');

// This is needed for pre node 17.
class Duplexify extends Duplex {
  constructor(options) {
    super(options);

    // https://github.com/nodejs/node/pull/34385

    if (options?.readable === false) {
      this._readableState.readable = false;
      this._readableState.ended = true;
      this._readableState.endEmitted = true;
    }

    if (options?.writable === false) {
      this._writableState.writable = false;
      this._writableState.ending = true;
      this._writableState.ended = true;
      this._writableState.finished = true;
    }
  }
}

module.exports = function duplexify(body, name) {
  if (isDuplexNodeStream(body)) {
    return body;
  }

  if (isReadableNodeStream(body)) {
    return _duplexify({ __proto__: null, readable: body });
  }

  if (isWritableNodeStream(body)) {
    return _duplexify({ __proto__: null, writable: body });
  }

  if (isNodeStream(body)) {
    return _duplexify({ __proto__: null, writable: false, readable: false });
  }

  if (isReadableStream(body)) {
    return _duplexify({ __proto__: null, readable: Readable.fromWeb(body) });
  }

  if (isWritableStream(body)) {
    return _duplexify({ __proto__: null, writable: Writable.fromWeb(body) });
  }

  if (typeof body === 'function') {
    const { value, write, final, destroy } = fromAsyncGen(body);

    // Body might be a constructor function instead of an async generator function.
    if (isDuplexNodeStream(value)) {
      return value;
    }

    if (isIterable(value)) {
      return from(Duplexify, value, {
        // TODO (ronag): highWaterMark?
        objectMode: true,
        write,
        final,
        destroy,
      });
    }

    const then = value?.then;
    if (typeof then === 'function') {
      let d;

      const promise = FunctionPrototypeCall(
        then,
        value,
        (val) => {
          if (val != null) {
            throw new ERR_INVALID_RETURN_VALUE('nully', 'body', val);
          }
        },
        (err) => {
          destroyer(d, err);
        },
      );

      return d = new Duplexify({
        // TODO (ronag): highWaterMark?
        objectMode: true,
        readable: false,
        write,
        final(cb) {
          final(async () => {
            try {
              await promise;
              process.nextTick(cb, null);
            } catch (err) {
              process.nextTick(cb, err);
            }
          });
        },
        destroy,
      });
    }

    throw new ERR_INVALID_RETURN_VALUE(
      'Iterable, AsyncIterable or AsyncFunction', name, value);
  }

  if (isBlob(body)) {
    return duplexify(body.arrayBuffer());
  }

  if (isIterable(body)) {
    return from(Duplexify, body, {
      // TODO (ronag): highWaterMark?
      objectMode: true,
      writable: false,
    });
  }

  if (
    isReadableStream(body?.readable) &&
    isWritableStream(body?.writable)
  ) {
    return Duplexify.fromWeb(body);
  }

  if (
    typeof body?.writable === 'object' ||
    typeof body?.readable === 'object'
  ) {
    const readable = body?.readable ?
      isReadableNodeStream(body?.readable) ? body?.readable :
        duplexify(body.readable) :
      undefined;

    const writable = body?.writable ?
      isWritableNodeStream(body?.writable) ? body?.writable :
        duplexify(body.writable) :
      undefined;

    return _duplexify({ __proto__: null, readable, writable });
  }

  const then = body?.then;
  if (typeof then === 'function') {
    let d;

    FunctionPrototypeCall(
      then,
      body,
      (val) => {
        if (val != null) {
          d.push(val);
        }
        d.push(null);
      },
      (err) => {
        destroyer(d, err);
      },
    );

    return d = new Duplexify({
      objectMode: true,
      writable: false,
      read() {},
    });
  }

  throw new ERR_INVALID_ARG_TYPE(
    name,
    ['Blob', 'ReadableStream', 'WritableStream', 'Stream', 'Iterable',
     'AsyncIterable', 'Function', '{ readable, writable } pair', 'Promise'],
    body);
};

function fromAsyncGen(fn) {
  let { promise, resolve } = PromiseWithResolvers();
  const ac = new AbortController();
  const signal = ac.signal;
  const value = fn(async function*() {
    while (true) {
      const _promise = promise;
      promise = null;
      const { chunk, done, cb } = await _promise;
      process.nextTick(cb);
      if (done) return;
      if (signal.aborted)
        throw new AbortError(undefined, { cause: signal.reason });
      ({ promise, resolve } = PromiseWithResolvers());
      yield chunk;
    }
  }(), { signal });

  return {
    value,
    write(chunk, encoding, cb) {
      const _resolve = resolve;
      resolve = null;
      _resolve({ __proto__: null, chunk, done: false, cb });
    },
    final(cb) {
      const _resolve = resolve;
      resolve = null;
      _resolve({ __proto__: null, done: true, cb });
    },
    destroy(err, cb) {
      ac.abort(err);

      // If the source async iterator is waiting for the next write/final
      // signal, unblock it so the readable side can observe the abort and
      // finish destroying.
      if (resolve !== null) {
        const _resolve = resolve;
        resolve = null;
        _resolve({ __proto__: null, done: true, cb() {} });
      }

      cb(err);
    },
  };
}

function _duplexify(pair) {
  const r = pair.readable && typeof pair.readable.read !== 'function' ?
    Readable.wrap(pair.readable) : pair.readable;
  const w = pair.writable;

  let readable = !!isReadable(r);
  let writable = !!isWritable(w);

  let ondrain;
  let onfinish;
  let onreadable;
  let onclose;
  let d;

  function onfinished(err) {
    const cb = onclose;
    onclose = null;

    if (cb) {
      cb(err);
    } else if (err) {
      d.destroy(err);
    }
  }

  // TODO(ronag): Avoid double buffering.
  // Implement Writable/Readable/Duplex traits.
  // See, https://github.com/nodejs/node/pull/33515.
  d = new Duplexify({
    // TODO (ronag): highWaterMark?
    readableObjectMode: !!r?.readableObjectMode,
    writableObjectMode: !!w?.writableObjectMode,
    readable,
    writable,
  });

  if (writable) {
    eos(w, (err) => {
      writable = false;
      if (err) {
        destroyer(r, err);
      }
      onfinished(err);
    });

    d._write = function(chunk, encoding, callback) {
      if (w.write(chunk, encoding)) {
        callback();
      } else {
        ondrain = callback;
      }
    };

    d._final = function(callback) {
      w.end();
      onfinish = callback;
    };

    w.on('drain', function() {
      if (ondrain) {
        const cb = ondrain;
        ondrain = null;
        cb();
      }
    });

    w.on('finish', function() {
      if (onfinish) {
        const cb = onfinish;
        onfinish = null;
        cb();
      }
    });
  }

  if (readable) {
    eos(r, (err) => {
      readable = false;
      if (err) {
        destroyer(w, err);
      }
      onfinished(err);
    });

    r.on('readable', function() {
      if (onreadable) {
        const cb = onreadable;
        onreadable = null;
        cb();
      }
    });

    r.on('end', function() {
      d.push(null);
    });

    d._read = function() {
      while (true) {
        const buf = r.read();

        if (buf === null) {
          onreadable = d._read;
          return;
        }

        if (!d.push(buf)) {
          return;
        }
      }
    };
  }

  d._destroy = function(err, callback) {
    if (!err && onclose !== null) {
      err = new AbortError();
    }

    onreadable = null;
    ondrain = null;
    onfinish = null;

    if (onclose === null) {
      callback(err);
    } else {
      onclose = callback;
      destroyer(w, err);
      destroyer(r, err);
    }
  };

  return d;
}

export default module.exports;
__reg.set('node:internal/streams/duplexify', module.exports);
"#;
