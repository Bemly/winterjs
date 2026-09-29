// node:process: argv / env / cwd / versions / hrtime / nextTick.
// 进程：参数、环境、版本、高精度时间与 nextTick。
// Run / 运行: winterjs2 --run sample/process/basics.js -- hello --flag
console.log('[process] argv0 is winterjs2:', process.argv[0].length > 0);
console.log('[process] execPath:', typeof process.execPath);
console.log('[process] pid>0:', process.pid > 0, 'platform:', process.platform);
console.log('[process] versions.node:', process.versions.node, 'v8:', typeof process.versions.v8);
console.log('[process] cwd:', process.cwd() === process.cwd());
console.log('[process] hrtime bigint>0:', process.hrtime.bigint() > 0n);

process.env.WJS_SAMPLE_DEMO = 'yes';
console.log('[process] env roundtrip:', process.env.WJS_SAMPLE_DEMO);

// nextTick runs before setImmediate (native queue, not queueMicrotask).
const order = [];
process.nextTick(() => order.push('nextTick'));
setImmediate(() => {
  order.push('immediate');
  console.log('[process] tick order:', order.join('>'));
});
