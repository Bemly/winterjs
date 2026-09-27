// node:stream: Readable / Writable / Duplex / Transform / pipeline.
// 流：四类流与管道（含 for-await 消费）。
// Run / 运行: winterjs --run sample/stream/basics.js
import { Readable, Writable, Transform, pipeline } from 'node:stream';

const rs = Readable.from(['x', 'y', 'z']);
let got = '';
for await (const c of rs) got += c;
console.log('[stream] from+for-await:', got === 'xyz');

const chunks = [];
const ws = new Writable({
  write(c, _e, cb) {
    chunks.push(String(c));
    cb();
  },
});
ws.write('a');
ws.end('b');
await new Promise((r) => ws.on('finish', r));
console.log('[stream] writable:', chunks.join('') === 'ab');

const up = new Transform({
  transform(c, _e, cb) {
    cb(null, String(c).toUpperCase());
  },
});
let out = '';
up.on('data', (c) => {
  out += c;
});
up.write('hi');
up.end();
await new Promise((r) => up.on('end', r));
console.log('[stream] transform:', out === 'HI');

await new Promise((resolve, reject) => {
  const pass = new Transform({
    transform(c, _e, cb) {
      cb(null, c);
    },
  });
  pipeline(Readable.from(['p']), pass, (err) => (err ? reject(err) : resolve()));
});
console.log('[stream] pipeline ok');
