// Demonstrates reading and writing files: sync APIs, encodings, callback forms, and fs.promises. / 演示文件读写：同步 API、编码、回调形式与 fs.promises。
// Run / 运行: winterjs --run sample/fs/read-write.js
import fs from 'node:fs';
import { readFileSync, writeFileSync, appendFileSync, readFile, writeFile, promises } from 'node:fs';
import { readFile as readFileP, writeFile as writeFileP } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'wjs-fs-rw-'));
const file = path.join(dir, 'hello.txt');

// 1. Sync write + read (utf8 default)
writeFileSync(file, 'hello winterjs');
console.log('[fs] readFileSync:', readFileSync(file, 'utf8'));

// 2. Sync append + encoding round-trips
appendFileSync(file, '\nsecond line');
console.log('[fs] lines:', readFileSync(file, 'utf8').trim().split('\n').length);

writeFileSync(path.join(dir, 'bin.bin'), Buffer.from([0x00, 0xff, 0x10]));
console.log('[fs] bytes:', [...readFileSync(path.join(dir, 'bin.bin'))].map((b) => b.toString(16)).join(' '));

const hexFile = path.join(dir, 'hex.txt');
writeFileSync(hexFile, 'cafebabe', 'hex');
console.log('[fs] hex->bytes length:', readFileSync(hexFile).length);
console.log('[fs] base64 read:', readFileSync(hexFile, 'base64url'));

// 3. Callback forms (err-first)
readFile(file, 'utf8', (err, data) => {
  if (err) throw err;
  console.log('[fs] readFile cb:', JSON.stringify(data));
  writeFile(path.join(dir, 'cb.txt'), 'written via callback', (err2) => {
    if (err2) throw err2;
    console.log('[fs] writeFile cb ok:', fs.existsSync(path.join(dir, 'cb.txt')));
    runPromises();
  });
});

// 4. fs.promises + node:fs/promises (same object under the hood)
async function runPromises() {
  const p = path.join(dir, 'promises.txt');
  await writeFileP(p, 'from fs/promises');
  console.log('[fs] promises read:', await readFileP(p, 'utf8'));
  await promises.writeFile(path.join(dir, 'p2.txt'), 'from fs.promises');
  console.log('[fs] fs.promises read:', await promises.readFile(path.join(dir, 'p2.txt'), 'utf8'));

  // 5. Encoding option objects work too
  console.log('[fs] opts encoding:', readFileSync(hexFile, { encoding: 'hex' }).slice(0, 8));

  fs.rmSync(dir, { recursive: true, force: true });
  console.log('[fs] cleaned up:', !fs.existsSync(dir));
  process.exit(0);
}
