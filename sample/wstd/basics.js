// WinterJS 小工具面：semver/yaml/jsonc/ip/shlex/spdx/qrcode（纯函数，零新依赖）。
// WinterJS utility surface: semver/yaml/jsonc/ip/shlex/spdx/qrcode.
// Run / 运行: winterjs --run sample/wstd/basics.js
console.log('[wstd] semver:', WinterJS.semver.valid('1.2.3'), WinterJS.semver.satisfies('1.2.3', '^1.0.0'), WinterJS.semver.compare('1.0.0', '1.0.0-alpha'));
console.log('[wstd] parse:', JSON.stringify(WinterJS.semver.parse('1.2.3-beta.1+build')) === '{"major":1,"minor":2,"patch":3,"pre":["beta","1"],"build":["build"]}');
console.log('[wstd] yaml:', JSON.stringify(WinterJS.yaml.parse('a: 1\nb: [x, true]\n')) === '{"a":1,"b":["x",true]}');
console.log('[wstd] yamlstr:', WinterJS.yaml.stringify({ a: 1 }).includes('a: 1'));
console.log('[wstd] jsonc:', JSON.stringify(WinterJS.jsonc.parse('{ "a": 1, // c\n }')) === '{"a":1}');
console.log('[wstd] ip:', WinterJS.ip.contains('10.0.0.0/8', '10.9.9.9'), JSON.stringify(WinterJS.ip.parse('192.168.1.0/24')) === '{"network":"192.168.1.0","prefixLen":24,"broadcast":"192.168.1.255"}');
console.log('[wstd] shlex:', JSON.stringify(WinterJS.shlex.split("a 'b c'")) === '["a","b c"]');
console.log('[wstd] spdx:', WinterJS.spdx.valid('MIT OR Apache-2.0'), !WinterJS.spdx.valid('nope-not-a-license'));
console.log('[wstd] qrcode:', typeof WinterJS.qrcode('hi') === 'string');
