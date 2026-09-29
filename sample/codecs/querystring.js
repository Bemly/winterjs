// node:querystring: parse, stringify and escape.
// node:querystring：解析、序列化与转义。
// Run / 运行: winterjs2 --run sample/codecs/querystring.js
import qs from 'node:querystring';

console.log('[querystring] parse:', JSON.stringify(qs.parse('a=1&b=2&b=3')));
console.log('[querystring] stringify:', qs.stringify({ a: '1', b: ['2', '3'] }));
console.log('[querystring] escape:', qs.escape('a b+c') === 'a%20b%2Bc');
console.log('[querystring] unescape:', qs.unescape('a%20b') === 'a b');
