// Demonstrates directory operations: mkdir, readdir (Dirent), stat, copy, rm, exists. / 演示目录操作：mkdir、readdir（Dirent）、stat、copy、rm、exists。
// Run / 运行: winterjs2 --run sample/fs/directory.js
import fs from 'node:fs';
import { mkdirSync, readdirSync, statSync, copyFileSync, rmSync, existsSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const base = fs.mkdtempSync(path.join(os.tmpdir(), 'wjs-fs-dir-'));

// 1. mkdirSync: plain and recursive (recursive returns the first created path)
mkdirSync(path.join(base, 'plain'));
const first = mkdirSync(path.join(base, 'a/b/c'), { recursive: true });
console.log('[fs] recursive mkdir created:', path.basename(first));

// 2. readdirSync: names and withFileTypes Dirents
fs.writeFileSync(path.join(base, 'a', 'file.txt'), 'x');
fs.writeFileSync(path.join(base, 'a', 'b', 'deep.md'), 'y');
console.log('[fs] readdir names:', readdirSync(path.join(base, 'a')));
for (const d of readdirSync(path.join(base, 'a'), { withFileTypes: true })) {
  console.log('[fs] dirent:', d.name, 'isDir=' + d.isDirectory(), 'isFile=' + d.isFile(), 'parent=' + path.basename(d.parentPath));
}

// 3. Recursive listing (supported option)
console.log('[fs] recursive:', readdirSync(path.join(base, 'a'), { recursive: true }).sort().join(','));

// 4. statSync basics
const st = statSync(path.join(base, 'a', 'file.txt'));
console.log('[fs] stat:', 'size=' + st.size, 'isFile=' + st.isFile(), 'isDir=' + st.isDirectory(), 'mtime=' + (st.mtime instanceof Date));
const missing = statSync(path.join(base, 'nope'), { throwIfNoEntry: false });
console.log('[fs] stat missing (throwIfNoEntry:false):', missing === undefined);

// 5. copyFileSync + existsSync + rmSync
copyFileSync(path.join(base, 'a', 'file.txt'), path.join(base, 'plain', 'copy.txt'));
console.log('[fs] copy exists:', existsSync(path.join(base, 'plain', 'copy.txt')));
rmSync(path.join(base, 'plain', 'copy.txt'));
console.log('[fs] after rm exists:', existsSync(path.join(base, 'plain', 'copy.txt')));

// 6. mkdirSync on existing directory without recursive throws EEXIST
try {
  mkdirSync(path.join(base, 'plain'));
} catch (e) {
  console.log('[fs] EEXIST:', e.code, 'syscall=' + e.syscall);
}

rmSync(base, { recursive: true, force: true });
console.log('[fs] cleaned up:', !existsSync(base));
