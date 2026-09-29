// btoa: binary string to base64.
// btoa：二进制串编成 base64。
// Run / 运行: winterjs2 --run sample/web/btoa.js
console.log('[btoa] hello:', btoa('hello') === 'aGVsbG8=');
console.log('[btoa] roundtrip:', atob(btoa('round-trip')) === 'round-trip');
