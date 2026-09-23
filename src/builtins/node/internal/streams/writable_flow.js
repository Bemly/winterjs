
// If we're already writing something, then just put this
// in the queue, and wait our turn.  Otherwise, call _write
// If we return false, then we need a drain event, so set that flag.
function writeOrBuffer(stream, state, chunk, encoding, callback) {
  const len = (state[kState] & kObjectMode) !== 0 ? 1 : chunk.length;

  state.length += len;

  if ((state[kState] & (kWriting | kErrored | kCorked | kConstructed)) !== kConstructed) {
    if ((state[kState] & kBuffered) === 0) {
      state[kState] |= kBuffered;
      state[kBufferedValue] = [];
    }

    state[kBufferedValue].push({ chunk, encoding, callback });
    if ((state[kState] & kAllBuffers) !== 0 && encoding !== 'buffer') {
      state[kState] &= ~kAllBuffers;
    }
    if ((state[kState] & kAllNoop) !== 0 && callback !== nop) {
      state[kState] &= ~kAllNoop;
    }
  } else {
    state.writelen = len;
    if (callback !== nop) {
      state.writecb = callback;
    }
    state[kState] |= kWriting | kSync | kExpectWriteCb;
    stream._write(chunk, encoding, state.onwrite);
    state[kState] &= ~kSync;
  }

  const ret = state.length < state.highWaterMark || state.length === 0;

  if (!ret) {
    state[kState] |= kNeedDrain;
  }

  // Return false if errored or destroyed in order to break
  // any synchronous while(stream.write(data)) loops.
  return ret && (state[kState] & (kDestroyed | kErrored)) === 0;
}

function doWrite(stream, state, writev, len, chunk, encoding, cb) {
  state.writelen = len;
  if (cb !== nop) {
    state.writecb = cb;
  }
  state[kState] |= kWriting | kSync | kExpectWriteCb;
  if ((state[kState] & kDestroyed) !== 0)
    state.onwrite(new ERR_STREAM_DESTROYED('write'));
  else if (writev)
    stream._writev(chunk, state.onwrite);
  else
    stream._write(chunk, encoding, state.onwrite);
  state[kState] &= ~kSync;
}

function onwriteError(stream, state, er, cb) {
  --state.pendingcb;

  cb(er);
  // Ensure callbacks are invoked even when autoDestroy is
  // not enabled. Passing `er` here doesn't make sense since
  // it's related to one specific write, not to the buffered
  // writes.
  errorBuffer(state);
  // This can emit error, but error must always follow cb.
  errorOrDestroy(stream, er);
}

function onwrite(stream, er) {
  const state = stream._writableState;

  if ((state[kState] & kExpectWriteCb) === 0) {
    errorOrDestroy(stream, new ERR_MULTIPLE_CALLBACK());
    return;
  }

  const sync = (state[kState] & kSync) !== 0;
  const cb = (state[kState] & kWriteCb) !== 0 ? state[kWriteCbValue] : nop;

  state.writecb = null;
  state[kState] &= ~(kWriting | kExpectWriteCb);
  state.length -= state.writelen;
  state.writelen = 0;

  if (er) {
    // Avoid V8 leak, https://github.com/nodejs/node/pull/34103#issuecomment-652002364
    er.stack; // eslint-disable-line no-unused-expressions

    if ((state[kState] & kErrored) === 0) {
      state[kErroredValue] = er;
      state[kState] |= kErrored;
    }

    // In case of duplex streams we need to notify the readable side of the
    // error.
    if (stream._readableState && !stream._readableState.errored) {
      stream._readableState.errored = er;
    }

    if (sync) {
      process.nextTick(onwriteError, stream, state, er, cb);
    } else {
      onwriteError(stream, state, er, cb);
    }
  } else {
    if ((state[kState] & kBuffered) !== 0) {
      clearBuffer(stream, state);
    }

    if (sync) {
      const needDrain = (state[kState] & kNeedDrain) !== 0 && state.length === 0;
      const needTick = needDrain || (state[kState] & kDestroyed !== 0) || cb !== nop;

      // It is a common case that the callback passed to .write() is always
      // the same. In that case, we do not schedule a new nextTick(), but
      // rather just increase a counter, to improve performance and avoid
      // memory allocations.
      if (cb === nop) {
        if ((state[kState] & kAfterWritePending) === 0 && needTick) {
          process.nextTick(afterWrite, stream, state, 1, cb);
          state[kState] |= kAfterWritePending;
        } else {
          state.pendingcb--;
          if ((state[kState] & kEnding) !== 0) {
            finishMaybe(stream, state, true);
          }
        }
      } else if ((state[kState] & kAfterWriteTickInfo) !== 0 &&
                 state[kAfterWriteTickInfoValue].cb === cb) {
        state[kAfterWriteTickInfoValue].count++;
      } else if (needTick) {
        state[kAfterWriteTickInfoValue] = { count: 1, cb, stream, state };
        process.nextTick(afterWriteTick, state[kAfterWriteTickInfoValue]);
        state[kState] |= (kAfterWritePending | kAfterWriteTickInfo);
      } else {
        state.pendingcb--;
        if ((state[kState] & kEnding) !== 0) {
          finishMaybe(stream, state, true);
        }
      }
    } else {
      afterWrite(stream, state, 1, cb);
    }
  }
}

