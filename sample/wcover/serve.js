// WinterJS2.serve: static-plus-dynamic HTTP serving over loopback.
// WinterJS2.serve：回环动静一体 HTTP 服务。
// Run / 运行: winterjs2 --run sample/wcover/serve.js
const s = await WinterJS2.serve({ port: 0, hostname: '127.0.0.1' }, () => new Response('ok'));
console.log('[serve] text:', (await (await fetch(`http://127.0.0.1:${s.addr.port}/`)).text()) === 'ok');
await s.shutdown();
