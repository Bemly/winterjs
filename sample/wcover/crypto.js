// WinterJS2.crypto: WebCrypto alias face.
// WinterJS2.crypto：WebCrypto 别名面。
// Run / 运行: winterjs2 --run sample/wcover/crypto.js
console.log('[crypto] subtle:', typeof WinterJS2.crypto.subtle === 'object');
console.log('[crypto] getRandomValues:', WinterJS2.crypto.getRandomValues(new Uint8Array(4)).length === 4);
