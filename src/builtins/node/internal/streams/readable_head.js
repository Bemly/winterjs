import { primordials } from 'node:internal/primordials';
import * as __m0 from 'node:buffer';
import * as __m1 from 'node:events';
import * as __m2 from 'node:internal/errors';
import * as __m3 from 'node:internal/fixed_queue';
import * as __m4 from 'node:internal/options';
import * as __m5 from 'node:internal/streams/add_abort_signal';
import * as __m6 from 'node:internal/streams/compose';
import * as __m7 from 'node:internal/streams/destroy';
import * as __m8 from 'node:internal/streams/end_of_stream';
import * as __m9 from 'node:internal/streams/from';
import * as __m10 from 'node:internal/streams/iter_classic';
import * as __m11 from 'node:internal/streams/iter_types';
import * as __m12 from 'node:internal/streams/legacy';
import * as __m13 from 'node:internal/streams/state';
import * as __m14 from 'node:internal/streams/utils';
import * as __m15 from 'node:internal/debuglog';
import * as __m16 from 'node:internal/validators';
import * as __m17 from 'node:internal/webstream_adapters';
import * as __m18 from 'node:string_decoder';

// CJS require 垫片（静态 spec → 内建模块 default 导出；循环依赖经懒访问解环）
const require = (spec) => __requireMap(spec);
function __requireMap(spec) {
  switch (spec) {
    case 'buffer': return __m0.default;
    case 'events': return __m1.default;
    case 'internal/errors': return __m2.default;
    case 'internal/fixed_queue': return __m3.default;
    case 'internal/options': return __m4.default;
    case 'internal/streams/add-abort-signal': return __m5.default;
    case 'internal/streams/compose': return __m6.default;
    case 'internal/streams/destroy': return __m7.default;
    case 'internal/streams/end-of-stream': return __m8.default;
    case 'internal/streams/from': return __m9.default;
    case 'internal/streams/iter/classic': return __m10.default;
    case 'internal/streams/iter/types': return __m11.default;
    case 'internal/streams/legacy': return __m12.default;
    case 'internal/streams/state': return __m13.default;
    case 'internal/streams/utils': return __m14.default;
    case 'internal/util/debuglog': return __m15.default;
    case 'internal/validators': return __m16.default;
    case 'internal/webstreams/adapters': return __m17.default;
    case 'string_decoder': return __m18.default;
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
  ArrayPrototypeIndexOf,
  AsyncIteratorPrototype,
  FunctionPrototypeCall,
  NumberIsInteger,
  NumberIsNaN,
  NumberParseInt,
  ObjectDefineProperties,
  ObjectKeys,
  ObjectSetPrototypeOf,
  Promise,
  PromisePrototypeThen,
  PromiseReject,
  PromiseResolve,
  ReflectApply,
  SafeSet,
  Symbol,
  SymbolAsyncDispose,
  SymbolAsyncIterator,
  SymbolFor,
  SymbolSpecies,
  TypedArrayPrototypeSet,
} = primordials;

module.exports = Readable;
Readable.ReadableState = ReadableState;

const EE = require('events');
const { Stream, prependListener } = require('internal/streams/legacy');
const { Buffer } = require('buffer');

const {
  addAbortSignal,
  addAbortSignalNoValidate,
} = require('internal/streams/add-abort-signal');
const { eos } = require('internal/streams/end-of-stream');

const { getOptionValue } = require('internal/options');

let debug = require('internal/util/debuglog').debuglog('stream', (fn) => {
  debug = fn;
});
const destroyImpl = require('internal/streams/destroy');
const {
  getHighWaterMark,
  getDefaultHighWaterMark,
} = require('internal/streams/state');
const {
  kState,
  // bitfields
  kObjectMode,
  kErrorEmitted,
  kAutoDestroy,
  kEmitClose,
  kDestroyed,
  kClosed,
  kCloseEmitted,
  kErrored,
  kConstructed,
  kOnConstructed,
} = require('internal/streams/utils');

const {
  AbortError,
  aggregateTwoErrors,
  codes: {
    ERR_INVALID_ARG_TYPE,
    ERR_METHOD_NOT_IMPLEMENTED,
    ERR_OUT_OF_RANGE,
    ERR_STREAM_ITER_MISSING_FLAG,
    ERR_STREAM_PUSH_AFTER_EOF,
    ERR_STREAM_UNSHIFT_AFTER_END_EVENT,
    ERR_UNKNOWN_ENCODING,
  },
} = require('internal/errors');
const {
  validateAbortSignal,
  validateObject,
} = require('internal/validators');

const FastBuffer = Buffer[SymbolSpecies];

const { StringDecoder } = require('string_decoder');
const from = require('internal/streams/from');
const FixedQueue = require('internal/fixed_queue');

ObjectSetPrototypeOf(Readable.prototype, Stream.prototype);
ObjectSetPrototypeOf(Readable, Stream);
const nop = () => {};

const { errorOrDestroy } = destroyImpl;

const kErroredValue = Symbol('kErroredValue');
const kDefaultEncodingValue = Symbol('kDefaultEncodingValue');
const kDecoderValue = Symbol('kDecoderValue');
const kEncodingValue = Symbol('kEncodingValue');

const kEnded = 1 << 9;
const kEndEmitted = 1 << 10;
const kReading = 1 << 11;
const kSync = 1 << 12;
const kNeedReadable = 1 << 13;
const kEmittedReadable = 1 << 14;
const kReadableListening = 1 << 15;
const kResumeScheduled = 1 << 16;
const kMultiAwaitDrain = 1 << 17;
const kReadingMore = 1 << 18;
const kDataEmitted = 1 << 19;
const kDefaultUTF8Encoding = 1 << 20;
const kDecoder = 1 << 21;
const kEncoding = 1 << 22;
const kHasFlowing = 1 << 23;
const kFlowing = 1 << 24;
const kHasPaused = 1 << 25;
const kPaused = 1 << 26;
const kDataListening = 1 << 27;
const kEndScheduled = 1 << 28;
const kEofReadablePending = 1 << 29;

// TODO(benjamingr) it is likely slower to do it this way than with free functions
function makeBitMapDescriptor(bit) {
  return {
    enumerable: false,
    get() { return (this[kState] & bit) !== 0; },
    set(value) {
      if (value) this[kState] |= bit;
      else this[kState] &= ~bit;
    },
  };
}
ObjectDefineProperties(ReadableState.prototype, {
  objectMode: makeBitMapDescriptor(kObjectMode),
  ended: makeBitMapDescriptor(kEnded),
  endEmitted: makeBitMapDescriptor(kEndEmitted),
  reading: makeBitMapDescriptor(kReading),
  // Stream is still being constructed and cannot be
  // destroyed until construction finished or failed.
  // Async construction is opt in, therefore we start as
  // constructed.
  constructed: makeBitMapDescriptor(kConstructed),
  // A flag to be able to tell if the event 'readable'/'data' is emitted
  // immediately, or on a later tick.  We set this to true at first, because
  // any actions that shouldn't happen until "later" should generally also
  // not happen before the first read call.
  sync: makeBitMapDescriptor(kSync),
  // Whenever we return null, then we set a flag to say
  // that we're awaiting a 'readable' event emission.
  needReadable: makeBitMapDescriptor(kNeedReadable),
  emittedReadable: makeBitMapDescriptor(kEmittedReadable),
  readableListening: makeBitMapDescriptor(kReadableListening),
  resumeScheduled: makeBitMapDescriptor(kResumeScheduled),
  // True if the error was already emitted and should not be thrown again.
  errorEmitted: makeBitMapDescriptor(kErrorEmitted),
  emitClose: makeBitMapDescriptor(kEmitClose),
  autoDestroy: makeBitMapDescriptor(kAutoDestroy),
  // Has it been destroyed.
  destroyed: makeBitMapDescriptor(kDestroyed),
  // Indicates whether the stream has finished destroying.
  closed: makeBitMapDescriptor(kClosed),
  // True if close has been emitted or would have been emitted
  // depending on emitClose.
  closeEmitted: makeBitMapDescriptor(kCloseEmitted),
  multiAwaitDrain: makeBitMapDescriptor(kMultiAwaitDrain),
  // If true, a maybeReadMore has been scheduled.
  readingMore: makeBitMapDescriptor(kReadingMore),
  dataEmitted: makeBitMapDescriptor(kDataEmitted),

  // Indicates whether the stream has errored. When true no further
  // _read calls, 'data' or 'readable' events should occur. This is needed
  // since when autoDestroy is disabled we need a way to tell whether the
  // stream has failed.
  errored: {
    __proto__: null,
    enumerable: false,
    get() {
      return (this[kState] & kErrored) !== 0 ? this[kErroredValue] : null;
    },
    set(value) {
      if (value) {
        this[kErroredValue] = value;
        this[kState] |= kErrored;
      } else {
        this[kState] &= ~kErrored;
      }
    },
  },

  defaultEncoding: {
    __proto__: null,
    enumerable: false,
    get() { return (this[kState] & kDefaultUTF8Encoding) !== 0 ? 'utf8' : this[kDefaultEncodingValue]; },
    set(value) {
      if (value === 'utf8' || value === 'utf-8') {
        this[kState] |= kDefaultUTF8Encoding;
      } else {
        this[kState] &= ~kDefaultUTF8Encoding;
        this[kDefaultEncodingValue] = value;
      }
    },
  },

  decoder: {
    __proto__: null,
    enumerable: false,
    get() {
      return (this[kState] & kDecoder) !== 0 ? this[kDecoderValue] : null;
    },
    set(value) {
      if (value) {
        this[kDecoderValue] = value;
        this[kState] |= kDecoder;
      } else {
        this[kState] &= ~kDecoder;
      }
    },
  },

  encoding: {
    __proto__: null,
    enumerable: false,
    get() {
      return (this[kState] & kEncoding) !== 0 ? this[kEncodingValue] : null;
    },
    set(value) {
      if (value) {
        this[kEncodingValue] = value;
        this[kState] |= kEncoding;
      } else {
        this[kState] &= ~kEncoding;
      }
    },
  },

  flowing: {
    __proto__: null,
    enumerable: false,
    get() {
      return (this[kState] & kHasFlowing) !== 0 ? (this[kState] & kFlowing) !== 0 : null;
    },
    set(value) {
      if (value == null) {
        this[kState] &= ~(kHasFlowing | kFlowing);
      } else if (value) {
        this[kState] |= (kHasFlowing | kFlowing);
      } else {
        this[kState] |= kHasFlowing;
        this[kState] &= ~kFlowing;
      }
    },
  },
});


function ReadableState(options, stream, isDuplex) {
  // Bit map field to store ReadableState more efficiently with 1 bit per field
  // instead of a V8 slot per field.
  this[kState] = kEmitClose | kAutoDestroy | kConstructed | kSync;

  // Object stream flag. Used to make read(n) ignore n and to
  // make all the buffer merging and length checks go away.
  if (options?.objectMode)
    this[kState] |= kObjectMode;

  if (isDuplex && options?.readableObjectMode)
    this[kState] |= kObjectMode;

  // The point at which it stops calling _read() to fill the buffer
  // Note: 0 is a valid value, means "don't call _read preemptively ever"
  this.highWaterMark = options ?
    getHighWaterMark(this, options, 'readableHighWaterMark', isDuplex) :
    getDefaultHighWaterMark(false);

  this.buffer = [];
  this.bufferIndex = 0;
  this.length = 0;
  this.pipes = [];

  // Should close be emitted on destroy. Defaults to true.
  if (options && options.emitClose === false) this[kState] &= ~kEmitClose;

  // Should .destroy() be called after 'end' (and potentially 'finish').
  if (options && options.autoDestroy === false) this[kState] &= ~kAutoDestroy;

  // Crypto is kind of old and crusty.  Historically, its default string
  // encoding is 'binary' so we have to make this configurable.
  // Everything else in the universe uses 'utf8', though.
  const defaultEncoding = options?.defaultEncoding;
  if (defaultEncoding == null || defaultEncoding === 'utf8' || defaultEncoding === 'utf-8') {
    this[kState] |= kDefaultUTF8Encoding;
  } else if (Buffer.isEncoding(defaultEncoding)) {
    this.defaultEncoding = defaultEncoding;
  } else {
    throw new ERR_UNKNOWN_ENCODING(defaultEncoding);
  }

  // Ref the piped dest which we need a drain event on it
  // type: null | Writable | Set<Writable>.
  this.awaitDrainWriters = null;

  if (options?.encoding) {
    this.decoder = new StringDecoder(options.encoding);
    this.encoding = options.encoding;
  }
}

ReadableState.prototype[kOnConstructed] = function onConstructed(stream) {
  if ((this[kState] & kNeedReadable) !== 0) {
    maybeReadMore(stream, this);
  }
};

function Readable(options) {
  if (!(this instanceof Readable))
    return new Readable(options);

  this._events ??= {
    close: undefined,
    error: undefined,
    data: undefined,
    end: undefined,
    readable: undefined,
    // Skip uncommon events...
    // pause: undefined,
    // resume: undefined,
    // pipe: undefined,
    // unpipe: undefined,
    // [destroyImpl.kConstruct]: undefined,
    // [destroyImpl.kDestroy]: undefined,
  };

  this._readableState = new ReadableState(options, this, false);

  if (options) {
    if (typeof options.read === 'function')
      this._read = options.read;

    if (typeof options.destroy === 'function')
      this._destroy = options.destroy;

    if (typeof options.construct === 'function')
      this._construct = options.construct;

    if (options.signal)
      addAbortSignal(options.signal, this);
  }

  Stream.call(this, options);

  if (this._construct != null) {
    destroyImpl.construct(this, () => {
      this._readableState[kOnConstructed](this);
    });
  }
}

Readable.prototype.destroy = destroyImpl.destroy;
Readable.prototype._undestroy = destroyImpl.undestroy;
Readable.prototype._destroy = function(err, cb) {
  cb(err);
};

Readable.prototype[EE.captureRejectionSymbol] = function(err) {
  this.destroy(err);
};

Readable.prototype[SymbolAsyncDispose] = async function() {
  let error;
  if (!this.destroyed) {
    error = this.readableEnded ? null : new AbortError();
    this.destroy(error);
  }
  await new Promise((resolve, reject) => eos(this, (err) => (err && err !== error ? reject(err) : resolve(null))));
};

// Manually shove something into the read() buffer.
// This returns true if the highWaterMark has not been hit yet,
// similar to how Writable.write() returns true if you should
// write() some more.
Readable.prototype.push = function(chunk, encoding) {
  debug('push', chunk);

  const state = this._readableState;
  return (state[kState] & kObjectMode) === 0 ?
    readableAddChunkPushByteMode(this, state, chunk, encoding) :
    readableAddChunkPushObjectMode(this, state, chunk, encoding);
};

// Unshift should *always* be something directly out of read().
Readable.prototype.unshift = function(chunk, encoding) {
  debug('unshift', chunk);
  const state = this._readableState;
  return (state[kState] & kObjectMode) === 0 ?
    readableAddChunkUnshiftByteMode(this, state, chunk, encoding) :
    readableAddChunkUnshiftObjectMode(this, state, chunk);
};


function readableAddChunkUnshiftByteMode(stream, state, chunk, encoding) {
  if (chunk === null) {
    state[kState] &= ~kReading;
    onEofChunk(stream, state);

    return false;
  }

  if (typeof chunk === 'string') {
    encoding ||= state.defaultEncoding;
    if (state.encoding !== encoding) {
      if (state.encoding) {
        // When unshifting, if state.encoding is set, we have to save
        // the string in the BufferList with the state encoding.
        chunk = Buffer.from(chunk, encoding).toString(state.encoding);
      } else {
        chunk = Buffer.from(chunk, encoding);
      }
    }
  } else if (Stream._isArrayBufferView(chunk)) {
    chunk = Stream._uint8ArrayToBuffer(chunk);
  } else if (chunk !== undefined && !(chunk instanceof Buffer)) {
    errorOrDestroy(stream, new ERR_INVALID_ARG_TYPE(
      'chunk', ['string', 'Buffer', 'TypedArray', 'DataView'], chunk));
    return false;
  }


  if (!(chunk && chunk.length > 0)) {
    return canPushMore(state);
  }

  return readableAddChunkUnshiftValue(stream, state, chunk);
}

function readableAddChunkUnshiftObjectMode(stream, state, chunk) {
  if (chunk === null) {
    state[kState] &= ~kReading;
    onEofChunk(stream, state);

    return false;
  }

  return readableAddChunkUnshiftValue(stream, state, chunk);
}

function readableAddChunkUnshiftValue(stream, state, chunk) {
  if ((state[kState] & kEndEmitted) !== 0)
    errorOrDestroy(stream, new ERR_STREAM_UNSHIFT_AFTER_END_EVENT());
  else if ((state[kState] & (kDestroyed | kErrored)) !== 0)
    return false;
  else
    addChunk(stream, state, chunk, true);

  return canPushMore(state);
}

function readableAddChunkPushByteMode(stream, state, chunk, encoding) {
  if (chunk === null) {
    state[kState] &= ~kReading;
    onEofChunk(stream, state);
    return false;
  }

  if (typeof chunk === 'string') {
    encoding ||= state.defaultEncoding;
    if (state.encoding !== encoding) {
      chunk = Buffer.from(chunk, encoding);
      encoding = '';
    }
  } else if (chunk instanceof Buffer) {
    encoding = '';
  } else if (Stream._isArrayBufferView(chunk)) {
    chunk = Stream._uint8ArrayToBuffer(chunk);
    encoding = '';
  } else if (chunk !== undefined) {
    errorOrDestroy(stream, new ERR_INVALID_ARG_TYPE(
      'chunk', ['string', 'Buffer', 'TypedArray', 'DataView'], chunk));
    return false;
  }

  if (!chunk || chunk.length <= 0) {
    state[kState] &= ~kReading;
    maybeReadMore(stream, state);

    return canPushMore(state);
  }

  if ((state[kState] & kEnded) !== 0) {
    errorOrDestroy(stream, new ERR_STREAM_PUSH_AFTER_EOF());
    return false;
  }

  if ((state[kState] & (kDestroyed | kErrored)) !== 0) {
    return false;
  }

  state[kState] &= ~kReading;
  if ((state[kState] & kDecoder) !== 0 && !encoding) {
    chunk = state[kDecoderValue].write(chunk);
    if (chunk.length === 0) {
      maybeReadMore(stream, state);
      return canPushMore(state);
    }
  }

  addChunk(stream, state, chunk, false);
  return canPushMore(state);
}

function readableAddChunkPushObjectMode(stream, state, chunk, encoding) {
  if (chunk === null) {
    state[kState] &= ~kReading;
    onEofChunk(stream, state);
    return false;
  }

  if ((state[kState] & kEnded) !== 0) {
    errorOrDestroy(stream, new ERR_STREAM_PUSH_AFTER_EOF());
    return false;
  }

  if ((state[kState] & (kDestroyed | kErrored)) !== 0) {
    return false;
  }

  state[kState] &= ~kReading;

  if ((state[kState] & kDecoder) !== 0 && !encoding) {
    chunk = state[kDecoderValue].write(chunk);
  }

  addChunk(stream, state, chunk, false);
  return canPushMore(state);
}

function canPushMore(state) {
  // We can push more data if we are below the highWaterMark.
  // Also, if we have no data yet, we can stand some more bytes.
  // This is to work around cases where hwm=0, such as the repl.
  return (state[kState] & kEnded) === 0 &&
    (state.length < state.highWaterMark || state.length === 0);
}

function addChunk(stream, state, chunk, addToFront) {
  if ((state[kState] & (kFlowing | kSync | kDataListening)) === (kFlowing | kDataListening) && state.length === 0) {
    // Use the guard to avoid creating `Set()` repeatedly
    // when we have multiple pipes.
    if ((state[kState] & kMultiAwaitDrain) !== 0) {
      state.awaitDrainWriters.clear();
    } else {
      state.awaitDrainWriters = null;
    }

    state[kState] |= kDataEmitted;
    stream.emit('data', chunk);
  } else {
    // Update the buffer info.
    state.length += (state[kState] & kObjectMode) !== 0 ? 1 : chunk.length;
    if (addToFront) {
      if (state.bufferIndex > 0) {
        state.buffer[--state.bufferIndex] = chunk;
      } else {
        state.buffer.unshift(chunk); // Slow path
      }
    } else {
      state.buffer.push(chunk);
    }

    if ((state[kState] & kNeedReadable) !== 0)
      emitReadable(stream);
  }
  maybeReadMore(stream, state);
}

Readable.prototype.isPaused = function() {
  const state = this._readableState;
  return (state[kState] & kPaused) !== 0 || (state[kState] & (kHasFlowing | kFlowing)) === kHasFlowing;
};

// Backwards compatibility.
Readable.prototype.setEncoding = function(enc) {
  const state = this._readableState;

  const decoder = new StringDecoder(enc);
  state.decoder = decoder;
  // If setEncoding(null), decoder.encoding equals utf8.
  state.encoding = state.decoder.encoding;

  // Iterate over current buffer to convert already stored Buffers:
  let content = '';
  for (const data of state.buffer.slice(state.bufferIndex)) {
    content += decoder.write(data);
  }
  if ((state[kState] & kEnded) !== 0)
    content += decoder.end();
  state.buffer.length = 0;
  state.bufferIndex = 0;

  if (content !== '')
    state.buffer.push(content);
  state.length = content.length;
  return this;
};
