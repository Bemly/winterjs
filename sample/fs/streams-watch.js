// node:fs streams + watch: pipe a file, then watchFile a temp file.
// fs 流与监听：管道拷贝文件 + 轮询监听临时文件。
// Run / 运行: winterjs2 --run sample/fs/streams-watch.js
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'wjs-fs-sw-'));
const src = path.join(dir, 'src.txt');
const dst = path.join(dir, 'dst.txt');
fs.writeFileSync(src, 'stream-payload');

await new Promise((resolve, reject) => {
  const rs = fs.createReadStream(src);
  const ws = fs.createWriteStream(dst);
  rs.on('error', reject);
  ws.on('error', reject);
  ws.on('finish', resolve);
  rs.pipe(ws);
});
console.log('[fs-sw] piped:', fs.readFileSync(dst, 'utf8') === 'stream-payload');

const target = path.join(dir, 'watched.txt');
fs.writeFileSync(target, 'v1');
let fired = false;
fs.watchFile(target, { interval: 50 }, () => {
  fired = true;
});
fs.writeFileSync(target, 'v2');
await new Promise((r) => setTimeout(r, 300));
fs.unwatchFile(target);
console.log('[fs-sw] watchFile fired:', fired);
fs.rmSync(dir, { recursive: true, force: true });
