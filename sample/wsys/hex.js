// WinterJS2.hex：hex 编解码（字符串 <-> 字节）。
// WinterJS2.hex: hex codec between strings and bytes.
// Run / 运行: winterjs2 --run sample/wsys/hex.js
console.log('[hex] encode:', WinterJS2.hex.encode('hi') === '6869');
console.log('[hex] decode:', Array.from(WinterJS2.hex.decode('6869')).join(',') === '104,105');
console.log('[hex] empty:', WinterJS2.hex.encode('') === '');
console.log('[hex] bad-throws:', (() => { try { WinterJS2.hex.decode('zz'); return false; } catch { return true; } })());
