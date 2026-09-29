// WinterJS.crypto: WebCrypto alias face.
// WinterJS.crypto：WebCrypto 别名面。
// Run / 运行: winterjs --run sample/wcover/crypto.js
console.log('[crypto] subtle:', typeof WinterJS.crypto.subtle === 'object');
console.log('[crypto] getRandomValues:', WinterJS.crypto.getRandomValues(new Uint8Array(4)).length === 4);
