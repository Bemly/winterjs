// WinterJS2.punycode: IDNA conversion both ways.
// WinterJS2.punycode：域名编码双向转换。
// Run / 运行: winterjs2 --run sample/wcover/punycode.js
console.log('[punycode] ascii:', WinterJS2.punycode.toASCII('münchen.de') === 'xn--mnchen-3ya.de');
console.log('[punycode] unicode:', WinterJS2.punycode.toUnicode('xn--mnchen-3ya.de') === 'münchen.de');
