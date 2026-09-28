// WinterJS memory:可观测 + 受控分配 + 手动堆（unsafe* 需 --allow-ffi）。
// WinterJS memory: observable + controlled alloc + manual heap.
// Run / 运行: winterjs --run sample/mem/basics.js --allow-ffi
console.log('[mem] allocator:', ['smmalloc', 'talc'].includes(WinterJS.memory().allocator));
console.log('[mem] rss:', WinterJS.memory().rss > 0);
const a = WinterJS.alloc(4);
console.log('[mem] alloc:', a.length === 4 && a[0] === 0);
a[0] = 7;
console.log('[mem] write:', a[0] === 7);

const id = WinterJS.unsafeAlloc(8);
console.log('[mem] size:', WinterJS.unsafeSize(id) === 8);
WinterJS.unsafeWrite(id, 0, new Uint8Array([1, 2, 3]));
WinterJS.unsafeWrite(id, 3, 'hi');
console.log('[mem] read:', Array.from(WinterJS.unsafeRead(id, 0, 5)).join(',') === '1,2,3,104,105');
console.log('[mem] list:', WinterJS.unsafeList().includes(id));
WinterJS.unsafeFree(id);
console.log('[mem] freed:', !WinterJS.unsafeList().includes(id));
