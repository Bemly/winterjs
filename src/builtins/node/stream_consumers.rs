//! `node:stream/consumers`（Node lib/stream/consumers.js 逐字内嵌，MIT）。
/// 来源：nodejs/node `stream/consumers.js`（MIT 头见源内）逐字内嵌；require → 垫片映射，
/// primordials → node:internal/primordials。偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"import { primordials } from 'node:internal/primordials';
import * as __m0 from 'node:buffer';
import * as __m1 from 'node:internal/blob';
import * as __m2 from 'node:internal/encoding';

// CJS require 垫片（静态 spec → 内建模块 default 导出；循环依赖经懒访问解环）
const require = (spec) => __requireMap(spec);
function __requireMap(spec) {
  switch (spec) {
    case 'buffer': return __m0.default;
    case 'internal/blob': return __m1.default;
    case 'internal/encoding': return __m2.default;
  default: throw new Error('unmapped internal require: ' + spec);
  }
}

const module = { exports: { __proto__: null } };

'use strict';

const {
  JSONParse,
  Uint8Array,
} = primordials;

const {
  TextDecoder,
} = require('internal/encoding');

const {
  Blob,
} = require('internal/blob');

const {
  Buffer,
} = require('buffer');

/**
 * @typedef {import('../internal/webstreams/readablestream').ReadableStream
 * } ReadableStream
 * @typedef {import('../internal/streams/readable')} Readable
 */

/**
 * @param {AsyncIterable|ReadableStream|Readable} stream
 * @returns {Promise<Blob>}
 */
async function blob(stream) {
  const chunks = [];
  for await (const chunk of stream)
    chunks.push(chunk);
  return new Blob(chunks);
}

/**
 * @param {AsyncIterable|ReadableStream|Readable} stream
 * @returns {Promise<ArrayBuffer>}
 */
async function arrayBuffer(stream) {
  const ret = await blob(stream);
  return ret.arrayBuffer();
}

/**
 * @param {AsyncIterable|ReadableStream|Readable} stream
 * @returns {Promise<Buffer>}
 */
async function buffer(stream) {
  return Buffer.from(await arrayBuffer(stream));
}

/**
 * @param {AsyncIterable|ReadableStream|Readable} stream
 * @returns {Promise<Uint8Array>}
 */
async function bytes(stream) {
  return new Uint8Array(await arrayBuffer(stream));
}

/**
 * @param {AsyncIterable|ReadableStream|Readable} stream
 * @returns {Promise<string>}
 */
async function text(stream) {
  const dec = new TextDecoder();
  let str = '';
  for await (const chunk of stream) {
    if (typeof chunk === 'string')
      str += chunk;
    else
      str += dec.decode(chunk, { stream: true });
  }
  // Flush the streaming TextDecoder so that any pending
  // incomplete multibyte characters are handled.
  str += dec.decode(undefined, { stream: false });
  return str;
}

/**
 * @param {AsyncIterable|ReadableStream|Readable} stream
 * @returns {Promise<any>}
 */
async function json(stream) {
  const str = await text(stream);
  return JSONParse(str);
}

module.exports = {
  arrayBuffer,
  blob,
  buffer,
  bytes,
  text,
  json,
};

export { buffer, text, arrayBuffer, json, blob, bytes };
export default module.exports;
"#;
