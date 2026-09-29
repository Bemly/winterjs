// WinterJS.util: printf-style format and object inspect.
// WinterJS.util：printf 风格 format 与对象 inspect。
// Run / 运行: winterjs --run sample/wcover/util.js
console.log('[util] format:', WinterJS.util.format('%s=%d', 'a', 1) === 'a=1');
console.log('[util] inspect:', WinterJS.util.inspect({ a: 1 }).includes('a: 1'));
console.log('[util] inspect-depth:', WinterJS.util.inspect({ a: { b: 2 } }, { depth: 0 }).includes('a:'));
