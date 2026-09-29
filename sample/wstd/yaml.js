// WinterJS2.yaml: YAML parse and stringify.
// WinterJS2.yaml：YAML 解析与序列化。
// Run / 运行: winterjs2 --run sample/wstd/yaml.js
console.log('[yaml] parse:', JSON.stringify(WinterJS2.yaml.parse('a: 1\nb: [x, true]\n')) === '{"a":1,"b":["x",true]}');
console.log('[yaml] stringify:', WinterJS2.yaml.stringify({ a: 1 }).includes('a: 1'));
console.log('[yaml] nested:', WinterJS2.yaml.parse('o:\n  n: 2\n').o.n === 2);
