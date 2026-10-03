import __reg from 'node:internal/registry';

function updateReadableListening(self) {
  const state = self._readableState;

  if (self.listenerCount('readable') > 0) {
    state[kState] |= kReadableListening;
  } else {
    state[kState] &= ~kReadableListening;
  }

  if ((state[kState] & (kHasPaused | kPaused | kResumeScheduled)) === (kHasPaused | kResumeScheduled)) {
    // Flowing needs to be set to true now, otherwise
    // the upcoming resume will not flow.
    state[kState] |= kHasFlowing | kFlowing;

    // Crude way to check if we should resume.
  } else if ((state[kState] & kDataListening) !== 0) {
    self.resume();
  } else if ((state[kState] & kReadableListening) === 0) {
    state[kState] &= ~(kHasFlowing | kFlowing);
  }
}

function nReadingNextTick(self) {
  debug('readable nexttick read 0');
  self.read(0);
}

// pause() and resume() are remnants of the legacy readable stream API
// If the user uses them, then switch into old mode.
Readable.prototype.resume = function() {
  const state = this._readableState;
  if ((state[kState] & kDestroyed) !== 0) {
    return this;
  }
  if ((state[kState] & kFlowing) === 0) {
    debug('resume');
    // We flow only if there is no one listening
    // for readable, but we still have to call
    // resume().
    state[kState] |= kHasFlowing;
    if ((state[kState] & kReadableListening) === 0) {
      state[kState] |= kFlowing;
    } else {
      state[kState] &= ~kFlowing;
    }
    resume(this, state);
  }
  state[kState] |= kHasPaused;
  state[kState] &= ~kPaused;
  return this;
};

function resume(stream, state) {
  if ((state[kState] & kResumeScheduled) === 0) {
    state[kState] |= kResumeScheduled;
    process.nextTick(resume_, stream, state);
  }
}

function resume_(stream, state) {
  debug('resume', (state[kState] & kReading) !== 0);
  if ((state[kState] & kReading) === 0) {
    stream.read(0);
  }

  state[kState] &= ~kResumeScheduled;
  stream.emit('resume');
  flow(stream);
  if ((state[kState] & (kFlowing | kReading)) === kFlowing)
    stream.read(0);
}

Readable.prototype.pause = function() {
  const state = this._readableState;
  if ((state[kState] & kDestroyed) !== 0) {
    return this;
  }
  debug('call pause');
  if ((state[kState] & (kHasFlowing | kFlowing)) !== kHasFlowing) {
    debug('pause');
    state[kState] |= kHasFlowing;
    state[kState] &= ~kFlowing;
    this.emit('pause');
  }
  state[kState] |= kHasPaused | kPaused;
  return this;
};

function flow(stream) {
  const state = stream._readableState;
  debug('flow');
  while ((state[kState] & kFlowing) !== 0 && stream.read() !== null);
}

// Wrap an old-style stream as the async data source.
// This is *not* part of the readable stream interface.
// It is an ugly unfortunate mess of history.
Readable.prototype.wrap = function(stream) {
  let paused = false;

  // TODO (ronag): Should this.destroy(err) emit
  // 'error' on the wrapped stream? Would require
  // a static factory method, e.g. Readable.wrap(stream).

  stream.on('data', (chunk) => {
    if (!this.push(chunk) && stream.pause) {
      paused = true;
      stream.pause();
    }
  });

  stream.on('end', () => {
    this.push(null);
  });

  stream.on('error', (err) => {
    errorOrDestroy(this, err);
  });

  stream.on('close', () => {
    this.destroy();
  });

  stream.on('destroy', () => {
    this.destroy();
  });

  this._read = () => {
    if (paused && stream.resume) {
      paused = false;
      stream.resume();
    }
  };

  // Proxy all the other methods. Important when wrapping filters and duplexes.
  const streamKeys = ObjectKeys(stream);
  for (let j = 0; j < streamKeys.length; j++) {
    const i = streamKeys[j];
    if (this[i] === undefined && typeof stream[i] === 'function') {
      this[i] = stream[i].bind(stream);
    }
  }

  return this;
};

Readable.prototype[SymbolAsyncIterator] = function() {
  return streamToAsyncIterator(this);
};

Readable.prototype.iterator = function(options) {
  if (options !== undefined) {
    validateObject(options, 'options');
  }
  return streamToAsyncIterator(this, options);
};

