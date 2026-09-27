// WHATWG URL: URL / URLSearchParams / URLPattern + base64 + TextEncoder/Decoder.
// WHATWG URL：URL / URLSearchParams / URLPattern + base64 + 编解码器。
// Run / 运行: winterjs --run sample/web/url.js
const u = new URL('https://user:pass@example.com:8080/p/a/t/h?x=1&x=2#frag');
console.log('[url] host:', u.host, 'origin:', u.origin, 'pathname:', u.pathname);
console.log('[url] search:', u.search, 'hash:', u.hash, 'user:', u.username);

const sp = new URLSearchParams('?a=1&a=2&b=%E4%B8%AD');
console.log('[url] getAll(a):', sp.getAll('a').join(','), 'get(b):', sp.get('b'));
sp.append('c', '3');
console.log('[url] serialized:', sp.toString());

u.searchParams.set('x', '42');
console.log('[url] mutated:', u.href);

const pat = new URLPattern({ pathname: '/users/:id' });
const m = pat.exec('https://example.com/users/123');
console.log('[url] pattern test:', pat.test('https://example.com/users/123'), 'id:', m?.pathname.groups.id);

console.log('[url] btoa:', btoa('hello'), 'atob:', atob('aGVsbG8='));

const enc = new TextEncoder();
const bytes = enc.encode('hi-中文');
console.log('[url] encoded bytes:', bytes.length);
console.log('[url] decoded:', new TextDecoder('utf-8').decode(bytes));
console.log('[url] fatal decode ok:', new TextDecoder('utf-8', { fatal: true }).decode(enc.encode('ok')));
