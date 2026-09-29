// CompressionStream: streaming compression (gzip/deflate + zstd extension).
// CompressionStream：流式压缩（gzip/deflate + winterjs2 扩展 zstd）。
// Run / 运行: winterjs2 --run sample/web/compression-stream.js
async function compress(format, text) {
  const cs = new CompressionStream(format);
  const writer = cs.writable.getWriter();
  writer.write(new TextEncoder().encode(text));
  writer.close();
  const chunks = [];
  for await (const c of cs.readable) chunks.push(c);
  const total = chunks.reduce((n, c) => n + c.length, 0);
  const packed = new Uint8Array(total);
  let off = 0;
  for (const c of chunks) {
    packed.set(c, off);
    off += c.length;
  }
  return packed;
}
const src = 'hello compression '.repeat(20);
console.log('[compression-stream] gzip bytes:', (await compress('gzip', src)).length > 0);
console.log('[compression-stream] deflate bytes:', (await compress('deflate', src)).length > 0);
console.log('[compression-stream] ns:', WinterJS2.CompressionStream === CompressionStream);
