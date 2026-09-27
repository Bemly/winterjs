// node:readline over in-memory streams (no TTY needed).
// readline：内存流问答，无需真实终端。
// Run / 运行: winterjs --run sample/readline/basics.js
import { createInterface } from 'node:readline';
import { Readable, Writable } from 'node:stream';

const input = Readable.from(['first line\n', 'second\n']);
let output = '';
const rl = createInterface({
  input,
  output: new Writable({
    write(c, _e, cb) {
      output += String(c);
      cb();
    },
  }),
  terminal: false,
});

const lines = [];
rl.on('line', (l) => lines.push(l));
await new Promise((r) => rl.on('close', r));
console.log('[readline] lines:', lines.join('|'));
rl.close();
