// node:readline over in-memory streams (no TTY needed).
// readline：无需真实终端的问答。
// Run / 运行: winterjs2 --run sample/readline/basics.js
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
