//! 本体小工具面（WinterJS.semver/yaml/jsonc/ip/shlex/spdx/qrcode）。
//!
//! 范围（用户拍板）：Web 标准与已有本体面跳过，真缺口全进 `WinterJS.*`。
//! 全员纯函数、直用树内轮子（deno_semver/npm 语义 + yaml-rust2 + jsonc-parser +
//! ipnet + shlex + spdx + qrcode），零新增依赖；错误 plain `TypeError`，
//! 无 node 错误码口径。`matches` 的 tag range（如 `latest`）不 panic，
//! 改走可读错（deno 侧 `matches` 遇 tag 即 panic）。

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::context::JSContext;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};

// ── natives ────────────────────────────────────────────────────────────────
// UNSAFE-BOUNDARY：全部 JSNative 入口经 `wrap_cx` + `Frame::from_raw`（结构性边界块）；
// 前置：调用方 realm 内 + 参数槽 rooted 后才分配；
// 覆盖：`tests/wstd.rs`（正常/报错/边界）。

fn arg_str(cx: &mut JSContext, frame: &Frame, i: u32, what: &str) -> Option<String> {
    if frame.argc() <= i {
        report_error(cx, &format!("TypeError: {what} requires an argument"));
        return None;
    }
    Some(value_to_string(cx, frame.arg(i)))
}

fn set_json(cx: &mut JSContext, frame: &Frame, v: &serde_json::Value) {
    let text = v.to_string();
    frame.set_rval({
        rooted!(&in(cx) let mut out = UndefinedValue());
        text.to_jsval(cx, out.handle_mut());
        out.get()
    });
}

fn set_str(cx: &mut JSContext, frame: &Frame, s: &str) {
    frame.set_rval({
        rooted!(&in(cx) let mut out = UndefinedValue());
        s.to_jsval(cx, out.handle_mut());
        out.get()
    });
}

fn set_bool(frame: &Frame, b: bool) {
    frame.set_rval(mozjs::jsval::BooleanValue(b));
}

/// `__wjs_wstd_semver_valid(v)` → bool（npm 语义）。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wstd.rs::wstd_semver_faces`。
pub unsafe extern "C" fn semver_valid(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(v) = arg_str(&mut cx, &frame, 0, "semver valid") else {
        return false;
    };
    set_bool(&frame, deno_semver::Version::parse_from_npm(&v).is_ok());
    true
}

fn version_json(v: &deno_semver::Version) -> serde_json::Value {
    serde_json::json!({
        "major": v.major,
        "minor": v.minor,
        "patch": v.patch,
        "pre": v.pre.iter().map(|p| p.to_string()).collect::<Vec<_>>(),
        "build": v.build.iter().map(|p| p.to_string()).collect::<Vec<_>>(),
    })
}

/// `__wjs_wstd_semver_parse(v)` → `{major,minor,patch,pre,build}` JSON 串。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wstd.rs::wstd_semver_faces`。
pub unsafe extern "C" fn semver_parse(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(v) = arg_str(&mut cx, &frame, 0, "semver parse") else {
        return false;
    };
    match deno_semver::Version::parse_from_npm(&v) {
        Ok(ver) => {
            set_json(&mut cx, &frame, &version_json(&ver));
            true
        }
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: invalid version: {e}"));
            false
        }
    }
}

/// `__wjs_wstd_semver_satisfies(v, range)` → bool（tag range 报可读错，不 panic）。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wstd.rs::wstd_semver_faces`。
pub unsafe extern "C" fn semver_satisfies(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(v), Some(r)) = (
        arg_str(&mut cx, &frame, 0, "semver satisfies"),
        arg_str(&mut cx, &frame, 1, "semver satisfies"),
    ) else {
        return false;
    };
    let ver = match deno_semver::Version::parse_from_npm(&v) {
        Ok(ver) => ver,
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: invalid version: {e}"));
            return false;
        }
    };
    let req = match deno_semver::VersionReq::parse_from_npm(&r) {
        Ok(req) => req,
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: invalid range: {e}"));
            return false;
        }
    };
    if req.tag().is_some() {
        report_error(&mut cx, "TypeError: satisfies does not accept tag ranges (e.g. latest)");
        return false;
    }
    set_bool(&frame, req.matches(&ver));
    true
}

