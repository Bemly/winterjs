// WinterJS2.spdx: license expression validation.
// WinterJS2.spdx：SPDX 许可证表达式校验。
// Run / 运行: winterjs2 --run sample/wstd/spdx.js
console.log('[spdx] or:', WinterJS2.spdx.valid('MIT OR Apache-2.0') === true);
console.log('[spdx] single:', WinterJS2.spdx.valid('MIT') === true);
console.log('[spdx] invalid:', WinterJS2.spdx.valid('nope-not-a-license') === false);
