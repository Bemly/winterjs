// Global process: the runtime process object (argv/env/versions/timers).
// 全局 process：运行时进程对象（参数/环境/版本/计时器）。
// Run / 运行: winterjs --run sample/process/global.js
console.log('[process] argv0:', process.argv[0].length > 0, 'platform:', process.platform);
console.log('[process] versions.node:', typeof process.versions.node === 'string');
console.log('[process] hrtime:', typeof process.hrtime.bigint() === 'bigint' && process.hrtime.bigint() >= 0n);
const order = [];
process.nextTick(() => order.push('nextTick'));
setImmediate(() => {
  order.push('immediate');
  console.log('[process] tick order:', order.join('>') === 'nextTick>immediate');
});