fn cmp_version(a: &deno_semver::Version, b: &deno_semver::Version) -> i32 {
    match a.major.cmp(&b.major) {
        std::cmp::Ordering::Equal => {}
        o => return if o == std::cmp::Ordering::Less { -1 } else { 1 },
    }
    match a.minor.cmp(&b.minor) {
        std::cmp::Ordering::Equal => {}
        o => return if o == std::cmp::Ordering::Less { -1 } else { 1 },
    }
    match a.patch.cmp(&b.patch) {
        std::cmp::Ordering::Equal => {}
        o => return if o == std::cmp::Ordering::Less { -1 } else { 1 },
    }
    // 空 pre（正式版）> 非空 pre；双非空按连接串字典序（近似，文档记录）。
    let (ae, be) = (a.pre.is_empty(), b.pre.is_empty());
    if ae && be {
        return 0;
    }
    if ae {
        return 1;
    }
    if be {
        return -1;
    }
    let (sa, sb) = (
        a.pre.iter().map(|p| p.to_string()).collect::<Vec<_>>().join("."),
        b.pre.iter().map(|p| p.to_string()).collect::<Vec<_>>().join("."),
    );
    match sa.cmp(&sb) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// `__wjs_wstd_semver_compare(a, b)` → -1|0|1。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wstd.rs::wstd_semver_faces`。
pub unsafe extern "C" fn semver_compare(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(a), Some(b)) = (
        arg_str(&mut cx, &frame, 0, "semver compare"),
        arg_str(&mut cx, &frame, 1, "semver compare"),
    ) else {
        return false;
    };
    let (Ok(va), Ok(vb)) = (
        deno_semver::Version::parse_from_npm(&a),
        deno_semver::Version::parse_from_npm(&b),
    ) else {
        report_error(&mut cx, "TypeError: semver compare requires valid versions");
        return false;
    };
    frame.set_rval(mozjs::jsval::Int32Value(cmp_version(&va, &vb)));
    true
}

fn yaml_to_json(y: &yaml_rust2::Yaml) -> Result<serde_json::Value, String> {
    use yaml_rust2::Yaml::*;
    match y {
        Null => Ok(serde_json::Value::Null),
        Boolean(b) => Ok(serde_json::Value::Bool(*b)),
        Integer(n) => Ok(serde_json::Value::Number((*n).into())),
        Real(s) => match s.parse::<f64>() {
            Ok(f) => serde_json::Number::from_f64(f)
                .map(serde_json::Value::Number)
                .ok_or_else(|| format!("bad YAML number: {s}")),
            Err(_) => Ok(serde_json::Value::String(s.clone())),
        },
        String(s) => Ok(serde_json::Value::String(s.clone())),
        Array(items) => items.iter().map(yaml_to_json).collect(),
        Hash(map) => {
            let mut o = serde_json::Map::new();
            for (k, v) in map {
                let key = match k {
                    String(s) => s.clone(),
                    Integer(n) => n.to_string(),
                    Boolean(b) => b.to_string(),
                    Real(s) => s.clone(),
                    Null => "null".to_string(),
                    _ => return Err("YAML mapping keys must be scalars".to_string()),
                };
                o.insert(key, yaml_to_json(v)?);
            }
            Ok(serde_json::Value::Object(o))
        }
        Alias(_) => Err("YAML aliases are not supported".to_string()),
        BadValue => Err("bad YAML value".to_string()),
    }
}

fn json_to_yaml(v: &serde_json::Value) -> yaml_rust2::Yaml {
    use yaml_rust2::Yaml::*;
    match v {
        serde_json::Value::Null => Null,
        serde_json::Value::Bool(b) => Boolean(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Integer(i)
            } else if let Some(f) = n.as_f64() {
                Real(format!("{f}"))
            } else {
                String(n.to_string())
            }
        }
        serde_json::Value::String(s) => String(s.clone()),
        serde_json::Value::Array(items) => Array(items.iter().map(json_to_yaml).collect()),
        serde_json::Value::Object(map) => Hash(
            map.iter()
                .map(|(k, v)| (String(k.clone()), json_to_yaml(v)))
                .collect(),
        ),
    }
}

