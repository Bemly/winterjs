// node:tls: server + client with local fixture cert (rejectUnauthorized:false).
// TLS：本地自签证书的服务端与客户端（仅样例与测试用证书，勿用于生产）。
// Run / 运行: winterjs --run sample/tls/server-client.js
import tls from 'node:tls';
import fs from 'node:fs';

const opts = {
  key: fs.readFileSync(new URL('./fixtures/key.pem', import.meta.url)),
  cert: fs.readFileSync(new URL('./fixtures/cert.pem', import.meta.url)),
};

const server = tls.createServer(opts, (socket) => {
  socket.write('secure-hello');
  socket.end();
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const port = server.address().port;

const got = await new Promise((resolve, reject) => {
  const client = tls.connect({ host: '127.0.0.1', port, rejectUnauthorized: false }, () => {
    console.log('[tls] authorized:', client.authorized === false, 'protocol:', typeof client.getProtocol());
  });
  let data = '';
  client.on('data', (c) => {
    data += c;
  });
  client.on('end', () => resolve(data));
  client.on('error', reject);
});
console.log('[tls] echo:', got === 'secure-hello');
server.close();
