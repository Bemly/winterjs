// atob: base64 to binary string.
// atob：base64 解成二进制串。
// Run / 运行: winterjs2 --run sample/web/atob.js
console.log('[atob] hello:', atob('aGVsbG8=') === 'hello');
console.log('[atob] empty:', atob('') === '');
