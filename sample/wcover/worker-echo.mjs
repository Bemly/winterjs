// B4 Worker 回显子端（parentPort 收发 JSON 文）。
import { parentPort } from 'node:worker_threads';
parentPort.on('message', (v) => { parentPort.postMessage(JSON.stringify({ echo: v })); });
