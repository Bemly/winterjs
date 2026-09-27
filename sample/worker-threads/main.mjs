// node:worker_threads: main posts a number, worker squares it.
// 工作线程：主线程发数，worker 回平方。
// Run / 运行: winterjs --run sample/worker-threads/main.mjs
import { Worker } from 'node:worker_threads';

const worker = new Worker(new URL('./helper.mjs', import.meta.url));
const result = await new Promise((resolve, reject) => {
  worker.on('message', resolve);
  worker.on('error', reject);
  worker.postMessage(9);
});
console.log('[worker] square:', result === 81);
await worker.terminate();