/// `__wjs_wstd_yaml_parse(s)` → 首文档 JSON 串（空即 `"null"`）。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wstd.rs::wstd_yaml_jsonc_faces`。
pub unsafe extern "C" fn yaml_parse(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(s) = arg_str(&mut cx, &frame, 0, "yaml parse") else {
        return false;
    };
    tracing::debug!(target: "winterjs::wstd", src_len = s.len(), "yaml parse");
    match yaml_rust2::YamlLoader::load_from_str(&s) {
        Ok(docs) => {
            let v = match docs.first() {
                Some(doc) => match yaml_to_json(doc) {
                    Ok(v) => v,
                    Err(msg) => {
                        report_error(&mut cx, &format!("TypeError: {msg}"));
                        return false;
                    }
                },
                None => serde_json::Value::Null,
            };
            set_json(&mut cx, &frame, &v);
            true
        }
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: invalid YAML: {e}"));
            false
        }
    }
}

/// `__wjs_wstd_yaml_stringify(json)`（JS 传 `JSON.stringify` 结果）。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wstd.rs::wstd_yaml_jsonc_faces`。
pub unsafe extern "C" fn yaml_stringify(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(s) = arg_str(&mut cx, &frame, 0, "yaml stringify") else {
        return false;
    };
    let v: serde_json::Value = match serde_json::from_str(&s) {
        Ok(v) => v,
        Err(_) => {
            report_error(&mut cx, "TypeError: yaml stringify requires serializable data");
            return false;
        }
    };
    let mut out = String::new();
    {
        let mut e = yaml_rust2::YamlEmitter::new(&mut out);
        if e.dump(&json_to_yaml(&v)).is_err() {
            report_error(&mut cx, "TypeError: yaml stringify failed");
            return false;
        }
    }
    set_str(&mut cx, &frame, &out);
    true
}

/// `__wjs_wstd_jsonc_parse(s)` → JSON 串（注释/尾逗号容忍）。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wstd.rs::wstd_yaml_jsonc_faces`。
pub unsafe extern "C" fn jsonc_parse(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(s) = arg_str(&mut cx, &frame, 0, "jsonc parse") else {
        return false;
    };
    tracing::debug!(target: "winterjs::wstd", src_len = s.len(), "jsonc parse");
    match jsonc_parser::parse_to_serde_value::<serde_json::Value>(&s, &Default::default()) {
        Ok(v) => {
            set_json(&mut cx, &frame, &v);
            true
        }
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: invalid JSONC: {e}"));
            false
        }
    }
}

/// `__wjs_wstd_ip_is_net(s)` / `__wjs_wstd_ip_is_addr(s)` → bool。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wstd.rs::wstd_ip_faces`。
pub unsafe extern "C" fn ip_is_net(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(s) = arg_str(&mut cx, &frame, 0, "ip isNet") else {
        return false;
    };
    set_bool(&frame, s.parse::<ipnet::IpNet>().is_ok());
    true
}

/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wstd.rs::wstd_ip_faces`。
pub unsafe extern "C" fn ip_is_addr(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(s) = arg_str(&mut cx, &frame, 0, "ip isAddr") else {
        return false;
    };
    set_bool(&frame, s.parse::<std::net::IpAddr>().is_ok());
    true
}

/// `__wjs_wstd_ip_contains(net, ip)` → bool。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wstd.rs::wstd_ip_faces`。
pub unsafe extern "C" fn ip_contains(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(n), Some(a)) = (
        arg_str(&mut cx, &frame, 0, "ip contains"),
        arg_str(&mut cx, &frame, 1, "ip contains"),
    ) else {
        return false;
    };
    let net: ipnet::IpNet = match n.parse() {
        Ok(net) => net,
        Err(_) => {
            report_error(&mut cx, "TypeError: ip contains requires a CIDR network");
            return false;
        }
    };
    let addr: std::net::IpAddr = match a.parse() {
        Ok(addr) => addr,
        Err(_) => {
            report_error(&mut cx, "TypeError: ip contains requires an IP address");
            return false;
        }
    };
    set_bool(&frame, net.contains(&addr));
    true
}

