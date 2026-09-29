// TextEncoder: strings to UTF-8 bytes.
// TextEncoder：字符串编成 UTF-8 字节。
// Run / 运行: winterjs2 --run sample/web/textencoder.js
const enc = new TextEncoder();
const bytes = enc.encode('hi-中文');
console.log('[textencoder] bytes:', bytes.length === 9);
console.log('[textencoder] encoding:', enc.encoding === 'utf-8');
const into = enc.encodeInto('hello', new Uint8Array(8));
console.log('[textencoder] encodeInto:', into.read === 5 && into.written === 5);
