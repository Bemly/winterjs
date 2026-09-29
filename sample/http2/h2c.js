// node:http2 (h2c plaintext): compat server + client session in one file.
// HTTP/2 明文：同一文件起兼容服务与客户端会话。
// Run / 运行: winterjs2 --run sample/http2/h2c.js
import http2 from 'node:http2';

const server = http2.createServer();
server.on('stream', (stream, headers) => {
  stream.respond({ ':status': 200, 'content-type': 'text/plain' });
  stream.end(`h2:${headers[':path']}`);
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const port = server.address().port;

const client = http2.connect(`http://127.0.0.1:${port}`);
const body = await new Promise((resolve, reject) => {
  const req = client.request({ ':path': '/demo' });
  let data = '';
  req.on('data', (c) => {
    data += c;
  });
  req.on('end', () => resolve(data));
  req.on('error', reject);
  req.end();
});
console.log('[http2] body:', body === 'h2:/demo');
client.close();
server.close();
