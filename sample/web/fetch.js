// fetch + Request / Response / Headers (offline: data: URLs).
// fetch 与请求/响应对象（离线可用 data: URL）。
// Run / 运行: winterjs --run sample/web/fetch.js
const res = await fetch('data:text/plain,hello-fetch');
console.log('[fetch] status:', res.status, 'ok:', res.ok);
console.log('[fetch] text:', await res.text());

const json = await (await fetch('data:application/json,{"n":7}')).json();
console.log('[fetch] json.n:', json.n);

const req = new Request('https://example.com/api?q=1', {
  method: 'POST',
  headers: { 'content-type': 'text/plain', 'x-a': '1' },
  body: 'payload',
});
console.log('[fetch] req:', req.method, req.url, req.headers.get('x-a'));
console.log('[fetch] req body:', await req.text());

const out = new Response(JSON.stringify({ ok: true }), {
  status: 201,
  headers: { 'content-type': 'application/json' },
});
console.log('[fetch] res:', out.status, out.headers.get('content-type'), (await out.json()).ok);

// AbortController aborts an in-flight fetch (data: resolves fast; abort a pending one).
const c = new AbortController();
c.abort('stop');
console.log('[fetch] aborted signal:', c.signal.aborted, c.signal.reason);
