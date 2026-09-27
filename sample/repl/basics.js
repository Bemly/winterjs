// node:repl: start a REPL over in-memory streams (no TTY needed).
// repl：内存流驱动的交互求值，无需真实终端。
// Run / 运行: winterjs --run sample/repl/basics.js
import { start, Recoverable } from 'node:repl';
import { Readable, Writable } from 'node:stream';

const input = Readable.from(['40 + 2\n']);
let output = '';
const r = start({
  input,
  output: new Writable({
    write(c, _e, cb) {
      output += String(c);
      cb();
    },
  }),
  terminal: false,
});
await new Promise((resolve) => r.on('exit', resolve));
console.log('[repl] evaluated:', output.includes('42'));

const rec = new Recoverable(new SyntaxError('unexpected end'));
console.log('[repl] recoverable is error:', rec instanceof Error);
