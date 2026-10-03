import { primordials } from 'node:internal/primordials';
import * as __m0 from 'node:internal/errors';
import * as __m1 from 'node:internal/util';
import * as __m2 from 'node:internal/validators';
import * as __m3 from 'node:internal/util/types';
import * as __m4 from 'node:internal/abort_controller';
import * as __m5 from 'node:internal/streams/iter_from';
import * as __m6 from 'node:internal/streams/iter_utils';
import * as __m7 from 'node:internal/streams/iter_types';

// CJS require 垫片（静态 spec → 内建模块 default 导出；循环依赖经懒访问解环）
const require = (spec) => __requireMap(spec);
function __requireMap(spec) {
  switch (spec) {
    case 'internal/errors': return __m0.default;
    case 'internal/util': return __m1.default;
    case 'internal/validators': return __m2.default;
    case 'internal/util/types': return __m3.default;
    case 'internal/abort_controller': return __m4.default;
    case 'internal/streams/iter/from': return __m5.default;
    case 'internal/streams/iter/utils': return __m6.default;
    case 'internal/streams/iter/types': return __m7.default;
  default: throw new Error('unmapped internal require: ' + spec);
  }
}

const module = { exports: { __proto__: null } };

'use strict';

// New Streams API - Pull Pipeline
//
// pull(), pullSync(), pipeTo(), pipeToSync()
// Pull-through pipelines with transforms. Data flows on-demand from source
// through transforms to consumer.

const {
  ArrayBufferIsView,
  ArrayFromAsync,
  ArrayIsArray,
  ArrayPrototypePush,
  ArrayPrototypeSlice,
  PromisePrototypeThen,
  PromiseResolve,
  SymbolAsyncIterator,
  SymbolIterator,
  TypedArrayPrototypeGetByteLength,
  Uint8Array,
} = primordials;

const {
  codes: {
    ERR_INVALID_ARG_TYPE,
    ERR_INVALID_ARG_VALUE,
    ERR_OUT_OF_RANGE,
  },
} = require('internal/errors');
const { lazyDOMException } = require('internal/util');
const { validateAbortSignal } = require('internal/validators');
const {
  isAnyArrayBuffer,
  isPromise,
  isUint8Array,
} = require('internal/util/types');
const { AbortController } = require('internal/abort_controller');

const {
  arrayBufferViewToUint8Array,
  from,
  fromSync,
  isSyncIterable,
  isAsyncIterable,
  isPrimitiveChunk,
  isUint8ArrayBatch,
  normalizeAsyncValue,
} = require('internal/streams/iter/from');

const {
  isPullOptions,
  isTransform,
  isTransformObject,
  parsePullArgs,
  toUint8Array,
  wrapError,
  yieldAbortable,
} = require('internal/streams/iter/utils');

const {
  kValidatedSource,
  kValidatedTransform,
  toAsyncStreamable,
  toStreamable,
} = require('internal/streams/iter/types');

// =============================================================================
// Type Guards and Helpers
// =============================================================================

/**
 * Check if a value is a Writer (has write method).
 * @returns {boolean}
 */
function hasMethod(value, name) {
  return typeof value?.[name] === 'function';
}

/**
 * Parse pipeTo/pipeToSync arguments: [...transforms, writer, options?]
 * @param {Array} args
 * @param {string} requiredMethod - 'write' for pipeTo, 'writeSync' for pipeToSync
 * @returns {{ transforms: Array, writer: object, options: object }}
 */