function afterWriteTick({ stream, state, count, cb }) {
  state[kState] &= ~kAfterWriteTickInfo;
  state[kAfterWriteTickInfoValue] = null;
  return afterWrite(stream, state, count, cb);
}

function afterWrite(stream, state, count, cb) {
  state[kState] &= ~kAfterWritePending;

  const needDrain = (state[kState] & (kEnding | kNeedDrain | kDestroyed)) === kNeedDrain && state.length === 0;
  if (needDrain) {
    state[kState] &= ~kNeedDrain;
    stream.emit('drain');
  }

  while (count-- > 0) {
    state.pendingcb--;
    cb(null);
  }

  if ((state[kState] & kDestroyed) !== 0) {
    errorBuffer(state);
  }

  if ((state[kState] & kEnding) !== 0) {
    finishMaybe(stream, state, true);
  }
}

// If there's something in the buffer waiting, then invoke callbacks.
function errorBuffer(state) {
  if ((state[kState] & kWriting) !== 0) {
    return;
  }

  if ((state[kState] & kBuffered) !== 0) {
    for (let n = state.bufferedIndex; n < state.buffered.length; ++n) {
      const { chunk, callback } = state[kBufferedValue][n];
      const len = (state[kState] & kObjectMode) !== 0 ? 1 : chunk.length;
      state.length -= len;
      callback(state.errored ?? new ERR_STREAM_DESTROYED('write'));
    }
  }


  callFinishedCallbacks(state, state.errored ?? new ERR_STREAM_DESTROYED('end'));

  resetBuffer(state);
}

// If there's something in the buffer waiting, then process it.
function clearBuffer(stream, state) {
  if ((state[kState] & (kDestroyed | kBufferProcessing | kCorked | kBuffered | kConstructed)) !==
      (kBuffered | kConstructed)) {
    return;
  }

  const objectMode = (state[kState] & kObjectMode) !== 0;
  const { [kBufferedValue]: buffered, bufferedIndex } = state;
  const bufferedLength = buffered.length - bufferedIndex;

  if (!bufferedLength) {
    return;
  }

  let i = bufferedIndex;

  state[kState] |= kBufferProcessing;
  if (bufferedLength > 1 && stream._writev) {
    state.pendingcb -= bufferedLength - 1;

    const callback = (state[kState] & kAllNoop) !== 0 ? nop : (err) => {
      for (let n = i; n < buffered.length; ++n) {
        buffered[n].callback(err);
      }
    };
    // Make a copy of `buffered` if it's going to be used by `callback` above,
    // since `doWrite` will mutate the array.
    const chunks = (state[kState] & kAllNoop) !== 0 && i === 0 ?
      buffered : ArrayPrototypeSlice(buffered, i);
    chunks.allBuffers = (state[kState] & kAllBuffers) !== 0;

    doWrite(stream, state, true, state.length, chunks, '', callback);

    resetBuffer(state);
  } else {
    do {
      const { chunk, encoding, callback } = buffered[i];
      buffered[i++] = null;
      const len = objectMode ? 1 : chunk.length;
      doWrite(stream, state, false, len, chunk, encoding, callback);
    } while (i < buffered.length && (state[kState] & kWriting) === 0);

    if (i === buffered.length) {
      resetBuffer(state);
    } else if (i > 256 && i * 2 >= buffered.length) {
      buffered.splice(0, i);
      state.bufferedIndex = 0;
    } else {
      state.bufferedIndex = i;
    }
  }
  state[kState] &= ~kBufferProcessing;
}

Writable.prototype._write = function(chunk, encoding, cb) {
  if (this._writev) {
    this._writev([{ chunk, encoding }], cb);
  } else {
    throw new ERR_METHOD_NOT_IMPLEMENTED('_write()');
  }
};

Writable.prototype._writev = null;

