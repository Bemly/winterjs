// WinterJS.shlex: shell word splitting.
// WinterJS.shlex：shell 切词。
// Run / 运行: winterjs --run sample/wstd/shlex.js
console.log('[shlex] quoted:', JSON.stringify(WinterJS.shlex.split("a 'b c'")) === '["a","b c"]');
console.log('[shlex] escaped:', JSON.stringify(WinterJS.shlex.split('a "b\\"c" d')) === '["a","b\\"c","d"]');