function streamToAsyncIterator(stream, options) {
  if (typeof stream.read !== 'function') {
    stream = Readable.wrap(stream, { objectMode: true });
  }

  const iter = createAsyncIterator(stream, options);
  iter.stream = stream;
  return iter;
}

// Async iterator over a Readable. Requests received while another is
// outstanding are queued and processed in order.
function createAsyncIterator(stream, options) {
  let callback = nop;
  let error;            // undefined: active, null: ended cleanly, else: Error
  let started = false;
  let completed = false;
  let inFlight = false; // An asynchronous request is outstanding
  let queue = null;     // Requests received while inFlight
  let draining = false;
  let cleanup;

  // Used both as the 'readable' listener (where `this === stream`) and
  // as a promise executor storing the resolver that wakes up a pending
  // pump().
  function wakeup(resolve) {
    if (this === stream) {
      callback();
      callback = nop;
    } else {
      callback = resolve;
    }
  }

  function start() {
    started = true;

    stream.on('readable', wakeup);

    cleanup = eos(stream, { writable: false }, (err) => {
      error = err ? aggregateTwoErrors(error, err) : null;
      callback();
      callback = nop;
    });
  }

  // Complete the iterator and either destroy the stream or detach
  // from it.
  function finalize() {
    completed = true;

    const preserveHalfOpenDuplex =
      error === null &&
      stream.allowHalfOpen === true &&
      stream.writable === true &&
      stream.writableEnded !== true;

    if (
      (error || options?.destroyOnReturn !== false) &&
      (error === undefined || stream._readableState.autoDestroy) &&
      !preserveHalfOpenDuplex
    ) {
      destroyImpl.destroyer(stream, null);
    } else {
      stream.off('readable', wakeup);
      cleanup();
    }
  }

  function settleError(err, reject) {
    error = aggregateTwoErrors(error, err);
    finalize();
    reject(error);
  }

  function drain() {
    // Requests settled synchronously call back into drain(); the guard
    // keeps a single loop going instead of recursing once per request.
    if (draining) {
      return;
    }
    draining = true;
    try {
      while (!inFlight && !queue.isEmpty()) {
        const req = queue.shift();
        if (req.type === 'next') {
          processNext(req.resolve, req.reject);
        } else if (req.type === 'return') {
          processReturn(req.value, req.resolve);
        } else {
          processThrow(req.value, req.reject);
        }
      }
    } finally {
      draining = false;
    }
  }

  // Thenable chunks are unwrapped before delivery; a rejection tears
  // down the iterator and the stream.
  function onChunkFulfilled(value) {
    inFlight = false;
    if (queue !== null) drain();
    return { done: false, value };
  }

  function onChunkRejected(err) {
    inFlight = false;
    error = aggregateTwoErrors(error, err);
    finalize();
    if (queue !== null) drain();
    throw error;
  }

  // Runs with inFlight === true; settles the request and hands over to
  // any requests that queued up behind it.
  function pump(resolve, reject) {
    const chunk = stream.destroyed ? null : stream.read();
    if (chunk !== null) {
      // Read `then` only once so that a getter cannot observe (or throw
      // on) a second access.
      const then = chunk.then;
      if (typeof then === 'function') {
        FunctionPrototypeCall(then, chunk, (value) => {
          inFlight = false;
          resolve({ done: false, value });
          if (queue !== null) drain();
        }, (err) => {
          inFlight = false;
          settleError(err, reject);
          if (queue !== null) drain();
        });
        return;
      }
      inFlight = false;
      resolve({ done: false, value: chunk });
      if (queue !== null) drain();
    } else if (error) {
      inFlight = false;
      settleError(error, reject);
      if (queue !== null) drain();
    } else if (error === null) {
      inFlight = false;
      finalize();
      resolve({ done: true, value: undefined });
      if (queue !== null) drain();
    } else {
      // No data buffered yet; wait for 'readable' or end-of-stream and
      // retry.
      PromisePrototypeThen(new Promise(wakeup), () => pump(resolve, reject));
    }
  }

  function processNext(resolve, reject) {
    if (completed) {
      resolve({ done: true, value: undefined });
      return;
    }
    if (!started) start();
    inFlight = true;
    pump(resolve, reject);
  }

  function processReturn(value, resolve) {
    if (!completed) {
      if (started) {
        finalize();
      } else {
        // Never started: complete without touching the stream.
        completed = true;
      }
    }
    resolve({ done: true, value });
  }

  function processThrow(err, reject) {
    if (completed || !started) {
      completed = true;
      reject(err);
      return;
    }
    settleError(err, reject);
  }

  return {
    __proto__: AsyncIteratorPrototype,
    next() {
      if (!inFlight && !completed) {
        if (!started) start();
        // Fast path: a chunk is already buffered.
        const chunk = stream.destroyed ? null : stream.read();
        if (chunk !== null) {
          // Read `then` only once so that a getter cannot observe (or
          // throw on) a second access.
          const then = chunk.then;
          if (typeof then === 'function') {
            inFlight = true;
            return FunctionPrototypeCall(
              then, chunk, onChunkFulfilled, onChunkRejected);
          }
          return PromiseResolve({ done: false, value: chunk });
        }
        if (error) {
          finalize();
          return PromiseReject(error);
        }
        if (error === null) {
          finalize();
          return PromiseResolve({ done: true, value: undefined });
        }
        // No data buffered yet; wait for 'readable' or end-of-stream.
        inFlight = true;
        return new Promise((resolve, reject) => {
          PromisePrototypeThen(new Promise(wakeup), () => pump(resolve, reject));
        });
      }
      return new Promise((resolve, reject) => {
        if (inFlight) {
          queue ??= new FixedQueue();
          queue.push({ __proto__: null, type: 'next', value: undefined, resolve, reject });
        } else {
          resolve({ done: true, value: undefined });
        }
      });
    },
    return(value) {
      return new Promise((resolve, reject) => {
        if (inFlight) {
          queue ??= new FixedQueue();
          queue.push({ __proto__: null, type: 'return', value, resolve, reject });
        } else {
          processReturn(value, resolve);
        }
      });
    },
    throw(err) {
      return new Promise((resolve, reject) => {
        if (inFlight) {
          queue ??= new FixedQueue();
          queue.push({ __proto__: null, type: 'throw', value: err, resolve, reject });
        } else {
          processThrow(err, reject);
        }
      });
    },
  };
}

