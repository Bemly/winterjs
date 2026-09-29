// WinterJS.yaml: YAML parse and stringify.
// WinterJS.yaml：YAML 解析与序列化。
// Run / 运行: winterjs --run sample/wstd/yaml.js
console.log('[yaml] parse:', JSON.stringify(WinterJS.yaml.parse('a: 1\nb: [x, true]\n')) === '{"a":1,"b":["x",true]}');
console.log('[yaml] stringify:', WinterJS.yaml.stringify({ a: 1 }).includes('a: 1'));
console.log('[yaml] nested:', WinterJS.yaml.parse('o:\n  n: 2\n').o.n === 2);
