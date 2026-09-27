// node:stream/promises + consumers + web: finished / text / json / web stream.
// 流扩展：promises 完成等待、消费者快捷读法与 Web 流互转。
// Run / 运行: winterjs --run sample/stream/extras.js
import { finished } from 'node:stream/promises';
import { text, json } from 'node:stream/consumers';
import { Readable } from 'node:stream';

console.log('[stream-x] text:', (await text(Readable.from(['a', 'b']))) === 'ab');
console.log('[stream-x] json:', (await json(Readable.from(['{"k":9}']))).k === 9);

const rs = Readable.from(['done']);
await finished(rs.pipe(new (await import('node:stream')).Writable({
  write(_c, _e, cb) {
    cb();
  },
})));
console.log('[stream-x] finished ok');
