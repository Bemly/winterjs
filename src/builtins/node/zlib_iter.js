import { primordials } from 'node:internal/primordials';
import * as __m0 from 'node:internal/util';
import * as __m1 from 'node:internal/streams/iter_transform';

// CJS require 垫片（静态 spec → 内建模块 default 导出；循环依赖经懒访问解环）
const require = (spec) => __requireMap(spec);
function __requireMap(spec) {
  switch (spec) {
    case 'internal/util': return __m0.default;
    case 'internal/streams/iter/transform': return __m1.default;
  default: throw new Error('unmapped internal require: ' + spec);
  }
}

const module = { exports: { __proto__: null } };

'use strict';

// Public entry point for the iterable compression/decompression API.
// Usage: require('zlib/iter') or require('node:zlib/iter')
// Requires: --experimental-stream-iter

const { emitExperimentalWarning } = require('internal/util');
emitExperimentalWarning('zlib/iter');

const {
  compressGzip,
  compressGzipSync,
  compressDeflate,
  compressDeflateSync,
  compressBrotli,
  compressBrotliSync,
  compressZstd,
  compressZstdSync,
  decompressGzip,
  decompressGzipSync,
  decompressDeflate,
  decompressDeflateSync,
  decompressBrotli,
  decompressBrotliSync,
  decompressZstd,
  decompressZstdSync,
} = require('internal/streams/iter/transform');

module.exports = {
  // Compression transforms (async)
  compressGzip,
  compressDeflate,
  compressBrotli,
  compressZstd,

  // Compression transforms (sync)
  compressGzipSync,
  compressDeflateSync,
  compressBrotliSync,
  compressZstdSync,

  // Decompression transforms (async)
  decompressGzip,
  decompressDeflate,
  decompressBrotli,
  decompressZstd,

  // Decompression transforms (sync)
  decompressGzipSync,
  decompressDeflateSync,
  decompressBrotliSync,
  decompressZstdSync,
};

export { compressGzip, compressDeflate, compressBrotli, compressZstd, compressGzipSync, compressDeflateSync, compressBrotliSync, compressZstdSync, decompressGzip, decompressDeflate, decompressBrotli, decompressZstd, decompressGzipSync, decompressDeflateSync, decompressBrotliSync, decompressZstdSync };
export default module.exports;
