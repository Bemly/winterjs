//! 本体小工具 JS 面：`WinterJS2.semver/yaml/jsonc/ip/shlex/spdx/qrcode`
//!（prelude 分域；拼接顺序见 mod.rs）。
//!
//! 薄壳规则：校验在调 native 之前；错误 plain `TypeError`，不带 node code。
pub const WSTD_JS: &str = r#"
{
  const __wstd_str = (v, what) => {
    if (typeof v !== "string") throw new TypeError(`${what} requires a string`);
    return v;
  };
  // native 侧 `report_error("TypeError: …")` 落通用 Error 类（4.208 同族）：
  // 薄壳统一改包真 TypeError（纯函数面，无 PermissionError 分支）。
  const __wstd_call = (f) => {
    try { return f(); } catch (e) {
      throw new TypeError(String((e && e.message) || e).replace(/^TypeError:\s*/, ""));
    }
  };
  const __wstd_json = (s) => JSON.parse(s);
  const semver = {
    valid(v) { return __wjs2_wstd_semver_valid(__wstd_str(v, "semver")); },
    parse(v) { return __wstd_call(() => __wstd_json(__wjs2_wstd_semver_parse(__wstd_str(v, "semver")))); },
    satisfies(v, r) { return __wstd_call(() => __wjs2_wstd_semver_satisfies(__wstd_str(v, "semver"), __wstd_str(r, "range"))); },
    compare(a, b) { return __wstd_call(() => __wjs2_wstd_semver_compare(__wstd_str(a, "semver"), __wstd_str(b, "semver"))); },
  };
  const yaml = {
    parse(s) { return __wstd_call(() => __wstd_json(__wjs2_wstd_yaml_parse(__wstd_str(s, "yaml")))); },
    stringify(v) { return __wstd_call(() => __wjs2_wstd_yaml_stringify(JSON.stringify(v))); },
  };
  const jsonc = {
    parse(s) { return __wstd_call(() => __wstd_json(__wjs2_wstd_jsonc_parse(__wstd_str(s, "jsonc")))); },
  };
  const ip = {
    isNet(s) { return __wjs2_wstd_ip_is_net(__wstd_str(s, "ip")); },
    isAddr(s) { return __wjs2_wstd_ip_is_addr(__wstd_str(s, "ip")); },
    contains(net, addr) { return __wstd_call(() => __wjs2_wstd_ip_contains(__wstd_str(net, "ip"), __wstd_str(addr, "ip"))); },
    parse(net) { return __wstd_call(() => __wstd_json(__wjs2_wstd_ip_parse(__wstd_str(net, "ip")))); },
  };
  const shlex = {
    split(s) { return __wstd_call(() => __wstd_json(__wjs2_wstd_shlex_split(__wstd_str(s, "shlex")))); },
  };
  const spdx = {
    valid(e) { return __wjs2_wstd_spdx_valid(__wstd_str(e, "spdx")); },
  };
  const qrcode = (text) => __wstd_call(() => __wjs2_wstd_qrcode(__wstd_str(text, "qrcode")));
  try {
    const W = globalThis.WinterJS2;
    if (W && W.semver === undefined) {
      W.semver = semver;
      W.yaml = yaml;
      W.jsonc = jsonc;
      W.ip = ip;
      W.shlex = shlex;
      W.spdx = spdx;
      W.qrcode = qrcode;
    }
  } catch {}
}
"#;
