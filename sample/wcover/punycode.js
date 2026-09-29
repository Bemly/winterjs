// WinterJS.punycode: IDNA conversion both ways.
// WinterJS.punycode：域名编码双向转换。
// Run / 运行: winterjs --run sample/wcover/punycode.js
console.log('[punycode] ascii:', WinterJS.punycode.toASCII('münchen.de') === 'xn--mnchen-3ya.de');
console.log('[punycode] unicode:', WinterJS.punycode.toUnicode('xn--mnchen-3ya.de') === 'münchen.de');
