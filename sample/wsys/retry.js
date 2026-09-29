// WinterJS2.retry：重试等待（constant/exponential 退避）与 run 循环。
// WinterJS2.retry: backoff delays (constant/exponential) and the run loop.
// Run / 运行: winterjs2 --run sample/wsys/retry.js
console.log('[retry] constant:', WinterJS2.retry.delay('constant', 2, { minMs: 100, maxMs: 1000 }) === 100);
console.log('[retry] exponential:', WinterJS2.retry.delay('exponential', 0, { minMs: 100, maxMs: 5000 }) === 100);
const r = await WinterJS2.retry.run(async (a) => { if (a < 2) throw new Error('x'); return 'ok'; }, { attempts: 3, minMs: 1, maxMs: 2 });
console.log('[retry] run:', r.value === 'ok' && r.attempts === 3);
