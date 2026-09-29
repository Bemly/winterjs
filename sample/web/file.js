// File: named Blob with lastModified.
// File：带文件名与修改时间的 Blob。
// Run / 运行: winterjs2 --run sample/web/file.js
const file = new File(['{"a":1}'], 'data.json', { type: 'application/json', lastModified: 0 });
console.log('[file] name/type/mtime:', file.name === 'data.json' && file.type === 'application/json' && file.lastModified === 0);
console.log('[file] json:', JSON.parse(await file.text()).a === 1);
console.log('[file] instanceof Blob:', file instanceof Blob);
