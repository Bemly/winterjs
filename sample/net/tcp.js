// node:net TCP echo: server + client on loopback in one file.
// TCP 回声：同一文件起服务与客户端（回环）。
// Run / 运行: winterjs --run sample/net/tcp.js
import net from 'node:net';

const server = net.createServer((socket) => {
  socket.on('data', (chunk) => socket.write(`echo:${chunk}`));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const port = server.address().port;

const got = await new Promise((resolve, reject) => {
  const client = net.connect(port, '127.0.0.1', () => client.write('ping'));
  let data = '';
  client.on('data', (c) => {
    data += c;
    client.end();
  });
  client.on('close', () => resolve(data));
  client.on('error', reject);
});
console.log('[net] echo:', got === 'echo:ping');
console.log('[net] isIP:', net.isIP('127.0.0.1') === 4 && net.isIPv6('::1'));
server.close();
