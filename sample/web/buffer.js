// Buffer global: alloc / from / concat / compare / encodings.
// Buffer 全局：分配、构造、拼接、比较与编码。
// Run / 运行: winterjs --run sample/web/buffer.js
console.log('[buffer] isBuffer:', Buffer.isBuffer(Buffer.from('x')), Buffer.isEncoding('base64url'));

const b = Buffer.from('hello');
console.log('[buffer] toString hex:', b.toString('hex'), 'base64:', b.toString('base64'));

const cat = Buffer.concat([Buffer.from([1, 2]), Buffer.from([3])]);
console.log('[buffer] concat:', [...cat].join(','), 'compare:', Buffer.compare(Buffer.from('a'), Buffer.from('b')));

const z = Buffer.alloc(4, 7);
console.log('[buffer] alloc fill:', [...z].join(','), 'byteLength:', Buffer.byteLength('中文', 'utf8'));

const u8 = new Uint8Array([104, 105]);
console.log('[buffer] from u8:', Buffer.from(u8).toString(), 'includes:', Buffer.from('hi').includes(105));
