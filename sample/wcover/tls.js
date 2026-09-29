// WinterJS2.tls: TLS client against a loopback server (self-signed fixture, sample-only).
// WinterJS2.tls：回环 TLS 客户端（自签样例证书，仅样例与测试用，勿用于生产）。
// Run / 运行: winterjs2 --run sample/wcover/tls.js
import tls from 'node:tls';
import fs from 'node:fs';

const server = tls.createServer({
  key: fs.readFileSync(new URL('../tls/fixtures/key.pem', import.meta.url)),
  cert: fs.readFileSync(new URL('../tls/fixtures/cert.pem', import.meta.url)),
}, (socket) => {
  socket.write('secure-hello');
  socket.end();
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const sock = await WinterJS2.tls.connect({ host: '127.0.0.1', port: server.address().port, rejectUnauthorized: false });
sock.write('ping');
let back = '';
for await (const chunk of sock) { back += new TextDecoder().decode(chunk); break; }
console.log('[tls] connected:', back.length > 0);
sock.destroy();
server.close();
