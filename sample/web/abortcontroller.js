// AbortController: abort an operation via its signal.
// AbortController：用 signal 中止操作。
// Run / 运行: winterjs2 --run sample/web/abortcontroller.js
const c = new AbortController();
console.log('[abortcontroller] pending:', c.signal.aborted === false);
c.signal.addEventListener('abort', () => console.log('[abortcontroller] reason:', String(c.signal.reason)));
c.abort('done');
console.log('[abortcontroller] aborted:', c.signal.aborted === true);
console.log('[abortcontroller] timeout signal:', AbortSignal.timeout(1000) instanceof AbortSignal);