/// `__wjs_wstd_ip_parse(net)` → `{network,prefixLen,broadcast?}` JSON 串。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wstd.rs::wstd_ip_faces`。
pub unsafe extern "C" fn ip_parse(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(n) = arg_str(&mut cx, &frame, 0, "ip parse") else {
        return false;
    };
    let net: ipnet::IpNet = match n.parse() {
        Ok(net) => net,
        Err(_) => {
            report_error(&mut cx, "TypeError: ip parse requires a CIDR network");
            return false;
        }
    };
    let v = match net {
        ipnet::IpNet::V4(n) => serde_json::json!({
            "network": n.network().to_string(),
            "prefixLen": n.prefix_len(),
            "broadcast": n.broadcast().to_string(),
        }),
        ipnet::IpNet::V6(n) => serde_json::json!({
            "network": n.network().to_string(),
            "prefixLen": n.prefix_len(),
        }),
    };
    set_json(&mut cx, &frame, &v);
    true
}

/// `__wjs_wstd_shlex_split(s)` → 参数数组 JSON 串（引号不配对即错）。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wstd.rs::wstd_misc_faces`。
pub unsafe extern "C" fn shlex_split(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(s) = arg_str(&mut cx, &frame, 0, "shlex split") else {
        return false;
    };
    match shlex::split(&s) {
        Some(parts) => {
            set_json(&mut cx, &frame, &serde_json::Value::Array(
                parts.into_iter().map(serde_json::Value::String).collect(),
            ));
            true
        }
        None => {
            report_error(&mut cx, "TypeError: shlex split found unbalanced quotes");
            false
        }
    }
}

/// `__wjs_wstd_spdx_valid(expr)` → bool。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wstd.rs::wstd_misc_faces`。
pub unsafe extern "C" fn spdx_valid(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(s) = arg_str(&mut cx, &frame, 0, "spdx valid") else {
        return false;
    };
    set_bool(&frame, s.parse::<spdx::Expression>().is_ok());
    true
}

/// `__wjs_wstd_qrcode(text)` → 终端块字符画（与 `--serve` LAN 码同渲染）。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wstd.rs::wstd_misc_faces`。
pub unsafe extern "C" fn qrcode(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(s) = arg_str(&mut cx, &frame, 0, "qrcode") else {
        return false;
    };
    if s.is_empty() || s.len() > 2048 {
        report_error(&mut cx, "TypeError: qrcode text must be 1..2048 bytes");
        return false;
    }
    tracing::debug!(target: "winterjs::wstd", text_len = s.len(), "qrcode");
    match qrcode::QrCode::new(s.as_bytes()) {
        Ok(code) => {
            let art = code
                .render::<qrcode::render::unicode::Dense1x2>()
                .quiet_zone(false)
                .module_dimensions(2, 1)
                .build();
            set_str(&mut cx, &frame, &art);
            true
        }
        Err(_) => {
            report_error(&mut cx, "TypeError: qrcode cannot encode this text");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::cmp_version;

    fn ver(s: &str) -> deno_semver::Version {
        deno_semver::Version::parse_from_npm(s).unwrap()
    }

    #[test]
    fn wstd_cmp_orders_release_over_pre() {
        assert_eq!(cmp_version(&ver("1.2.3"), &ver("1.2.3")), 0);
        assert_eq!(cmp_version(&ver("1.2.3"), &ver("1.2.4")), -1);
        assert_eq!(cmp_version(&ver("2.0.0"), &ver("1.9.9")), 1);
        assert_eq!(cmp_version(&ver("1.0.0"), &ver("1.0.0-alpha")), 1);
        assert_eq!(cmp_version(&ver("1.0.0-alpha"), &ver("1.0.0")), -1);
    }

    #[test]
    fn wstd_yaml_roundtrip_shapes() {
        let v = super::yaml_to_json(&yaml_rust2::YamlLoader::load_from_str("a: 1\nb: [x, true]\n").unwrap()[0]).unwrap();
        assert_eq!(v["a"], 1);
        assert_eq!(v["b"][1], true);
        assert!(super::yaml_to_json(&yaml_rust2::Yaml::Alias(0)).is_err());
    }
}
