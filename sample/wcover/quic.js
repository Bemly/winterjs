// WinterJS.quic: interface only (experimental; loopback handshake unsupported in this build).
// WinterJS.quic：仅接口面（实验；本构建回环握手不支持，只验形态）。
// Run / 运行: winterjs --run sample/wcover/quic.js
console.log('[quic] listen:', typeof WinterJS.quic.listen === 'function');
console.log('[quic] connect:', typeof WinterJS.quic.connect === 'function');
