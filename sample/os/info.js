// node:os: platform / arch / cpus / memory / user / uptime / network.
// 系统信息：平台、架构、CPU、内存、用户、运行时长与网卡。
// Run / 运行: winterjs --run sample/os/info.js
import os from 'node:os';

console.log('[os] platform/arch/release:', os.platform(), os.arch(), typeof os.release());
console.log('[os] cpus:', os.cpus().length > 0, 'model:', typeof os.cpus()[0].model);
console.log('[os] mem bytes:', os.totalmem() > 0, os.freemem() >= 0);
console.log('[os] homedir/tmpdir:', typeof os.homedir(), typeof os.tmpdir());
console.log('[os] hostname:', typeof os.hostname(), 'uptime>=0:', os.uptime() >= 0);
console.log('[os] loadavg len:', os.loadavg().length === 3);
console.log('[os] user:', typeof os.userInfo().username);
console.log('[os] net ifs:', Object.keys(os.networkInterfaces()).length >= 0);
console.log('[os] EOL:', JSON.stringify(os.EOL));
