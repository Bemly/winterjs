// WinterJS.tcp: Promise sockets over loopback echo.
// WinterJS.tcp：回环 TCP 回声（Promise 风格套接字）。
// Run / 运行: winterjs --run sample/wcover/tcp.js
const s = await WinterJS.tcp.listen(0, '127.0.0.1');
const c = await WinterJS.tcp.connect('127.0.0.1', s.address().port);
c.write('ping');
for await (const sock of s) {
  for await (const chunk of sock) { sock.write(chunk); sock.end(); break; }
  break;
}
let back = '';
for await (const chunk of c) back += new TextDecoder().decode(chunk);
console.log('[tcp] echo:', back === 'ping');
c.destroy();
s.close();
