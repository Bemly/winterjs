// node:https: same loopback shape as tls (fixture cert, no verification).
// HTTPS：与 tls 同形的回环请求（样例证书，不校验）。
// Run / 运行: winterjs2 --run sample/https/get.js
import https from 'node:https';
import fs from 'node:fs';

const opts = {
  key: fs.readFileSync(new URL('../tls/fixtures/key.pem', import.meta.url)),
  cert: fs.readFileSync(new URL('../tls/fixtures/cert.pem', import.meta.url)),
};

const server = https.createServer(opts, (req, res) => {
  // Known deviation (see sample/docs/api.en.md §https): the 'request'
  // listener currently fires twice per connection — guard idempotently.
  if (req.url !== '/hi' || res.headersSent) return;
  res.writeHead(200, { 'content-type': 'text/plain' });
  res.end(`https:${req.url}`);
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const port = server.address().port;

const body = await new Promise((resolve, reject) => {
  https
    .get({ host: '127.0.0.1', port, path: '/hi', rejectUnauthorized: false }, (res) => {
      let data = '';
      res.on('data', (c) => {
        data += c;
      });
      res.on('end', () => resolve(data));
    })
    .on('error', reject);
});
console.log('[https] body:', body === 'https:/hi');
server.close();
