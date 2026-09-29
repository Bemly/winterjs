// node:crypto keys: RSA/ECDSA/Ed25519 generate + sign/verify, ECDH, random.
// 非对称：密钥生成、签名验签、ECDH 与随机数。
// Run / 运行: winterjs2 --run sample/crypto/keys.js
import { generateKeyPairSync, createSign, createVerify, createECDH, randomBytes, randomUUID, randomInt } from 'node:crypto';

const { publicKey, privateKey } = generateKeyPairSync('ec', { namedCurve: 'P-256' });
const msg = Buffer.from('sign-me');
const sig = createSign('SHA-256').update(msg).sign(privateKey);
console.log('[keys] ecdsa verify:', createVerify('SHA-256').update(msg).verify(publicKey, sig));

const ed = generateKeyPairSync('ed25519');
console.log('[keys] ed keypair:', ed.publicKey.type === 'public' && ed.privateKey.type === 'private');

const a = createECDH('P-256');
a.generateKeys();
const b = createECDH('P-256');
b.generateKeys();
console.log('[keys] ecdh agree:', a.computeSecret(b.getPublicKey()).equals(b.computeSecret(a.getPublicKey())));

console.log('[keys] random:', randomBytes(8).length === 8, randomUUID().length === 36, randomInt(0, 100) < 100);
