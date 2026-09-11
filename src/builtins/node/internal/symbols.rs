//! `node:internal/events/symbols`（Node `lib/internal/events/symbols.js` 逐字，MIT）。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
const kFirstEventParam = Symbol('kFirstEventParam');

export { kFirstEventParam };
export default { kFirstEventParam };
"#;
