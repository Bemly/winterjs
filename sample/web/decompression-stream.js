// DecompressionStream: streaming decompression round-trip.
// DecompressionStream：流式解压往返。
// Run / 运行: winterjs --run sample/web/decompression-stream.js
async function roundtrip(format, text) {
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
  const ds = new DecompressionStream(format);
  const dw = ds.writable.getWriter();
  dw.write(packed);
  dw.close();
  let out = '';
  for await (const chunk of ds.readable) out += new TextDecoder().decode(chunk);
  return out;
}
const src = 'hello compression '.repeat(20);
console.log('[decompression-stream] gzip ok:', (await roundtrip('gzip', src)) === src);
console.log('[decompression-stream] deflate ok:', (await roundtrip('deflate', src)) === src);
console.log('[decompression-stream] zstd ok:', (await roundtrip('zstd', src)) === src);
console.log('[decompression-stream] ns:', WinterJS.DecompressionStream === DecompressionStream);
