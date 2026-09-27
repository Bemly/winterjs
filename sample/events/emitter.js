// node:events: on / once / emit / off / error routing / maxListeners.
// 事件发射器：监听、一次、注销、错误路由与上限。
// Run / 运行: winterjs --run sample/events/emitter.js
import { EventEmitter } from 'node:events';

const ee = new EventEmitter();
let count = 0;
const inc = (n) => {
  count += n;
};
ee.on('add', inc);
ee.emit('add', 2);
ee.emit('add', 3);
console.log('[events] count:', count === 5, 'names:', ee.eventNames().join(','));

ee.once('once', () => console.log('[events] once fired'));
ee.emit('once');
ee.emit('once');

ee.off('add', inc);
ee.emit('add', 100);
console.log('[events] after off:', count === 5);

ee.on('error', (e) => console.log('[events] caught:', e.message));
ee.emit('error', new Error('boom-err'));

ee.setMaxListeners(20);
console.log('[events] max:', ee.getMaxListeners() === 20);
