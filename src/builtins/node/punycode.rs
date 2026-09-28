//! `node:punycode`（算法核在 prelude `punycode.rs`，与 `WinterJS.punycode`
//! 同源；MIT，原 punycode.js 2.1.0 逐字移植）。
//! 本模块只发 DEP0040 并重导出（CJS `module.exports = punycode` → ESM）。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node lib/punycode.js (punycode.js 2.1.0, verbatim).
'use strict';

// 10f：DEP0040 弃用警告（真机 require 期即发；test-punycode.js 点名）。
if (typeof process === 'object' && typeof process.emitWarning === 'function') {
  process.emitWarning(
    'The `punycode` module is deprecated. Please use a userland alternative instead.',
    { type: 'DeprecationWarning', code: 'DEP0040' });
}
const __core = globalThis.__wjs_puny;
if (!__core || typeof __core.encode !== 'function') throw new Error('punycode core missing (prelude)');

/** Define the public API (same shape as the verbatim port) */
const punycode = {
	'version': '2.1.0',
	'ucs2': {
		'decode': __core.ucs2.decode,
		'encode': __core.ucs2.encode
	},
	'decode': __core.decode,
	'encode': __core.encode,
	'toASCII': __core.toASCII,
	'toUnicode': __core.toUnicode
};
const { ucs2decode, ucs2encode, decode, encode, toASCII, toUnicode } = {
	ucs2decode: __core.ucs2.decode,
	ucs2encode: __core.ucs2.encode,
	decode: __core.decode,
	encode: __core.encode,
	toASCII: __core.toASCII,
	toUnicode: __core.toUnicode
};
export default punycode;
export { ucs2decode, ucs2encode, decode, encode, toASCII, toUnicode };
"#;
