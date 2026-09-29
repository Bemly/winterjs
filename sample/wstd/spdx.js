// WinterJS.spdx: license expression validation.
// WinterJS.spdx：SPDX 许可证表达式校验。
// Run / 运行: winterjs --run sample/wstd/spdx.js
console.log('[spdx] or:', WinterJS.spdx.valid('MIT OR Apache-2.0') === true);
console.log('[spdx] single:', WinterJS.spdx.valid('MIT') === true);
console.log('[spdx] invalid:', WinterJS.spdx.valid('nope-not-a-license') === false);
