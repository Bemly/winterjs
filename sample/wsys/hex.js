// WinterJS.hex：hex 编解码（字符串 <-> 字节）。
// WinterJS.hex: hex codec between strings and bytes.
// Run / 运行: winterjs --run sample/wsys/hex.js
console.log('[hex] encode:', WinterJS.hex.encode('hi') === '6869');
console.log('[hex] decode:', Array.from(WinterJS.hex.decode('6869')).join(',') === '104,105');
console.log('[hex] empty:', WinterJS.hex.encode('') === '');
console.log('[hex] bad-throws:', (() => { try { WinterJS.hex.decode('zz'); return false; } catch { return true; } })());
