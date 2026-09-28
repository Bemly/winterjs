// 原生覆盖 B6：WinterJS.stream/diag/domain/trace/async/serve（+ crypto 别名）。
// Run / 运行: winterjs --run sample/wcover/b6.js
const rs = new ReadableStream({ start(c) { c.enqueue(new TextEncoder().encode('hi')); c.close(); } });
let out = '';
await WinterJS.stream.pipeline(rs, new WritableStream({ write(c) { out += new TextDecoder().decode(c); } }));
console.log('[wcover] pipe:', out === 'hi');
const ch = WinterJS.diagnostics.channel('wcover');
ch.subscribe((m) => console.log('[wcover] diag:', JSON.stringify(m) === '{"a":1}'));
ch.publish({ a: 1 });
const als = new WinterJS.AsyncLocalStorage();
console.log('[wcover] als:', als.run('v', () => als.getStore()) === 'v');
console.log('[wcover] subtle:', typeof WinterJS.crypto.subtle === 'object');
const s = await WinterJS.serve({ port: 0, hostname: '127.0.0.1' }, () => new Response('ok'));
console.log('[wcover] serve:', (await (await fetch(`http://127.0.0.1:${s.addr.port}/`)).text()) === 'ok');
await s.shutdown();
