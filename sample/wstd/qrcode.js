// WinterJS.qrcode: terminal QR code rendering.
// WinterJS.qrcode：终端二维码渲染。
// Run / 运行: winterjs --run sample/wstd/qrcode.js
const code = WinterJS.qrcode('hi');
console.log('[qrcode] string:', typeof code === 'string' && code.length > 0);
console.log('[qrcode] blocks:', code.includes('█') || code.includes('▀') || code.includes(' ') === true);
