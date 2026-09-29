// node:child_process: self-spawn via process.execPath (cross-platform, no shell).
// 子进程：用自身可执行文件启动子进程（跨平台，不依赖 shell）。
// Run / 运行: winterjs2 --run sample/child-process/spawn.js
import { spawnSync, execFileSync } from 'node:child_process';

const r = spawnSync(process.execPath, ['--eval', 'console.log(20 + 22)'], { encoding: 'utf8' });
console.log('[child] spawnSync:', r.status === 0 && r.stdout.trim() === '42');

const out = execFileSync(process.execPath, ['--eval', "'hi'.toUpperCase()"], { encoding: 'utf8' });
console.log('[child] execFile:', out.trim() === 'HI');
