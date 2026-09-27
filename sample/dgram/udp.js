// node:dgram UDP echo on loopback.
// UDP 回声（回环）。
// Run / 运行: winterjs --run sample/dgram/udp.js
import dgram from 'node:dgram';

const server = dgram.createSocket('udp4');
await new Promise((r) => server.bind(0, '127.0.0.1', r));
const port = server.address().port;
server.on('message', (msg, rinfo) => {
  server.send(`echo:${msg}`, rinfo.port, rinfo.address);
});

const client = dgram.createSocket('udp4');
const got = await new Promise((resolve, reject) => {
  client.on('message', (msg) => {
    resolve(String(msg));
    client.close();
  });
  client.on('error', reject);
  client.send('ping', port, '127.0.0.1');
});
console.log('[dgram] echo:', got === 'echo:ping');
server.close();
