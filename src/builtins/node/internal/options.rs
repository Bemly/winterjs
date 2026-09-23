//! `node:internal/options`——getOptionValue 存根（默认关；偏差记档）。
/// 源：nodejs/node（MIT）对应 internal 件的最小对位实现；偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"const options = {
  '--pending-deprecation': false,
  '--no-deprecation': false,
  '--enable-source-maps': false,
  '--experimental-stream-iter': false,
  // node 口径：max-http-header-size 缺省 16384（max-http-headers 套件）。
  '--max-http-header-size': 16384,
  hasIntl: false,
};
function getOptionValue(name) {
  return options[name] ?? false;
}
export { getOptionValue, options };
export default { getOptionValue, options };

"#;
