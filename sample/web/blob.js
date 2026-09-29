// Blob: construction, slicing, text/arrayBuffer.
// Blob：构造、切片、读文本与字节。
// Run / 运行: winterjs --run sample/web/blob.js
const blob = new Blob(['hello ', 'world'], { type: 'text/plain' });
console.log('[blob] size/type:', blob.size === 11 && blob.type === 'text/plain');
console.log('[blob] slice text:', await blob.slice(0, 5).text() === 'hello');
console.log('[blob] bytes:', new Uint8Array(await blob.arrayBuffer()).length === 11);
