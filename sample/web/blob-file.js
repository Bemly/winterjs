// Blob / File: construction, slicing, text/arrayBuffer.
// Blob / File：构造、切片、读文本与字节。
// Run / 运行: winterjs --run sample/web/blob-file.js
const blob = new Blob(['hello ', 'world'], { type: 'text/plain' });
console.log('[blob] size:', blob.size, 'type:', blob.type);

const part = blob.slice(0, 5);
console.log('[blob] slice text:', await part.text());

const buf = await blob.arrayBuffer();
console.log('[blob] bytes:', new Uint8Array(buf).length);

const file = new File(['{"a":1}'], 'data.json', { type: 'application/json', lastModified: 0 });
console.log('[blob] file:', file.name, file.type, file.lastModified === 0);
console.log('[blob] json:', JSON.parse(await file.text()).a);
