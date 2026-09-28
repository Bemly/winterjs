// WinterJS / Deno / Bun namespaces (all offline, no imports).
// WinterJS / Deno / Bun 命名空间（全离线，免导入）。
// Run / 运行: winterjs --run sample/winterjs/namespaces.js
console.log('[ns] WinterJS:', WinterJS.version === process.versions.winterjs, WinterJS.cwd() === process.cwd());
console.log('[ns] Deno:', Deno.version.deno === WinterJS.version, Deno.pid === process.pid, Object.isFrozen(Deno));
console.log('[ns] Bun:', typeof Bun.version === 'string', Bun.main === String(process.argv[1] || ''), Bun.which('sh') === '/bin/sh');

// Deno file I/O rides node:fs; Bun.file mirrors it.
await Deno.writeTextFile('ns-demo.txt', 'hi');
console.log('[ns] deno-fs:', await Deno.readTextFile('ns-demo.txt') === 'hi', await Bun.file('ns-demo.txt').text() === 'hi');
await WinterJS.image; // namespace present alongside image surface
console.log('[ns] storage:', WinterJS.storage === globalThis.storage, WinterJS.Deno === Deno, WinterJS.Bun === Bun);
await Deno.remove('ns-demo.txt');
console.log('[ns] done');
