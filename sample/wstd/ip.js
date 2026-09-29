// WinterJS2.ip: CIDR containment and parsing.
// WinterJS2.ip：CIDR 包含判断与解析。
// Run / 运行: winterjs2 --run sample/wstd/ip.js
console.log('[ip] contains:', WinterJS2.ip.contains('10.0.0.0/8', '10.9.9.9') === true);
console.log('[ip] not-contains:', WinterJS2.ip.contains('10.0.0.0/8', '11.0.0.1') === false);
console.log('[ip] parse:', JSON.stringify(WinterJS2.ip.parse('192.168.1.0/24')) === '{"network":"192.168.1.0","prefixLen":24,"broadcast":"192.168.1.255"}');
