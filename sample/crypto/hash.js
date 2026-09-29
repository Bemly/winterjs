// node:crypto hashing: createHash / Hmac across algorithms.
// 哈希：多算法摘要与 HMAC。
// Run / 运行: winterjs2 --run sample/crypto/hash.js
import { createHash, createHmac, getHashes } from 'node:crypto';

console.log('[hash] sha256:', createHash('sha256').update('abc').digest('hex').slice(0, 12));
console.log('[hash] md5+sha1+sha512:', createHash('md5').update('x').digest('hex').length === 32);
console.log('[hash] chained:', createHash('sha1').update('a').update('b').digest('base64').length > 0);
console.log('[hash] hmac:', createHmac('sha256', 'key').update('msg').digest('hex').length === 64);
console.log('[hash] has sha256:', getHashes().includes('sha256'));
