// node:punycode: IDNA conversion and ucs2 helpers.
// node:punycode：域名编码转换与 ucs2 工具。
// Run / 运行: winterjs --run sample/codecs/punycode.js
import punycode from 'node:punycode';

console.log('[punycode] ascii:', punycode.toASCII('münchen.de') === 'xn--mnchen-3ya.de');
console.log('[punycode] unicode:', punycode.toUnicode('xn--mnchen-3ya.de') === 'münchen.de');
console.log('[punycode] ucs2 len:', punycode.ucs2.decode('中文').length === 2);
console.log('[punycode] encode:', punycode.encode('münchen') === 'mnchen-3ya');
