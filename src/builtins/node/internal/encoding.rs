//! `node:internal/encoding`——`internal/encoding` 对位：全局 TextEncoder/TextDecoder。
/// 源：nodejs/node（MIT）对应 internal 件的最小对位实现；偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"const TextEncoder = globalThis.TextEncoder;
const TextDecoder = globalThis.TextDecoder;
export { TextEncoder, TextDecoder };
export default { TextEncoder, TextDecoder };

"#;
