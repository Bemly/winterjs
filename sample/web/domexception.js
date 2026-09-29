// DOMException: named DOM errors.
// DOMException：具名 DOM 错误。
// Run / 运行: winterjs --run sample/web/domexception.js
const err = new DOMException('aborted here', 'AbortError');
console.log('[domexception] name/code:', err.name === 'AbortError' && err.code === 20);
console.log('[domexception] message:', err.message === 'aborted here');
console.log('[domexception] instanceof Error:', err instanceof Error);
