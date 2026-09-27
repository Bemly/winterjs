// console global + node:console Console class over custom streams.
// console 全局与 node:console 的 Console 类（自定义流）。
// Run / 运行: winterjs --run sample/web/console.js
console.log('[console] log %s %d', 'fmt', 42);
console.info('[console] info visible');
console.warn('[console] warn visible');
console.error('[console] error visible');
console.dir({ a: { b: [1, 2, 3] } }, { depth: 3 });

const { Console } = await import('node:console');
const lines = [];
const fake = { write: (s) => lines.push(String(s)) };
const c = new Console({ stdout: fake, stderr: fake });
c.log('captured', 1);
console.log('[console] Console captured:', lines.join('').includes('captured'));
