// node:crypto ciphers: AES-GCM / AES-CCM / ChaCha20-Poly1305 round-trips.
// 对称加解密：三种 AEAD 往返。
// Run / 运行: winterjs --run sample/crypto/cipher.js
import { createCipheriv, createDecipheriv, randomBytes } from 'node:crypto';

function roundtrip(algo, keyLen, ivLen, opts) {
  const key = randomBytes(keyLen);
  const iv = randomBytes(ivLen);
  const enc = createCipheriv(algo, key, iv, opts);
  const ct = Buffer.concat([enc.update('secret-msg', 'utf8'), enc.final()]);
  const tag = enc.getAuthTag();
  const dec = createDecipheriv(algo, key, iv, opts);
  dec.setAuthTag(tag);
  return Buffer.concat([dec.update(ct), dec.final()]).toString() === 'secret-msg';
}

console.log('[cipher] aes-256-gcm:', roundtrip('aes-256-gcm', 32, 12));
console.log('[cipher] aes-128-ccm:', roundtrip('aes-128-ccm', 16, 12, { authTagLength: 8 }));
console.log('[cipher] chacha20-poly1305:', roundtrip('chacha20-poly1305', 32, 12));
