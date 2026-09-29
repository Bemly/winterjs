// WinterJS2.transpile：TypeScript 转译（去类型注记，全离线）。
// WinterJS2.transpile: TypeScript-to-JS transpile (type stripping, fully offline).
// Run / 运行: winterjs2 --run sample/wsys/transpile.js
console.log('[transpile] const:', WinterJS2.transpile('const x: number = 1;').includes('const x = 1'));
console.log('[transpile] export:', WinterJS2.transpile('const x: number = 1; export default x;').includes('const x = 1'));
