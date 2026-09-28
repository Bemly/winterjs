// 原生覆盖 B1：WinterJS.assert/util/punycode（Web 风）。
// Native coverage B1: assert (structural), util.format/inspect, punycode core.
// Run / 运行: winterjs --run sample/wcover/b1.js
WinterJS.assert.ok(true);
WinterJS.assert.deepEqual({ a: [1, { b: 2 }] }, { a: [1, { b: 2 }] });
console.log('[wcover] assert:', true);
console.log('[wcover] format:', WinterJS.util.format('%s=%d', 'a', 1) === 'a=1');
console.log('[wcover] inspect:', WinterJS.util.inspect({ a: 1 }).includes('a: 1'));
console.log('[wcover] puny:', WinterJS.punycode.toASCII('münchen.de') === 'xn--mnchen-3ya.de');
console.log('[wcover] unicode:', WinterJS.punycode.toUnicode('xn--mnchen-3ya.de') === 'münchen.de');
try { WinterJS.assert.equal(1, 2); console.log('[wcover] BAD'); }
catch (e) { console.log('[wcover] throws:', e.name === 'AssertionError' && e.code === 'ERR_ASSERTION'); }
