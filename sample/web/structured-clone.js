// structuredClone: deep copy of plain data (Boundary: Date/Map/Set/RegExp/
// TypedArray currently come back as plain objects — see sample/docs/api.*).
// 结构化克隆：纯数据的深拷贝（边界：Date/Map/Set/正则/类型化数组目前回到普通对象，见文档）。
// Run / 运行: winterjs2 --run sample/web/structured-clone.js
const src = { a: 1, nested: { list: [1, 2, { three: 3 }] }, s: 'text', n: null };
const c = structuredClone(src);
c.nested.list[2].three = 99;
console.log('[clone] deep:', src.nested.list[2].three === 3 && c.nested.list[0] === 1);
console.log('[clone] fresh objects:', c !== src && c.nested !== src.nested);
