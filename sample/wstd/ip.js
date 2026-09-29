// WinterJS.ip: CIDR containment and parsing.
// WinterJS.ip：CIDR 包含判断与解析。
// Run / 运行: winterjs --run sample/wstd/ip.js
console.log('[ip] contains:', WinterJS.ip.contains('10.0.0.0/8', '10.9.9.9') === true);
console.log('[ip] not-contains:', WinterJS.ip.contains('10.0.0.0/8', '11.0.0.1') === false);
console.log('[ip] parse:', JSON.stringify(WinterJS.ip.parse('192.168.1.0/24')) === '{"network":"192.168.1.0","prefixLen":24,"broadcast":"192.168.1.255"}');