let composeImpl;

Readable.prototype.compose = function compose(stream, options) {
  if (options != null) {
    validateObject(options, 'options');
  }
  if (options?.signal != null) {
    validateAbortSignal(options.signal, 'options.signal');
  }

  composeImpl ??= require('internal/streams/compose');
  const composedStream = composeImpl(this, stream);

  if (options?.signal) {
    // Not validating as we already validated before
    addAbortSignalNoValidate(
      options.signal,
      composedStream,
    );
  }

  return composedStream;
};

// Making it explicit these properties are not enumerable
// because otherwise some prototype manipulation in
// userland will fail.
ObjectDefineProperties(Readable.prototype, {
  readable: {
    __proto__: null,
    get() {
      const r = this._readableState;
      // r.readable === false means that this is part of a Duplex stream
      // where the readable side was disabled upon construction.
      // Compat. The user might manually disable readable side through
      // deprecated setter.
      return !!r && r.readable !== false && !r.destroyed && !r.errorEmitted &&
        !r.endEmitted;
    },
    set(val) {
      // Backwards compat.
      if (this._readableState) {
        this._readableState.readable = !!val;
      }
    },
  },

  readableDidRead: {
    __proto__: null,
    enumerable: false,
    get: function() {
      return this._readableState.dataEmitted;
    },
  },

  readableAborted: {
    __proto__: null,
    enumerable: false,
    get: function() {
      return !!(
        this._readableState.readable !== false &&
        (this._readableState.destroyed || this._readableState.errored) &&
        !this._readableState.endEmitted
      );
    },
  },

  readableHighWaterMark: {
    __proto__: null,
    enumerable: false,
    get: function() {
      return this._readableState.highWaterMark;
    },
  },

  readableBuffer: {
    __proto__: null,
    enumerable: false,
    get: function() {
      return this._readableState?.buffer;
    },
  },

  readableFlowing: {
    __proto__: null,
    enumerable: false,
    get: function() {
      return this._readableState.flowing;
    },
    set: function(state) {
      if (this._readableState) {
        this._readableState.flowing = state;
      }
    },
  },

  readableLength: {
    __proto__: null,
    enumerable: false,
    get() {
      return this._readableState.length;
    },
  },

  readableObjectMode: {
    __proto__: null,
    enumerable: false,
    get() {
      return this._readableState ? this._readableState.objectMode : false;
    },
  },

  readableEncoding: {
    __proto__: null,
    enumerable: false,
    get() {
      return this._readableState ? this._readableState.encoding : null;
    },
  },

  errored: {
    __proto__: null,
    enumerable: false,
    get() {
      return this._readableState ? this._readableState.errored : null;
    },
  },

  closed: {
    __proto__: null,
    get() {
      return this._readableState ? this._readableState.closed : false;
    },
  },

  destroyed: {
    __proto__: null,
    enumerable: false,
    get() {
      return this._readableState ? this._readableState.destroyed : false;
    },
    set(value) {
      // We ignore the value if the stream
      // has not been initialized yet.
      if (!this._readableState) {
        return;
      }

      // Backward compatibility, the user is explicitly
      // managing destroyed.
      this._readableState.destroyed = value;
    },
  },

  readableEnded: {
    __proto__: null,
    enumerable: false,
    get() {
      return this._readableState ? this._readableState.endEmitted : false;
    },
  },

});

