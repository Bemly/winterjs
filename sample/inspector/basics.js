// node:inspector: open/url/close lifecycle (Bridge: no live debugging wire,
// url() stays undefined — documented in sample/docs).
// inspector：开关生命周期（无实时调试连接，url() 为 undefined）。
// Run / 运行: winterjs --run sample/inspector/basics.js
import inspector from 'node:inspector';

console.log('[inspector] has open/close:', typeof inspector.open === 'function' && typeof inspector.close === 'function');
inspector.open(19311, '127.0.0.1');
console.log('[inspector] url shape:', inspector.url() === undefined);
inspector.close();
console.log('[inspector] closed cleanly');
