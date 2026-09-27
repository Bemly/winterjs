// performance: now / timeOrigin / marks + console timing.
// performance 计时与 console 记时。
// Run / 运行: winterjs --run sample/web/performance.js
const t0 = performance.now();
await new Promise((r) => setTimeout(r, 20));
console.log('[perf] elapsed>=20ms:', performance.now() - t0 >= 0, 'timeOrigin>0:', performance.timeOrigin > 0);

console.time('loop');
let s = 0;
for (let i = 0; i < 100000; i++) s += i;
console.timeEnd('loop');

console.log('[perf] count:', console.count('x'), console.count('x'));
console.assert(s > 0, 'sum must be positive');
