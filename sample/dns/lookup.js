// node:dns: localhost lookup (offline-safe) + server config.
// DNS：本地回环查询（离线可用）与服务器配置。
// Run / 运行: winterjs --run sample/dns/lookup.js
import dns, { lookup, getServers } from 'node:dns';
import { lookup as lookupP } from 'node:dns/promises';

lookup('localhost', (err, address, family) => {
  if (err) throw err;
  console.log('[dns] localhost:', address, 'family:', family);
});

const { address } = await lookupP('localhost');
console.log('[dns] promises localhost:', typeof address === 'string');
console.log('[dns] servers configured:', getServers().length >= 0);
console.log('[dns] resolveAny fn:', typeof dns.resolveAny === 'function');