function parsePipeToArgs(args, requiredMethod) {
  if (args.length === 0) {
    throw new ERR_INVALID_ARG_VALUE('args', args, 'pipeTo requires a writer argument');
  }

  let options;
  let writerIndex = args.length - 1;

  // Check if last arg is options
  const last = args[args.length - 1];
  if (isPullOptions(last) && !hasMethod(last, requiredMethod)) {
    options = last;
    writerIndex = args.length - 2;
  }

  if (writerIndex < 0) {
    throw new ERR_INVALID_ARG_VALUE('args', args, 'pipeTo requires a writer argument');
  }

  const writer = args[writerIndex];
  if (!hasMethod(writer, requiredMethod)) {
    throw new ERR_INVALID_ARG_TYPE(
      'writer', `object with a ${requiredMethod} method`, writer);
  }

  const transforms = ArrayPrototypeSlice(args, 0, writerIndex);
  for (let i = 0; i < transforms.length; i++) {
    if (!isTransform(transforms[i])) {
      throw new ERR_INVALID_ARG_TYPE(
        `transforms[${i}]`, ['Function', 'Object with transform()'],
        transforms[i]);
    }
  }

  return {
    __proto__: null,
    transforms,
    writer,
    options,
  };
}

function canUseSyncIterablePipeToFastPath(source, transforms, signal) {
  if (signal !== undefined ||
      transforms.length !== 0 ||
      isPrimitiveChunk(source) ||
      ArrayIsArray(source) ||
      source?.[kValidatedSource] ||
      !isSyncIterable(source) ||
      isAsyncIterable(source)) {
    return false;
  }

  // Preserve from()'s top-level protocol precedence for custom iterables.
  return typeof source[toAsyncStreamable] !== 'function' &&
    typeof source[toStreamable] !== 'function';
}

// =============================================================================
// Transform Output Flattening
// =============================================================================

/**
 * Flatten transform yield to Uint8Array chunks (sync).
 * @yields {Uint8Array}
 */
function* flattenTransformYieldSync(value) {
  if (isUint8Array(value)) {
    yield value;
    return;
  }
  if (typeof value === 'string') {
    yield toUint8Array(value);
    return;
  }
  if (isAnyArrayBuffer(value)) {
    yield new Uint8Array(value);
    return;
  }
  if (ArrayBufferIsView(value)) {
    yield arrayBufferViewToUint8Array(value);
    return;
  }
  // Must be Iterable<TransformYield>
  if (isSyncIterable(value)) {
    for (const item of value) {
      yield* flattenTransformYieldSync(item);
    }
    return;
  }
  throw new ERR_INVALID_ARG_TYPE(
    'value',
    ['Uint8Array', 'string', 'ArrayBuffer', 'ArrayBufferView', 'Iterable'],
    value);
}

/**
 * Flatten transform yield to Uint8Array chunks (async).
 * @yields {Uint8Array}
 */
async function* flattenTransformYieldAsync(value) {
  if (isUint8Array(value)) {
    yield value;
    return;
  }
  if (typeof value === 'string') {
    yield toUint8Array(value);
    return;
  }
  if (isAnyArrayBuffer(value)) {
    yield new Uint8Array(value);
    return;
  }
  if (ArrayBufferIsView(value)) {
    yield arrayBufferViewToUint8Array(value);
    return;
  }
  // Check for async iterable first
  if (isAsyncIterable(value)) {
    for await (const item of value) {
      yield* flattenTransformYieldAsync(item);
    }
    return;
  }
  // Must be sync Iterable<TransformYield>, no nested async iterables
  if (isSyncIterable(value)) {
    for (const item of value) {
      yield* flattenTransformYieldSync(item);
    }
    return;
  }
  throw new ERR_INVALID_ARG_TYPE(
    'value',
    ['Uint8Array', 'string', 'ArrayBuffer', 'ArrayBufferView',
     'Iterable', 'AsyncIterable'],
    value);
}

/**
 * Process transform result (sync).
 * @yields {Uint8Array[]}
 */
