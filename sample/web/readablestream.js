// ReadableStream: enqueue + for-await collection.
// ReadableStream：入列与 for-await 收集。
// Run / 运行: winterjs --run sample/web/readablestream.js
const rs = new ReadableStream({
  start(c) {
    c.enqueue('a');
    c.enqueue('b');
    c.close();
  },
});
let collected = '';
for await (const chunk of rs) collected += chunk;
console.log('[readablestream] collected:', collected === 'ab');
console.log('[readablestream] strategies:', new CountQueuingStrategy({ highWaterMark: 4 }).highWaterMark === 4, new ByteLengthQueuingStrategy({ highWaterMark: 16 }).highWaterMark === 16);
