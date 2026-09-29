// WritableStream: write + close, pipeTo target.
// WritableStream：写入与关闭，可作 pipeTo 目标。
// Run / 运行: winterjs --run sample/web/writablestream.js
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
console.log('[writablestream] piped chunks:', count === 3);
const w = new WritableStream({ close() { count += 10; } });
await w.getWriter().close();
console.log('[writablestream] close hook:', count === 13);
