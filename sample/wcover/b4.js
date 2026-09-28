// Worker 全局 + WinterJS.cluster/test/vm：线程、测试与求值。
// Run / 运行: winterjs --run sample/wcover/b4.js
const w = new Worker('sample/wcover/worker-echo.mjs');
w.postMessage({ n: 1 });
const got = await new Promise((res, rej) => {
  w.onmessage = (e) => res(e.data);
  w.onerror = (e) => rej(e.error);
  setTimeout(() => rej(new Error('timeout')), 5000);
});
console.log('[wcover] worker:', JSON.stringify(got) === '{"echo":{"n":1}}');
w.terminate();
console.log('[wcover] vm:', WinterJS.vm.run('40 + 2') === 42);
console.log('[wcover] cluster:', typeof WinterJS.cluster.isPrimary === 'boolean');
console.log('[wcover] test:', typeof WinterJS.test.test === 'function');
