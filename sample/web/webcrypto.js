// WebCrypto: getRandomValues / randomUUID / subtle.digest + AES-GCM + HMAC.
// WebCrypto：随机数 / UUID / 摘要 + AES-GCM + HMAC。
// Run / 运行: winterjs --run sample/web/webcrypto.js
const rnd = crypto.getRandomValues(new Uint8Array(16));
console.log('[webcrypto] random bytes:', rnd.length, rnd instanceof Uint8Array);
console.log('[webcrypto] uuid shape:', crypto.randomUUID().split('-').length === 5);

const hex = (b) => [...new Uint8Array(b)].map((x) => x.toString(16).padStart(2, '0')).join('');
console.log('[webcrypto] sha256:', hex(await crypto.subtle.digest('SHA-256', new TextEncoder().encode('abc'))).slice(0, 16));

const key = await crypto.subtle.generateKey({ name: 'AES-GCM', length: 256 }, true, ['encrypt', 'decrypt']);
const iv = crypto.getRandomValues(new Uint8Array(12));
const ct = await crypto.subtle.encrypt({ name: 'AES-GCM', iv }, key, new TextEncoder().encode('secret'));
console.log('[webcrypto] aes-gcm ct bytes:', ct.byteLength > 16);
console.log('[webcrypto] aes-gcm roundtrip:', new TextDecoder().decode(await crypto.subtle.decrypt({ name: 'AES-GCM', iv }, key, ct)));

const hkey = await crypto.subtle.importKey('raw', new TextEncoder().encode('k'), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
console.log('[webcrypto] hmac bytes:', (await crypto.subtle.sign('HMAC', hkey, new TextEncoder().encode('m'))).byteLength > 0);