function* processTransformResultSync(result) {
  if (result === null) {
    return;
  }
  // Single Uint8Array -> wrap as batch
  if (isUint8Array(result)) {
    yield [result];
    return;
  }
  // String -> UTF-8 encode and wrap as batch
  if (typeof result === 'string') {
    yield [toUint8Array(result)];
    return;
  }
  // ArrayBuffer / ArrayBufferView -> convert and wrap
  if (isAnyArrayBuffer(result)) {
    yield [new Uint8Array(result)];
    return;
  }
  if (ArrayBufferIsView(result)) {
    yield [arrayBufferViewToUint8Array(result)];
    return;
  }
  // Uint8Array[] batch
  if (isUint8ArrayBatch(result)) {
    if (result.length > 0) {
      yield result;
    }
    return;
  }
  // Iterable or Generator
  if (isSyncIterable(result)) {
    const batch = [];
    for (const item of result) {
      for (const chunk of flattenTransformYieldSync(item)) {
        ArrayPrototypePush(batch, chunk);
      }
    }
    if (batch.length > 0) {
      yield batch;
    }
    return;
  }
  throw new ERR_INVALID_ARG_TYPE(
    'result',
    ['null', 'Uint8Array', 'string', 'ArrayBuffer',
     'ArrayBufferView', 'Array', 'Iterable'],
    result);
}

/**
 * Append normalized transform result batches to an array (sync).
 * @param {Array<Uint8Array[]>} target
 * @param {*} result
 */
function appendTransformResultSync(target, result) {
  if (result === null) {
    return;
  }
  if (isUint8ArrayBatch(result)) {
    if (result.length > 0) {
      ArrayPrototypePush(target, result);
    }
    return;
  }
  if (isUint8Array(result)) {
    ArrayPrototypePush(target, [result]);
    return;
  }
  if (typeof result === 'string') {
    ArrayPrototypePush(target, [toUint8Array(result)]);
    return;
  }
  if (isAnyArrayBuffer(result)) {
    ArrayPrototypePush(target, [new Uint8Array(result)]);
    return;
  }
  if (ArrayBufferIsView(result)) {
    ArrayPrototypePush(target, [arrayBufferViewToUint8Array(result)]);
    return;
  }
  for (const batch of processTransformResultSync(result)) {
    ArrayPrototypePush(target, batch);
  }
}

/**
 * Process transform result (async).
 * @yields {Uint8Array[]}
 */
async function* processTransformResultAsync(result) {
  // Handle Promise
  if (isPromise(result)) {
    const resolved = await result;
    yield* processTransformResultAsync(resolved);
    return;
  }
  if (result === null) {
    return;
  }
  // Single Uint8Array -> wrap as batch
  if (isUint8Array(result)) {
    yield [result];
    return;
  }
  // String -> UTF-8 encode and wrap as batch
  if (typeof result === 'string') {
    yield [toUint8Array(result)];
    return;
  }
  // ArrayBuffer / ArrayBufferView -> convert and wrap
  if (isAnyArrayBuffer(result)) {
    yield [new Uint8Array(result)];
    return;
  }
  if (ArrayBufferIsView(result)) {
    yield [arrayBufferViewToUint8Array(result)];
    return;
  }
  // Uint8Array[] batch
  if (isUint8ArrayBatch(result)) {
    if (result.length > 0) {
      yield result;
    }
    return;
  }
  // Check for async iterable/generator first
  if (isAsyncIterable(result)) {
    const batch = [];
    for await (const item of result) {
      if (isUint8Array(item)) {
        ArrayPrototypePush(batch, item);
        continue;
      }
      for await (const chunk of flattenTransformYieldAsync(item)) {
        ArrayPrototypePush(batch, chunk);
      }
    }
    if (batch.length > 0) {
      yield batch;
    }
    return;
  }
  // Sync Iterable or Generator
  if (isSyncIterable(result)) {
    const batch = [];
    for (const item of result) {
      if (isUint8Array(item)) {
        ArrayPrototypePush(batch, item);
        continue;
      }
      // Note: This iteration is synchronous, since async iterables
      // may not be nested within sync iterables.
      for (const chunk of flattenTransformYieldSync(item)) {
        ArrayPrototypePush(batch, chunk);
      }
    }
    if (batch.length > 0) {
      yield batch;
    }
    return;
  }
  throw new ERR_INVALID_ARG_TYPE(
    'result',
    ['null', 'Uint8Array', 'string', 'ArrayBuffer',
     'ArrayBufferView', 'Array', 'Iterable', 'AsyncIterable', 'Promise'],
    result);
}

