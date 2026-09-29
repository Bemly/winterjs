// WinterJS2 memory:可观测 + 受控分配 + 手动堆（unsafe* 需 --allow-ffi）。
// WinterJS2 memory: observable + controlled alloc + manual heap.
// Run / 运行: winterjs2 --run sample/mem/basics.js --allow-ffi
console.log('[mem] allocator:', ['smmalloc', 'talc'].includes(WinterJS2.memory().allocator));
console.log('[mem] rss:', WinterJS2.memory().rss > 0);
const a = WinterJS2.alloc(4);
console.log('[mem] alloc:', a.length === 4 && a[0] === 0);
a[0] = 7;
console.log('[mem] write:', a[0] === 7);

const id = WinterJS2.unsafeAlloc(8);
console.log('[mem] size:', WinterJS2.unsafeSize(id) === 8);
WinterJS2.unsafeWrite(id, 0, new Uint8Array([1, 2, 3]));
WinterJS2.unsafeWrite(id, 3, 'hi');
console.log('[mem] read:', Array.from(WinterJS2.unsafeRead(id, 0, 5)).join(',') === '1,2,3,104,105');
console.log('[mem] list:', WinterJS2.unsafeList().includes(id));
WinterJS2.unsafeFree(id);
console.log('[mem] freed:', !WinterJS2.unsafeList().includes(id));
