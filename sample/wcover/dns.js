// WinterJS2.dns: lookups (localhost resolves offline).
// WinterJS2.dns：域名查询（localhost 离线可解）。
// Run / 运行: winterjs2 --run sample/wcover/dns.js
const addrs = await WinterJS2.dns.lookup('localhost');
console.log('[dns] localhost:', addrs.some((a) => a.address === '127.0.0.1'));
console.log('[dns] family field:', addrs.every((a) => typeof a.family === 'number'));
