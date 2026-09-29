// WinterJS2 / Deno / Bun namespaces (all offline, no imports).
// WinterJS2 / Deno / Bun 命名空间（全离线，免导入）。
// Run / 运行: winterjs2 --run sample/winterjs2/namespaces.js
console.log('[ns] WinterJS2:', WinterJS2.version === process.versions.winterjs2, WinterJS2.cwd() === process.cwd());
console.log('[ns] Deno:', Deno.version.deno === WinterJS2.version, Deno.pid === process.pid, Object.isFrozen(Deno));
console.log('[ns] Bun:', typeof Bun.version === 'string', Bun.main === String(process.argv[1] || ''), Bun.which('sh') === '/bin/sh');

// Deno file I/O rides node:fs; Bun.file mirrors it.
await Deno.writeTextFile('ns-demo.txt', 'hi');
console.log('[ns] deno-fs:', await Deno.readTextFile('ns-demo.txt') === 'hi', await Bun.file('ns-demo.txt').text() === 'hi');
await WinterJS2.image; // namespace present alongside image surface
console.log('[ns] storage:', WinterJS2.storage === globalThis.storage, WinterJS2.Deno === Deno, WinterJS2.Bun === Bun);
await Deno.remove('ns-demo.txt');
console.log('[ns] done');
