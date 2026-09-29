// Web Worker: JSON-message threads.
// Worker：JSON 消息线程。
// Run / 运行: winterjs --run sample/wcover/worker.js
const w = new Worker('sample/wcover/worker-echo.mjs');
w.postMessage({ n: 1 });
const got = await new Promise((res, rej) => {
  w.onmessage = (e) => res(e.data);
  w.onerror = (e) => rej(e.error);
  setTimeout(() => rej(new Error('timeout')), 5000);
});
console.log('[worker] echo:', JSON.stringify(got) === '{"echo":{"n":1}}');
w.terminate();
