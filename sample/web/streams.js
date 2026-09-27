// Web Streams: ReadableStream / WritableStream / TransformStream + pipe.
// Web 流：可读/可写/转换流与管道。
// Run / 运行: winterjs --run sample/web/streams.js
const rs = new ReadableStream({
  start(c) {
    c.enqueue('a');
    c.enqueue('b');
    c.close();
  },
});
let collected = '';
for await (const chunk of rs) collected += chunk;
console.log('[streams] collected:', collected);

const upper = new TransformStream({
  transform(chunk, c) {
    c.enqueue(String(chunk).toUpperCase());
  },
});
const w = upper.writable.getWriter();
w.write('hello ');
w.write('stream');
w.close();
let text = '';
for await (const chunk of upper.readable) text += chunk;
console.log('[streams] transformed:', text);

// pipeTo with a counting writable.
let count = 0;
const counter = new WritableStream({
  write() {
    count += 1;
  },
});
await new ReadableStream({
  start(c) {
    c.enqueue(1);
    c.enqueue(2);
    c.enqueue(3);
    c.close();
  },
}).pipeTo(counter);
console.log('[streams] piped chunks:', count);

console.log(
  '[streams] strategies:',
  new CountQueuingStrategy({ highWaterMark: 4 }).highWaterMark,
  new ByteLengthQueuingStrategy({ highWaterMark: 16 }).highWaterMark,
);
