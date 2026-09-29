// module: ESM import plus require(esm) interop.
// module：ESM 导入与 require(esm) 互操作。
// Run / 运行: winterjs --run sample/process/module.js
const m = await import('./helper.mjs');
console.log('[module] dynamic import:', m.triple(14) === 42 && m.NAME === 'esm-tag');
console.log('[module] require-esm:', require('./helper.mjs').triple(7) === 21);
