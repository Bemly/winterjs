// Worker helper: squares the number it receives, then exits.
// Worker  helper：平方收到的数字后退出。
import { parentPort } from 'node:worker_threads';

parentPort.on('message', (n) => {
  parentPort.postMessage(n * n);
});
