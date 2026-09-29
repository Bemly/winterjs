// require: CJS module loading (exports, builtin modules, resolve).
// require：CJS 模块加载（导出对象、内建模块、路径解析）。
// Run / 运行: winterjs --run sample/process/require.js
const helper = require('./helper.cjs');
console.log('[require] cjs exports:', helper.double(21) === 42 && helper.TAG === 'require-tag');
console.log('[require] builtin:', typeof require('node:path').join === 'function');
console.log('[require] resolve:', require.resolve('./helper.cjs').endsWith('helper.cjs'));
