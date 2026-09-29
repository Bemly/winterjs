// node:path (+ posix/win32): join / resolve / parse / relative / extname.
// 路径：拼接、解析、相对路径与扩展名（含 posix/win32 子路径）。
// Run / 运行: winterjs2 --run sample/path/basics.js
import path, { join, resolve, basename, dirname, extname } from 'node:path';
import posix from 'node:path/posix';
import win32 from 'node:path/win32';

console.log('[path] join:', join('/a', 'b', '..', 'c'));
console.log('[path] basename/ext:', basename('/a/b/c.txt'), extname('/a/b/c.txt'), dirname('/a/b/c.txt'));
console.log('[path] parse name:', path.parse('/a/b/c.txt').name);
console.log('[path] relative:', path.relative('/a/b', '/a/b/c/d.js'));
console.log('[path] isAbsolute:', path.isAbsolute('/x'), path.isAbsolute('x'));
console.log('[path] sep/delimiter:', JSON.stringify(path.sep), JSON.stringify(path.delimiter));
console.log('[path] posix join:', posix.join('a', 'b'));
console.log('[path] win32 join:', win32.join('C:\\a', 'b'), 'sep:', JSON.stringify(win32.sep));
console.log('[path] resolve tail:', resolve('/a', 'b').endsWith('b'));
