// node:module + CJS/ESM interop: require, createRequire, __dirname.
// 模块互操作：require、CJS 变量与 createRequire。
// Run / 运行: winterjs2 --run sample/module/main.mjs
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const helper = require('./helper.cjs');
console.log('[module] cjs require:', helper.double(21) === 42, helper.TAG);

console.log('[module] builtin require:', typeof require('node:path').join === 'function');
console.log('[module] resolve:', require.resolve('./helper.cjs').endsWith('helper.cjs'));
