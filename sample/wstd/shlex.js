// WinterJS2.shlex: shell word splitting.
// WinterJS2.shlex：shell 切词。
// Run / 运行: winterjs2 --run sample/wstd/shlex.js
console.log('[shlex] quoted:', JSON.stringify(WinterJS2.shlex.split("a 'b c'")) === '["a","b c"]');
console.log('[shlex] escaped:', JSON.stringify(WinterJS2.shlex.split('a "b\\"c" d')) === '["a","b\\"c","d"]');
