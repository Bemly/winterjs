// WinterJS2.quic: interface only (experimental; loopback handshake unsupported in this build).
// WinterJS2.quic：仅接口面（实验；本构建回环握手不支持，只验形态）。
// Run / 运行: winterjs2 --run sample/wcover/quic.js
console.log('[quic] listen:', typeof WinterJS2.quic.listen === 'function');
console.log('[quic] connect:', typeof WinterJS2.quic.connect === 'function');
