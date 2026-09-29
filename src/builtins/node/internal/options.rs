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
// CLI 起点剥下的 node 运行时旗（`__wjs2_nodeCompat`，见 cli::strip_node_compat_args）
// 覆盖缺省：`--k` 即 true、`--no-k` 即 k=false、`--k=v` 数字形转数、余为串。
(() => {
  const flags = Array.isArray(globalThis.__wjs2_nodeCompat) ? globalThis.__wjs2_nodeCompat : [];
  for (const f of flags) {
    const eq = f.indexOf('=');
    if (eq > 0) {
      const v = f.slice(eq + 1);
      options[f.slice(0, eq)] = v !== '' && !Number.isNaN(Number(v)) ? Number(v) : v;
    } else {
      options[f] = true;
      if (f.startsWith('--no-')) options['--' + f.slice(5)] = false;
    }
  }
})();
function getOptionValue(name) {
  return options[name] ?? false;
}
export { getOptionValue, options };
export default { getOptionValue, options };

"#;
