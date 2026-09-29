// node:diagnostics_channel + async_hooks + perf_hooks + v8 + domain + tty.
// 可观测与遗留面：通道订阅、ALS、性能钩子、堆统计、domain 与 tty 判定。
// Run / 运行: winterjs2 --run sample/observe/basics.js
import { channel } from 'node:diagnostics_channel';
import { AsyncLocalStorage } from 'node:async_hooks';
import { performance } from 'node:perf_hooks';
import v8 from 'node:v8';
import domain from 'node:domain';
import tty from 'node:tty';

const ch = channel('wjs.demo');
ch.subscribe((msg) => console.log('[observe] channel got:', msg.n));
ch.publish({ n: 7 });

const als = new AsyncLocalStorage();
als.run({ req: 1 }, () => {
  setImmediate(() => console.log('[observe] als store:', als.getStore().req === 1));
});

const t0 = performance.now();
console.log('[observe] perf now number:', typeof t0 === 'number');

// node:v8 is a startupSnapshot-only bridge (heap numbers are engine-specific,
// see sample/docs/api.*): the real surface is isBuildingSnapshot().
console.log('[observe] snapshot building:', v8.startupSnapshot.isBuildingSnapshot() === false);

const d = domain.create();
d.on('error', (e) => console.log('[observe] domain caught:', e.message));
d.run(() => {
  throw new Error('domain-boom');
});

console.log('[observe] stdin isTTY:', tty.isatty(0) === false || typeof tty.isatty(0) === 'boolean');