Writable.prototype.end = function(chunk, encoding, cb) {
  const state = this._writableState;

  if (typeof chunk === 'function') {
    cb = chunk;
    chunk = null;
    encoding = null;
  } else if (typeof encoding === 'function') {
    cb = encoding;
    encoding = null;
  }

  let err;

  if (chunk != null) {
    const ret = _write(this, chunk, encoding);
    if (ret instanceof Error) {
      err = ret;
    }
  }

  // .end() fully uncorks.
  if ((state[kState] & kCorked) !== 0) {
    state.corked = 1;
    this.uncork();
  }

  if (err) {
    // Do nothing...
  } else if ((state[kState] & (kEnding | kErrored)) === 0) {
    // This is forgiving in terms of unnecessary calls to end() and can hide
    // logic errors. However, usually such errors are harmless and causing a
    // hard error can be disproportionately destructive. It is not always
    // trivial for the user to determine whether end() needs to be called
    // or not.

    state[kState] |= kEnding;
    finishMaybe(this, state, true);
    state[kState] |= kEnded;
  } else if ((state[kState] & kFinished) !== 0) {
    err = new ERR_STREAM_ALREADY_FINISHED('end');
  } else if ((state[kState] & kDestroyed) !== 0) {
    // node 口径：已销毁且记错时 end 回调带已记错误（writable-finished 套件
    // end-again 形）；无错沿旧路 STREAM_DESTROYED。
    err = state.errored ?? new ERR_STREAM_DESTROYED('end');
  }

  if (typeof cb === 'function') {
    if (err) {
      process.nextTick(cb, err);
    } else if ((state[kState] & kErrored) !== 0) {
      process.nextTick(cb, state[kErroredValue]);
    } else if ((state[kState] & kFinished) !== 0) {
      process.nextTick(cb, null);
    } else {
      state[kState] |= kOnFinished;
      state[kOnFinishedValue] ??= [];
      state[kOnFinishedValue].push(cb);
    }
  }

  return this;
};

function needFinish(state) {
  return (
    // State is ended && constructed but not destroyed, finished, writing, errorEmitted or closedEmitted
    (state[kState] & (
      kEnding |
          kDestroyed |
          kConstructed |
          kFinished |
          kWriting |
          kErrorEmitted |
          kCloseEmitted |
          kErrored |
          kBuffered
    )) === (kEnding | kConstructed) && state.length === 0);
}

function onFinish(stream, state, err) {
  if ((state[kState] & kPrefinished) !== 0) {
    errorOrDestroy(stream, err ?? new ERR_MULTIPLE_CALLBACK());
    return;
  }
  state.pendingcb--;
  if (err) {
    callFinishedCallbacks(state, err);
    errorOrDestroy(stream, err, (state[kState] & kSync) !== 0);
  } else if (needFinish(state)) {
    state[kState] |= kPrefinished;
    stream.emit('prefinish');
    // Backwards compat. Don't check state.sync here.
    // Some streams assume 'finish' will be emitted
    // asynchronously relative to _final callback.
    state.pendingcb++;
    process.nextTick(finish, stream, state);
  }
}

function prefinish(stream, state) {
  if ((state[kState] & (kPrefinished | kFinalCalled)) !== 0) {
    return;
  }

  if (typeof stream._final === 'function' && (state[kState] & kDestroyed) === 0) {
    state[kState] |= kFinalCalled | kSync;
    state.pendingcb++;

    try {
      stream._final((err) => onFinish(stream, state, err));
    } catch (err) {
      onFinish(stream, state, err);
    }

    state[kState] &= ~kSync;
  } else {
    state[kState] |= kFinalCalled | kPrefinished;
    stream.emit('prefinish');
  }
}

function finishMaybe(stream, state, sync) {
  if (needFinish(state)) {
    prefinish(stream, state);
    if (state.pendingcb === 0) {
      if (sync) {
        state.pendingcb++;
        process.nextTick((stream, state) => {
          if (needFinish(state)) {
            finish(stream, state);
          } else {
            state.pendingcb--;
          }
        }, stream, state);
      } else if (needFinish(state)) {
        state.pendingcb++;
        finish(stream, state);
      }
    }
  }
}

function finish(stream, state) {
  state.pendingcb--;
  state[kState] |= kFinished;

  callFinishedCallbacks(state, null);

  stream.emit('finish');

  if ((state[kState] & kAutoDestroy) !== 0) {
    // In case of duplex streams we need a way to detect
    // if the readable side is ready for autoDestroy as well.
    const rState = stream._readableState;
    const autoDestroy = !rState || (
      rState.autoDestroy &&
      // We don't expect the readable to ever 'end'
      // if readable is explicitly set to false.
      (rState.endEmitted || rState.readable === false)
    );
    if (autoDestroy) {
      stream.destroy();
    }
  }
}

