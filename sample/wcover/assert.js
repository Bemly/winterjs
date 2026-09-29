// WinterJS.assert: structured assertions with AssertionError.
// WinterJS.assert：结构化断言（含 AssertionError）。
// Run / 运行: winterjs --run sample/wcover/assert.js
WinterJS.assert.ok(true);
WinterJS.assert.deepEqual({ a: [1, { b: 2 }] }, { a: [1, { b: 2 }] });
console.log('[assert] pass:', true);
try { WinterJS.assert.equal(1, 2); console.log('[assert] BAD'); }
catch (e) { console.log('[assert] throws:', e.name === 'AssertionError' && e.code === 'ERR_ASSERTION'); }