ObjectDefineProperties(ReadableState.prototype, {
  // Legacy getter for `pipesCount`.
  pipesCount: {
    __proto__: null,
    get() {
      return this.pipes.length;
    },
  },

  // Legacy property for `paused`.
  paused: {
    __proto__: null,
    get() {
      return (this[kState] & kPaused) !== 0;
    },
    set(value) {
      this[kState] |= kHasPaused;
      if (value) {
        this[kState] |= kPaused;
      } else {
        this[kState] &= ~kPaused;
      }
    },
  },
});

// Exposed for testing purposes only.
Readable._fromList = fromList;

// Pluck off n bytes from an array of buffers.
// Length is the combined lengths of all the buffers in the list.
// This function is designed to be inlinable, so please take care when making
// changes to the function body.
function fromList(n, state) {
  // `state.length` cannot change while this function runs (only the
  // caller updates it, after this returns) and the chunk lengths feeding
  // the copy loops cannot change across the copy calls, so every
  // repeated property load below is hoisted into a local.
  const stateLength = state.length;

  // nothing buffered.
  if (stateLength === 0)
    return null;

  let idx = state.bufferIndex;
  let ret;

  const buf = state.buffer;
  const len = buf.length;

  if ((state[kState] & kObjectMode) !== 0) {
    ret = buf[idx];
    buf[idx++] = null;
  } else if (!n || n >= stateLength) {
    // Read it all, truncate the list.
    if ((state[kState] & kDecoder) !== 0) {
      ret = '';
      while (idx < len) {
        ret += buf[idx];
        buf[idx++] = null;
      }
    } else if (len - idx === 0) {
      ret = new FastBuffer();
    } else if (len - idx === 1) {
      ret = buf[idx];
      buf[idx++] = null;
    } else {
      ret = Buffer.allocUnsafe(stateLength);

      let i = 0;
      while (idx < len) {
        const data = buf[idx];
        TypedArrayPrototypeSet(ret, data, i);
        i += data.length;
        buf[idx++] = null;
      }
    }
  } else {
    const first = buf[idx];
    const firstLength = first.length;
    if (n < firstLength) {
      // `slice` is the same for buffers and strings.
      ret = first.slice(0, n);
      buf[idx] = first.slice(n);
    } else if (n === firstLength) {
      // First chunk is a perfect match.
      ret = first;
      buf[idx++] = null;
    } else if ((state[kState] & kDecoder) !== 0) {
      ret = '';
      while (idx < len) {
        const str = buf[idx];
        const strLength = str.length;
        if (n > strLength) {
          ret += str;
          n -= strLength;
          buf[idx++] = null;
        } else {
          if (n === strLength) {
            ret += str;
            buf[idx++] = null;
          } else {
            ret += str.slice(0, n);
            buf[idx] = str.slice(n);
          }
          break;
        }
      }
    } else {
      ret = Buffer.allocUnsafe(n);

      const retLen = n;
      while (idx < len) {
        const data = buf[idx];
        const dataLength = data.length;
        if (n > dataLength) {
          TypedArrayPrototypeSet(ret, data, retLen - n);
          n -= dataLength;
          buf[idx++] = null;
        } else {
          if (n === dataLength) {
            TypedArrayPrototypeSet(ret, data, retLen - n);
            buf[idx++] = null;
          } else {
            TypedArrayPrototypeSet(ret, new FastBuffer(data.buffer, data.byteOffset, n), retLen - n);
            buf[idx] = new FastBuffer(data.buffer, data.byteOffset + n, dataLength - n);
          }
          break;
        }
      }
    }
  }

  if (idx === len) {
    state.buffer.length = 0;
    state.bufferIndex = 0;
  } else if (idx > 1024) {
    state.buffer.splice(0, idx);
    state.bufferIndex = 0;
  } else {
    state.bufferIndex = idx;
  }

  return ret;
}

