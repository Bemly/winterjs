// WinterJS2.jsonc: JSON with comments and trailing commas.
// WinterJS2.jsonc：容忍注释与尾逗号的 JSON。
// Run / 运行: winterjs2 --run sample/wstd/jsonc.js
console.log('[jsonc] comment:', JSON.stringify(WinterJS2.jsonc.parse('{ "a": 1, // c\n }')) === '{"a":1}');
console.log('[jsonc] trailing-comma:', JSON.stringify(WinterJS2.jsonc.parse('{"a": [1, 2,]}')) === '{"a":[1,2]}');
