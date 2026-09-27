// node:string_decoder / querystring / punycode: small codecs.
// 小编解码器：字符串解码器、查询串与 punycode。
// Run / 运行: winterjs --run sample/codecs/basics.js
import { StringDecoder } from 'node:string_decoder';
import qs from 'node:querystring';
import punycode from 'node:punycode';

const dec = new StringDecoder('utf8');
const euro = Buffer.from('€');
console.log('[codecs] split decode:', dec.write(euro.subarray(0, 2)) + dec.end(euro.subarray(2)));

console.log('[codecs] parse:', JSON.stringify(qs.parse('a=1&b=2&b=3')));
console.log('[codecs] stringify:', qs.stringify({ a: '1', b: ['2', '3'] }));
console.log('[codecs] escape:', qs.escape('a b+c'));

console.log('[codecs] ascii:', punycode.toASCII('münchen.de'), 'unicode:', punycode.toUnicode('xn--mnchen-3ya.de'));
console.log('[codecs] ucs2 len:', punycode.ucs2.decode('中文').length === 2);
