// Request: method, URL, headers and body.
// Request：方法、地址、头与请求体。
// Run / 运行: winterjs --run sample/web/request.js
const req = new Request('https://example.com/api?q=1', {
  method: 'POST',
  headers: { 'content-type': 'text/plain', 'x-a': '1' },
  body: 'payload',
});
console.log('[request] method/url/header:', req.method === 'POST' && req.url === 'https://example.com/api?q=1' && req.headers.get('x-a') === '1');
console.log('[request] body:', await req.text() === 'payload');
console.log('[request] headers-iter:', [...req.headers.keys()].join(',') === 'content-type,x-a');
