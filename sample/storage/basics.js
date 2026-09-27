// WinterCG storage: async KV + localStorage (project-level turso file).
// WinterCG 存储：异步 KV + localStorage（项目级 turso 文件）。
// Run / 运行: winterjs --run sample/storage/basics.js --storage-path ./winterjs-storage.db
await storage.set('user:1', { name: 'alice', tags: ['a', 'b'] });
console.log('[storage] get:', (await storage.get('user:1')).name === 'alice');
console.log('[storage] has:', await storage.has('user:1'), await storage.has('nope'));
await storage.set('user:2', 'bob');
console.log('[storage] keys:', JSON.stringify(await storage.keys('user:')));
console.log('[storage] size:', await storage.size());
await storage.set('bin', new Uint8Array([1, 2, 3]));
console.log('[storage] u8:', (await storage.get('bin')).join(',') === '1,2,3');
console.log('[storage] del:', await storage.delete('user:2'), await storage.get('user:2'));

localStorage.setItem('theme', 'dark');
console.log('[storage] ls:', localStorage.getItem('theme') === 'dark', localStorage.length === 1);
console.log('[storage] ls-key:', localStorage.key(0) === 'theme');

// Inspect from another process (turso passthrough):
// 换进程查看（turso 透传）：winterjs --db ./winterjs-storage.db --exec "SELECT k FROM wjs_kv ORDER BY k"
