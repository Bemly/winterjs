// WinterJS2.assert: structured assertions with AssertionError.
// WinterJS2.assert：结构化断言（含 AssertionError）。
// Run / 运行: winterjs2 --run sample/wcover/assert.js
WinterJS2.assert.ok(true);
WinterJS2.assert.deepEqual({ a: [1, { b: 2 }] }, { a: [1, { b: 2 }] });
console.log('[assert] pass:', true);
try { WinterJS2.assert.equal(1, 2); console.log('[assert] BAD'); }
catch (e) { console.log('[assert] throws:', e.name === 'AssertionError' && e.code === 'ERR_ASSERTION'); }
