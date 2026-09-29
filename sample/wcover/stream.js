// WinterJS2.stream: stream pipelining.
// WinterJS2.stream：流管道。
// Run / 运行: winterjs2 --run sample/wcover/stream.js
const rs = new ReadableStream({ start(c) { c.enqueue(new TextEncoder().encode('hi')); c.close(); } });
let out = '';
await WinterJS2.stream.pipeline(rs, new WritableStream({ write(c) { out += new TextDecoder().decode(c); } }));
console.log('[stream] pipeline:', out === 'hi');
