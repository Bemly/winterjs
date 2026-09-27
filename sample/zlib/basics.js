// node:zlib: gzip / deflate / brotli round-trips + crc32.
// 压缩：三种格式往返与 crc32。
// Run / 运行: winterjs --run sample/zlib/basics.js
import { gzipSync, gunzipSync, deflateSync, inflateSync, brotliCompressSync, brotliDecompressSync, crc32 } from 'node:zlib';

const src = Buffer.from('hello zlib '.repeat(50));
console.log('[zlib] gzip:', gunzipSync(gzipSync(src)).equals(src));
console.log('[zlib] deflate:', inflateSync(deflateSync(src)).equals(src));
console.log('[zlib] brotli:', brotliDecompressSync(brotliCompressSync(src)).equals(src));
console.log('[zlib] crc32 nonzero:', crc32(src) !== 0);
