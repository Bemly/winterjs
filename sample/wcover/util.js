// WinterJS2.util: printf-style format and object inspect.
// WinterJS2.util：printf 风格 format 与对象 inspect。
// Run / 运行: winterjs2 --run sample/wcover/util.js
console.log('[util] format:', WinterJS2.util.format('%s=%d', 'a', 1) === 'a=1');
console.log('[util] inspect:', WinterJS2.util.inspect({ a: 1 }).includes('a: 1'));
console.log('[util] inspect-depth:', WinterJS2.util.inspect({ a: { b: 2 } }, { depth: 0 }).includes('a:'));
