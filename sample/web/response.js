// Response: status, headers and body readers.
// Response：状态码、头与 body 读取。
// Run / 运行: winterjs --run sample/web/response.js
const out = new Response(JSON.stringify({ ok: true }), {
  status: 201,
  headers: { 'content-type': 'application/json' },
});
console.log('[response] status/type:', out.status === 201 && out.headers.get('content-type') === 'application/json');
console.log('[response] json:', (await out.json()).ok === true);
console.log('[response] not-ok:', new Response('x', { status: 404 }).ok === false);
console.log('[response] redirect:', Response.redirect('https://example.com/', 302).status === 302);
const j = Response.json({ a: 1 }, { status: 201 });
console.log('[response] static-json:', j.status === 201 && j.headers.get('content-type') === 'application/json' && await j.text() === '{"a":1}');