function callFinishedCallbacks(state, err) {
  if ((state[kState] & kOnFinished) === 0) {
    return;
  }

  const onfinishCallbacks = state[kOnFinishedValue];
  state[kOnFinishedValue] = null;
  state[kState] &= ~kOnFinished;
  for (let i = 0; i < onfinishCallbacks.length; i++) {
    onfinishCallbacks[i](err);
  }
}

ObjectDefineProperties(Writable.prototype, {
  closed: {
    __proto__: null,
    get() {
      return this._writableState ? (this._writableState[kState] & kClosed) !== 0 : false;
    },
  },

  destroyed: {
    __proto__: null,
    get() {
      return this._writableState ? (this._writableState[kState] & kDestroyed) !== 0 : false;
    },
    set(value) {
      // Backward compatibility, the user is explicitly managing destroyed.
      if (!this._writableState) return;

      if (value) this._writableState[kState] |= kDestroyed;
      else this._writableState[kState] &= ~kDestroyed;
    },
  },

  writable: {
    __proto__: null,
    get() {
      const w = this._writableState;
      // w.writable === false means that this is part of a Duplex stream
      // where the writable side was disabled upon construction.
      // Compat. The user might manually disable writable side through
      // deprecated setter.
      return !!w && w.writable !== false &&
        (w[kState] & (kEnding | kEnded | kDestroyed | kErrored)) === 0;
    },
    set(val) {
      // Backwards compatible.
      if (this._writableState) {
        this._writableState.writable = !!val;
      }
    },
  },

  writableFinished: {
    __proto__: null,
    get() {
      const state = this._writableState;
      return state ? (state[kState] & kFinished) !== 0 : false;
    },
  },

  writableObjectMode: {
    __proto__: null,
    get() {
      const state = this._writableState;
      return state ? (state[kState] & kObjectMode) !== 0 : false;
    },
  },

  writableBuffer: {
    __proto__: null,
    get() {
      const state = this._writableState;
      return state && state.getBuffer();
    },
  },

  writableEnded: {
    __proto__: null,
    get() {
      const state = this._writableState;
      return state ? (state[kState] & kEnding) !== 0 : false;
    },
  },

  writableNeedDrain: {
    __proto__: null,
    get() {
      const state = this._writableState;
      return state ? (state[kState] & (kDestroyed | kEnding | kNeedDrain)) === kNeedDrain : false;
    },
  },

  writableHighWaterMark: {
    __proto__: null,
    get() {
      const state = this._writableState;
      return state?.highWaterMark;
    },
  },

  writableCorked: {
    __proto__: null,
    get() {
      const state = this._writableState;
      return state ? state.corked : 0;
    },
  },

  writableLength: {
    __proto__: null,
    get() {
      const state = this._writableState;
      return state?.length;
    },
  },

  errored: {
    __proto__: null,
    enumerable: false,
    get() {
      const state = this._writableState;
      return state ? state.errored : null;
    },
  },

  writableAborted: {
    __proto__: null,
    get: function() {
      const state = this._writableState;
      return (
        (state[kState] & (kHasWritable | kWritable)) !== kHasWritable &&
        (state[kState] & (kDestroyed | kErrored)) !== 0 &&
        (state[kState] & kFinished) === 0
      );
    },
  },
});

const destroy = destroyImpl.destroy;
Writable.prototype.destroy = function(err, cb) {
  const state = this._writableState;

  // Invoke pending callbacks.
  if ((state[kState] & (kBuffered | kOnFinished)) !== 0 && (state[kState] & kDestroyed) === 0) {
    process.nextTick(errorBuffer, state);
  }

  destroy.call(this, err, cb);
  return this;
};

Writable.prototype._undestroy = destroyImpl.undestroy;
Writable.prototype._destroy = function(err, cb) {
  cb(err);
};

Writable.prototype[EE.captureRejectionSymbol] = function(err) {
  this.destroy(err);
};

let webStreamsAdapters;

// Lazy to avoid circular references
function lazyWebStreams() {
  if (webStreamsAdapters === undefined)
    webStreamsAdapters = require('internal/webstreams/adapters');
  return webStreamsAdapters;
}

Writable.fromWeb = function(writableStream, options) {
  return lazyWebStreams().newStreamWritableFromWritableStream(
    writableStream,
    options);
};

Writable.toWeb = function(streamWritable) {
  return lazyWebStreams().newWritableStreamFromStreamWritable(streamWritable);
};

Writable.prototype[SymbolAsyncDispose] = async function() {
  let error;
  if (!this.destroyed) {
    error = this.writableFinished ? null : new AbortError();
    this.destroy(error);
  }
  await new Promise((resolve, reject) =>
    eos(this, (err) => (err && err.name !== 'AbortError' ? reject(err) : resolve(null))),
  );
};

export default module.exports;
