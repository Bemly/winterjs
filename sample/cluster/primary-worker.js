// node:cluster: primary forks one worker, message round-trip, clean exit.
// 集群：主进程 fork 一个 worker，消息往返后干净退出。
// Run / 运行: winterjs2 --run sample/cluster/primary-worker.js
import cluster from 'node:cluster';

if (cluster.isPrimary) {
  const worker = cluster.fork();
  worker.on('message', (msg) => {
    console.log('[cluster] msg:', msg === 'worker-ready');
    worker.disconnect();
  });
  worker.on('exit', (code) => console.log('[cluster] exit:', code === 0 || code === null));
} else {
  process.send('worker-ready');
}
