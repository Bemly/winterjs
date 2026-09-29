// WHATWG URL: parsing and mutation.
// WHATWG URL：解析与改写。
// Run / 运行: winterjs2 --run sample/web/url.js
const u = new URL('https://user:pass@example.com:8080/p/a/t/h?x=1&x=2#frag');
console.log('[url] host:', u.host, 'origin:', u.origin, 'pathname:', u.pathname);
console.log('[url] search:', u.search, 'hash:', u.hash, 'user:', u.username);
u.searchParams.set('x', '42');
console.log('[url] mutated:', u.href);
console.log('[url] canParse:', URL.canParse('https://example.com/') === true);
