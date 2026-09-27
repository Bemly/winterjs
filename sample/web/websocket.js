// WebSocket client shape (offline-safe: bad URL throws synchronously).
// WebSocket 客户端形态（离线可跑：非法 URL 同步抛错；常量可见）。
// Run / 运行: winterjs --run sample/web/websocket.js
console.log('[ws] constants:', WebSocket.CONNECTING === 0 && WebSocket.OPEN === 1 && WebSocket.CLOSED === 3);
try {
  new WebSocket('not-a-url');
  console.log('[ws] bad url: NO-THROW (unexpected)');
} catch (e) {
  console.log('[ws] bad url throws:', e instanceof Error);
}
// Full echo flow against a live server is covered by sample/serve-hello/ README.
