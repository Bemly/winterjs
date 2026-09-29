// WinterJS.path: path utilities.
// WinterJS.path：路径工具。
// Run / 运行: winterjs --run sample/wcover/path.js
console.log('[path] join:', WinterJS.path.join('a', 'b') === 'a/b');
console.log('[path] basename:', WinterJS.path.basename('/x/y.js') === 'y.js');
