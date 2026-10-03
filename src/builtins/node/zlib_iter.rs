//! `node:zlib/iter`（Node lib/zlib/iter.js 逐字内嵌，MIT；`--experimental-stream-iter` 门控）。
//! 来源见 `zlib_iter.js`（`internal/streams/iter/transform` 薄包装）；注册门控见 `node::normalize_spec`。

/// 内嵌源（§0.9 单片，未超限）。
pub const SOURCE: &str = include_str!("zlib_iter.js");
