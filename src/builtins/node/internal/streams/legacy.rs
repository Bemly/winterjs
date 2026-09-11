//! `node:internal/streams/legacy`（Node lib/internal/streams/legacy.js 逐字内嵌，MIT）。
/// 来源：nodejs/node `internal/streams/legacy.js`（MIT 头见源内）逐字内嵌；require → 垫片映射，
/// primordials → node:internal/primordials。偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"import { primordials } from 'node:internal/primordials';
import * as __m0 from 'node:events';

// CJS require 垫片（静态 spec → 内建模块 default 导出；循环依赖经懒访问解环）
const require = (spec) => __requireMap(spec);
function __requireMap(spec) {
  switch (spec) {
    case 'events': return __m0.default;
  default: throw new Error('unmapped internal require: ' + spec);
  }
}

const module = { exports: { __proto__: null } };

'use strict';

const {
  ArrayIsArray,
  ObjectSetPrototypeOf,
  ReflectOwnKeys,
} = primordials;

const EE = require('events');

function Stream(opts) {
  EE.call(this, opts);
}
ObjectSetPrototypeOf(Stream.prototype, EE.prototype);
ObjectSetPrototypeOf(Stream, EE);

Stream.prototype.pipe = function(dest, options) {
  const source = this;

  function ondata(chunk) {
    if (dest.writable && dest.write(chunk) === false && source.pause) {
      source.pause();
    }
  }

  source.on('data', ondata);

  function ondrain() {
    if (source.readable && source.resume) {
      source.resume();
    }
  }

  dest.on('drain', ondrain);

  // If the 'end' option is not supplied, dest.end() will be called when
  // source gets the 'end' or 'close' events.  Only dest.end() once.
  if (!dest._isStdio && (!options || options.end !== false)) {
    source.on('end', onend);
    source.on('close', onclose);
  }

  let didOnEnd = false;
  function onend() {
    if (didOnEnd) return;
    didOnEnd = true;

    dest.end();
  }


  function onclose() {
    if (didOnEnd) return;
    didOnEnd = true;

    if (typeof dest.destroy === 'function') dest.destroy();
  }

  // Don't leave dangling pipes when there are errors.
  function onerror(er) {
    cleanup();
    // If we removed the last error handler, trigger an unhandled error event.
    if (this.listenerCount?.('error') === 0) {
      this.emit('error', er);
    }
  }

  prependListener(source, 'error', onerror);
  prependListener(dest, 'error', onerror);

  // Remove all the event listeners that were added.
  function cleanup() {
    source.removeListener('data', ondata);
    dest.removeListener('drain', ondrain);

    source.removeListener('end', onend);
    source.removeListener('close', onclose);

    source.removeListener('error', onerror);
    dest.removeListener('error', onerror);

    source.removeListener('end', cleanup);
    source.removeListener('close', cleanup);

    dest.removeListener('close', cleanup);
  }

  source.on('end', cleanup);
  source.on('close', cleanup);

  dest.on('close', cleanup);
  dest.emit('pipe', source);

  // Allow for unix-like usage: A.pipe(B).pipe(C)
  return dest;
};

Stream.prototype.eventNames = function eventNames() {
  const names = [];
  for (const key of ReflectOwnKeys(this._events)) {
    if (typeof this._events[key] === 'function' || (ArrayIsArray(this._events[key]) && this._events[key].length > 0)) {
      names.push(key);
    }
  }
  return names;
};

function prependListener(emitter, event, fn) {
  // Sadly this is not cacheable as some libraries bundle their own
  // event emitter implementation with them.
  if (typeof emitter.prependListener === 'function')
    return emitter.prependListener(event, fn);

  // This is a hack to make sure that our error handler is attached before any
  // userland ones.  NEVER DO THIS. This is here only because this code needs
  // to continue to work with older versions of Node.js that do not include
  // the prependListener() method. The goal is to eventually remove this hack.
  if (!emitter._events || !emitter._events[event])
    emitter.on(event, fn);
  else if (ArrayIsArray(emitter._events[event]))
    emitter._events[event].unshift(fn);
  else
    emitter._events[event] = [fn, emitter._events[event]];
}

module.exports = { Stream, prependListener };

export default module.exports;
"#;
