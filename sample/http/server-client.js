// node:http: server + client round-trip (JSON echo, keep-alive agent).
// HTTP：服务端与客户端往返（JSON 回声、复用连接）。
// Run / 运行: winterjs --run sample/http/server-client.js
import http from 'node:http';

const server = http.createServer((req, res) => {
  let body = '';
  req.on('data', (c) => {
    body += c;
  });
  req.on('end', () => {
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ method: req.method, url: req.url, body }));
  });
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const port = server.address().port;

const agent = new http.Agent({ keepAlive: true });
const post = (payload) =>
  new Promise((resolve, reject) => {
    const req = http.request({ host: '127.0.0.1', port, path: '/api', method: 'POST', agent }, (res) => {
      let data = '';
      res.on('data', (c) => {
        data += c;
      });
      res.on('end', () => resolve({ status: res.statusCode, data }));
    });
    req.on('error', reject);
    req.end(payload);
  });

const r1 = await post('one');
const r2 = await post('two');
console.log('[http] statuses:', r1.status === 200 && r2.status === 200);
console.log('[http] echo:', JSON.parse(r2.data).body === 'two');
console.log('[http] reused socket:', agent.getName({}) === agent.getName({}));
server.close();
agent.destroy();