function endReadable(stream) {
  const state = stream._readableState;

  debug('endReadable');
  if ((state[kState] & (kEndEmitted | kEndScheduled)) === 0) {
    state[kState] |= kEnded | kEndScheduled;
    process.nextTick(endReadableNT, state, stream);
  }
}

function endReadableNT(state, stream) {
  debug('endReadableNT');

  // The scheduled tick is running; allow endReadable() to schedule again.
  // This matters both when the 'end' emission is skipped below (e.g. after
  // an unshift()) and when the stream is later reset for reuse
  // (see undestroy()), which clears kEndEmitted but not this flag.
  state[kState] &= ~kEndScheduled;

  // Check that we didn't get one last unshift.
  if ((state[kState] & (kErrored | kCloseEmitted | kEndEmitted)) === 0 && state.length === 0) {
    state[kState] |= kEndEmitted;
    stream.emit('end');

    if (stream.writable && stream.allowHalfOpen === false) {
      process.nextTick(endWritableNT, stream);
    } else if (state.autoDestroy) {
      // In case of duplex streams we need a way to detect
      // if the writable side is ready for autoDestroy as well.
      const wState = stream._writableState;
      const autoDestroy = !wState || (
        wState.autoDestroy &&
        // We don't expect the writable to ever 'finish'
        // if writable is explicitly set to false.
        (wState.finished || wState.writable === false)
      );

      if (autoDestroy) {
        stream.destroy();
      }
    }
  }
}

function endWritableNT(stream) {
  const writable = stream.writable && !stream.writableEnded &&
    !stream.destroyed;
  if (writable) {
    stream.end();
  }
}

Readable.from = function(iterable, opts) {
  return from(Readable, iterable, opts);
};

let webStreamsAdapters;

// Lazy to avoid circular references
function lazyWebStreams() {
  if (webStreamsAdapters === undefined)
    webStreamsAdapters = require('internal/webstreams/adapters');
  return webStreamsAdapters;
}

Readable.fromWeb = function(readableStream, options) {
  return lazyWebStreams().newStreamReadableFromReadableStream(
    readableStream,
    options);
};

Readable.toWeb = function(streamReadable, options) {
  return lazyWebStreams().newReadableStreamFromStreamReadable(
    streamReadable,
    options);
};

Readable.wrap = function(src, options) {
  return new Readable({
    objectMode: src.readableObjectMode ?? src.objectMode ?? true,
    ...options,
    destroy(err, callback) {
      destroyImpl.destroyer(src, err);
      callback(err);
    },
  }).wrap(src);
};

// Interop with the stream/iter API via the toAsyncStreamable protocol.
//
// The batched iterator logic lives in classic.js (shared with the
// fromReadable() utility for duck-typed streams). This prototype method
// calls createBatchedAsyncIterator directly -- it must NOT call
// fromReadable() since fromReadable() checks for toAsyncStreamable,
// which would create infinite recursion.
//
// The flag cannot be checked at module load time (readable.js loads during
// bootstrap before options are available). Instead, toAsyncStreamable is
// always defined but lazily initializes on first call -- throwing if the
// flag is not set.
{
  const toAsyncStreamable = SymbolFor('Stream.toAsyncStreamable');
  let createBatchedAsyncIterator;
  let normalizeBatch;
  let kValidatedSource;

  Readable.prototype[toAsyncStreamable] = function() {
    if (createBatchedAsyncIterator === undefined) {
      if (!getOptionValue('--experimental-stream-iter')) {
        throw new ERR_STREAM_ITER_MISSING_FLAG();
      }
      ({
        createBatchedAsyncIterator,
        normalizeBatch,
      } = require('internal/streams/iter/classic'));
      ({ kValidatedSource } = require('internal/streams/iter/types'));
    }
    const state = this._readableState;
    const normalize = (state.objectMode || state.encoding) ?
      normalizeBatch : null;
    const iter = createBatchedAsyncIterator(this, normalize);
    iter[kValidatedSource] = true;
    iter.stream = this;
    return iter;
  };
}

export default module.exports;
__reg.set('node:internal/streams/readable', module.exports);
